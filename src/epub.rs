//! EPUB -> Blocks. EPUB is structured (spine of XHTML, explicit <p>/<h*>), so
//! this is cleaner than the PDF path: no font-size/indent heuristics, no
//! dehyphenation, and chapter headings come straight from the markup.

use crate::clean::squeeze;
use crate::source::{Block, Doc};
use anyhow::{Context, Result};
use epub::doc::EpubDoc;
use scraper::{Html, Selector};

/// Cover image bytes + mime, if the EPUB declares one.
pub fn cover(path: &str) -> Option<(Vec<u8>, String)> {
    EpubDoc::new(path).ok()?.get_cover()
}

pub fn load(path: &str) -> Result<Doc> {
    let mut doc = EpubDoc::new(path).context("open epub")?;
    let title = doc.mdata("title").map(|m| m.value.clone());
    let author = doc.mdata("creator").map(|m| m.value.clone());
    let sel = Selector::parse("h1,h2,h3,h4,h5,h6,p").unwrap();
    let mut blocks = vec![];
    loop {
        if doc.get_current_mime().unwrap_or_default().contains("html") {
            if let Some((content, _)) = doc.get_current_str() {
                parse_xhtml(&content, &sel, &mut blocks);
            }
        }
        if !doc.go_next() {
            break;
        }
    }
    Ok(Doc {
        title,
        author,
        blocks,
    })
}

fn parse_xhtml(html: &str, sel: &Selector, out: &mut Vec<Block>) {
    let dom = Html::parse_document(html);
    for el in dom.select(sel) {
        let text = squeeze(&el.text().collect::<Vec<_>>().join(" "));
        if text.is_empty() {
            continue;
        }
        match el.value().name() {
            "h1" => out.push(Block::Heading(1, text)),
            "h2" => out.push(Block::Heading(2, text)),
            _ if el.value().name().starts_with('h') => out.push(Block::Heading(3, text)),
            _ => out.push(Block::Para(text)),
        }
    }
}
