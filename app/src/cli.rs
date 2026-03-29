use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use pepperx_session::TriggerSource;

use crate::session_runtime::LiveRuntimeHandle;
use crate::settings::AppSettings;
use crate::transcript_log::TranscriptEntry;
use crate::transcription::{
    transcribe_wav_and_cleanup_and_insert_friendly_to_log, transcribe_wav_and_cleanup_to_log,
    transcribe_wav_to_log, TranscriptionRunError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Gui,
    RecordAndTranscribe,
    TranscribeWav { wav_path: PathBuf },
    TranscribeWavAndCleanup { wav_path: PathBuf },
    TranscribeWavAndInsertFriendly { wav_path: PathBuf },
    TranscribeWavAndCleanupAndInsertFriendly { wav_path: PathBuf },
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

pub fn run(startup_mode: StartupMode) -> Result<Option<TranscriptEntry>, TranscriptionRunError> {
    let settings = AppSettings::load_or_default();

    run_with(
        startup_mode,
        || record_and_transcribe(settings.preferred_microphone.clone(), wait_for_stop_signal),
        transcribe_wav_to_log,
        transcribe_wav_and_cleanup_to_log,
        crate::transcription::transcribe_wav_and_insert_friendly_to_log,
        transcribe_wav_and_cleanup_and_insert_friendly_to_log,
    )
}

pub fn run_with<R, F, G, H, I>(
    startup_mode: StartupMode,
    record_and_transcribe: R,
    transcribe: F,
    transcribe_and_cleanup: G,
    transcribe_and_insert_friendly: H,
    transcribe_and_cleanup_and_insert_friendly: I,
) -> Result<Option<TranscriptEntry>, TranscriptionRunError>
where
    R: FnOnce() -> Result<TranscriptEntry, TranscriptionRunError>,
    F: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
    G: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
    H: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
    I: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
{
    match startup_mode {
        StartupMode::Gui => Ok(None),
        StartupMode::RecordAndTranscribe => record_and_transcribe().map(Some),
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
