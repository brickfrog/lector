//! lector — render a book into a chaptered read-along audiobook (.m4b + .ass).
//!
//!   lector book.epub -o out/ --voice af_heart --speed 1.25
//!   lector voices
//!   lector --help
//!
//! source (PDF via pdftotext / EPUB natively / others via ebook-convert) ->
//! Blocks -> chapters at headings -> sentence_pack -> Kokoro synth (onnxruntime,
//! ~4x realtime) -> per-word timing from pred_dur -> stream PCM -> ffmpeg m4b
//! (AAC + chapter markers) + read-along .ass. Crash-safe: each unit's audio +
//! timings commit to a .ckpt dir as it finishes, so a re-run resumes instead of
//! redoing hours.

mod ass;
mod checkpoint;
mod chunk;
mod clean;
mod epub;
mod pdf;
mod source;
mod synth;
mod timing;

use anyhow::{bail, Context, Result};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use source::Block;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use timing::Timed;

const SR: usize = 24000;
const MAX_TOKENS: usize = 500; // Kokoro caps near 510 phoneme tokens
const WPM: f64 = 170.0; // Kokoro's rough speaking rate, for estimates only

#[derive(Parser)]
#[command(
    name = "lector",
    version,
    about = "Render a book into a chaptered read-along audiobook (.m4b + word-synced .ass)",
    after_help = "Play the result with:  mpv --sub-file=out/book.ass out/book.m4b",
    subcommand_precedence_over_arg = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Asset directory (also LECTOR_ASSETS); overrides automatic discovery
    #[arg(long, global = true, value_name = "DIR")]
    assets: Option<PathBuf>,
    #[command(flatten)]
    render: RenderArgs,
}

#[derive(Args)]
struct OutputArgs {
    /// Output directory
    #[arg(short, long, default_value = ".", value_name = "DIR")]
    out: PathBuf,

    /// Speaking speed multiplier, 0.25-4.0 (pitch-preserving; subtitles and
    /// chapters stay in sync)
    #[arg(short, long, default_value_t = 1.0, value_name = "X")]
    speed: f64,

    /// AAC bitrate in kbit/s
    #[arg(short, long, default_value_t = 64, value_name = "KBPS")]
    bitrate: u32,

    /// Max words per read-along subtitle line
    #[arg(long, default_value_t = 12, value_name = "N")]
    line_words: usize,

    /// Subtitle font
    #[arg(long, default_value = "Georgia", value_name = "FONT")]
    sub_font: String,

    /// Subtitle font size (720p canvas)
    #[arg(long, default_value_t = 40, value_name = "PT")]
    sub_size: u32,

    /// Skip embedding cover art
    #[arg(long)]
    no_cover: bool,

    /// Also export word timings as JSON and conventional SRT subtitles
    #[arg(long)]
    export_timings: bool,
}

#[derive(Args)]
struct RenderArgs {
    /// Book to render (.pdf, .epub, .mobi, .azw3, .fb2, ...)
    input: Option<PathBuf>,

    /// Kokoro voice (see `lector voices`)
    #[arg(short, long, default_value = "bm_george", value_name = "NAME")]
    voice: String,

    /// espeak accent: "a" (en-us), "b" (en-gb), or any espeak voice id.
    /// Default: inferred from the voice prefix
    #[arg(short, long, value_name = "LANG")]
    lang: Option<String>,

    /// onnxruntime threads [default: all cores]
    #[arg(short = 'j', long, value_name = "N")]
    threads: Option<usize>,

    /// Keep the checkpoint dir after a successful render (now the default)
    #[arg(long)]
    keep_checkpoint: bool,

    /// Delete reusable audio/timings after successful encoding
    #[arg(long, conflicts_with = "keep_checkpoint")]
    discard_checkpoint: bool,

    /// Parse and plan only: print the chapter breakdown and exit
    #[arg(long)]
    dry_run: bool,

    #[command(flatten)]
    output: OutputArgs,
    /// Include chapter numbers/ranges from --dry-run (e.g. 2-5,8)
    #[arg(long)]
    chapters: Option<String>,
    /// Exclude chapter numbers/ranges (e.g. 1,9-12)
    #[arg(long)]
    exclude_chapters: Option<String>,
    /// JSON dictionary mapping written words to spoken replacements
    #[arg(long, value_name = "FILE")]
    pronunciations: Option<PathBuf>,
    #[arg(skip)]
    preview_words: Option<usize>,
}

impl std::ops::Deref for RenderArgs {
    type Target = OutputArgs;
    fn deref(&self) -> &OutputArgs {
        &self.output
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// List installed voices
    Voices,
    /// Check external tools, asset integrity and model compatibility
    Doctor {
        #[arg(short, long, default_value = "bm_george")]
        voice: String,
    },
    /// Render a short audition (about 30 seconds at the default word limit)
    Preview {
        #[arg(long, default_value_t = 85, value_parser = clap::value_parser!(u32).range(1..))]
        words: u32,
        #[command(flatten)]
        render: RenderArgs,
    },
    /// Export the extracted text for inspection, without loading the model
    Inspect {
        input: PathBuf,
        /// Write plain text to a file instead of stdout
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long)]
        chapters: Option<String>,
        #[arg(long)]
        exclude_chapters: Option<String>,
    },
    /// Encode a completed checkpoint without source book, model or eSpeak
    Reencode {
        checkpoint: PathBuf,
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Serialize, Deserialize)]
struct Rec {
    samples: usize,
    timed: Vec<Timed>,
}

/// Deterministic render plan, cached in the checkpoint so resume is exact.
#[derive(Serialize, Deserialize)]
struct Plan {
    #[serde(default)]
    identity: Vec<String>,
    units: Vec<String>,
    chapters: Vec<(usize, String)>, // (unit index where the chapter starts, title)
    title: Option<String>,
    author: Option<String>,
    #[serde(default)]
    voice: String,
    #[serde(default)]
    lang: String,
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Kokoro voice naming: `<lang><gender>_<name>` (af_heart, bm_george, ...).
fn describe_voice(name: &str) -> String {
    let mut c = name.chars();
    let lang = match c.next() {
        Some('a') => "American English",
        Some('b') => "British English",
        Some('e') => "Spanish",
        Some('f') => "French",
        Some('h') => "Hindi",
        Some('i') => "Italian",
        Some('j') => "Japanese",
        Some('p') => "Brazilian Portuguese",
        Some('z') => "Mandarin",
        _ => return String::new(),
    };
    match c.next() {
        Some('f') => format!("{lang}, female"),
        Some('m') => format!("{lang}, male"),
        _ => String::new(),
    }
}

/// espeak voice for a Kokoro voice prefix, when --lang isn't given.
fn infer_lang(voice: &str) -> &'static str {
    match voice.chars().next() {
        Some('a') => "en-us",
        Some('e') => "es",
        Some('f') => "fr-fr",
        Some('h') => "hi",
        Some('i') => "it",
        Some('j') => "ja",
        Some('p') => "pt-br",
        Some('z') => "cmn",
        _ => "en-gb",
    }
}

fn installed_voices() -> Vec<String> {
    let mut v: Vec<String> = vec![];
    if let Ok(rd) = fs::read_dir(asset("")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "bin") {
                if let Some(stem) = p.file_stem() {
                    v.push(stem.to_string_lossy().into_owned());
                }
            }
        }
    }
    v.sort();
    v
}

fn list_voices() {
    let dir = asset("");
    let v = installed_voices();
    if v.is_empty() {
        println!("no voices in {} (run: just vendor)", dir.display());
        return;
    }
    let w = v.iter().map(|n| n.len()).max().unwrap_or(0);
    println!("voices in {}:", dir.display());
    for n in &v {
        let mut desc = describe_voice(n);
        if n == "bm_george" {
            desc.push_str(" (default)");
        }
        println!("  {n:w$}  {}", desc.trim());
    }
}

static ASSET_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn asset(name: &str) -> PathBuf {
    ASSET_DIR.get_or_init(|| discover_assets(None)).join(name)
}

fn discover_assets(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = explicit.or_else(|| std::env::var_os("LECTOR_ASSETS").map(PathBuf::from)) {
        return path;
    }
    let mut candidates = vec![PathBuf::from("assets")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("assets"));
        }
    }
    if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(data).join("lector/assets"));
    } else if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/share/lector/assets"));
    }
    candidates
        .into_iter()
        .find(|p| p.is_dir())
        .unwrap_or_else(|| PathBuf::from("assets"))
}

fn f32_to_s16(f: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(f.len() * 2);
    for &x in f {
        let s = x.clamp(-1.0, 1.0);
        let v = if s < 0.0 { s * 32768.0 } else { s * 32767.0 } as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

// char budget is a proxy; split anything that actually exceeds the token cap
fn token_split(text: &str, synth: &synth::Synth, out: &mut Vec<String>) -> Result<()> {
    if synth.phonemes_token_len(&synth.phonemize(text)?) <= MAX_TOKENS {
        out.push(text.to_string());
        return Ok(());
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= 1 {
        bail!(
            "single word exceeds model token limit; shorten or replace it: {}",
            text.chars().take(80).collect::<String>()
        );
    }
    let mid = words.len().div_ceil(2);
    token_split(&words[..mid].join(" "), synth, out)?;
    token_split(&words[mid..].join(" "), synth, out)
}

/// Group blocks into chapters: h1/h2 break chapters (and are narrated),
/// minor headings just narrate.
fn collect_chapters(blocks: Vec<Block>) -> Vec<(String, Vec<String>)> {
    let mut chapters: Vec<(String, Vec<String>)> = vec![];
    let mut cur_title = String::new();
    let mut cur: Vec<String> = vec![];
    for b in blocks {
        match b {
            Block::Chapter(t) => {
                if !cur.is_empty() {
                    chapters.push((std::mem::take(&mut cur_title), std::mem::take(&mut cur)));
                }
                cur_title = t;
            }
            Block::Heading(level, t) if level <= 2 => {
                if !cur.is_empty() {
                    chapters.push((std::mem::take(&mut cur_title), std::mem::take(&mut cur)));
                }
                cur_title = t.clone();
                cur.push(t); // narrate the heading too
            }
            Block::Heading(_, t) => cur.push(t),
            Block::Para(p) => cur.push(p),
        }
    }
    if !cur.is_empty() {
        chapters.push((cur_title, cur));
    }
    chapters
}

/// load -> chapters (split at headings) -> sentence_pack -> token-cap split.
/// Headings are narrated and also mark chapter starts (unit index -> title).
fn compute_plan(
    input: &str,
    synth: &synth::Synth,
    voice: &str,
    lang: &str,
    cli: &RenderArgs,
) -> Result<Plan> {
    let doc = source::load(input)?;
    let title = doc.title.clone();
    let author = doc.author.clone();
    let chapters = select_chapters(
        collect_chapters(doc.blocks),
        cli.chapters.as_deref(),
        cli.exclude_chapters.as_deref(),
    )?;
    if chapters.is_empty() {
        bail!("no readable text found in {input}");
    }

    let mut units = vec![];
    let mut marks: Vec<(usize, String)> = vec![];
    let chapters = limit_preview(chapters, cli.preview_words);
    for (ctitle, paras) in &chapters {
        let name = if ctitle.is_empty() {
            title.clone().unwrap_or_else(|| "Start".into())
        } else {
            ctitle.clone()
        };
        marks.push((units.len(), name));
        for c in chunk::sentence_pack(paras, 300) {
            token_split(&c, synth, &mut units)?;
        }
    }
    println!("{} chapters -> {} units", chapters.len(), units.len());
    Ok(Plan {
        identity: vec![],
        units,
        chapters: marks,
        title,
        author,
        voice: voice.to_string(),
        lang: lang.to_string(),
    })
}

/// Print the chapter breakdown without synthesizing anything.
fn dry_run(input: &str, cli: &RenderArgs) -> Result<()> {
    let doc = source::load(input)?;
    let speed = cli.speed;
    let chapters = select_chapters(
        collect_chapters(doc.blocks),
        cli.chapters.as_deref(),
        cli.exclude_chapters.as_deref(),
    )?;
    if chapters.is_empty() {
        bail!("no readable text found in {input}");
    }
    if let Some(t) = &doc.title {
        println!("title:  {t}");
    }
    if let Some(a) = &doc.author {
        println!("author: {a}");
    }
    let mut total_words = 0usize;
    println!();
    for (i, (title, paras)) in chapters.iter().enumerate() {
        let words: usize = paras.iter().map(|p| p.split_whitespace().count()).sum();
        total_words += words;
        let name = if title.is_empty() {
            "(untitled)"
        } else {
            title
        };
        println!(
            "  {:>3}. {name}  ({words} words, ~{})",
            i + 1,
            fmt_dur(words as f64 / WPM * 60.0 / speed)
        );
    }
    println!(
        "\n{} chapters, {total_words} words, ~{} at {speed}x (dry run; nothing rendered)",
        chapters.len(),
        fmt_dur(total_words as f64 / WPM * 60.0 / speed)
    );
    Ok(())
}

/// "1h23m" / "17m" / "42s" — for estimates and progress, not timestamps.
fn fmt_dur(secs: f64) -> String {
    let s = secs.round() as i64;
    if s >= 3600 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

/// Decompose a speed into ffmpeg atempo stages (each stage must be in 0.5-2.0).
fn atempo_chain(speed: f64) -> String {
    let mut parts: Vec<String> = vec![];
    let mut s = speed;
    while s > 2.0 {
        parts.push("atempo=2.0".into());
        s /= 2.0;
    }
    while s < 0.5 {
        parts.push("atempo=0.5".into());
        s *= 2.0;
    }
    parts.push(format!("atempo={s}"));
    parts.join(",")
}

// ffmetadata escaping: =, ;, #, \ and newline get a leading backslash
fn ffmeta_esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if matches!(c, '=' | ';' | '#' | '\\' | '\n') {
            out.push('\\');
        }
        out.push(if matches!(c, '\n' | '\r') { ' ' } else { c });
    }
    out
}

fn main() -> Result<()> {
    let matches = Cli::command().get_matches();
    if matches.subcommand_name().is_some() {
        for id in matches.ids() {
            if id != "assets"
                && matches.value_source(id.as_str()) == Some(clap::parser::ValueSource::CommandLine)
            {
                bail!("place render/output options after the subcommand (only --assets is global)");
            }
        }
    }
    let cli = Cli::from_arg_matches(&matches)?;
    ASSET_DIR.set(discover_assets(cli.assets)).ok();
    match cli.command {
        Some(Cmd::Voices) => {
            list_voices();
            Ok(())
        }
        Some(Cmd::Doctor { voice }) => doctor(&voice),
        Some(Cmd::Preview { words, mut render }) => {
            render.preview_words = Some(words as usize);
            run_render(render)
        }
        Some(Cmd::Inspect {
            input,
            out,
            chapters,
            exclude_chapters,
        }) => {
            let input = fs::canonicalize(input)?;
            let doc = source::load(input.to_str().context("input path must be UTF-8")?)?;
            let chapters = select_chapters(
                collect_chapters(doc.blocks),
                chapters.as_deref(),
                exclude_chapters.as_deref(),
            )?;
            let text = chapters
                .iter()
                .flat_map(|(_, paras)| paras.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n")
                + "\n";
            if let Some(out) = out {
                fs::write(out, text)?;
            } else {
                print!("{text}");
            }
            Ok(())
        }
        Some(Cmd::Reencode { checkpoint, output }) => reencode(&checkpoint, &output),
        None => run_render(cli.render),
    }
}

fn validate_output(cli: &OutputArgs) -> Result<()> {
    anyhow::ensure!(
        (0.25..=4.0).contains(&cli.speed),
        "--speed must be within 0.25-4.0"
    );
    anyhow::ensure!(
        cli.line_words > 0 && cli.sub_size > 0 && cli.bitrate > 0,
        "line words, font size and bitrate must be positive"
    );
    anyhow::ensure!(
        !cli.sub_font.contains([',', '\n', '\r']),
        "subtitle font cannot contain commas or newlines"
    );
    Ok(())
}

fn run_render(cli: RenderArgs) -> Result<()> {
    validate_output(&cli)?;
    let Some(input) = &cli.input else {
        Cli::command().print_help().ok();
        std::process::exit(2)
    };
    if !(0.25..=4.0).contains(&cli.speed) {
        bail!("--speed must be within 0.25-4.0 (got {})", cli.speed);
    }
    let input = fs::canonicalize(input)
        .with_context(|| format!("no such file: {}", input.display()))?
        .to_str()
        .context("input path must be UTF-8")?
        .to_string();

    if cli.dry_run {
        return dry_run(&input, &cli);
    }

    anyhow::ensure!(cli.threads != Some(0), "thread count must be positive");
    anyhow::ensure!(
        !cli.voice.is_empty()
            && cli
                .voice
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "voice must be an installed voice name, not a path"
    );
    let encoder = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .context("ffmpeg is required; run lector doctor before rendering")?;
    anyhow::ensure!(
        encoder.status.success(),
        "ffmpeg is not working; run lector doctor"
    );
    let voice = &cli.voice;
    let espeak_lang: String = match cli.lang.as_deref() {
        Some("a") => "en-us".into(),
        Some("b") => "en-gb".into(),
        Some(other) => other.into(),
        None => infer_lang(voice).into(),
    };
    let voice_bin = asset(&format!("{voice}.bin"));
    if !voice_bin.exists() {
        let have = installed_voices();
        if have.is_empty() {
            bail!("voice '{voice}' not found and no voices installed (run: just vendor)");
        }
        bail!("voice '{voice}' not found; installed: {}", have.join(", "));
    }
    let threads = cli.threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8)
    });

    let fname = Path::new(&input)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "book".into());
    let mut slug = {
        let s = slugify(&fname);
        if s.is_empty() {
            "book".into()
        } else {
            s
        }
    };
    if cli.preview_words.is_some() {
        slug.push_str("-preview");
    }
    let out_dir = &cli.out;
    fs::create_dir_all(out_dir)?;
    let _lock = checkpoint::lock(&out_dir.join(format!(".{slug}.lock")))?;
    let ckpt = out_dir.join(format!(".{slug}.ckpt"));
    fs::create_dir_all(&ckpt)?;
    let pcm_path = ckpt.join("audio.s16");
    let timed_path = ckpt.join("timed.jsonl");
    let plan_path = ckpt.join("plan.json");

    println!(
        "{fname} -> {}  [voice {voice} ({espeak_lang}), speed {}x, {} threads]",
        out_dir.join(format!("{slug}.m4b")).display(),
        cli.speed,
        threads
    );

    let mut synth = synth::Synth::new(
        &asset("kokoro_dur.onnx"),
        &asset("tokenizer.json"),
        &voice_bin,
        &espeak_lang,
        threads,
    )
    .context("init synth (run vendor or check assets/)")?;

    if let Some(path) = &cli.pronunciations {
        synth.set_pronunciations(path)?;
    }

    // Content fingerprints prevent changed inputs/assets from mixing with cached audio.
    let mut identity = vec!["lector-plan-v3".to_string()];
    for path in [
        PathBuf::from(&input),
        asset("kokoro_dur.onnx"),
        asset("tokenizer.json"),
        voice_bin.clone(),
    ] {
        identity.push(checkpoint::digest(&path)?);
    }
    if let Some(path) = &cli.pronunciations {
        identity.push(checkpoint::digest(path)?);
    }
    let espeak_version = Command::new("espeak-ng").arg("--version").output()?;
    anyhow::ensure!(
        espeak_version.status.success(),
        "cannot determine espeak-ng version"
    );
    identity.push(format!(
        "{:?}/{:?}/{:?}",
        cli.chapters, cli.exclude_chapters, cli.preview_words
    ));
    identity.push(String::from_utf8_lossy(&espeak_version.stdout).into_owned());

    // --- plan: load checkpoint or compute ---
    let plan: Plan = if plan_path.exists() {
        let p: Plan = serde_json::from_str(&fs::read_to_string(&plan_path)?)?;
        anyhow::ensure!(p.identity == identity, "checkpoint {} belongs to different input/assets or an older version; use a different output directory or move the checkpoint aside", ckpt.display());
        if (&p.voice, &p.lang) != (voice, &espeak_lang) {
            bail!(
                "checkpoint {} was rendered with voice {} ({}); re-run with those \
                 or delete the checkpoint to start over",
                ckpt.display(),
                p.voice,
                p.lang
            );
        }
        println!(
            "resuming: {} units, {} chapters (checkpoint)",
            p.units.len(),
            p.chapters.len()
        );
        p
    } else {
        let mut p = compute_plan(&input, &synth, voice, &espeak_lang, &cli)?;
        p.identity = identity;
        checkpoint::atomic_write(&plan_path, &serde_json::to_vec(&p)?)?;
        p
    };
    let units = &plan.units;

    // --- resume: replay committed units, make audio.s16 consistent ---
    let records = checkpoint::recover(&ckpt, units.len())?;
    let done = records.len();
    let mut total_samples: usize = records.iter().map(|r| r.samples).sum();
    drop(records);
    if done > 0 {
        println!("  {done}/{} units already done; continuing", units.len());
    }

    // --- synth remaining units; commit PCM then timings (crash-safe order) ---
    let mut pcm = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&pcm_path)?,
    );
    let mut tlog = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&timed_path)?;
    checkpoint::sync_dir(&ckpt)?;
    let t0 = Instant::now();
    let start_samples = total_samples;
    for i in done..units.len() {
        let u = synth.synth(&units[i])?;
        let timed = timing::align_words(&u.words, &u.word_phonemes, &u.phonemes, &u.dur);
        pcm.write_all(&f32_to_s16(&u.wav))?;
        pcm.flush()?;
        pcm.get_ref().sync_data()?;
        let rec = Rec {
            samples: u.wav.len(),
            timed,
        };
        writeln!(tlog, "{}", serde_json::to_string(&rec)?)?;
        tlog.sync_data()?;
        total_samples += u.wav.len();
        if i.is_multiple_of(10) || i == units.len() - 1 {
            let audio_min = total_samples as f64 / SR as f64 / 60.0;
            let run_audio = (total_samples - start_samples) as f64 / SR as f64;
            let elapsed = t0.elapsed().as_secs_f64();
            let rtf = if run_audio > 0.0 {
                elapsed / run_audio
            } else {
                0.0
            };
            let done_run = (i + 1 - done) as f64;
            let eta = elapsed / done_run * (units.len() - i - 1) as f64;
            print!(
                "\r  unit {}/{}  audio {audio_min:.0}min  RTF {rtf:.2}x  ETA {}   ",
                i + 1,
                units.len(),
                fmt_dur(eta)
            );
            std::io::stdout().flush().ok();
        }
    }
    pcm.flush()?;
    drop(pcm);
    println!();

    assemble(&plan, &ckpt, &slug, &cli, Some(&input))?;
    if cli.discard_checkpoint {
        fs::remove_dir_all(&ckpt)?;
    } else {
        println!("checkpoint kept: {}", ckpt.display());
    }
    Ok(())
}

fn reencode(ckpt: &Path, cli: &OutputArgs) -> Result<()> {
    validate_output(cli)?;
    let ckpt = fs::canonicalize(ckpt)?;
    let name = ckpt
        .file_name()
        .and_then(|s| s.to_str())
        .context("invalid checkpoint name")?;
    let slug = name
        .strip_prefix('.')
        .and_then(|s| s.strip_suffix(".ckpt"))
        .context("expected .<book>.ckpt directory")?;
    let parent = ckpt.parent().context("checkpoint has no parent")?;
    let _source_lock = checkpoint::lock(&parent.join(format!(".{slug}.lock")))?;
    fs::create_dir_all(&cli.out)?;
    let _dest_lock = if fs::canonicalize(&cli.out)? != parent {
        Some(checkpoint::lock(&cli.out.join(format!(".{slug}.lock")))?)
    } else {
        None
    };
    let plan: Plan = serde_json::from_slice(&fs::read(ckpt.join("plan.json"))?)?;
    assemble(&plan, &ckpt, slug, cli, None)
}

fn assemble(
    plan: &Plan,
    ckpt: &Path,
    slug: &str,
    cli: &OutputArgs,
    input: Option<&str>,
) -> Result<()> {
    let units = &plan.units;
    anyhow::ensure!(
        plan.chapters.iter().all(|(index, _)| *index < units.len())
            && plan.chapters.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "checkpoint has invalid chapter boundaries"
    );
    let records = checkpoint::recover(ckpt, units.len())?;
    anyhow::ensure!(
        !units.is_empty() && records.len() == units.len(),
        "checkpoint is incomplete; resume the original render first"
    );
    let per_unit: Vec<_> = records.iter().map(|r| r.timed.clone()).collect();
    let durations: Vec<_> = records
        .iter()
        .map(|r| r.samples as f64 / SR as f64)
        .collect();
    let pcm_path = ckpt.join("audio.s16");
    let out_dir = &cli.out;
    // --- assemble: chapter-tagged .m4b (speed applied here) + read-along .ass ---
    // cumulative start time (seconds) per unit -> chapter timestamps
    let mut cum = vec![0f64; units.len() + 1];
    for i in 0..units.len() {
        cum[i + 1] = cum[i] + durations[i];
    }
    let total = *cum.last().unwrap_or(&0.0);
    // output timeline is shrunk/stretched by atempo; keep chapters + subs in sync
    let out_cum: Vec<f64> = cum.iter().map(|t| t / cli.speed).collect();
    let out_total = total / cli.speed;
    let meta_path = ckpt.join("meta.ff");
    fs::write(&meta_path, build_ffmeta(plan, &out_cum, out_total))?;
    let cover = if cli.no_cover {
        None
    } else {
        input
            .and_then(|input| source::write_cover(input, ckpt))
            .or_else(|| {
                ["cover.png", "cover.jpg"]
                    .into_iter()
                    .map(|name| ckpt.join(name))
                    .find(|p| p.exists())
            })
    };

    // encode raw PCM + metadata (+ cover) -> m4b (ipod muxer: AAC + chapters + nav)
    let m4b_path = out_dir.join(format!("{slug}.m4b"));
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-f", "s16le", "-ar", "24000", "-ac", "1", "-i"])
        .arg(&pcm_path)
        .args(["-f", "ffmetadata", "-i"])
        .arg(&meta_path);
    if let Some(c) = &cover {
        cmd.arg("-i").arg(c);
    }
    cmd.args(["-map", "0:a"]);
    if cover.is_some() {
        cmd.args(["-map", "2:v"]);
    }
    if cli.speed != 1.0 {
        cmd.args(["-af", &atempo_chain(cli.speed)]);
    }
    cmd.args(["-map_metadata", "1", "-c:a", "aac", "-b:a"])
        .arg(format!("{}k", cli.bitrate));
    if cover.is_some() {
        // cover as attached picture (album art), shown by mpv / audiobook players
        cmd.args(["-c:v", "mjpeg", "-disposition:v", "attached_pic"]);
    }
    // Stage output in the destination filesystem; ffmpeg never touches a prior render.
    let staged_audio = tempfile::NamedTempFile::new_in(out_dir)?;
    let ff = cmd
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "ipod",
        ])
        .arg(staged_audio.path())
        .output()
        .context("run ffmpeg; audio/timings were preserved in the checkpoint")?;
    anyhow::ensure!(
        ff.status.success(),
        "ffmpeg failed; checkpoint preserved at {}: {}",
        ckpt.display(),
        String::from_utf8_lossy(&ff.stderr).trim()
    );
    staged_audio.as_file().sync_all()?;
    let out_speed = cli.speed;

    let mut merged = timing::merge_timed(&per_unit, &durations);
    for t in &mut merged {
        t.start_s /= out_speed;
        t.end_s /= out_speed;
    }
    if cli.export_timings {
        checkpoint::atomic_write(
            &out_dir.join(format!("{slug}.json")),
            &serde_json::to_vec_pretty(&merged)?,
        )?;
        checkpoint::atomic_write(
            &out_dir.join(format!("{slug}.srt")),
            timing::to_srt(&merged, cli.line_words).as_bytes(),
        )?;
    }
    let ass_path = out_dir.join(format!("{slug}.ass"));
    let subtitles = ass::to_ass(&merged, cli.line_words, &cli.sub_font, cli.sub_size);
    // Each artifact is atomically replaced. Checkpoint survives any publication error.
    checkpoint::atomic_write(&ass_path, subtitles.as_bytes())?;
    staged_audio.persist(&m4b_path).map_err(|e| e.error)?;
    checkpoint::sync_dir(out_dir)?;
    let audio_out = m4b_path;

    println!(
        "done: {} + {} ({}, {} words, {} chapters)",
        audio_out.display(),
        ass_path.display(),
        fmt_dur(total / out_speed),
        merged.len(),
        plan.chapters.len()
    );
    println!(
        "play: mpv --sub-file={} {}",
        ass_path.display(),
        audio_out.display()
    );
    Ok(())
}

/// FFMETADATA1 doc with global title/artist + one [CHAPTER] per heading.
fn build_ffmeta(plan: &Plan, cum: &[f64], total: f64) -> String {
    let mut m = String::from(";FFMETADATA1\n");
    if let Some(t) = &plan.title {
        m.push_str(&format!("title={}\n", ffmeta_esc(t)));
    }
    if let Some(a) = &plan.author {
        m.push_str(&format!("artist={}\n", ffmeta_esc(a)));
    }
    let ms = |s: f64| (s * 1000.0) as i64;
    for (i, (uidx, ctitle)) in plan.chapters.iter().enumerate() {
        let start = ms(cum.get(*uidx).copied().unwrap_or(0.0));
        let end = if i + 1 < plan.chapters.len() {
            ms(cum.get(plan.chapters[i + 1].0).copied().unwrap_or(total))
        } else {
            ms(total)
        };
        if end <= start {
            continue; // skip empty chapters (mp4 rejects zero/negative spans)
        }
        m.push_str("[CHAPTER]\nTIMEBASE=1/1000\n");
        m.push_str(&format!(
            "START={start}\nEND={end}\ntitle={}\n",
            ffmeta_esc(ctitle)
        ));
    }
    m
}

fn chapter_set(spec: &str, count: usize) -> Result<std::collections::BTreeSet<usize>> {
    let mut set = std::collections::BTreeSet::new();
    for item in spec.split(',') {
        let item = item.trim();
        let (a, b) = item.split_once('-').unwrap_or((item, item));
        let a: usize = a
            .parse()
            .context("chapter selection must use numbers or ranges, e.g. 2-5,8")?;
        let b: usize = b.parse().context("invalid chapter range")?;
        anyhow::ensure!(
            a > 0 && a <= b && b <= count,
            "chapter range {item} is outside 1-{count}"
        );
        set.extend(a..=b);
    }
    Ok(set)
}

fn select_chapters(
    chapters: Vec<(String, Vec<String>)>,
    include: Option<&str>,
    exclude: Option<&str>,
) -> Result<Vec<(String, Vec<String>)>> {
    let count = chapters.len();
    let keep = include.map(|s| chapter_set(s, count)).transpose()?;
    let omit = exclude
        .map(|s| chapter_set(s, count))
        .transpose()?
        .unwrap_or_default();
    let result: Vec<_> = chapters
        .into_iter()
        .enumerate()
        .filter(|(i, _)| {
            let n = i + 1;
            keep.as_ref().is_none_or(|set| set.contains(&n)) && !omit.contains(&n)
        })
        .map(|(_, chapter)| chapter)
        .collect();
    anyhow::ensure!(!result.is_empty(), "no readable chapters selected");
    Ok(result)
}

fn limit_preview(
    chapters: Vec<(String, Vec<String>)>,
    limit: Option<usize>,
) -> Vec<(String, Vec<String>)> {
    let Some(mut remaining) = limit else {
        return chapters;
    };
    let mut out = vec![];
    for (title, paras) in chapters {
        let mut chosen = vec![];
        for p in paras {
            if remaining == 0 {
                break;
            }
            let words: Vec<_> = p.split_whitespace().take(remaining).collect();
            remaining -= words.len();
            chosen.push(words.join(" "));
        }
        if !chosen.is_empty() {
            out.push((title, chosen));
        }
        if remaining == 0 {
            break;
        }
    }
    out
}

fn doctor(voice: &str) -> Result<()> {
    let mut errors = vec![];
    println!("assets: {}", asset("").display());
    for (tool, arg, required) in [
        ("espeak-ng", "--version", true),
        ("ffmpeg", "-version", true),
        ("pdftotext", "-v", false),
        ("pdfinfo", "-v", false),
        ("pdftoppm", "-v", false),
        ("ebook-convert", "--version", false),
    ] {
        match Command::new(tool).arg(arg).output() {
            Ok(out) if out.status.success() => println!("ok: {tool}"),
            _ if required => errors.push(format!("missing or broken {tool}")),
            _ => println!("optional: {tool} unavailable (PDF/Calibre support limited)"),
        }
    }
    anyhow::ensure!(
        voice.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !voice.is_empty(),
        "invalid voice name"
    );
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../scripts/assets.json"))?;
    for entry in manifest["files"]
        .as_array()
        .context("invalid asset manifest")?
    {
        let path = entry["path"].as_str().context("invalid asset path")?;
        let name = Path::new(path).file_name().unwrap().to_str().unwrap();
        if name != "tokenizer.json" && name != format!("{voice}.bin") {
            continue;
        }
        match checkpoint::digest(&asset(name)) {
            Ok(sum) if Some(sum.as_str()) == entry["sha256"].as_str() => {
                println!("ok: {name} checksum")
            }
            _ => errors.push(format!(
                "{name} missing or differs from pinned asset; run scripts/vendor.py"
            )),
        }
    }
    let model_metadata = asset("kokoro_dur.json");
    if model_metadata.exists() {
        let metadata: serde_json::Value = serde_json::from_slice(&fs::read(model_metadata)?)?;
        match checkpoint::digest(&asset("kokoro_dur.onnx")) {
            Ok(sum) if Some(sum.as_str()) == metadata["sha256"].as_str() => {
                println!("ok: model checksum")
            }
            _ => errors.push("model checksum differs from export metadata".into()),
        }
    } else {
        println!("note: model has no export checksum sidecar; checking inference only");
    }
    if errors.is_empty() {
        match synth::Synth::new(
            &asset("kokoro_dur.onnx"),
            &asset("tokenizer.json"),
            &asset(&format!("{voice}.bin")),
            infer_lang(voice),
            1,
        )
        .and_then(|mut synth| synth.synth("Ready."))
        {
            Ok(unit) if !unit.wav.is_empty() && unit.wav.iter().all(|x| x.is_finite()) => {
                println!("ok: model inference and duration output")
            }
            Ok(_) => errors.push("model returned invalid audio".into()),
            Err(e) => errors.push(format!("model: {e:#}")),
        }
    }
    anyhow::ensure!(
        errors.is_empty(),
        "doctor found problems:\n{}",
        errors.join("\n")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chapter_ranges_and_preview_preserve_order() {
        let chapters = vec![
            ("one".into(), vec!["one two three".into()]),
            ("two".into(), vec!["four five six".into()]),
            ("three".into(), vec!["seven".into()]),
        ];
        let selected = select_chapters(chapters, Some("1-3"), Some("2")).unwrap();
        let preview = limit_preview(selected, Some(4));
        assert_eq!(preview[0].1, ["one two three"]);
        assert_eq!(preview[1].1, ["seven"]);
        assert!(chapter_set("0", 3).is_err());
        assert!(chapter_set("3-1", 3).is_err());
        assert!(chapter_set("1-4", 3).is_err());
    }
    #[test]
    fn cli_structure_and_reencode_options() {
        Cli::command().debug_assert();
        assert!(Cli::try_parse_from(["lector", "--assets", "custom", "doctor"]).is_ok());
        assert!(Cli::try_parse_from(["lector", "preview", "book.epub", "--words", "0"]).is_err());
        assert!(
            Cli::try_parse_from(["lector", "reencode", ".book.ckpt", "--voice", "af_heart"])
                .is_err()
        );
    }
    #[test]
    fn tempo_stages_cover_extremes() {
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.5");
        assert_eq!(atempo_chain(4.0), "atempo=2.0,atempo=2");
    }
}
