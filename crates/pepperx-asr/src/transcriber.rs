use hound::{SampleFormat, WavReader};
use parakeet_rs::Nemotron;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const BACKEND_NAME: &str = "parakeet-rs";

const ENCODER_FILE_NAME: &str = "encoder.onnx";
const DECODER_JOINT_FILE_NAME: &str = "decoder_joint.onnx";
const TOKENIZER_FILE_NAME: &str = "tokenizer.model";

/// Number of f32 samples in a 560ms chunk at 16 kHz.
const STREAMING_CHUNK_SAMPLES: usize = 8960;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionRequest {
    pub wav_path: PathBuf,
    pub model_dir: PathBuf,
    pub model_name: String,
}

impl TranscriptionRequest {
    pub fn new(
        wav_path: impl Into<PathBuf>,
        model_dir: impl Into<PathBuf>,
        model_name: impl Into<String>,
    ) -> Self {
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

// ---------------------------------------------------------------------------
// Batch mode -- transcribe a complete WAV file in one shot
// ---------------------------------------------------------------------------

pub fn transcribe_wav(
    request: &TranscriptionRequest,
) -> Result<TranscriptionResult, TranscriptionError> {
    validate_wav_path(&request.wav_path)?;
    validate_model_dir(&request.model_dir)?;

    let mut model = Nemotron::from_pretrained(&request.model_dir, None).map_err(|_| {
        TranscriptionError::RecognizerInitializationFailed(request.model_dir.clone())
    })?;

    let canonical_wav_path = std::fs::canonicalize(&request.wav_path)
        .map_err(|_| TranscriptionError::MissingWavFile(request.wav_path.clone()))?;

    let start = Instant::now();
    let transcript_text = model
        .transcribe_file(&canonical_wav_path)
        .map_err(|_| TranscriptionError::DecodeFailed(request.wav_path.clone()))?;

    Ok(TranscriptionResult {
        wav_path: canonical_wav_path,
        transcript_text,
        backend_name: BACKEND_NAME.to_string(),
        model_name: request.model_name.clone(),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

// ---------------------------------------------------------------------------
// Streaming mode -- feed 560ms chunks during recording
// ---------------------------------------------------------------------------

pub struct StreamingTranscriber {
    model: Nemotron,
    /// Leftover samples from the previous `feed_chunk` call that did not fill
    /// a complete 560ms window.
    pending: Vec<f32>,
}

impl StreamingTranscriber {
    /// Create a new streaming transcriber backed by the Nemotron model in
    /// `model_dir`.
    pub fn new(model_dir: &Path) -> Result<Self, TranscriptionError> {
        validate_model_dir(model_dir)?;
        let model = Nemotron::from_pretrained(model_dir, None)
            .map_err(|_| TranscriptionError::RecognizerInitializationFailed(model_dir.into()))?;
        Ok(Self {
            model,
            pending: Vec::with_capacity(STREAMING_CHUNK_SAMPLES),
        })
    }

    /// Feed raw mono 16 kHz f32 samples.  Returns the current partial
    /// transcript after processing any complete 560ms windows contained in
    /// `samples` (combined with any leftover samples from previous calls).
    pub fn feed_chunk(&mut self, samples: &[f32]) -> Result<String, TranscriptionError> {
        self.pending.extend_from_slice(samples);

        while self.pending.len() >= STREAMING_CHUNK_SAMPLES {
            let chunk: [f32; STREAMING_CHUNK_SAMPLES] = self.pending
                [..STREAMING_CHUNK_SAMPLES]
                .try_into()
                .expect("slice length verified");
            self.model
                .transcribe_chunk(&chunk)
                .map_err(|_| TranscriptionError::DecodeFailed(PathBuf::from("<streaming>")))?;
            self.pending.drain(..STREAMING_CHUNK_SAMPLES);
        }

        Ok(self.model.get_transcript())
    }

    /// Flush any remaining buffered samples (zero-padded to a full 560ms
    /// window) and return the final accumulated transcript.
    pub fn flush(&mut self) -> Result<String, TranscriptionError> {
        if !self.pending.is_empty() {
            let mut padded = [0.0f32; STREAMING_CHUNK_SAMPLES];
            let n = self.pending.len().min(STREAMING_CHUNK_SAMPLES);
            padded[..n].copy_from_slice(&self.pending[..n]);
            self.model
                .transcribe_chunk(&padded)
                .map_err(|_| TranscriptionError::DecodeFailed(PathBuf::from("<streaming>")))?;
            self.pending.clear();
        }

        Ok(self.model.get_transcript())
    }

    /// Return the full accumulated transcript so far without flushing pending
    /// samples.
    pub fn transcript(&self) -> String {
        self.model.get_transcript()
    }

    /// Reset the model state so this transcriber can be reused for a new
    /// utterance.
    pub fn reset(&mut self) {
        self.model.reset();
        self.pending.clear();
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn validate_wav_path(wav_path: &Path) -> Result<(), TranscriptionError> {
    if wav_path.is_file() {
        Ok(())
    } else {
        Err(TranscriptionError::MissingWavFile(wav_path.to_path_buf()))
    }
}

fn validate_model_dir(model_dir: &Path) -> Result<(), TranscriptionError> {
    for file_name in [
        ENCODER_FILE_NAME,
        DECODER_JOINT_FILE_NAME,
        TOKENIZER_FILE_NAME,
    ] {
        required_model_file(model_dir, file_name)?;
    }
    Ok(())
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

fn load_wav(wav_path: &Path) -> Result<(PathBuf, i32, Vec<f32>), TranscriptionError> {
    let canonical_wav_path = std::fs::canonicalize(wav_path)
        .map_err(|_| TranscriptionError::MissingWavFile(wav_path.to_path_buf()))?;
    let mut reader = WavReader::open(&canonical_wav_path)
        .map_err(|_| TranscriptionError::InvalidWaveFile(canonical_wav_path.clone()))?;
    let spec = reader.spec();

    if spec.channels != 1 {
        return Err(TranscriptionError::InvalidWaveFile(canonical_wav_path));
    }

    let sample_rate = spec.sample_rate as i32;
    let samples = read_wav_samples(&mut reader, spec.bits_per_sample, spec.sample_format)
        .map_err(|_| TranscriptionError::InvalidWaveFile(canonical_wav_path.clone()))?;

    Ok((canonical_wav_path, sample_rate, samples))
}

fn read_wav_samples<R>(
    reader: &mut WavReader<R>,
    bits_per_sample: u16,
    sample_format: SampleFormat,
) -> Result<Vec<f32>, hound::Error>
where
    R: std::io::Read,
{
    match sample_format {
        SampleFormat::Float => reader.samples::<f32>().collect(),
        SampleFormat::Int if bits_per_sample <= 16 => reader
            .samples::<i16>()
            .map(|sample| sample.map(|sample| sample as f32 / i16::MAX as f32))
            .collect(),
        SampleFormat::Int if bits_per_sample <= 32 => {
            let scale = ((1_i64 << (bits_per_sample - 1)) - 1) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|sample| sample as f32 / scale))
                .collect()
        }
        _ => Err(hound::Error::FormatError("unsupported wave encoding")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStringExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn transcriber_rejects_missing_wav_files() {
        let request = TranscriptionRequest::new(
            "/tmp/does-not-exist.wav",
            unique_test_root("model-dir"),
            "nemotron-speech-streaming-en-0.6b",
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
        fs::copy(fixture_path(), &wav_path).unwrap();
        let request = TranscriptionRequest::new(
            &wav_path,
            &model_dir,
            "nemotron-speech-streaming-en-0.6b",
        );

        let error = transcribe_wav(&request).unwrap_err();

        assert_eq!(
            error,
            TranscriptionError::IncompleteModelDir {
                model_dir,
                missing_file: "encoder.onnx",
            }
        );
    }

    #[test]
    fn transcriber_exposes_parakeet_backend_name() {
        assert_eq!(BACKEND_NAME, "parakeet-rs");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn transcriber_loads_non_utf8_wav_paths_and_normalizes_source_path() {
        let root = unique_test_root("non-utf8-wave");
        fs::create_dir_all(&root).unwrap();
        let wav_path = root.join(std::ffi::OsString::from_vec(vec![
            0x70, 0x65, 0x70, 0x70, 0x65, 0x72, 0x80, 0x2e, 0x77, 0x61, 0x76,
        ]));
        fs::copy(fixture_path(), &wav_path).unwrap();

        let (normalized_path, sample_rate, samples) = load_wav(&wav_path).unwrap();

        assert_eq!(normalized_path, std::fs::canonicalize(&wav_path).unwrap());
        assert_eq!(sample_rate, 16_000);
        assert!(!samples.is_empty());
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
            "nemotron-speech-streaming-en-0.6b",
        );

        let result = transcribe_wav(&request).expect("transcribe fixture");

        assert!(!result.transcript_text.trim().is_empty());
        assert!(result.transcript_text.to_lowercase().contains("pepper"));
    }

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/loop1-hello.wav")
    }

    fn unique_test_root(suffix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pepper-x-asr-{suffix}-{unique}"))
    }
}
