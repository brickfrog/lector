//! Per-phoneme pred_dur -> per-word timestamps. Port of the MoonBit core
//! `timing` (align_words + fmt_time) and `assemble` (merge_timed).

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Timed {
    pub text: String,
    pub start_s: f64,
    pub end_s: f64,
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

// count phonemes ignoring spaces + stress marks (move between isolated/in-context)
fn stripped_len(s: &str) -> usize {
    s.chars()
        .filter(|&c| c != ' ' && c != '\u{02c8}' && c != '\u{02cc}')
        .count()
}

/// Robust word alignment: exact per-group pred_dur timeline, then place each
/// display word on it by cumulative phoneme-length proportion. Tolerant of
/// espeak function-word merges / number expansions; never drops or desyncs.
pub fn align_words(
    words: &[String],
    word_phonemes: &[String],
    phonemes: &str,
    pred_dur: &[i64],
) -> Vec<Timed> {
    let mut result = vec![];
    let w = words.len();
    if w == 0 || pred_dur.len() < 3 {
        return result;
    }
    let groups: Vec<&str> = phonemes.split_whitespace().collect();
    let g = groups.len();
    if g == 0 {
        return result;
    }

    // exact per-group [start,end] via the kokoro join magic (half-frames, -3 BOS)
    let magic = 80.0f64;
    let base = pred_dur[0] - 3;
    let mut left: i64 = 2 * if base > 0 { base } else { 0 };
    let mut right = left;
    let mut i = 1usize;
    let mut gstart = vec![0f64; g];
    let mut gend = vec![0f64; g];
    let mut timed = 0usize;
    for gi in 0..g {
        if i >= pred_dur.len() - 1 {
            break;
        }
        let glen = char_len(groups[gi]);
        let j = i + glen;
        if j >= pred_dur.len() {
            break;
        }
        let s = left as f64 / magic;
        let mut dur: i64 = 0;
        for k in i..j {
            dur += pred_dur[k];
        }
        let has_space = gi < g - 1;
        let space = if has_space { pred_dur[j] } else { 0 };
        left = right + 2 * dur + space;
        let e = left as f64 / magic;
        right = left + space;
        i = j + if has_space { 1 } else { 0 };
        gstart[gi] = s;
        gend[gi] = e;
        timed += 1;
    }
    if timed == 0 {
        return result;
    }
    for k in timed..g {
        gstart[k] = gend[timed - 1];
        gend[k] = gend[timed - 1];
    }

    // cumulative stripped-phoneme weights for the time axis
    let mut gcum = vec![0f64; g];
    let mut gw = vec![0f64; g];
    let mut gtot = 0f64;
    for gi in 0..g {
        let l = stripped_len(groups[gi]) as f64;
        let lw = if l > 0.0 { l } else { 1.0 };
        gw[gi] = lw;
        gtot += lw;
        gcum[gi] = gtot;
    }
    let mut wcum = vec![0f64; w + 1];
    let mut wtot = 0f64;
    for wi in 0..w {
        let ph = word_phonemes.get(wi).map(|s| s.as_str()).unwrap_or("");
        let l = stripped_len(ph) as f64;
        wtot += if l > 0.0 { l } else { 1.0 };
        wcum[wi + 1] = wtot;
    }
    if wtot == 0.0 {
        return result;
    }

    let time_at = |p: f64| -> f64 {
        if p <= 0.0 {
            return gstart[0];
        }
        for gi in 0..g {
            if p <= gcum[gi] || gi == g - 1 {
                let gsp = gcum[gi] - gw[gi];
                let frac = ((p - gsp) / gw[gi]).clamp(0.0, 1.0);
                return gstart[gi] + frac * (gend[gi] - gstart[gi]);
            }
        }
        gend[g - 1]
    };
    for wi in 0..w {
        let p0 = wcum[wi] / wtot * gtot;
        let p1 = wcum[wi + 1] / wtot * gtot;
        result.push(Timed {
            text: words[wi].clone(),
            start_s: time_at(p0),
            end_s: time_at(p1),
        });
    }
    result
}

/// Format seconds as an ASS "H:MM:SS.cc" timestamp.
pub fn fmt_time(t: f64) -> String {
    let total_cs = (t * 100.0 + 0.5) as i64;
    let h = total_cs / 360000;
    let rem = total_cs % 360000;
    let m = rem / 6000;
    let rem2 = rem % 6000;
    let s = rem2 / 100;
    let cs = rem2 % 100;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

/// Concatenate per-unit timings onto one timeline, shifting by cumulative audio.
pub fn merge_timed(units: &[Vec<Timed>], durations: &[f64]) -> Vec<Timed> {
    let mut out = vec![];
    let mut off = 0f64;
    for (i, u) in units.iter().enumerate() {
        for t in u {
            out.push(Timed {
                text: t.text.clone(),
                start_s: t.start_s + off,
                end_s: t.end_s + off,
            });
        }
        off += durations[i];
    }
    out
}
