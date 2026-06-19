// PDF -> cleaned paragraphs (pdf.js + core, all local) -> gapless continuous
// read-along of the whole document (Web Audio, render-ahead, word highlight).
import { ingestPdf, type Ingested } from "./ingest.js"
import { Reader, type Rendered } from "./player.js"
import type { Progress } from "./bridge.js"

const $ = (id: string) => document.getElementById(id)!
const status = $("status")
const info = $("info")
const paras = $("paras")
const readerEl = $("reader")
const bar = $("bar") as HTMLProgressElement
const playBtn = $("play") as HTMLButtonElement
const prevBtn = $("prev") as HTMLButtonElement
const nextBtn = $("next") as HTMLButtonElement
const pos = $("pos")

let current: Ingested | null = null
let player: Reader | null = null
let playing = false
let everStarted = false

const mb = (n: number) => (n / 1e6).toFixed(0)
const showBar = () => bar.setAttribute("data-on", "")
const hideBar = () => bar.removeAttribute("data-on")
const determinate = (frac: number) => {
  bar.max = 1
  bar.value = frac
}
const indeterminate = () => bar.removeAttribute("value")

// --- read-along highlight (player drives the active word index) ---
let hlSpans: HTMLSpanElement[] = []
function showChunk(idx: number, total: number, r: Rendered) {
  stopTimer()
  hideBar()
  hlSpans = r.timed.map((t) => {
    const s = document.createElement("span")
    s.className = "w"
    s.textContent = t.text
    return s
  })
  readerEl.replaceChildren()
  hlSpans.forEach((s) => readerEl.append(s, document.createTextNode(" ")))
  pos.textContent = `chunk ${idx + 1} / ${total}`
  prevBtn.disabled = idx === 0
  nextBtn.disabled = idx >= total - 1
}
function highlightWord(active: number) {
  for (let i = 0; i < hlSpans.length; i++) {
    hlSpans[i].classList.toggle("active", i === active)
    hlSpans[i].classList.toggle("spoken", i < active)
  }
}

// --- progress / elapsed timer for the chunk the user is waiting on ---
let timer = 0
function stopTimer() {
  if (timer) {
    clearInterval(timer)
    timer = 0
  }
}
function onProgress(p: Progress) {
  showBar()
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
  } else if (p.phase === "phonemize") {
    indeterminate()
    status.textContent = "phonemizing…"
  } else if (p.phase === "infer") {
    indeterminate()
    const t0 = performance.now()
    stopTimer()
    timer = window.setInterval(() => {
      status.textContent = `synthesizing first chunk… ${((performance.now() - t0) / 1000).toFixed(1)}s`
    }, 100)
  }
}

function setPlaying(on: boolean) {
  playing = on
  playBtn.textContent = on ? "⏸ Pause" : "▶ Play"
}

async function loadPdf(src: string | ArrayBuffer) {
  status.textContent = "extracting…"
  readerEl.replaceChildren()
  pos.textContent = ""
  player?.dispose()
  player = null
  setPlaying(false)
  everStarted = false
  playBtn.disabled = prevBtn.disabled = nextBtn.disabled = true
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
    if (current.chunks.length === 0) {
      status.innerHTML = '<span class="bad">no readable text found</span>'
      return
    }
    player = new Reader(current.chunks, "bm_george", "b", {
      onChunk: showChunk,
      onWord: highlightWord,
      onProgress,
      onState: setPlaying,
      onStatus: (m) => (status.textContent = m),
      onError: (e) => {
        stopTimer()
        hideBar()
        setPlaying(false)
        status.innerHTML = '<span class="bad">synth failed: ' + e.message + "</span>"
      },
      onEnd: () => {
        everStarted = false
        status.innerHTML = '<span class="ok">✓ finished</span>'
      },
    })
    playBtn.disabled = false
    status.innerHTML = '<span class="ok">extracted ✓</span> — press Play to read aloud'
  } catch (e) {
    status.innerHTML = '<span class="bad">extract failed: ' + (e as Error).message + "</span>"
  }
}

// start at the first chunk with real prose (skip a bare heading chunk)
const startIdx = () => {
  const i = current?.chunks.findIndex((c) => c.length > 40) ?? -1
  return i >= 0 ? i : 0
}

playBtn.onclick = () => {
  if (!player) return
  if (playing) player.pause()
  else if (everStarted) player.resume()
  else {
    everStarted = true
    player.playFrom(startIdx())
  }
}
prevBtn.onclick = () => {
  everStarted = true
  player?.prev()
}
nextBtn.onclick = () => {
  everStarted = true
  player?.next()
}
;($("sample") as HTMLButtonElement).onclick = () => loadPdf("/sample.pdf")
;($("file") as HTMLInputElement).onchange = async (e) => {
  const f = (e.target as HTMLInputElement).files?.[0]
  if (f) loadPdf(await f.arrayBuffer())
}

loadPdf("/sample.pdf") // auto-load on start
