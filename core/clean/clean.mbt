// PDF span cleanup -> narratable paragraphs. Port of reference/clean.py, with
// the hard-coded 15pt body threshold generalized to an auto-detected body size.
//
// Input is per-page text spans (text + font size + top-left x/y), which a host
// (pdf.js in the browser, PyMuPDF natively) produces. We drop sub-body spans
// (footnotes/headers/refs), group spans into lines, detect paragraphs by
// left-indent, then dehyphenate + collapse whitespace.

///|
pub(all) struct Span {
  text : String
  size : Double
  x : Double
  y : Double
}

///|
let indent_eps : Double = 4.0

///|
let line_tol : Double = 4.0 // spans within this y-distance are one line

///|
fn is_lower(c : Char) -> Bool {
  c >= 'a' && c <= 'z'
}

///|
fn is_ws(c : Char) -> Bool {
  c == ' ' || c == '\n' || c == '\t' || c == '\r'
}

///|
fn is_numeral_char(c : Char) -> Bool {
  (c >= '0' && c <= '9') ||
  c == 'i' ||
  c == 'v' ||
  c == 'x' ||
  c == 'l' ||
  c == 'c' ||
  c == 'I' ||
  c == 'V' ||
  c == 'X' ||
  c == 'L' ||
  c == 'C'
}

///|
fn to_chars(s : String) -> Array[Char] {
  let a : Array[Char] = []
  for c in s {
    a.push(c)
  }
  a
}

///|
/// Collapse whitespace runs to single spaces and trim ends.
pub fn squeeze(s : String) -> String {
  let out = StringBuilder::new()
  let mut started = false
  let mut pending = false
  for c in s {
    if is_ws(c) {
      if started {
        pending = true
      }
    } else {
      if pending {
        out.write_char(' ')
        pending = false
      }
      out.write_char(c)
      started = true
    }
  }
  out.to_string()
}

///|
/// Rejoin words split across a line break: "infor- mation" -> "information".
/// Only lowercase-hyphen-space-lowercase, to spare real em-dashes.
pub fn dehyphenate(s : String) -> String {
  let cs = to_chars(s)
  let n = cs.length()
  let out = StringBuilder::new()
  let mut i = 0
  while i < n {
    let c = cs[i]
    if is_lower(c) && i + 1 < n && cs[i + 1] == '-' {
      let mut j = i + 2
      let mut saw_ws = false
      while j < n && is_ws(cs[j]) {
        saw_ws = true
        j = j + 1
      }
      if saw_ws && j < n && is_lower(cs[j]) {
        out.write_char(c) // drop the hyphen + whitespace; next char attaches
        i = j
        continue
      }
    }
    out.write_char(c)
    i = i + 1
  }
  out.to_string()
}

///|
fn is_standalone_numeral(s : String) -> Bool {
  let cs = to_chars(s)
  let n = cs.length()
  if n < 1 || n > 4 {
    return false
  }
  for c in cs {
    if !is_numeral_char(c) {
      return false
    }
  }
  true
}

///|
/// Dominant font size by character count (the body text), rounded to a point.
pub fn detect_body_size(spans : Array[Span]) -> Double {
  let counts : Array[Int] = Array::make(256, 0)
  for s in spans {
    let k = (s.size + 0.5).to_int()
    if k >= 0 && k < 256 {
      counts[k] = counts[k] + s.text.length()
    }
  }
  let mut best = 16
  let mut best_c = -1
  for k in 0..<256 {
    if counts[k] > best_c {
      best_c = counts[k]
      best = k
    }
  }
  best.to_double()
}

///|
// insertion sort spans by (y, then x) — pages have few spans, simplicity > speed
fn sorted_by_yx(spans : Array[Span]) -> Array[Span] {
  let r : Array[Span] = []
  for s in spans {
    r.push(s)
    let mut i = r.length() - 1
    while i > 0 && (r[i - 1].y > s.y || (r[i - 1].y == s.y && r[i - 1].x > s.x)) {
      r[i] = r[i - 1]
      i = i - 1
    }
    r[i] = s
  }
  r
}

///|
priv struct Line {
  x : Double
  text : String
}

///|
/// One page's spans -> paragraphs (body text only, reflowed).
pub fn page_paragraphs(
  spans : Array[Span],
  body_min_size : Double,
) -> Array[String] {
  // keep body spans with non-empty text
  let kept : Array[Span] = []
  for s in spans {
    if s.size >= body_min_size && squeeze(s.text) != "" {
      kept.push(s)
    }
  }
  if kept.length() == 0 {
    return []
  }
  let ordered = sorted_by_yx(kept)

  // group into lines by y proximity
  let lines : Array[Line] = []
  let mut cur_y = ordered[0].y
  let mut cur_x = ordered[0].x
  let cur_txt = StringBuilder::new()
  cur_txt.write_string(ordered[0].text)
  for idx in 1..<ordered.length() {
    let s = ordered[idx]
    if s.y - cur_y <= line_tol {
      cur_txt.write_char(' ')
      cur_txt.write_string(s.text)
      if s.x < cur_x {
        cur_x = s.x
      }
    } else {
      lines.push({ x: cur_x, text: squeeze(cur_txt.to_string()) })
      cur_txt.reset()
      cur_txt.write_string(s.text)
      cur_y = s.y
      cur_x = s.x
    }
  }
  lines.push({ x: cur_x, text: squeeze(cur_txt.to_string()) })

  // left margin = smallest line x
  let mut margin = lines[0].x
  for ln in lines {
    if ln.x < margin {
      margin = ln.x
    }
  }

  // paragraphs: indent past the margin starts a new one; drop page numbers
  let paras : Array[String] = []
  let cur = StringBuilder::new()
  let mut have = false
  for ln in lines {
    if is_standalone_numeral(ln.text) {
      continue
    }
    let indented = ln.x > margin + indent_eps
    if indented && have {
      paras.push(squeeze(dehyphenate(cur.to_string())))
      cur.reset()
      cur.write_string(ln.text)
    } else {
      if have {
        cur.write_char(' ')
      }
      cur.write_string(ln.text)
      have = true
    }
  }
  if have {
    paras.push(squeeze(dehyphenate(cur.to_string())))
  }
  paras
}
