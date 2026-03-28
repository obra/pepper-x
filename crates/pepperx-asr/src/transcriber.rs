use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig, Wave,
};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const BACKEND_NAME: &str = "sherpa-onnx";
const MODEL_TYPE_NEMO_TRANSDUCER: &str = "nemo_transducer";
const ENCODER_FILE_NAME: &str = "encoder.int8.onnx";
const DECODER_FILE_NAME: &str = "decoder.int8.onnx";
const JOINER_FILE_NAME: &str = "joiner.int8.onnx";
const TOKENS_FILE_NAME: &str = "tokens.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRequest {
    pub wav_path: PathBuf,
    pub model_dir: PathBuf,
    pub model_name: String,
}

impl TranscriptionRequest {
    pub fn new(wav_path: impl Into<PathBuf>, model_dir: impl Into<PathBuf>, model_name: impl Into<String>) -> Self {
        Self {
            wav_path: wav_path.into(),
            model_dir: model_dir.into(),
            model_name: model_name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionResult {
    pub wav_path: PathBuf,
    pub transcript_text: String,
    pub backend_name: String,
    pub model_name: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptionError {
    MissingWavFile(PathBuf),
    IncompleteModelDir {
        model_dir: PathBuf,
        missing_file: &'static str,
    },
    InvalidWaveFile(PathBuf),
    RecognizerInitializationFailed(PathBuf),
    DecodeFailed(PathBuf),
}

pub fn transcribe_wav(request: &TranscriptionRequest) -> Result<TranscriptionResult, TranscriptionError> {
    validate_wav_path(&request.wav_path)?;
    let model_paths = ModelPaths::discover(&request.model_dir)?;
    let wave = Wave::read(&request.wav_path.to_string_lossy())
        .ok_or_else(|| TranscriptionError::InvalidWaveFile(request.wav_path.clone()))?;
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(model_paths.encoder.to_string_lossy().into_owned()),
        decoder: Some(model_paths.decoder.to_string_lossy().into_owned()),
        joiner: Some(model_paths.joiner.to_string_lossy().into_owned()),
    };
    config.model_config.tokens = Some(model_paths.tokens.to_string_lossy().into_owned());
    config.model_config.model_type = Some(MODEL_TYPE_NEMO_TRANSDUCER.into());
    config.model_config.provider = Some("cpu".into());

    let recognizer = OfflineRecognizer::create(&config)
        .ok_or_else(|| TranscriptionError::RecognizerInitializationFailed(request.model_dir.clone()))?;
    let stream = recognizer.create_stream();
    let start = Instant::now();
    stream.accept_waveform(wave.sample_rate(), wave.samples());
    recognizer.decode(&stream);
    let result = stream
        .get_result()
        .ok_or_else(|| TranscriptionError::DecodeFailed(request.wav_path.clone()))?;

    Ok(TranscriptionResult {
        wav_path: request.wav_path.clone(),
        transcript_text: result.text,
        backend_name: BACKEND_NAME.to_string(),
        model_name: request.model_name.clone(),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn validate_wav_path(wav_path: &Path) -> Result<(), TranscriptionError> {
    if wav_path.is_file() {
        Ok(())
    } else {
        Err(TranscriptionError::MissingWavFile(wav_path.to_path_buf()))
    }
}

#[derive(Debug, Clone)]
struct ModelPaths {
    encoder: PathBuf,
    decoder: PathBuf,
    joiner: PathBuf,
    tokens: PathBuf,
}

impl ModelPaths {
    fn discover(model_dir: &Path) -> Result<Self, TranscriptionError> {
        Ok(Self {
            encoder: required_model_file(model_dir, ENCODER_FILE_NAME)?,
            decoder: required_model_file(model_dir, DECODER_FILE_NAME)?,
            joiner: required_model_file(model_dir, JOINER_FILE_NAME)?,
            tokens: required_model_file(model_dir, TOKENS_FILE_NAME)?,
        })
    }
}

fn required_model_file(
    model_dir: &Path,
    file_name: &'static str,
) -> Result<PathBuf, TranscriptionError> {
    let path = model_dir.join(file_name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(TranscriptionError::IncompleteModelDir {
            model_dir: model_dir.to_path_buf(),
            missing_file: file_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn transcriber_rejects_missing_wav_files() {
        let request = TranscriptionRequest::new(
            "/tmp/does-not-exist.wav",
            unique_test_root("model-dir"),
            "nemo-parakeet-tdt-0.6b-v2-int8",
        );

        let error = transcribe_wav(&request).unwrap_err();

        assert_eq!(
            error,
            TranscriptionError::MissingWavFile(PathBuf::from("/tmp/does-not-exist.wav"))
        );
    }

    #[test]
    fn transcriber_rejects_incomplete_model_directories() {
        let model_dir = unique_test_root("incomplete-model");
        fs::create_dir_all(&model_dir).unwrap();
        let wav_path = model_dir.join("existing.wav");
        fs::write(&wav_path, b"not-a-real-wave-yet").unwrap();
        let request = TranscriptionRequest::new(
            &wav_path,
            &model_dir,
            "nemo-parakeet-tdt-0.6b-v2-int8",
        );

        let error = transcribe_wav(&request).unwrap_err();

        assert_eq!(
            error,
            TranscriptionError::IncompleteModelDir {
                model_dir,
                missing_file: "encoder.int8.onnx",
            }
        );
    }

    #[test]
    fn transcriber_exposes_sherpa_backend_name() {
        assert_eq!(BACKEND_NAME, "sherpa-onnx");
    }

    #[test]
    #[ignore = "requires PEPPERX_PARAKEET_MODEL_DIR and the loop1 WAV fixture"]
    fn transcriber_real_backend_transcribes_fixture() {
        let model_dir = PathBuf::from(
            std::env::var("PEPPERX_PARAKEET_MODEL_DIR")
                .expect("PEPPERX_PARAKEET_MODEL_DIR must point at a Parakeet model bundle"),
        );
        let request = TranscriptionRequest::new(
            fixture_path(),
            model_dir,
            "nemo-parakeet-tdt-0.6b-v2-int8",
        );

        let result = transcribe_wav(&request).expect("transcribe fixture");

        assert!(!result.transcript_text.trim().is_empty());
        assert!(result.transcript_text.to_lowercase().contains("pepper"));
    }

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/loop1-hello.wav")
    }

    fn unique_test_root(suffix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pepper-x-asr-{suffix}-{unique}"))
    }
}
