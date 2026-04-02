use crate::SelectedMicrophone;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(target_os = "linux")]
use crate::devices::stable_pipewire_microphone_id;
#[cfg(target_os = "linux")]
use pipewire as pw;
#[cfg(target_os = "linux")]
use pw::{properties::properties, spa};
#[cfg(target_os = "linux")]
use spa::param::format::{MediaSubtype, MediaType};
#[cfg(target_os = "linux")]
use spa::param::format_utils;
#[cfg(target_os = "linux")]
use spa::pod::Pod;
#[cfg(target_os = "linux")]
use std::cell::{Cell, RefCell};
#[cfg(target_os = "linux")]
use std::rc::Rc;
#[cfg(target_os = "linux")]
use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(target_os = "linux")]
const APP_ID: &str = "com.obra.PepperX";
#[cfg(target_os = "linux")]
const STREAM_NAME: &str = "pepperx-microphone-capture";
const SIGNAL_LEVEL_THRESHOLD: f32 = 0.02;
#[cfg(target_os = "linux")]
const SIGNAL_PROBE_TIMEOUT: Duration = Duration::from_millis(300);
#[cfg(target_os = "linux")]
const SIGNAL_PROBE_POLL_INTERVAL: Duration = Duration::from_millis(25);
#[cfg(target_os = "linux")]
const POST_STOP_FLUSH_DURATION: Duration = Duration::from_millis(200);
#[cfg(target_os = "linux")]
const POST_STOP_FLUSH_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Number of f32 samples in a 560ms chunk at 16 kHz, matching the streaming
/// transcriber's window size.
const STREAMING_CHUNK_SAMPLES: usize = 8960;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingRequest {
    output_wav_path: PathBuf,
    selected_microphone: Option<SelectedMicrophone>,
}

impl RecordingRequest {
    pub fn new(
        output_wav_path: impl Into<PathBuf>,
        selected_microphone: Option<SelectedMicrophone>,
    ) -> Self {
        Self {
            output_wav_path: output_wav_path.into(),
            selected_microphone,
        }
    }

    pub fn output_wav_path(&self) -> &Path {
        &self.output_wav_path
    }

    pub fn selected_microphone(&self) -> Option<&SelectedMicrophone> {
        self.selected_microphone.as_ref()
    }
}

/// A sender that receives audio chunks (Vec<f32>) during recording for
/// streaming transcription.  The chunks are 560ms worth of mono 16 kHz f32
/// samples (8960 samples each).
pub type ChunkSink = std::sync::mpsc::Sender<Vec<f32>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingArtifact {
    wav_path: PathBuf,
    selected_microphone: Option<SelectedMicrophone>,
    elapsed: Duration,
}

impl RecordingArtifact {
    pub fn new(
        wav_path: impl Into<PathBuf>,
        selected_microphone: Option<SelectedMicrophone>,
        elapsed: Duration,
    ) -> Self {
        Self {
            wav_path: wav_path.into(),
            selected_microphone,
            elapsed,
        }
    }

    pub fn wav_path(&self) -> &Path {
        &self.wav_path
    }

    pub fn selected_microphone(&self) -> Option<&SelectedMicrophone> {
        self.selected_microphone.as_ref()
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingError {
    message: String,
}

impl RecordingError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(dead_code)]
    fn unsupported_platform() -> Self {
        Self::new("live recording is only supported on linux")
    }
}

impl fmt::Display for RecordingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RecordingError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalLevelSample {
    normalized_level: f32,
}

impl SignalLevelSample {
    pub fn from_interleaved_samples(interleaved_samples: &[f32]) -> Self {
        Self::from_normalized_samples(interleaved_samples)
    }

    pub fn from_pcm_samples(pcm_samples: &[i16]) -> Self {
        let normalized_samples = pcm_samples
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32)
            .collect::<Vec<_>>();
        Self::from_normalized_samples(&normalized_samples)
    }

    pub(crate) fn from_normalized_samples(normalized_samples: &[f32]) -> Self {
        let normalized_level = normalized_samples
            .iter()
            .copied()
            .map(f32::abs)
            .fold(0.0, f32::max);

        Self { normalized_level }
    }

    pub fn normalized_level(&self) -> f32 {
        self.normalized_level
    }

    pub fn signal_present(&self) -> bool {
        self.normalized_level >= SIGNAL_LEVEL_THRESHOLD
    }

    pub fn peak_fraction(&self) -> f32 {
        self.normalized_level()
    }

    pub fn has_signal(&self) -> bool {
        self.signal_present()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalLevelErrorKind {
    UnsupportedPlatform,
    NoSignal,
    CaptureFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalLevelError {
    kind: SignalLevelErrorKind,
    message: String,
}

impl SignalLevelError {
    pub fn new(kind: SignalLevelErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> SignalLevelErrorKind {
        self.kind
    }

    #[cfg(not(target_os = "linux"))]
    fn unsupported_platform() -> Self {
        Self::new(
            SignalLevelErrorKind::UnsupportedPlatform,
            "microphone signal checks are only supported on linux",
        )
    }
}

impl fmt::Display for SignalLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SignalLevelError {}

#[cfg(target_os = "linux")]
enum RecordingCommand {
    Stop,
}

pub struct ActiveRecording {
    request: RecordingRequest,
    #[cfg(target_os = "linux")]
    control_sender: pw::channel::Sender<RecordingCommand>,
    #[cfg(target_os = "linux")]
    worker: Option<thread::JoinHandle<Result<RecordingArtifact, RecordingError>>>,
    /// Optional channel for streaming audio chunks to an external consumer
    /// (e.g. a streaming transcriber).  Set via
    /// [`start_recording_with_chunk_sink`].
    #[allow(dead_code)]
    chunk_sink: Option<ChunkSink>,
}

impl ActiveRecording {
    pub fn selected_microphone(&self) -> Option<&SelectedMicrophone> {
        self.request.selected_microphone()
    }

    pub fn stop(self) -> Result<RecordingArtifact, RecordingError> {
        #[cfg(target_os = "linux")]
        {
            let mut active_recording = self;
            active_recording
                .control_sender
                .send(RecordingCommand::Stop)
                .map_err(|_| RecordingError::new("failed to stop Pepper X recording worker"))?;
            let worker = active_recording
                .worker
                .take()
                .expect("recording worker should be present");

            return worker.join().map_err(|_| {
                RecordingError::new("Pepper X recording worker panicked while stopping")
            })?;
        }

        #[cfg(not(target_os = "linux"))]
        {
            Err(RecordingError::unsupported_platform())
        }
    }
}

pub fn start_recording(request: RecordingRequest) -> Result<ActiveRecording, RecordingError> {
    start_recording_with_chunk_sink(request, None)
}

/// Start a recording and, if `chunk_sink` is `Some`, send 560ms audio chunks
/// through it as they arrive from PipeWire.  This allows a streaming
/// transcriber to process audio in parallel with recording.
pub fn start_recording_with_chunk_sink(
    request: RecordingRequest,
    chunk_sink: Option<ChunkSink>,
) -> Result<ActiveRecording, RecordingError> {
    #[cfg(target_os = "linux")]
    {
        return start_linux_recording(request, chunk_sink);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (request, chunk_sink);
        Err(RecordingError::unsupported_platform())
    }
}

pub fn probe_signal_level(
    selected_microphone: Option<SelectedMicrophone>,
) -> Result<SignalLevelSample, SignalLevelError> {
    #[cfg(target_os = "linux")]
    {
        return probe_linux_signal_level(selected_microphone);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = selected_microphone;
        Err(SignalLevelError::unsupported_platform())
    }
}

pub type InputLevelSample = SignalLevelSample;
pub type InputLevelError = SignalLevelError;
pub type InputLevelErrorKind = SignalLevelErrorKind;

pub fn sample_input_level(
    selected_microphone: Option<SelectedMicrophone>,
) -> Result<InputLevelSample, InputLevelError> {
    probe_signal_level(selected_microphone)
}

#[cfg(target_os = "linux")]
fn start_linux_recording(
    request: RecordingRequest,
    chunk_sink: Option<ChunkSink>,
) -> Result<ActiveRecording, RecordingError> {
    let (control_sender, control_receiver) = pw::channel::channel();
    let (setup_sender, setup_receiver) = mpsc::channel();
    let worker_request = request.clone();
    let worker_chunk_sink = chunk_sink.clone();
    let worker = thread::spawn(move || {
        capture_recording(worker_request, control_receiver, setup_sender, worker_chunk_sink)
    });

    match setup_receiver
        .recv()
        .map_err(|_| RecordingError::new("Pepper X recording setup channel closed unexpectedly"))?
    {
        Ok(()) => Ok(ActiveRecording {
            request,
            control_sender,
            worker: Some(worker),
            chunk_sink,
        }),
        Err(error) => {
            let _ = worker.join();
            Err(error)
        }
    }
}

#[cfg(target_os = "linux")]
fn probe_linux_signal_level(
    selected_microphone: Option<SelectedMicrophone>,
) -> Result<SignalLevelSample, SignalLevelError> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|error| {
        SignalLevelError::new(
            SignalLevelErrorKind::CaptureFailed,
            format!("failed to create PipeWire main loop: {error}"),
        )
    })?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|error| {
        SignalLevelError::new(
            SignalLevelErrorKind::CaptureFailed,
            format!("failed to create PipeWire context: {error}"),
        )
    })?;
    let core = context.connect_rc(None).map_err(|error| {
        SignalLevelError::new(
            SignalLevelErrorKind::CaptureFailed,
            format!("failed to connect to PipeWire: {error}"),
        )
    })?;

    let target_node_id = resolve_selected_node_id(&core, &mainloop, selected_microphone.as_ref())
        .map_err(|error| {
        SignalLevelError::new(SignalLevelErrorKind::CaptureFailed, error.to_string())
    })?;
    let captured_audio = Rc::new(RefCell::new(CapturedAudio::default()));
    let recording_error = Rc::new(RefCell::new(None::<RecordingError>));
    let (stream, _listener) = configure_capture_stream(
        &core,
        &mainloop,
        &captured_audio,
        &recording_error,
        target_node_id,
        None,
    )
    .map_err(|error| {
        SignalLevelError::new(SignalLevelErrorKind::CaptureFailed, error.to_string())
    })?;

    let started_at = Instant::now();
    let mut best_sample: Option<SignalLevelSample> = None;
    while started_at.elapsed() < SIGNAL_PROBE_TIMEOUT {
        if let Some(error) = recording_error.borrow_mut().take() {
            let _ = stream.disconnect();
            return Err(SignalLevelError::new(
                SignalLevelErrorKind::CaptureFailed,
                error.to_string(),
            ));
        }

        let sample = {
            let captured_audio = captured_audio.borrow();
            (!captured_audio.interleaved_samples.is_empty()).then(|| {
                SignalLevelSample::from_normalized_samples(&captured_audio.interleaved_samples)
            })
        };
        if let Some(sample) = sample {
            if sample.signal_present() {
                let _ = stream.disconnect();
                return Ok(sample);
            }
            best_sample = Some(match best_sample {
                Some(prev) if prev.normalized_level() >= sample.normalized_level() => prev,
                _ => sample,
            });
        }

        mainloop.loop_().iterate(SIGNAL_PROBE_POLL_INTERVAL);
    }

    let _ = stream.disconnect();
    Err(SignalLevelError::new(
        SignalLevelErrorKind::NoSignal,
        "Pepper X did not detect microphone signal",
    ))
}

#[cfg(target_os = "linux")]
fn capture_recording(
    request: RecordingRequest,
    control_receiver: pw::channel::Receiver<RecordingCommand>,
    setup_sender: mpsc::Sender<Result<(), RecordingError>>,
    chunk_sink: Option<ChunkSink>,
) -> Result<RecordingArtifact, RecordingError> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|error| {
        RecordingError::new(format!("failed to create PipeWire main loop: {error}"))
    })?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|error| {
        RecordingError::new(format!("failed to create PipeWire context: {error}"))
    })?;
    let core = context
        .connect_rc(None)
        .map_err(|error| RecordingError::new(format!("failed to connect to PipeWire: {error}")))?;

    let target_node_id = resolve_selected_node_id(&core, &mainloop, request.selected_microphone())?;
    let captured_audio = Rc::new(RefCell::new(CapturedAudio::default()));
    let recording_error = Rc::new(RefCell::new(None::<RecordingError>));
    let stop_requested = Rc::new(Cell::new(false));
    let (stream, _listener) = configure_capture_stream(
        &core,
        &mainloop,
        &captured_audio,
        &recording_error,
        target_node_id,
        chunk_sink,
    )?;

    let _control = control_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let stop_requested = stop_requested.clone();
        move |command| match command {
            RecordingCommand::Stop => {
                stop_requested.set(true);
                mainloop.quit();
            }
        }
    });

    setup_sender
        .send(Ok(()))
        .map_err(|_| RecordingError::new("failed to report Pepper X recording setup success"))?;
    mainloop.run();

    // After stop is requested, continue capturing for a short duration so that
    // trailing speech that was still in the PipeWire buffer makes it into the
    // recording.  This avoids clipping the last syllable when the user releases
    // the modifier key slightly before finishing a word.
    if stop_requested.get() && recording_error.borrow().is_none() {
        let flush_start = Instant::now();
        while flush_start.elapsed() < POST_STOP_FLUSH_DURATION {
            mainloop.loop_().iterate(POST_STOP_FLUSH_POLL_INTERVAL);
            if recording_error.borrow().is_some() {
                break;
            }
        }
    }

    let _ = stream.disconnect();

    if let Some(error) = recording_error.borrow_mut().take() {
        return Err(error);
    }

    if !stop_requested.get() {
        return Err(RecordingError::new(
            "Pepper X recording stopped before a stop request was received",
        ));
    }

    let captured_audio = captured_audio.borrow().clone();
    materialize_recording_artifact(&request, captured_audio)
}

#[cfg(target_os = "linux")]
fn resolve_selected_node_id(
    core: &pw::core::CoreRc,
    mainloop: &pw::main_loop::MainLoopRc,
    selected_microphone: Option<&SelectedMicrophone>,
) -> Result<Option<u32>, RecordingError> {
    let Some(selected_microphone) = selected_microphone else {
        return Ok(None);
    };

    let registry = core.get_registry().map_err(|error| {
        RecordingError::new(format!("failed to get PipeWire registry: {error}"))
    })?;
    let selected_stable_id = selected_microphone.stable_id().to_string();
    let matching_node_id = Rc::new(RefCell::new(None::<u32>));
    let enumeration_error = Rc::new(RefCell::new(None::<RecordingError>));
    let done = Rc::new(Cell::new(false));

    let _registry_listener = registry
        .add_listener_local()
        .global({
            let matching_node_id = matching_node_id.clone();
            let enumeration_error = enumeration_error.clone();
            let mainloop = mainloop.clone();
            move |global| {
                if global.type_.to_str() != "PipeWire:Interface:Node" {
                    return;
                }

                let Some(properties) = global.props else {
                    return;
                };

                match stable_pipewire_microphone_id(properties) {
                    Ok(Some(stable_id)) if stable_id == selected_stable_id => {
                        *matching_node_id.borrow_mut() = Some(global.id);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        if enumeration_error.borrow().is_none() {
                            *enumeration_error.borrow_mut() =
                                Some(RecordingError::new(error.to_string()));
                        }
                        mainloop.quit();
                    }
                }
            }
        })
        .register();

    let pending = core.sync(0).map_err(|error| {
        RecordingError::new(format!("failed to sync PipeWire registry: {error}"))
    })?;
    let _core_listener = core
        .add_listener_local()
        .done({
            let done = done.clone();
            let mainloop = mainloop.clone();
            move |id, sequence| {
                if id == pw::core::PW_ID_CORE && sequence == pending {
                    done.set(true);
                    mainloop.quit();
                }
            }
        })
        .register();

    while !done.get() && enumeration_error.borrow().is_none() {
        mainloop.run();
    }

    if let Some(error) = enumeration_error.borrow_mut().take() {
        return Err(error);
    }

    let matching_node_id = *matching_node_id.borrow();

    matching_node_id
        .ok_or_else(|| {
            RecordingError::new(format!(
                "failed to find selected microphone in PipeWire: {}",
                selected_microphone.display_name()
            ))
        })
        .map(Some)
}

#[cfg(target_os = "linux")]
fn configure_capture_stream(
    core: &pw::core::CoreRc,
    mainloop: &pw::main_loop::MainLoopRc,
    captured_audio: &Rc<RefCell<CapturedAudio>>,
    recording_error: &Rc<RefCell<Option<RecordingError>>>,
    target_node_id: Option<u32>,
    chunk_sink: Option<ChunkSink>,
) -> Result<(pw::stream::StreamRc, Box<dyn std::any::Any>), RecordingError> {
    let stream = pw::stream::StreamRc::new(
        core.clone(),
        STREAM_NAME,
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Production",
            *pw::keys::APP_ID => APP_ID,
        },
    )
    .map_err(|error| RecordingError::new(format!("failed to create PipeWire stream: {error}")))?;

    let listener = stream
        .add_local_listener_with_user_data(spa::param::audio::AudioInfoRaw::new())
        .state_changed({
            let recording_error = recording_error.clone();
            let mainloop = mainloop.clone();
            move |_, _, _, next_state| {
                if let pw::stream::StreamState::Error(error) = next_state {
                    if recording_error.borrow().is_none() {
                        *recording_error.borrow_mut() = Some(RecordingError::new(format!(
                            "PipeWire stream error: {error}"
                        )));
                    }
                    mainloop.quit();
                }
            }
        })
        .param_changed({
            let captured_audio = captured_audio.clone();
            let recording_error = recording_error.clone();
            let mainloop = mainloop.clone();
            move |_, format, id, param| {
                let Some(param) = param else {
                    return;
                };
                if id != pw::spa::param::ParamType::Format.as_raw() {
                    return;
                }

                let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                    return;
                };
                if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                    return;
                }

                if let Err(error) = format.parse(param) {
                    if recording_error.borrow().is_none() {
                        *recording_error.borrow_mut() = Some(RecordingError::new(format!(
                            "failed to parse PipeWire audio format: {error}"
                        )));
                    }
                    mainloop.quit();
                    return;
                }

                if format.format() != spa::param::audio::AudioFormat::S16LE
                    || format.channels() != 1
                {
                    if recording_error.borrow().is_none() {
                        *recording_error.borrow_mut() = Some(RecordingError::new(format!(
                            "PipeWire negotiated unsupported capture format: {:?} {}ch",
                            format.format(),
                            format.channels()
                        )));
                    }
                    mainloop.quit();
                    return;
                }

                let mut captured_audio = captured_audio.borrow_mut();
                captured_audio.sample_rate_hz = format.rate();
                captured_audio.channel_count = format.channels() as u16;
            }
        })
        .process({
            let captured_audio = captured_audio.clone();
            let recording_error = recording_error.clone();
            let mainloop = mainloop.clone();
            // State for streaming chunk dispatch — only allocated when a sink
            // is provided.
            let chunk_sink = Rc::new(RefCell::new(chunk_sink));
            let streaming_pending: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(
                Vec::with_capacity(STREAMING_CHUNK_SAMPLES),
            ));
            move |stream, format| match stream.dequeue_buffer() {
                None => {}
                Some(mut buffer) => {
                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }

                    let data = &mut datas[0];
                    let chunk = data.chunk();
                    let offset = chunk.offset() as usize;
                    let end = offset + chunk.size() as usize;
                    let Some(bytes) = data.data() else {
                        return;
                    };
                    if end > bytes.len() {
                        if recording_error.borrow().is_none() {
                            *recording_error.borrow_mut() = Some(RecordingError::new(
                                "PipeWire provided an invalid capture chunk",
                            ));
                        }
                        mainloop.quit();
                        return;
                    }

                    let payload = &bytes[offset..end];
                    if payload.len() % std::mem::size_of::<i16>() != 0 {
                        if recording_error.borrow().is_none() {
                            *recording_error.borrow_mut() =
                                Some(RecordingError::new("PipeWire provided partial PCM samples"));
                        }
                        mainloop.quit();
                        return;
                    }

                    let mut captured_audio = captured_audio.borrow_mut();
                    if captured_audio.sample_rate_hz == 0 {
                        captured_audio.sample_rate_hz = format.rate();
                    }
                    if captured_audio.channel_count == 0 {
                        captured_audio.channel_count = format.channels() as u16;
                    }

                    for sample_bytes in payload.chunks_exact(std::mem::size_of::<i16>()) {
                        let sample = i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]) as f32
                            / i16::MAX as f32;
                        captured_audio.interleaved_samples.push(sample);

                        // Also accumulate for the streaming chunk sink.
                        if chunk_sink.borrow().is_some() {
                            let mut pending = streaming_pending.borrow_mut();
                            pending.push(sample);
                            if pending.len() >= STREAMING_CHUNK_SAMPLES {
                                let chunk_data: Vec<f32> =
                                    pending.drain(..STREAMING_CHUNK_SAMPLES).collect();
                                let mut sink_ref = chunk_sink.borrow_mut();
                                if let Some(sink) = sink_ref.as_ref() {
                                    // Non-blocking try_send is not available on
                                    // std mpsc — .send() may block momentarily
                                    // if the receiver is slow, but the
                                    // transcriber thread processes chunks fast
                                    // enough that this should not stall.  If the
                                    // receiver has been dropped (e.g. the
                                    // transcriber thread panicked), silently
                                    // discard the sink so we stop trying.
                                    if sink.send(chunk_data).is_err() {
                                        *sink_ref = None;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
        .register()
        .map_err(|error| {
            RecordingError::new(format!(
                "failed to register PipeWire stream listener: {error}"
            ))
        })?;

    let values = audio_capture_param_values()?;
    let pod = Pod::from_bytes(&values).ok_or_else(|| {
        RecordingError::new("failed to build PipeWire capture pod from serialized bytes")
    })?;
    let mut params = [pod];
    stream
        .connect(
            spa::utils::Direction::Input,
            target_node_id,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| {
            RecordingError::new(format!("failed to connect PipeWire stream: {error}"))
        })?;

    Ok((stream, Box::new(listener)))
}

#[cfg(target_os = "linux")]
pub(crate) fn audio_capture_param_values() -> Result<Vec<u8>, RecordingError> {
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::S16LE);
    audio_info.set_channels(1);
    audio_info.set_rate(16_000);
    let object = pw::spa::pod::Object {
        type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: pw::spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };
    let values = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(object),
    )
    .map_err(|error| {
        RecordingError::new(format!(
            "failed to serialize PipeWire capture format: {error}"
        ))
    })?
    .0
    .into_inner();

    Ok(values)
}

#[cfg(any(test, target_os = "linux"))]
#[derive(Debug, Clone, Default)]
struct CapturedAudio {
    sample_rate_hz: u32,
    channel_count: u16,
    interleaved_samples: Vec<f32>,
}

#[cfg(any(test, target_os = "linux"))]
fn materialize_recording_artifact(
    request: &RecordingRequest,
    captured_audio: CapturedAudio,
) -> Result<RecordingArtifact, RecordingError> {
    if captured_audio.sample_rate_hz == 0 {
        return Err(RecordingError::new(
            "recorded audio did not negotiate a sample rate",
        ));
    }
    if captured_audio.channel_count == 0 {
        return Err(RecordingError::new(
            "recorded audio did not negotiate any channels",
        ));
    }

    if let Some(parent) = request.output_wav_path().parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            RecordingError::new(format!(
                "failed to create recording output directory {}: {error}",
                parent.display()
            ))
        })?;
    }

    let mono_samples = mix_to_mono(
        &captured_audio.interleaved_samples,
        captured_audio.channel_count,
    );
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: captured_audio.sample_rate_hz,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(request.output_wav_path(), spec).map_err(|error| {
            RecordingError::new(format!(
                "failed to create recording wav {}: {error}",
                request.output_wav_path().display()
            ))
        })?;

    for sample in &mono_samples {
        writer
            .write_sample(float_to_i16(*sample))
            .map_err(|error| {
                RecordingError::new(format!("failed to write recording wav: {error}"))
            })?;
    }
    writer.finalize().map_err(|error| {
        RecordingError::new(format!("failed to finalize recording wav: {error}"))
    })?;

    let elapsed = if mono_samples.is_empty() {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(mono_samples.len() as f64 / captured_audio.sample_rate_hz as f64)
    };

    Ok(RecordingArtifact::new(
        request.output_wav_path().to_path_buf(),
        request.selected_microphone().cloned(),
        elapsed,
    ))
}

#[cfg(any(test, target_os = "linux"))]
fn mix_to_mono(interleaved_samples: &[f32], channel_count: u16) -> Vec<f32> {
    let channel_count = channel_count as usize;
    if channel_count <= 1 {
        return interleaved_samples.to_vec();
    }

    interleaved_samples
        .chunks_exact(channel_count)
        .map(|frame| frame.iter().copied().sum::<f32>() / channel_count as f32)
        .collect()
}

#[cfg(any(test, target_os = "linux"))]
fn float_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32).round() as i16
}

#[cfg(test)]
fn selected_target_object(request: &RecordingRequest) -> Option<String> {
    request.selected_microphone().map(|microphone| {
        microphone
            .stable_id()
            .strip_prefix("pipewire:node.name=")
            .or_else(|| microphone.stable_id().strip_prefix("pipewire:object.path="))
            .unwrap_or(microphone.stable_id())
            .to_string()
    })
}

#[cfg(test)]
mod recording_runtime {
    use super::*;

    #[test]
    fn recording_target_object_uses_selected_microphone_node_name() {
        let request = RecordingRequest::new(
            unique_wav_path("node-target"),
            Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )),
        );

        assert_eq!(
            selected_target_object(&request).as_deref(),
            Some("alsa_input.usb-blue-yeti-00.analog-stereo")
        );
    }

    #[test]
    fn recording_artifact_writes_mono_pcm_wav() {
        let wav_path = unique_wav_path("artifact");
        let request = RecordingRequest::new(
            &wav_path,
            Some(SelectedMicrophone::new(
                "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
                "Blue Yeti",
            )),
        );

        let artifact = materialize_recording_artifact(
            &request,
            CapturedAudio {
                sample_rate_hz: 16_000,
                channel_count: 2,
                interleaved_samples: vec![0.5, 0.25, -0.5, -0.25],
            },
        )
        .expect("artifact should be written");

        let reader = hound::WavReader::open(&wav_path).expect("written wav should open");
        let spec = reader.spec();
        let samples = reader
            .into_samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .expect("samples should decode");

        assert_eq!(artifact.wav_path(), wav_path.as_path());
        assert_eq!(
            artifact.selected_microphone(),
            request.selected_microphone()
        );
        assert_eq!(artifact.elapsed(), Duration::from_micros(125));
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(samples.len(), 2);
        assert!(samples[0] > 0);
        assert!(samples[1] < 0);

        let _ = std::fs::remove_file(wav_path);
    }

    #[test]
    fn device_signal_level_reports_silence_without_marking_signal_present() {
        let sample = SignalLevelSample::from_pcm_samples(&[0, 0, 0, 0]);

        assert_eq!(sample.normalized_level(), 0.0);
        assert!(!sample.signal_present());
    }

    #[test]
    fn device_signal_level_detects_meaningful_input_for_meter_updates() {
        let sample = SignalLevelSample::from_pcm_samples(&[0, 16_384, -16_384, 0]);

        assert!(sample.normalized_level() > 0.45);
        assert!(sample.signal_present());
    }

    #[test]
    fn device_signal_level_supports_interleaved_float_samples_on_all_platforms() {
        let sample = SignalLevelSample::from_interleaved_samples(&[0.0, 0.5, -0.25, 0.0]);

        assert_eq!(sample.normalized_level(), 0.5);
        assert!(sample.signal_present());
    }

    fn unique_wav_path(suffix: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pepper-x-recording-{suffix}-{unique}.wav"))
    }
}
