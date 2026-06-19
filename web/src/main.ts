// PDF -> cleaned paragraphs (pdf.js + core, all local) -> optional read-along
// of the first chunk (Kokoro synth in the worker + word-synced highlight).
import * as lector from "./core/lector.js"
import { init, synth } from "./bridge.js"
import { encodeWav } from "./wav.js"
import { ingestPdf, type Ingested } from "./ingest.js"

const $ = (id: string) => document.getElementById(id)!
const status = $("status")
const info = $("info")
const paras = $("paras")
const reader = $("reader")
const audio = $("audio") as HTMLAudioElement
const readBtn = $("read") as HTMLButtonElement
const bar = $("bar") as HTMLProgressElement

let current: Ingested | null = null

const mb = (n: number) => (n / 1e6).toFixed(0)
const showBar = () => bar.setAttribute("data-on", "")
const hideBar = () => bar.removeAttribute("data-on")
const determinate = (frac: number) => {
  bar.max = 1
  bar.value = frac
}
const indeterminate = () => bar.removeAttribute("value")

async function loadPdf(src: string | ArrayBuffer) {
  status.textContent = "extracting…"
  reader.replaceChildren()
  try {
    current = await ingestPdf(src)
    info.innerHTML =
      `<b>${current.numPages}</b> pages · <b>${current.paragraphs.length}</b> paragraphs · ` +
      `<b>${current.chunks.length}</b> chunk(s)`
    paras.replaceChildren(
      ...current.paragraphs.map((p) => {
        const el = document.createElement("p")
        el.textContent = p
        return el
      }),
    )
    readBtn.disabled = current.chunks.length === 0
    status.innerHTML = '<span class="ok">extracted ✓</span> — press Read for a read-along'
  } catch (e) {
    status.innerHTML = '<span class="bad">extract failed: ' + (e as Error).message + "</span>"
  }
}

async function readAlong(text: string) {
  readBtn.disabled = true
  showBar()
  indeterminate()
  let timer = 0
  const stopTimer = () => {
    if (timer) {
      clearInterval(timer)
      timer = 0
    }
  }
  try {
    await init((p) => {
      if (p.phase === "download") {
        if (p.total) {
          determinate(p.loaded / p.total)
          status.textContent = `downloading model ${Math.floor((100 * p.loaded) / p.total)}% (${mb(p.loaded)}/${mb(p.total)} MB, one time)`
        } else {
          indeterminate()
          status.textContent = `downloading model ${mb(p.loaded)} MB…`
        }
      } else if (p.phase === "compile") {
        indeterminate()
        status.textContent = "compiling model…"
      }
    })
    const r = await synth(text, "bm_george", "b", (p) => {
      if (p.phase === "phonemize") {
        indeterminate()
        status.textContent = "phonemizing…"
      } else if (p.phase === "infer") {
        indeterminate()
        const t0 = performance.now()
        stopTimer()
        timer = window.setInterval(() => {
          status.textContent = `synthesizing… ${((performance.now() - t0) / 1000).toFixed(1)}s`
        }, 100)
      }
    })
    stopTimer()
    const timed = lector.align_words(r.words, r.wordPhonemes, r.phonemes, r.duration)

    reader.replaceChildren()
    const spans: HTMLSpanElement[] = timed.map((t: any) => {
      const s = document.createElement("span")
      s.className = "w"
      s.textContent = t.text
      reader.append(s, document.createTextNode(" "))
      return s
    })
    audio.src = URL.createObjectURL(encodeWav(r.waveform, r.sampleRate))
    let raf = 0
    const tick = () => {
      const t = audio.currentTime
      for (let i = 0; i < timed.length; i++) {
        const active = t >= timed[i].start_s && t < timed[i].end_s
        spans[i].classList.toggle("active", active)
        spans[i].classList.toggle("spoken", t >= timed[i].end_s && !active)
      }
      raf = requestAnimationFrame(tick)
    }
    audio.onplay = () => (raf = requestAnimationFrame(tick))
    audio.onpause = () => cancelAnimationFrame(raf)
    hideBar()
    status.innerHTML = '<span class="ok">ready — press play ✓</span>'
  } catch (e) {
    hideBar()
    status.innerHTML = '<span class="bad">synth failed: ' + (e as Error).message + "</span>"
  } finally {
    stopTimer()
    readBtn.disabled = false
  }
}

;($("sample") as HTMLButtonElement).onclick = () => loadPdf("/sample.pdf")
;($("file") as HTMLInputElement).onchange = async (e) => {
  const f = (e.target as HTMLInputElement).files?.[0]
  if (f) loadPdf(await f.arrayBuffer())
}
readBtn.onclick = () => {
  if (!current) return
  // skip bare headings; read the first chunk with real prose in it
  const chunk = current.chunks.find((c) => c.length > 40) ?? current.chunks[0]
  readAlong(chunk)
}

loadPdf("/sample.pdf") // auto-load on start
