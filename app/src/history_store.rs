use pepperx_audio::{RecordingArtifact, SelectedMicrophone};
use pepperx_session::TriggerSource;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::transcript_log::{TranscriptEntry, TranscriptLog};

const HISTORY_DIR_NAME: &str = "history";
const RECORDINGS_DIR_NAME: &str = "recordings";
const ARCHIVE_METADATA_FILE_NAME: &str = "run.json";
const ARCHIVED_SOURCE_WAV_FILE_NAME: &str = "source.wav";

/// Result of a privacy purge of history / recording artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryPurgeStats {
    pub deleted_files: usize,
    pub deleted_dirs: usize,
    pub freed_bytes: u64,
    pub errors: usize,
}

impl HistoryPurgeStats {
    pub fn summary_text(&self) -> String {
        if self.deleted_files == 0 && self.deleted_dirs == 0 && self.errors == 0 {
            return "No history or recordings to purge.".into();
        }
        let mb = self.freed_bytes as f64 / (1024.0 * 1024.0);
        if self.errors == 0 {
            format!(
                "Purged history ({} file(s), {} folder(s)), freed {:.1} MB.",
                self.deleted_files, self.deleted_dirs, mb
            )
        } else {
            format!(
                "Purged history ({} file(s), {} folder(s)), freed {:.1} MB ({} error(s)).",
                self.deleted_files, self.deleted_dirs, mb, self.errors
            )
        }
    }
}

/// Kept for older call sites / tests that still use the previous name.
pub type RecordingPurgeStats = HistoryPurgeStats;

const TRANSCRIPT_LOG_FILE_NAME: &str = "transcript-log.jsonl";

#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveWriteRequest {
    pub entry: TranscriptEntry,
    pub runtime_metadata: RunRuntimeMetadata,
    pub parent_run_id: Option<String>,
    pub prompt_profile: Option<String>,
    pub supporting_context_text: Option<String>,
    pub ocr_text: Option<String>,
}

impl ArchiveWriteRequest {
    pub fn new(entry: TranscriptEntry) -> Self {
        Self {
            entry,
            runtime_metadata: RunRuntimeMetadata::wav_import(),
            parent_run_id: None,
            prompt_profile: None,
            supporting_context_text: None,
            ocr_text: None,
        }
    }

    pub fn with_runtime_metadata(mut self, runtime_metadata: RunRuntimeMetadata) -> Self {
        self.runtime_metadata = runtime_metadata;
        self
    }

    pub fn with_parent_run_id(mut self, parent_run_id: impl Into<String>) -> Self {
        self.parent_run_id = Some(parent_run_id.into());
        self
    }

    pub fn with_prompt_profile(mut self, prompt_profile: impl Into<String>) -> Self {
        self.prompt_profile = Some(prompt_profile.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_supporting_context(mut self, supporting_context_text: impl Into<String>) -> Self {
        self.supporting_context_text = Some(supporting_context_text.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_ocr_text(mut self, ocr_text: impl Into<String>) -> Self {
        self.ocr_text = Some(ocr_text.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArchivedRun {
    pub run_id: String,
    pub archived_at_ms: u64,
    pub run_dir: PathBuf,
    pub metadata_path: PathBuf,
    pub entry: TranscriptEntry,
    pub runtime_metadata: RunRuntimeMetadata,
    pub archived_source_wav_path: Option<PathBuf>,
    pub parent_run_id: Option<String>,
    pub prompt_profile: Option<String>,
    pub supporting_context_text: Option<String>,
    pub ocr_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HistoryStore {
    root: PathBuf,
    history_root: PathBuf,
    legacy_log: TranscriptLog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRuntimeMetadata {
    pub input_origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_microphone: Option<SelectedMicrophone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

impl RunRuntimeMetadata {
    pub fn wav_import() -> Self {
        Self {
            input_origin: "wav-import".into(),
            trigger_source: None,
            selected_microphone: None,
            recording_elapsed_ms: None,
            failure_stage: None,
            failure_reason: None,
        }
    }

    pub fn archived_rerun() -> Self {
        Self {
            input_origin: "archived-rerun".into(),
            trigger_source: None,
            selected_microphone: None,
            recording_elapsed_ms: None,
            failure_stage: None,
            failure_reason: None,
        }
    }

    pub fn for_live_recording(
        trigger_source: TriggerSource,
        recording_artifact: &RecordingArtifact,
    ) -> Self {
        Self {
            input_origin: "live-recording".into(),
            trigger_source: Some(trigger_source_label(trigger_source).into()),
            selected_microphone: recording_artifact.selected_microphone().cloned(),
            recording_elapsed_ms: Some(recording_artifact.elapsed().as_millis() as u64),
            failure_stage: None,
            failure_reason: None,
        }
    }

    pub fn with_failure(
        mut self,
        failure_stage: impl Into<String>,
        failure_reason: impl Into<String>,
    ) -> Self {
        self.failure_stage = Some(failure_stage.into());
        self.failure_reason = Some(failure_reason.into());
        self
    }
}

impl HistoryStore {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let history_root = root.join(HISTORY_DIR_NAME);
        fs::create_dir_all(&history_root)?;

        Ok(Self {
            legacy_log: TranscriptLog::open(&root)?,
            root,
            history_root,
        })
    }

    pub fn archive_run(&self, request: &ArchiveWriteRequest) -> io::Result<ArchivedRun> {
        let run_id = next_run_id();
        let archived_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_millis() as u64;
        let run_dir = self.history_root.join(&run_id);
        fs::create_dir_all(&run_dir)?;

        let archived_source_wav_path =
            archive_source_wav(&request.entry.source_wav_path, &run_dir).transpose()?;
        let metadata_path = run_dir.join(ARCHIVE_METADATA_FILE_NAME);
        let metadata = StoredArchivedRun {
            run_id: run_id.clone(),
            archived_at_ms,
            entry: request.entry.clone(),
            runtime_metadata: request.runtime_metadata.clone(),
            archived_source_wav_path: archived_source_wav_path.clone(),
            parent_run_id: request.parent_run_id.clone(),
            prompt_profile: request.prompt_profile.clone(),
            supporting_context_text: request.supporting_context_text.clone(),
            ocr_text: request.ocr_text.clone(),
        };
        let metadata_json = serde_json::to_vec_pretty(&metadata)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(&metadata_path, metadata_json)?;

        self.legacy_log.append(&request.entry)?;

        Ok(metadata.into_archived_run(run_dir, metadata_path))
    }

    pub fn load_run(&self, run_id: &str) -> io::Result<Option<ArchivedRun>> {
        let run_dir = self.history_root.join(run_id);
        let metadata_path = run_dir.join(ARCHIVE_METADATA_FILE_NAME);
        if !metadata_path.is_file() {
            return Ok(None);
        }

        let metadata_json = fs::read_to_string(&metadata_path)?;
        let metadata: StoredArchivedRun = serde_json::from_str(&metadata_json)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        Ok(Some(metadata.into_archived_run(run_dir, metadata_path)))
    }

    pub fn recent_runs(&self) -> io::Result<Vec<ArchivedRun>> {
        let mut runs = Vec::new();
        if self.history_root.is_dir() {
            for entry in fs::read_dir(&self.history_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }

                let run_dir = entry.path();
                let metadata_path = run_dir.join(ARCHIVE_METADATA_FILE_NAME);
                if !metadata_path.is_file() {
                    continue;
                }

                let metadata_json = fs::read_to_string(&metadata_path)?;
                let metadata: StoredArchivedRun = serde_json::from_str(&metadata_json)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                runs.push(metadata.into_archived_run(run_dir, metadata_path));
            }
        }

        if runs.is_empty() {
            return self.legacy_runs();
        }

        runs.sort_by(|left, right| {
            right
                .archived_at_ms
                .cmp(&left.archived_at_ms)
                .then_with(|| right.run_id.cmp(&left.run_id))
        });
        Ok(runs)
    }

    pub fn recent_entries(&self) -> io::Result<Vec<TranscriptEntry>> {
        Ok(self
            .recent_runs()?
            .into_iter()
            .map(|run| run.entry)
            .collect())
    }

    /// Privacy purge: remove past dictation history and recording artifacts that
    /// are not required for Pepper X to keep working.
    ///
    /// Deletes:
    /// - `history/run-*` directories (WAV + `run.json` transcripts/OCR/context)
    /// - `transcript-log.jsonl`
    /// - Pepper X captures in `recordings/` (`live-recording-*.wav`, `test-dictation-*.wav`)
    ///
    /// Keeps settings, setup/onboarding, correction memory, models, and any
    /// non-Pepper files under a user-managed recordings directory.
    pub fn purge_history_for_privacy(&self) -> io::Result<HistoryPurgeStats> {
        let mut stats = HistoryPurgeStats::default();

        self.purge_pepperx_recording_files(&mut stats);
        self.purge_history_run_dirs(&mut stats);
        self.purge_transcript_log(&mut stats);

        // Keep an empty history root so subsequent archive_run/open stay valid.
        if let Err(error) = fs::create_dir_all(&self.history_root) {
            let _ = error;
            stats.errors += 1;
        }

        Ok(stats)
    }

    /// Alias kept for older call sites.
    pub fn purge_recording_audio(&self) -> io::Result<HistoryPurgeStats> {
        self.purge_history_for_privacy()
    }

    fn purge_pepperx_recording_files(&self, stats: &mut HistoryPurgeStats) {
        let recordings_dir = self.root.join(RECORDINGS_DIR_NAME);
        if !recordings_dir.is_dir() {
            return;
        }
        let entries = match fs::read_dir(&recordings_dir) {
            Ok(entries) => entries,
            Err(_) => {
                stats.errors += 1;
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !is_pepperx_recording_filename(name) {
                continue;
            }
            match remove_file_with_size(&path) {
                Ok(bytes) => {
                    stats.deleted_files += 1;
                    stats.freed_bytes += bytes;
                }
                Err(_) => stats.errors += 1,
            }
        }
    }

    fn purge_history_run_dirs(&self, stats: &mut HistoryPurgeStats) {
        if !self.history_root.is_dir() {
            return;
        }
        let entries = match fs::read_dir(&self.history_root) {
            Ok(entries) => entries,
            Err(_) => {
                stats.errors += 1;
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                match remove_dir_all_with_size(&path) {
                    Ok((files, bytes)) => {
                        stats.deleted_dirs += 1;
                        stats.deleted_files += files;
                        stats.freed_bytes += bytes;
                    }
                    Err(_) => stats.errors += 1,
                }
            } else if path.is_file() {
                match remove_file_with_size(&path) {
                    Ok(bytes) => {
                        stats.deleted_files += 1;
                        stats.freed_bytes += bytes;
                    }
                    Err(_) => stats.errors += 1,
                }
            }
        }
    }

    fn purge_transcript_log(&self, stats: &mut HistoryPurgeStats) {
        let log_path = self.root.join(TRANSCRIPT_LOG_FILE_NAME);
        if !log_path.is_file() {
            return;
        }
        match remove_file_with_size(&log_path) {
            Ok(bytes) => {
                stats.deleted_files += 1;
                stats.freed_bytes += bytes;
            }
            Err(_) => stats.errors += 1,
        }
    }

    fn legacy_runs(&self) -> io::Result<Vec<ArchivedRun>> {
        Ok(self
            .legacy_log
            .recent_entries()?
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let run_id = format!("legacy-{index}");
                let run_dir = self.root.join(HISTORY_DIR_NAME).join(&run_id);
                let metadata_path = run_dir.join(ARCHIVE_METADATA_FILE_NAME);
                ArchivedRun {
                    run_id,
                    archived_at_ms: 0,
                    run_dir,
                    metadata_path,
                    entry,
                    runtime_metadata: RunRuntimeMetadata::wav_import(),
                    archived_source_wav_path: None,
                    parent_run_id: None,
                    prompt_profile: None,
                    supporting_context_text: None,
                    ocr_text: None,
                }
            })
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredArchivedRun {
    run_id: String,
    archived_at_ms: u64,
    entry: TranscriptEntry,
    runtime_metadata: RunRuntimeMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archived_source_wav_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    supporting_context_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ocr_text: Option<String>,
}

impl StoredArchivedRun {
    fn into_archived_run(self, run_dir: PathBuf, metadata_path: PathBuf) -> ArchivedRun {
        ArchivedRun {
            run_id: self.run_id,
            archived_at_ms: self.archived_at_ms,
            run_dir,
            metadata_path,
            entry: self.entry,
            runtime_metadata: self.runtime_metadata,
            archived_source_wav_path: self.archived_source_wav_path,
            parent_run_id: self.parent_run_id,
            prompt_profile: self.prompt_profile,
            supporting_context_text: self.supporting_context_text,
            ocr_text: self.ocr_text,
        }
    }
}

fn archive_source_wav(source_wav_path: &Path, run_dir: &Path) -> Option<io::Result<PathBuf>> {
    if !source_wav_path.is_file() {
        return None;
    }

    let archived_source_wav_path = run_dir.join(ARCHIVED_SOURCE_WAV_FILE_NAME);
    Some(
        fs::copy(source_wav_path, &archived_source_wav_path)
            .map(|_| archived_source_wav_path)
            .map_err(io::Error::from),
    )
}

fn is_pepperx_recording_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".wav")
        && (lower.starts_with("live-recording-") || lower.starts_with("test-dictation-"))
}

fn remove_file_with_size(path: &Path) -> io::Result<u64> {
    let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    fs::remove_file(path)?;
    Ok(size)
}

fn remove_dir_all_with_size(path: &Path) -> io::Result<(usize, u64)> {
    let (files, bytes) = dir_tree_stats(path)?;
    fs::remove_dir_all(path)?;
    Ok((files, bytes))
}

fn dir_tree_stats(path: &Path) -> io::Result<(usize, u64)> {
    let mut files = 0usize;
    let mut bytes = 0u64;
    if path.is_file() {
        return Ok((1, fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)));
    }
    if !path.is_dir() {
        return Ok((0, 0));
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            let (child_files, child_bytes) = dir_tree_stats(&child)?;
            files += child_files;
            bytes += child_bytes;
        } else if child.is_file() {
            files += 1;
            bytes += fs::metadata(&child).map(|meta| meta.len()).unwrap_or(0);
        }
    }
    Ok((files, bytes))
}

fn next_run_id() -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("run-{}-{unique}", std::process::id())
}

fn trigger_source_label(trigger_source: TriggerSource) -> &'static str {
    match trigger_source {
        TriggerSource::ModifierOnly => "modifier-only",
        TriggerSource::StandardShortcut => "standard-shortcut",
        TriggerSource::ShellAction => "shell-action",
    }
}

#[cfg(test)]
mod history_store_tests {
    use super::*;
    use crate::transcript_log::{CleanupDiagnostics, InsertionDiagnostics};
    use std::time::Duration;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pepper-x-history-store-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn transcript_entry(source_wav_path: &std::path::Path) -> TranscriptEntry {
        let mut entry = TranscriptEntry::new(
            source_wav_path,
            "hello from pepper x",
            "parakeet-rs",
            "nemotron-speech-streaming-en-0.6b",
            Duration::from_millis(42),
        );
        entry.cleanup = Some(CleanupDiagnostics::succeeded(
            "llama.cpp",
            "qwen3.5-2b-q4_k_m.gguf",
            "Hello from Pepper X.",
            Duration::from_millis(17),
        ));
        entry.insertion = Some(
            InsertionDiagnostics::succeeded("atspi-editable-text", "Text Editor")
                .with_target_class("text-editor"),
        );
        entry
    }

    #[test]
    fn transcript_archive_writes_run_specific_directory_and_metadata() {
        let root = temp_root("archive-write");
        std::fs::create_dir_all(&root).unwrap();
        let source_wav_path = root.join("source.wav");
        std::fs::write(&source_wav_path, b"pepper-x-audio").unwrap();
        let store = HistoryStore::open(&root).expect("history store should open");
        let request = ArchiveWriteRequest::new(transcript_entry(&source_wav_path))
            .with_prompt_profile("ordinary-dictation")
            .with_supporting_context("surrounding text")
            .with_ocr_text("ocr words");

        let archived = store
            .archive_run(&request)
            .expect("archive write should succeed");

        assert!(!archived.run_id.is_empty());
        assert!(archived.run_dir.is_dir());
        assert!(archived.metadata_path.is_file());
        assert_eq!(archived.entry, request.entry);
        assert_eq!(archived.parent_run_id, None);
        assert_eq!(
            archived.prompt_profile.as_deref(),
            Some("ordinary-dictation")
        );
        assert_eq!(
            archived.supporting_context_text.as_deref(),
            Some("surrounding text")
        );
        assert_eq!(archived.ocr_text.as_deref(), Some("ocr words"));
        assert_eq!(
            archived.archived_source_wav_path.as_deref(),
            Some(archived.run_dir.join("source.wav").as_path())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn history_store_loads_runs_newest_first() {
        let root = temp_root("newest-first");
        std::fs::create_dir_all(&root).unwrap();
        let source_one = root.join("source-one.wav");
        let source_two = root.join("source-two.wav");
        std::fs::write(&source_one, b"one").unwrap();
        std::fs::write(&source_two, b"two").unwrap();
        let store = HistoryStore::open(&root).expect("history store should open");
        let first = store
            .archive_run(&ArchiveWriteRequest::new(transcript_entry(&source_one)))
            .expect("first archive write should succeed");
        std::thread::sleep(Duration::from_millis(2));
        let second = store
            .archive_run(&ArchiveWriteRequest::new(transcript_entry(&source_two)))
            .expect("second archive write should succeed");

        let runs = store.recent_runs().expect("history store should load runs");

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].run_id, second.run_id);
        assert_eq!(runs[1].run_id, first.run_id);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn history_store_preserves_prompt_profile_and_context_artifacts() {
        let root = temp_root("context-artifacts");
        std::fs::create_dir_all(&root).unwrap();
        let source_wav_path = root.join("source.wav");
        std::fs::write(&source_wav_path, b"pepper-x-audio").unwrap();
        let store = HistoryStore::open(&root).expect("history store should open");

        store
            .archive_run(
                &ArchiveWriteRequest::new(transcript_entry(&source_wav_path))
                    .with_prompt_profile("ordinary-dictation")
                    .with_supporting_context("line before\nline after")
                    .with_ocr_text("ocr fallback text"),
            )
            .expect("archive write should succeed");

        let archived = store
            .recent_runs()
            .expect("history store should load runs")
            .into_iter()
            .next()
            .expect("one archived run should exist");

        assert_eq!(
            archived.prompt_profile.as_deref(),
            Some("ordinary-dictation")
        );
        assert_eq!(
            archived.supporting_context_text.as_deref(),
            Some("line before\nline after")
        );
        assert_eq!(archived.ocr_text.as_deref(), Some("ocr fallback text"));
        assert!(archived.entry.cleanup.is_some());
        assert!(archived.entry.insertion.is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn history_store_load_run_round_trips_parent_linkage() {
        let root = temp_root("load-run");
        std::fs::create_dir_all(&root).unwrap();
        let source_wav_path = root.join("source.wav");
        std::fs::write(&source_wav_path, b"pepper-x-audio").unwrap();
        let store = HistoryStore::open(&root).expect("history store should open");

        let parent = store
            .archive_run(&ArchiveWriteRequest::new(transcript_entry(
                &source_wav_path,
            )))
            .expect("parent archive write should succeed");
        let child = store
            .archive_run(
                &ArchiveWriteRequest::new(transcript_entry(&source_wav_path))
                    .with_parent_run_id(parent.run_id.clone()),
            )
            .expect("child archive write should succeed");
        let reloaded_child = store
            .load_run(&child.run_id)
            .expect("history store should load a specific run")
            .expect("child run should exist");

        assert_eq!(reloaded_child.run_id, child.run_id);
        assert_eq!(reloaded_child.parent_run_id, Some(parent.run_id));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn purge_history_for_privacy_wipes_runs_log_and_wavs_keeps_settings_surface() {
        let root = temp_root("purge-privacy");
        std::fs::create_dir_all(&root).unwrap();
        let source_wav_path = root.join("source.wav");
        std::fs::write(&source_wav_path, b"pepper-x-audio-payload").unwrap();
        let settings_path = root.join("settings.json");
        let setup_path = root.join("setup.json");
        let corrections_dir = root.join("corrections");
        std::fs::write(&settings_path, b"{\"keep\":true}").unwrap();
        std::fs::write(&setup_path, b"{\"keep\":true}").unwrap();
        std::fs::create_dir_all(&corrections_dir).unwrap();
        std::fs::write(corrections_dir.join("corrections.jsonl"), b"keep").unwrap();

        let store = HistoryStore::open(&root).expect("history store should open");
        let archived = store
            .archive_run(&ArchiveWriteRequest::new(transcript_entry(&source_wav_path)))
            .expect("archive write should succeed");
        assert!(archived.metadata_path.is_file());
        assert!(root.join("transcript-log.jsonl").is_file());

        let recordings_dir = root.join("recordings");
        std::fs::create_dir_all(&recordings_dir).unwrap();
        let live = recordings_dir.join("live-recording-1-2.wav");
        let test_dictation = recordings_dir.join("test-dictation-99.wav");
        let keep_user_file = recordings_dir.join("Sean-notes.flac");
        let keep_other_wav = recordings_dir.join("meeting-notes.wav");
        std::fs::write(&live, b"live-wav").unwrap();
        std::fs::write(&test_dictation, b"test-wav").unwrap();
        std::fs::write(&keep_user_file, b"user-flac").unwrap();
        std::fs::write(&keep_other_wav, b"other-wav").unwrap();

        let stats = store
            .purge_history_for_privacy()
            .expect("purge should succeed");

        assert!(stats.deleted_files >= 4);
        assert_eq!(stats.deleted_dirs, 1);
        assert!(stats.freed_bytes > 0);
        assert_eq!(stats.errors, 0);
        assert!(!live.exists());
        assert!(!test_dictation.exists());
        assert!(keep_user_file.is_file());
        assert!(keep_other_wav.is_file());
        assert!(!archived.run_dir.exists());
        assert!(!root.join("transcript-log.jsonl").exists());
        assert!(root.join("history").is_dir());
        assert!(store.recent_runs().expect("reload history").is_empty());
        assert!(settings_path.is_file());
        assert!(setup_path.is_file());
        assert!(corrections_dir.join("corrections.jsonl").is_file());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn is_pepperx_recording_filename_matches_live_and_test_only() {
        assert!(is_pepperx_recording_filename(
            "live-recording-103408-1784193243847383261.wav"
        ));
        assert!(is_pepperx_recording_filename("test-dictation-1.wav"));
        assert!(is_pepperx_recording_filename("LIVE-RECORDING-x.WAV"));
        assert!(!is_pepperx_recording_filename("meeting-notes.wav"));
        assert!(!is_pepperx_recording_filename("live-recording-x.flac"));
        assert!(!is_pepperx_recording_filename("source.wav"));
    }
}
