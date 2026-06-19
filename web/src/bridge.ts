// Thin client over the synth worker. Keeps inference off the main thread.

export interface SynthResult {
  phonemes: string
  words: string[]
  wordPhonemes: string[]
  inputIds: number[]
  duration: number[]
  waveform: Float32Array
  sampleRate: number
}

export type Progress =
  | { phase: "download"; loaded: number; total: number }
  | { phase: "compile" }
  | { phase: "phonemize" }
  | { phase: "infer" }

type OnProgress = (p: Progress) => void

const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" })

let seq = 0
const pending = new Map<
  number,
  { resolve: (v: any) => void; reject: (e: Error) => void; onProgress?: OnProgress }
>()

worker.onmessage = (e: MessageEvent) => {
  const { id, ok, r, error, progress } = e.data
  const p = pending.get(id)
  if (!p) return
  if (progress) {
    p.onProgress?.(progress)
    return
  }
  pending.delete(id)
  ok ? p.resolve(r) : p.reject(new Error(error))
}

function call<T>(
  type: string,
  args: Record<string, unknown> = {},
  onProgress?: OnProgress,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const id = ++seq
    pending.set(id, { resolve, reject, onProgress })
    worker.postMessage({ id, type, ...args })
  })
}

export const init = (onProgress?: OnProgress) => call<void>("init", {}, onProgress)

export const synth = (
  text: string,
  voice = "bm_george",
  lang: "a" | "b" = "b",
  onProgress?: OnProgress,
) => call<SynthResult>("synth", { text, voice, lang }, onProgress)

export function gpuAvailable(): boolean {
  return typeof (navigator as any).gpu !== "undefined"
}
