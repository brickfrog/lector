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
mod chunk;
mod clean;
mod epub;
mod pdf;
mod source;
mod synth;
mod timing;

use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use serde::{Deserialize, Serialize};
use source::Block;
use std::fs::{self, File, OpenOptions};
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
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Book to render (.pdf, .epub, .mobi, .azw3, .fb2, ...)
    input: Option<PathBuf>,

    /// Output directory
    #[arg(short, long, default_value = ".", value_name = "DIR")]
    out: PathBuf,

    /// Kokoro voice (see `lector voices`)
    #[arg(short, long, default_value = "bm_george", value_name = "NAME")]
    voice: String,

    /// espeak accent: "a" (en-us), "b" (en-gb), or any espeak voice id.
    /// Default: inferred from the voice prefix
    #[arg(short, long, value_name = "LANG")]
    lang: Option<String>,

    /// Speaking speed multiplier, 0.25-4.0 (pitch-preserving; subtitles and
    /// chapters stay in sync)
    #[arg(short, long, default_value_t = 1.0, value_name = "X")]
    speed: f64,

    /// AAC bitrate in kbit/s
    #[arg(short, long, default_value_t = 64, value_name = "KBPS")]
    bitrate: u32,

    /// onnxruntime threads [default: all cores]
    #[arg(short = 'j', long, value_name = "N")]
    threads: Option<usize>,

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

    /// Keep the checkpoint dir after a successful render
    #[arg(long)]
    keep_checkpoint: bool,

    /// Parse and plan only: print the chapter breakdown and exit
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// List installed voices
    Voices,
}

#[derive(Serialize, Deserialize)]
struct Rec {
    samples: usize,
    timed: Vec<Timed>,
}

/// Deterministic render plan, cached in the checkpoint so resume is exact.
#[derive(Serialize, Deserialize)]
struct Plan {
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

fn asset(name: &str) -> PathBuf {
    // assets live next to the executable's crate dir; resolve relative to CWD too
    let exe_rel = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(name);
    if exe_rel.exists() {
        exe_rel
    } else {
        Path::new("assets").join(name)
    }
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
fn token_split(text: &str, synth: &synth::Synth, out: &mut Vec<String>) {
    if synth.phonemes_token_len(&synth.phonemize(text)) <= MAX_TOKENS {
        out.push(text.to_string());
        return;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= 1 {
        out.push(text.to_string());
        return;
    }
    let mid = words.len().div_ceil(2);
    token_split(&words[..mid].join(" "), synth, out);
    token_split(&words[mid..].join(" "), synth, out);
}

/// Group blocks into chapters: h1/h2 break chapters (and are narrated),
/// minor headings just narrate.
fn collect_chapters(blocks: Vec<Block>) -> Vec<(String, Vec<String>)> {
    let mut chapters: Vec<(String, Vec<String>)> = vec![];
    let mut cur_title = String::new();
    let mut cur: Vec<String> = vec![];
    for b in blocks {
        match b {
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
fn compute_plan(input: &str, synth: &synth::Synth, voice: &str, lang: &str) -> Result<Plan> {
    let doc = source::load(input)?;
    let title = doc.title.clone();
    let author = doc.author.clone();
    let chapters = collect_chapters(doc.blocks);
    if chapters.is_empty() {
        bail!("no readable text found in {input}");
    }

    let mut units = vec![];
    let mut marks: Vec<(usize, String)> = vec![];
    for (ctitle, paras) in &chapters {
        let name = if ctitle.is_empty() {
            title.clone().unwrap_or_else(|| "Start".into())
        } else {
            ctitle.clone()
        };
        marks.push((units.len(), name));
        for c in chunk::sentence_pack(paras, 300) {
            token_split(&c, synth, &mut units);
        }
    }
    println!("{} chapters -> {} units", chapters.len(), units.len());
    Ok(Plan {
        units,
        chapters: marks,
        title,
        author,
        voice: voice.to_string(),
        lang: lang.to_string(),
    })
}

/// Print the chapter breakdown without synthesizing anything.
fn dry_run(input: &str, speed: f64) -> Result<()> {
    let doc = source::load(input)?;
    let chapters = collect_chapters(doc.blocks);
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
        out.push(if c == '\n' { ' ' } else { c });
    }
    out
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Cmd::Voices) = cli.command {
        list_voices();
        return Ok(());
    }
    let Some(input) = &cli.input else {
        Cli::command().print_help().ok();
        std::process::exit(2)
    };
    if !(0.25..=4.0).contains(&cli.speed) {
        bail!("--speed must be within 0.25-4.0 (got {})", cli.speed);
    }
    let input = fs::canonicalize(input)
        .with_context(|| format!("no such file: {}", input.display()))?
        .to_string_lossy()
        .into_owned();

    if cli.dry_run {
        return dry_run(&input, cli.speed);
    }

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
    let slug = {
        let s = slugify(&fname);
        if s.is_empty() {
            "book".into()
        } else {
            s
        }
    };
    let out_dir = &cli.out;
    fs::create_dir_all(out_dir)?;
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
        asset("kokoro_dur.onnx").to_str().unwrap(),
        asset("tokenizer.json").to_str().unwrap(),
        voice_bin.to_str().unwrap(),
        &espeak_lang,
        threads,
    )
    .context("init synth (run vendor or check assets/)")?;

    // --- plan: load checkpoint or compute ---
    let plan: Plan = if plan_path.exists() {
        let p: Plan = serde_json::from_str(&fs::read_to_string(&plan_path)?)?;
        if !p.voice.is_empty() && (&p.voice, &p.lang) != (voice, &espeak_lang) {
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
        let p = compute_plan(&input, &synth, voice, &espeak_lang)?;
        fs::write(&plan_path, serde_json::to_string(&p)?)?;
        p
    };
    let units = &plan.units;

    // --- resume: replay committed units, make audio.s16 consistent ---
    let mut per_unit: Vec<Vec<Timed>> = vec![];
    let mut durations: Vec<f64> = vec![];
    let mut total_samples: usize = 0;
    if timed_path.exists() {
        for line in fs::read_to_string(&timed_path)?.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let rec: Rec = serde_json::from_str(line)?;
            durations.push(rec.samples as f64 / SR as f64);
            total_samples += rec.samples;
            per_unit.push(rec.timed);
        }
    }
    let done = per_unit.len();
    let expected = (total_samples * 2) as u64;
    if pcm_path.exists() {
        let sz = fs::metadata(&pcm_path)?.len();
        if sz > expected {
            OpenOptions::new()
                .write(true)
                .open(&pcm_path)?
                .set_len(expected)?; // drop partial tail
        } else if sz < expected {
            bail!("checkpoint inconsistent (audio < timings); delete {ckpt:?} to restart");
        }
    }
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
    let t0 = Instant::now();
    let start_samples = total_samples;
    for i in done..units.len() {
        let u = synth.synth(&units[i])?;
        let timed = timing::align_words(&u.words, &u.word_phonemes, &u.phonemes, &u.dur);
        pcm.write_all(&f32_to_s16(&u.wav))?;
        pcm.flush()?;
        let rec = Rec {
            samples: u.wav.len(),
            timed: timed.clone(),
        };
        writeln!(tlog, "{}", serde_json::to_string(&rec)?)?;
        per_unit.push(timed);
        durations.push(u.wav.len() as f64 / SR as f64);
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
    fs::write(&meta_path, build_ffmeta(&plan, &out_cum, out_total))?;
    let cover = if cli.no_cover {
        None
    } else {
        source::write_cover(&input, &ckpt)
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
    let ff = cmd
        .args(["-f", "ipod"])
        .arg(&m4b_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let (audio_out, out_speed) = match ff {
        Ok(s) if s.success() => (m4b_path, cli.speed),
        _ => {
            // fallback wav is the raw 1.0x PCM, so subs must stay at 1.0x too
            let wav_path = out_dir.join(format!("{slug}.wav"));
            write_wav(&wav_path, &pcm_path, total_samples)?;
            eprintln!("(ffmpeg unavailable or failed; wrote .wav without chapters)");
            if cli.speed != 1.0 {
                eprintln!("(--speed needs ffmpeg; .wav is at 1.0x)");
            }
            (wav_path, 1.0)
        }
    };

    let mut merged = timing::merge_timed(&per_unit, &durations);
    for t in &mut merged {
        t.start_s /= out_speed;
        t.end_s /= out_speed;
    }
    let ass_path = out_dir.join(format!("{slug}.ass"));
    fs::write(
        &ass_path,
        ass::to_ass(&merged, cli.line_words, &cli.sub_font, cli.sub_size),
    )?;

    if cli.keep_checkpoint {
        println!("checkpoint kept: {}", ckpt.display());
    } else {
        fs::remove_dir_all(&ckpt).ok();
    }
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

fn write_wav(out: &Path, pcm: &Path, total_samples: usize) -> Result<()> {
    let data_len = (total_samples * 2) as u32;
    let mut f = BufWriter::new(File::create(out)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVE")?;
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&(SR as u32).to_le_bytes())?;
    f.write_all(&((SR * 2) as u32).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    let mut r = File::open(pcm)?;
    std::io::copy(&mut r, &mut f)?;
    Ok(())
}
