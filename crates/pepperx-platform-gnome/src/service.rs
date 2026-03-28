use pepperx_ipc::{
    parse_trigger_source, Capabilities, CapabilityPayload, OBJECT_PATH, SERVICE_NAME,
};
use pepperx_session::{RecordingSession, SessionError, SessionState, TriggerSource};
use std::sync::{
    mpsc::Sender,
    Arc, Mutex,
};
use zbus::{
    blocking::{connection::Builder as ConnectionBuilder, Connection},
    fdo,
    interface,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    ShowSettings,
    ShowHistory,
}

#[derive(Debug)]
pub struct ServiceHandle {
    _connection: Connection,
}

impl ServiceHandle {
    pub fn start(command_sender: Sender<AppCommand>) -> zbus::Result<Self> {
        let service = PepperXService::new(command_sender);
        let connection = ConnectionBuilder::session()?
            .name(SERVICE_NAME)?
            .serve_at(OBJECT_PATH, service)?
            .build()?;

        Ok(Self {
            _connection: connection,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PepperXService {
    state: Arc<ServiceState>,
}

#[derive(Debug)]
struct ServiceState {
    session: Mutex<RecordingSession>,
    capabilities: Mutex<Capabilities>,
    command_sender: Sender<AppCommand>,
}

impl PepperXService {
    pub fn new(command_sender: Sender<AppCommand>) -> Self {
        Self {
            state: Arc::new(ServiceState {
                session: Mutex::new(RecordingSession::new()),
                capabilities: Mutex::new(Capabilities::shell_default(env!("CARGO_PKG_VERSION"))),
                command_sender,
            }),
        }
    }

    pub fn session_state(&self) -> SessionState {
        self.state.session.lock().expect("session lock poisoned").state()
    }

    fn mark_extension_connected(&self) {
        self.state
            .capabilities
            .lock()
            .expect("capabilities lock poisoned")
            .extension_connected = true;
    }

    fn capabilities(&self) -> Capabilities {
        self.state
            .capabilities
            .lock()
            .expect("capabilities lock poisoned")
            .clone()
    }

    fn send_command(&self, command: AppCommand) -> fdo::Result<()> {
        self.state.command_sender.send(command).map_err(|error| {
            fdo::Error::Failed(format!("failed to route app command: {error}"))
        })
    }

    fn start_session(&self, trigger_source: TriggerSource) -> fdo::Result<()> {
        match self
            .state
            .session
            .lock()
            .expect("session lock poisoned")
            .start_recording(trigger_source)
        {
            Ok(_) => Ok(()),
            Err(SessionError::AlreadyRecording) => {
                eprintln!("[Pepper X] duplicate request ignored: start");
                Ok(())
            }
            Err(error) => Err(fdo::Error::Failed(format!(
                "failed to start recording: {error:?}"
            ))),
        }
    }

    fn stop_session(&self) -> fdo::Result<()> {
        match self
            .state
            .session
            .lock()
            .expect("session lock poisoned")
            .stop_recording()
        {
            Ok(_) => Ok(()),
            Err(SessionError::NotRecording) => {
                eprintln!("[Pepper X] duplicate request ignored: stop");
                Ok(())
            }
            Err(error) => Err(fdo::Error::Failed(format!(
                "failed to stop recording: {error:?}"
            ))),
        }
    }
}

#[interface(name = "com.obra.PepperX")]
impl PepperXService {
    fn ping(&self) -> &'static str {
        self.mark_extension_connected();
        "pong"
    }

    fn start_recording(&self, trigger_source: &str) -> fdo::Result<()> {
        let trigger_source = parse_trigger_source(trigger_source)
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;

        self.start_session(trigger_source)
    }

    fn stop_recording(&self) -> fdo::Result<()> {
        self.stop_session()
    }

    fn show_settings(&self) -> fdo::Result<()> {
        self.send_command(AppCommand::ShowSettings)
    }

    fn show_history(&self) -> fdo::Result<()> {
        self.send_command(AppCommand::ShowHistory)
    }

    fn get_capabilities(&self) -> CapabilityPayload {
        self.capabilities().to_dbus_payload()
    }
}

#[cfg(test)]
mod service_contract {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn service_contract_routes_start_and_stop_recording() {
        let (sender, _receiver) = channel();
        let service = PepperXService::new(sender);

        assert_eq!(service.ping(), "pong");
        service
            .start_recording(pepperx_ipc::trigger_source_name(TriggerSource::ModifierOnly))
            .unwrap();
        assert_eq!(service.session_state(), SessionState::Recording);

        service.stop_recording().unwrap();
        assert_eq!(service.session_state(), SessionState::Idle);
    }

    #[test]
    fn service_contract_routes_shell_actions() {
        let (sender, receiver) = channel();
        let service = PepperXService::new(sender);

        service.show_settings().unwrap();
        assert_eq!(receiver.recv().unwrap(), AppCommand::ShowSettings);

        service.show_history().unwrap();
        assert_eq!(receiver.recv().unwrap(), AppCommand::ShowHistory);
    }

    #[test]
    fn service_contract_reports_capabilities() {
        let (sender, _receiver) = channel();
        let service = PepperXService::new(sender);

        assert_eq!(
            Capabilities::from_dbus_payload(service.get_capabilities()),
            Capabilities {
                modifier_only_supported: true,
                extension_connected: false,
                version: "0.1.0".to_string(),
            }
        );
    }

    #[test]
    fn service_contract_ignores_duplicate_start_requests() {
        let (sender, _receiver) = channel();
        let service = PepperXService::new(sender);

        service
            .start_recording(pepperx_ipc::trigger_source_name(TriggerSource::ModifierOnly))
            .unwrap();
        service
            .start_recording(pepperx_ipc::trigger_source_name(TriggerSource::ModifierOnly))
            .unwrap();

        assert_eq!(service.session_state(), SessionState::Recording);
    }

    #[test]
    fn service_contract_ignores_duplicate_stop_requests() {
        let (sender, _receiver) = channel();
        let service = PepperXService::new(sender);

        service.stop_recording().unwrap();

        assert_eq!(service.session_state(), SessionState::Idle);
    }
}
