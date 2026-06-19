// Synthesis worker: all the heavy lifting (ort init, model inference, phonemize)
// runs here so the main thread / UI never freezes. Posts results back with the
// waveform transferred (zero-copy).

import * as ort from "onnxruntime-web"
import { phonemize } from "phonemizer"

ort.env.wasm.wasmPaths = `https://cdn.jsdelivr.net/npm/onnxruntime-web@${ort.env.versions.web}/dist/`

// Single fp32 on WASM. The full matrix we actually measured on this hardware:
//   q8 / fp16            -> silent (ort-web can't run quantized/half Kokoro)
//   fp32 on webgpu       -> garbage audio (ort-web webgpu numerics, AMD/Linux/Vulkan)
//   our fp32 on wasm     -> correct, ~16s  <-- the only good config
// So: pin wasm. Speed is wasm-bound; the real fix is render-ahead caching (synth
// chunks in the worker, play cached) which lands with ingest.
const MODEL_URL = "/models/kokoro_dur.onnx"
const VOCAB_URL = "/tokenizer/tokenizer.json"
const voiceUrl = (v: string) => `/voices/${v}.bin`

let session: ort.InferenceSession | null = null
let vocab: Record<string, number> | null = null

function progress(id: number, p: Record<string, unknown>) {
  ;(self as any).postMessage({ id, progress: p })
}

async function init(id: number) {
  if (session && vocab) return // already loaded (cached for the session)
  vocab = (await (await fetch(VOCAB_URL)).json()).model.vocab
  // stream the model so we can report real download progress (~311MB first load)
  const resp = await fetch(MODEL_URL)
  const total = Number(resp.headers.get("content-length")) || 0
  const reader = resp.body!.getReader()
  const chunks: Uint8Array[] = []
  let loaded = 0
  let lastPost = 0
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    chunks.push(value)
    loaded += value.length
    if (loaded - lastPost >= 2_000_000 || loaded === total) {
      lastPost = loaded
      progress(id, { phase: "download", loaded, total })
    }
  }
  const bytes = new Uint8Array(loaded)
  let off = 0
  for (const c of chunks) {
    bytes.set(c, off)
    off += c.length
  }
  progress(id, { phase: "compile" })
  session = await ort.InferenceSession.create(bytes, { executionProviders: ["wasm"] })
}

function tokenize(phonemes: string): number[] {
  const v = vocab!
  const ids: number[] = []
  for (const ch of phonemes) {
    const id = v[ch]
    if (id !== undefined) ids.push(id)
  }
  return [0, ...ids, 0]
}

async function synth(id: number, text: string, voice: string, lang: "a" | "b") {
  if (!session || !vocab) throw new Error("not initialized")
  const espeakLang = lang === "b" ? "en-gb" : "en-us"
  progress(id, { phase: "phonemize" })
  // whole-string phonemes drive synthesis (correct prosody / function-word
  // reduction); per-word phonemes are only an alignment guide for the read-along.
  const phonemes = (await phonemize(text, espeakLang)).join(" ")
  const words = text.trim().split(/\s+/)
  const wordPhonemes = await Promise.all(
    words.map((w) => phonemize(w, espeakLang).then((a) => a.join(" "))),
  )
  const inputIds = tokenize(phonemes)
  const len = inputIds.length
  const buf = await (await fetch(voiceUrl(voice))).arrayBuffer()
  const packed = new Float32Array(buf)
  const off = 256 * Math.min(Math.max(len - 2, 0), 509)
  const style = packed.slice(off, off + 256)
  const feeds = {
    input_ids: new ort.Tensor("int64", BigInt64Array.from(inputIds.map((n) => BigInt(n))), [1, len]),
    style: new ort.Tensor("float32", style, [1, 256]),
    speed: new ort.Tensor("int32", Int32Array.from([1]), [1]),
  }
  progress(id, { phase: "infer" }) // wasm compute blocks the worker; no sub-progress
  const out = await session.run(feeds)
  const duration = Array.from(out.duration.data as ArrayLike<bigint | number>).map(Number)
  const waveform = Float32Array.from(out.waveform.data as Float32Array)
  return { phonemes, words, wordPhonemes, inputIds, duration, waveform, sampleRate: 24000 }
}

self.onmessage = async (e: MessageEvent) => {
  const { id, type, text, voice, lang } = e.data
  try {
    if (type === "init") {
      await init(id)
      ;(self as any).postMessage({ id, ok: true })
    } else if (type === "synth") {
      const r = await synth(id, text, voice, lang)
      ;(self as any).postMessage({ id, ok: true, r }, [r.waveform.buffer])
    }
  } catch (err) {
    ;(self as any).postMessage({ id, ok: false, error: String(err) })
  }
}
