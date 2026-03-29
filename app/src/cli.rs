use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use pepperx_models::{
    bootstrap_model, catalog_model, default_cache_root, model_inventory, ModelInventoryEntry,
    ModelKind,
};
use pepperx_session::TriggerSource;

use crate::session_runtime::LiveRuntimeHandle;
use crate::settings::AppSettings;
use crate::transcript_log::TranscriptEntry;
use crate::transcription::{
    rerun_archived_run_to_log, transcribe_wav_and_cleanup_and_insert_friendly_to_log,
    transcribe_wav_and_cleanup_to_log, transcribe_wav_to_log, ArchivedRunRerunRequest,
    TranscriptionRunError,
};

#[derive(Debug)]
pub enum CliRunError {
    Transcription(TranscriptionRunError),
    Io(std::io::Error),
    InvalidModelSelection {
        model_id: String,
        expected_kind: ModelKind,
    },
    UnknownModel(String),
}

impl From<TranscriptionRunError> for CliRunError {
    fn from(error: TranscriptionRunError) -> Self {
        Self::Transcription(error)
    }
}

impl From<std::io::Error> for CliRunError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for CliRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transcription(error) => error.fmt(f),
            Self::Io(error) => write!(f, "Pepper X model management failed: {error}"),
            Self::InvalidModelSelection {
                model_id,
                expected_kind,
            } => write!(
                f,
                "Pepper X model {model_id} is not a supported {} model",
                model_kind_label(*expected_kind)
            ),
            Self::UnknownModel(model_id) => {
                write!(f, "Pepper X model is not supported: {model_id}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Gui,
    ListModels,
    BootstrapModel {
        model_id: String,
    },
    SetDefaultAsrModel {
        model_id: String,
    },
    SetDefaultCleanupModel {
        model_id: String,
    },
    SetCleanupPromptProfile {
        profile: String,
    },
    RecordAndTranscribe,
    RerunArchivedRun {
        run_id: String,
        asr_model_id: Option<String>,
        cleanup_model_id: Option<String>,
        cleanup_prompt_profile: Option<String>,
    },
    TranscribeWav {
        wav_path: PathBuf,
    },
    TranscribeWavAndCleanup {
        wav_path: PathBuf,
    },
    TranscribeWavAndInsertFriendly {
        wav_path: PathBuf,
    },
    TranscribeWavAndCleanupAndInsertFriendly {
        wav_path: PathBuf,
    },
}

pub fn parse_args<I, S>(args: I) -> Result<StartupMode, String>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _bin_name = args.next();

    match args.next() {
        None => Ok(StartupMode::Gui),
        Some(flag) if flag == OsStr::new("--list-models") => match args.next() {
            None => Ok(StartupMode::ListModels),
            Some(_) => Err("--list-models does not accept positional arguments".into()),
        },
        Some(flag) if flag == OsStr::new("--bootstrap-model") => match (args.next(), args.next()) {
            (Some(model_id), None) => Ok(StartupMode::BootstrapModel {
                model_id: model_id.to_string_lossy().into_owned(),
            }),
            (None, _) => Err("--bootstrap-model requires a model id".into()),
            (Some(_), Some(_)) => Err("--bootstrap-model accepts exactly one model id".into()),
        },
        Some(flag) if flag == OsStr::new("--set-default-asr-model") => {
            match (args.next(), args.next()) {
                (Some(model_id), None) => Ok(StartupMode::SetDefaultAsrModel {
                    model_id: model_id.to_string_lossy().into_owned(),
                }),
                (None, _) => Err("--set-default-asr-model requires a model id".into()),
                (Some(_), Some(_)) => {
                    Err("--set-default-asr-model accepts exactly one model id".into())
                }
            }
        }
        Some(flag) if flag == OsStr::new("--set-default-cleanup-model") => {
            match (args.next(), args.next()) {
                (Some(model_id), None) => Ok(StartupMode::SetDefaultCleanupModel {
                    model_id: model_id.to_string_lossy().into_owned(),
                }),
                (None, _) => Err("--set-default-cleanup-model requires a model id".into()),
                (Some(_), Some(_)) => {
                    Err("--set-default-cleanup-model accepts exactly one model id".into())
                }
            }
        }
        Some(flag) if flag == OsStr::new("--set-cleanup-prompt-profile") => {
            match (args.next(), args.next()) {
                (Some(profile), None) => Ok(StartupMode::SetCleanupPromptProfile {
                    profile: profile.to_string_lossy().into_owned(),
                }),
                (None, _) => Err("--set-cleanup-prompt-profile requires a profile name".into()),
                (Some(_), Some(_)) => {
                    Err("--set-cleanup-prompt-profile accepts exactly one profile name".into())
                }
            }
        }
        Some(flag) if flag == OsStr::new("--transcribe-wav") => match (args.next(), args.next()) {
            (Some(wav_path), None) => Ok(StartupMode::TranscribeWav {
                wav_path: PathBuf::from(wav_path),
            }),
            (None, _) => Err("--transcribe-wav requires a WAV path".into()),
            (Some(_), Some(_)) => Err("--transcribe-wav accepts exactly one WAV path".into()),
        },
        Some(flag) if flag == OsStr::new("--record-and-transcribe") => match args.next() {
            None => Ok(StartupMode::RecordAndTranscribe),
            Some(_) => Err("--record-and-transcribe does not accept positional arguments".into()),
        },
        Some(flag) if flag == OsStr::new("--rerun-archived-run") => {
            let Some(run_id) = args.next() else {
                return Err("--rerun-archived-run requires a run id".into());
            };
            let mut asr_model_id = None;
            let mut cleanup_model_id = None;
            let mut cleanup_prompt_profile = None;

            while let Some(flag) = args.next() {
                match flag.as_os_str() {
                    flag if flag == OsStr::new("--asr-model") => {
                        let Some(model_id) = args.next() else {
                            return Err("--asr-model requires a model id".into());
                        };
                        asr_model_id = Some(model_id.to_string_lossy().into_owned());
                    }
                    flag if flag == OsStr::new("--cleanup-model") => {
                        let Some(model_id) = args.next() else {
                            return Err("--cleanup-model requires a model id".into());
                        };
                        cleanup_model_id = Some(model_id.to_string_lossy().into_owned());
                    }
                    flag if flag == OsStr::new("--cleanup-prompt-profile") => {
                        let Some(profile) = args.next() else {
                            return Err("--cleanup-prompt-profile requires a profile name".into());
                        };
                        cleanup_prompt_profile = Some(profile.to_string_lossy().into_owned());
                    }
                    other => {
                        return Err(format!(
                            "unknown Pepper X rerun argument: {}",
                            PathBuf::from(other).display()
                        ));
                    }
                }
            }

            Ok(StartupMode::RerunArchivedRun {
                run_id: run_id.to_string_lossy().into_owned(),
                asr_model_id,
                cleanup_model_id,
                cleanup_prompt_profile,
            })
        }
        Some(flag) if flag == OsStr::new("--transcribe-wav-and-cleanup") => {
            match (args.next(), args.next()) {
                (Some(wav_path), None) => Ok(StartupMode::TranscribeWavAndCleanup {
                    wav_path: PathBuf::from(wav_path),
                }),
                (None, _) => Err("--transcribe-wav-and-cleanup requires a WAV path".into()),
                (Some(_), Some(_)) => {
                    Err("--transcribe-wav-and-cleanup accepts exactly one WAV path".into())
                }
            }
        }
        Some(flag) if flag == OsStr::new("--transcribe-wav-and-insert-friendly") => {
            match (args.next(), args.next()) {
                (Some(wav_path), None) => Ok(StartupMode::TranscribeWavAndInsertFriendly {
                    wav_path: PathBuf::from(wav_path),
                }),
                (None, _) => Err("--transcribe-wav-and-insert-friendly requires a WAV path".into()),
                (Some(_), Some(_)) => {
                    Err("--transcribe-wav-and-insert-friendly accepts exactly one WAV path".into())
                }
            }
        }
        Some(flag) if flag == OsStr::new("--transcribe-wav-and-cleanup-and-insert-friendly") => {
            match (args.next(), args.next()) {
                (Some(wav_path), None) => {
                    Ok(StartupMode::TranscribeWavAndCleanupAndInsertFriendly {
                        wav_path: PathBuf::from(wav_path),
                    })
                }
                (None, _) => Err(
                    "--transcribe-wav-and-cleanup-and-insert-friendly requires a WAV path".into(),
                ),
                (Some(_), Some(_)) => Err(
                    "--transcribe-wav-and-cleanup-and-insert-friendly accepts exactly one WAV path"
                        .into(),
                ),
            }
        }
        Some(flag) => Err(format!(
            "unknown Pepper X argument: {}",
            PathBuf::from(flag).display()
        )),
    }
}

pub fn run(startup_mode: StartupMode) -> Result<Option<TranscriptEntry>, CliRunError> {
    let settings = AppSettings::load_or_default();

    match startup_mode {
        StartupMode::ListModels => {
            let cache_root = default_cache_root();
            println!(
                "{}",
                format_model_status_report(&settings, &cache_root, &model_inventory(&cache_root))
            );
            Ok(None)
        }
        StartupMode::BootstrapModel { model_id } => {
            let model = catalog_model(&model_id)
                .ok_or_else(|| CliRunError::UnknownModel(model_id.clone()))?;
            let cache_root = default_cache_root();
            let readiness = bootstrap_model(model, &cache_root).map_err(|error| {
                CliRunError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    error.to_string(),
                ))
            })?;
            println!(
                "Bootstrapped {} model {} into {}",
                model_kind_label(model.kind),
                model.id,
                readiness.install_path.display()
            );
            Ok(None)
        }
        StartupMode::SetDefaultAsrModel { model_id } => {
            update_default_model(model_id, ModelKind::Asr, |settings, model_id| {
                settings.preferred_asr_model = model_id;
            })?;
            Ok(None)
        }
        StartupMode::SetDefaultCleanupModel { model_id } => {
            update_default_model(model_id, ModelKind::Cleanup, |settings, model_id| {
                settings.preferred_cleanup_model = model_id;
            })?;
            Ok(None)
        }
        StartupMode::SetCleanupPromptProfile { profile } => {
            let mut settings = AppSettings::load_or_default();
            settings.cleanup_prompt_profile = profile.clone();
            settings.save()?;
            println!("Updated Pepper X cleanup prompt profile to {profile}");
            Ok(None)
        }
        other_mode => run_with(
            other_mode,
            || record_and_transcribe(settings.preferred_microphone.clone(), wait_for_stop_signal),
            rerun_archived_run_to_log,
            transcribe_wav_to_log,
            transcribe_wav_and_cleanup_to_log,
            crate::transcription::transcribe_wav_and_insert_friendly_to_log,
            transcribe_wav_and_cleanup_and_insert_friendly_to_log,
        )
        .map_err(Into::into),
    }
}

pub fn run_with<R, Q, F, G, H, I>(
    startup_mode: StartupMode,
    record_and_transcribe: R,
    rerun_archived_run: Q,
    transcribe: F,
    transcribe_and_cleanup: G,
    transcribe_and_insert_friendly: H,
    transcribe_and_cleanup_and_insert_friendly: I,
) -> Result<Option<TranscriptEntry>, TranscriptionRunError>
where
    R: FnOnce() -> Result<TranscriptEntry, TranscriptionRunError>,
    Q: FnOnce(ArchivedRunRerunRequest) -> Result<TranscriptEntry, TranscriptionRunError>,
    F: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
    G: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
    H: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
    I: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
{
    match startup_mode {
        StartupMode::Gui => Ok(None),
        StartupMode::ListModels => Ok(None),
        StartupMode::BootstrapModel { .. } => Ok(None),
        StartupMode::SetDefaultAsrModel { .. } => Ok(None),
        StartupMode::SetDefaultCleanupModel { .. } => Ok(None),
        StartupMode::SetCleanupPromptProfile { .. } => Ok(None),
        StartupMode::RecordAndTranscribe => record_and_transcribe().map(Some),
        StartupMode::RerunArchivedRun {
            run_id,
            asr_model_id,
            cleanup_model_id,
            cleanup_prompt_profile,
        } => rerun_archived_run(ArchivedRunRerunRequest {
            run_id,
            asr_model_id,
            cleanup_model_id,
            cleanup_prompt_profile,
        })
        .map(Some),
        StartupMode::TranscribeWav { wav_path } => transcribe(&wav_path).map(Some),
        StartupMode::TranscribeWavAndCleanup { wav_path } => {
            transcribe_and_cleanup(&wav_path).map(Some)
        }
        StartupMode::TranscribeWavAndInsertFriendly { wav_path } => {
            transcribe_and_insert_friendly(&wav_path).map(Some)
        }
        StartupMode::TranscribeWavAndCleanupAndInsertFriendly { wav_path } => {
            transcribe_and_cleanup_and_insert_friendly(&wav_path).map(Some)
        }
    }
}

fn record_and_transcribe<F>(
    selected_microphone: Option<pepperx_audio::SelectedMicrophone>,
    wait_for_stop: F,
) -> Result<TranscriptEntry, TranscriptionRunError>
where
    F: FnOnce() -> std::io::Result<()>,
{
    let runtime = LiveRuntimeHandle::new(selected_microphone);
    runtime
        .record_and_transcribe(TriggerSource::ShellAction, wait_for_stop)
        .map_err(|error| TranscriptionRunError::LiveRecording(error.to_string()))
}

fn wait_for_stop_signal() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    wait_for_stop_signal_from(&mut stdin)
}

fn wait_for_stop_signal_from<R>(reader: &mut R) -> std::io::Result<()>
where
    R: std::io::BufRead,
{
    let mut stop_line = String::new();
    let bytes_read = reader.read_line(&mut stop_line)?;
    if bytes_read == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "stdin closed before Pepper X live stop signal",
        ));
    }

    Ok(())
}

fn update_default_model<F>(
    model_id: String,
    expected_kind: ModelKind,
    update: F,
) -> Result<(), CliRunError>
where
    F: FnOnce(&mut AppSettings, String),
{
    let model =
        catalog_model(&model_id).ok_or_else(|| CliRunError::UnknownModel(model_id.clone()))?;
    if model.kind != expected_kind {
        return Err(CliRunError::InvalidModelSelection {
            model_id,
            expected_kind,
        });
    }

    let mut settings = AppSettings::load_or_default();
    update(&mut settings, model.id.into());
    settings.save()?;
    println!(
        "Updated Pepper X default {} model to {}",
        model_kind_label(expected_kind),
        model.id
    );
    Ok(())
}

fn format_model_status_report(
    settings: &AppSettings,
    cache_root: &std::path::Path,
    inventory: &[ModelInventoryEntry],
) -> String {
    let mut lines = vec![
        format!("Model cache: {}", cache_root.display()),
        format!("Default ASR model: {}", settings.preferred_asr_model),
        format!(
            "Default cleanup model: {}",
            settings.preferred_cleanup_model
        ),
        format!(
            "Cleanup prompt profile: {}",
            settings.cleanup_prompt_profile
        ),
        String::from("Supported models:"),
    ];

    for entry in inventory {
        let status = if entry.readiness.is_ready {
            "ready".to_string()
        } else {
            format!("missing {}", entry.readiness.missing_files.join(", "))
        };
        lines.push(format!(
            "- {} [{}] {}",
            entry.id,
            model_kind_label(entry.kind),
            status
        ));
    }

    lines.join("\n")
}

fn model_kind_label(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::Asr => "asr",
        ModelKind::Cleanup => "cleanup",
    }
}

#[cfg(test)]
mod cli_mode {
    use super::*;
    use std::time::Duration;

    #[test]
    fn app_shell_recording_cli_mode_parses_live_recording_flag() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--record-and-transcribe".to_string(),
        ])
        .expect("live recording mode should parse");

        assert_eq!(command, StartupMode::RecordAndTranscribe);
    }

    #[test]
    fn rerun_cli_mode_parses_archived_run_with_optional_overrides() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--rerun-archived-run".to_string(),
            "run-123".to_string(),
            "--asr-model".to_string(),
            "nemo-parakeet-tdt-1.1b".to_string(),
            "--cleanup-model".to_string(),
            "qwen2.5-1.5b".to_string(),
            "--cleanup-prompt-profile".to_string(),
            "literal-dictation".to_string(),
        ])
        .expect("rerun mode should parse");

        assert_eq!(
            command,
            StartupMode::RerunArchivedRun {
                run_id: "run-123".into(),
                asr_model_id: Some("nemo-parakeet-tdt-1.1b".into()),
                cleanup_model_id: Some("qwen2.5-1.5b".into()),
                cleanup_prompt_profile: Some("literal-dictation".into()),
            }
        );
    }

    #[test]
    fn model_status_cli_mode_parses_list_models() {
        let command = parse_args(["pepper-x".to_string(), "--list-models".to_string()]).unwrap();

        assert_eq!(command, StartupMode::ListModels);
    }

    #[test]
    fn model_status_cli_mode_parses_bootstrap_model() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--bootstrap-model".to_string(),
            "nemo-parakeet-tdt-0.6b-v2-int8".to_string(),
        ])
        .unwrap();

        assert_eq!(
            command,
            StartupMode::BootstrapModel {
                model_id: "nemo-parakeet-tdt-0.6b-v2-int8".into(),
            }
        );
    }

    #[test]
    fn model_status_cli_mode_parses_set_default_cleanup_model() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--set-default-cleanup-model".to_string(),
            "qwen2.5-3b-instruct-q4_k_m.gguf".to_string(),
        ])
        .unwrap();

        assert_eq!(
            command,
            StartupMode::SetDefaultCleanupModel {
                model_id: "qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            }
        );
    }

    #[test]
    fn app_shell_recording_cli_mode_rejects_extra_live_recording_arguments() {
        let error = parse_args([
            "pepper-x".to_string(),
            "--record-and-transcribe".to_string(),
            "extra".to_string(),
        ])
        .unwrap_err();

        assert_eq!(
            error,
            "--record-and-transcribe does not accept positional arguments"
        );
    }

    #[test]
    fn app_shell_recording_cli_mode_runs_live_recording_without_gui() {
        let wav_path = PathBuf::from("/tmp/live.wav");
        let expected = TranscriptEntry::new(
            &wav_path,
            "hello from live pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(64),
        );

        let result = run_with(
            StartupMode::RecordAndTranscribe,
            || Ok(expected.clone()),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
        )
        .expect("live recording mode should succeed");

        assert_eq!(result, Some(expected));
    }

    #[test]
    fn app_shell_recording_cli_mode_rejects_eof_stop_signal() {
        let mut closed_stdin = std::io::Cursor::new(Vec::<u8>::new());

        let error = wait_for_stop_signal_from(&mut closed_stdin).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn rerun_cli_mode_runs_archived_run_without_gui() {
        let wav_path = PathBuf::from("/tmp/rerun.wav");
        let expected = TranscriptEntry::new(
            &wav_path,
            "hello from rerun pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-1.1b",
            Duration::from_millis(41),
        );
        let mut observed_request = None;

        let result = run_with(
            StartupMode::RerunArchivedRun {
                run_id: "run-123".into(),
                asr_model_id: Some("nemo-parakeet-tdt-1.1b".into()),
                cleanup_model_id: Some("qwen2.5-1.5b".into()),
                cleanup_prompt_profile: Some("literal-dictation".into()),
            },
            || unreachable!(),
            |request| {
                observed_request = Some(request);
                Ok(expected.clone())
            },
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
        )
        .expect("rerun mode should succeed");

        assert_eq!(
            observed_request,
            Some(ArchivedRunRerunRequest {
                run_id: "run-123".into(),
                asr_model_id: Some("nemo-parakeet-tdt-1.1b".into()),
                cleanup_model_id: Some("qwen2.5-1.5b".into()),
                cleanup_prompt_profile: Some("literal-dictation".into()),
            })
        );
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn cli_mode_parses_transcribe_wav_path() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--transcribe-wav".to_string(),
            "/tmp/loop1.wav".to_string(),
        ])
        .unwrap();

        assert_eq!(
            command,
            StartupMode::TranscribeWav {
                wav_path: PathBuf::from("/tmp/loop1.wav"),
            }
        );
    }

    #[test]
    fn cli_mode_parses_transcribe_wav_and_insert_friendly_path() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--transcribe-wav-and-insert-friendly".to_string(),
            "/tmp/loop1.wav".to_string(),
        ])
        .unwrap();

        assert_eq!(
            command,
            StartupMode::TranscribeWavAndInsertFriendly {
                wav_path: PathBuf::from("/tmp/loop1.wav"),
            }
        );
    }

    #[test]
    fn cleanup_cli_mode_parses_transcribe_wav_and_cleanup_path() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--transcribe-wav-and-cleanup".to_string(),
            "/tmp/loop5.wav".to_string(),
        ])
        .expect("cleanup mode should parse");

        assert_eq!(
            command,
            StartupMode::TranscribeWavAndCleanup {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
            }
        );
    }

    #[test]
    fn cli_mode_rejects_missing_transcribe_wav_path() {
        let error =
            parse_args(["pepper-x".to_string(), "--transcribe-wav".to_string()]).unwrap_err();

        assert_eq!(error, "--transcribe-wav requires a WAV path");
    }

    #[test]
    fn cleanup_cli_mode_rejects_missing_cleanup_wav_path() {
        let error = parse_args([
            "pepper-x".to_string(),
            "--transcribe-wav-and-cleanup".to_string(),
        ])
        .unwrap_err();

        assert_eq!(error, "--transcribe-wav-and-cleanup requires a WAV path");
    }

    #[test]
    fn cleanup_insert_cli_mode_parses_cleanup_and_insert_friendly_path() {
        let command = parse_args([
            "pepper-x".to_string(),
            "--transcribe-wav-and-cleanup-and-insert-friendly".to_string(),
            "/tmp/loop5.wav".to_string(),
        ])
        .expect("cleanup plus insert mode should parse");

        assert_eq!(
            command,
            StartupMode::TranscribeWavAndCleanupAndInsertFriendly {
                wav_path: PathBuf::from("/tmp/loop5.wav"),
            }
        );
    }

    #[test]
    fn cleanup_insert_cli_mode_rejects_missing_cleanup_and_insert_wav_path() {
        let error = parse_args([
            "pepper-x".to_string(),
            "--transcribe-wav-and-cleanup-and-insert-friendly".to_string(),
        ])
        .unwrap_err();

        assert_eq!(
            error,
            "--transcribe-wav-and-cleanup-and-insert-friendly requires a WAV path"
        );
    }

    #[test]
    fn cleanup_insert_cli_mode_runs_transcribe_wav_and_cleanup_and_insert_without_gui() {
        let wav_path = PathBuf::from("/tmp/loop5.wav");
        let mut observed_path = None;
        let mut expected = TranscriptEntry::new(
            &wav_path,
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(37),
        );
        expected.cleanup = Some(crate::transcript_log::CleanupDiagnostics::succeeded(
            "llama.cpp",
            "qwen2.5-3b-instruct-q4_k_m.gguf",
            "Hello from Pepper X.",
            Duration::from_millis(19),
        ));

        let result = run_with(
            StartupMode::TranscribeWavAndCleanupAndInsertFriendly {
                wav_path: wav_path.clone(),
            },
            || unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |path: &std::path::Path| {
                observed_path = Some(path.to_path_buf());
                Ok(expected.clone())
            },
        )
        .unwrap();

        assert_eq!(observed_path, Some(wav_path));
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn cli_mode_rejects_missing_friendly_insert_wav_path() {
        let error = parse_args([
            "pepper-x".to_string(),
            "--transcribe-wav-and-insert-friendly".to_string(),
        ])
        .unwrap_err();

        assert_eq!(
            error,
            "--transcribe-wav-and-insert-friendly requires a WAV path"
        );
    }

    #[test]
    fn cli_mode_rejects_unknown_argument() {
        let error = parse_args(["pepper-x".to_string(), "--wat".to_string()]).unwrap_err();

        assert_eq!(error, "unknown Pepper X argument: --wat");
    }

    #[test]
    fn cli_mode_runs_transcribe_wav_without_gui() {
        let wav_path = PathBuf::from("/tmp/loop1.wav");
        let expected = TranscriptEntry::new(
            &wav_path,
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(37),
        );
        let mut observed_path = None;

        let result = run_with(
            StartupMode::TranscribeWav {
                wav_path: wav_path.clone(),
            },
            || unreachable!(),
            |_| unreachable!(),
            |path: &std::path::Path| {
                observed_path = Some(path.to_path_buf());
                Ok(expected.clone())
            },
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
        )
        .unwrap();

        assert_eq!(observed_path, Some(wav_path));
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn cleanup_cli_mode_runs_transcribe_wav_and_cleanup_without_gui() {
        let wav_path = PathBuf::from("/tmp/loop5.wav");
        let mut observed_path = None;
        let mut expected = TranscriptEntry::new(
            &wav_path,
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(37),
        );
        expected.cleanup = Some(crate::transcript_log::CleanupDiagnostics::succeeded(
            "llama.cpp",
            "qwen2.5-3b-instruct-q4_k_m.gguf",
            "Hello from Pepper X.",
            Duration::from_millis(19),
        ));

        let result = run_with(
            StartupMode::TranscribeWavAndCleanup {
                wav_path: wav_path.clone(),
            },
            || unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |path: &std::path::Path| {
                observed_path = Some(path.to_path_buf());
                Ok(expected.clone())
            },
            |_| unreachable!(),
            |_| unreachable!(),
        )
        .unwrap();

        assert_eq!(observed_path, Some(wav_path));
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn cli_mode_runs_transcribe_wav_and_insert_friendly_without_gui() {
        let wav_path = PathBuf::from("/tmp/loop1.wav");
        let expected = TranscriptEntry::new(
            &wav_path,
            "hello from pepper x",
            "sherpa-onnx",
            "nemo-parakeet-tdt-0.6b-v2-int8",
            Duration::from_millis(37),
        );
        let mut observed_path = None;

        let result = run_with(
            StartupMode::TranscribeWavAndInsertFriendly {
                wav_path: wav_path.clone(),
            },
            || unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |path: &std::path::Path| {
                observed_path = Some(path.to_path_buf());
                Ok(expected.clone())
            },
            |_| unreachable!(),
        )
        .unwrap();

        assert_eq!(observed_path, Some(wav_path));
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn cli_mode_keeps_gui_startup_mode_outside_runner() {
        let result = run_with(
            StartupMode::Gui,
            || unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
        )
        .unwrap();

        assert_eq!(result, None);
    }

    #[cfg(unix)]
    #[test]
    fn cli_mode_preserves_non_utf8_wav_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let wav_path = OsString::from_vec(vec![0x66, 0x6f, 0x80, 0x6f, 0x2e, 0x77, 0x61, 0x76]);
        let command = parse_args([
            OsString::from("pepper-x"),
            OsString::from("--transcribe-wav"),
            wav_path.clone(),
        ])
        .unwrap();

        assert_eq!(
            command,
            StartupMode::TranscribeWav {
                wav_path: PathBuf::from(wav_path),
            }
        );
    }
}
