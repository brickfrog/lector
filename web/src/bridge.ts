// Kokoro bridge, fully local: no runtime fetches to HF or a CDN.
// phonemize (espeak-wasm, bundled) -> char-level tokenize (vendored vocab) ->
// voice style (vendored .bin) -> our duration-exposing ONNX in onnxruntime-web
// -> { waveform, duration }. Everything served from /public.

import * as ort from "onnxruntime-web"
import { phonemize } from "phonemizer"

// ort's wasm from the CDN (pinned to the installed version). This is the one
// remaining runtime external dep; voice/tokenizer/model are vendored locally.
// TODO(offline): vendoring ort's wasm is blocked by its package `exports` map +
// Vite's dynamic-import handling of the glue .mjs — revisit with a static-copy
// plugin or a prod build step.
ort.env.wasm.wasmPaths = `https://cdn.jsdelivr.net/npm/onnxruntime-web@${ort.env.versions.web}/dist/`

// fp32: q8's int8 vocoder ops compute as silence under ort-web's wasm backend
// (the duration head survives, the waveform doesn't). fp32 is the format kokoro-js
// uses in-browser, so it's ort-web-safe. TODO: a correct fp16 to halve the size.
const MODEL_URL = "/models/kokoro_dur.onnx"
const VOCAB_URL = "/tokenizer/tokenizer.json"
const voiceUrl = (v: string) => `/voices/${v}.bin`

let session: ort.InferenceSession | null = null
let vocab: Record<string, number> | null = null

export async function init(onProgress?: (s: string) => void) {
  onProgress?.("loading vocab…")
  vocab = (await (await fetch(VOCAB_URL)).json()).model.vocab
  onProgress?.("loading model…")
  session = await ort.InferenceSession.create(MODEL_URL)
  onProgress?.("model ready")
}

// Char-level tokenization matching the kokoro export: map each phoneme char to a
// vocab id (dropping unknowns), wrap with 0 (bos/eos).
function tokenize(phonemes: string): number[] {
  const v = vocab!
  const ids: number[] = []
  for (const ch of phonemes) {
    const id = v[ch]
    if (id !== undefined) ids.push(id)
  }
  return [0, ...ids, 0]
}

export interface SynthResult {
  phonemes: string
  inputIds: number[]
  duration: number[]
  waveform: Float32Array
  sampleRate: number
}

export async function synth(
  text: string,
  voice = "bm_george",
  lang: "a" | "b" = "b",
): Promise<SynthResult> {
  if (!session || !vocab) throw new Error("call init() first")

  const phonemes = (await phonemize(text, lang === "b" ? "en-gb" : "en-us")).join(" ")
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
  const out = await session.run(feeds)
  const duration = Array.from(out.duration.data as ArrayLike<bigint | number>).map(Number)
  return { phonemes, inputIds, duration, waveform: out.waveform.data as Float32Array, sampleRate: 24000 }
}
