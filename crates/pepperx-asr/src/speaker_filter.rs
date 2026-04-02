use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Minimum duration of filtered audio below which we fall back to the original.
const FALLBACK_THRESHOLD: Duration = Duration::from_millis(750);

/// Frame length in samples at 16 kHz used for energy analysis.
const FRAME_LENGTH_SAMPLES: usize = 480; // 30 ms at 16 kHz

/// Energy threshold relative to peak frame energy for VAD decisions.
const ENERGY_SILENCE_RATIO: f32 = 0.02;

/// Maximum gap (in frames) between speech segments that we merge together.
const MERGE_GAP_FRAMES: usize = 10; // ~300 ms at 30 ms/frame

/// Number of leading speech frames used to build the target speaker energy profile.
const PROFILE_FRAME_COUNT: usize = 34; // ~1 second of speech

/// Maximum energy ratio difference allowed between a segment and the target
/// speaker profile. Segments whose mean energy deviates beyond this factor are
/// filtered out.
const SPEAKER_ENERGY_TOLERANCE: f32 = 3.0;

/// Result of the speaker filtering pass.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerFilterResult {
    /// Path to the filtered WAV file (may be the same as the input if fallback).
    pub filtered_wav_path: PathBuf,
    /// Whether the filter actually removed any audio.
    pub filtering_applied: bool,
    /// Whether we fell back to the full recording.
    pub fell_back_to_full: bool,
    /// Number of speech segments detected.
    pub segment_count: usize,
    /// Number of segments attributed to the target speaker.
    pub target_speaker_segments: usize,
    /// Duration of the original audio.
    pub original_duration: Duration,
    /// Duration of the filtered audio.
    pub filtered_duration: Duration,
    /// Human-readable reason if fallback occurred.
    pub fallback_reason: Option<String>,
}

/// Errors that can occur during speaker filtering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerFilterError {
    InvalidWavFile(PathBuf),
    IoError(String),
}

impl std::fmt::Display for SpeakerFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidWavFile(path) => {
                write!(f, "invalid WAV file for speaker filtering: {}", path.display())
            }
            Self::IoError(msg) => write!(f, "speaker filter I/O error: {msg}"),
        }
    }
}

impl std::error::Error for SpeakerFilterError {}

/// Apply energy-based speaker filtering to a WAV file.
///
/// The algorithm:
/// 1. Load the WAV and split into fixed-length frames.
/// 2. Compute per-frame RMS energy and identify speech frames via an
///    energy threshold derived from the peak frame energy.
/// 3. Merge adjacent speech frames into contiguous segments.
/// 4. Build a target speaker energy profile from the first speech segment
///    (assumption: the first speaker is the microphone user).
/// 5. Keep only segments whose mean energy is within tolerance of the target
///    profile.
/// 6. Write the filtered samples to `output_wav_path`.
/// 7. If the filtered audio is shorter than `FALLBACK_THRESHOLD`, return the
///    original path instead.
pub fn filter_other_speakers(
    input_wav_path: &Path,
    output_wav_path: &Path,
) -> Result<SpeakerFilterResult, SpeakerFilterError> {
    let (sample_rate, samples) = load_mono_wav(input_wav_path)?;
    let original_duration = samples_to_duration(samples.len(), sample_rate);

    // Edge case: very short recordings should not be filtered.
    if original_duration < FALLBACK_THRESHOLD {
        return Ok(SpeakerFilterResult {
            filtered_wav_path: input_wav_path.to_path_buf(),
            filtering_applied: false,
            fell_back_to_full: true,
            segment_count: 0,
            target_speaker_segments: 0,
            original_duration,
            filtered_duration: original_duration,
            fallback_reason: Some("recording too short for speaker filtering".into()),
        });
    }

    let frame_energies = compute_frame_energies(&samples);
    let speech_mask = build_speech_mask(&frame_energies);
    let segments = merge_speech_segments(&speech_mask);

    if segments.is_empty() {
        return Ok(SpeakerFilterResult {
            filtered_wav_path: input_wav_path.to_path_buf(),
            filtering_applied: false,
            fell_back_to_full: true,
            segment_count: 0,
            target_speaker_segments: 0,
            original_duration,
            filtered_duration: original_duration,
            fallback_reason: Some("no speech segments detected".into()),
        });
    }

    let target_energy = target_speaker_energy(&frame_energies, &segments);
    let target_segments = filter_segments_by_energy(&frame_energies, &segments, target_energy);
    let segment_count = segments.len();
    let target_speaker_segments = target_segments.len();

    if target_segments.is_empty() {
        return Ok(SpeakerFilterResult {
            filtered_wav_path: input_wav_path.to_path_buf(),
            filtering_applied: false,
            fell_back_to_full: true,
            segment_count,
            target_speaker_segments: 0,
            original_duration,
            filtered_duration: original_duration,
            fallback_reason: Some("no segments matched target speaker profile".into()),
        });
    }

    let filtered_samples = extract_segment_samples(&samples, &target_segments);
    let filtered_duration = samples_to_duration(filtered_samples.len(), sample_rate);

    if filtered_duration < FALLBACK_THRESHOLD {
        return Ok(SpeakerFilterResult {
            filtered_wav_path: input_wav_path.to_path_buf(),
            filtering_applied: false,
            fell_back_to_full: true,
            segment_count,
            target_speaker_segments,
            original_duration,
            filtered_duration,
            fallback_reason: Some(format!(
                "filtered audio too short ({:.2}s < {:.2}s threshold)",
                filtered_duration.as_secs_f64(),
                FALLBACK_THRESHOLD.as_secs_f64(),
            )),
        });
    }

    // No actual filtering happened if all segments were kept.
    let filtering_applied = target_speaker_segments < segment_count;

    if !filtering_applied {
        return Ok(SpeakerFilterResult {
            filtered_wav_path: input_wav_path.to_path_buf(),
            filtering_applied: false,
            fell_back_to_full: false,
            segment_count,
            target_speaker_segments,
            original_duration,
            filtered_duration: original_duration,
            fallback_reason: None,
        });
    }

    write_mono_wav(output_wav_path, sample_rate, &filtered_samples)?;

    Ok(SpeakerFilterResult {
        filtered_wav_path: output_wav_path.to_path_buf(),
        filtering_applied: true,
        fell_back_to_full: false,
        segment_count,
        target_speaker_segments,
        original_duration,
        filtered_duration,
        fallback_reason: None,
    })
}

/// A contiguous range of frames identified as a speech segment.
#[derive(Debug, Clone, Copy)]
struct SpeechSegment {
    start_frame: usize,
    end_frame: usize, // exclusive
}

impl SpeechSegment {
    fn sample_range(&self) -> (usize, usize) {
        (
            self.start_frame * FRAME_LENGTH_SAMPLES,
            self.end_frame * FRAME_LENGTH_SAMPLES,
        )
    }
}

fn load_mono_wav(path: &Path) -> Result<(u32, Vec<f32>), SpeakerFilterError> {
    let mut reader =
        WavReader::open(path).map_err(|_| SpeakerFilterError::InvalidWavFile(path.to_path_buf()))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;

    let raw_samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SpeakerFilterError::InvalidWavFile(path.to_path_buf()))?,
        SampleFormat::Int if spec.bits_per_sample <= 16 => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| SpeakerFilterError::InvalidWavFile(path.to_path_buf()))?,
        SampleFormat::Int => {
            let scale = ((1_i64 << (spec.bits_per_sample - 1)) - 1) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| SpeakerFilterError::InvalidWavFile(path.to_path_buf()))?
        }
    };

    // Mix to mono if multi-channel.
    let channels = spec.channels as usize;
    let mono_samples = if channels <= 1 {
        raw_samples
    } else {
        raw_samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    Ok((sample_rate, mono_samples))
}

fn write_mono_wav(
    path: &Path,
    sample_rate: u32,
    samples: &[f32],
) -> Result<(), SpeakerFilterError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SpeakerFilterError::IoError(format!("create dir: {e}")))?;
    }

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::create(path, spec)
        .map_err(|e| SpeakerFilterError::IoError(format!("create wav: {e}")))?;

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm = (clamped * i16::MAX as f32).round() as i16;
        writer
            .write_sample(pcm)
            .map_err(|e| SpeakerFilterError::IoError(format!("write sample: {e}")))?;
    }

    writer
        .finalize()
        .map_err(|e| SpeakerFilterError::IoError(format!("finalize wav: {e}")))?;

    Ok(())
}

fn compute_frame_energies(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks(FRAME_LENGTH_SAMPLES)
        .map(|frame| {
            let sum_sq: f32 = frame.iter().map(|s| s * s).sum();
            (sum_sq / frame.len() as f32).sqrt()
        })
        .collect()
}

fn build_speech_mask(frame_energies: &[f32]) -> Vec<bool> {
    let peak_energy = frame_energies
        .iter()
        .copied()
        .fold(0.0_f32, f32::max);

    if peak_energy < f32::EPSILON {
        return vec![false; frame_energies.len()];
    }

    let threshold = peak_energy * ENERGY_SILENCE_RATIO;
    frame_energies
        .iter()
        .map(|&energy| energy >= threshold)
        .collect()
}

fn merge_speech_segments(speech_mask: &[bool]) -> Vec<SpeechSegment> {
    let mut segments = Vec::new();
    let mut current_start: Option<usize> = None;

    for (i, &is_speech) in speech_mask.iter().enumerate() {
        match (is_speech, current_start) {
            (true, None) => {
                current_start = Some(i);
            }
            (false, Some(start)) => {
                // Check if the gap is small enough to bridge.
                let gap_end = speech_mask
                    .iter()
                    .skip(i)
                    .take(MERGE_GAP_FRAMES)
                    .position(|&s| s);

                if gap_end.is_none() {
                    segments.push(SpeechSegment {
                        start_frame: start,
                        end_frame: i,
                    });
                    current_start = None;
                }
                // Otherwise keep going — we bridge the gap.
            }
            _ => {}
        }
    }

    if let Some(start) = current_start {
        segments.push(SpeechSegment {
            start_frame: start,
            end_frame: speech_mask.len(),
        });
    }

    segments
}

fn target_speaker_energy(
    frame_energies: &[f32],
    segments: &[SpeechSegment],
) -> f32 {
    let mut profile_frames = Vec::new();
    for segment in segments {
        for frame_idx in segment.start_frame..segment.end_frame {
            if frame_idx < frame_energies.len() {
                profile_frames.push(frame_energies[frame_idx]);
            }
            if profile_frames.len() >= PROFILE_FRAME_COUNT {
                break;
            }
        }
        if profile_frames.len() >= PROFILE_FRAME_COUNT {
            break;
        }
    }

    if profile_frames.is_empty() {
        return 0.0;
    }

    profile_frames.iter().sum::<f32>() / profile_frames.len() as f32
}

fn filter_segments_by_energy(
    frame_energies: &[f32],
    segments: &[SpeechSegment],
    target_energy: f32,
) -> Vec<SpeechSegment> {
    if target_energy < f32::EPSILON {
        return segments.to_vec();
    }

    segments
        .iter()
        .filter(|segment| {
            let segment_energies: Vec<f32> = (segment.start_frame..segment.end_frame)
                .filter_map(|i| frame_energies.get(i).copied())
                .collect();

            if segment_energies.is_empty() {
                return false;
            }

            let mean_energy =
                segment_energies.iter().sum::<f32>() / segment_energies.len() as f32;
            let ratio = mean_energy / target_energy;

            // Accept if the segment energy is within tolerance of the target.
            ratio >= (1.0 / SPEAKER_ENERGY_TOLERANCE) && ratio <= SPEAKER_ENERGY_TOLERANCE
        })
        .copied()
        .collect()
}

fn extract_segment_samples(samples: &[f32], segments: &[SpeechSegment]) -> Vec<f32> {
    let mut output = Vec::new();
    for segment in segments {
        let (start, end) = segment.sample_range();
        let end = end.min(samples.len());
        let start = start.min(end);
        output.extend_from_slice(&samples[start..end]);
    }
    output
}

fn samples_to_duration(sample_count: usize, sample_rate: u32) -> Duration {
    if sample_rate == 0 || sample_count == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(sample_count as f64 / sample_rate as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_wav_path(suffix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pepper-x-speaker-filter-{suffix}-{unique}.wav"))
    }

    fn write_test_wav(path: &Path, sample_rate: u32, samples: &[f32]) {
        write_mono_wav(path, sample_rate, samples).expect("should write test wav");
    }

    fn sine_wave(frequency: f32, sample_rate: u32, duration_secs: f32, amplitude: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                amplitude
                    * (2.0 * std::f32::consts::PI * frequency * i as f32 / sample_rate as f32)
                        .sin()
            })
            .collect()
    }

    fn silence(sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        vec![0.0; (sample_rate as f32 * duration_secs) as usize]
    }

    #[test]
    fn falls_back_when_recording_is_too_short() {
        let input = unique_wav_path("short-input");
        // 0.5s at 16 kHz — below the 0.75s threshold.
        write_test_wav(&input, 16_000, &sine_wave(440.0, 16_000, 0.5, 0.5));

        let output = unique_wav_path("short-output");
        let result =
            filter_other_speakers(&input, &output).expect("filter should succeed");

        assert!(!result.filtering_applied);
        assert!(result.fell_back_to_full);
        assert_eq!(result.filtered_wav_path, input);
        assert!(result.fallback_reason.is_some());

        let _ = std::fs::remove_file(&input);
    }

    #[test]
    fn passes_through_single_speaker_recording() {
        let input = unique_wav_path("single-speaker-input");
        // 2 seconds of a single tone — simulates one speaker.
        write_test_wav(&input, 16_000, &sine_wave(440.0, 16_000, 2.0, 0.5));

        let output = unique_wav_path("single-speaker-output");
        let result =
            filter_other_speakers(&input, &output).expect("filter should succeed");

        // All segments match the target speaker, so no filtering is needed.
        assert!(!result.filtering_applied);
        assert!(!result.fell_back_to_full);
        assert!(result.segment_count > 0);
        assert_eq!(result.target_speaker_segments, result.segment_count);

        let _ = std::fs::remove_file(&input);
    }

    #[test]
    fn filters_loud_other_speaker() {
        let input = unique_wav_path("two-speaker-input");
        let mut samples = Vec::new();

        // Speaker 1 (target): 1.5s of moderate speech at amplitude 0.15
        samples.extend(sine_wave(300.0, 16_000, 1.5, 0.15));
        // Long gap to ensure separate segments
        samples.extend(silence(16_000, 1.0));
        // Speaker 2 (other): 1.5s of very loud speech at amplitude 0.95
        samples.extend(sine_wave(600.0, 16_000, 1.5, 0.95));
        // Long gap
        samples.extend(silence(16_000, 1.0));
        // Speaker 1 again: 1s at amplitude 0.15
        samples.extend(sine_wave(300.0, 16_000, 1.0, 0.15));

        write_test_wav(&input, 16_000, &samples);

        let output = unique_wav_path("two-speaker-output");
        let result =
            filter_other_speakers(&input, &output).expect("filter should succeed");

        assert!(result.filtering_applied);
        assert!(!result.fell_back_to_full);
        assert!(result.target_speaker_segments < result.segment_count);
        assert!(result.filtered_duration < result.original_duration);

        let _ = std::fs::remove_file(&input);
        let _ = std::fs::remove_file(&output);
    }

    #[test]
    fn falls_back_when_no_speech_detected() {
        let input = unique_wav_path("silence-input");
        // 2 seconds of silence
        write_test_wav(&input, 16_000, &silence(16_000, 2.0));

        let output = unique_wav_path("silence-output");
        let result =
            filter_other_speakers(&input, &output).expect("filter should succeed");

        assert!(!result.filtering_applied);
        assert!(result.fell_back_to_full);
        assert_eq!(result.segment_count, 0);

        let _ = std::fs::remove_file(&input);
    }

    #[test]
    fn frame_energy_computation_produces_correct_values() {
        // A frame of all 0.5 should have RMS = 0.5.
        let frame = vec![0.5_f32; FRAME_LENGTH_SAMPLES];
        let energies = compute_frame_energies(&frame);
        assert_eq!(energies.len(), 1);
        assert!((energies[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn speech_mask_identifies_loud_frames() {
        let mut energies = vec![0.001; 20]; // silence
        energies[5] = 0.5; // speech
        energies[6] = 0.4; // speech
        energies[7] = 0.3; // speech

        let mask = build_speech_mask(&energies);
        assert!(!mask[0]);
        assert!(mask[5]);
        assert!(mask[6]);
        assert!(mask[7]);
        assert!(!mask[10]);
    }

    #[test]
    fn segment_merging_bridges_short_gaps() {
        let mut mask = vec![false; 30];
        // Two speech bursts with a short gap.
        mask[2] = true;
        mask[3] = true;
        mask[4] = true;
        // gap at 5..8 (3 frames — within MERGE_GAP_FRAMES)
        mask[8] = true;
        mask[9] = true;
        mask[10] = true;

        let segments = merge_speech_segments(&mask);

        // Should merge into a single segment.
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_frame, 2);
        assert_eq!(segments[0].end_frame, 11);
    }

    #[test]
    fn samples_to_duration_handles_edge_cases() {
        assert_eq!(samples_to_duration(0, 16_000), Duration::ZERO);
        assert_eq!(samples_to_duration(16_000, 0), Duration::ZERO);
        assert_eq!(samples_to_duration(16_000, 16_000), Duration::from_secs(1));
    }
}
