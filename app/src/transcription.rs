use serde::{Deserialize, Serialize};
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use pepperx_asr::{transcribe_wav, TranscriptionError, TranscriptionRequest, TranscriptionResult};
use pepperx_cleanup::{run_cleanup, CleanupError, CleanupRequest, CleanupResult};
use pepperx_corrections::{learn_correction, CorrectionStore};
use pepperx_models::{catalog_model, default_cache_root, model_readiness, ModelKind};
use pepperx_platform_gnome::atspi::{
    insert_text_into_friendly_target, FriendlyInsertOutcome, FriendlyInsertPolicy,
    FriendlyInsertRunError, FRIENDLY_INSERT_BACKEND_NAME, UINPUT_TEXT_BACKEND_NAME,
};
use pepperx_platform_gnome::context::{capture_supporting_context, SupportingContext};

use crate::history_store::{ArchiveWriteRequest, HistoryStore};
use crate::settings::{corrections_store_path, AppSettings};
use crate::transcript_log::{
    nonempty_env_path, state_root, CleanupDiagnostics, InsertionDiagnostics, LearningDiagnostics,
    TranscriptEntry,
};

#[cfg(test)]
const MODEL_NAME: &str = "nemo-parakeet-tdt-0.6b-v2-int8";
const FRIENDLY_TARGET_APPLICATION_ID: &str = "org.gnome.TextEditor";
const DEFAULT_UINPUT_HELPER_BIN: &str = "/usr/libexec/pepper-x/pepperx-uinput-helper";
const UINPUT_HELPER_STARTUP_TIMEOUT: Duration = Duration::from_millis(500);
const UINPUT_HELPER_STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(25);
const DISABLE_CONTEXT_CAPTURE_ENV: &str = "PEPPERX_DISABLE_CONTEXT_CAPTURE";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedRunRerunRequest {
    pub run_id: String,
    pub asr_model_id: Option<String>,
    pub cleanup_model_id: Option<String>,
    pub cleanup_prompt_profile: Option<String>,
}

#[derive(Debug)]
pub enum TranscriptionRunError {
    UnknownAsrModel(String),
    UnreadyAsrModel {
        model_id: String,
        install_path: PathBuf,
        missing_files: Vec<String>,
    },
    ArchivedRunNotFound(String),
    ArchivedRunMissingSourceWav(String),
    TranscriptLog(std::io::Error),
    Asr(TranscriptionError),
    FriendlyInsert(FriendlyInsertRunError),
    LiveRecording(String),
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

impl From<FriendlyInsertRunError> for TranscriptionRunError {
    fn from(error: FriendlyInsertRunError) -> Self {
        Self::FriendlyInsert(error)
    }
}

impl std::fmt::Display for TranscriptionRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAsrModel(model_id) => {
                write!(f, "Pepper X ASR model is not supported: {model_id}")
            }
            Self::UnreadyAsrModel {
                model_id,
                install_path,
                missing_files,
            } => write!(
                f,
                "Pepper X ASR model {model_id} is not ready at {}: missing {}",
                install_path.display(),
                missing_files.join(", ")
            ),
            Self::ArchivedRunNotFound(run_id) => {
                write!(f, "Pepper X archived run was not found: {run_id}")
            }
            Self::ArchivedRunMissingSourceWav(run_id) => write!(
                f,
                "Pepper X archived run is missing its source WAV: {run_id}"
            ),
            Self::TranscriptLog(error) => {
                write!(f, "failed to write Pepper X transcript log: {error}")
            }
            Self::Asr(error) => write!(
                f,
                "Pepper X transcription failed: {}",
                describe_asr_error(error)
            ),
            Self::FriendlyInsert(error) => {
                write!(f, "Pepper X friendly insertion failed: {error}")
            }
            Self::LiveRecording(error) => write!(f, "Pepper X live recording failed: {error}"),
        }
    }
}

pub fn transcribe_wav_to_log(wav_path: &Path) -> Result<TranscriptEntry, TranscriptionRunError> {
    let result = transcribe_wav_result(wav_path)?;
    archive_transcription_result(result)
}

pub fn transcribe_recorded_wav_to_log(
    wav_path: &Path,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    transcribe_wav_to_log(wav_path)
}

pub fn transcribe_wav_and_cleanup_to_log(
    wav_path: &Path,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    let settings = AppSettings::load_or_default();
    let prompt_profile = settings.cleanup_prompt_profile.clone();
    let cache_root = default_cache_root();
    let supporting_context = capture_cleanup_context();
    let result = transcribe_wav_result(wav_path)?;
    archive_transcription_result_with_cleanup(
        result,
        Some(prompt_profile.clone()),
        supporting_context.clone(),
        move |transcript_text| {
            let model_path = configured_cleanup_model_path_with(&settings, &cache_root)?;
            let correction_memory_text = load_correction_store().prompt_memory_text();
            run_cleanup(&CleanupRequest {
                transcript_text: transcript_text.into(),
                model_path,
                supporting_context_text: supporting_context.supporting_context_text.clone(),
                ocr_text: supporting_context.ocr_text.clone(),
                correction_memory_text,
                prompt_profile: prompt_profile.clone(),
            })
        },
    )
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

pub fn transcribe_wav_and_cleanup_and_insert_friendly_to_log(
    wav_path: &Path,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    let settings = AppSettings::load_or_default();
    let prompt_profile = settings.cleanup_prompt_profile.clone();
    let cache_root = default_cache_root();
    let supporting_context = capture_cleanup_context();
    let result = transcribe_wav_result(wav_path)?;
    archive_transcription_result_with_cleanup_and_friendly_insert(
        result,
        Some(prompt_profile.clone()),
        supporting_context.clone(),
        move |transcript_text| {
            let model_path = configured_cleanup_model_path_with(&settings, &cache_root)?;
            let correction_memory_text = load_correction_store().prompt_memory_text();
            run_cleanup(&CleanupRequest {
                transcript_text: transcript_text.into(),
                model_path,
                supporting_context_text: supporting_context.supporting_context_text.clone(),
                ocr_text: supporting_context.ocr_text.clone(),
                correction_memory_text,
                prompt_profile: prompt_profile.clone(),
            })
        },
        |transcript_text| {
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
        },
    )
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
        FriendlyInsertRunError::Access(format!("failed to encode Pepper X uinput request: {error}"))
    })?;
    stream.write_all(b"\n").map_err(|error| {
        FriendlyInsertRunError::Access(format!("failed to write Pepper X uinput request: {error}"))
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

fn connect_or_spawn_uinput_helper(
    socket_path: &Path,
) -> Result<UnixStream, FriendlyInsertRunError> {
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
    Command::new(&helper_bin).spawn().map_err(|error| {
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
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
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
    let settings = AppSettings::load_or_default();
    transcribe_wav_result_with_model_id(wav_path, &settings.preferred_asr_model)
}

fn transcribe_wav_result_with_model_id(
    wav_path: &Path,
    model_id: &str,
) -> Result<TranscriptionResult, TranscriptionRunError> {
    let model_dir = configured_model_dir_for_model_id(model_id)?;
    let request = TranscriptionRequest::new(wav_path, &model_dir, model_id);
    Ok(transcribe_wav(&request)?)
}

pub fn rerun_archived_run_to_log(
    request: ArchivedRunRerunRequest,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    let cache_root = default_cache_root();
    let cleanup_cache_root = cache_root.clone();
    let explicit_asr_model = request.asr_model_id.is_some();
    let explicit_cleanup_model = request.cleanup_model_id.is_some();
    rerun_archived_run_with(
        request,
        move |wav_path, model_id| {
            let model_dir = if explicit_asr_model {
                configured_requested_model_dir_for_model_id_with(model_id, &cache_root)?
            } else {
                configured_model_dir_for_model_id(model_id)?
            };
            let request = TranscriptionRequest::new(wav_path, &model_dir, model_id);
            Ok(transcribe_wav(&request)?)
        },
        move |cleanup_model_id, request| {
            let request = CleanupRequest {
                model_path: if explicit_cleanup_model {
                    configured_requested_cleanup_model_path_for_model_id_with(
                        cleanup_model_id,
                        &cleanup_cache_root,
                    )?
                } else {
                    configured_cleanup_model_path_for_model_id(
                        cleanup_model_id,
                        &cleanup_cache_root,
                    )?
                },
                ..request
            };
            run_cleanup(&request)
        },
    )
}

fn rerun_archived_run_with<T, C>(
    request: ArchivedRunRerunRequest,
    transcribe: T,
    cleanup: C,
) -> Result<TranscriptEntry, TranscriptionRunError>
where
    T: FnOnce(&Path, &str) -> Result<TranscriptionResult, TranscriptionRunError>,
    C: FnOnce(&str, CleanupRequest) -> Result<CleanupResult, CleanupError>,
{
    let store = HistoryStore::open(state_root())?;
    let original_run = store
        .load_run(&request.run_id)?
        .ok_or_else(|| TranscriptionRunError::ArchivedRunNotFound(request.run_id.clone()))?;
    let archived_source_wav_path =
        original_run
            .archived_source_wav_path
            .clone()
            .ok_or_else(|| {
                TranscriptionRunError::ArchivedRunMissingSourceWav(original_run.run_id.clone())
            })?;
    let asr_model_id = request
        .asr_model_id
        .clone()
        .unwrap_or_else(|| original_run.entry.model_name.clone());
    let result = transcribe(&archived_source_wav_path, &asr_model_id)?;
    let mut entry = transcript_entry_from_result(result);
    let cleanup_model_id = request.cleanup_model_id.clone().or_else(|| {
        original_run
            .entry
            .cleanup
            .as_ref()
            .map(|cleanup| cleanup.model_name.clone())
    });
    let prompt_profile = request
        .cleanup_prompt_profile
        .clone()
        .or_else(|| original_run.prompt_profile.clone());
    let supporting_context = SupportingContext {
        supporting_context_text: original_run.supporting_context_text.clone(),
        ocr_text: original_run.ocr_text.clone(),
        used_ocr: original_run
            .entry
            .cleanup
            .as_ref()
            .map(|cleanup| cleanup.used_ocr)
            .unwrap_or(false),
    };

    if let Some(cleanup_model_id) = cleanup_model_id.as_deref() {
        let cleanup_request = CleanupRequest {
            transcript_text: entry.transcript_text.clone(),
            model_path: PathBuf::new(),
            supporting_context_text: original_run.supporting_context_text.clone(),
            ocr_text: original_run.ocr_text.clone(),
            correction_memory_text: load_correction_store().prompt_memory_text(),
            prompt_profile: prompt_profile
                .clone()
                .unwrap_or_else(|| AppSettings::load_or_default().cleanup_prompt_profile),
        };
        record_cleanup(&mut entry, &supporting_context, |transcript_text| {
            let request = CleanupRequest {
                transcript_text: transcript_text.into(),
                ..cleanup_request
            };
            cleanup(cleanup_model_id, request)
        });
    }

    archive_transcript_entry_with_request(
        entry,
        prompt_profile,
        Some(&supporting_context),
        Some(original_run.run_id),
    )
}

pub(crate) fn archive_transcription_result(
    result: TranscriptionResult,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    archive_transcript_entry(transcript_entry_from_result(result))
}

fn archive_transcription_result_with_cleanup<F>(
    result: TranscriptionResult,
    prompt_profile: Option<String>,
    supporting_context: SupportingContext,
    cleanup: F,
) -> Result<TranscriptEntry, TranscriptionRunError>
where
    F: FnOnce(&str) -> Result<CleanupResult, CleanupError>,
{
    archive_transcription_result_with_cleanup_context(
        result,
        prompt_profile,
        supporting_context,
        cleanup,
    )
}

fn archive_transcription_result_with_cleanup_context<F>(
    result: TranscriptionResult,
    prompt_profile: Option<String>,
    supporting_context: SupportingContext,
    cleanup: F,
) -> Result<TranscriptEntry, TranscriptionRunError>
where
    F: FnOnce(&str) -> Result<CleanupResult, CleanupError>,
{
    let mut entry = transcript_entry_from_result(result);
    record_cleanup(&mut entry, &supporting_context, cleanup);
    archive_transcript_entry_with_request(entry, prompt_profile, Some(&supporting_context), None)
}

pub(crate) fn archive_transcription_result_with_friendly_insert<F>(
    result: TranscriptionResult,
    insert: F,
) -> Result<TranscriptEntry, TranscriptionRunError>
where
    F: FnOnce(&str) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError>,
{
    let mut entry = transcript_entry_from_result(result);
    let insert_text = entry.transcript_text.clone();
    let _ = record_friendly_insert(&mut entry, &insert_text, insert);
    archive_transcript_entry(entry)
}

fn archive_transcription_result_with_cleanup_and_friendly_insert<C, I>(
    result: TranscriptionResult,
    prompt_profile: Option<String>,
    supporting_context: SupportingContext,
    cleanup: C,
    insert: I,
) -> Result<TranscriptEntry, TranscriptionRunError>
where
    C: FnOnce(&str) -> Result<CleanupResult, CleanupError>,
    I: FnOnce(&str) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError>,
{
    let mut entry = transcript_entry_from_result(result);
    record_cleanup(&mut entry, &supporting_context, cleanup);
    let insert_text = entry.display_text().to_string();
    let insert_error = record_friendly_insert(&mut entry, &insert_text, insert).err();
    let entry = archive_transcript_entry_with_request(
        entry,
        prompt_profile,
        Some(&supporting_context),
        None,
    )?;

    match insert_error {
        Some(error) => Err(TranscriptionRunError::FriendlyInsert(error)),
        None => Ok(entry),
    }
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
    archive_transcript_entry_with_request(entry, None, None, None)
}

fn archive_transcript_entry_with_request(
    entry: TranscriptEntry,
    prompt_profile: Option<String>,
    supporting_context: Option<&SupportingContext>,
    parent_run_id: Option<String>,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    let store = HistoryStore::open(state_root())?;
    let mut request = ArchiveWriteRequest::new(entry.clone());
    if let Some(parent_run_id) = parent_run_id {
        request = request.with_parent_run_id(parent_run_id);
    }
    if let Some(prompt_profile) = prompt_profile {
        request = request.with_prompt_profile(prompt_profile);
    }
    if let Some(supporting_context) = supporting_context {
        if let Some(supporting_context_text) = supporting_context.supporting_context_text.as_ref() {
            request = request.with_supporting_context(supporting_context_text.clone());
        }
        if let Some(ocr_text) = supporting_context.ocr_text.as_ref() {
            request = request.with_ocr_text(ocr_text.clone());
        }
    }
    store.archive_run(&request)?;
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
    if let Some(model_dir) = nonempty_env_path("PEPPERX_PARAKEET_MODEL_DIR") {
        return Ok(model_dir);
    }

    let settings = AppSettings::load_or_default();
    configured_model_dir_for_model_id_with(&settings.preferred_asr_model, &default_cache_root())
}

#[cfg(test)]
fn configured_cleanup_model_path() -> Result<PathBuf, CleanupError> {
    if let Some(model_path) = nonempty_env_path("PEPPERX_CLEANUP_MODEL_PATH") {
        return Ok(model_path);
    }

    let settings = AppSettings::load_or_default();
    configured_cleanup_model_path_for_model_id_with(
        &settings.preferred_cleanup_model,
        &default_cache_root(),
    )
}

fn configured_model_dir_for_model_id(model_id: &str) -> Result<PathBuf, TranscriptionRunError> {
    resolve_model_dir_for_model_id_with(model_id, &default_cache_root(), true)
}

fn configured_cleanup_model_path_for_model_id(
    model_id: &str,
    cache_root: &Path,
) -> Result<PathBuf, CleanupError> {
    resolve_cleanup_model_path_for_model_id_with(model_id, cache_root, true)
}

fn configured_requested_model_dir_for_model_id_with(
    model_id: &str,
    cache_root: &Path,
) -> Result<PathBuf, TranscriptionRunError> {
    resolve_model_dir_for_model_id_with(model_id, cache_root, false)
}

fn configured_requested_cleanup_model_path_for_model_id_with(
    model_id: &str,
    cache_root: &Path,
) -> Result<PathBuf, CleanupError> {
    resolve_cleanup_model_path_for_model_id_with(model_id, cache_root, false)
}

fn configured_model_dir_with(
    settings: &AppSettings,
    cache_root: &Path,
) -> Result<PathBuf, TranscriptionRunError> {
    configured_model_dir_for_model_id_with(&settings.preferred_asr_model, cache_root)
}

fn configured_model_dir_for_model_id_with(
    model_id: &str,
    cache_root: &Path,
) -> Result<PathBuf, TranscriptionRunError> {
    resolve_model_dir_for_model_id_with(model_id, cache_root, true)
}

fn resolve_model_dir_for_model_id_with(
    model_id: &str,
    cache_root: &Path,
    allow_env_override: bool,
) -> Result<PathBuf, TranscriptionRunError> {
    if allow_env_override {
        if let Some(model_dir) = nonempty_env_path("PEPPERX_PARAKEET_MODEL_DIR") {
            return Ok(model_dir);
        }
    }

    let model = catalog_model(model_id)
        .ok_or_else(|| TranscriptionRunError::UnknownAsrModel(model_id.into()))?;
    if model.kind != ModelKind::Asr {
        return Err(TranscriptionRunError::UnknownAsrModel(model_id.into()));
    }
    let readiness = model_readiness(model, cache_root);
    if readiness.is_ready {
        Ok(readiness.install_path)
    } else {
        Err(TranscriptionRunError::UnreadyAsrModel {
            model_id: model.id.into(),
            install_path: readiness.install_path,
            missing_files: readiness.missing_files,
        })
    }
}

fn configured_cleanup_model_path_with(
    settings: &AppSettings,
    cache_root: &Path,
) -> Result<PathBuf, CleanupError> {
    configured_cleanup_model_path_for_model_id(&settings.preferred_cleanup_model, cache_root)
}

fn configured_cleanup_model_path_for_model_id_with(
    model_id: &str,
    cache_root: &Path,
) -> Result<PathBuf, CleanupError> {
    resolve_cleanup_model_path_for_model_id_with(model_id, cache_root, true)
}

fn resolve_cleanup_model_path_for_model_id_with(
    model_id: &str,
    cache_root: &Path,
    allow_env_override: bool,
) -> Result<PathBuf, CleanupError> {
    if allow_env_override {
        if let Some(model_path) = nonempty_env_path("PEPPERX_CLEANUP_MODEL_PATH") {
            return Ok(model_path);
        }
    }

    let model =
        catalog_model(model_id).ok_or_else(|| CleanupError::UnsupportedModel(model_id.into()))?;
    if model.kind != ModelKind::Cleanup {
        return Err(CleanupError::UnsupportedModel(model_id.into()));
    }
    let readiness = model_readiness(model, cache_root);
    if readiness.is_ready {
        Ok(readiness.install_path)
    } else {
        Err(CleanupError::MissingModelPath(readiness.install_path))
    }
}

fn record_cleanup<F>(
    entry: &mut TranscriptEntry,
    supporting_context: &SupportingContext,
    cleanup: F,
) where
    F: FnOnce(&str) -> Result<CleanupResult, CleanupError>,
{
    entry.cleanup = Some(match cleanup(&entry.transcript_text) {
        Ok(result) => cleanup_diagnostics_from_result(result),
        Err(error) => cleanup_diagnostics_from_error(&error, supporting_context.used_ocr),
    });
}

fn cleanup_diagnostics_from_result(result: CleanupResult) -> CleanupDiagnostics {
    let mut diagnostics = CleanupDiagnostics::succeeded(
        result.backend_name,
        result.model_name,
        result.cleaned_text,
        Duration::from_millis(result.elapsed_ms),
    );
    diagnostics.used_ocr = result.used_ocr;
    diagnostics
}

fn cleanup_diagnostics_from_error(
    error: &CleanupError,
    used_ocr_context: bool,
) -> CleanupDiagnostics {
    let mut diagnostics = CleanupDiagnostics::failed(
        error.backend_name(),
        error
            .model_name()
            .unwrap_or_else(|| String::from("unknown")),
        error.to_string(),
    );
    diagnostics.used_ocr = used_ocr_context;
    diagnostics
}

fn capture_cleanup_context() -> SupportingContext {
    if context_capture_is_disabled() {
        return SupportingContext::default();
    }

    match capture_supporting_context() {
        Ok(context) => context,
        Err(error) => {
            eprintln!("[Pepper X] failed to capture cleanup context: {error}");
            SupportingContext::default()
        }
    }
}

fn context_capture_is_disabled() -> bool {
    matches!(
        std::env::var(DISABLE_CONTEXT_CAPTURE_ENV).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

fn load_correction_store() -> CorrectionStore {
    let store_root = corrections_store_path();
    CorrectionStore::load(&store_root).unwrap_or_else(|error| {
        eprintln!(
            "[Pepper X] failed to load correction store {}: {error}",
            store_root.display()
        );
        CorrectionStore::new(store_root)
    })
}

fn record_friendly_insert<F>(
    entry: &mut TranscriptEntry,
    insert_text: &str,
    insert: F,
) -> Result<(), FriendlyInsertRunError>
where
    F: FnOnce(&str) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError>,
{
    match insert(insert_text) {
        Ok(outcome) => {
            entry.insertion = Some(insertion_diagnostics_from_outcome(outcome));
            entry.learning = persist_prompt_memory_update(entry, insert_text);
            Ok(())
        }
        Err(error) => {
            entry.insertion = Some(insertion_diagnostics_from_error(&error));
            entry.learning = None;
            Err(error)
        }
    }
}

fn persist_prompt_memory_update(
    entry: &TranscriptEntry,
    insert_text: &str,
) -> Option<LearningDiagnostics> {
    let learned = learn_correction(&entry.transcript_text, insert_text, true)?;
    let mut correction_store = load_correction_store();
    correction_store.set_preferred_transcription(&learned.source, &learned.replacement);
    if let Err(error) = correction_store.persist() {
        eprintln!(
            "[Pepper X] failed to persist correction memory {}: {error}",
            corrections_store_path().display()
        );
        return None;
    }

    Some(LearningDiagnostics::prompt_memory(
        learned.source,
        learned.replacement,
    ))
}

fn insertion_diagnostics_from_outcome(outcome: FriendlyInsertOutcome) -> InsertionDiagnostics {
    InsertionDiagnostics::succeeded(
        outcome.selection.backend_name,
        outcome.target_application_name,
    )
    .with_target_class(outcome.target_class)
    .with_attempted_backends(outcome.selection.attempted_backends.iter().copied())
}

fn insertion_diagnostics_from_error(error: &FriendlyInsertRunError) -> InsertionDiagnostics {
    match error.selected_backend() {
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
                diagnostics =
                    diagnostics.with_attempted_backends(error.attempted_backends().iter().copied());
            }

            diagnostics
        }
    }
}

#[cfg(test)]
mod app_shell {
    use super::*;
    use crate::history_store::{ArchiveWriteRequest, HistoryStore};
    use crate::settings::AppSettings;
    use crate::transcript_log::{env_lock, TranscriptLog};
    use pepperx_cleanup::cleanup::{CleanupError, CleanupResult};
    use pepperx_corrections::CorrectionStore;
    use pepperx_models::catalog_model;
    use pepperx_platform_gnome::context::SupportingContext;
    use std::ffi::OsString;

    fn materialize_ready_model(cache_root: &Path, model_id: &str) -> PathBuf {
        let model = catalog_model(model_id).expect("model should exist in catalog");
        let install_path = cache_root.join(model.install_path);
        match model.install_layout {
            pepperx_models::InstallLayout::Directory => {
                std::fs::create_dir_all(&install_path).unwrap();
                for required_file in model.required_files {
                    std::fs::write(install_path.join(required_file), b"pepper-x-model").unwrap();
                }
            }
            pepperx_models::InstallLayout::File => {
                if let Some(parent) = install_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                let file_bytes: &[u8] = match model.kind {
                    pepperx_models::ModelKind::Cleanup => b"GGUFpepper-x-model",
                    pepperx_models::ModelKind::Asr => b"pepper-x-model",
                };
                std::fs::write(&install_path, file_bytes).unwrap();
            }
        }
        install_path
    }

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_or_remove_env_var(name: &str, value: Option<OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    #[test]
    fn transcription_run_rejects_empty_model_dir_override() {
        let _guard = lock_env();
        let previous_model_dir = std::env::var_os("PEPPERX_PARAKEET_MODEL_DIR");
        std::env::set_var("PEPPERX_PARAKEET_MODEL_DIR", "");

        let error = configured_model_dir().unwrap_err();

        assert!(matches!(
            error,
            TranscriptionRunError::UnreadyAsrModel { model_id, .. }
                if model_id == "nemo-parakeet-tdt-0.6b-v2-int8"
        ));
        set_or_remove_env_var("PEPPERX_PARAKEET_MODEL_DIR", previous_model_dir);
    }

    #[test]
    fn model_status_transcription_rejects_unready_selected_asr_model() {
        let _guard = lock_env();
        let settings = AppSettings {
            preferred_asr_model: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
            ..AppSettings::default()
        };
        let previous_model_dir = std::env::var_os("PEPPERX_PARAKEET_MODEL_DIR");
        std::env::remove_var("PEPPERX_PARAKEET_MODEL_DIR");
        let cache_root = std::env::temp_dir().join(format!(
            "pepper-x-model-status-unready-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let error = configured_model_dir_with(&settings, &cache_root).unwrap_err();

        assert!(matches!(
            error,
            TranscriptionRunError::UnreadyAsrModel { model_id, .. }
                if model_id == "nemo-parakeet-tdt-0.6b-v2-int8"
        ));
        set_or_remove_env_var("PEPPERX_PARAKEET_MODEL_DIR", previous_model_dir);
        let _ = std::fs::remove_dir_all(cache_root);
    }

    #[test]
    fn model_status_cleanup_model_env_override_takes_precedence_over_settings() {
        let _guard = env_lock().lock().unwrap();
        let override_path = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-override-{}-{}.gguf",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let previous_cleanup_model = std::env::var_os("PEPPERX_CLEANUP_MODEL_PATH");
        std::fs::write(&override_path, b"GGUFpepper-x-cleanup-model").unwrap();
        std::env::set_var("PEPPERX_CLEANUP_MODEL_PATH", &override_path);
        let settings = AppSettings {
            preferred_cleanup_model: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            ..AppSettings::default()
        };
        let cache_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let configured_path = configured_cleanup_model_path_with(&settings, &cache_root)
            .expect("cleanup env override should win");

        assert_eq!(configured_path, override_path);
        match previous_cleanup_model {
            Some(previous_cleanup_model) => {
                std::env::set_var("PEPPERX_CLEANUP_MODEL_PATH", previous_cleanup_model)
            }
            None => std::env::remove_var("PEPPERX_CLEANUP_MODEL_PATH"),
        }
        let _ = std::fs::remove_file(override_path);
        let _ = std::fs::remove_dir_all(cache_root);
    }

    #[test]
    fn rerun_explicit_asr_model_ignores_env_override_path() {
        let _guard = env_lock().lock().unwrap();
        let cache_root = std::env::temp_dir().join(format!(
            "pepper-x-rerun-asr-explicit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let override_path = cache_root.join("override-model-dir");
        let previous_model_dir = std::env::var_os("PEPPERX_PARAKEET_MODEL_DIR");
        std::fs::create_dir_all(&override_path).unwrap();
        std::env::set_var("PEPPERX_PARAKEET_MODEL_DIR", &override_path);
        let expected_path = materialize_ready_model(&cache_root, "nemo-parakeet-tdt-0.6b-v3-int8");

        let configured_path = configured_requested_model_dir_for_model_id_with(
            "nemo-parakeet-tdt-0.6b-v3-int8",
            &cache_root,
        )
        .expect("explicit rerun ASR model should resolve from the catalog cache");

        assert_eq!(configured_path, expected_path);
        match previous_model_dir {
            Some(previous_model_dir) => {
                std::env::set_var("PEPPERX_PARAKEET_MODEL_DIR", previous_model_dir)
            }
            None => std::env::remove_var("PEPPERX_PARAKEET_MODEL_DIR"),
        }
        let _ = std::fs::remove_dir_all(cache_root);
    }

    #[test]
    fn rerun_explicit_cleanup_model_ignores_env_override_path() {
        let _guard = env_lock().lock().unwrap();
        let cache_root = std::env::temp_dir().join(format!(
            "pepper-x-rerun-cleanup-explicit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let override_path = cache_root.join("override-model.gguf");
        let previous_cleanup_model = std::env::var_os("PEPPERX_CLEANUP_MODEL_PATH");
        std::fs::create_dir_all(&cache_root).unwrap();
        std::fs::write(&override_path, b"override-model").unwrap();
        std::env::set_var("PEPPERX_CLEANUP_MODEL_PATH", &override_path);
        let expected_path =
            materialize_ready_model(&cache_root, "qwen2.5-1.5b-instruct-q4_k_m.gguf");

        let configured_path = configured_requested_cleanup_model_path_for_model_id_with(
            "qwen2.5-1.5b-instruct-q4_k_m.gguf",
            &cache_root,
        )
        .expect("explicit rerun cleanup model should resolve from the catalog cache");

        assert_eq!(configured_path, expected_path);
        match previous_cleanup_model {
            Some(previous_cleanup_model) => {
                std::env::set_var("PEPPERX_CLEANUP_MODEL_PATH", previous_cleanup_model)
            }
            None => std::env::remove_var("PEPPERX_CLEANUP_MODEL_PATH"),
        }
        let _ = std::fs::remove_dir_all(cache_root);
    }

    #[test]
    fn rerun_explicit_asr_model_rejects_cleanup_model_ids() {
        let cache_root = std::env::temp_dir().join(format!(
            "pepper-x-rerun-asr-kind-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let error = configured_requested_model_dir_for_model_id_with(
            "qwen2.5-3b-instruct-q4_k_m.gguf",
            &cache_root,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            TranscriptionRunError::UnknownAsrModel(model_id)
                if model_id == "qwen2.5-3b-instruct-q4_k_m.gguf"
        ));
    }

    #[test]
    fn rerun_explicit_cleanup_model_rejects_asr_model_ids() {
        let cache_root = std::env::temp_dir().join(format!(
            "pepper-x-rerun-cleanup-kind-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let error = configured_requested_cleanup_model_path_for_model_id_with(
            "nemo-parakeet-tdt-0.6b-v2-int8",
            &cache_root,
        )
        .unwrap_err();

        assert_eq!(
            error,
            CleanupError::UnsupportedModel("nemo-parakeet-tdt-0.6b-v2-int8".into())
        );
    }

    #[test]
    fn cleanup_context_capture_can_be_explicitly_disabled_for_headless_runs() {
        let _guard = env_lock().lock().unwrap();
        let previous = std::env::var_os(DISABLE_CONTEXT_CAPTURE_ENV);
        std::env::set_var(DISABLE_CONTEXT_CAPTURE_ENV, "1");

        assert!(context_capture_is_disabled());
        assert_eq!(capture_cleanup_context(), SupportingContext::default());

        match previous {
            Some(previous) => std::env::set_var(DISABLE_CONTEXT_CAPTURE_ENV, previous),
            None => std::env::remove_var(DISABLE_CONTEXT_CAPTURE_ENV),
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
    fn cleanup_runtime_archives_cleaned_text_separately_from_raw_transcript() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-success-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_cleanup(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello from Pepper X.".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
            },
        )
        .expect("archive cleanup entry");

        assert_eq!(entry.transcript_text, "hello from pepper x");
        assert_eq!(entry.display_text(), "Hello from Pepper X.");
        assert_eq!(
            entry.cleanup,
            Some(CleanupDiagnostics::succeeded(
                "llama.cpp",
                "qwen2.5-3b-instruct-q4_k_m.gguf",
                "Hello from Pepper X.",
                Duration::from_millis(19),
            ))
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
    fn cleanup_runtime_falls_back_to_raw_transcript_when_model_is_unavailable() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_cleanup(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Err(CleanupError::MissingModelPath(PathBuf::from(
                    "/tmp/missing.gguf",
                )))
            },
        )
        .expect("archive raw-only fallback entry");

        assert_eq!(entry.transcript_text, "hello from pepper x");
        assert_eq!(entry.display_text(), "hello from pepper x");
        assert_eq!(
            entry.cleanup,
            Some(CleanupDiagnostics::failed(
                "llama.cpp",
                "unknown",
                "cleanup model path does not exist: /tmp/missing.gguf",
            ))
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
    fn cleanup_runtime_archives_missing_cleanup_model_configuration_as_failure() {
        let _guard = lock_env();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-missing-model-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cache_home = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-missing-model-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        let previous_cleanup_model = std::env::var_os("PEPPERX_CLEANUP_MODEL_PATH");
        let previous_xdg_cache_home = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        std::env::remove_var("PEPPERX_CLEANUP_MODEL_PATH");
        std::env::set_var("XDG_CACHE_HOME", &cache_home);

        let entry = archive_transcription_result_with_cleanup(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |transcript_text| {
                let model_path = configured_cleanup_model_path()?;
                run_cleanup(&CleanupRequest {
                    transcript_text: transcript_text.into(),
                    model_path,
                    supporting_context_text: None,
                    ocr_text: None,
                    correction_memory_text: None,
                    prompt_profile: "ordinary-dictation".into(),
                })
            },
        )
        .expect("archive raw-only fallback entry");
        let expected_model_path = cache_home
            .join("pepper-x")
            .join("models")
            .join("cleanup/qwen2.5-3b-instruct-q4_k_m.gguf");

        assert_eq!(entry.transcript_text, "hello from pepper x");
        assert_eq!(entry.display_text(), "hello from pepper x");
        assert_eq!(
            entry.cleanup,
            Some(CleanupDiagnostics::failed(
                "llama.cpp",
                "unknown",
                format!(
                    "cleanup model path does not exist: {}",
                    expected_model_path.display()
                ),
            ))
        );

        match previous_cleanup_model {
            Some(previous_cleanup_model) => {
                std::env::set_var("PEPPERX_CLEANUP_MODEL_PATH", previous_cleanup_model)
            }
            None => std::env::remove_var("PEPPERX_CLEANUP_MODEL_PATH"),
        }
        set_or_remove_env_var("XDG_CACHE_HOME", previous_xdg_cache_home);
        match previous_state_root {
            Some(previous_state_root) => {
                std::env::set_var("PEPPERX_STATE_ROOT", previous_state_root)
            }
            None => std::env::remove_var("PEPPERX_STATE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(cache_home);
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn cleanup_ocr_runtime_archives_used_ocr_flag_from_cleanup_result() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-ocr-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_cleanup(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello from Pepper X.".into(),
                    elapsed_ms: 19,
                    used_ocr: true,
                })
            },
        )
        .expect("archive cleanup entry");

        assert_eq!(
            entry.cleanup,
            Some(CleanupDiagnostics {
                backend_name: "llama.cpp".into(),
                model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                cleaned_text: Some("Hello from Pepper X.".into()),
                elapsed_ms: 19,
                used_ocr: true,
                succeeded: true,
                failure_reason: None,
            })
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
    fn cleanup_ocr_runtime_archives_supporting_context_artifacts() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-context-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_cleanup_context(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext {
                supporting_context_text: Some("line before\nline after".into()),
                ocr_text: Some("ocr fallback".into()),
                used_ocr: true,
            },
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello from Pepper X.".into(),
                    elapsed_ms: 19,
                    used_ocr: true,
                })
            },
        )
        .expect("archive cleanup entry");
        let archived = HistoryStore::open(&state_root)
            .expect("open history store")
            .recent_runs()
            .expect("load archived runs")
            .into_iter()
            .next()
            .expect("expected one archived run");

        assert_eq!(entry.display_text(), "Hello from Pepper X.");
        assert_eq!(
            archived.supporting_context_text.as_deref(),
            Some("line before\nline after")
        );
        assert_eq!(archived.ocr_text.as_deref(), Some("ocr fallback"));

        match previous_state_root {
            Some(previous_state_root) => {
                std::env::set_var("PEPPERX_STATE_ROOT", previous_state_root)
            }
            None => std::env::remove_var("PEPPERX_STATE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn cleanup_corrections_runtime_does_not_rewrite_cleanup_output() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-corrections-preferred-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        let mut correction_store = CorrectionStore::new(state_root.join("corrections"));
        correction_store.set_preferred_transcription("hello from pepper x", "Hello from Pepper X.");
        correction_store
            .persist()
            .expect("persist correction store");

        let entry = archive_transcription_result_with_cleanup(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "hello from pepper x".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
            },
        )
        .expect("archive corrected cleanup entry");

        assert_eq!(entry.display_text(), "hello from pepper x");
        assert_eq!(
            entry
                .cleanup
                .as_ref()
                .and_then(|cleanup| cleanup.cleaned_text()),
            Some("hello from pepper x")
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
    fn cleanup_corrections_runtime_keeps_phrase_replacement_memory_out_of_current_output() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-corrections-replacements-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        let mut correction_store = CorrectionStore::new(state_root.join("corrections"));
        correction_store.add_replacement_rule("pepper x", "Pepper X");
        correction_store.add_replacement_rule("pepper", "Pepper");
        correction_store
            .persist()
            .expect("persist correction store");

        let entry = archive_transcription_result_with_cleanup(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "pepper x and pepper".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "pepper x and pepper".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
            },
        )
        .expect("archive corrected cleanup entry");

        assert_eq!(entry.display_text(), "pepper x and pepper");
        assert_eq!(
            entry
                .cleanup
                .as_ref()
                .and_then(|cleanup| cleanup.cleaned_text()),
            Some("pepper x and pepper")
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
    fn cleanup_ocr_runtime_archives_used_ocr_flag_on_cleanup_failure() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-ocr-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_cleanup_context(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext {
                supporting_context_text: Some("line before\nline after".into()),
                ocr_text: Some("ocr fallback".into()),
                used_ocr: true,
            },
            |_| {
                Err(CleanupError::MissingModelPath(PathBuf::from(
                    "/tmp/missing.gguf",
                )))
            },
        )
        .expect("archive cleanup failure entry");

        assert_eq!(
            entry.cleanup,
            Some(CleanupDiagnostics {
                backend_name: "llama.cpp".into(),
                model_name: "unknown".into(),
                cleaned_text: None,
                elapsed_ms: 0,
                used_ocr: true,
                succeeded: false,
                failure_reason: Some("cleanup model path does not exist: /tmp/missing.gguf".into()),
            })
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
            error
                .selected_backend()
                .expect("selected backend")
                .backend_name,
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

    #[test]
    fn cleanup_insert_runtime_uses_cleaned_text_for_friendly_insertion() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-insert-success-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        let mut inserted_text = None;

        let entry = archive_transcription_result_with_cleanup_and_friendly_insert(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello from Pepper X.".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
            },
            |text| {
                inserted_text = Some(text.to_string());
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
                    before_text: "Hello from Pepper X.".into(),
                    after_text: "Hello from Pepper X.".into(),
                })
            },
        )
        .expect("archive cleanup plus insert entry");

        assert_eq!(inserted_text.as_deref(), Some("Hello from Pepper X."));
        assert_eq!(entry.transcript_text, "hello from pepper x");
        assert_eq!(entry.display_text(), "Hello from Pepper X.");
        assert_eq!(
            entry.cleanup,
            Some(CleanupDiagnostics::succeeded(
                "llama.cpp",
                "qwen2.5-3b-instruct-q4_k_m.gguf",
                "Hello from Pepper X.",
                Duration::from_millis(19),
            ))
        );
        assert_eq!(
            entry.insertion,
            Some(
                InsertionDiagnostics::succeeded("atspi-editable-text", "Text Editor")
                    .with_target_class("text-editor")
                    .with_attempted_backends(["atspi-editable-text"])
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
    fn cleanup_insert_runtime_returns_insertion_error_after_archiving_entry() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-cleanup-insert-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let error = archive_transcription_result_with_cleanup_and_friendly_insert(
            TranscriptionResult {
                wav_path: state_root.join("loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello from Pepper X.".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
            },
            |text| {
                assert_eq!(text, "Hello from Pepper X.");
                Err(FriendlyInsertRunError::MissingFocusedTarget)
            },
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Pepper X friendly insertion failed: friendly insertion could not find a focused target"
        );

        let entries = TranscriptLog::open(&state_root)
            .expect("open transcript log")
            .recent_entries()
            .expect("load transcript log");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].transcript_text, "hello from pepper x");
        assert_eq!(entries[0].display_text(), "Hello from Pepper X.");
        assert_eq!(
            entries[0].cleanup,
            Some(CleanupDiagnostics::succeeded(
                "llama.cpp",
                "qwen2.5-3b-instruct-q4_k_m.gguf",
                "Hello from Pepper X.",
                Duration::from_millis(19),
            ))
        );
        assert_eq!(
            entries[0].insertion,
            Some(InsertionDiagnostics::failed(
                FRIENDLY_INSERT_BACKEND_NAME,
                "unknown",
                "friendly insertion could not find a focused target"
            ))
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
    fn post_paste_learning_persists_prompt_memory_for_future_cleanup() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-post-paste-learning-success-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_cleanup_and_friendly_insert(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello from Pepper X.".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
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
                    before_text: "Hello from Pepper X.".into(),
                    after_text: "Hello from Pepper X.".into(),
                })
            },
        )
        .expect("archive cleanup plus insert entry");

        let correction_store =
            CorrectionStore::load(state_root.join("corrections")).expect("load correction store");
        let prompt_memory = correction_store
            .prompt_memory_text()
            .expect("prompt memory should be present");

        assert_eq!(
            entry.learning,
            Some(crate::transcript_log::LearningDiagnostics::prompt_memory(
                "hello from pepper x",
                "Hello from Pepper X."
            ))
        );
        assert!(prompt_memory.contains("- raw: hello from pepper x"));
        assert!(prompt_memory.contains("  preferred: Hello from Pepper X."));
        assert_eq!(
            TranscriptLog::open(&state_root)
                .expect("open transcript log")
                .recent_entries()
                .expect("load transcript log")[0]
                .learning,
            Some(crate::transcript_log::LearningDiagnostics::prompt_memory(
                "hello from pepper x",
                "Hello from Pepper X."
            ))
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
    fn post_paste_learning_rejects_failed_insertions() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-post-paste-learning-failed-insert-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let error = archive_transcription_result_with_cleanup_and_friendly_insert(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello from Pepper X.".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
            },
            |_| Err(FriendlyInsertRunError::MissingFocusedTarget),
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Pepper X friendly insertion failed: friendly insertion could not find a focused target"
        );
        assert_eq!(
            TranscriptLog::open(&state_root)
                .expect("open transcript log")
                .recent_entries()
                .expect("load transcript log")[0]
                .learning,
            None
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
    fn post_paste_learning_rejects_destructive_prompt_memory_updates() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-post-paste-learning-destructive-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);

        let entry = archive_transcription_result_with_cleanup_and_friendly_insert(
            TranscriptionResult {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
                transcript_text: "hello from pepper x".into(),
                backend_name: "sherpa-onnx".into(),
                model_name: MODEL_NAME.into(),
                elapsed_ms: 42,
            },
            Some("ordinary-dictation".into()),
            SupportingContext::default(),
            |_| {
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
                    cleaned_text: "Hello".into(),
                    elapsed_ms: 19,
                    used_ocr: false,
                })
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
                    before_text: "Hello".into(),
                    after_text: "Hello".into(),
                })
            },
        )
        .expect("archive cleanup plus insert entry");

        assert_eq!(entry.learning, None);

        match previous_state_root {
            Some(previous_state_root) => {
                std::env::set_var("PEPPERX_STATE_ROOT", previous_state_root)
            }
            None => std::env::remove_var("PEPPERX_STATE_ROOT"),
        }
        let _ = std::fs::remove_dir_all(state_root);
    }

    #[test]
    fn rerun_archived_run_uses_archived_wav_and_override_models_and_prompt() {
        let _guard = env_lock().lock().unwrap();
        let state_root = std::env::temp_dir().join(format!(
            "pepper-x-rerun-archived-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&state_root);
        let previous_state_root = std::env::var_os("PEPPERX_STATE_ROOT");
        std::env::set_var("PEPPERX_STATE_ROOT", &state_root);
        std::fs::create_dir_all(&state_root).unwrap();
        let source_wav_path = state_root.join("source.wav");
        std::fs::write(&source_wav_path, b"pepper-x-rerun-audio").unwrap();

        let mut original_entry = TranscriptEntry::new(
            &source_wav_path,
            "hello from pepper x",
            "sherpa-onnx",
            MODEL_NAME,
            Duration::from_millis(42),
        );
        original_entry.cleanup = Some(CleanupDiagnostics::succeeded(
            "llama.cpp",
            "qwen2.5-3b-instruct-q4_k_m.gguf",
            "Hello from Pepper X.",
            Duration::from_millis(19),
        ));

        let original_run = HistoryStore::open(&state_root)
            .expect("open history store")
            .archive_run(
                &ArchiveWriteRequest::new(original_entry)
                    .with_prompt_profile("ordinary-dictation")
                    .with_supporting_context("line before\nline after")
                    .with_ocr_text("ocr fallback text"),
            )
            .expect("archive original run");
        let mut observed_asr = None;
        let mut observed_cleanup_model = None;
        let mut observed_cleanup_request = None;

        let entry = rerun_archived_run_with(
            ArchivedRunRerunRequest {
                run_id: original_run.run_id.clone(),
                asr_model_id: Some("nemo-parakeet-tdt-0.6b-v3-int8".into()),
                cleanup_model_id: Some("qwen2.5-1.5b-instruct-q4_k_m.gguf".into()),
                cleanup_prompt_profile: Some("literal-dictation".into()),
            },
            |wav_path: &Path, model_id: &str| {
                observed_asr = Some((wav_path.to_path_buf(), model_id.to_string()));
                Ok(TranscriptionResult {
                    wav_path: wav_path.to_path_buf(),
                    transcript_text: "hello from pepper ex".into(),
                    backend_name: "sherpa-onnx".into(),
                    model_name: model_id.into(),
                    elapsed_ms: 11,
                })
            },
            |cleanup_model_id: &str, request: CleanupRequest| {
                observed_cleanup_model = Some(cleanup_model_id.to_string());
                observed_cleanup_request = Some(request.clone());
                Ok(CleanupResult {
                    backend_name: "llama.cpp".into(),
                    model_name: cleanup_model_id.into(),
                    cleaned_text: "Hello from Pepper Ex.".into(),
                    elapsed_ms: 9,
                    used_ocr: true,
                })
            },
        )
        .expect("rerun archived run");
        let archived_runs = HistoryStore::open(&state_root)
            .expect("open history store")
            .recent_runs()
            .expect("load archived runs");
        let rerun = archived_runs
            .into_iter()
            .next()
            .expect("rerun should be archived");
        let cleanup_request = observed_cleanup_request.expect("cleanup request should be recorded");

        assert_eq!(
            observed_asr,
            Some((
                original_run
                    .archived_source_wav_path
                    .clone()
                    .expect("archived source wav path"),
                "nemo-parakeet-tdt-0.6b-v3-int8".into()
            ))
        );
        assert_eq!(
            observed_cleanup_model.as_deref(),
            Some("qwen2.5-1.5b-instruct-q4_k_m.gguf")
        );
        assert_eq!(cleanup_request.prompt_profile, "literal-dictation");
        assert_eq!(
            cleanup_request.supporting_context_text.as_deref(),
            Some("line before\nline after")
        );
        assert_eq!(
            cleanup_request.ocr_text.as_deref(),
            Some("ocr fallback text")
        );
        assert_eq!(entry.display_text(), "Hello from Pepper Ex.");
        assert_eq!(
            rerun.parent_run_id.as_deref(),
            Some(original_run.run_id.as_str())
        );
        assert_eq!(rerun.prompt_profile.as_deref(), Some("literal-dictation"));
        assert_eq!(rerun.entry.model_name, "nemo-parakeet-tdt-0.6b-v3-int8");
        assert_eq!(
            rerun
                .entry
                .cleanup
                .as_ref()
                .map(|cleanup| cleanup.model_name.as_str()),
            Some("qwen2.5-1.5b-instruct-q4_k_m.gguf")
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
