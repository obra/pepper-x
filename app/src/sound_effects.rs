use std::process::Command;

/// A sound event to play via `canberra-gtk-play`.
///
/// Uses XDG sound-naming-spec event IDs so the desktop theme can resolve
/// the actual .oga file.  Playback is always non-blocking and failures
/// are silently ignored so recording is never delayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    RecordingStart,
    RecordingStop,
}

impl SoundEvent {
    /// XDG sound event ID passed to `canberra-gtk-play -i <id>`.
    fn event_id(self) -> &'static str {
        match self {
            SoundEvent::RecordingStart => "bell",
            SoundEvent::RecordingStop => "complete",
        }
    }

    /// Human-readable description passed to `canberra-gtk-play -d <desc>`.
    fn description(self) -> &'static str {
        match self {
            SoundEvent::RecordingStart => "Pepper X recording started",
            SoundEvent::RecordingStop => "Pepper X recording stopped",
        }
    }
}

/// Play a sound event in a background thread.
///
/// The function returns immediately.  If `canberra-gtk-play` is missing
/// or the playback fails for any reason the error is logged to stderr
/// and silently discarded.
pub fn play_sound(event: SoundEvent) {
    std::thread::Builder::new()
        .name("pepperx-sound-effect".into())
        .spawn(move || {
            let result = Command::new("canberra-gtk-play")
                .arg("-i")
                .arg(event.event_id())
                .arg("-d")
                .arg(event.description())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            match result {
                Ok(status) if !status.success() => {
                    eprintln!(
                        "[Pepper X] sound effect {:?} exited with {}",
                        event.event_id(),
                        status
                    );
                }
                Err(error) => {
                    eprintln!(
                        "[Pepper X] failed to play sound effect {:?}: {error}",
                        event.event_id()
                    );
                }
                Ok(_) => {}
            }
        })
        .ok(); // If thread spawn itself fails, silently ignore
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_event_ids_are_valid_xdg_names() {
        // XDG sound names must be non-empty and not contain path separators
        for event in [SoundEvent::RecordingStart, SoundEvent::RecordingStop] {
            let id = event.event_id();
            assert!(!id.is_empty());
            assert!(!id.contains('/'));
            assert!(!id.contains('\\'));
        }
    }

    #[test]
    fn sound_events_have_distinct_ids() {
        assert_ne!(
            SoundEvent::RecordingStart.event_id(),
            SoundEvent::RecordingStop.event_id()
        );
    }

    #[test]
    fn sound_events_have_descriptions() {
        assert!(!SoundEvent::RecordingStart.description().is_empty());
        assert!(!SoundEvent::RecordingStop.description().is_empty());
    }
}
