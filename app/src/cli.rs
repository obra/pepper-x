use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::transcript_log::TranscriptEntry;
use crate::transcription::{transcribe_wav_to_log, TranscriptionRunError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Gui,
    TranscribeWav { wav_path: PathBuf },
    TranscribeWavAndInsertFriendly { wav_path: PathBuf },
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
        Some(flag) => Err(format!(
            "unknown Pepper X argument: {}",
            PathBuf::from(flag).display()
        )),
    }
}

pub fn run(startup_mode: StartupMode) -> Result<Option<TranscriptEntry>, TranscriptionRunError> {
    run_with(
        startup_mode,
        transcribe_wav_to_log,
        crate::transcription::transcribe_wav_and_insert_friendly_to_log,
    )
}

pub fn run_with<F, G>(
    startup_mode: StartupMode,
    transcribe: F,
    transcribe_and_insert_friendly: G,
) -> Result<Option<TranscriptEntry>, TranscriptionRunError>
where
    F: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
    G: FnOnce(&std::path::Path) -> Result<TranscriptEntry, TranscriptionRunError>,
{
    match startup_mode {
        StartupMode::Gui => Ok(None),
        StartupMode::TranscribeWav { wav_path } => transcribe(&wav_path).map(Some),
        StartupMode::TranscribeWavAndInsertFriendly { wav_path } => {
            transcribe_and_insert_friendly(&wav_path).map(Some)
        }
    }
}

#[cfg(test)]
mod cli_mode {
    use super::*;
    use std::time::Duration;

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
    fn cli_mode_rejects_missing_transcribe_wav_path() {
        let error =
            parse_args(["pepper-x".to_string(), "--transcribe-wav".to_string()]).unwrap_err();

        assert_eq!(error, "--transcribe-wav requires a WAV path");
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
    fn cli_mode_keeps_gui_startup_mode_outside_runner() {
        let result = run_with(StartupMode::Gui, |_| unreachable!(), |_| unreachable!()).unwrap();

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
