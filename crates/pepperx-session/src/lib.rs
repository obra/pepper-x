#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Recording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    ModifierOnly,
    StandardShortcut,
    ShellAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    AlreadyRecording,
    NotRecording,
}

impl SessionError {
    pub fn is_duplicate_request(self) -> bool {
        matches!(self, Self::AlreadyRecording | Self::NotRecording)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingSession {
    state: SessionState,
    active_trigger_source: Option<TriggerSource>,
}

impl RecordingSession {
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
            active_trigger_source: None,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn active_trigger_source(&self) -> Option<TriggerSource> {
        self.active_trigger_source
    }

    pub fn start_recording(
        &mut self,
        trigger_source: TriggerSource,
    ) -> Result<SessionState, SessionError> {
        if self.state == SessionState::Recording {
            return Err(SessionError::AlreadyRecording);
        }

        self.state = SessionState::Recording;
        self.active_trigger_source = Some(trigger_source);
        Ok(self.state)
    }

    pub fn stop_recording(&mut self) -> Result<SessionState, SessionError> {
        if self.state == SessionState::Idle {
            return Err(SessionError::NotRecording);
        }

        self.state = SessionState::Idle;
        self.active_trigger_source = None;
        Ok(self.state)
    }
}

impl Default for RecordingSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod session_state {
    use super::*;

    #[test]
    fn session_state_starts_idle() {
        let session = RecordingSession::new();

        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.active_trigger_source(), None);
    }

    #[test]
    fn session_state_starts_recording() {
        let mut session = RecordingSession::new();

        session.start_recording(TriggerSource::ModifierOnly).unwrap();

        assert_eq!(session.state(), SessionState::Recording);
        assert_eq!(
            session.active_trigger_source(),
            Some(TriggerSource::ModifierOnly)
        );
    }

    #[test]
    fn session_state_stops_recording() {
        let mut session = RecordingSession::new();
        session.start_recording(TriggerSource::ModifierOnly).unwrap();

        session.stop_recording().unwrap();

        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.active_trigger_source(), None);
    }

    #[test]
    fn session_state_rejects_duplicate_start() {
        let mut session = RecordingSession::new();
        session.start_recording(TriggerSource::ModifierOnly).unwrap();

        let error = session
            .start_recording(TriggerSource::StandardShortcut)
            .unwrap_err();

        assert_eq!(error, SessionError::AlreadyRecording);
    }

    #[test]
    fn session_state_rejects_duplicate_stop() {
        let mut session = RecordingSession::new();

        let error = session.stop_recording().unwrap_err();

        assert_eq!(error, SessionError::NotRecording);
    }

    #[test]
    fn session_state_tracks_trigger_sources() {
        let mut session = RecordingSession::new();

        session.start_recording(TriggerSource::StandardShortcut).unwrap();
        assert_eq!(
            session.active_trigger_source(),
            Some(TriggerSource::StandardShortcut)
        );

        session.stop_recording().unwrap();
        session.start_recording(TriggerSource::ShellAction).unwrap();

        assert_eq!(session.active_trigger_source(), Some(TriggerSource::ShellAction));
    }

    #[test]
    fn session_state_flags_duplicate_requests() {
        assert!(SessionError::AlreadyRecording.is_duplicate_request());
        assert!(SessionError::NotRecording.is_duplicate_request());
    }
}
