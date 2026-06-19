// PDF -> cleaned paragraphs -> chunks, using pdf.js (text extraction) + the
// MoonBit core's clean/pack. No model/ort needed: this whole path is local.
import * as pdfjs from "pdfjs-dist"
import workerUrl from "pdfjs-dist/build/pdf.worker.mjs?url"
import * as lector from "./core/lector.js"
import type { Span } from "./core/lector.js"

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl

export interface Ingested {
  numPages: number
  paragraphs: string[]
  chunks: string[]
}

export async function ingestPdf(src: string | ArrayBuffer): Promise<Ingested> {
  const doc = await pdfjs.getDocument(
    typeof src === "string" ? { url: src } : { data: src },
  ).promise

  const paragraphs: string[] = []
  for (let p = 1; p <= doc.numPages; p++) {
    const page = await doc.getPage(p)
    const tc = await page.getTextContent()
    const spans: Span[] = []
    for (const it of tc.items as any[]) {
      if (!it.str) continue
      const t = it.transform as number[] // [a,b,c,d,e,f]; e=x, f=y (bottom-up)
      const size = Math.hypot(t[2], t[3])
      spans.push({ text: it.str, size, x: t[4], y: -t[5] }) // -y => top-down order
    }
    if (spans.length === 0) continue
    const body = lector.detect_body_size(spans)
    // keep spans at >=85% of the body size (drops footnotes/refs/page numbers)
    for (const para of lector.page_paragraphs(spans, body * 0.85)) {
      paragraphs.push(para)
    }
  }

  // Kokoro caps at ~510 phoneme tokens; sentence-pack to ~300 chars stays well
  // under that (English runs ~1.3 phonemes/char). pack(8000) was for the server
  // engine and overflows the model's bert position embeddings.
  const chunks = lector.sentence_pack(paragraphs, 300)
  return { numPages: doc.numPages, paragraphs, chunks }
}
