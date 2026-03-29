use std::path::{Path, PathBuf};

use pepperx_audio::recording::{
    start_recording, ActiveRecording, RecordingArtifact, RecordingError, RecordingRequest,
};
use pepperx_audio::SelectedMicrophone;
use pepperx_session::{RecordingSession, SessionError, SessionState, TriggerSource};

use crate::transcript_log::TranscriptEntry;
use crate::transcription::TranscriptionRunError;

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

#[derive(Debug)]
pub enum SessionRuntimeError {
    Session(SessionError),
    Recording(RecordingError),
    Transcription(TranscriptionRunError),
}

impl std::fmt::Display for SessionRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(error) => write!(f, "Pepper X session error: {error:?}"),
            Self::Recording(error) => write!(f, "Pepper X recording error: {error}"),
            Self::Transcription(error) => write!(f, "Pepper X transcription error: {error}"),
        }
    }
}

impl std::error::Error for SessionRuntimeError {}

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
    T: FnMut(&Path) -> Result<TranscriptEntry, TranscriptionRunError>,
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
    T: FnMut(&Path) -> Result<TranscriptEntry, TranscriptionRunError>,
{
    pub fn new(recorder: R, next_output_wav_path: P, transcribe_recorded_wav: T) -> Self {
        Self {
            session: RecordingSession::new(),
            recorder,
            next_output_wav_path,
            transcribe_recorded_wav,
        }
    }

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
        self.session.stop_recording()?;
        let recorded_wav = self.recorder.stop_recording()?;
        Ok((self.transcribe_recorded_wav)(recorded_wav.wav_path())?)
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
        stop_result: Option<Result<RecordingArtifact, RecordingError>>,
    }

    impl Recorder for FakeRecorder {
        fn start_recording(&mut self, request: RecordingRequest) -> Result<(), RecordingError> {
            self.observed_requests.borrow_mut().push(request);
            Ok(())
        }

        fn stop_recording(&mut self) -> Result<RecordingArtifact, RecordingError> {
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
            stop_result: Some(Ok(RecordingArtifact::new(
                "/tmp/pepper-x-live.wav",
                None,
                Duration::from_millis(250),
            ))),
        };
        let mut runtime = SessionRuntime::new(
            recorder,
            || PathBuf::from("/tmp/pepper-x-live.wav"),
            move |wav_path| {
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
}
