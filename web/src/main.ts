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

    // audio
    audio.src = URL.createObjectURL(encodeWav(r.waveform, r.sampleRate))

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
