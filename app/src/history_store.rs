use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::transcript_log::{TranscriptEntry, TranscriptLog};

const HISTORY_DIR_NAME: &str = "history";
const ARCHIVE_METADATA_FILE_NAME: &str = "run.json";
const ARCHIVED_SOURCE_WAV_FILE_NAME: &str = "source.wav";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveWriteRequest {
    pub entry: TranscriptEntry,
    pub prompt_profile: Option<String>,
    pub supporting_context_text: Option<String>,
    pub ocr_text: Option<String>,
}

impl ArchiveWriteRequest {
    pub fn new(entry: TranscriptEntry) -> Self {
        Self {
            entry,
            prompt_profile: None,
            supporting_context_text: None,
            ocr_text: None,
        }
    }

    pub fn with_prompt_profile(mut self, prompt_profile: impl Into<String>) -> Self {
        self.prompt_profile = Some(prompt_profile.into());
        self
    }

    pub fn with_supporting_context(mut self, supporting_context_text: impl Into<String>) -> Self {
        self.supporting_context_text = Some(supporting_context_text.into());
        self
    }

    pub fn with_ocr_text(mut self, ocr_text: impl Into<String>) -> Self {
        self.ocr_text = Some(ocr_text.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedRun {
    pub run_id: String,
    pub archived_at_ms: u64,
    pub run_dir: PathBuf,
    pub metadata_path: PathBuf,
    pub entry: TranscriptEntry,
    pub archived_source_wav_path: Option<PathBuf>,
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
            archived_source_wav_path: archived_source_wav_path.clone(),
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
                    archived_source_wav_path: None,
                    prompt_profile: None,
                    supporting_context_text: None,
                    ocr_text: None,
                }
            })
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredArchivedRun {
    run_id: String,
    archived_at_ms: u64,
    entry: TranscriptEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archived_source_wav_path: Option<PathBuf>,
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
            archived_source_wav_path: self.archived_source_wav_path,
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

fn next_run_id() -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("run-{}-{unique}", std::process::id())
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
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(42),
        );
        entry.cleanup = Some(CleanupDiagnostics::succeeded(
            "llama.cpp",
            "qwen2.5-3b-instruct-q4_k_m.gguf",
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
}
