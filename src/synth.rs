//! Kokoro synthesis: espeak-ng phonemes -> tokenize -> onnxruntime inference.
//! Returns the waveform (f32 @ 24kHz) and per-phoneme pred_dur frames.

use anyhow::{anyhow, Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

// ort's error type isn't Send+Sync, so it can't go through `?` into anyhow.
fn oe<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow!("ort: {e}")
}

/// One synthesized unit: the waveform plus everything alignment needs.
pub struct Unit {
    pub wav: Vec<f32>, // f32 @ 24kHz
    pub dur: Vec<i64>, // per-phoneme pred_dur frames
    pub phonemes: String,
    pub words: Vec<String>,
    pub word_phonemes: Vec<String>,
}

pub struct Synth {
    session: Session,
    vocab: HashMap<String, i64>,
    voice: Vec<f32>, // 510 * 256 style packs
    lang: String,    // espeak voice, e.g. "en-gb"
    word_cache: HashMap<String, String>,
    pronunciations: HashMap<String, String>,
}

pub fn phonemize(text: &str, lang: &str) -> Result<String> {
    // Text must never be interpreted as espeak options. stdin also avoids ARG_MAX.
    let mut child = Command::new("espeak-ng")
        .args(["-q", "--ipa", "-v", lang, "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run espeak-ng (is it installed?)")?;
    let mut stdin = child.stdin.take().context("open espeak stdin")?;
    // Drain output concurrently with writing: large passages can fill both pipes.
    let text = text.to_owned();
    let writer = std::thread::spawn(move || stdin.write_all(text.as_bytes()));
    let out = child.wait_with_output()?;
    let write_result = writer
        .join()
        .map_err(|_| anyhow!("espeak stdin writer failed"))?;
    anyhow::ensure!(
        out.status.success(),
        "espeak-ng failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    write_result.context("write text to espeak-ng")?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "))
}

impl Synth {
    pub fn new(
        model: &std::path::Path,
        tokenizer: &std::path::Path,
        voice_bin: &std::path::Path,
        lang: &str,
        threads: usize,
    ) -> Result<Self> {
        let vj: Value = serde_json::from_str(&std::fs::read_to_string(tokenizer)?)
            .context("parse tokenizer.json")?;
        let mut vocab = HashMap::new();
        if let Value::Object(m) = &vj["model"]["vocab"] {
            for (k, v) in m {
                if let Some(id) = v.as_i64() {
                    vocab.insert(k.clone(), id);
                }
            }
        }
        anyhow::ensure!(!vocab.is_empty(), "tokenizer has no vocabulary");
        let bytes = std::fs::read(voice_bin)?;
        anyhow::ensure!(
            bytes.len() == 510 * 256 * 4,
            "voice must contain 510 × 256 float32 values (got {} bytes)",
            bytes.len()
        );
        let voice: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect();
        anyhow::ensure!(
            voice.iter().all(|v| v.is_finite()),
            "voice contains non-finite values"
        );
        let environment = ort::init().with_name("lector").build().map_err(oe)?;
        let session = Session::builder(&environment)
            .map_err(oe)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(oe)?
            .with_intra_threads(threads)
            .map_err(oe)?
            .commit_from_file(model)
            .map_err(oe)?;
        Ok(Self {
            session,
            vocab,
            voice,
            lang: lang.to_string(),
            word_cache: HashMap::new(),
            pronunciations: HashMap::new(),
        })
    }

    pub fn phonemes_token_len(&self, phon: &str) -> usize {
        phon.chars()
            .filter(|c| self.vocab.contains_key(&c.to_string()))
            .count()
    }

    pub fn set_pronunciations(&mut self, path: &std::path::Path) -> Result<()> {
        let map: HashMap<String, String> = serde_json::from_slice(&std::fs::read(path)?)
            .context("pronunciations must be a JSON object of written word -> spoken text")?;
        for (key, value) in &map {
            anyhow::ensure!(
                !key.is_empty()
                    && !key.chars().any(char::is_whitespace)
                    && !value.trim().is_empty(),
                "pronunciation keys must be single words and values must be nonempty"
            );
        }
        self.pronunciations = map;
        self.word_cache.clear();
        Ok(())
    }

    pub fn phonemize(&self, text: &str) -> Result<String> {
        let spoken = replace_words(text, &self.pronunciations);
        let phonemes = phonemize(&spoken, &self.lang)?;
        // Alignment must index exactly the same sequence as the model tokenizer.
        Ok(phonemes
            .chars()
            .filter(|c| self.vocab.contains_key(&c.to_string()))
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "))
    }

    /// Per-word phonemes (the alignment guide), memoized — books repeat words.
    fn word_phonemes(&mut self, words: &[String]) -> Result<Vec<String>> {
        let mut out = Vec::with_capacity(words.len());
        for w in words {
            if let Some(p) = self.word_cache.get(w) {
                out.push(p.clone());
            } else {
                let p = self.phonemize(w)?;
                self.word_cache.insert(w.clone(), p.clone());
                out.push(p);
            }
        }
        Ok(out)
    }

    /// Synthesize one unit; everything the caller needs to run alignment.
    pub fn synth(&mut self, text: &str) -> Result<Unit> {
        let phonemes = self.phonemize(text)?;
        let words: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
        let word_phonemes = self.word_phonemes(&words)?;

        let mut ids: Vec<i64> = vec![0];
        for ch in phonemes.chars() {
            if let Some(&id) = self.vocab.get(&ch.to_string()) {
                ids.push(id);
            }
        }
        ids.push(0);
        let n = ids.len();
        if n <= 2 {
            return Err(anyhow!("empty phonemes for unit"));
        }
        anyhow::ensure!(n <= 512, "unit exceeds model token limit: {} tokens", n);
        let off = 256 * (n.saturating_sub(2)).min(509);
        let style: Vec<f32> = self.voice[off..off + 256].to_vec();

        let ids_t = Tensor::from_array(([1usize, n], ids)).map_err(oe)?;
        let style_t = Tensor::from_array(([1usize, 256usize], style)).map_err(oe)?;
        let speed_t = Tensor::from_array(([1usize], vec![1i32])).map_err(oe)?;
        let outputs = self
            .session
            .run(ort::inputs! {
                "input_ids" => ids_t,
                "style" => style_t,
                "speed" => speed_t,
            })
            .map_err(oe)?;
        anyhow::ensure!(
            outputs.contains_key("waveform") && outputs.contains_key("duration"),
            "model must expose waveform and duration outputs; use scripts/export_dur.py"
        );
        let (_, wav) = outputs["waveform"]
            .try_extract_tensor::<f32>()
            .map_err(oe)?;
        let (_, dur) = outputs["duration"]
            .try_extract_tensor::<i64>()
            .map_err(oe)?;
        anyhow::ensure!(
            !wav.is_empty()
                && wav.iter().all(|x| x.is_finite())
                && dur.len() == n
                && dur.iter().all(|d| *d > 0),
            "model returned invalid audio/durations"
        );
        Ok(Unit {
            wav: wav.to_vec(),
            dur: dur.to_vec(),
            phonemes,
            words,
            word_phonemes,
        })
    }
}

fn replace_words(text: &str, map: &HashMap<String, String>) -> String {
    text.split_whitespace()
        .map(|word| {
            if let Some(value) = map.get(word) {
                return value.clone();
            }
            let core = word.trim_matches(|c: char| !c.is_alphanumeric());
            if let Some(value) = map.get(core) {
                let start = word.find(core).unwrap_or(0);
                format!("{}{}{}", &word[..start], value, &word[start + core.len()..])
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pronunciations_preserve_punctuation_and_case() {
        let map = HashMap::from([
            ("Ada".into(), "Ay dah".into()),
            ("API".into(), "A P I".into()),
        ]);
        assert_eq!(
            replace_words("Ada, uses (API). ada", &map),
            "Ay dah, uses (A P I). ada"
        );
    }
    #[test]
    #[ignore = "requires espeak-ng; CI runs ignored tool tests separately"]
    fn option_shaped_text_cannot_write_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("must-not-exist");
        let text = format!("--phonout={}", target.display());
        let phonemes = phonemize(&text, "en-gb").unwrap();
        assert!(!phonemes.is_empty());
        assert!(!target.exists());
    }
    #[test]
    #[ignore = "requires espeak-ng; CI runs ignored tool tests separately"]
    fn invalid_language_is_an_error() {
        assert!(phonemize("hello", "lector-nonexistent-language").is_err());
    }
}
