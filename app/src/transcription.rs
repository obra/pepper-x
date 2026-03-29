use serde::{Deserialize, Serialize};
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use pepperx_asr::{transcribe_wav, TranscriptionError, TranscriptionRequest, TranscriptionResult};
use pepperx_platform_gnome::atspi::{
    insert_text_into_friendly_target, FriendlyInsertOutcome, FriendlyInsertPolicy,
    FriendlyInsertRunError, FRIENDLY_INSERT_BACKEND_NAME, UINPUT_TEXT_BACKEND_NAME,
};

use crate::transcript_log::{
    nonempty_env_path, state_root, InsertionDiagnostics, TranscriptEntry, TranscriptLog,
};

const MODEL_NAME: &str = "nemo-parakeet-tdt-0.6b-v2-int8";
const FRIENDLY_TARGET_APPLICATION_ID: &str = "org.gnome.TextEditor";
const DEFAULT_UINPUT_HELPER_BIN: &str = "/usr/libexec/pepper-x/pepperx-uinput-helper";
const UINPUT_HELPER_STARTUP_TIMEOUT: Duration = Duration::from_millis(500);
const UINPUT_HELPER_STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct UinputInsertRequest {
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct UinputInsertResponse {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

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
        insert_with_uinput_fallback(
            transcript_text,
            |transcript_text| {
                insert_text_into_friendly_target(
                    transcript_text,
                    &FriendlyInsertPolicy {
                        target_application_id: FRIENDLY_TARGET_APPLICATION_ID,
                    },
                )
            },
            request_uinput_text_insertion,
        )
    })
}

fn insert_with_uinput_fallback<P, H>(
    text: &str,
    platform_insert: P,
    helper_insert: H,
) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError>
where
    P: FnOnce(&str) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError>,
    H: FnOnce(UinputInsertRequest) -> Result<(), FriendlyInsertRunError>,
{
    match platform_insert(text) {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            let Some(selection) = error.selected_backend().cloned() else {
                return Err(error);
            };

            if selection.backend_name != UINPUT_TEXT_BACKEND_NAME {
                return Err(error);
            }

            let target_application_name = error.target_application_name().unwrap_or("unknown");
            helper_insert(UinputInsertRequest { text: text.into() }).map_err(|helper_error| {
                FriendlyInsertRunError::SelectedBackendFailure {
                    selection: selection.clone(),
                    target_application_name: target_application_name.into(),
                    reason: Box::new(helper_error),
                }
            })?;
            let target_class = selection.target_class.to_string();

            Ok(FriendlyInsertOutcome {
                selection,
                target_application_name: target_application_name.into(),
                target_class,
                caret_offset: -1,
                before_text: String::new(),
                after_text: String::new(),
            })
        }
    }
}

fn request_uinput_text_insertion(
    request: UinputInsertRequest,
) -> Result<(), FriendlyInsertRunError> {
    let socket_path = configured_uinput_helper_socket_path()?;
    let mut stream = connect_or_spawn_uinput_helper(&socket_path)?;
    serde_json::to_writer(&mut stream, &request).map_err(|error| {
        FriendlyInsertRunError::Access(format!(
            "failed to encode Pepper X uinput request: {error}"
        ))
    })?;
    stream.write_all(b"\n").map_err(|error| {
        FriendlyInsertRunError::Access(format!(
            "failed to write Pepper X uinput request: {error}"
        ))
    })?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| {
            FriendlyInsertRunError::Access(format!(
                "failed to finish Pepper X uinput request: {error}"
            ))
        })?;

    let response: UinputInsertResponse =
        serde_json::from_reader(BufReader::new(stream)).map_err(|error| {
            FriendlyInsertRunError::Access(format!(
                "failed to decode Pepper X uinput response: {error}"
            ))
        })?;

    match response.error {
        Some(error) => Err(FriendlyInsertRunError::Access(format!(
            "Pepper X uinput helper failed: {error}"
        ))),
        None if response.ok => Ok(()),
        None => Err(FriendlyInsertRunError::Access(
            "Pepper X uinput helper returned an empty response".into(),
        )),
    }
}

fn connect_or_spawn_uinput_helper(socket_path: &Path) -> Result<UnixStream, FriendlyInsertRunError> {
    match UnixStream::connect(socket_path) {
        Ok(stream) => return Ok(stream),
        Err(initial_error) if initial_error.kind() != std::io::ErrorKind::NotFound => {
            return Err(FriendlyInsertRunError::Access(format!(
                "failed to connect to Pepper X uinput helper at {}: {initial_error}",
                socket_path.display()
            )));
        }
        Err(_) => {}
    }

    let helper_bin = configured_uinput_helper_bin_path();
    Command::new(&helper_bin)
        .spawn()
        .map_err(|error| {
            FriendlyInsertRunError::Access(format!(
                "failed to launch Pepper X uinput helper {}: {error}",
                helper_bin.display()
            ))
        })?;

    let deadline = std::time::Instant::now() + UINPUT_HELPER_STARTUP_TIMEOUT;
    loop {
        match UnixStream::connect(socket_path) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if std::time::Instant::now() < deadline
                    && matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::NotFound
                    ) =>
            {
                std::thread::sleep(UINPUT_HELPER_STARTUP_POLL_INTERVAL);
            }
            Err(error) => {
                return Err(FriendlyInsertRunError::Access(format!(
                    "Pepper X uinput helper did not become ready at {}: {error}",
                    socket_path.display()
                )));
            }
        }
    }
}

fn configured_uinput_helper_bin_path() -> PathBuf {
    nonempty_env_path("PEPPERX_UINPUT_HELPER_BIN")
        .unwrap_or_else(|| PathBuf::from(DEFAULT_UINPUT_HELPER_BIN))
}

fn configured_uinput_helper_socket_path() -> Result<PathBuf, FriendlyInsertRunError> {
    if let Some(socket_path) = nonempty_env_path("PEPPERX_UINPUT_HELPER_SOCKET") {
        return Ok(socket_path);
    }

    let runtime_dir = nonempty_env_path("XDG_RUNTIME_DIR").ok_or_else(|| {
        FriendlyInsertRunError::Access(
            "Pepper X uinput fallback requires XDG_RUNTIME_DIR or PEPPERX_UINPUT_HELPER_SOCKET"
                .into(),
        )
    })?;

    Ok(runtime_dir.join("pepper-x").join("uinput-helper.sock"))
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
    fn app_shell_archives_clipboard_insert_success_diagnostics() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-clipboard-insert-success-{}-{}",
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
                transcript_text: "paste through clipboard".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            |_| {
                Ok(FriendlyInsertOutcome {
                    selection: pepperx_platform_gnome::atspi::FriendlyInsertSelection {
                        backend_name: pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                        target_application_id: "firefox".into(),
                        target_class: "browser-textarea",
                        attempted_backends: vec![
                            pepperx_platform_gnome::atspi::FRIENDLY_INSERT_BACKEND_NAME,
                            pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                        ],
                    },
                    target_application_name: "Firefox".into(),
                    target_class: "browser-textarea".into(),
                    caret_offset: -1,
                    before_text: String::new(),
                    after_text: String::new(),
                })
            },
        )
        .expect("archive entry");

        assert_eq!(
            entry.insertion,
            Some(
                InsertionDiagnostics::succeeded(
                    pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                    "Firefox",
                )
                .with_target_class("browser-textarea")
                .with_attempted_backends([
                    pepperx_platform_gnome::atspi::FRIENDLY_INSERT_BACKEND_NAME,
                    pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
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
    fn uinput_insert_routes_selected_uinput_backend_to_helper() {
        let mut helper_requests = Vec::new();

        let outcome = insert_with_uinput_fallback(
            "hello hostile target",
            |_| {
                Err(FriendlyInsertRunError::SelectedBackendFailure {
                    selection: pepperx_platform_gnome::atspi::FriendlyInsertSelection {
                        backend_name: UINPUT_TEXT_BACKEND_NAME,
                        target_application_id: "wine".into(),
                        target_class: "hostile",
                        attempted_backends: vec![
                            pepperx_platform_gnome::atspi::FRIENDLY_INSERT_BACKEND_NAME,
                            pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                            pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                            UINPUT_TEXT_BACKEND_NAME,
                        ],
                    },
                    target_application_name: "Wine".into(),
                    reason: Box::new(FriendlyInsertRunError::Access(
                        "friendly insertion backend uinput-text is not implemented yet".into(),
                    )),
                })
            },
            |request| {
                helper_requests.push(request.text.clone());
                Ok(())
            },
        )
        .expect("uinput helper should recover the last fallback");

        assert_eq!(helper_requests, vec!["hello hostile target".to_string()]);
        assert_eq!(outcome.selection.backend_name, UINPUT_TEXT_BACKEND_NAME);
        assert_eq!(outcome.target_application_name, "Wine");
        assert_eq!(outcome.target_class, "hostile");
        assert_eq!(
            outcome.selection.attempted_backends,
            vec![
                pepperx_platform_gnome::atspi::FRIENDLY_INSERT_BACKEND_NAME,
                pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                UINPUT_TEXT_BACKEND_NAME,
            ]
        );
    }

    #[test]
    fn uinput_insert_skips_helper_when_platform_backend_is_not_uinput() {
        let error = insert_with_uinput_fallback(
            "leave this alone",
            |_| {
                Err(FriendlyInsertRunError::SelectedBackendFailure {
                    selection: pepperx_platform_gnome::atspi::FriendlyInsertSelection {
                        backend_name: pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                        target_application_id: "firefox".into(),
                        target_class: "browser-textarea",
                        attempted_backends: vec![
                            pepperx_platform_gnome::atspi::FRIENDLY_INSERT_BACKEND_NAME,
                            pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                        ],
                    },
                    target_application_name: "Firefox".into(),
                    reason: Box::new(FriendlyInsertRunError::Access(
                        "clipboard mediation failed".into(),
                    )),
                })
            },
            |_| panic!("helper should not run before the uinput fallback"),
        )
        .unwrap_err();

        assert_eq!(
            error.selected_backend().expect("selected backend").backend_name,
            pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME
        );
        assert_eq!(error.target_application_name(), Some("Firefox"));
    }

    #[test]
    fn uinput_insert_request_serializes_text_only_payload() {
        let payload = serde_json::to_string(&UinputInsertRequest {
            text: "hello wine".into(),
        })
        .expect("serialize request");

        assert_eq!(payload, r#"{"text":"hello wine"}"#);
        assert!(!payload.contains("keycode"));
        assert!(!payload.contains("modifier"));
        assert!(!payload.contains("shortcut"));
    }

    #[test]
    fn uinput_insert_uses_libexec_helper_path_by_default() {
        let previous_helper_bin = std::env::var_os("PEPPERX_UINPUT_HELPER_BIN");
        std::env::remove_var("PEPPERX_UINPUT_HELPER_BIN");

        let helper_path = configured_uinput_helper_bin_path();

        assert_eq!(
            helper_path,
            PathBuf::from("/usr/libexec/pepper-x/pepperx-uinput-helper")
        );

        match previous_helper_bin {
            Some(previous_helper_bin) => {
                std::env::set_var("PEPPERX_UINPUT_HELPER_BIN", previous_helper_bin)
            }
            None => std::env::remove_var("PEPPERX_UINPUT_HELPER_BIN"),
        }
    }

    #[test]
    fn uinput_insert_archives_last_fallback_diagnostics() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-uinput-insert-success-{}-{}",
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
                wav_path: state_root.join("loop4-uinput.wav"),
                transcript_text: "type into wine".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            |_| {
                insert_with_uinput_fallback(
                    "type into wine",
                    |_| {
                        Err(FriendlyInsertRunError::SelectedBackendFailure {
                            selection: pepperx_platform_gnome::atspi::FriendlyInsertSelection {
                                backend_name: UINPUT_TEXT_BACKEND_NAME,
                                target_application_id: "wine".into(),
                                target_class: "hostile",
                                attempted_backends: vec![
                                    pepperx_platform_gnome::atspi::FRIENDLY_INSERT_BACKEND_NAME,
                                    pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                                    pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                                    UINPUT_TEXT_BACKEND_NAME,
                                ],
                            },
                            target_application_name: "Wine".into(),
                            reason: Box::new(FriendlyInsertRunError::Access(
                                "friendly insertion backend uinput-text is not implemented yet"
                                    .into(),
                            )),
                        })
                    },
                    |_| Ok(()),
                )
            },
        )
        .expect("archive entry");

        assert_eq!(
            entry.insertion,
            Some(
                InsertionDiagnostics::succeeded(UINPUT_TEXT_BACKEND_NAME, "Wine")
                    .with_target_class("hostile")
                    .with_attempted_backends([
                        pepperx_platform_gnome::atspi::FRIENDLY_INSERT_BACKEND_NAME,
                        pepperx_platform_gnome::atspi::STRING_INJECTION_BACKEND_NAME,
                        pepperx_platform_gnome::atspi::CLIPBOARD_PASTE_BACKEND_NAME,
                        UINPUT_TEXT_BACKEND_NAME,
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
