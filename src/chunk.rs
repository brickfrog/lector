//! Paragraphs -> sentence-sized chunks. Port of the MoonBit core `chunk`.
//! Kokoro caps near 510 phoneme tokens, so synth units are sentence-sized.

fn split_sentences(p: &str) -> Vec<String> {
    let cs: Vec<char> = p.chars().collect();
    let n = cs.len();
    let mut out = vec![];
    let mut buf = String::new();
    let mut started = false;
    let mut i = 0;
    while i < n {
        let c = cs[i];
        let is_ws = matches!(c, ' ' | '\n' | '\t' | '\r');
        if is_ws {
            if started {
                buf.push(' ');
            }
            while i + 1 < n && matches!(cs[i + 1], ' ' | '\n' | '\t' | '\r') {
                i += 1;
            }
            i += 1;
            continue;
        }
        buf.push(c);
        started = true;
        if c == '.' || c == '!' || c == '?' {
            let mut j = i + 1;
            if j < n && matches!(cs[j], '"' | '\'' | ')' | ']') {
                buf.push(cs[j]);
                j += 1;
            }
            if j >= n || matches!(cs[j], ' ' | '\n' | '\t' | '\r') {
                out.push(std::mem::take(&mut buf));
                started = false;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// Split paragraphs into sentences, greedily pack to <= max_chars per chunk.
pub fn sentence_pack(paras: &[String], max_chars: usize) -> Vec<String> {
    let mut chunks = vec![];
    let mut cur = String::new();
    for p in paras {
        for s in split_sentences(p) {
            if s.is_empty() {
                continue;
            }
            if cur.is_empty() {
                cur = s;
            } else if cur.chars().count() + s.chars().count() + 1 > max_chars {
                chunks.push(std::mem::take(&mut cur));
                cur = s;
            } else {
                cur.push(' ');
                cur.push_str(&s);
            }
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}
