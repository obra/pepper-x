use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const LOG_FILE_NAME: &str = "transcript-log.jsonl";
const APP_STATE_DIR_NAME: &str = "pepper-x";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub source_wav_path: PathBuf,
    pub transcript_text: String,
    pub backend_name: String,
    pub model_name: String,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<CleanupDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion: Option<InsertionDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learning: Option<LearningDiagnostics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diarization: Option<DiarizationSummary>,
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
            cleanup: None,
            insertion: None,
            learning: None,
            diarization: None,
        }
    }

    pub fn display_text(&self) -> &str {
        self.cleanup
            .as_ref()
            .and_then(CleanupDiagnostics::cleaned_text)
            .unwrap_or(&self.transcript_text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupDiagnostics {
    pub backend_name: String,
    pub model_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleaned_text: Option<String>,
    pub elapsed_ms: u64,
    pub used_ocr: bool,
    pub succeeded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

impl CleanupDiagnostics {
    pub fn succeeded(
        backend_name: impl Into<String>,
        model_name: impl Into<String>,
        cleaned_text: impl Into<String>,
        elapsed: Duration,
    ) -> Self {
        Self {
            backend_name: backend_name.into(),
            model_name: model_name.into(),
            cleaned_text: Some(cleaned_text.into()),
            elapsed_ms: elapsed.as_millis() as u64,
            used_ocr: false,
            succeeded: true,
            failure_reason: None,
        }
    }

    pub fn failed(
        backend_name: impl Into<String>,
        model_name: impl Into<String>,
        failure_reason: impl Into<String>,
    ) -> Self {
        Self {
            backend_name: backend_name.into(),
            model_name: model_name.into(),
            cleaned_text: None,
            elapsed_ms: 0,
            used_ocr: false,
            succeeded: false,
            failure_reason: Some(failure_reason.into()),
        }
    }

    pub fn cleaned_text(&self) -> Option<&str> {
        self.cleaned_text.as_deref().filter(|_| self.succeeded)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningDiagnostics {
    pub action: String,
    pub source_text: String,
    pub replacement_text: String,
}

impl LearningDiagnostics {
    pub fn prompt_memory(
        source_text: impl Into<String>,
        replacement_text: impl Into<String>,
    ) -> Self {
        Self {
            action: "prompt-memory".into(),
            source_text: source_text.into(),
            replacement_text: replacement_text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiarizationSegment {
    pub speaker: String,
    pub start_secs: f64,
    pub end_secs: f64,
}

impl DiarizationSegment {
    pub fn new(speaker: impl Into<String>, start_secs: f64, end_secs: f64) -> Self {
        Self {
            speaker: speaker.into(),
            start_secs,
            end_secs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiarizationSummary {
    pub target_speaker: Option<String>,
    pub segments: Vec<DiarizationSegment>,
    pub filtering_used: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

impl DiarizationSummary {
    pub fn new(segments: Vec<DiarizationSegment>, filtering_used: bool) -> Self {
        Self {
            target_speaker: None,
            segments,
            filtering_used,
            fallback_reason: None,
        }
    }

    pub fn with_target_speaker(mut self, target_speaker: impl Into<String>) -> Self {
        self.target_speaker = Some(target_speaker.into());
        self
    }

    pub fn with_fallback_reason(mut self, fallback_reason: impl Into<String>) -> Self {
        self.fallback_reason = Some(fallback_reason.into());
        self
    }

    pub fn distinct_speakers(&self) -> Vec<&str> {
        let mut speakers: Vec<&str> = self
            .segments
            .iter()
            .map(|seg| seg.speaker.as_str())
            .collect();
        speakers.sort();
        speakers.dedup();
        speakers
    }
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
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
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
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
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
        assert!(
            copied.contains("\"attempted_backends\":[\"atspi-editable-text\",\"clipboard-paste\"]")
        );
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

    #[test]
    fn cleanup_transcript_log_round_trips_cleanup_diagnostics_from_jsonl() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(
                br#"{"source_wav_path":"loop5.wav","transcript_text":"hello from pepper x","backend_name":"sherpa-onnx","model_name":"nemo-parakeet-tdt-0.6b-v2-int8","elapsed_ms":7,"cleanup":{"backend_name":"llama.cpp","model_name":"qwen2.5-3b-instruct-q4_k_m.gguf","cleaned_text":"Hello from Pepper X.","elapsed_ms":19,"used_ocr":false,"succeeded":true}}"#,
            )
            .expect("append loop5 cleanup entry");
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
            .expect("load loop5 cleanup entry");
        let copy_root = temp_root();
        let copy_log = TranscriptLog::open(&copy_root).expect("open copy log");

        copy_log.append(&entry).expect("append copied entry");

        let copied = std::fs::read_to_string(copy_log.log_path()).expect("read copied log");
        assert!(copied.contains("\"transcript_text\":\"hello from pepper x\""));
        assert!(copied.contains("\"cleanup\":{"));
        assert!(copied.contains("\"cleaned_text\":\"Hello from Pepper X.\""));
    }

    #[test]
    fn correction_memory_transcript_log_round_trips_learning_diagnostics_from_jsonl() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(
                br#"{"source_wav_path":"loop5.wav","transcript_text":"hello from pepper x","backend_name":"sherpa-onnx","model_name":"nemo-parakeet-tdt-0.6b-v2-int8","elapsed_ms":7,"learning":{"action":"prompt-memory","source_text":"hello from pepper x","replacement_text":"Hello from Pepper X."}}"#,
            )
            .expect("append loop5 learning entry");
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
            .expect("load loop5 learning entry");
        let copy_root = temp_root();
        let copy_log = TranscriptLog::open(&copy_root).expect("open copy log");

        assert_eq!(
            entry.learning,
            Some(LearningDiagnostics::prompt_memory(
                "hello from pepper x",
                "Hello from Pepper X."
            ))
        );
        copy_log.append(&entry).expect("append copied entry");

        let copied = std::fs::read_to_string(copy_log.log_path()).expect("read copied log");
        assert!(copied.contains("\"learning\":{"));
        assert!(copied.contains("\"action\":\"prompt-memory\""));
    }

    #[test]
    fn transcript_log_round_trips_diarization_summary_from_jsonl() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        let mut entry = TranscriptEntry::new(
            root.join("diarized.wav"),
            "hello from pepper x",
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
            Duration::from_millis(42),
        );
        entry.diarization = Some(
            DiarizationSummary::new(
                vec![
                    DiarizationSegment::new("Speaker 0", 0.0, 2.5),
                    DiarizationSegment::new("Speaker 1", 2.5, 5.0),
                    DiarizationSegment::new("Speaker 0", 5.0, 7.5),
                ],
                true,
            )
            .with_target_speaker("Speaker 0"),
        );

        log.append(&entry).expect("append entry");

        let reloaded = log
            .recent_entries()
            .expect("load entries")
            .into_iter()
            .next()
            .expect("load diarized entry");

        let diarization = reloaded.diarization.expect("diarization should be present");
        assert_eq!(diarization.target_speaker.as_deref(), Some("Speaker 0"));
        assert!(diarization.filtering_used);
        assert_eq!(diarization.fallback_reason, None);
        assert_eq!(diarization.segments.len(), 3);
        assert_eq!(diarization.segments[0].speaker, "Speaker 0");
        assert!((diarization.segments[0].start_secs - 0.0).abs() < f64::EPSILON);
        assert!((diarization.segments[0].end_secs - 2.5).abs() < f64::EPSILON);
        assert_eq!(diarization.segments[1].speaker, "Speaker 1");
        assert!((diarization.segments[1].start_secs - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn transcript_log_round_trips_diarization_with_fallback_reason() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        let mut entry = TranscriptEntry::new(
            root.join("fallback.wav"),
            "short audio",
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
            Duration::from_millis(10),
        );
        entry.diarization = Some(
            DiarizationSummary::new(
                vec![DiarizationSegment::new("Speaker 0", 0.0, 1.0)],
                true,
            )
            .with_fallback_reason("recording too short for diarization"),
        );

        log.append(&entry).expect("append entry");

        let reloaded = log
            .recent_entries()
            .expect("load entries")
            .into_iter()
            .next()
            .expect("load fallback entry");

        let diarization = reloaded.diarization.expect("diarization should be present");
        assert!(diarization.filtering_used);
        assert_eq!(diarization.target_speaker, None);
        assert_eq!(
            diarization.fallback_reason.as_deref(),
            Some("recording too short for diarization")
        );
        assert_eq!(diarization.segments.len(), 1);
    }

    #[test]
    fn transcript_log_loads_entries_without_diarization_field() {
        let root = temp_root();
        let log = TranscriptLog::open(&root).expect("open log");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log.log_path())
            .expect("open transcript log")
            .write_all(
                br#"{"source_wav_path":"no-diarization.wav","transcript_text":"hello","backend_name":"sherpa-onnx","model_name":"nemo-parakeet-tdt-0.6b-v3-int8","elapsed_ms":7}"#,
            )
            .expect("append entry without diarization");
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
            .expect("load entry without diarization");

        assert_eq!(entry.diarization, None);
    }

    #[test]
    fn diarization_summary_distinct_speakers_deduplicates_and_sorts() {
        let summary = DiarizationSummary::new(
            vec![
                DiarizationSegment::new("Speaker 1", 0.0, 1.0),
                DiarizationSegment::new("Speaker 0", 1.0, 2.0),
                DiarizationSegment::new("Speaker 1", 2.0, 3.0),
                DiarizationSegment::new("Speaker 0", 3.0, 4.0),
            ],
            false,
        );

        let speakers = summary.distinct_speakers();
        assert_eq!(speakers, vec!["Speaker 0", "Speaker 1"]);
    }
}
