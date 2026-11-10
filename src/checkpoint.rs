//! Durable render records: audio is synced before its corresponding journal record.
use crate::{Rec, SR};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

pub fn digest(path: &Path) -> Result<String> {
    let mut input = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn sync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("output has no parent directory")?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(data)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    sync_dir(parent)
}

/// Keep this handle alive until publication and checkpoint cleanup have finished.
/// The lock file is deliberately retained: unlinking it creates an inode race.
pub fn lock(path: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    f.try_lock()
        .with_context(|| format!("another render holds {}", path.display()))?;
    Ok(f)
}

/// Accept only newline-terminated commits. A torn last record is discarded, while
/// corruption in a committed record is reported rather than silently losing work.
pub fn recover(dir: &Path, units: usize) -> Result<Vec<Rec>> {
    let log = dir.join("timed.jsonl");
    let pcm = dir.join("audio.s16");
    let mut records = Vec::new();
    let mut bytes = 0u64;
    let mut samples = 0u64;
    if log.exists() {
        let mut input = BufReader::new(File::open(&log)?);
        let mut line = Vec::new();
        loop {
            line.clear();
            if input.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if line.last() != Some(&b'\n') {
                break;
            }
            let record: Rec =
                serde_json::from_slice(&line).context("corrupt committed checkpoint record")?;
            if records.len() >= units {
                bail!("checkpoint has more records than planned units");
            }
            let duration = record.samples as f64 / SR as f64;
            anyhow::ensure!(record.samples > 0, "checkpoint contains empty audio");
            let mut last_start = 0.0;
            for t in &record.timed {
                anyhow::ensure!(
                    t.start_s.is_finite()
                        && t.end_s.is_finite()
                        && t.start_s >= last_start
                        && t.end_s >= t.start_s
                        && t.end_s <= duration + 0.1,
                    "checkpoint contains invalid word timing"
                );
                last_start = t.start_s;
            }
            samples = samples
                .checked_add(record.samples as u64)
                .context("checkpoint sample count overflow")?;
            bytes += line.len() as u64;
            records.push(record);
        }
    }
    let expected = samples
        .checked_mul(2)
        .context("checkpoint audio size overflow")?;
    let actual = if pcm.exists() {
        fs::metadata(&pcm)?.len()
    } else {
        0
    };
    anyhow::ensure!(actual >= expected, "checkpoint inconsistent: audio missing or shorter than committed timings; preserve it for recovery");
    if log.exists() {
        let f = OpenOptions::new().write(true).open(log)?;
        f.set_len(bytes)?;
        f.sync_all()?;
    }
    if actual > expected {
        let f = OpenOptions::new().write(true).open(pcm)?;
        f.set_len(expected)?;
        f.sync_all()?;
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> &'static [u8] {
        b"{\"samples\":2,\"timed\":[]}\n"
    }
    #[test]
    fn torn_tail_rolls_back_audio_and_log() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("audio.s16"), [0u8; 10]).unwrap();
        let mut log = record().to_vec();
        log.extend_from_slice(b"{\"samples\":");
        fs::write(d.path().join("timed.jsonl"), log).unwrap();
        assert_eq!(recover(d.path(), 2).unwrap().len(), 1);
        assert_eq!(fs::metadata(d.path().join("audio.s16")).unwrap().len(), 4);
        assert_eq!(fs::read(d.path().join("timed.jsonl")).unwrap(), record());
        assert_eq!(recover(d.path(), 2).unwrap().len(), 1);
    }
    #[test]
    fn missing_audio_and_excess_records_are_errors() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("timed.jsonl"), record()).unwrap();
        assert!(recover(d.path(), 1).is_err());
        fs::write(d.path().join("audio.s16"), [0u8; 4]).unwrap();
        assert!(recover(d.path(), 0).is_err());
    }
    #[test]
    fn lock_excludes_second_writer() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("lock");
        let first = lock(&p).unwrap();
        assert!(lock(&p).is_err());
        drop(first);
        assert!(lock(&p).is_ok());
    }
    #[test]
    fn atomic_replacement_and_fingerprint() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("plan");
        atomic_write(&p, b"first").unwrap();
        let old = digest(&p).unwrap();
        atomic_write(&p, b"second").unwrap();
        assert_ne!(old, digest(&p).unwrap());
        assert_eq!(fs::read(p).unwrap(), b"second");
    }
}
