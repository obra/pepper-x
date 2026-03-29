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
        .with_target_class(outcome.target_class)
        .with_attempted_backends(outcome.selection.attempted_backends.iter().copied()),
        Err(error) => match error.selected_backend() {
            Some(selection) => InsertionDiagnostics::failed(
                selection.backend_name,
                error.target_application_name().unwrap_or("unknown"),
                error.to_string(),
            )
            .with_target_class(selection.target_class)
            .with_attempted_backends(selection.attempted_backends.iter().copied()),
            None => {
                let mut diagnostics = InsertionDiagnostics::failed(
                FRIENDLY_INSERT_BACKEND_NAME,
                error.target_application_name().unwrap_or("unknown"),
                error.to_string(),
                );

                if let Some(target_class) = error.target_class() {
                    diagnostics = diagnostics.with_target_class(target_class);
                }

                if !error.attempted_backends().is_empty() {
                    diagnostics = diagnostics
                        .with_attempted_backends(error.attempted_backends().iter().copied());
                }

                diagnostics
            }
        },
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
                        target_class: "text-editor",
                        attempted_backends: vec!["atspi-editable-text"],
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
                    .with_attempted_backends(["atspi-editable-text"])
            )
        );
        assert_eq!(
            TranscriptLog::open(&state_root)
                .expect("open transcript log")
                .recent_entries()
                .expect("load entries"),
            vec![entry.clone()]
        );
        let raw = std::fs::read_to_string(state_root.join("transcript-log.jsonl"))
            .expect("read transcript log");
        assert!(raw.contains("\"attempted_backends\":[\"atspi-editable-text\"]"));
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
        let raw = std::fs::read_to_string(state_root.join("transcript-log.jsonl"))
            .expect("read transcript log");
        assert!(!raw.contains("\"attempted_backends\":"));
        match previous_state_root {
            Some(previous_state_root) => {
                std::env::set_var("PEPPERX_STATE_ROOT", previous_state_root)
            }
            None => std::env::remove_var("PEPPERX_STATE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn app_shell_archives_selected_backend_failure_diagnostics() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-selected-backend-failure-{}-{}",
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
                wav_path: state_root.join("loop4.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            |_| {
                Err(FriendlyInsertRunError::SelectedBackendFailure {
                    selection: pepperx_platform_gnome::atspi::FriendlyInsertSelection {
                        backend_name: pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                        target_application_id: "ghostty".into(),
                        target_class: "terminal",
                        attempted_backends: vec![
                            FRIENDLY_INSERT_BACKEND_NAME,
                            pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                        ],
                    },
                    target_application_name: "Ghostty".into(),
                    reason: Box::new(FriendlyInsertRunError::ReadbackMismatch),
                })
            },
        )
        .expect("archive entry");

        assert_eq!(
            entry.insertion,
            Some(
                InsertionDiagnostics::failed(
                    pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                    "Ghostty",
                    "friendly insertion readback did not match the requested text"
                )
                .with_target_class("terminal")
                .with_attempted_backends([
                    FRIENDLY_INSERT_BACKEND_NAME,
                    pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                ])
            )
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
    fn app_shell_archives_unsupported_target_failure_application_name() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-unsupported-target-failure-{}-{}",
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
                wav_path: state_root.join("loop4.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            |_| {
                Err(FriendlyInsertRunError::UnsupportedTarget(
                    pepperx_platform_gnome::atspi::FriendlyInsertFailure {
                        backend_name: FRIENDLY_INSERT_BACKEND_NAME,
                        reason:
                            pepperx_platform_gnome::atspi::FriendlyInsertError::UnsupportedApplication {
                                expected_application_id: "org.gnome.TextEditor".into(),
                                actual_application_id: "org.gnome.Calculator".into(),
                            },
                        target_application_name: Some("Calculator".into()),
                        target_class: Some("unsupported"),
                        attempted_backends: Vec::new(),
                    },
                ))
            },
        )
        .expect("archive entry");

        assert_eq!(
            entry.insertion,
            Some(InsertionDiagnostics::failed(
                FRIENDLY_INSERT_BACKEND_NAME,
                "Calculator",
                "atspi-editable-text: friendly insertion target application id org.gnome.Calculator does not match org.gnome.TextEditor"
            )
            .with_target_class("unsupported"))
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
