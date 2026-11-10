use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lector"))
        .args(args)
        .output()
        .unwrap()
}
fn success(out: Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn epub(path: &Path) {
    let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
    let entries = [
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ),
        (
            "OPS/book.opf",
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">test</dc:identifier><dc:title>Fixture</dc:title><dc:language>en</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="body" href="body.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="nav"/><itemref idref="body"/></spine></package>"#,
        ),
        (
            "OPS/nav.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="body.xhtml#one">First chapter</a></li><li><a href="body.xhtml#two">Second chapter</a></li></ol></nav></body></html>"#,
        ),
        (
            "OPS/body.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><section id="one"><p>First prose with un<em>break</em>able words.</p><ul><li>List item.</li></ul></section><section id="two"><p>Second prose.</p><div>Container text.</div></section></body></html>"#,
        ),
    ];
    for (name, text) in entries {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}
fn checkpoint(parent: &Path) -> std::path::PathBuf {
    let dir = parent.join(".fixture.ckpt");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("plan.json"), r#"{"identity":["test"],"units":["Hello"],"chapters":[[0,"Start"]],"title":"Fixture","author":null,"voice":"test","lang":"en"}"#).unwrap();
    fs::write(dir.join("audio.s16"), vec![0u8; 48000]).unwrap();
    fs::write(
        dir.join("timed.jsonl"),
        "{\"samples\":24000,\"timed\":[{\"text\":\"Hello\",\"start_s\":0.0,\"end_s\":1.0}]}\n",
    )
    .unwrap();
    dir
}

#[test]
fn epub_navigation_selection_and_text_export() {
    let d = tempfile::tempdir().unwrap();
    let book = d.path().join("book.epub");
    epub(&book);
    let book = book.to_str().unwrap();
    let text = success(cli(&["inspect", book]));
    assert!(
        text.contains("unbreakable")
            && text.contains("List item.")
            && text.contains("Container text.")
    );
    assert!(!text.contains("First chapter")); // nav labels aren't narrated
    let plan = success(cli(&[book, "--dry-run"]));
    assert!(
        plan.contains("First chapter")
            && plan.contains("Second chapter")
            && plan.contains("2 chapters")
    );
    let text = success(cli(&[
        "inspect",
        book,
        "--chapters",
        "2",
        "--assets",
        "/does/not/exist",
    ]));
    assert!(!text.contains("First prose") && text.contains("Second prose"));
    assert!(!cli(&["inspect", book, "--chapters", "3"]).status.success());
    assert!(!cli(&["inspect", book, "--exclude-chapters", "1-2"])
        .status
        .success());
}

#[test]
fn global_assets_option_before_subcommand() {
    let d = tempfile::tempdir().unwrap();
    let text = success(cli(&["--assets", d.path().to_str().unwrap(), "voices"]));
    assert!(text.contains("no voices"));
}

#[test]
#[cfg(unix)]
fn failed_encoder_keeps_checkpoint_and_existing_outputs() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let cp = checkpoint(d.path());
    let bin = d.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let fake = bin.join("ffmpeg");
    fs::write(
        &fake,
        "#!/bin/sh\necho deliberate-encoder-failure >&2\nexit 29\n",
    )
    .unwrap();
    fs::set_permissions(fake, fs::Permissions::from_mode(0o755)).unwrap();
    let old = d.path().join("fixture.m4b");
    fs::write(&old, b"previous audio").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_lector"))
        .env("PATH", bin)
        .args([
            "reencode",
            cp.to_str().unwrap(),
            "-o",
            d.path().to_str().unwrap(),
            "--no-cover",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("deliberate-encoder-failure"));
    assert_eq!(fs::read(old).unwrap(), b"previous audio");
    assert_eq!(fs::metadata(cp.join("audio.s16")).unwrap().len(), 48000);
    assert!(cp.join("timed.jsonl").exists());
}

#[test]
#[ignore = "requires ffmpeg and ffprobe; CI runs tool tests separately"]
fn reencode_without_model_scales_chapters_and_exports() {
    let d = tempfile::tempdir().unwrap();
    let cp = checkpoint(d.path());
    success(cli(&[
        "--assets",
        "/does/not/exist",
        "reencode",
        cp.to_str().unwrap(),
        "-o",
        d.path().to_str().unwrap(),
        "--speed",
        "2",
        "--no-cover",
        "--export-timings",
    ]));
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_chapters",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(d.path().join("fixture.m4b"))
        .output()
        .unwrap();
    let info: serde_json::Value = serde_json::from_str(&success(probe)).unwrap();
    assert_eq!(info["chapters"][0]["end_time"], "0.500000");
    let words: serde_json::Value =
        serde_json::from_slice(&fs::read(d.path().join("fixture.json")).unwrap()).unwrap();
    assert_eq!(words[0]["end_s"], 0.5);
    assert!(fs::read_to_string(d.path().join("fixture.srt"))
        .unwrap()
        .contains("00:00:00,500"));
    assert!(cp.join("audio.s16").exists());
}

#[test]
#[ignore = "requires Poppler; CI runs tool tests separately"]
fn invalid_pdf_reports_parser_failure() {
    let d = tempfile::tempdir().unwrap();
    let input = d.path().join("invalid.pdf");
    fs::write(&input, b"not a PDF").unwrap();
    let result = cli(&[input.to_str().unwrap(), "--dry-run"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("pdftotext failed"));
}

#[test]
#[cfg(unix)]
fn calibre_conversion_uses_private_cleaned_temp_directories() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let converted = d.path().join("converted.epub");
    epub(&converted);
    let input = d.path().join("book.mobi");
    fs::write(&input, b"fixture").unwrap();
    let fake = d.path().join("ebook-convert");
    fs::write(&fake, "#!/bin/sh\nif [ \"$1\" = --version ]; then exit 0; fi\nprintf '%s' \"$2\" > \"$LECTOR_TEST_PATH\"\n/bin/cp \"$LECTOR_TEST_EPUB\" \"$2\"\n").unwrap();
    fs::set_permissions(fake, fs::Permissions::from_mode(0o755)).unwrap();
    let recorded = d.path().join("path.txt");
    let mut paths = vec![];
    for _ in 0..2 {
        let out = Command::new(env!("CARGO_BIN_EXE_lector"))
            .env("PATH", d.path())
            .env("LECTOR_TEST_PATH", &recorded)
            .env("LECTOR_TEST_EPUB", &converted)
            .args(["inspect", input.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(success(out).contains("First prose"));
        let path = fs::read_to_string(&recorded).unwrap();
        assert!(!Path::new(&path).parent().unwrap().exists());
        paths.push(path);
    }
    assert_ne!(paths[0], paths[1]);
}

#[test]
fn oversized_epub_entry_is_rejected_before_parsing() {
    let d = tempfile::tempdir().unwrap();
    let book = d.path().join("bomb.epub");
    let mut zip = zip::ZipWriter::new(fs::File::create(&book).unwrap());
    zip.start_file(
        "huge.txt",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    let chunk = vec![b'x'; 1024 * 1024];
    for _ in 0..65 {
        zip.write_all(&chunk).unwrap();
    }
    zip.finish().unwrap();
    let result = cli(&["inspect", book.to_str().unwrap()]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("64 MiB"));
}

#[test]
fn epub2_ncx_chapter_anchors() {
    use std::io::Read;
    let d = tempfile::tempdir().unwrap();
    let book = d.path().join("book.epub");
    epub(&book);
    let mut archive = zip::ZipArchive::new(fs::File::open(&book).unwrap()).unwrap();
    let mut entries = vec![];
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut content = String::new();
        entry.read_to_string(&mut content).unwrap();
        if name == "OPS/book.opf" {
            content = content
                .replace("version=\"3.0\"", "version=\"2.0\"")
                .replace(
                    "href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"",
                    "href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"",
                )
                .replace("<spine><itemref idref=\"nav\"/>", "<spine toc=\"nav\">");
        }
        entries.push((name, content));
    }
    drop(archive);
    entries.push(("OPS/toc.ncx".into(), r##"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head/><docTitle><text>Fixture</text></docTitle><navMap><navPoint id="a" playOrder="1"><navLabel><text>NCX first</text></navLabel><content src="body.xhtml#one"/></navPoint><navPoint id="b" playOrder="2"><navLabel><text>NCX second</text></navLabel><content src="body.xhtml#two"/></navPoint></navMap></ncx>"##.into()));
    let mut zip = zip::ZipWriter::new(fs::File::create(&book).unwrap());
    for (name, content) in entries {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let result = success(cli(&[book.to_str().unwrap(), "--dry-run"]));
    assert!(
        result.contains("NCX first")
            && result.contains("NCX second")
            && result.contains("2 chapters")
    );
}

#[test]
#[ignore = "requires Poppler; CI runs tool tests separately"]
fn pdf_text_and_heading_extraction() {
    let d = tempfile::tempdir().unwrap();
    let input = d.path().join("book.pdf");
    let content = "BT /F1 20 Tf 72 740 Td (Chapter One) Tj /F1 12 Tf 0 -30 Td (Hello from a simple PDF page with enough body text.) Tj ET";
    let objects = ["<< /Type /Catalog /Pages 2 0 R >>".into(), "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(), "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(), format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len())];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = vec![];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{object}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str("xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    fs::write(&input, pdf).unwrap();
    let text = success(cli(&["inspect", input.to_str().unwrap()]));
    assert!(text.contains("Hello from a simple PDF page"));
    let plan = success(cli(&[input.to_str().unwrap(), "--dry-run"]));
    assert!(plan.contains("Chapter One") && plan.contains("1 chapters"));
}
