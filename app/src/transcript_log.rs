use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LOG_FILE_NAME: &str = "transcript-log.jsonl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub source_wav_path: PathBuf,
    pub transcript_text: String,
    pub backend_name: String,
    pub model_name: String,
    pub elapsed_ms: u64,
}

impl TranscriptEntry {
    pub fn new(
        source_wav_path: impl Into<PathBuf>,
        transcript_text: impl Into<String>,
        backend_name: impl Into<String>,
        model_name: impl Into<String>,
        elapsed: Duration,
    ) -> Self {
        Self {
            source_wav_path: source_wav_path.into(),
            transcript_text: transcript_text.into(),
            backend_name: backend_name.into(),
            model_name: model_name.into(),
            elapsed_ms: elapsed.as_millis() as u64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranscriptLog {
    log_path: PathBuf,
}

impl TranscriptLog {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref();
        fs::create_dir_all(root)?;
        Ok(Self {
            log_path: root.join(LOG_FILE_NAME),
        })
    }

    pub fn append(&self, entry: &TranscriptEntry) -> io::Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        let mut writer = io::BufWriter::new(file);
        serde_json::to_writer(&mut writer, entry)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        writer.write_all(b"\n")?;
        writer.flush()
    }

    pub fn recent_entries(&self) -> io::Result<Vec<TranscriptEntry>> {
        let file = match File::open(&self.log_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };

        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let entry = serde_json::from_str(&line)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            entries.push(entry);
        }

        entries.reverse();
        Ok(entries)
    }

    #[cfg(test)]
    fn log_path(&self) -> &Path {
        &self.log_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("pepper-x-transcript-log-{unique}-{}", std::process::id()))
    }

    #[test]
    fn transcript_log_append_and_reload_preserve_entry_fields() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        let expected = TranscriptEntry::new(
            "/tmp/loop1/sample.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(1234),
        );

        log.append(&expected).expect("append entry");

        let reopened = TranscriptLog::open(&root).expect("reopen log");
        let entries = reopened.recent_entries().expect("load entries");

        assert_eq!(entries, vec![expected]);
    }

    #[test]
    fn transcript_log_recent_entries_are_returned_newest_first() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        let first = TranscriptEntry::new(
            "first.wav",
            "first",
            "backend-one",
            "model-one",
            Duration::from_millis(10),
        );
        let second = TranscriptEntry::new(
            "second.wav",
            "second",
            "backend-two",
            "model-two",
            Duration::from_millis(20),
        );

        log.append(&first).expect("append first");
        log.append(&second).expect("append second");

        let entries = log.recent_entries().expect("load entries");

        assert_eq!(entries, vec![second, first]);
    }

    #[test]
    fn transcript_log_uses_jsonl_storage() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        let expected = TranscriptEntry::new(
            "sample.wav",
            "sample",
            "backend",
            "model",
            Duration::from_millis(42),
        );

        log.append(&expected).expect("append entry");

        let raw = std::fs::read_to_string(log.log_path()).expect("read log");
        assert!(raw.ends_with('\n'));
        assert_eq!(raw.lines().count(), 1);
        assert!(raw.contains("\"transcript_text\":\"sample\""));
        assert!(raw.contains("\"backend_name\":\"backend\""));
        assert!(raw.contains("\"model_name\":\"model\""));
    }
}
