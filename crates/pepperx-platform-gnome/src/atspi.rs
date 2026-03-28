use crate::service::PepperXService;
use std::ffi::{c_char, c_void, CString};
use std::fmt;
use std::ptr::NonNull;
use std::sync::Mutex;

const CONTROL_LEFT_KEYSYM: u32 = 65_507;
const CONTROL_RIGHT_KEYSYM: u32 = 65_508;

pub const FRIENDLY_INSERT_BACKEND_NAME: &str = "atspi-editable-text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FriendlyInsertPolicy {
    pub target_application_id: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendlyFocusedTarget {
    pub application_id: String,
    pub is_editable: bool,
    pub supports_editable_text: bool,
    pub supports_caret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendlyInsertSelection {
    pub backend_name: &'static str,
    pub target_application_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendlyInsertFailure {
    pub backend_name: &'static str,
    pub reason: FriendlyInsertError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendlyInsertError {
    UnsupportedApplication {
        expected_application_id: String,
        actual_application_id: String,
    },
    TargetNotEditable,
    MissingEditableText,
    MissingCaretSurface,
}

impl fmt::Display for FriendlyInsertFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.backend_name, self.reason)
    }
}

impl fmt::Display for FriendlyInsertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedApplication {
                expected_application_id,
                actual_application_id,
            } => {
                write!(
                    f,
                    "friendly insertion target application id {} does not match {}",
                    actual_application_id, expected_application_id
                )
            }
            Self::TargetNotEditable => f.write_str("friendly insertion target is not editable"),
            Self::MissingEditableText => {
                f.write_str("friendly insertion target is missing EditableText support")
            }
            Self::MissingCaretSurface => {
                f.write_str("friendly insertion target is missing a caret surface")
            }
        }
    }
}

#[derive(Debug)]
pub struct ModifierCaptureHandle {
    device: NonNull<ffi::AtspiDevice>,
}

impl ModifierCaptureHandle {
    pub fn start(app_id: &str, service: PepperXService) -> Result<Self, ModifierCaptureError> {
        let app_id =
            CString::new(app_id).map_err(|_| ModifierCaptureError::InvalidApplicationId)?;

        unsafe {
            let init_result = ffi::atspi_init();
            if init_result < 0 {
                return Err(ModifierCaptureError::InitializationFailed(init_result));
            }

            let device =
                NonNull::new(ffi::atspi_device_a11y_manager_try_new_full(app_id.as_ptr()).cast())
                    .ok_or(ModifierCaptureError::Unavailable)?;

            let callback_state = Box::new(CallbackState::new(service));
            ffi::atspi_device_add_key_watcher(
                device.as_ptr(),
                Some(key_watcher_callback),
                Box::into_raw(callback_state).cast(),
                Some(destroy_callback),
            );

            if ffi::atspi_device_grab_keyboard(device.as_ptr()) == glib::ffi::GFALSE {
                glib::gobject_ffi::g_object_unref(device.as_ptr().cast());
                return Err(ModifierCaptureError::KeyboardGrabFailed);
            }

            Ok(Self { device })
        }
    }
}

impl Drop for ModifierCaptureHandle {
    fn drop(&mut self) {
        unsafe {
            ffi::atspi_device_ungrab_keyboard(self.device.as_ptr());
            glib::gobject_ffi::g_object_unref(self.device.as_ptr().cast());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierCaptureError {
    InvalidApplicationId,
    InitializationFailed(i32),
    Unavailable,
    KeyboardGrabFailed,
}

impl fmt::Display for ModifierCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidApplicationId => f.write_str("invalid AT-SPI application id"),
            Self::InitializationFailed(code) => {
                write!(f, "AT-SPI initialization failed with code {code}")
            }
            Self::Unavailable => {
                f.write_str("GNOME 48 accessibility keyboard monitoring is unavailable")
            }
            Self::KeyboardGrabFailed => f.write_str("AT-SPI keyboard grab failed"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoldSignal {
    Start,
    Stop,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ModifierHoldState {
    control_mask: u8,
    active: bool,
    chord_blocked: bool,
}

impl ModifierHoldState {
    fn handle_key_event(&mut self, pressed: bool, keysym: u32) -> Option<HoldSignal> {
        if let Some(bit) = control_bit(keysym) {
            if pressed {
                let already_down = self.control_mask & bit != 0;
                self.control_mask |= bit;

                if already_down || self.active || self.chord_blocked {
                    return None;
                }

                self.active = true;
                return Some(HoldSignal::Start);
            }

            self.control_mask &= !bit;

            if self.active && self.control_mask == 0 {
                self.active = false;
                self.chord_blocked = false;
                return Some(HoldSignal::Stop);
            }

            if self.control_mask == 0 {
                self.chord_blocked = false;
            }

            return None;
        }

        if !pressed || self.control_mask == 0 {
            return None;
        }

        if self.active {
            self.active = false;
            self.chord_blocked = true;
            return Some(HoldSignal::Stop);
        }

        self.chord_blocked = true;
        None
    }
}

#[derive(Debug)]
struct CallbackState {
    service: PepperXService,
    hold_state: Mutex<ModifierHoldState>,
}

impl CallbackState {
    fn new(service: PepperXService) -> Self {
        Self {
            service,
            hold_state: Mutex::new(ModifierHoldState::default()),
        }
    }

    fn handle_key_event(&self, pressed: bool, keysym: u32) {
        let signal = self
            .hold_state
            .lock()
            .expect("modifier hold state lock poisoned")
            .handle_key_event(pressed, keysym);

        match signal {
            Some(HoldSignal::Start) => self.service.start_modifier_only_recording(),
            Some(HoldSignal::Stop) => self.service.stop_modifier_only_recording(),
            None => {}
        }
    }
}

pub fn select_friendly_insert_backend(
    target: &FriendlyFocusedTarget,
    policy: &FriendlyInsertPolicy,
) -> Result<FriendlyInsertSelection, FriendlyInsertFailure> {
    if target.application_id != policy.target_application_id {
        return Err(FriendlyInsertFailure {
            backend_name: FRIENDLY_INSERT_BACKEND_NAME,
            reason: FriendlyInsertError::UnsupportedApplication {
                expected_application_id: policy.target_application_id.into(),
                actual_application_id: target.application_id.clone(),
            },
        });
    }

    if !target.is_editable {
        return Err(FriendlyInsertFailure {
            backend_name: FRIENDLY_INSERT_BACKEND_NAME,
            reason: FriendlyInsertError::TargetNotEditable,
        });
    }

    if !target.supports_editable_text {
        return Err(FriendlyInsertFailure {
            backend_name: FRIENDLY_INSERT_BACKEND_NAME,
            reason: FriendlyInsertError::MissingEditableText,
        });
    }

    if !target.supports_caret {
        return Err(FriendlyInsertFailure {
            backend_name: FRIENDLY_INSERT_BACKEND_NAME,
            reason: FriendlyInsertError::MissingCaretSurface,
        });
    }

    Ok(FriendlyInsertSelection {
        backend_name: FRIENDLY_INSERT_BACKEND_NAME,
        target_application_id: target.application_id.clone(),
    })
}

fn control_bit(keysym: u32) -> Option<u8> {
    match keysym {
        CONTROL_LEFT_KEYSYM => Some(0b01),
        CONTROL_RIGHT_KEYSYM => Some(0b10),
        _ => None,
    }
}

unsafe extern "C" fn key_watcher_callback(
    _device: *mut ffi::AtspiDevice,
    pressed: glib::ffi::gboolean,
    _keycode: glib::ffi::guint,
    keysym: glib::ffi::guint,
    _modifiers: glib::ffi::guint,
    _keystring: *const c_char,
    user_data: glib::ffi::gpointer,
) {
    let callback_state = &*(user_data.cast::<CallbackState>());
    callback_state.handle_key_event(pressed != glib::ffi::GFALSE, keysym);
}

unsafe extern "C" fn destroy_callback(user_data: glib::ffi::gpointer) {
    if !user_data.is_null() {
        drop(Box::from_raw(user_data.cast::<CallbackState>()));
    }
}

mod ffi {
    use super::c_void;
    use glib::ffi::{gboolean, gchar, gint, gpointer, guint, GDestroyNotify};

    #[repr(C)]
    pub struct AtspiDevice(c_void);

    #[link(name = "atspi")]
    unsafe extern "C" {
        pub fn atspi_init() -> gint;
        pub fn atspi_device_a11y_manager_try_new_full(app_id: *const gchar) -> *mut AtspiDevice;
        pub fn atspi_device_add_key_watcher(
            device: *mut AtspiDevice,
            callback: Option<
                unsafe extern "C" fn(
                    device: *mut AtspiDevice,
                    pressed: gboolean,
                    keycode: guint,
                    keysym: guint,
                    modifiers: guint,
                    keystring: *const gchar,
                    user_data: gpointer,
                ),
            >,
            user_data: gpointer,
            callback_destroyed: GDestroyNotify,
        );
        pub fn atspi_device_grab_keyboard(device: *mut AtspiDevice) -> gboolean;
        pub fn atspi_device_ungrab_keyboard(device: *mut AtspiDevice);
    }
}

#[cfg(test)]
mod modifier_hold_state {
    use super::*;

    #[test]
    fn modifier_hold_state_starts_on_first_control_press() {
        let mut state = ModifierHoldState::default();

        assert_eq!(
            state.handle_key_event(true, CONTROL_LEFT_KEYSYM),
            Some(HoldSignal::Start)
        );
        assert_eq!(state.handle_key_event(true, CONTROL_LEFT_KEYSYM), None);
        assert_eq!(
            state.handle_key_event(false, CONTROL_LEFT_KEYSYM),
            Some(HoldSignal::Stop)
        );
    }

    #[test]
    fn modifier_hold_state_waits_for_last_control_release() {
        let mut state = ModifierHoldState::default();

        assert_eq!(
            state.handle_key_event(true, CONTROL_LEFT_KEYSYM),
            Some(HoldSignal::Start)
        );
        assert_eq!(state.handle_key_event(true, CONTROL_RIGHT_KEYSYM), None);
        assert_eq!(state.handle_key_event(false, CONTROL_LEFT_KEYSYM), None);
        assert_eq!(
            state.handle_key_event(false, CONTROL_RIGHT_KEYSYM),
            Some(HoldSignal::Stop)
        );
    }

    #[test]
    fn modifier_hold_state_cancels_modifier_shortcuts() {
        let mut state = ModifierHoldState::default();

        assert_eq!(
            state.handle_key_event(true, CONTROL_LEFT_KEYSYM),
            Some(HoldSignal::Start)
        );
        assert_eq!(state.handle_key_event(true, 97), Some(HoldSignal::Stop));
        assert_eq!(state.handle_key_event(true, CONTROL_RIGHT_KEYSYM), None);
        assert_eq!(state.handle_key_event(false, CONTROL_LEFT_KEYSYM), None);
        assert_eq!(state.handle_key_event(false, CONTROL_RIGHT_KEYSYM), None);
        assert_eq!(
            state.handle_key_event(true, CONTROL_RIGHT_KEYSYM),
            Some(HoldSignal::Start)
        );
    }
}

#[cfg(test)]
mod friendly_insert_validation {
    use super::*;

    #[test]
    fn friendly_insert_rejects_non_text_editor_targets() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.Calculator".into(),
                is_editable: true,
                supports_editable_text: true,
                supports_caret: true,
            },
            &FriendlyInsertPolicy {
                target_application_id: "org.gnome.TextEditor",
            },
        )
        .unwrap_err();

        assert_eq!(error.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert_eq!(
            error.reason,
            FriendlyInsertError::UnsupportedApplication {
                expected_application_id: "org.gnome.TextEditor".into(),
                actual_application_id: "org.gnome.Calculator".into(),
            }
        );
    }

    #[test]
    fn friendly_insert_rejects_non_editable_targets() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: false,
                supports_editable_text: true,
                supports_caret: true,
            },
            &FriendlyInsertPolicy {
                target_application_id: "org.gnome.TextEditor",
            },
        )
        .unwrap_err();

        assert_eq!(error.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert_eq!(error.reason, FriendlyInsertError::TargetNotEditable);
    }

    #[test]
    fn friendly_insert_rejects_targets_without_editable_text() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: true,
                supports_editable_text: false,
                supports_caret: true,
            },
            &FriendlyInsertPolicy {
                target_application_id: "org.gnome.TextEditor",
            },
        )
        .unwrap_err();

        assert_eq!(error.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert_eq!(error.reason, FriendlyInsertError::MissingEditableText);
    }

    #[test]
    fn friendly_insert_rejects_targets_without_caret_surface() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: true,
                supports_editable_text: true,
                supports_caret: false,
            },
            &FriendlyInsertPolicy {
                target_application_id: "org.gnome.TextEditor",
            },
        )
        .unwrap_err();

        assert_eq!(error.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert_eq!(error.reason, FriendlyInsertError::MissingCaretSurface);
    }

    #[test]
    fn friendly_insert_reports_stable_backend_name() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: true,
                supports_editable_text: true,
                supports_caret: true,
            },
            &FriendlyInsertPolicy {
                target_application_id: "org.gnome.TextEditor",
            },
        )
        .unwrap();

        assert_eq!(selection.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert_eq!(selection.target_application_id, "org.gnome.TextEditor");
    }
}
