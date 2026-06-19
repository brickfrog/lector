// Native render: PDF -> audio.wav + karaoke .ass, for the in-browser audiobook
// player to consume. Reuses the SAME core + synth logic as the browser (so the
// .ass matches exactly) but runs onnxruntime *natively* (~5x faster than realtime
// vs the browser's ~0.25x), which is the whole point of rendering off-browser.
//
//   bun render.mjs <input.pdf> [voice=bm_george] [lang=b]
//
// Writes public/books/<slug>/audio.{mp3,wav} + book.ass and updates
// public/books/index.json, so the web app lists it and you just click to play.
// Needs the vendored assets (bash vendor.sh): public/models, public/voices,
// public/tokenizer.
import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs"
import * as lector from "./src/core/lector.js"
import { phonemize } from "phonemizer"
import * as ort from "onnxruntime-node"
import { readFileSync, writeFileSync, mkdirSync, existsSync, rmSync } from "node:fs"
import { basename } from "node:path"
import { execFileSync } from "node:child_process"

const [pdfPath, voiceName = "bm_george", lang = "b"] = process.argv.slice(2)
if (!pdfPath) {
  console.error("usage: bun render.mjs <input.pdf> [voice=bm_george] [lang=b]")
  process.exit(1)
}
const title = basename(pdfPath).replace(/\.[^.]+$/, "")
const slug = title.replace(/[^a-z0-9]+/gi, "-").toLowerCase().replace(/(^-|-$)/g, "") || "book"
const espeak = lang === "b" ? "en-gb" : "en-us"
const SR = 24000

// --- 1. extract + clean + chunk (identical to ingest.ts) ---
const data = new Uint8Array(readFileSync(pdfPath))
// verbosity:0 = errors only (silences the harmless per-page standardFontDataUrl
// warning; we only extract text, never render fonts).
const doc = await pdfjs.getDocument({
  data,
  useWorkerFetch: false,
  isEvalSupported: false,
  verbosity: 0,
}).promise
const paragraphs = []
for (let p = 1; p <= doc.numPages; p++) {
  const page = await doc.getPage(p)
  const tc = await page.getTextContent()
  const spans = []
  for (const it of tc.items) {
    if (!it.str) continue
    const t = it.transform
    spans.push({ text: it.str, size: Math.hypot(t[2], t[3]), x: t[4], y: -t[5] })
  }
  if (!spans.length) continue
  const body = lector.detect_body_size(spans)
  for (const para of lector.page_paragraphs(spans, body * 0.85)) paragraphs.push(para)
}
const chunks = lector.sentence_pack(paragraphs, 300)

// --- 2. vocab (needed to count tokens), then enforce Kokoro's ~510-token cap ---
const vocab = JSON.parse(readFileSync("public/tokenizer/tokenizer.json", "utf8")).model.vocab
const tokenLen = (phon) => [...phon].reduce((n, c) => n + (vocab[c] !== undefined ? 1 : 0), 0)

// A char budget is only a proxy; the model hard-caps at ~510 phoneme tokens
// (its bert position embeddings). Split any unit that actually exceeds it,
// halving on word boundaries until every piece fits. Bulletproof: uses the real
// phonemizer, not a heuristic.
const MAX_TOKENS = 500
async function splitToLimit(text) {
  const phon = (await phonemize(text, espeak)).join(" ")
  if (tokenLen(phon) <= MAX_TOKENS) return [text]
  const words = text.trim().split(/\s+/)
  if (words.length <= 1) return [text] // unsplittable single token; let it ride
  const mid = Math.ceil(words.length / 2)
  const [a, b] = await Promise.all([
    splitToLimit(words.slice(0, mid).join(" ")),
    splitToLimit(words.slice(mid).join(" ")),
  ])
  return [...a, ...b]
}
const units = []
for (const c of chunks) units.push(...(await splitToLimit(c)))
console.log(
  `${doc.numPages} pages -> ${paragraphs.length} paragraphs -> ${chunks.length} chunks -> ${units.length} units`,
)

// --- model + voice ---
const vbuf = readFileSync(`public/voices/${voiceName}.bin`)
const voice = new Float32Array(vbuf.buffer, vbuf.byteOffset, Math.floor(vbuf.byteLength / 4))
const session = await ort.InferenceSession.create("public/models/kokoro_dur.onnx", {
  executionProviders: ["cpu"],
  intraOpNumThreads: 8,
})

// --- 3. synth every chunk (native ORT), align words via the core ---
const pcms = []
const perChunkTimed = []
const durations = []
const t0 = Date.now()
for (let i = 0; i < units.length; i++) {
  const text = units[i]
  const phonemes = (await phonemize(text, espeak)).join(" ")
  const words = text.trim().split(/\s+/)
  const wordPhonemes = await Promise.all(words.map((w) => phonemize(w, espeak).then((a) => a.join(" "))))
  const ids = [0, ...[...phonemes].map((c) => vocab[c]).filter((x) => x !== undefined), 0]
  const len = ids.length
  const off = 256 * Math.min(Math.max(len - 2, 0), 509)
  const style = voice.slice(off, off + 256)
  const out = await session.run({
    input_ids: new ort.Tensor("int64", BigInt64Array.from(ids.map((n) => BigInt(n))), [1, len]),
    style: new ort.Tensor("float32", Float32Array.from(style), [1, 256]),
    speed: new ort.Tensor("int32", Int32Array.from([1]), [1]),
  })
  const duration = Array.from(out.duration.data).map(Number)
  const waveform = Float32Array.from(out.waveform.data)
  pcms.push(waveform)
  perChunkTimed.push(lector.align_words(words, wordPhonemes, phonemes, duration))
  durations.push(waveform.length / SR)
  if (i % 10 === 0 || i === units.length - 1) {
    const audioS = durations.reduce((a, b) => a + b, 0)
    const wallS = (Date.now() - t0) / 1000
    process.stdout.write(`\r  unit ${i + 1}/${units.length}  audio ${audioS.toFixed(0)}s  wall ${wallS.toFixed(0)}s  RTF ${(wallS / audioS).toFixed(2)}x   `)
  }
}
console.log()

// --- 4. merge timeline + concat audio (core does the offset math) ---
const merged = lector.merge_timed(perChunkTimed, durations)
const total = pcms.reduce((a, p) => a + p.length, 0)
const full = new Float32Array(total)
let o = 0
for (const p of pcms) {
  full.set(p, o)
  o += p.length
}

// --- 5. write WAV (16-bit PCM) + .ass ---
function wav(samples, sr) {
  const buf = Buffer.alloc(44 + samples.length * 2)
  buf.write("RIFF", 0)
  buf.writeUInt32LE(36 + samples.length * 2, 4)
  buf.write("WAVE", 8)
  buf.write("fmt ", 12)
  buf.writeUInt32LE(16, 16)
  buf.writeUInt16LE(1, 20)
  buf.writeUInt16LE(1, 22)
  buf.writeUInt32LE(sr, 24)
  buf.writeUInt32LE(sr * 2, 28)
  buf.writeUInt16LE(2, 32)
  buf.writeUInt16LE(16, 34)
  buf.write("data", 36)
  buf.writeUInt32LE(samples.length * 2, 40)
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]))
    buf.writeInt16LE((s < 0 ? s * 0x8000 : s * 0x7fff) | 0, 44 + i * 2)
  }
  return buf
}
const dir = `public/books/${slug}`
mkdirSync(dir, { recursive: true })
writeFileSync(`${dir}/book.ass`, lector.to_ass(merged, 12))
writeFileSync(`${dir}/audio.wav`, wav(full, SR))

// compress to mp3 if ffmpeg is around (a whole-book WAV is gigabytes); else keep wav
let audioFile = "audio.wav"
try {
  execFileSync("ffmpeg", ["-y", "-i", `${dir}/audio.wav`, "-b:a", "64k", `${dir}/audio.mp3`], {
    stdio: "ignore",
  })
  rmSync(`${dir}/audio.wav`)
  audioFile = "audio.mp3"
} catch {
  console.warn("(ffmpeg not found; keeping .wav — large for long books)")
}

// update the manifest the web app lists from
const manifestPath = "public/books/index.json"
let manifest = []
if (existsSync(manifestPath)) {
  try {
    manifest = JSON.parse(readFileSync(manifestPath, "utf8"))
  } catch {
    /* ignore */
  }
}
manifest = manifest.filter((b) => b.slug !== slug)
manifest.unshift({
  slug,
  title,
  audio: audioFile,
  seconds: Math.round(full.length / SR),
  words: merged.length,
})
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))

const mins = (full.length / SR / 60).toFixed(0)
console.log(`done: "${title}" (${mins} min, ${merged.length} words)`)
console.log(`-> open http://localhost:5173 and click "${title}"`)
