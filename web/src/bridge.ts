// Thin client over the synth worker. Keeps inference off the main thread.

export interface SynthResult {
  phonemes: string
  inputIds: number[]
  duration: number[]
  waveform: Float32Array
  sampleRate: number
}

const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" })

let seq = 0
const pending = new Map<number, { resolve: (v: any) => void; reject: (e: Error) => void }>()

worker.onmessage = (e: MessageEvent) => {
  const { id, ok, r, error } = e.data
  const p = pending.get(id)
  if (!p) return
  pending.delete(id)
  ok ? p.resolve(r) : p.reject(new Error(error))
}

function call<T>(type: string, args: Record<string, unknown> = {}): Promise<T> {
  return new Promise((resolve, reject) => {
    const id = ++seq
    pending.set(id, { resolve, reject })
    worker.postMessage({ id, type, ...args })
  })
}

export const init = () => call<void>("init")

export const synth = (text: string, voice = "bm_george", lang: "a" | "b" = "b") =>
  call<SynthResult>("synth", { text, voice, lang })

export function gpuAvailable(): boolean {
  return typeof (navigator as any).gpu !== "undefined"
}
