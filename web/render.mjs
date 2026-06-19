// Native render: PDF -> audio.wav + karaoke .ass, for the in-browser audiobook
// player to consume. Reuses the SAME core + synth logic as the browser (so the
// .ass matches exactly) but runs onnxruntime *natively* (~5x faster than realtime
// vs the browser's ~0.25x), which is the whole point of rendering off-browser.
//
//   bun render.mjs <input.pdf> [outBase=book] [voice=bm_george] [lang=b]
//
// Needs the vendored assets (bash vendor.sh): public/models, public/voices,
// public/tokenizer.
import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs"
import * as lector from "./src/core/lector.js"
import { phonemize } from "phonemizer"
import * as ort from "onnxruntime-node"
import { readFileSync, writeFileSync } from "node:fs"

const [pdfPath, outBase = "book", voiceName = "bm_george", lang = "b"] = process.argv.slice(2)
if (!pdfPath) {
  console.error("usage: bun render.mjs <input.pdf> [outBase] [voice] [lang]")
  process.exit(1)
}
const espeak = lang === "b" ? "en-gb" : "en-us"
const SR = 24000

// --- 1. extract + clean + chunk (identical to ingest.ts) ---
const data = new Uint8Array(readFileSync(pdfPath))
const doc = await pdfjs.getDocument({ data, useWorkerFetch: false, isEvalSupported: false }).promise
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
console.log(`${doc.numPages} pages -> ${paragraphs.length} paragraphs -> ${chunks.length} chunks`)

// --- 2. model + vocab + voice ---
const vocab = JSON.parse(readFileSync("public/tokenizer/tokenizer.json", "utf8")).model.vocab
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
for (let i = 0; i < chunks.length; i++) {
  const text = chunks[i]
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
  if (i % 10 === 0 || i === chunks.length - 1) {
    const audioS = durations.reduce((a, b) => a + b, 0)
    const wallS = (Date.now() - t0) / 1000
    process.stdout.write(`\r  chunk ${i + 1}/${chunks.length}  audio ${audioS.toFixed(0)}s  wall ${wallS.toFixed(0)}s  RTF ${(wallS / audioS).toFixed(2)}x   `)
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
writeFileSync(`${outBase}.wav`, wav(full, SR))
writeFileSync(`${outBase}.ass`, lector.to_ass(merged, 12))
console.log(`wrote ${outBase}.wav (${(full.length / SR).toFixed(0)}s) + ${outBase}.ass (${merged.length} words)`)
