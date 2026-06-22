//! Input abstraction: any supported book format -> an ordered list of Blocks
//! (paragraphs + headings). Downstream (chunk/synth/timing) only sees text;
//! headings drive chapter markers.

use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

pub enum Block {
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
    let tmp = std::env::temp_dir().join(format!(
        "lector-{}.epub",
        Path::new(path)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    let status = Command::new("ebook-convert")
        .arg(path)
        .arg(&tmp)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("ebook-convert failed on {path}");
    }
    let doc = crate::epub::load(tmp.to_str().unwrap());
    let _ = std::fs::remove_file(&tmp);
    doc
}
