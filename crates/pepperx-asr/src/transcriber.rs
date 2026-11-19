use crate::decoder::{decode_audio_file, DecodeError, TARGET_SAMPLE_RATE};
use parakeet_rs::{
    ExecutionConfig, ExecutionProvider, Nemotron, NemotronMode, ParakeetTDT, TimestampMode,
    Transcriber,
};
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
    /// Source audio path (WAV/MP3/FLAC/OGG/AAC/…). Field name kept for API stability.
    pub wav_path: PathBuf,
    pub model_dir: PathBuf,
    pub model_name: String,
    /// Target language for multilingual models ("fr-FR", "en-US", "auto", etc.)
    /// `None` falls back to "fr-FR".
    pub target_lang: Option<String>,
}

impl TranscriptionRequest {
    /// Create a new transcription request (default language = French)
    pub fn new(
        wav_path: impl Into<PathBuf>,
        model_dir: impl Into<PathBuf>,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            wav_path: wav_path.into(),
            model_dir: model_dir.into(),
            model_name: model_name.into(),
            target_lang: None,
        }
    }

    /// Create a new transcription request with explicit target language
    pub fn new_with_lang(
        wav_path: impl Into<PathBuf>,
        model_dir: impl Into<PathBuf>,
        model_name: impl Into<String>,
        target_lang: impl Into<String>,
    ) -> Self {
        Self {
            wav_path: wav_path.into(),
            model_dir: model_dir.into(),
            model_name: model_name.into(),
            target_lang: Some(target_lang.into()),
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
    /// Decode/convert failed; `detail` is human-readable (format, I/O, empty, …).
    AudioDecodeFailed {
        path: PathBuf,
        detail: String,
    },
    RecognizerInitializationFailed(PathBuf),
    DecodeFailed(PathBuf),
    LanguageConfigFailed(String),
}

// ---------------------------------------------------------------------------
// Batch mode -- transcribe a complete audio file in one shot
// ---------------------------------------------------------------------------

/// Transcribe an audio file (WAV, MP3, FLAC, OGG, AAC/M4A, …).
///
/// Any container supported by Symphonia is decoded to mono PCM at 16 kHz, then
/// fed to the Nemotron offline path. Live capture still uses WAV via PipeWire.
pub fn transcribe_wav(
    request: &TranscriptionRequest,
) -> Result<TranscriptionResult, TranscriptionError> {
    validate_audio_path(&request.wav_path)?;
    validate_model_dir(&request.model_dir)?;

    if is_parakeet_tdt_dir(&request.model_dir) {
        return transcribe_wav_tdt(request);
    }

    let mut model = Nemotron::from_pretrained(&request.model_dir, None).map_err(|_| {
        TranscriptionError::RecognizerInitializationFailed(request.model_dir.clone())
    })?;

    configure_multilingual(&mut model, request.target_lang.as_deref())?;

    let canonical_source_path = std::fs::canonicalize(&request.wav_path)
        .map_err(|_| TranscriptionError::MissingWavFile(request.wav_path.clone()))?;

    // Always normalize through mono 16 kHz PCM (in-memory). For non-WAV sources
    // this is required; for multi-rate/multi-channel WAV it is also required.
    let decoded =
        decode_audio_file(&canonical_source_path).map_err(|error| map_decode_error(error))?;
    debug_assert_eq!(decoded.sample_rate, TARGET_SAMPLE_RATE);

    let start = Instant::now();
    let transcript_text = model
        .transcribe_audio(&decoded.samples)
        .map_err(|_| TranscriptionError::DecodeFailed(request.wav_path.clone()))?;

    Ok(TranscriptionResult {
        wav_path: canonical_source_path,
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
    /// Target language used for this transcriber (for logging / debugging)
    target_lang: Option<String>,
}

impl StreamingTranscriber {
    /// Create a new streaming transcriber with optional target language.
    pub fn new(
        model_dir: &Path,
        target_lang: Option<impl Into<String>>,
    ) -> Result<Self, TranscriptionError> {
        validate_model_dir(model_dir)?;

        let mut model = Nemotron::from_pretrained(model_dir, None)
            .map_err(|_| TranscriptionError::RecognizerInitializationFailed(model_dir.to_path_buf()))?;

        let lang = target_lang.map(Into::into);
        configure_multilingual(&mut model, lang.as_deref())?;

        Ok(Self {
            model,
            pending: Vec::with_capacity(STREAMING_CHUNK_SAMPLES),
            target_lang: lang,
        })
    }

    /// Feed raw mono 16 kHz f32 samples. Returns the current partial transcript
    /// after processing any complete 560ms windows contained in `samples`
    /// (combined with any leftover samples from previous calls).
    pub fn feed_chunk(&mut self, samples: &[f32]) -> Result<String, TranscriptionError> {
        self.pending.extend_from_slice(samples);

        while self.pending.len() >= STREAMING_CHUNK_SAMPLES {
            let chunk: [f32; STREAMING_CHUNK_SAMPLES] = self.pending[..STREAMING_CHUNK_SAMPLES]
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

    /// Return the current transcript without flushing pending samples.
    pub fn transcript(&self) -> String {
        self.model.get_transcript()
    }

    /// Reset the model state for a new utterance.
    pub fn reset(&mut self) {
        self.model.reset();
        self.pending.clear();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Configure target language for multilingual Nemotron models.
fn configure_multilingual(
    model: &mut Nemotron,
    target_lang: Option<&str>,
) -> Result<(), TranscriptionError> {
    if model.mode() == NemotronMode::Multilingual {
        let lang = target_lang.unwrap_or("fr-FR");
        model
            .set_target_lang(lang)
            .map_err(|e| TranscriptionError::LanguageConfigFailed(format!("lang={}: {}", lang, e)))?;
    }
    Ok(())
}

fn validate_audio_path(audio_path: &Path) -> Result<(), TranscriptionError> {
    if audio_path.is_file() {
        Ok(())
    } else {
        Err(TranscriptionError::MissingWavFile(audio_path.to_path_buf()))
    }
}

struct CachedParakeetTdt {
    model_dir: std::path::PathBuf,
    force_cpu: bool,
    provider: &'static str,
    model: ParakeetTDT,
}

fn asr_cache() -> &'static std::sync::Mutex<Option<CachedParakeetTdt>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<CachedParakeetTdt>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

fn asr_force_cpu() -> bool {
    std::env::var("PEPPERX_ASR_EP")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("cpu"))
}

/// Load a Parakeet TDT model and keep it for the life of the process.
/// A Nemotron directory is ignored. A later call with another directory
/// replaces the cached model.
pub fn prewarm_parakeet_tdt(model_dir: &Path) -> Result<(), TranscriptionError> {
    if !is_parakeet_tdt_dir(model_dir) {
        return Ok(());
    }
    let _cached = cached_parakeet_tdt(model_dir)?;
    Ok(())
}

fn cached_parakeet_tdt(
    model_dir: &Path,
) -> Result<std::sync::MutexGuard<'static, Option<CachedParakeetTdt>>, TranscriptionError> {
    let force_cpu = asr_force_cpu();
    let mut guard = asr_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let reusable = guard.as_ref().is_some_and(|cached| {
        cached.force_cpu == force_cpu && cached.model_dir == model_dir
    });
    if reusable {
        let provider = guard.as_ref().map(|cached| cached.provider).unwrap_or("cpu");
        eprintln!("[Pepper X] ASR reusing cached model ({provider})");
        return Ok(guard);
    }
    let (model, provider) = load_parakeet_tdt(model_dir)?;
    eprintln!("[Pepper X] ASR model cached ({provider})");
    *guard = Some(CachedParakeetTdt {
        model_dir: model_dir.to_path_buf(),
        force_cpu,
        provider,
        model,
    });
    Ok(guard)
}

fn load_parakeet_tdt(model_dir: &Path) -> Result<(ParakeetTDT, &'static str), TranscriptionError> {
    let force_cpu = std::env::var("PEPPERX_ASR_EP")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("cpu"));
    if !force_cpu {
        let config = ExecutionConfig::new().with_execution_provider(ExecutionProvider::Cuda);
        match ParakeetTDT::from_pretrained(model_dir, Some(config)) {
            Ok(model) => {
                eprintln!("[Pepper X] ASR execution provider: cuda");
                return Ok((model, "cuda"));
            }
            Err(error) => {
                eprintln!("[Pepper X] ASR CUDA init failed, falling back to cpu: {error}");
            }
        }
    } else {
        eprintln!("[Pepper X] ASR execution provider: cpu (PEPPERX_ASR_EP=cpu)");
    }
    ParakeetTDT::from_pretrained(model_dir, None)
        .map(|model| (model, "cpu"))
        .map_err(|_| TranscriptionError::RecognizerInitializationFailed(model_dir.to_path_buf()))
}

fn is_parakeet_tdt_dir(model_dir: &Path) -> bool {
    model_dir.join("encoder-model.onnx").is_file()
        && model_dir.join("decoder_joint-model.onnx").is_file()
        && model_dir.join("vocab.txt").is_file()
}

fn transcribe_wav_tdt(
    request: &TranscriptionRequest,
) -> Result<TranscriptionResult, TranscriptionError> {
    let mut cached = cached_parakeet_tdt(&request.model_dir)?;
    let model = &mut cached
        .as_mut()
        .expect("cached parakeet model")
        .model;

    let canonical_source_path = std::fs::canonicalize(&request.wav_path)
        .map_err(|_| TranscriptionError::MissingWavFile(request.wav_path.clone()))?;
    let decoded =
        decode_audio_file(&canonical_source_path).map_err(|error| map_decode_error(error))?;

    let start = Instant::now();
    let transcript = model
        .transcribe_samples(
            decoded.samples,
            TARGET_SAMPLE_RATE,
            1,
            Some(TimestampMode::Words),
        )
        .map_err(|_| TranscriptionError::DecodeFailed(request.wav_path.clone()))?;

    Ok(TranscriptionResult {
        wav_path: canonical_source_path,
        transcript_text: transcript.text,
        backend_name: BACKEND_NAME.to_string(),
        model_name: request.model_name.clone(),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn validate_model_dir(model_dir: &Path) -> Result<(), TranscriptionError> {
    if is_parakeet_tdt_dir(model_dir) {
        required_model_file(model_dir, "encoder-model.onnx.data")?;
        return Ok(());
    }
    // Base files (compatible with both English-only and smcleod INT8 multilingual)
    for file_name in [
        ENCODER_FILE_NAME,
        DECODER_JOINT_FILE_NAME,
        TOKENIZER_FILE_NAME,
    ] {
        if file_name == DECODER_JOINT_FILE_NAME && model_dir.join("decoder.onnx").exists() {
            continue;
        }
        if file_name == TOKENIZER_FILE_NAME && model_dir.join("tokenizer.json").exists() {
            continue;
        }
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

fn map_decode_error(error: DecodeError) -> TranscriptionError {
    match error {
        DecodeError::MissingFile(path) | DecodeError::OpenFailed(path) => {
            TranscriptionError::MissingWavFile(path)
        }
        DecodeError::UnsupportedFormat(path) => TranscriptionError::AudioDecodeFailed {
            path,
            detail: "unsupported audio format".into(),
        },
        DecodeError::NoAudioTrack(path) => TranscriptionError::AudioDecodeFailed {
            path,
            detail: "no audio track found".into(),
        },
        DecodeError::MissingSampleRate(path) => TranscriptionError::AudioDecodeFailed {
            path,
            detail: "missing sample rate metadata".into(),
        },
        DecodeError::EmptyAudio(path) => TranscriptionError::AudioDecodeFailed {
            path,
            detail: "decoded audio is empty".into(),
        },
        DecodeError::DecodeFailed { path, detail } => {
            TranscriptionError::AudioDecodeFailed { path, detail }
        }
    }
}

/// Load any supported audio file as mono f32 at 16 kHz.
///
/// Kept for tests and internal callers; production transcription goes through
/// [`transcribe_wav`].
pub fn load_audio_for_asr(audio_path: &Path) -> Result<(PathBuf, i32, Vec<f32>), TranscriptionError> {
    let canonical = std::fs::canonicalize(audio_path)
        .map_err(|_| TranscriptionError::MissingWavFile(audio_path.to_path_buf()))?;
    let decoded = decode_audio_file(&canonical).map_err(map_decode_error)?;
    Ok((
        canonical,
        decoded.sample_rate as i32,
        decoded.samples,
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
            "nemotron-3.5-0.6b",
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
            "nemotron-3.5-0.6b",
        );

        let error = transcribe_wav(&request).unwrap_err();

        assert!(matches!(error, TranscriptionError::IncompleteModelDir { .. }));
    }

    #[test]
    fn transcriber_accepts_parakeet_tdt_layout_before_load() {
        let model_dir = unique_test_root("tdt-layout");
        fs::create_dir_all(&model_dir).unwrap();
        for name in [
            "encoder-model.onnx",
            "encoder-model.onnx.data",
            "decoder_joint-model.onnx",
            "vocab.txt",
        ] {
            fs::write(model_dir.join(name), b"not-a-model").unwrap();
        }
        let wav_path = model_dir.join("existing.wav");
        fs::copy(fixture_path(), &wav_path).unwrap();

        let error = transcribe_wav(&TranscriptionRequest::new(
            &wav_path,
            &model_dir,
            "parakeet-ultra",
        ))
        .unwrap_err();

        assert!(matches!(
            error,
            TranscriptionError::RecognizerInitializationFailed(_)
        ));
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

        let (normalized_path, sample_rate, samples) = load_audio_for_asr(&wav_path).unwrap();

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
            "nemotron-3.5-0.6b-multilingual",
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
