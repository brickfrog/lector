// Kokoro bridge (Step A: inference plumbing).
// phonemize -> tokenize -> voice style -> run our duration-exposing ONNX in
// onnxruntime-web -> { waveform, duration }. Word-alignment + render come next.

import * as ort from "onnxruntime-web"
import { AutoTokenizer } from "@huggingface/transformers"
import { phonemize } from "phonemizer"

// dev: pull ort's wasm from CDN pinned to the installed version
ort.env.wasm.wasmPaths = `https://cdn.jsdelivr.net/npm/onnxruntime-web@${ort.env.versions.web}/dist/`

const MODEL_URL = "/models/kokoro_dur_q8.onnx"
const TOK_ID = "onnx-community/Kokoro-82M-v1.0-ONNX"
const voiceUrl = (v: string) =>
  `https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main/voices/${v}.bin`

let session: ort.InferenceSession | null = null
let tokenizer: any = null

export async function init(onProgress?: (s: string) => void) {
  onProgress?.("loading tokenizer…")
  tokenizer = await AutoTokenizer.from_pretrained(TOK_ID)
  onProgress?.("loading model (110 MB)…")
  session = await ort.InferenceSession.create(MODEL_URL)
  onProgress?.("model ready")
}

export interface SynthResult {
  phonemes: string
  inputIds: number[]
  duration: number[] // per-token frames
  waveform: Float32Array
  sampleRate: number
}

export async function synth(
  text: string,
  voice = "bm_george",
  lang: "a" | "b" = "b",
): Promise<SynthResult> {
  if (!session || !tokenizer) throw new Error("call init() first")

  const phonemes = (await phonemize(text, lang === "b" ? "en-gb" : "en-us")).join(" ")
  const enc = tokenizer(phonemes, { truncation: true })
  const inputIds = Array.from(enc.input_ids.data as ArrayLike<bigint | number>).map(Number)
  const len = inputIds.length

  // voice pack: [N,256] float32, sliced by (len-2)
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
  const waveform = out.waveform.data as Float32Array
  return { phonemes, inputIds, duration, waveform, sampleRate: 24000 }
}
