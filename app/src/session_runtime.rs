use std::path::{Path, PathBuf};

use pepperx_audio::recording::{
    start_recording, ActiveRecording, RecordingArtifact, RecordingError, RecordingRequest,
};
use pepperx_audio::SelectedMicrophone;
use pepperx_ipc::{LiveStatus, SharedLiveStatus};
use pepperx_platform_gnome::service::{RecordingRuntime, RecordingRuntimeError};
use pepperx_session::{RecordingSession, SessionError, SessionState, TriggerSource};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::transcript_log::state_root;
use crate::transcript_log::TranscriptEntry;
use crate::transcription::{
    transcribe_recorded_wav_to_log_with_status, LivePipelineRequest, TranscriptionRunError,
};

pub trait Recorder {
    fn start_recording(&mut self, request: RecordingRequest) -> Result<(), RecordingError>;
    fn stop_recording(&mut self) -> Result<RecordingArtifact, RecordingError>;
}

pub struct PipeWireRecorder {
    active_recording: Option<ActiveRecording>,
}

impl PipeWireRecorder {
    pub fn new() -> Self {
        Self {
            active_recording: None,
        }
    }
}

impl Default for PipeWireRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder for PipeWireRecorder {
    fn start_recording(&mut self, request: RecordingRequest) -> Result<(), RecordingError> {
        debug_assert!(
            self.active_recording.is_none(),
            "PipeWireRecorder should only start one active recording at a time"
        );

        self.active_recording = Some(start_recording(request)?);
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<RecordingArtifact, RecordingError> {
        let active_recording = self
            .active_recording
            .take()
            .expect("active recording should exist before stop");

        active_recording.stop()
    }
}

type LivePipelineTranscriber =
    Box<dyn FnMut(LivePipelineRequest) -> Result<TranscriptEntry, TranscriptionRunError> + Send>;
type LiveSessionRuntime =
    SessionRuntime<PipeWireRecorder, fn() -> PathBuf, LivePipelineTranscriber>;

pub struct LiveRuntimeHandle {
    runtime: Mutex<LiveSessionRuntime>,
    selected_microphone: Option<SelectedMicrophone>,
    live_status: SharedLiveStatus,
}

impl LiveRuntimeHandle {
    pub fn new(
        selected_microphone: Option<SelectedMicrophone>,
        live_status: SharedLiveStatus,
    ) -> Self {
        let transcriber_status = live_status.clone();
        Self {
            runtime: Mutex::new(SessionRuntime::new(
                PipeWireRecorder::new(),
                next_live_recording_wav_path,
                Box::new(move |request| {
                    transcribe_recorded_wav_to_log_with_status(request, transcriber_status.clone())
                }),
            )),
            selected_microphone,
            live_status,
        }
    }

    pub fn start_recording(
        &self,
        trigger_source: TriggerSource,
    ) -> Result<(), SessionRuntimeError> {
        let result = self
            .runtime
            .lock()
            .expect("live runtime lock poisoned")
            .start_recording(trigger_source, self.selected_microphone.clone())
            .map(|_| ());

        match &result {
            Ok(()) => self.live_status.replace(LiveStatus::recording()),
            Err(SessionRuntimeError::Session(SessionError::AlreadyRecording)) => {}
            Err(error) => self
                .live_status
                .replace(LiveStatus::error(error.to_string())),
        }

        result
    }

    pub fn record_and_transcribe<F>(
        &self,
        trigger_source: TriggerSource,
        wait_for_stop: F,
    ) -> Result<TranscriptEntry, SessionRuntimeError>
    where
        F: FnOnce() -> std::io::Result<()>,
    {
        self.runtime
            .lock()
            .expect("live runtime lock poisoned")
            .record_and_transcribe(
                trigger_source,
                self.selected_microphone.clone(),
                wait_for_stop,
            )
    }

    fn stop_recording_in_background(&self) -> Result<(), SessionRuntimeError> {
        let request = match self
            .runtime
            .lock()
            .expect("live runtime lock poisoned")
            .finish_recording()
        {
            Ok(request) => request,
            Err(error @ SessionRuntimeError::Session(SessionError::NotRecording)) => {
                return Err(error)
            }
            Err(error) => {
                self.live_status
                    .replace(LiveStatus::error(error.to_string()));
                return Err(error);
            }
        };
        self.live_status.replace(LiveStatus::transcribing());
        let live_status = self.live_status.clone();
        std::thread::Builder::new()
            .name("pepperx-live-pipeline".into())
            .spawn(move || {
                if let Err(error) =
                    transcribe_recorded_wav_to_log_with_status(request, live_status.clone())
                {
                    live_status.replace(LiveStatus::error(error.to_string()));
                }
            })
            .map_err(|error| {
                self.live_status
                    .replace(LiveStatus::error(error.to_string()));
                SessionRuntimeError::BackgroundSpawn(error)
            })?;
        Ok(())
    }
}

impl RecordingRuntime for LiveRuntimeHandle {
    fn start_recording(&self, trigger_source: TriggerSource) -> Result<(), RecordingRuntimeError> {
        LiveRuntimeHandle::start_recording(self, trigger_source).map_err(runtime_error)
    }

    fn stop_recording(&self) -> Result<(), RecordingRuntimeError> {
        self.stop_recording_in_background().map_err(runtime_error)
    }
}

#[derive(Debug)]
pub enum SessionRuntimeError {
    Session(SessionError),
    Recording(RecordingError),
    Transcription(TranscriptionRunError),
    WaitForStop(std::io::Error),
    BackgroundSpawn(std::io::Error),
}

impl std::fmt::Display for SessionRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "Pepper X session error: {error:?}"),
            Self::Recording(error) => write!(f, "Pepper X recording error: {error}"),
            Self::Transcription(error) => write!(f, "Pepper X transcription error: {error}"),
            Self::WaitForStop(error) => {
                write!(f, "Pepper X live stop signal failed: {error}")
            }
            Self::BackgroundSpawn(error) => {
                write!(
                    f,
                    "Pepper X failed to start the live pipeline worker: {error}"
                )
            }
        }
    }
}

impl std::error::Error for SessionRuntimeError {}

fn runtime_error(error: SessionRuntimeError) -> RecordingRuntimeError {
    match error {
        SessionRuntimeError::Session(SessionError::AlreadyRecording) => {
            RecordingRuntimeError::DuplicateStart
        }
        SessionRuntimeError::Session(SessionError::NotRecording) => {
            RecordingRuntimeError::DuplicateStop
        }
        other => RecordingRuntimeError::Failed(other.to_string()),
    }
}

fn next_live_recording_wav_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    state_root().join("recordings").join(format!(
        "live-recording-{}-{unique}.wav",
        std::process::id()
    ))
}

impl From<SessionError> for SessionRuntimeError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

impl From<RecordingError> for SessionRuntimeError {
    fn from(error: RecordingError) -> Self {
        Self::Recording(error)
    }
}

impl From<TranscriptionRunError> for SessionRuntimeError {
    fn from(error: TranscriptionRunError) -> Self {
        Self::Transcription(error)
    }
}

pub struct SessionRuntime<R, P, T>
where
    R: Recorder,
    P: FnMut() -> PathBuf,
    T: FnMut(LivePipelineRequest) -> Result<TranscriptEntry, TranscriptionRunError>,
{
    session: RecordingSession,
    recorder: R,
    next_output_wav_path: P,
    transcribe_recorded_wav: T,
}

impl<R, P, T> SessionRuntime<R, P, T>
where
    R: Recorder,
    P: FnMut() -> PathBuf,
    T: FnMut(LivePipelineRequest) -> Result<TranscriptEntry, TranscriptionRunError>,
{
    pub fn new(recorder: R, next_output_wav_path: P, transcribe_recorded_wav: T) -> Self {
        Self {
            session: RecordingSession::new(),
            recorder,
            next_output_wav_path,
            transcribe_recorded_wav,
        }
    }

    #[cfg(test)]
    pub fn session_state(&self) -> SessionState {
        self.session.state()
    }

    pub fn start_recording(
        &mut self,
        trigger_source: TriggerSource,
        selected_microphone: Option<SelectedMicrophone>,
    ) -> Result<SessionState, SessionRuntimeError> {
        self.session.start_recording(trigger_source)?;

        let request = RecordingRequest::new((self.next_output_wav_path)(), selected_microphone);
        if let Err(error) = self.recorder.start_recording(request) {
            self.session
                .stop_recording()
                .expect("session should roll back");
            return Err(error.into());
        }

        Ok(self.session.state())
    }

    pub fn stop_recording(&mut self) -> Result<TranscriptEntry, SessionRuntimeError> {
        let request = self.finish_recording()?;
        Ok((self.transcribe_recorded_wav)(request)?)
    }

    pub fn finish_recording(&mut self) -> Result<LivePipelineRequest, SessionRuntimeError> {
        let trigger_source = self
            .session
            .active_trigger_source()
            .ok_or(SessionError::NotRecording)?;
        self.session.stop_recording()?;
        let recorded_wav = self.recorder.stop_recording()?;
        Ok(LivePipelineRequest::new(trigger_source, recorded_wav))
    }

    pub fn record_and_transcribe<W>(
        &mut self,
        trigger_source: TriggerSource,
        selected_microphone: Option<SelectedMicrophone>,
        wait_for_stop: W,
    ) -> Result<TranscriptEntry, SessionRuntimeError>
    where
        W: FnOnce() -> std::io::Result<()>,
    {
        self.start_recording(trigger_source, selected_microphone)?;

        if let Err(error) = wait_for_stop() {
            self.session
                .stop_recording()
                .expect("session should reset after live stop wait fails");
            let _ = self.recorder.stop_recording();
            return Err(SessionRuntimeError::WaitForStop(error));
        }

        self.stop_recording()
    }
}

#[cfg(test)]
mod session_runtime {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    struct FakeRecorder {
        observed_requests: Rc<RefCell<Vec<RecordingRequest>>>,
        stop_calls: Rc<RefCell<usize>>,
        stop_result: Option<Result<RecordingArtifact, RecordingError>>,
    }

    impl Recorder for FakeRecorder {
        fn start_recording(&mut self, request: RecordingRequest) -> Result<(), RecordingError> {
            self.observed_requests.borrow_mut().push(request);
            Ok(())
        }

        fn stop_recording(&mut self) -> Result<RecordingArtifact, RecordingError> {
            *self.stop_calls.borrow_mut() += 1;
            self.stop_result
                .take()
                .expect("stop result should be configured")
        }
    }

    #[test]
    fn session_runtime_starts_recording_with_selected_microphone() {
        let selected_microphone = Some(SelectedMicrophone::new(
            "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
            "Blue Yeti",
        ));
        let observed_requests = Rc::new(RefCell::new(Vec::new()));
        let recorder = FakeRecorder {
            observed_requests: observed_requests.clone(),
            stop_calls: Rc::new(RefCell::new(0)),
            stop_result: None,
        };

        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/pepper-x-live.wav"),
            |_| unreachable!(),
        );

        let session_state = runtime
            .start_recording(TriggerSource::ModifierOnly, selected_microphone.clone())
            .expect("runtime should start recording");

        assert_eq!(session_state, SessionState::Recording);
        assert_eq!(runtime.session_state(), SessionState::Recording);
        assert_eq!(
            observed_requests.borrow().as_slice(),
            &[RecordingRequest::new(
                "/tmp/pepper-x-live.wav",
                selected_microphone
            )]
        );
    }

    #[test]
    fn session_runtime_rejects_duplicate_start_and_stop_requests() {
        let recorder = FakeRecorder {
            observed_requests: Rc::new(RefCell::new(Vec::new())),
            stop_calls: Rc::new(RefCell::new(0)),
            stop_result: Some(Ok(RecordingArtifact::new(
                "/tmp/pepper-x-live.wav",
                None,
                Duration::from_millis(250),
            ))),
        };
        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/live.wav"),
            |_| {
                Ok(TranscriptEntry::new(
                    "/tmp/live.wav",
                    "hello",
                    "sherpa-onnx",
                    "model",
                    Duration::from_millis(40),
                ))
            },
        );

        runtime
            .start_recording(TriggerSource::ModifierOnly, None)
            .expect("first start should work");

        let duplicate_start = runtime
            .start_recording(TriggerSource::ModifierOnly, None)
            .unwrap_err();
        assert!(matches!(
            duplicate_start,
            SessionRuntimeError::Session(SessionError::AlreadyRecording)
        ));

        runtime.stop_recording().expect("first stop should work");

        let duplicate_stop = runtime.stop_recording().unwrap_err();
        assert!(matches!(
            duplicate_stop,
            SessionRuntimeError::Session(SessionError::NotRecording)
        ));
    }

    #[test]
    fn session_runtime_stops_recording_and_hands_wav_to_transcriber() {
        let observed_wav_paths = Rc::new(RefCell::new(Vec::new()));
        let observed_wav_paths_clone = observed_wav_paths.clone();
        let recorder = FakeRecorder {
            observed_requests: Rc::new(RefCell::new(Vec::new())),
            stop_calls: Rc::new(RefCell::new(0)),
            stop_result: Some(Ok(RecordingArtifact::new(
                "/tmp/pepper-x-live.wav",
                None,
                Duration::from_millis(250),
            ))),
        };
        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/pepper-x-live.wav"),
            move |request| {
                let wav_path = request.recording_artifact().wav_path();
                observed_wav_paths_clone
                    .borrow_mut()
                    .push(wav_path.to_path_buf());
                Ok(TranscriptEntry::new(
                    wav_path,
                    "hello from pepper x",
                    "sherpa-onnx",
                    "nemo-parakeet-tdt-0.6b-v2-int8",
                    Duration::from_millis(37),
                ))
            },
        );

        runtime
            .start_recording(TriggerSource::ModifierOnly, None)
            .expect("start should succeed");
        let entry = runtime.stop_recording().expect("stop should transcribe");

        assert_eq!(
            observed_wav_paths.borrow().as_slice(),
            &[PathBuf::from("/tmp/pepper-x-live.wav")]
        );
        assert_eq!(
            entry.source_wav_path,
            PathBuf::from("/tmp/pepper-x-live.wav")
        );
        assert_eq!(runtime.session_state(), SessionState::Idle);
    }

    #[test]
    fn session_runtime_hands_live_pipeline_request_to_transcriber() {
        let selected_microphone = Some(SelectedMicrophone::new(
            "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
            "Blue Yeti",
        ));
        let observed_requests = Rc::new(RefCell::new(Vec::new()));
        let observed_requests_clone = observed_requests.clone();
        let recorder = FakeRecorder {
            observed_requests: Rc::new(RefCell::new(Vec::new())),
            stop_calls: Rc::new(RefCell::new(0)),
            stop_result: Some(Ok(RecordingArtifact::new(
                "/tmp/pepper-x-live.wav",
                selected_microphone.clone(),
                Duration::from_millis(250),
            ))),
        };
        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/pepper-x-live.wav"),
            move |request| {
                observed_requests_clone.borrow_mut().push(request.clone());
                Ok(TranscriptEntry::new(
                    request.recording_artifact().wav_path(),
                    "hello from pepper x",
                    "sherpa-onnx",
                    "nemo-parakeet-tdt-0.6b-v2-int8",
                    Duration::from_millis(37),
                ))
            },
        );

        runtime
            .start_recording(TriggerSource::ModifierOnly, selected_microphone.clone())
            .expect("start should succeed");
        runtime.stop_recording().expect("stop should transcribe");

        let observed_requests = observed_requests.borrow();
        let request = observed_requests
            .first()
            .expect("transcriber should observe one live request");
        assert_eq!(request.trigger_source(), TriggerSource::ModifierOnly);
        assert_eq!(
            request.recording_artifact().wav_path(),
            Path::new("/tmp/pepper-x-live.wav")
        );
        assert_eq!(
            request.recording_artifact().selected_microphone(),
            selected_microphone.as_ref()
        );
        assert_eq!(
            request.recording_artifact().elapsed(),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn session_runtime_records_until_stop_signal_and_transcribes() {
        let observed_recordings = Rc::new(RefCell::new(Vec::new()));
        let observed_recordings_clone = observed_recordings.clone();
        let wait_calls = Rc::new(RefCell::new(0));
        let wait_calls_clone = wait_calls.clone();
        let recorder = FakeRecorder {
            observed_requests: Rc::new(RefCell::new(Vec::new())),
            stop_calls: Rc::new(RefCell::new(0)),
            stop_result: Some(Ok(RecordingArtifact::new(
                "/tmp/pepper-x-live.wav",
                None,
                Duration::from_millis(250),
            ))),
        };
        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/pepper-x-live.wav"),
            move |request| {
                observed_recordings_clone.borrow_mut().push((
                    request.recording_artifact().wav_path().to_path_buf(),
                    request.recording_artifact().selected_microphone().cloned(),
                    request.recording_artifact().elapsed(),
                    request.trigger_source(),
                ));
                Ok(TranscriptEntry::new(
                    request.recording_artifact().wav_path(),
                    "hello from pepper x",
                    "sherpa-onnx",
                    "nemo-parakeet-tdt-0.6b-v2-int8",
                    Duration::from_millis(37),
                ))
            },
        );

        let entry = runtime
            .record_and_transcribe(TriggerSource::ShellAction, None, move || {
                *wait_calls_clone.borrow_mut() += 1;
                Ok(())
            })
            .expect("record-and-transcribe should succeed");

        assert_eq!(*wait_calls.borrow(), 1);
        assert_eq!(
            observed_recordings.borrow().as_slice(),
            &[(
                PathBuf::from("/tmp/pepper-x-live.wav"),
                None,
                Duration::from_millis(250),
                TriggerSource::ShellAction,
            )]
        );
        assert_eq!(
            entry.source_wav_path,
            PathBuf::from("/tmp/pepper-x-live.wav")
        );
        assert_eq!(runtime.session_state(), SessionState::Idle);
    }

    #[test]
    fn session_runtime_stops_recording_when_wait_for_stop_fails() {
        let stop_calls = Rc::new(RefCell::new(0));
        let recorder = FakeRecorder {
            observed_requests: Rc::new(RefCell::new(Vec::new())),
            stop_calls: stop_calls.clone(),
            stop_result: Some(Ok(RecordingArtifact::new(
                "/tmp/pepper-x-live.wav",
                None,
                Duration::from_millis(250),
            ))),
        };
        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/pepper-x-live.wav"),
            |_| unreachable!("wait failure should skip transcription"),
        );

        let error = runtime
            .record_and_transcribe(TriggerSource::ShellAction, None, || {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "stdin closed",
                ))
            })
            .unwrap_err();

        assert!(matches!(error, SessionRuntimeError::WaitForStop(_)));
        assert_eq!(*stop_calls.borrow(), 1);
        assert_eq!(runtime.session_state(), SessionState::Idle);
    }

    #[test]
    fn session_runtime_stop_passes_live_recording_metadata_to_shared_pipeline() {
        let selected_microphone = Some(SelectedMicrophone::new(
            "pipewire:node.name=alsa_input.usb-blue-yeti-00.analog-stereo",
            "Blue Yeti",
        ));
        let observed_recordings = Rc::new(RefCell::new(Vec::new()));
        let observed_recordings_clone = observed_recordings.clone();
        let recorder = FakeRecorder {
            observed_requests: Rc::new(RefCell::new(Vec::new())),
            stop_calls: Rc::new(RefCell::new(0)),
            stop_result: Some(Ok(RecordingArtifact::new(
                "/tmp/pepper-x-live.wav",
                selected_microphone.clone(),
                Duration::from_millis(725),
            ))),
        };
        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/pepper-x-live.wav"),
            move |request| {
                observed_recordings_clone.borrow_mut().push((
                    request.recording_artifact().wav_path().to_path_buf(),
                    request.recording_artifact().selected_microphone().cloned(),
                    request.recording_artifact().elapsed(),
                    request.trigger_source(),
                ));
                Ok(TranscriptEntry::new(
                    request.recording_artifact().wav_path(),
                    "hello from pepper x",
                    "sherpa-onnx",
                    "nemo-parakeet-tdt-0.6b-v2-int8",
                    Duration::from_millis(37),
                ))
            },
        );

        runtime
            .start_recording(TriggerSource::ModifierOnly, selected_microphone.clone())
            .expect("start should succeed");
        runtime.stop_recording().expect("stop should transcribe");

        assert_eq!(
            observed_recordings.borrow().as_slice(),
            &[(
                PathBuf::from("/tmp/pepper-x-live.wav"),
                selected_microphone,
                Duration::from_millis(725),
                TriggerSource::ModifierOnly,
            )]
        );
    }
}
