//! Decode common audio containers to mono PCM at the ASR target sample rate.
//!
//! Uses [Symphonia](https://github.com/pdeljanov/Symphonia) (pure Rust). Live
//! capture still produces 16 kHz mono WAV via PipeWire + hound; this module is
//! for batch import of external files (WAV, MP3, FLAC, OGG/Vorbis, AAC/M4A, …).

use std::fs::File;
use std::path::{Path, PathBuf};

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Sample rate expected by the Nemotron / Parakeet ASR backends.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Mono PCM ready for ASR (`TARGET_SAMPLE_RATE`).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl DecodedAudio {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    MissingFile(PathBuf),
    OpenFailed(PathBuf),
    UnsupportedFormat(PathBuf),
    NoAudioTrack(PathBuf),
    MissingSampleRate(PathBuf),
    DecodeFailed { path: PathBuf, detail: String },
    EmptyAudio(PathBuf),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingFile(path) => {
                write!(f, "audio file does not exist: {}", path.display())
            }
            Self::OpenFailed(path) => write!(f, "failed to open audio file: {}", path.display()),
            Self::UnsupportedFormat(path) => {
                write!(f, "unsupported audio format: {}", path.display())
            }
            Self::NoAudioTrack(path) => {
                write!(f, "no audio track in file: {}", path.display())
            }
            Self::MissingSampleRate(path) => {
                write!(f, "audio file missing sample rate: {}", path.display())
            }
            Self::DecodeFailed { path, detail } => {
                write!(f, "failed to decode {}: {detail}", path.display())
            }
            Self::EmptyAudio(path) => write!(f, "audio file is empty: {}", path.display()),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Extensions accepted for external audio import (case-insensitive).
pub const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "wave", "mp3", "flac", "ogg", "oga", "opus", "aac", "m4a", "mp4", "caf", "aiff", "aif",
];

/// Return true when `path` has a known importable audio extension.
pub fn is_supported_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            SUPPORTED_AUDIO_EXTENSIONS
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
        .unwrap_or(false)
}

/// Decode any supported audio file to mono f32 at [`TARGET_SAMPLE_RATE`].
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio, DecodeError> {
    if !path.is_file() {
        return Err(DecodeError::MissingFile(path.to_path_buf()));
    }

    let file = File::open(path).map_err(|_| DecodeError::OpenFailed(path.to_path_buf()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|error| match error {
            SymphoniaError::Unsupported(_) => DecodeError::UnsupportedFormat(path.to_path_buf()),
            other => DecodeError::DecodeFailed {
                path: path.to_path_buf(),
                detail: other.to_string(),
            },
        })?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| DecodeError::NoAudioTrack(path.to_path_buf()))?
        .clone();

    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| DecodeError::MissingSampleRate(path.to_path_buf()))?;

    let channel_count = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1)
        .max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| DecodeError::DecodeFailed {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;

    let track_id = track.id;
    let mut interleaved: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::ResetRequired) => {
                // Seek-less decode: treat reset as end-of-stream for batch import.
                break;
            }
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::DecodeError(_)) => {
                // Skip corrupt packets when possible.
                continue;
            }
            Err(error) => {
                // Many demuxers surface EOF as a generic error after the last packet.
                if interleaved.is_empty() {
                    return Err(DecodeError::DecodeFailed {
                        path: path.to_path_buf(),
                        detail: error.to_string(),
                    });
                }
                break;
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => append_planar_or_interleaved_f32(&decoded, &mut interleaved),
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => {
                return Err(DecodeError::DecodeFailed {
                    path: path.to_path_buf(),
                    detail: error.to_string(),
                });
            }
        }
    }

    if interleaved.is_empty() {
        return Err(DecodeError::EmptyAudio(path.to_path_buf()));
    }

    let mono = mix_to_mono(&interleaved, channel_count);
    let samples = resample_mono(&mono, sample_rate, TARGET_SAMPLE_RATE);

    if samples.is_empty() {
        return Err(DecodeError::EmptyAudio(path.to_path_buf()));
    }

    Ok(DecodedAudio {
        samples,
        sample_rate: TARGET_SAMPLE_RATE,
    })
}

/// Decode any supported audio file and write a temporary mono 16 kHz 16-bit PCM WAV.
///
/// Used by History import so Nemotron always sees a canonical WAV, regardless of
/// the source container (FLAC/MP3/OGG/…). Caller owns cleanup of the returned path
/// (see [`TempMonoWav`]).
pub fn convert_to_temp_mono_16k_wav(path: &Path) -> Result<TempMonoWav, DecodeError> {
    let decoded = decode_audio_file(path)?;
    write_temp_mono_16k_wav(&decoded, path)
}

/// RAII guard for a temporary mono 16 kHz WAV produced for ASR.
#[derive(Debug)]
pub struct TempMonoWav {
    pub path: PathBuf,
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl TempMonoWav {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempMonoWav {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_temp_mono_16k_wav(
    decoded: &DecodedAudio,
    source_path: &Path,
) -> Result<TempMonoWav, DecodeError> {
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("audio");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "pepper-x-import-{}-{}-{unique}.wav",
        sanitize_stem(stem),
        std::process::id()
    ));

    write_mono_16k_wav(&path, &decoded.samples).map_err(|detail| DecodeError::DecodeFailed {
        path: source_path.to_path_buf(),
        detail: format!("failed to write temp mono WAV {}: {detail}", path.display()),
    })?;

    Ok(TempMonoWav {
        path,
        samples: decoded.samples.clone(),
        sample_rate: decoded.sample_rate,
    })
}

/// Write mono PCM as 16-bit integer WAV at [`TARGET_SAMPLE_RATE`].
pub fn write_mono_16k_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let amplitude = (clamped * i16::MAX as f32).round() as i16;
        writer.write_sample(amplitude).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(())
}

fn sanitize_stem(stem: &str) -> String {
    stem.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect()
}

/// Mix interleaved multi-channel samples down to mono by averaging channels.
fn mix_to_mono(interleaved: &[f32], channel_count: usize) -> Vec<f32> {
    if channel_count <= 1 {
        return interleaved.to_vec();
    }

    let frames = interleaved.len() / channel_count;
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let base = frame * channel_count;
        let mut sum = 0.0f32;
        for ch in 0..channel_count {
            sum += interleaved[base + ch];
        }
        mono.push(sum / channel_count as f32);
    }
    mono
}

/// Minimum input length before we fan out across rayon workers.
/// Below this, single-thread rubato is cheaper (no spawn / join overhead).
const PARALLEL_RESAMPLE_THRESHOLD: usize = 240_000; // ~5 s @ 48 kHz

/// Input chunk size for parallel segments (~2 s @ 48 kHz).
const PARALLEL_CHUNK_INPUT: usize = 96_000;

/// Rubato FFT chunk size (frames). Good speed/quality tradeoff for speech.
const RUBATO_CHUNK_FRAMES: usize = 1024;

/// High-quality mono resampler (rubato FFT).
///
/// Large clips are split into independent chunks and resampled in parallel with
/// rayon — each worker owns its own [`FftFixedIn`] so there is no shared state.
/// Short clips stay single-threaded to avoid thread-pool overhead.
pub fn resample_mono(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return samples.to_vec();
    }

    if samples.len() < PARALLEL_RESAMPLE_THRESHOLD || rayon::current_num_threads() <= 1 {
        return resample_mono_rubato(samples, from_rate, to_rate)
            .unwrap_or_else(|_| resample_mono_linear(samples, from_rate, to_rate));
    }

    resample_mono_parallel(samples, from_rate, to_rate)
}

fn resample_mono_parallel(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    use rayon::prelude::*;

    let chunk_in = PARALLEL_CHUNK_INPUT.max(RUBATO_CHUNK_FRAMES * 4);
    // Overlap so filter warm-up at segment edges is discarded (speech-safe).
    let overlap = (from_rate as usize / 20).max(512).min(chunk_in / 4); // ~50 ms
    let ratio = f64::from(to_rate) / f64::from(from_rate);

    let starts: Vec<usize> = (0..samples.len()).step_by(chunk_in).collect();
    let pieces: Vec<Vec<f32>> = starts
        .into_par_iter()
        .map(|start| {
            let end = (start + chunk_in).min(samples.len());
            let pad_start = start.saturating_sub(overlap);
            let pad_end = (end + overlap).min(samples.len());
            let padded = &samples[pad_start..pad_end];

            let resampled = resample_mono_rubato(padded, from_rate, to_rate)
                .unwrap_or_else(|_| resample_mono_linear(padded, from_rate, to_rate));

            let skip = if start == 0 {
                0
            } else {
                ((start - pad_start) as f64 * ratio).round() as usize
            };
            let keep = ((end - start) as f64 * ratio).round() as usize;
            let slice_start = skip.min(resampled.len());
            let slice_end = (slice_start + keep).min(resampled.len());
            resampled[slice_start..slice_end].to_vec()
        })
        .collect();

    let expected = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(expected);
    for piece in pieces {
        out.extend_from_slice(&piece);
    }
    // Length can drift by a few samples from rounding; trim or pad for stability.
    if out.len() > expected {
        out.truncate(expected);
    } else if out.len() < expected {
        out.resize(expected, 0.0);
    }
    out
}

/// Offline rubato FFT resample following the crate's recommended clip procedure.
fn resample_mono_rubato(samples: &[f32], from_rate: u32, to_rate: u32) -> Result<Vec<f32>, String> {
    use rubato::{FftFixedIn, Resampler};

    let from = from_rate as usize;
    let to = to_rate as usize;
    let mut resampler = FftFixedIn::<f32>::new(from, to, RUBATO_CHUNK_FRAMES, 2, 1)
        .map_err(|error| error.to_string())?;

    let delay = resampler.output_delay();
    let new_length = ((samples.len() as f64) * (to as f64) / (from as f64))
        .round()
        .max(1.0) as usize;
    let mut output = Vec::with_capacity(new_length + delay + RUBATO_CHUNK_FRAMES);
    let mut pos = 0;

    // Bulk of the clip: full input frames.
    loop {
        let needed = resampler.input_frames_next();
        if needed == 0 || pos + needed > samples.len() {
            break;
        }
        let chunk = &samples[pos..pos + needed];
        let waves_out = resampler
            .process(&[chunk], None)
            .map_err(|error| error.to_string())?;
        output.extend_from_slice(&waves_out[0]);
        pos += needed;
    }

    // Remainder of the clip.
    if pos < samples.len() {
        let rem = &samples[pos..];
        let waves_out = resampler
            .process_partial(Some(&[rem]), None)
            .map_err(|error| error.to_string())?;
        output.extend_from_slice(&waves_out[0]);
    }

    // Drain internal delay buffers.
    while output.len() < new_length + delay {
        let waves_out = resampler
            .process_partial(None::<&[&[f32]]>, None)
            .map_err(|error| error.to_string())?;
        if waves_out[0].is_empty() {
            break;
        }
        output.extend_from_slice(&waves_out[0]);
    }

    let start = delay.min(output.len());
    let end = (start + new_length).min(output.len());
    Ok(output[start..end].to_vec())
}

/// Fallback linear resampler if rubato construction/process fails.
fn resample_mono_linear(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if samples.is_empty() || from_rate == 0 || to_rate == 0 {
        return Vec::new();
    }
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = f64::from(to_rate) / f64::from(from_rate);
    let out_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(out_len);
    let last = samples.len() - 1;

    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        if idx >= last {
            out.push(samples[last]);
        } else {
            let s0 = samples[idx];
            let s1 = samples[idx + 1];
            out.push(s0 + (s1 - s0) * frac);
        }
    }
    out
}

fn append_planar_or_interleaved_f32(decoded: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    match decoded {
        AudioBufferRef::F32(buf) => append_from_signal(buf, out),
        AudioBufferRef::U8(buf) => append_converted(buf, out, |s| (f32::from(s) - 128.0) / 128.0),
        AudioBufferRef::U16(buf) => {
            append_converted(buf, out, |s| (f32::from(s) - 32768.0) / 32768.0)
        }
        AudioBufferRef::U24(buf) => append_converted(buf, out, |s| {
            let v = s.inner() as f32;
            (v - 8_388_608.0) / 8_388_608.0
        }),
        AudioBufferRef::U32(buf) => {
            append_converted(buf, out, |s| (s as f64 / 2_147_483_648.0 - 1.0) as f32)
        }
        AudioBufferRef::S8(buf) => append_converted(buf, out, |s| f32::from(s) / 128.0),
        AudioBufferRef::S16(buf) => append_converted(buf, out, |s| f32::from(s) / 32768.0),
        AudioBufferRef::S24(buf) => append_converted(buf, out, |s| s.inner() as f32 / 8_388_608.0),
        AudioBufferRef::S32(buf) => {
            append_converted(buf, out, |s| (s as f64 / 2_147_483_648.0) as f32)
        }
        AudioBufferRef::F64(buf) => append_converted(buf, out, |s| s as f32),
    }
}

fn append_from_signal(buf: &symphonia::core::audio::AudioBuffer<f32>, out: &mut Vec<f32>) {
    let channels = buf.spec().channels.count();
    let frames = buf.frames();
    out.reserve(frames * channels);
    for frame in 0..frames {
        for ch in 0..channels {
            out.push(buf.chan(ch)[frame]);
        }
    }
}

fn append_converted<S, F>(
    buf: &symphonia::core::audio::AudioBuffer<S>,
    out: &mut Vec<f32>,
    convert: F,
) where
    S: symphonia::core::sample::Sample + Copy,
    F: Fn(S) -> f32,
{
    let channels = buf.spec().channels.count();
    let frames = buf.frames();
    out.reserve(frames * channels);
    for frame in 0..frames {
        for ch in 0..channels {
            out.push(convert(buf.chan(ch)[frame]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn supported_extensions_cover_common_formats() {
        assert!(is_supported_audio_extension(Path::new("a.wav")));
        assert!(is_supported_audio_extension(Path::new("a.MP3")));
        assert!(is_supported_audio_extension(Path::new("a.flac")));
        assert!(is_supported_audio_extension(Path::new("a.ogg")));
        assert!(is_supported_audio_extension(Path::new("a.m4a")));
        assert!(is_supported_audio_extension(Path::new("a.aac")));
        assert!(!is_supported_audio_extension(Path::new("a.txt")));
        assert!(!is_supported_audio_extension(Path::new("a")));
    }

    #[test]
    fn resample_mono_identity_when_rates_match() {
        let samples = vec![0.0, 0.5, -0.5, 1.0];
        assert_eq!(resample_mono(&samples, 16_000, 16_000), samples);
    }

    #[test]
    fn resample_mono_downsamples_length() {
        // 32 kHz → 16 kHz: roughly half the samples (short = sequential path).
        let samples: Vec<f32> = (0..32_000).map(|i| (i as f32 * 0.001).sin()).collect();
        let out = resample_mono(&samples, 32_000, 16_000);
        assert!((out.len() as i32 - 16_000).abs() <= 8, "len={}", out.len());
    }

    #[test]
    fn resample_mono_parallel_path_matches_expected_length() {
        // Above PARALLEL_RESAMPLE_THRESHOLD so rayon path is used when threads > 1.
        let samples: Vec<f32> = (0..300_000).map(|i| (i as f32 * 0.0005).sin()).collect();
        let out = resample_mono(&samples, 48_000, 16_000);
        let expected = ((300_000.0_f64) * 16_000.0 / 48_000.0).round() as i32;
        assert!(
            (out.len() as i32 - expected).abs() <= 16,
            "len={} expected={}",
            out.len(),
            expected
        );
    }

    #[test]
    fn decode_mono_16k_wav_fixture_or_synthetic() {
        let root = unique_tmp("decode-wav");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("tone.wav");
        write_sine_wav(&path, 16_000, 1, 0.25);

        let decoded = decode_audio_file(&path).expect("decode synthetic wav");
        assert_eq!(decoded.sample_rate, TARGET_SAMPLE_RATE);
        assert!(!decoded.samples.is_empty());
        // ~0.25 s at 16 kHz
        assert!((decoded.samples.len() as i32 - 4_000).abs() < 200);
    }

    #[test]
    fn decode_stereo_48k_wav_is_mixed_and_resampled() {
        let root = unique_tmp("decode-stereo");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("stereo.wav");
        write_sine_wav(&path, 48_000, 2, 0.1);

        let decoded = decode_audio_file(&path).expect("decode stereo wav");
        assert_eq!(decoded.sample_rate, TARGET_SAMPLE_RATE);
        // 0.1 s at 16 kHz ≈ 1600 samples
        assert!((decoded.samples.len() as i32 - 1_600).abs() < 100);
    }

    #[test]
    fn decode_missing_file_errors() {
        let err = decode_audio_file(Path::new("/tmp/pepper-x-missing-audio-xyz.wav")).unwrap_err();
        assert!(matches!(err, DecodeError::MissingFile(_)));
    }

    #[test]
    fn convert_to_temp_mono_16k_wav_writes_canonical_wav() {
        let root = unique_tmp("temp-wav");
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("stereo.wav");
        write_sine_wav(&source, 48_000, 2, 0.05);

        let temp = convert_to_temp_mono_16k_wav(&source).expect("convert");
        assert!(temp.path.is_file());
        assert_eq!(temp.sample_rate, TARGET_SAMPLE_RATE);

        let reader = hound::WavReader::open(temp.path()).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, TARGET_SAMPLE_RATE);
        assert_eq!(spec.bits_per_sample, 16);
        drop(temp);
    }

    fn write_sine_wav(path: &Path, sample_rate: u32, channels: u16, duration_secs: f32) {
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        let total_frames = (sample_rate as f32 * duration_secs) as usize;
        for n in 0..total_frames {
            let t = n as f32 / sample_rate as f32;
            let sample = (t * 440.0 * std::f32::consts::TAU).sin();
            let amplitude = (sample * i16::MAX as f32 * 0.2) as i16;
            for _ in 0..channels {
                writer.write_sample(amplitude).unwrap();
            }
        }
        writer.finalize().unwrap();
    }

    fn unique_tmp(suffix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pepper-x-decoder-{suffix}-{unique}"))
    }
}
