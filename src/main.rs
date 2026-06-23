//! lector — render a book into a chaptered read-along audiobook (.m4b + .ass).
//!
//!   lector <book.pdf|.epub|.mobi|...> [outDir=.] [voice=bm_george] [lang=a|b]
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
const THREADS: usize = 8;

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

fn list_voices() {
    let dir = asset("");
    let mut v: Vec<String> = vec![];
    if let Ok(rd) = fs::read_dir(&dir) {
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
    if v.is_empty() {
        println!("no voices in {} (run: just vendor)", dir.display());
    } else {
        println!("voices in {}:", dir.display());
        for n in v {
            println!("  {n}");
        }
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

/// load -> chapters (split at headings) -> sentence_pack -> token-cap split.
/// Headings are narrated and also mark chapter starts (unit index -> title).
fn compute_plan(input: &str, synth: &synth::Synth) -> Result<Plan> {
    let doc = source::load(input)?;
    let title = doc.title.clone();
    let author = doc.author.clone();

    let mut chapters: Vec<(String, Vec<String>)> = vec![];
    let mut cur_title = String::new();
    let mut cur: Vec<String> = vec![];
    for b in doc.blocks {
        match b {
            // only major headings (h1/h2) break chapters; minor ones just narrate
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
    })
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
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: lector <book.pdf|.epub|.mobi|...> [outDir=.] [voice=bm_george] [lang=a|b]"
        );
        std::process::exit(1);
    }
    if args[1] == "voices" {
        list_voices();
        return Ok(());
    }
    let input = fs::canonicalize(&args[1])?.to_string_lossy().into_owned();
    let out_dir = args.get(2).cloned().unwrap_or_else(|| ".".into());
    let voice = args.get(3).cloned().unwrap_or_else(|| "bm_george".into());
    // espeak accent: explicit arg wins, else inferred from the voice prefix
    // (a* = American voices -> en-us, b* = British -> en-gb)
    let espeak_lang = match args.get(4).map(|s| s.as_str()) {
        Some("a") => "en-us",
        Some("b") => "en-gb",
        _ if voice.starts_with('a') => "en-us",
        _ => "en-gb",
    };

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
    fs::create_dir_all(&out_dir)?;
    let ckpt = Path::new(&out_dir).join(format!(".{slug}.ckpt"));
    fs::create_dir_all(&ckpt)?;
    let pcm_path = ckpt.join("audio.s16");
    let timed_path = ckpt.join("timed.jsonl");
    let plan_path = ckpt.join("plan.json");

    let mut synth = synth::Synth::new(
        asset("kokoro_dur.onnx").to_str().unwrap(),
        asset("tokenizer.json").to_str().unwrap(),
        asset(&format!("{voice}.bin")).to_str().unwrap(),
        espeak_lang,
        THREADS,
    )
    .context("init synth (run vendor or check assets/)")?;

    // --- plan: load checkpoint or compute ---
    let plan: Plan = if plan_path.exists() {
        let p: Plan = serde_json::from_str(&fs::read_to_string(&plan_path)?)?;
        println!(
            "resuming: {} units, {} chapters (checkpoint)",
            p.units.len(),
            p.chapters.len()
        );
        p
    } else {
        let p = compute_plan(&input, &synth)?;
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
        let (wav, dur, phonemes, words, word_phonemes) = synth.synth(&units[i])?;
        let timed = timing::align_words(&words, &word_phonemes, &phonemes, &dur);
        pcm.write_all(&f32_to_s16(&wav))?;
        pcm.flush()?;
        let rec = Rec {
            samples: wav.len(),
            timed: timed.clone(),
        };
        writeln!(tlog, "{}", serde_json::to_string(&rec)?)?;
        per_unit.push(timed);
        durations.push(wav.len() as f64 / SR as f64);
        total_samples += wav.len();
        if i.is_multiple_of(10) || i == units.len() - 1 {
            let audio_min = total_samples as f64 / SR as f64 / 60.0;
            let run_audio = (total_samples - start_samples) as f64 / SR as f64;
            let rtf = if run_audio > 0.0 {
                t0.elapsed().as_secs_f64() / run_audio
            } else {
                0.0
            };
            print!(
                "\r  unit {}/{}  audio {audio_min:.0}min  RTF {rtf:.2}x   ",
                i + 1,
                units.len()
            );
            std::io::stdout().flush().ok();
        }
    }
    pcm.flush()?;
    drop(pcm);
    println!();

    // --- assemble: read-along .ass + chapter-tagged .m4b ---
    let merged = timing::merge_timed(&per_unit, &durations);
    let ass_path = Path::new(&out_dir).join(format!("{slug}.ass"));
    fs::write(&ass_path, ass::to_ass(&merged, 12))?;

    // cumulative start time (seconds) per unit -> chapter timestamps
    let mut cum = vec![0f64; units.len() + 1];
    for i in 0..units.len() {
        cum[i + 1] = cum[i] + durations[i];
    }
    let total = *cum.last().unwrap_or(&0.0);
    let meta_path = ckpt.join("meta.ff");
    fs::write(&meta_path, build_ffmeta(&plan, &cum, total))?;
    let cover = source::write_cover(&input, &ckpt);

    // encode raw PCM + metadata (+ cover) -> m4b (ipod muxer: AAC + chapters + nav)
    let m4b_path = Path::new(&out_dir).join(format!("{slug}.m4b"));
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
    cmd.args(["-map_metadata", "1", "-c:a", "aac", "-b:a", "64k"]);
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
    let audio_out = match ff {
        Ok(s) if s.success() => m4b_path,
        _ => {
            let wav_path = Path::new(&out_dir).join(format!("{slug}.wav"));
            write_wav(&wav_path, &pcm_path, total_samples)?;
            eprintln!("(ffmpeg unavailable or failed; wrote .wav without chapters)");
            wav_path
        }
    };

    fs::remove_dir_all(&ckpt).ok();
    let mins = total / 60.0;
    println!(
        "done: {} + {} ({mins:.0} min, {} words, {} chapters)",
        audio_out.display(),
        ass_path.display(),
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
