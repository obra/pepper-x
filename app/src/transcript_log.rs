use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LOG_FILE_NAME: &str = "transcript-log.jsonl";
const APP_STATE_DIR_NAME: &str = "pepper-x";

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
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        let mut payload = serde_json::to_vec(entry)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        payload.push(b'\n');
        file.write_all(&payload)?;
        file.flush()
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

            if let Ok(entry) = serde_json::from_str(&line) {
                entries.push(entry);
            }
        }

        entries.reverse();
        Ok(entries)
    }

    #[cfg(test)]
    fn log_path(&self) -> &Path {
        &self.log_path
    }
}

pub fn state_root() -> PathBuf {
    if let Some(root) = nonempty_env_path("PEPPERX_STATE_ROOT") {
        return root;
    }

    if let Some(xdg_state_home) = nonempty_env_path("XDG_STATE_HOME") {
        return xdg_state_home.join(APP_STATE_DIR_NAME);
    }

    if let Some(home) = nonempty_env_path("HOME") {
        return home.join(".local").join("state").join(APP_STATE_DIR_NAME);
    }

    PathBuf::from(APP_STATE_DIR_NAME)
}

pub(crate) fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
pub(crate) fn env_lock() -> &'static std::sync::Mutex<()> {
    static ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    ENV_LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn set_or_remove_env_var(name: &str, value: Option<OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pepper-x-transcript-log-{unique}-{}",
            std::process::id()
        ))
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

    #[test]
    fn transcript_log_prefers_explicit_state_root_override() {
        let _guard = env_lock().lock().unwrap();
        let expected = temp_root();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_xdg_state_home = std::env::var_os("XDG_STATE_HOME");
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("PEPPERX_STATE_ROOT", &expected);
        std::env::remove_var("XDG_STATE_HOME");
        std::env::remove_var("HOME");

        let root = state_root();

        assert_eq!(root, expected);
        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        set_or_remove_env_var("XDG_STATE_HOME", previous_xdg_state_home);
        set_or_remove_env_var("HOME", previous_home);
    }

    #[test]
    fn transcript_log_skips_truncated_lines_and_keeps_valid_entries() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        let expected = TranscriptEntry::new(
            root.join("sample.wav"),
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(42),
        );

        log.append(&expected).expect("append valid entry");
        std::fs::OpenOptions::new()
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(br#"{"transcript_text":"truncated""#)
            .expect("append truncated json");

        let entries = log.recent_entries().expect("load entries");

        assert_eq!(entries, vec![expected]);
    }

    #[test]
    fn transcript_log_ignores_empty_state_root_override() {
        let _guard = env_lock().lock().unwrap();
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_xdg_state_home = std::env::var_os("XDG_STATE_HOME");
        let previous_home = std::env::var_os("HOME");
        let expected = temp_root();
        std::env::set_var("PEPPERX_STATE_ROOT", "");
        std::env::set_var("XDG_STATE_HOME", &expected);
        std::env::remove_var("HOME");

        let root = state_root();

        assert_eq!(root, expected.join(APP_STATE_DIR_NAME));
        set_or_remove_env_var("PEPPERX_STATE_ROOT", previous_state_root);
        set_or_remove_env_var("XDG_STATE_HOME", previous_xdg_state_home);
        set_or_remove_env_var("HOME", previous_home);
    }
}
