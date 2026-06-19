// Greedy paragraph packing (build.py:49-60), plus sentence-level packing for
// TTS engines with short input limits: Kokoro caps at ~510 phoneme tokens (its
// position embeddings), so synth chunks must be sentence-sized, not the 8000-char
// blocks a server engine like MiniMax accepts.

///|
/// Split prose into sentences. Breaks after `.`/`!`/`?` (plus any closing quote
/// or bracket) when followed by whitespace or end of string. Leading whitespace
/// of each sentence is dropped. Decimal points like "1970." inside a number are
/// not special-cased; that is acceptable for the chunk-size heuristic.
fn split_sentences(p : String) -> Array[String] {
  let cs : Array[Char] = []
  for c in p {
    cs.push(c)
  }
  let n = cs.length()
  let out : Array[String] = []
  let buf = StringBuilder::new()
  let mut started = false
  let mut i = 0
  while i < n {
    let c = cs[i]
    // collapse runs of whitespace; skip leading whitespace of a sentence
    let is_ws = c == ' ' || c == '\n' || c == '\t' || c == '\r'
    if is_ws {
      if started {
        buf.write_char(' ')
      }
      // skip the rest of the whitespace run
      while i + 1 < n &&
            (cs[i + 1] == ' ' ||
            cs[i + 1] == '\n' ||
            cs[i + 1] == '\t' ||
            cs[i + 1] == '\r') {
        i = i + 1
      }
      i = i + 1
      continue
    }
    buf.write_char(c)
    started = true
    if c == '.' || c == '!' || c == '?' {
      // absorb a closing quote/bracket that belongs to this sentence
      let mut j = i + 1
      if j < n &&
        (cs[j] == '"' || cs[j] == '\'' || cs[j] == ')' || cs[j] == ']') {
        buf.write_char(cs[j])
        j = j + 1
      }
      // boundary only if end-of-string or next char is whitespace
      if j >= n ||
        cs[j] == ' ' ||
        cs[j] == '\n' ||
        cs[j] == '\t' ||
        cs[j] == '\r' {
        out.push(buf.to_string())
        buf.reset()
        started = false
        i = j
        continue
      }
    }
    i = i + 1
  }
  let tail = buf.to_string()
  if tail != "" {
    out.push(tail)
  }
  out
}

///|
/// Split paragraphs into sentences, then greedily pack sentences into chunks of
/// at most `max_chars` (joined by a single space). A sentence longer than
/// `max_chars` becomes its own chunk rather than being dropped or truncated.
/// Use this for Kokoro: ~300 chars stays comfortably under the 510-phoneme cap.
pub fn sentence_pack(paras : Array[String], max_chars : Int) -> Array[String] {
  let chunks : Array[String] = []
  let mut cur = ""
  for p in paras {
    for s in split_sentences(p) {
      if s == "" {
        continue
      }
      if cur == "" {
        cur = s
      } else if cur.length() + s.length() + 1 > max_chars {
        chunks.push(cur)
        cur = s
      } else {
        cur = cur + " " + s
      }
    }
  }
  if cur != "" {
    chunks.push(cur)
  }
  chunks
}

// Greedy paragraph packing. Port of build.py:49-60.

///|
/// Greedily pack whole paragraphs into chunks of at most `max_chars`, joining
/// paragraphs within a chunk by a blank line ("\n\n"). Never splits a paragraph.
/// Character counts use MoonBit String length (UTF-16 code units) — fine as a
/// size heuristic for English prose.
pub fn pack(paras : Array[String], max_chars : Int) -> Array[String] {
  let chunks : Array[String] = []
  let mut cur = ""
  for p in paras {
    if cur != "" && cur.length() + p.length() + 1 > max_chars {
      chunks.push(cur)
      cur = p
    } else if cur == "" {
      cur = p
    } else {
      cur = cur + "\n\n" + p
    }
  }
  if cur != "" {
    chunks.push(cur)
  }
  chunks
}
