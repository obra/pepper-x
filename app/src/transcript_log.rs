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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion: Option<InsertionDiagnostics>,
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
            insertion: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsertionDiagnostics {
    pub backend_name: String,
    pub target_application_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_class: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempted_backends: Vec<String>,
    pub succeeded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

impl InsertionDiagnostics {
    pub fn succeeded(
        backend_name: impl Into<String>,
        target_application_name: impl Into<String>,
    ) -> Self {
        Self {
            backend_name: backend_name.into(),
            target_application_name: target_application_name.into(),
            target_class: None,
            attempted_backends: Vec::new(),
            succeeded: true,
            failure_reason: None,
        }
    }

    pub fn failed(
        backend_name: impl Into<String>,
        target_application_name: impl Into<String>,
        failure_reason: impl Into<String>,
    ) -> Self {
        Self {
            backend_name: backend_name.into(),
            target_application_name: target_application_name.into(),
            target_class: None,
            attempted_backends: Vec::new(),
            succeeded: false,
            failure_reason: Some(failure_reason.into()),
        }
    }

    pub fn with_target_class(mut self, target_class: impl Into<String>) -> Self {
        self.target_class = Some(target_class.into());
        self
    }

    pub fn with_attempted_backends<I, S>(mut self, attempted_backends: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.attempted_backends = attempted_backends.into_iter().map(Into::into).collect();
        self
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
        let mut expected = TranscriptEntry::new(
            "/tmp/loop1/sample.wav",
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(1234),
        );
        expected.insertion = Some(
            InsertionDiagnostics::succeeded("atspi-editable-text", "Text Editor")
                .with_target_class("text-editor"),
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
        let mut expected = TranscriptEntry::new(
            "sample.wav",
            "sample",
            "backend",
            "model",
            Duration::from_millis(42),
        );
        expected.insertion = Some(
            InsertionDiagnostics::failed(
                "atspi-editable-text",
                "Calculator",
                "friendly insertion target is not editable",
            )
            .with_target_class("unsupported"),
        );

        log.append(&expected).expect("append entry");

        let raw = std::fs::read_to_string(log.log_path()).expect("read log");
        assert!(raw.ends_with('\n'));
        assert_eq!(raw.lines().count(), 1);
        assert!(raw.contains("\"transcript_text\":\"sample\""));
        assert!(raw.contains("\"backend_name\":\"backend\""));
        assert!(raw.contains("\"model_name\":\"model\""));
        assert!(raw.contains("\"target_application_name\":\"Calculator\""));
        assert!(raw.contains("\"target_class\":\"unsupported\""));
        assert!(raw.contains("\"failure_reason\":\"friendly insertion target is not editable\""));
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
    fn transcript_log_keeps_loop1_entries_without_insertion_diagnostics() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(
                br#"{"source_wav_path":"legacy.wav","transcript_text":"legacy","backend_name":"backend","model_name":"model","elapsed_ms":11}"#,
            )
            .expect("append legacy entry");
        std::fs::OpenOptions::new()
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(b"\n")
            .expect("append newline");

        let entries = log.recent_entries().expect("load entries");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].transcript_text, "legacy");
        assert_eq!(entries[0].insertion, None);
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

    #[test]
    fn transcript_log_round_trips_attempted_backends_from_jsonl() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(
                br#"{"source_wav_path":"loop4.wav","transcript_text":"hello","backend_name":"sherpa-onnx","model_name":"nemo-parakeet-tdt-0.6b-v2-int8","elapsed_ms":7,"insertion":{"backend_name":"clipboard-paste","target_application_name":"Firefox","target_class":"browser-textarea","attempted_backends":["atspi-editable-text","clipboard-paste"],"succeeded":true}}"#,
            )
            .expect("append loop4 entry");
        std::fs::OpenOptions::new()
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(b"\n")
            .expect("append newline");

        let entry = log
            .recent_entries()
            .expect("load entries")
            .into_iter()
            .next()
            .expect("load loop4 entry");
        let copy_root = temp_root();
        let copy_log = TranscriptLog::open(&copy_root).expect("open copy log");

        copy_log.append(&entry).expect("append copied entry");

        let copied = std::fs::read_to_string(copy_log.log_path()).expect("read copied log");
        assert!(copied.contains("\"attempted_backends\":[\"atspi-editable-text\",\"clipboard-paste\"]"));
    }

    #[test]
    fn transcript_log_round_trips_uinput_fallback_diagnostics_from_jsonl() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(
                br#"{"source_wav_path":"loop4-uinput.wav","transcript_text":"hello wine","backend_name":"sherpa-onnx","model_name":"nemo-parakeet-tdt-0.6b-v2-int8","elapsed_ms":9,"insertion":{"backend_name":"uinput-text","target_application_name":"Wine","target_class":"hostile","attempted_backends":["atspi-editable-text","atspi-key-string","clipboard-paste","uinput-text"],"succeeded":true}}"#,
            )
            .expect("append loop4 uinput entry");
        std::fs::OpenOptions::new()
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(b"\n")
            .expect("append newline");

        let entry = log
            .recent_entries()
            .expect("load entries")
            .into_iter()
            .next()
            .expect("load loop4 uinput entry");
        let copy_root = temp_root();
        let copy_log = TranscriptLog::open(&copy_root).expect("open copy log");

        copy_log.append(&entry).expect("append copied entry");

        let copied = std::fs::read_to_string(copy_log.log_path()).expect("read copied log");
        assert!(copied.contains("\"backend_name\":\"uinput-text\""));
        assert!(copied.contains("\"attempted_backends\":[\"atspi-editable-text\",\"atspi-key-string\",\"clipboard-paste\",\"uinput-text\"]"));
    }
}
