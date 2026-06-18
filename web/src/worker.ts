// Synthesis worker: all the heavy lifting (ort init, model inference, phonemize)
// runs here so the main thread / UI never freezes. Posts results back with the
// waveform transferred (zero-copy).

import * as ort from "onnxruntime-web"
import { phonemize } from "phonemizer"

ort.env.wasm.wasmPaths = `https://cdn.jsdelivr.net/npm/onnxruntime-web@${ort.env.versions.web}/dist/`

// Dual model: onnx-community's stock fp16 for AUDIO (webgpu-fast, no iSTFT CPU
// fallback), our tiny duration-only model for TIMING. Same weights -> the
// waveform length and our durations agree to the frame.
const AUDIO_URL = "/models/kokoro_stock_fp16.onnx"
const DUR_URL = "/models/kokoro_duronly.onnx"
const VOCAB_URL = "/tokenizer/tokenizer.json"
const voiceUrl = (v: string) => `/voices/${v}.bin`

let audioSession: ort.InferenceSession | null = null
let durSession: ort.InferenceSession | null = null
let vocab: Record<string, number> | null = null

async function init() {
  vocab = (await (await fetch(VOCAB_URL)).json()).model.vocab
  const opts = { executionProviders: ["webgpu", "wasm"] }
  ;[audioSession, durSession] = await Promise.all([
    ort.InferenceSession.create(AUDIO_URL, opts),
    ort.InferenceSession.create(DUR_URL, opts),
  ])
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

async function synth(text: string, voice: string, lang: "a" | "b") {
  if (!audioSession || !durSession || !vocab) throw new Error("not initialized")
  const phonemes = (await phonemize(text, lang === "b" ? "en-gb" : "en-us")).join(" ")
  const inputIds = tokenize(phonemes)
  const len = inputIds.length
  const buf = await (await fetch(voiceUrl(voice))).arrayBuffer()
  const packed = new Float32Array(buf)
  const off = 256 * Math.min(Math.max(len - 2, 0), 509)
  const style = packed.slice(off, off + 256)

  const ids = new ort.Tensor("int64", BigInt64Array.from(inputIds.map((n) => BigInt(n))), [1, len])
  const styleT = new ort.Tensor("float32", style, [1, 256])

  // audio (stock fp16, speed=float32) and duration (duronly, speed=int32)
  const [aout, dout] = await Promise.all([
    audioSession.run({ input_ids: ids, style: styleT, speed: new ort.Tensor("float32", Float32Array.from([1]), [1]) }),
    durSession.run({ input_ids: ids, style: styleT, speed: new ort.Tensor("int32", Int32Array.from([1]), [1]) }),
  ])
  const waveform = Float32Array.from(aout.waveform.data as Float32Array)
  const duration = Array.from(dout.duration.data as ArrayLike<bigint | number>).map(Number)
  return { phonemes, inputIds, duration, waveform, sampleRate: 24000 }
}

self.onmessage = async (e: MessageEvent) => {
  const { id, type, text, voice, lang } = e.data
  try {
    if (type === "init") {
      await init()
      ;(self as any).postMessage({ id, ok: true })
    } else if (type === "synth") {
      const r = await synth(text, voice, lang)
      ;(self as any).postMessage({ id, ok: true, r }, [r.waveform.buffer])
    }
  } catch (err) {
    ;(self as any).postMessage({ id, ok: false, error: String(err) })
  }
}
