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
