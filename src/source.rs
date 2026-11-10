//! Input abstraction: any supported book format -> an ordered list of Blocks
//! (paragraphs + headings). Downstream (chunk/synth/timing) only sees text;
//! headings drive chapter markers.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Write the book's cover image into `dest_dir` and return its path, if any.
/// EPUB: the declared cover. PDF: page 1 rendered with pdftoppm.
pub fn write_cover(input: &str, dest_dir: &Path) -> Option<PathBuf> {
    match ext_of(input).as_str() {
        "epub" => {
            let (data, mime) = crate::epub::cover(input)?;
            let ext = if mime.contains("png") { "png" } else { "jpg" };
            let p = dest_dir.join(format!("cover.{ext}"));
            std::fs::write(&p, data).ok()?;
            Some(p)
        }
        "pdf" => {
            let prefix = dest_dir.join("cover"); // pdftoppm appends .png
            let ok = Command::new("pdftoppm")
                .args(["-png", "-f", "1", "-l", "1", "-singlefile", "-r", "96"])
                .arg(input)
                .arg(&prefix)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            let png = dest_dir.join("cover.png");
            (ok && png.exists()).then_some(png)
        }
        _ => None,
    }
}

pub enum Block {
    Chapter(String),     // navigation boundary; not narrated
    Heading(u8, String), // level 1..=3, title text
    Para(String),
}

#[derive(Default)]
pub struct Doc {
    pub title: Option<String>,
    pub author: Option<String>,
    pub blocks: Vec<Block>,
}

/// Load a book by extension. PDF natively; EPUB natively; other ebook formats
/// via `ebook-convert` (calibre) to a temp EPUB when available.
pub fn load(path: &str) -> Result<Doc> {
    let ext = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => pdf_doc(path),
        "epub" => crate::epub::load(path),
        "mobi" | "azw" | "azw3" | "fb2" | "fbz" | "lit" | "pdb" | "htmlz" => {
            convert_then_epub(path)
        }
        other => bail!("unsupported format: .{other} (try pdf or epub)"),
    }
}

// --- PDF: pdftotext spans -> clean blocks, with a global body size ---
fn pdf_doc(path: &str) -> Result<Doc> {
    let pages = crate::pdf::extract(path)?;
    let all: Vec<_> = pages.iter().flatten().cloned().collect();
    let body = crate::clean::detect_body_size(&all);
    let mut blocks = vec![];
    for spans in &pages {
        if spans.is_empty() {
            continue;
        }
        blocks.extend(crate::clean::page_blocks(spans, body));
    }
    let (title, author) = pdf_meta(path);
    Ok(Doc {
        title,
        author,
        blocks,
    })
}

fn pdf_meta(path: &str) -> (Option<String>, Option<String>) {
    let mut title = None;
    let mut author = None;
    if let Ok(out) = Command::new("pdfinfo").arg(path).output() {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(v) = line.strip_prefix("Title:") {
                let v = v.trim();
                if !v.is_empty() {
                    title = Some(v.to_string());
                }
            } else if let Some(v) = line.strip_prefix("Author:") {
                let v = v.trim();
                if !v.is_empty() {
                    author = Some(v.to_string());
                }
            }
        }
    }
    (title, author)
}

// --- other ebook formats: convert to a temp EPUB via calibre, then parse ---
fn convert_then_epub(path: &str) -> Result<Doc> {
    if Command::new("ebook-convert")
        .arg("--version")
        .output()
        .is_err()
    {
        bail!(
            "'{}' needs calibre's ebook-convert on PATH (or convert it to EPUB first)",
            path
        );
    }
    let temp = tempfile::Builder::new().prefix("lector-").tempdir()?;
    let tmp = temp.path().join("converted.epub");
    let status = Command::new("ebook-convert")
        .arg(path)
        .arg(&tmp)
        .stdout(std::process::Stdio::null())
        .output()?;
    if !status.status.success() {
        bail!(
            "ebook-convert failed on {path}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    let doc = crate::epub::load(tmp.to_str().context("temporary path must be UTF-8")?);
    let _ = std::fs::remove_file(&tmp);
    doc
}
