//! Kokoro synthesis: espeak-ng phonemes -> tokenize -> onnxruntime inference.
//! Returns the waveform (f32 @ 24kHz) and per-phoneme pred_dur frames.

use anyhow::{anyhow, Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;

// ort's error type isn't Send+Sync, so it can't go through `?` into anyhow.
fn oe<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow!("ort: {e}")
}

pub struct Synth {
    session: Session,
    vocab: HashMap<String, i64>,
    voice: Vec<f32>, // 510 * 256 style packs
    lang: String,    // espeak voice, e.g. "en-gb"
    word_cache: HashMap<String, String>,
}

pub fn phonemize(text: &str, lang: &str) -> String {
    let out = Command::new("espeak-ng")
        .args(["-q", "--ipa", "-v", lang, text])
        .output()
        .expect("espeak-ng failed to run");
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl Synth {
    pub fn new(
        model: &str,
        tokenizer: &str,
        voice_bin: &str,
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
        let bytes = std::fs::read(voice_bin)?;
        let voice: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let session = Session::builder()
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
        })
    }

    pub fn phonemes_token_len(&self, phon: &str) -> usize {
        phon.chars()
            .filter(|c| self.vocab.contains_key(&c.to_string()))
            .count()
    }

    pub fn phonemize(&self, text: &str) -> String {
        phonemize(text, &self.lang)
    }

    /// Per-word phonemes (the alignment guide), memoized — books repeat words.
    fn word_phonemes(&mut self, words: &[String]) -> Vec<String> {
        let mut out = Vec::with_capacity(words.len());
        for w in words {
            if let Some(p) = self.word_cache.get(w) {
                out.push(p.clone());
            } else {
                let p = phonemize(w, &self.lang);
                self.word_cache.insert(w.clone(), p.clone());
                out.push(p);
            }
        }
        out
    }

    /// Synthesize one unit. Returns (waveform f32@24k, pred_dur, phonemes, words,
    /// word_phonemes) so the caller can run alignment.
    pub fn synth(
        &mut self,
        text: &str,
    ) -> Result<(Vec<f32>, Vec<i64>, String, Vec<String>, Vec<String>)> {
        let phonemes = phonemize(text, &self.lang);
        let words: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
        let word_phonemes = self.word_phonemes(&words);

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
        let (_, wav) = outputs["waveform"]
            .try_extract_tensor::<f32>()
            .map_err(oe)?;
        let (_, dur) = outputs["duration"]
            .try_extract_tensor::<i64>()
            .map_err(oe)?;
        Ok((wav.to_vec(), dur.to_vec(), phonemes, words, word_phonemes))
    }
}
