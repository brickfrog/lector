//! PDF -> per-page text spans via `pdftotext -bbox` (poppler). The word box
//! height is a font-size proxy, which the cleaner uses to drop footnotes/headers.

use crate::clean::Span;
use anyhow::{Context, Result};
use std::process::Command;

fn attr(line: &str, name: &str) -> Option<f64> {
    let key = format!("{name}=\"");
    let i = line.find(&key)? + key.len();
    let rest = &line[i..];
    let j = rest.find('"')?;
    rest[..j].parse::<f64>().ok().filter(|v| v.is_finite())
}

fn unescape(s: &str) -> String {
    scraper::Html::parse_fragment(s)
        .root_element()
        .text()
        .collect()
}

/// Extract spans grouped by page. Coordinates are top-down (yMin from top).
pub fn extract(pdf: &str) -> Result<Vec<Vec<Span>>> {
    let out = Command::new("pdftotext")
        .args(["-bbox", pdf, "-"])
        .output()
        .context("pdftotext failed to run (is poppler installed?)")?;
    anyhow::ensure!(
        out.status.success(),
        "pdftotext failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    let xml = String::from_utf8_lossy(&out.stdout);
    let mut pages: Vec<Vec<Span>> = vec![];
    let mut cur: Vec<Span> = vec![];
    let mut in_page = false;
    for line in xml.lines() {
        let l = line.trim();
        if l.starts_with("<page") {
            if in_page {
                pages.push(std::mem::take(&mut cur));
            }
            in_page = true;
        } else if l.starts_with("<word") {
            let (Some(xmin), Some(ymin), Some(_xmax), Some(ymax)) = (
                attr(l, "xMin"),
                attr(l, "yMin"),
                attr(l, "xMax"),
                attr(l, "yMax"),
            ) else {
                continue;
            };
            let text = match (l.find('>'), l.rfind("</word>")) {
                (Some(a), Some(b)) if b > a + 1 => unescape(&l[a + 1..b]),
                _ => continue,
            };
            cur.push(Span {
                text,
                size: ymax - ymin,
                x: xmin,
                y: ymin,
            });
        }
    }
    if in_page {
        pages.push(cur);
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entities_decode_once_including_numeric_unicode() {
        assert_eq!(
            unescape("A &amp; B &amp;lt; &#x2014; &lt;tag&gt;"),
            "A & B &lt; — <tag>"
        );
    }
}
