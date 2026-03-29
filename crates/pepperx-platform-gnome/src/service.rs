use pepperx_ipc::{
    parse_trigger_source, Capabilities, CapabilityPayload, OBJECT_PATH, SERVICE_NAME,
};
use pepperx_session::TriggerSource;
use std::sync::{mpsc::Sender, Arc, Mutex};
use zbus::{
    blocking::{connection::Builder as ConnectionBuilder, Connection},
    fdo, interface,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    ShowSettings,
    ShowHistory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingRuntimeError {
    DuplicateStart,
    DuplicateStop,
    Failed(String),
}

pub trait RecordingRuntime: Send + Sync {
    fn start_recording(&self, trigger_source: TriggerSource) -> Result<(), RecordingRuntimeError>;

    fn stop_recording(&self) -> Result<(), RecordingRuntimeError>;
}

pub struct ServiceHandle {
    _connection: Connection,
    service: PepperXService,
}

impl ServiceHandle {
    pub fn start(
        command_sender: Sender<AppCommand>,
        recording_runtime: Arc<dyn RecordingRuntime>,
    ) -> zbus::Result<Self> {
        let service = PepperXService::new(command_sender, recording_runtime);
        let connection = ConnectionBuilder::session()?
            .name(SERVICE_NAME)?
            .serve_at(OBJECT_PATH, service.clone())?
            .build()?;

        Ok(Self {
            _connection: connection,
            service,
        })
    }

    pub fn service(&self) -> PepperXService {
        self.service.clone()
    }
}

#[derive(Clone)]
pub struct PepperXService {
    state: Arc<ServiceState>,
}

impl std::fmt::Debug for PepperXService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PepperXService").finish_non_exhaustive()
    }
}

struct ServiceState {
    recording_runtime: Arc<dyn RecordingRuntime>,
    capabilities: Mutex<Capabilities>,
    command_sender: Sender<AppCommand>,
}

impl PepperXService {
    pub fn new(
        command_sender: Sender<AppCommand>,
        recording_runtime: Arc<dyn RecordingRuntime>,
    ) -> Self {
        Self {
            state: Arc::new(ServiceState {
                recording_runtime,
                capabilities: Mutex::new(Capabilities::shell_default(env!("CARGO_PKG_VERSION"))),
                command_sender,
            }),
        }
    }

    pub fn set_modifier_only_supported(&self, supported: bool) {
        self.state
            .capabilities
            .lock()
            .expect("capabilities lock poisoned")
            .modifier_only_supported = supported;
    }

    pub fn start_modifier_only_recording(&self) {
        match self.start_session(TriggerSource::ModifierOnly) {
            Ok(()) => eprintln!("[Pepper X] modifier-only start"),
            Err(error) => eprintln!("[Pepper X] modifier-only start failed: {error}"),
        }
    }

    pub fn stop_modifier_only_recording(&self) {
        match self.stop_session() {
            Ok(()) => eprintln!("[Pepper X] modifier-only stop"),
            Err(error) => eprintln!("[Pepper X] modifier-only stop failed: {error}"),
        }
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
        self.state
            .command_sender
            .send(command)
            .map_err(|error| fdo::Error::Failed(format!("failed to route app command: {error}")))
    }

    fn start_session(&self, trigger_source: TriggerSource) -> fdo::Result<()> {
        match self.state.recording_runtime.start_recording(trigger_source) {
            Ok(()) => Ok(()),
            Err(RecordingRuntimeError::DuplicateStart) => {
                eprintln!("[Pepper X] duplicate request ignored: start");
                Ok(())
            }
            Err(RecordingRuntimeError::DuplicateStop) => Err(fdo::Error::Failed(
                "failed to start recording: runtime reported duplicate stop".into(),
            )),
            Err(RecordingRuntimeError::Failed(error)) => Err(fdo::Error::Failed(format!(
                "failed to start recording: {error}"
            ))),
        }
    }

    fn stop_session(&self) -> fdo::Result<()> {
        match self.state.recording_runtime.stop_recording() {
            Ok(()) => Ok(()),
            Err(RecordingRuntimeError::DuplicateStop) => {
                eprintln!("[Pepper X] duplicate request ignored: stop");
                Ok(())
            }
            Err(RecordingRuntimeError::DuplicateStart) => Err(fdo::Error::Failed(
                "failed to stop recording: runtime reported duplicate start".into(),
            )),
            Err(RecordingRuntimeError::Failed(error)) => Err(fdo::Error::Failed(format!(
                "failed to stop recording: {error}"
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
    use std::collections::VecDeque;
    use std::sync::{mpsc::channel, Arc, Mutex};

    #[derive(Debug)]
    struct FakeRecordingRuntime {
        started_triggers: Mutex<Vec<TriggerSource>>,
        stop_calls: Mutex<usize>,
        start_results: Mutex<VecDeque<Result<(), RecordingRuntimeError>>>,
        stop_results: Mutex<VecDeque<Result<(), RecordingRuntimeError>>>,
    }

    impl FakeRecordingRuntime {
        fn succeeding() -> Arc<Self> {
            Arc::new(Self {
                started_triggers: Mutex::new(Vec::new()),
                stop_calls: Mutex::new(0),
                start_results: Mutex::new(VecDeque::from([Ok(())])),
                stop_results: Mutex::new(VecDeque::from([Ok(())])),
            })
        }

        fn duplicate_start() -> Arc<Self> {
            Arc::new(Self {
                started_triggers: Mutex::new(Vec::new()),
                stop_calls: Mutex::new(0),
                start_results: Mutex::new(VecDeque::from([
                    Ok(()),
                    Err(RecordingRuntimeError::DuplicateStart),
                ])),
                stop_results: Mutex::new(VecDeque::from([Ok(())])),
            })
        }

        fn duplicate_stop() -> Arc<Self> {
            Arc::new(Self {
                started_triggers: Mutex::new(Vec::new()),
                stop_calls: Mutex::new(0),
                start_results: Mutex::new(VecDeque::from([Ok(())])),
                stop_results: Mutex::new(VecDeque::from([
                    Ok(()),
                    Err(RecordingRuntimeError::DuplicateStop),
                ])),
            })
        }

        fn failing_stop(message: &str) -> Arc<Self> {
            Arc::new(Self {
                started_triggers: Mutex::new(Vec::new()),
                stop_calls: Mutex::new(0),
                start_results: Mutex::new(VecDeque::from([Ok(())])),
                stop_results: Mutex::new(VecDeque::from([Err(RecordingRuntimeError::Failed(
                    message.into(),
                ))])),
            })
        }
    }

    impl RecordingRuntime for FakeRecordingRuntime {
        fn start_recording(
            &self,
            trigger_source: TriggerSource,
        ) -> Result<(), RecordingRuntimeError> {
            self.started_triggers.lock().unwrap().push(trigger_source);
            self.start_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }

        fn stop_recording(&self) -> Result<(), RecordingRuntimeError> {
            *self.stop_calls.lock().unwrap() += 1;
            self.stop_results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(()))
        }
    }

    #[test]
    fn service_contract_routes_start_and_stop_recording_through_runtime() {
        let (sender, _receiver) = channel();
        let runtime = FakeRecordingRuntime::succeeding();
        let service = PepperXService::new(sender, runtime.clone());

        assert_eq!(service.ping(), "pong");
        service
            .start_recording(pepperx_ipc::trigger_source_name(
                TriggerSource::ModifierOnly,
            ))
            .unwrap();
        service.stop_recording().unwrap();

        assert_eq!(
            *runtime.started_triggers.lock().unwrap(),
            vec![TriggerSource::ModifierOnly]
        );
        assert_eq!(*runtime.stop_calls.lock().unwrap(), 1);
    }

    #[test]
    fn service_contract_routes_shell_actions() {
        let (sender, receiver) = channel();
        let service = PepperXService::new(sender, FakeRecordingRuntime::succeeding());

        service.show_settings().unwrap();
        assert_eq!(receiver.recv().unwrap(), AppCommand::ShowSettings);

        service.show_history().unwrap();
        assert_eq!(receiver.recv().unwrap(), AppCommand::ShowHistory);
    }

    #[test]
    fn service_contract_reports_capabilities() {
        let (sender, _receiver) = channel();
        let service = PepperXService::new(sender, FakeRecordingRuntime::succeeding());

        assert_eq!(
            Capabilities::from_dbus_payload(service.get_capabilities()),
            Capabilities {
                modifier_only_supported: false,
                extension_connected: false,
                version: "0.1.0".to_string(),
            }
        );
    }

    #[test]
    fn service_contract_ignores_duplicate_start_requests() {
        let (sender, _receiver) = channel();
        let runtime = FakeRecordingRuntime::duplicate_start();
        let service = PepperXService::new(sender, runtime.clone());

        service
            .start_recording(pepperx_ipc::trigger_source_name(
                TriggerSource::ModifierOnly,
            ))
            .unwrap();
        service
            .start_recording(pepperx_ipc::trigger_source_name(
                TriggerSource::ModifierOnly,
            ))
            .unwrap();

        assert_eq!(
            *runtime.started_triggers.lock().unwrap(),
            vec![TriggerSource::ModifierOnly, TriggerSource::ModifierOnly]
        );
    }

    #[test]
    fn service_contract_ignores_duplicate_stop_requests() {
        let (sender, _receiver) = channel();
        let runtime = FakeRecordingRuntime::duplicate_stop();
        let service = PepperXService::new(sender, runtime.clone());

        service
            .start_recording(pepperx_ipc::trigger_source_name(
                TriggerSource::ModifierOnly,
            ))
            .unwrap();
        service.stop_recording().unwrap();
        service.stop_recording().unwrap();

        assert_eq!(*runtime.stop_calls.lock().unwrap(), 2);
    }

    #[test]
    fn service_contract_surfaces_runtime_failures() {
        let (sender, _receiver) = channel();
        let service = PepperXService::new(sender, FakeRecordingRuntime::failing_stop("boom"));

        let error = service.stop_recording().unwrap_err();

        match error {
            fdo::Error::Failed(message) => assert!(message.contains("boom")),
            other => panic!("expected failed D-Bus error, got {other:?}"),
        }
    }
}
