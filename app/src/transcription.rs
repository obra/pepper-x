use std::path::{Path, PathBuf};

use pepperx_asr::{transcribe_wav, TranscriptionError, TranscriptionRequest, TranscriptionResult};

use crate::transcript_log::{nonempty_env_path, state_root, TranscriptEntry, TranscriptLog};

const MODEL_NAME: &str = "nemo-parakeet-tdt-0.6b-v2-int8";

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
    let model_dir = configured_model_dir()?;
    let request = TranscriptionRequest::new(wav_path, &model_dir, MODEL_NAME);
    let result = transcribe_wav(&request)?;
    archive_transcription_result(result)
}

pub(crate) fn archive_transcription_result(
    result: TranscriptionResult,
) -> Result<TranscriptEntry, TranscriptionRunError> {
    let entry = TranscriptEntry::new(
        result.wav_path,
        result.transcript_text,
        result.backend_name,
        result.model_name,
        std::time::Duration::from_millis(result.elapsed_ms),
    );
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
mod tests {
    use super::*;

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
}
