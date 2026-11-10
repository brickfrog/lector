//! EPUB spine extraction with navigation markers and block-aware text traversal.
use crate::clean::squeeze;
use crate::source::{Block, Doc};
use anyhow::{Context, Result};
use epub::doc::{EpubDoc, NavPoint};
use scraper::{ElementRef, Html, Selector};
use std::path::{Component, Path};

/// Bound decompression before passing the archive to the EPUB parser. Counts
/// actual bytes, not just attacker-controlled central-directory size fields.
fn preflight(path: &str) -> Result<()> {
    let file = std::fs::File::open(path)?;
    anyhow::ensure!(
        file.metadata()?.len() <= 1024 * 1024 * 1024,
        "EPUB exceeds 1 GiB compressed limit"
    );
    let mut zip = zip::ZipArchive::new(file)?;
    anyhow::ensure!(
        zip.len() <= 10000,
        "EPUB has too many entries (limit 10000)"
    );
    let mut total = 0u64;
    use std::io::Read;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let count = std::io::copy(&mut entry.take(64 * 1024 * 1024 + 1), &mut std::io::sink())?;
        anyhow::ensure!(
            count <= 64 * 1024 * 1024,
            "EPUB entry exceeds 64 MiB decompressed limit"
        );
        total += count;
        anyhow::ensure!(
            total <= 512 * 1024 * 1024,
            "EPUB exceeds 512 MiB decompressed limit"
        );
    }
    Ok(())
}

pub fn cover(path: &str) -> Option<(Vec<u8>, String)> {
    preflight(path).ok()?;
    EpubDoc::new(path).ok()?.get_cover()
}

#[derive(Clone, Debug)]
struct Mark {
    file: String,
    id: Option<String>,
    title: String,
}

fn target(base: &Path, href: &str, title: String) -> Mark {
    let (file, id) = href
        .split_once('#')
        .map_or((href, None), |(f, i)| (f, Some(i)));
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    };
    let joined = base.join(decode(file));
    let mut parts = vec![];
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    Mark {
        file: parts.join("/"),
        id: id.filter(|s| !s.is_empty()).map(decode),
        title,
    }
}

fn ncx_marks(points: &[NavPoint], out: &mut Vec<Mark>, depth: usize) {
    for point in points {
        out.push(target(
            Path::new(""),
            &point.content.to_string_lossy(),
            point.label.clone(),
        ));
        if depth < 2 {
            ncx_marks(&point.children, out, depth + 1);
        }
    }
}

pub fn load(path: &str) -> Result<Doc> {
    preflight(path).context("validate EPUB archive")?;
    let mut doc = EpubDoc::new(path).context("open epub")?;
    let title = doc.mdata("title").map(|m| m.value.clone());
    let author = doc.mdata("creator").map(|m| m.value.clone());
    let mut marks = vec![];
    // Prefer EPUB3 navigation; retain NCX support for EPUB2.
    if let Some(id) = doc.get_nav_id() {
        let base = doc
            .resources
            .get(&id)
            .map(|r| r.path.parent().unwrap_or(Path::new("")).to_path_buf());
        if let (Some(base), Some((content, _))) = (base, doc.get_resource_str(&id)) {
            let html = Html::parse_document(&content);
            let nav_sel = Selector::parse("nav").unwrap();
            let link_sel = Selector::parse("a[href]").unwrap();
            if let Some(nav) = html.select(&nav_sel).find(|e| {
                e.attr("epub:type")
                    .is_some_and(|s| s.split_whitespace().any(|t| t == "toc"))
                    || e.attr("role") == Some("doc-toc")
            }) {
                for link in nav.select(&link_sel) {
                    let title = squeeze(&link.text().collect::<String>());
                    if !title.is_empty() {
                        marks.push(target(&base, link.attr("href").unwrap(), title));
                    }
                }
            }
        }
    }
    if marks.is_empty() {
        ncx_marks(&doc.toc, &mut marks, 1);
    }
    let nav_id = doc.get_nav_id();
    let nav_path = nav_id.and_then(|id| doc.resources.get(&id).map(|r| r.path.clone()));
    let mut blocks = vec![];
    loop {
        if doc.get_current_mime().unwrap_or_default().contains("html") {
            let path = doc
                .get_current_path()
                .context("missing EPUB spine resource")?;
            // Navigation itself is not book prose.
            if Some(&path) != nav_path.as_ref() {
                let (content, _) = doc
                    .get_current_str()
                    .context("cannot read EPUB spine text")?;
                let file = target(Path::new(""), &path.to_string_lossy(), String::new()).file;
                let local: Vec<_> = marks.iter().filter(|m| m.file == file).cloned().collect();
                parse_xhtml(&content, &local, &mut blocks);
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

struct Extractor<'a> {
    out: &'a mut Vec<Block>,
    buffer: String,
    heading: Option<u8>,
    marks: &'a [Mark],
}
impl Extractor<'_> {
    fn flush(&mut self) {
        let text = squeeze(&std::mem::take(&mut self.buffer));
        if !text.is_empty() {
            self.out.push(match self.heading {
                Some(level) if self.marks.is_empty() => Block::Heading(level, text),
                _ => Block::Para(text),
            });
        }
    }
    fn visit(&mut self, el: ElementRef<'_>) {
        let name = el.value().name();
        if matches!(name, "head" | "script" | "style" | "nav" | "noscript")
            || el.attr("hidden").is_some()
            || el.attr("aria-hidden") == Some("true")
        {
            return;
        }
        if let Some(id) = el.attr("id").or_else(|| el.attr("name")) {
            for mark in self.marks.iter().filter(|m| m.id.as_deref() == Some(id)) {
                self.flush();
                self.out.push(Block::Chapter(mark.title.clone()));
            }
        }
        let block = matches!(
            name,
            "p" | "div"
                | "li"
                | "blockquote"
                | "pre"
                | "dt"
                | "dd"
                | "tr"
                | "section"
                | "article"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
        );
        if block {
            self.flush();
        }
        let previous = self.heading;
        if matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
            self.heading = name[1..].parse().ok();
        }
        if matches!(name, "br" | "td" | "th") {
            self.buffer.push(' ');
        }
        for child in el.children() {
            if let Some(text) = child.value().as_text() {
                self.buffer.push_str(text);
            } else if let Some(el) = ElementRef::wrap(child) {
                self.visit(el);
            }
        }
        if block {
            self.flush();
        }
        if matches!(name, "td" | "th") {
            self.buffer.push(' ');
        }
        self.heading = previous;
    }
}

fn parse_xhtml(html: &str, marks: &[Mark], out: &mut Vec<Block>) {
    for mark in marks.iter().filter(|m| m.id.is_none()) {
        out.push(Block::Chapter(mark.title.clone()));
    }
    let dom = Html::parse_document(html);
    let mut parser = Extractor {
        out,
        buffer: String::new(),
        heading: None,
        marks,
    };
    parser.visit(dom.root_element());
    parser.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_inline_words_and_hidden_content() {
        let mut out = vec![];
        parse_xhtml("<h1>Title</h1><p>un<em>break</em>able &amp; kind</p><ul><li>First<p>Nested</p>Tail</li><li>Second</li></ul><div>Plain<br>text</div><p hidden>secret</p><script>bad</script>", &[], &mut out);
        let texts: Vec<_> = out
            .iter()
            .map(|b| match b {
                Block::Para(t) | Block::Heading(_, t) | Block::Chapter(t) => t.as_str(),
            })
            .collect();
        assert_eq!(
            texts,
            [
                "Title",
                "unbreakable & kind",
                "First",
                "Nested",
                "Tail",
                "Second",
                "Plain text"
            ]
        );
    }
    #[test]
    fn navigation_anchors_split_without_duplicate_narration() {
        let mut out = vec![];
        let marks = vec![target(
            Path::new(""),
            "chapter.xhtml#part",
            "Navigation title".into(),
        )];
        parse_xhtml(
            "<p>Before</p><section id='part'><h1>Visible title</h1><p>After</p></section>",
            &marks,
            &mut out,
        );
        assert!(matches!(&out[1], Block::Chapter(t) if t == "Navigation title"));
        assert!(matches!(&out[2], Block::Para(t) if t == "Visible title"));
        assert_eq!(out.len(), 4);
    }
}
