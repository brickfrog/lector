//! PDF spans -> narratable paragraphs. Port of the MoonBit core `clean`:
//! drop sub-body spans (footnotes/headers/page numbers), group into lines, split
//! paragraphs on left-indent, dehyphenate, collapse whitespace.

use crate::source::Block;

#[derive(Clone)]
pub struct Span {
    pub text: String,
    pub size: f64,
    pub x: f64,
    pub y: f64,
}

const INDENT_EPS: f64 = 4.0;
const LINE_TOL: f64 = 4.0;

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\n' || c == '\t' || c == '\r'
}

/// Collapse whitespace runs to single spaces, trim ends.
pub fn squeeze(s: &str) -> String {
    let mut out = String::new();
    let mut started = false;
    let mut pending = false;
    for c in s.chars() {
        if is_ws(c) {
            if started {
                pending = true;
            }
        } else {
            if pending {
                out.push(' ');
                pending = false;
            }
            out.push(c);
            started = true;
        }
    }
    out
}

/// Rejoin words split across a line break: "infor- mation" -> "information".
/// Only lowercase-hyphen-space-lowercase, to spare real em-dashes.
pub fn dehyphenate(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let n = cs.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        let c = cs[i];
        if c.is_ascii_lowercase() && i + 1 < n && cs[i + 1] == '-' {
            let mut j = i + 2;
            let mut saw_ws = false;
            while j < n && is_ws(cs[j]) {
                saw_ws = true;
                j += 1;
            }
            if saw_ws && j < n && cs[j].is_ascii_lowercase() {
                out.push(c); // drop hyphen + whitespace; next char attaches
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_numeral_char(c: char) -> bool {
    c.is_ascii_digit() || "ivxlcIVXLC".contains(c)
}

fn is_standalone_numeral(s: &str) -> bool {
    let n = s.chars().count();
    if !(1..=4).contains(&n) {
        return false;
    }
    s.chars().all(is_numeral_char)
}

/// Dominant font size by character count (the body text), rounded to a point.
pub fn detect_body_size(spans: &[Span]) -> f64 {
    let mut counts = [0i64; 256];
    for s in spans {
        let k = (s.size + 0.5) as i64;
        if (0..256).contains(&k) {
            counts[k as usize] += s.text.chars().count() as i64;
        }
    }
    let mut best = 16usize;
    let mut best_c = -1i64;
    for k in 0..256 {
        if counts[k] > best_c {
            best_c = counts[k];
            best = k;
        }
    }
    best as f64
}

fn sorted_by_yx(spans: &[Span]) -> Vec<Span> {
    let mut r: Vec<Span> = spans.to_vec();
    r.sort_by(|a, b| {
        a.y.partial_cmp(&b.y)
            .unwrap()
            .then(a.x.partial_cmp(&b.x).unwrap())
    });
    r
}

struct Line {
    x: f64,
    size: f64,
    text: String,
}

fn heading_level(ratio: f64) -> u8 {
    if ratio >= 1.8 {
        1
    } else if ratio >= 1.4 {
        2
    } else {
        3
    }
}

/// One page's spans -> blocks (paragraphs + headings, body text only, reflowed).
/// `body` is the document's dominant font size; lines notably larger than it and
/// short become Headings (chapter/section markers, still narrated).
pub fn page_blocks(spans: &[Span], body: f64) -> Vec<Block> {
    let min_size = body * 0.85;
    let kept: Vec<Span> = spans
        .iter()
        .filter(|s| s.size >= min_size && !squeeze(&s.text).is_empty())
        .cloned()
        .collect();
    if kept.is_empty() {
        return vec![];
    }
    let ordered = sorted_by_yx(&kept);

    // group into lines by y proximity, tracking the line's dominant (max) size
    let mut lines: Vec<Line> = vec![];
    let mut cur_y = ordered[0].y;
    let mut cur_x = ordered[0].x;
    let mut cur_size = ordered[0].size;
    let mut cur_txt = ordered[0].text.clone();
    for s in &ordered[1..] {
        if s.y - cur_y <= LINE_TOL {
            cur_txt.push(' ');
            cur_txt.push_str(&s.text);
            cur_x = cur_x.min(s.x);
            cur_size = cur_size.max(s.size);
        } else {
            lines.push(Line {
                x: cur_x,
                size: cur_size,
                text: squeeze(&cur_txt),
            });
            cur_txt = s.text.clone();
            cur_y = s.y;
            cur_x = s.x;
            cur_size = s.size;
        }
    }
    lines.push(Line {
        x: cur_x,
        size: cur_size,
        text: squeeze(&cur_txt),
    });

    let margin = lines.iter().map(|l| l.x).fold(f64::INFINITY, f64::min);
    let head_thresh = body * 1.25;

    let mut blocks: Vec<Block> = vec![];
    let mut cur = String::new();
    let mut have = false;
    let flush = |cur: &mut String, have: &mut bool, blocks: &mut Vec<Block>| {
        if *have {
            blocks.push(Block::Para(squeeze(&dehyphenate(cur))));
            cur.clear();
            *have = false;
        }
    };
    for ln in &lines {
        if is_standalone_numeral(&ln.text) {
            continue;
        }
        let is_heading = ln.size >= head_thresh && ln.text.split_whitespace().count() <= 14;
        if is_heading {
            flush(&mut cur, &mut have, &mut blocks);
            // merge consecutive heading lines of similar size into one heading
            if let Some(Block::Heading(_, prev)) = blocks.last_mut() {
                prev.push(' ');
                prev.push_str(&ln.text);
            } else {
                blocks.push(Block::Heading(
                    heading_level(ln.size / body),
                    ln.text.clone(),
                ));
            }
            continue;
        }
        let indented = ln.x > margin + INDENT_EPS;
        if indented && have {
            flush(&mut cur, &mut have, &mut blocks);
            cur.push_str(&ln.text);
            have = true;
        } else {
            if have {
                cur.push(' ');
            }
            cur.push_str(&ln.text);
            have = true;
        }
    }
    flush(&mut cur, &mut have, &mut blocks);
    blocks
}
