use std::path::{Path, PathBuf};

use pepperx_asr::{transcribe_wav, TranscriptionError, TranscriptionRequest, TranscriptionResult};
use pepperx_platform_gnome::atspi::{
    insert_text_into_friendly_target, FriendlyInsertOutcome, FriendlyInsertPolicy,
    FriendlyInsertRunError, FRIENDLY_INSERT_BACKEND_NAME,
};

use crate::transcript_log::{
    nonempty_env_path, state_root, InsertionDiagnostics, TranscriptEntry, TranscriptLog,
};

const MODEL_NAME: &str = "nemo-parakeet-tdt-0.6b-v2-int8";
const FRIENDLY_TARGET_APPLICATION_ID: &str = "org.gnome.TextEditor";

#[derive(Debug)]
pub enum TranscriptionRunError {
    MissingModelDir,
    TranscriptLog(std::io::Error),
    Asr(TranscriptionError),
}

impl From<std::io::Error> for TranscriptionRunError {
    fn from(error: std::io::Error) -> Self {
        Self::TranscriptLog(error)
    }
}

impl From<TranscriptionError> for TranscriptionRunError {
    fn from(error: TranscriptionError) -> Self {
        Self::Asr(error)
    }
}

impl std::fmt::Display for TranscriptionRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingModelDir => {
                write!(
                    f,
                    "PEPPERX_PARAKEET_MODEL_DIR must point at a Parakeet model bundle"
                )
            }
            Self::TranscriptLog(error) => {
                write!(f, "failed to write Pepper X transcript log: {error}")
            }
            Self::Asr(error) => write!(
                f,
                "Pepper X transcription failed: {}",
                describe_asr_error(error)
            ),
        }
    }
}

pub fn transcribe_wav_to_log(wav_path: &Path) -> Result<TranscriptEntry, TranscriptionRunError> {
    let result = transcribe_wav_result(wav_path)?;
    archive_transcription_result(result)
}

pub fn transcribe_wav_and_insert_friendly_to_log(
    wav_path: &Path,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    let result = transcribe_wav_result(wav_path)?;
    archive_transcription_result_with_friendly_insert(result, |transcript_text| {
        insert_text_into_friendly_target(
            transcript_text,
            &FriendlyInsertPolicy {
                target_application_id: FRIENDLY_TARGET_APPLICATION_ID,
            },
        )
    })
}

fn transcribe_wav_result(wav_path: &Path) -> Result<TranscriptionResult, TranscriptionRunError> {
    let model_dir = configured_model_dir()?;
    let request = TranscriptionRequest::new(wav_path, &model_dir, MODEL_NAME);
    Ok(transcribe_wav(&request)?)
}

pub(crate) fn archive_transcription_result(
    result: TranscriptionResult,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    archive_transcript_entry(transcript_entry_from_result(result))
}

pub(crate) fn archive_transcription_result_with_friendly_insert<F>(
    result: TranscriptionResult,
    insert: F,
) -> Result<TranscriptEntry, TranscriptionRunError>
where
    F: FnOnce(&str) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError>,
{
    let mut entry = transcript_entry_from_result(result);
    entry.insertion = Some(match insert(&entry.transcript_text) {
        Ok(outcome) => InsertionDiagnostics::succeeded(
            outcome.selection.backend_name,
            outcome.target_application_name,
        )
        .with_target_class(outcome.target_class),
        Err(error) => {
            InsertionDiagnostics::failed(FRIENDLY_INSERT_BACKEND_NAME, "unknown", error.to_string())
        }
    });
    archive_transcript_entry(entry)
}

fn transcript_entry_from_result(result: TranscriptionResult) -> TranscriptEntry {
    TranscriptEntry::new(
        result.wav_path,
        result.transcript_text,
        result.backend_name,
        result.model_name,
        std::time::Duration::from_millis(result.elapsed_ms),
    )
}

fn archive_transcript_entry(
    entry: TranscriptEntry,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    let log = TranscriptLog::open(state_root())?;
    log.append(&entry)?;
    Ok(entry)
}

fn describe_asr_error(error: &TranscriptionError) -> String {
    match error {
        TranscriptionError::MissingWavFile(path) => {
            format!("WAV file does not exist: {}", path.display())
        }
        TranscriptionError::IncompleteModelDir {
            model_dir,
            missing_file,
        } => format!(
            "model bundle is missing {} in {}",
            missing_file,
            model_dir.display()
        ),
        TranscriptionError::InvalidWaveFile(path) => {
            format!("invalid WAV file: {}", path.display())
        }
        TranscriptionError::RecognizerInitializationFailed(model_dir) => format!(
            "failed to initialize recognizer from {}",
            model_dir.display()
        ),
        TranscriptionError::DecodeFailed(path) => {
            format!("failed to decode {}", path.display())
        }
    }
}

fn configured_model_dir() -> Result<PathBuf, TranscriptionRunError> {
    nonempty_env_path("PEPPERX_PARAKEET_MODEL_DIR").ok_or(TranscriptionRunError::MissingModelDir)
}

#[cfg(test)]
mod app_shell {
    use super::*;
    use crate::transcript_log::{env_lock, TranscriptLog};

    #[test]
    fn transcription_run_rejects_empty_model_dir_override() {
        let previous_model_dir = std::env::var_os("PEPPERX_PARAKEET_MODEL_DIR");
        std::env::set_var("PEPPERX_PARAKEET_MODEL_DIR", "");

        let error = configured_model_dir().unwrap_err();

        assert!(matches!(error, TranscriptionRunError::MissingModelDir));
        match previous_model_dir {
            Some(previous_model_dir) => {
                std::env::set_var("PEPPERX_PARAKEET_MODEL_DIR", previous_model_dir)
            }
            None => std::env::remove_var("PEPPERX_PARAKEET_MODEL_DIR"),
        }
    }

    #[test]
    fn app_shell_archives_friendly_insert_success_diagnostics() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-friendly-insert-success-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_friendly_insert(
            TranscriptionResult {
                wav_path: state_root.join("loop2.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            |_| {
                Ok(FriendlyInsertOutcome {
                    selection: pepperx_platform_gnome::atspi::FriendlyInsertSelection {
                        backend_name: "atspi-editable-text",
                        target_application_id: "org.gnome.TextEditor".into(),
                    },
                    target_application_name: "Text Editor".into(),
                    target_class: "text-editor".into(),
                    caret_offset: 5,
                    before_text: "hello from pepper x".into(),
                    after_text: "hello from pepper x".into(),
                })
            },
        )
        .expect("archive entry");

        assert_eq!(
            entry.insertion,
            Some(
                InsertionDiagnostics::succeeded("atspi-editable-text", "Text Editor")
                    .with_target_class("text-editor")
            )
        );
        assert_eq!(
            TranscriptLog::open(&state_root)
                .expect("open transcript log")
                .recent_entries()
                .expect("load entries"),
            vec![entry.clone()]
        );
        match previous_state_root {
            Some(previous_state_root) => {
                std::env::set_var("PEPPERX_STATE_ROOT", previous_state_root)
            }
            None => std::env::remove_var("PEPPERX_STATE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn app_shell_archives_friendly_insert_failure_diagnostics() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-friendly-insert-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_friendly_insert(
            TranscriptionResult {
                wav_path: state_root.join("loop2.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            |_| Err(FriendlyInsertRunError::MissingFocusedTarget),
        )
        .expect("archive entry");

        assert_eq!(
            entry.insertion,
            Some(InsertionDiagnostics::failed(
                FRIENDLY_INSERT_BACKEND_NAME,
                "unknown",
                "friendly insertion could not find a focused target"
            ))
        );
        assert_eq!(
            TranscriptLog::open(&state_root)
                .expect("open transcript log")
                .recent_entries()
                .expect("load entries"),
            vec![entry.clone()]
        );
        match previous_state_root {
            Some(previous_state_root) => {
                std::env::set_var("PEPPERX_STATE_ROOT", previous_state_root)
            }
            None => std::env::remove_var("PEPPERX_STATE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(state_root);
    }
}
