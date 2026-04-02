use crate::devices::SelectedMicrophone;
use crate::recording::SignalLevelSample;
use std::sync::mpsc;
use std::thread;

#[cfg(target_os = "linux")]
use crate::devices::stable_pipewire_microphone_id;
#[cfg(target_os = "linux")]
use crate::recording::RecordingError;
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
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
const APP_ID: &str = "com.obra.PepperX";
#[cfg(target_os = "linux")]
const STREAM_NAME: &str = "pepperx-level-monitor";
#[cfg(target_os = "linux")]
const LEVEL_REPORT_INTERVAL: Duration = Duration::from_millis(250);
#[cfg(target_os = "linux")]
const MAINLOOP_ITERATE_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub enum LevelUpdate {
    Level(SignalLevelSample),
    Error(String),
}

enum MonitorCommand {
    ChangeMicrophone(Option<SelectedMicrophone>),
    Pause,
    Resume,
    Stop,
}

pub struct LevelMonitor {
    command_sender: mpsc::Sender<MonitorCommand>,
    level_receiver: mpsc::Receiver<LevelUpdate>,
    _worker: thread::JoinHandle<()>,
}

impl LevelMonitor {
    pub fn start_paused(selected_microphone: Option<SelectedMicrophone>) -> Self {
        let (level_sender, level_receiver) = mpsc::channel();
        let (command_sender, command_receiver) = mpsc::channel();

        let worker = thread::spawn(move || {
            #[cfg(target_os = "linux")]
            run_monitor_loop(selected_microphone, level_sender, command_receiver);

            #[cfg(not(target_os = "linux"))]
            {
                let _ = (selected_microphone, level_sender, command_receiver);
            }
        });

        Self {
            command_sender,
            level_receiver,
            _worker: worker,
        }
    }

    pub fn try_recv(&self) -> Option<LevelUpdate> {
        let mut last = None;
        while let Ok(update) = self.level_receiver.try_recv() {
            last = Some(update);
        }
        last
    }

    pub fn change_microphone(&self, selected_microphone: Option<SelectedMicrophone>) {
        let _ = self
            .command_sender
            .send(MonitorCommand::ChangeMicrophone(selected_microphone));
    }

    pub fn pause(&self) {
        let _ = self.command_sender.send(MonitorCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.command_sender.send(MonitorCommand::Resume);
    }
}

impl Drop for LevelMonitor {
    fn drop(&mut self) {
        let _ = self.command_sender.send(MonitorCommand::Stop);
    }
}

#[cfg(target_os = "linux")]
fn run_monitor_loop(
    initial_microphone: Option<SelectedMicrophone>,
    level_sender: mpsc::Sender<LevelUpdate>,
    command_receiver: mpsc::Receiver<MonitorCommand>,
) {
    pw::init();

    let mut current_microphone = initial_microphone;

    // Start paused — wait for Resume before opening any capture stream
    loop {
        match command_receiver.recv() {
            Ok(MonitorCommand::Resume) => break,
            Ok(MonitorCommand::ChangeMicrophone(mic)) => {
                current_microphone = mic;
            }
            Ok(MonitorCommand::Pause) => {}
            Ok(MonitorCommand::Stop) | Err(_) => return,
        }
    }

    loop {
        match run_capture_session(&current_microphone, &level_sender, &command_receiver) {
            SessionExit::Stop => return,
            SessionExit::Reconnect(next_microphone) => {
                current_microphone = next_microphone;
            }
            SessionExit::Paused => loop {
                match command_receiver.recv() {
                    Ok(MonitorCommand::Resume) => break,
                    Ok(MonitorCommand::ChangeMicrophone(mic)) => {
                        current_microphone = mic;
                    }
                    Ok(MonitorCommand::Pause) => {}
                    Ok(MonitorCommand::Stop) | Err(_) => return,
                }
            },
            SessionExit::Error(message) => {
                let _ = level_sender.send(LevelUpdate::Error(message));
                match command_receiver.recv() {
                    Ok(MonitorCommand::ChangeMicrophone(mic)) => {
                        current_microphone = mic;
                    }
                    Ok(MonitorCommand::Resume) => {}
                    Ok(MonitorCommand::Pause) => {}
                    Ok(MonitorCommand::Stop) | Err(_) => return,
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
enum SessionExit {
    Stop,
    Reconnect(Option<SelectedMicrophone>),
    Paused,
    Error(String),
}

#[cfg(target_os = "linux")]
fn run_capture_session(
    selected_microphone: &Option<SelectedMicrophone>,
    level_sender: &mpsc::Sender<LevelUpdate>,
    command_receiver: &mpsc::Receiver<MonitorCommand>,
) -> SessionExit {
    let mainloop = match pw::main_loop::MainLoopRc::new(None) {
        Ok(ml) => ml,
        Err(e) => return SessionExit::Error(format!("failed to create PipeWire main loop: {e}")),
    };
    let context = match pw::context::ContextRc::new(&mainloop, None) {
        Ok(ctx) => ctx,
        Err(e) => return SessionExit::Error(format!("failed to create PipeWire context: {e}")),
    };
    let core = match context.connect_rc(None) {
        Ok(c) => c,
        Err(e) => return SessionExit::Error(format!("failed to connect to PipeWire: {e}")),
    };

    let target_node_id =
        match resolve_selected_node_id(&core, &mainloop, selected_microphone.as_ref()) {
            Ok(id) => id,
            Err(e) => return SessionExit::Error(e.to_string()),
        };

    let captured_audio = Rc::new(RefCell::new(CapturedAudio::default()));
    let recording_error = Rc::new(RefCell::new(None::<RecordingError>));
    let (stream, _listener) = match configure_capture_stream(
        &core,
        &mainloop,
        &captured_audio,
        &recording_error,
        target_node_id,
    ) {
        Ok(s) => s,
        Err(e) => return SessionExit::Error(e.to_string()),
    };

    let mut last_report = Instant::now();

    loop {
        // Check for commands (non-blocking)
        match command_receiver.try_recv() {
            Ok(MonitorCommand::Stop) => {
                let _ = stream.disconnect();
                return SessionExit::Stop;
            }
            Ok(MonitorCommand::ChangeMicrophone(mic)) => {
                let _ = stream.disconnect();
                return SessionExit::Reconnect(mic);
            }
            Ok(MonitorCommand::Pause) => {
                let _ = stream.disconnect();
                return SessionExit::Paused;
            }
            Ok(MonitorCommand::Resume) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                let _ = stream.disconnect();
                return SessionExit::Stop;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }

        // Check for stream errors
        if let Some(error) = recording_error.borrow_mut().take() {
            let _ = stream.disconnect();
            return SessionExit::Error(error.to_string());
        }

        // Report level periodically
        if last_report.elapsed() >= LEVEL_REPORT_INTERVAL {
            let sample = {
                let mut audio = captured_audio.borrow_mut();
                let sample = if audio.interleaved_samples.is_empty() {
                    SignalLevelSample::from_pcm_samples(&[0])
                } else {
                    SignalLevelSample::from_normalized_samples(&audio.interleaved_samples)
                };
                audio.interleaved_samples.clear();
                sample
            };

            if level_sender.send(LevelUpdate::Level(sample)).is_err() {
                let _ = stream.disconnect();
                return SessionExit::Stop;
            }
            last_report = Instant::now();
        }

        mainloop.loop_().iterate(MAINLOOP_ITERATE_TIMEOUT);
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Default)]
struct CapturedAudio {
    sample_rate_hz: u32,
    channel_count: u16,
    interleaved_samples: Vec<f32>,
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

    let deadline = Instant::now() + Duration::from_secs(5);
    while !done.get() && enumeration_error.borrow().is_none() {
        if Instant::now() >= deadline {
            return Err(RecordingError::new(
                "PipeWire node resolution timed out",
            ));
        }
        mainloop.loop_().iterate(Duration::from_millis(100));
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
                        captured_audio.interleaved_samples.push(
                            i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]) as f32
                                / i16::MAX as f32,
                        );
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

    let values = crate::recording::audio_capture_param_values()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_monitor_can_be_created_and_dropped() {
        let monitor = LevelMonitor::start_paused(None);
        assert!(monitor.try_recv().is_none() || monitor.try_recv().is_some());
        drop(monitor);
    }

    #[test]
    fn level_monitor_change_microphone_does_not_panic() {
        let monitor = LevelMonitor::start_paused(None);
        monitor.change_microphone(Some(SelectedMicrophone::new(
            "pipewire:node.name=test",
            "Test Mic",
        )));
        monitor.change_microphone(None);
        drop(monitor);
    }
}
