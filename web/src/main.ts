// Full read-along slice: text -> Kokoro (browser) -> core timings -> audio +
// word-synced DOM highlighting. All client-side, no Python.
import * as lector from "./core/lector.js"
import { init, synth } from "./bridge.js"
import { buildTokens } from "./align.js"
import { encodeWav } from "./wav.js"

const status = document.getElementById("status")!
const audio = document.getElementById("audio") as HTMLAudioElement
const reader = document.getElementById("reader")!

const TEXT =
  "The basic concept of exit, voice, and loyalty was developed by Professor Hirschman in 1970."

async function run() {
  try {
    await init((s) => (status.textContent = s))
    status.textContent = "synthesizing…"
    const r = await synth(TEXT, "bm_george", "b")

    const tokens = buildTokens(TEXT, r.phonemes)
    const timed = lector.join_timestamps(tokens, r.duration)

    // lay out words as spans
    reader.replaceChildren()
    const spans: HTMLSpanElement[] = timed.map((t: any) => {
      const s = document.createElement("span")
      s.className = "w"
      s.textContent = t.text
      reader.append(s, document.createTextNode(" "))
      return s
    })

    // --- diagnostics: is the in-browser waveform actually audio? ---
    let maxAmp = 0
    for (let i = 0; i < r.waveform.length; i++) {
      const a = Math.abs(r.waveform[i])
      if (a > maxAmp) maxAmp = a
    }
    const diag = document.createElement("div")
    diag.style.cssText = "font-size:.8rem;color:#888;margin:.5rem 0"
    diag.textContent =
      `waveform: ${r.waveform.length} samples, max|amp|=${maxAmp.toFixed(3)} ` +
      `(ctor=${(r.waveform as any).constructor?.name})`
    reader.before(diag)

    // audio + a direct download to bypass the <audio> element entirely
    const wavBlob = encodeWav(r.waveform, r.sampleRate)
    const url = URL.createObjectURL(wavBlob)
    audio.src = url
    audio.addEventListener("error", () =>
      (diag.textContent += `  | audio error: ${audio.error?.code}`))
    audio.addEventListener("loadedmetadata", () =>
      (diag.textContent += `  | audio.duration=${audio.duration.toFixed(2)}`))
    const dl = document.createElement("a")
    dl.href = url
    dl.download = "lector_sample.wav"
    dl.textContent = "⬇ download the WAV (play it directly)"
    dl.style.cssText = "display:block;font-size:.85rem;margin:.5rem 0"
    diag.after(dl)

    // word-synced highlight, driven by audio.currentTime
    let raf = 0
    const tick = () => {
      const t = audio.currentTime
      for (let i = 0; i < timed.length; i++) {
        const active = t >= timed[i].start_s && t < timed[i].end_s
        const spoken = t >= timed[i].end_s
        spans[i].classList.toggle("active", active)
        spans[i].classList.toggle("spoken", spoken && !active)
      }
      raf = requestAnimationFrame(tick)
    }
    audio.onplay = () => (raf = requestAnimationFrame(tick))
    audio.onpause = () => cancelAnimationFrame(raf)

    status.innerHTML =
      '<span class="ok">ready — press play to read along ✓</span> (' +
      timed.length + " words, " + (r.waveform.length / r.sampleRate).toFixed(1) + "s)"
  } catch (e) {
    status.innerHTML = '<span class="bad">failed: ' + (e as Error).message + "</span>"
    console.error(e)
  }
}

run()
