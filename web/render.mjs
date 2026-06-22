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
import { readFileSync, writeFileSync, mkdirSync, existsSync, rmSync, createWriteStream, createReadStream } from "node:fs"
import { basename } from "node:path"
import { execFileSync } from "node:child_process"
import { once } from "node:events"
import { pipeline } from "node:stream/promises"

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

// --- 3. synth every unit (native ORT), STREAM audio to disk, align via core ---
// A whole book is hours of audio = gigabytes of PCM; we must not hold it in RAM.
// Stream raw s16le to a temp file (constant memory) and let ffmpeg encode it.
const dir = `public/books/${slug}`
mkdirSync(dir, { recursive: true })
const pcmPath = `${dir}/audio.s16`
const pcmStream = createWriteStream(pcmPath)

function f32toS16(f32) {
  const b = Buffer.allocUnsafe(f32.length * 2)
  for (let i = 0; i < f32.length; i++) {
    const s = Math.max(-1, Math.min(1, f32[i]))
    b.writeInt16LE((s < 0 ? s * 0x8000 : s * 0x7fff) | 0, i * 2)
  }
  return b
}

const perChunkTimed = []
const durations = []
let totalSamples = 0
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
  const waveform = out.waveform.data // Float32Array view
  if (!pcmStream.write(f32toS16(waveform))) await once(pcmStream, "drain") // backpressure
  perChunkTimed.push(lector.align_words(words, wordPhonemes, phonemes, duration))
  durations.push(waveform.length / SR)
  totalSamples += waveform.length
  if (i % 10 === 0 || i === units.length - 1) {
    const audioS = totalSamples / SR
    const wallS = (Date.now() - t0) / 1000
    process.stdout.write(`\r  unit ${i + 1}/${units.length}  audio ${audioS.toFixed(0)}s  wall ${wallS.toFixed(0)}s  RTF ${(wallS / audioS).toFixed(2)}x   `)
  }
}
pcmStream.end()
await once(pcmStream, "finish")
console.log()

// --- 4. timeline (.ass) + encode the streamed PCM ---
const merged = lector.merge_timed(perChunkTimed, durations)
writeFileSync(`${dir}/book.ass`, lector.to_ass(merged, 12))

let audioFile = "audio.mp3"
try {
  // ffmpeg reads the raw PCM file directly (no big buffer in our process)
  execFileSync(
    "ffmpeg",
    ["-y", "-f", "s16le", "-ar", String(SR), "-ac", "1", "-i", pcmPath, "-b:a", "64k", `${dir}/audio.mp3`],
    { stdio: "ignore" },
  )
  rmSync(pcmPath)
} catch {
  // no ffmpeg: wrap the PCM into a .wav by streaming (header + copy, low memory)
  const dataLen = totalSamples * 2
  const h = Buffer.alloc(44)
  h.write("RIFF", 0)
  h.writeUInt32LE(36 + dataLen, 4)
  h.write("WAVE", 8)
  h.write("fmt ", 12)
  h.writeUInt32LE(16, 16)
  h.writeUInt16LE(1, 20)
  h.writeUInt16LE(1, 22)
  h.writeUInt32LE(SR, 24)
  h.writeUInt32LE(SR * 2, 28)
  h.writeUInt16LE(2, 32)
  h.writeUInt16LE(16, 34)
  h.write("data", 36)
  h.writeUInt32LE(dataLen, 40)
  writeFileSync(`${dir}/audio.wav`, h)
  await pipeline(createReadStream(pcmPath), createWriteStream(`${dir}/audio.wav`, { flags: "a" }))
  rmSync(pcmPath)
  audioFile = "audio.wav"
  console.warn("(ffmpeg not found; wrote .wav — large for long books)")
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
  seconds: Math.round(totalSamples / SR),
  words: merged.length,
})
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))

const mins = (totalSamples / SR / 60).toFixed(0)
console.log(`done: "${title}" (${mins} min, ${merged.length} words)`)
console.log(`-> open http://localhost:5173 and click "${title}"`)
