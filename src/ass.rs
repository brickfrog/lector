//! Render timed words to an ASS read-along document (word-timed). Port of the MoonBit core `ass`.

use crate::timing::{fmt_time, Timed};

const HEADER: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Read,Georgia,40,&H0000FFFF,&H80AAAAAA,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,1,2,80,80,60,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";

fn ends_sentence(w: &str) -> bool {
    let enders = ['.', '!', '?', ':', ';'];
    let quotes = ["", "\"", "'", ")", "]"];
    for q in quotes {
        for e in enders {
            if w.ends_with(&format!("{e}{q}")) {
                return true;
            }
        }
    }
    false
}

/// Group words into screen lines: break on a sentence-ender once the line has
/// >= 4 words, else at max_words.
fn split_lines(words: &[String], max_words: usize) -> Vec<Vec<String>> {
    let mut lines = vec![];
    let mut cur: Vec<String> = vec![];
    for w in words {
        cur.push(w.clone());
        if (ends_sentence(w) && cur.len() >= 4) || cur.len() >= max_words {
            lines.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn escape(w: &str) -> String {
    w.chars()
        .map(|c| match c {
            '{' => '(',
            '}' => ')',
            '\n' => ' ',
            other => other,
        })
        .collect()
}

fn round_cs(sec: f64) -> i64 {
    let v = (sec * 100.0 + 0.5) as i64;
    if v < 1 {
        1
    } else {
        v
    }
}

/// Render timed tokens to a full ASS read-along document.
pub fn to_ass(timed: &[Timed], max_words: usize) -> String {
    let mut out = String::from(HEADER);
    let words: Vec<String> = timed.iter().map(|t| t.text.clone()).collect();
    let lines = split_lines(&words, max_words);
    let mut wi = 0usize;
    for line in &lines {
        let n = line.len();
        let start = timed[wi].start_s;
        let mut durs = Vec::with_capacity(n);
        for k in 0..n {
            let gi = wi + k;
            let this_start = timed[gi].start_s;
            let nxt = if k + 1 < n {
                timed[wi + k + 1].start_s
            } else {
                timed[gi].end_s
            };
            durs.push(round_cs(nxt - this_start));
        }
        let sumcs: i64 = durs.iter().sum();
        let end = start + sumcs as f64 / 100.0;
        let mut body = String::new();
        for k in 0..n {
            body.push_str(&format!("{{\\kf{}}}", durs[k]));
            body.push_str(&escape(&line[k]));
            if k + 1 < n {
                body.push(' ');
            }
        }
        out.push_str(&format!(
            "Dialogue: 0,{},{},Read,,0,0,0,,{}\n",
            fmt_time(start),
            fmt_time(end),
            body
        ));
        wi += n;
    }
    out
}
