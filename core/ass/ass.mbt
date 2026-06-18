// ASS (Advanced SubStation Alpha) karaoke generation. Port of align.py:38-153.

///|
let header : String = "[Script Info]\n" +
  "ScriptType: v4.00+\n" +
  "PlayResX: 1280\n" +
  "PlayResY: 720\n" +
  "WrapStyle: 0\n\n" +
  "[V4+ Styles]\n" +
  "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n" +
  "Style: Read,Georgia,40,&H0000FFFF,&H80AAAAAA,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,1,2,80,80,60,1\n\n" +
  "[Events]\n" +
  "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"

///|
/// Does this word end a sentence/clause? `[.!?:;]` optionally followed by a
/// closing quote/bracket. Port of the `align.py` regex.
fn ends_sentence(w : String) -> Bool {
  let enders = [".", "!", "?", ":", ";"]
  let quotes = ["", "\"", "'", ")", "]"]
  for q in quotes {
    for e in enders {
      if w.has_suffix(e + q) {
        return true
      }
    }
  }
  false
}

///|
/// Group display words into screen lines: break on a sentence-ender once the
/// line has >=4 words, or at `max_words`. Port of align.py:38-48.
pub fn split_lines(
  words : Array[String],
  max_words : Int,
) -> Array[Array[String]] {
  let lines : Array[Array[String]] = []
  let mut cur : Array[String] = []
  for w in words {
    cur.push(w)
    if (ends_sentence(w) && cur.length() >= 4) || cur.length() >= max_words {
      lines.push(cur)
      cur = []
    }
  }
  if cur.length() > 0 {
    lines.push(cur)
  }
  lines
}

///|
/// Keep ASS override syntax out of word text: braces -> parens, newline -> space.
fn escape(w : String) -> String {
  let sb = StringBuilder::new()
  for c in w {
    if c == '{' {
      sb.write_char('(')
    } else if c == '}' {
      sb.write_char(')')
    } else if c == '\n' {
      sb.write_char(' ')
    } else {
      sb.write_char(c)
    }
  }
  sb.to_string()
}

///|
fn round_cs(sec : Double) -> Int {
  let v = (sec * 100.0 + 0.5).to_int()
  if v < 1 {
    1
  } else {
    v
  }
}

///|
/// Render timed tokens to a full ASS karaoke document. Per-word `\kf` duration
/// folds the inter-word gap into the preceding word (last word uses end-start).
/// Port of align.py:118-153.
pub fn to_ass(timed : Array[@ir.TimedToken], max_words : Int) -> String {
  let sb = StringBuilder::new()
  sb.write_string(header)
  let words : Array[String] = []
  for t in timed {
    words.push(t.text)
  }
  let lines = split_lines(words, max_words)
  let mut wi = 0
  for line in lines {
    let n = line.length()
    let start = timed[wi].start_s
    let durs : Array[Int] = []
    for k in 0..<n {
      let gi = wi + k
      let this_start = timed[gi].start_s
      let nxt = if k + 1 < n {
        timed[wi + k + 1].start_s
      } else {
        timed[gi].end_s
      }
      durs.push(round_cs(nxt - this_start))
    }
    let mut sumcs = 0
    for d in durs {
      sumcs = sumcs + d
    }
    let end = start + sumcs.to_double() / 100.0
    let body = StringBuilder::new()
    for k in 0..<n {
      body.write_string("{\\kf")
      body.write_string(durs[k].to_string())
      body.write_string("}")
      body.write_string(escape(line[k]))
      if k + 1 < n {
        body.write_string(" ")
      }
    }
    sb.write_string("Dialogue: 0,")
    sb.write_string(@timing.fmt_time(start))
    sb.write_string(",")
    sb.write_string(@timing.fmt_time(end))
    sb.write_string(",Read,,0,0,0,,")
    sb.write_string(body.to_string())
    sb.write_string("\n")
    wi = wi + n
  }
  sb.to_string()
}
