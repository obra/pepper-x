use crate::service::PepperXService;
use gtk::prelude::*;
use std::ffi::{c_char, c_void, CStr, CString};
use std::fmt;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::Duration;

const CONTROL_LEFT_KEYSYM: u32 = 65_507;
const CONTROL_RIGHT_KEYSYM: u32 = 65_508;
const V_KEYSYM: u32 = b'v' as u32;
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(75);

pub const FRIENDLY_INSERT_BACKEND_NAME: &str = "atspi-editable-text";
pub const STRING_INJECTION_BACKEND_NAME: &str = "atspi-key-string";
pub const CLIPBOARD_PASTE_BACKEND_NAME: &str = "clipboard-paste";
pub const UINPUT_TEXT_BACKEND_NAME: &str = "uinput-text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FriendlyInsertBackend {
    EditableText,
    StringInjection,
    ClipboardPaste,
    UinputText,
}

impl FriendlyInsertBackend {
    fn backend_name(self) -> &'static str {
        match self {
            Self::EditableText => FRIENDLY_INSERT_BACKEND_NAME,
            Self::StringInjection => STRING_INJECTION_BACKEND_NAME,
            Self::ClipboardPaste => CLIPBOARD_PASTE_BACKEND_NAME,
            Self::UinputText => UINPUT_TEXT_BACKEND_NAME,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FriendlyInsertPolicy {
    pub target_application_id: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FriendlyInsertTargetClass {
    TextEditor,
    BrowserTextarea,
    Terminal,
    Hostile,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendlyFocusedTarget {
    pub application_id: String,
    pub is_editable: bool,
    pub supports_text: bool,
    pub supports_editable_text: bool,
    pub supports_caret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendlyInsertSelection {
    pub backend_name: &'static str,
    pub target_application_id: String,
    pub target_class: &'static str,
    pub attempted_backends: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendlyInsertOutcome {
    pub selection: FriendlyInsertSelection,
    pub target_application_name: String,
    pub target_class: String,
    pub caret_offset: i32,
    pub before_text: String,
    pub after_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendlyInsertFailure {
    pub backend_name: &'static str,
    pub reason: FriendlyInsertError,
    pub target_application_name: Option<String>,
    pub target_class: Option<&'static str>,
    pub attempted_backends: Vec<&'static str>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FriendlyInsertRunError {
    InvalidInsertText,
    InitializationFailed(i32),
    MissingFocusedTarget,
    UnsupportedTarget(FriendlyInsertFailure),
    SelectedBackendFailure {
        selection: FriendlyInsertSelection,
        target_application_name: String,
        reason: Box<FriendlyInsertRunError>,
    },
    Access(String),
    ReadbackMismatch,
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

impl fmt::Display for FriendlyInsertRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInsertText => {
                f.write_str("friendly insertion text contains an unsupported NUL byte")
            }
            Self::InitializationFailed(code) => {
                write!(f, "AT-SPI initialization failed with code {code}")
            }
            Self::MissingFocusedTarget => {
                f.write_str("friendly insertion could not find a focused target")
            }
            Self::UnsupportedTarget(error) => write!(f, "{error}"),
            Self::SelectedBackendFailure { reason, .. } => write!(f, "{reason}"),
            Self::Access(message) => f.write_str(message),
            Self::ReadbackMismatch => {
                f.write_str("friendly insertion readback did not match the requested text")
            }
        }
    }
}

impl FriendlyInsertRunError {
    pub fn selected_backend(&self) -> Option<&FriendlyInsertSelection> {
        match self {
            Self::SelectedBackendFailure { selection, .. } => Some(selection),
            _ => None,
        }
    }

    pub fn target_application_name(&self) -> Option<&str> {
        match self {
            Self::SelectedBackendFailure {
                target_application_name,
                ..
            } => Some(target_application_name),
            Self::UnsupportedTarget(error) => error.target_application_name.as_deref(),
            _ => None,
        }
    }

    pub fn target_class(&self) -> Option<&'static str> {
        match self {
            Self::SelectedBackendFailure { selection, .. } => Some(selection.target_class),
            Self::UnsupportedTarget(error) => error.target_class,
            _ => None,
        }
    }

    pub fn attempted_backends(&self) -> &[&'static str] {
        match self {
            Self::SelectedBackendFailure { selection, .. } => &selection.attempted_backends,
            Self::UnsupportedTarget(error) => &error.attempted_backends,
            _ => &[],
        }
    }
}

impl FriendlyInsertFailure {
    fn with_target_application_name(mut self, target_application_name: impl Into<String>) -> Self {
        self.target_application_name = Some(target_application_name.into());
        self
    }
}

fn friendly_insert_target_class_from_application_id(
    application_id: &str,
) -> FriendlyInsertTargetClass {
    match application_id {
        "org.gnome.TextEditor" | "gnome-text-editor" => FriendlyInsertTargetClass::TextEditor,
        "browser-textarea"
        | "firefox"
        | "org.mozilla.firefox"
        | "chromium"
        | "chromium-browser"
        | "google-chrome"
        | "com.google.Chrome"
        | "brave-browser"
        | "com.brave.Browser"
        | "microsoft-edge"
        | "com.microsoft.Edge"
        | "vivaldi"
        | "com.vivaldi.Vivaldi" => FriendlyInsertTargetClass::BrowserTextarea,
        "ghostty" | "xterm" | "gnome-terminal" | "gnome-terminal-server" => {
            FriendlyInsertTargetClass::Terminal
        }
        "wine" | "wine64-preloader" => FriendlyInsertTargetClass::Hostile,
        _ => FriendlyInsertTargetClass::Unsupported,
    }
}

fn friendly_insert_target_class_name(target_class: FriendlyInsertTargetClass) -> &'static str {
    match target_class {
        FriendlyInsertTargetClass::TextEditor => "text-editor",
        FriendlyInsertTargetClass::BrowserTextarea => "browser-textarea",
        FriendlyInsertTargetClass::Terminal => "terminal",
        FriendlyInsertTargetClass::Hostile => "hostile",
        FriendlyInsertTargetClass::Unsupported => "unsupported",
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
    let expected_target_class =
        friendly_insert_target_class_from_application_id(policy.target_application_id);
    let actual_target_class =
        friendly_insert_target_class_from_application_id(&target.application_id);
    let target_class = friendly_insert_target_class_name(actual_target_class);
    let fallback_chain = fallback_chain_for_target_class(actual_target_class);
    let attempted_backends = fallback_chain
        .iter()
        .map(|backend| backend.backend_name())
        .collect::<Vec<_>>();

    if expected_target_class == FriendlyInsertTargetClass::Unsupported
        || actual_target_class == FriendlyInsertTargetClass::Unsupported
        || expected_target_class != actual_target_class
    {
        return Err(FriendlyInsertFailure {
            backend_name: FRIENDLY_INSERT_BACKEND_NAME,
            reason: FriendlyInsertError::UnsupportedApplication {
                expected_application_id: policy.target_application_id.into(),
                actual_application_id: target.application_id.clone(),
            },
            target_application_name: None,
            target_class: Some(target_class),
            attempted_backends: Vec::new(),
        });
    }
    let selected_backend = fallback_chain
        .iter()
        .copied()
        .find(|backend| backend_matches_target(*backend, actual_target_class, target))
        .ok_or_else(|| {
            fallback_selection_failure(target, target_class, attempted_backends.clone())
        })?;
    let attempted_backends = fallback_chain
        .iter()
        .copied()
        .take_while(|backend| *backend != selected_backend)
        .chain(std::iter::once(selected_backend))
        .map(FriendlyInsertBackend::backend_name)
        .collect::<Vec<_>>();

    Ok(FriendlyInsertSelection {
        backend_name: selected_backend.backend_name(),
        target_application_id: target.application_id.clone(),
        target_class,
        attempted_backends,
    })
}

pub fn insert_text_into_friendly_target(
    text: &str,
    policy: &FriendlyInsertPolicy,
) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError> {
    let insert_text = CString::new(text).map_err(|_| FriendlyInsertRunError::InvalidInsertText)?;
    let insert_length = i32::try_from(text.chars().count())
        .map_err(|_| FriendlyInsertRunError::InvalidInsertText)?;

    unsafe {
        let init_result = ffi::atspi_init();
        if init_result < 0 {
            return Err(FriendlyInsertRunError::InitializationFailed(init_result));
        }
    }

    let target = focused_friendly_target(policy)?;

    match target.selection.backend_name {
        FRIENDLY_INSERT_BACKEND_NAME => {
            let editable_text = target
                .editable_text
                .as_ref()
                .expect("editable-text backend should have EditableText");
            let caret_offset = target
                .caret_offset
                .expect("editable-text backend should have a caret offset");
            let before_text = target
                .before_text
                .as_ref()
                .expect("editable-text backend should have a text snapshot");
            let text_iface = target
                .text
                .as_ref()
                .expect("editable-text backend should have Text support");

            unsafe {
                editable_text_insert_text(
                    editable_text.as_ptr(),
                    caret_offset,
                    insert_text.as_ptr(),
                    insert_length,
                )
                .map_err(|error| {
                    with_selected_backend_failure(
                        &target.selection,
                        &target.application_name,
                        error,
                    )
                })?;
            }

            let after_text = unsafe { text_contents(text_iface.as_ptr()) }.map_err(|error| {
                with_selected_backend_failure(&target.selection, &target.application_name, error)
            })?;
            let expected_after_text = apply_insert_at_char_offset(
                before_text,
                text,
                usize::try_from(caret_offset).map_err(|_| {
                    with_selected_backend_failure(
                        &target.selection,
                        &target.application_name,
                        FriendlyInsertRunError::ReadbackMismatch,
                    )
                })?,
            )
            .ok_or_else(|| {
                with_selected_backend_failure(
                    &target.selection,
                    &target.application_name,
                    FriendlyInsertRunError::ReadbackMismatch,
                )
            })?;

            if after_text != expected_after_text {
                return Err(with_selected_backend_failure(
                    &target.selection,
                    &target.application_name,
                    FriendlyInsertRunError::ReadbackMismatch,
                ));
            }

            Ok(FriendlyInsertOutcome {
                selection: target.selection,
                target_application_name: target.application_name,
                target_class: target.target_class.into(),
                caret_offset,
                before_text: before_text.clone(),
                after_text,
            })
        }
        STRING_INJECTION_BACKEND_NAME => {
            unsafe {
                generate_keyboard_string(insert_text.as_ptr()).map_err(|error| {
                    with_selected_backend_failure(
                        &target.selection,
                        &target.application_name,
                        error,
                    )
                })?;
            }

            Ok(FriendlyInsertOutcome {
                selection: target.selection,
                target_application_name: target.application_name,
                target_class: target.target_class.into(),
                caret_offset: -1,
                before_text: String::new(),
                after_text: String::new(),
            })
        }
        CLIPBOARD_PASTE_BACKEND_NAME => {
            let mut adapter = clipboard_paste_adapter().map_err(|error| {
                with_selected_backend_failure(&target.selection, &target.application_name, error)
            })?;

            run_clipboard_paste_with_adapter(
                text,
                &target.selection,
                &target.application_name,
                target.target_class,
                &mut adapter,
            )
            .map_err(|error| {
                with_selected_backend_failure(&target.selection, &target.application_name, error)
            })
        }
        _ => Err(FriendlyInsertRunError::Access(format!(
            "friendly insertion backend {} is not implemented",
            target.selection.backend_name
        ))),
    }
}

fn run_clipboard_paste_with_adapter<A: ClipboardPasteAdapter>(
    text: &str,
    selection: &FriendlyInsertSelection,
    target_application_name: &str,
    target_class: &'static str,
    adapter: &mut A,
) -> Result<FriendlyInsertOutcome, FriendlyInsertRunError> {
    let snapshot = adapter.snapshot()?;
    adapter.set_text(text)?;

    let paste_result = adapter.paste();
    let restore_result = adapter.restore(snapshot);

    if let Err(error) = restore_result {
        return Err(error);
    }

    paste_result?;

    Ok(FriendlyInsertOutcome {
        selection: selection.clone(),
        target_application_name: target_application_name.into(),
        target_class: target_class.into(),
        caret_offset: -1,
        before_text: String::new(),
        after_text: String::new(),
    })
}

fn clipboard_paste_adapter() -> Result<GdkClipboardPasteAdapter, FriendlyInsertRunError> {
    if !gtk::is_initialized_main_thread() {
        gtk::init().map_err(|error| {
            FriendlyInsertRunError::Access(format!(
                "failed to initialize GTK clipboard access: {error}"
            ))
        })?;
    }

    let display = gtk::gdk::Display::default().ok_or_else(|| {
        FriendlyInsertRunError::Access(
            "GTK clipboard mediation requires a live default display".into(),
        )
    })?;

    Ok(GdkClipboardPasteAdapter {
        clipboard: display.clipboard(),
    })
}

fn control_bit(keysym: u32) -> Option<u8> {
    match keysym {
        CONTROL_LEFT_KEYSYM => Some(0b01),
        CONTROL_RIGHT_KEYSYM => Some(0b10),
        _ => None,
    }
}

fn fallback_chain_for_target_class(
    target_class: FriendlyInsertTargetClass,
) -> &'static [FriendlyInsertBackend] {
    match target_class {
        FriendlyInsertTargetClass::TextEditor => &[FriendlyInsertBackend::EditableText],
        FriendlyInsertTargetClass::BrowserTextarea => &[
            FriendlyInsertBackend::EditableText,
            FriendlyInsertBackend::ClipboardPaste,
        ],
        FriendlyInsertTargetClass::Terminal => &[
            FriendlyInsertBackend::EditableText,
            FriendlyInsertBackend::StringInjection,
            FriendlyInsertBackend::ClipboardPaste,
        ],
        FriendlyInsertTargetClass::Hostile => &[
            FriendlyInsertBackend::EditableText,
            FriendlyInsertBackend::StringInjection,
            FriendlyInsertBackend::ClipboardPaste,
            FriendlyInsertBackend::UinputText,
        ],
        FriendlyInsertTargetClass::Unsupported => &[],
    }
}

fn backend_matches_target(
    backend: FriendlyInsertBackend,
    target_class: FriendlyInsertTargetClass,
    target: &FriendlyFocusedTarget,
) -> bool {
    match backend {
        FriendlyInsertBackend::EditableText => {
            target.is_editable && target.supports_editable_text && target.supports_caret
        }
        FriendlyInsertBackend::StringInjection => {
            target_class == FriendlyInsertTargetClass::Terminal && target.supports_text
        }
        FriendlyInsertBackend::ClipboardPaste => {
            target.is_editable
                && matches!(
                    target_class,
                    FriendlyInsertTargetClass::BrowserTextarea
                        | FriendlyInsertTargetClass::Terminal
                        | FriendlyInsertTargetClass::Hostile
                )
        }
        FriendlyInsertBackend::UinputText => target_class == FriendlyInsertTargetClass::Hostile,
    }
}

fn fallback_selection_failure(
    target: &FriendlyFocusedTarget,
    target_class: &'static str,
    attempted_backends: Vec<&'static str>,
) -> FriendlyInsertFailure {
    let reason = if !target.is_editable {
        FriendlyInsertError::TargetNotEditable
    } else if !target.supports_editable_text {
        FriendlyInsertError::MissingEditableText
    } else if !target.supports_caret {
        FriendlyInsertError::MissingCaretSurface
    } else {
        FriendlyInsertError::MissingEditableText
    };

    FriendlyInsertFailure {
        backend_name: FRIENDLY_INSERT_BACKEND_NAME,
        reason,
        target_application_name: None,
        target_class: Some(target_class),
        attempted_backends,
    }
}

fn with_selected_backend_failure(
    selection: &FriendlyInsertSelection,
    target_application_name: &str,
    error: FriendlyInsertRunError,
) -> FriendlyInsertRunError {
    match error {
        FriendlyInsertRunError::SelectedBackendFailure { .. } => error,
        _ => FriendlyInsertRunError::SelectedBackendFailure {
            selection: selection.clone(),
            target_application_name: target_application_name.into(),
            reason: Box::new(error),
        },
    }
}

fn ensure_runtime_supported_backend(
    selection: &FriendlyInsertSelection,
    target_application_name: &str,
) -> Result<(), FriendlyInsertRunError> {
    if matches!(
        selection.backend_name,
        FRIENDLY_INSERT_BACKEND_NAME | STRING_INJECTION_BACKEND_NAME | CLIPBOARD_PASTE_BACKEND_NAME
    ) {
        return Ok(());
    }

    Err(FriendlyInsertRunError::SelectedBackendFailure {
        selection: selection.clone(),
        target_application_name: target_application_name.into(),
        reason: Box::new(FriendlyInsertRunError::Access(format!(
            "friendly insertion backend {} is not implemented yet",
            selection.backend_name
        ))),
    })
}

struct FocusedFriendlyTarget {
    selection: FriendlyInsertSelection,
    application_name: String,
    target_class: &'static str,
    text: Option<OwnedGObject<ffi::AtspiText>>,
    editable_text: Option<OwnedGObject<ffi::AtspiEditableText>>,
    before_text: Option<String>,
    caret_offset: Option<i32>,
}

trait ClipboardPasteAdapter {
    type Snapshot;

    fn snapshot(&mut self) -> Result<Option<Self::Snapshot>, FriendlyInsertRunError>;
    fn set_text(&mut self, text: &str) -> Result<(), FriendlyInsertRunError>;
    fn paste(&mut self) -> Result<(), FriendlyInsertRunError>;
    fn restore(&mut self, snapshot: Option<Self::Snapshot>) -> Result<(), FriendlyInsertRunError>;
}

struct GdkClipboardPasteAdapter {
    clipboard: gtk::gdk::Clipboard,
}

impl ClipboardPasteAdapter for GdkClipboardPasteAdapter {
    type Snapshot = gtk::gdk::ContentProvider;

    fn snapshot(&mut self) -> Result<Option<Self::Snapshot>, FriendlyInsertRunError> {
        Ok(self.clipboard.content())
    }

    fn set_text(&mut self, text: &str) -> Result<(), FriendlyInsertRunError> {
        self.clipboard.set_text(text);
        Ok(())
    }

    fn paste(&mut self) -> Result<(), FriendlyInsertRunError> {
        unsafe {
            generate_keyboard_key_press(CONTROL_LEFT_KEYSYM.into())?;
            generate_keyboard_key_press_release(V_KEYSYM.into())?;
            generate_keyboard_key_release(CONTROL_LEFT_KEYSYM.into())?;
        }

        // Let the focused target request clipboard contents before restoring ownership.
        std::thread::sleep(CLIPBOARD_RESTORE_DELAY);
        Ok(())
    }

    fn restore(&mut self, snapshot: Option<Self::Snapshot>) -> Result<(), FriendlyInsertRunError> {
        self.clipboard
            .set_content(snapshot.as_ref())
            .map_err(|error| {
                FriendlyInsertRunError::Access(format!(
                    "failed to restore clipboard contents: {error}"
                ))
            })
    }
}

struct OwnedGObject<T> {
    ptr: NonNull<T>,
}

impl<T> OwnedGObject<T> {
    unsafe fn new(ptr: *mut T) -> Option<Self> {
        Some(Self {
            ptr: NonNull::new(ptr)?,
        })
    }

    fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }

    unsafe fn clone_ref(&self) -> Self {
        Self {
            ptr: NonNull::new(glib::gobject_ffi::g_object_ref(self.ptr.as_ptr().cast()).cast())
                .expect("g_object_ref returned null"),
        }
    }
}

impl<T> Drop for OwnedGObject<T> {
    fn drop(&mut self) {
        unsafe {
            glib::gobject_ffi::g_object_unref(self.ptr.as_ptr().cast());
        }
    }
}

fn focused_friendly_target(
    policy: &FriendlyInsertPolicy,
) -> Result<FocusedFriendlyTarget, FriendlyInsertRunError> {
    let focused = unsafe { find_focused_accessible()? };
    let application = unsafe { accessible_application(focused.as_ptr())? };
    let application_name = unsafe { accessible_name(application.as_ptr())? };
    let application_id = unsafe { friendly_application_id_from_process_id(focused.as_ptr())? };
    let actual_target_class = friendly_insert_target_class_from_application_id(&application_id);
    let is_editable = unsafe {
        accessible_has_state(focused.as_ptr(), ffi::ATSPI_STATE_EDITABLE)
            .map_err(FriendlyInsertRunError::Access)?
    };
    let text = unsafe { OwnedGObject::new(ffi::atspi_accessible_get_text_iface(focused.as_ptr())) };
    let editable_text = unsafe {
        OwnedGObject::new(ffi::atspi_accessible_get_editable_text_iface(
            focused.as_ptr(),
        ))
    };
    let caret_offset = match actual_target_class {
        FriendlyInsertTargetClass::TextEditor | FriendlyInsertTargetClass::BrowserTextarea => {
            match text.as_ref() {
                Some(text) => unsafe { text_caret_offset(text.as_ptr())? },
                None => -1,
            }
        }
        FriendlyInsertTargetClass::Terminal
        | FriendlyInsertTargetClass::Hostile
        | FriendlyInsertTargetClass::Unsupported => -1,
    };
    let target = FriendlyFocusedTarget {
        application_id,
        is_editable,
        supports_text: text.is_some(),
        supports_editable_text: editable_text.is_some(),
        supports_caret: caret_offset >= 0,
    };
    let target_class = friendly_insert_target_class_name(actual_target_class);
    let selection = select_friendly_insert_backend(&target, policy).map_err(|error| {
        FriendlyInsertRunError::UnsupportedTarget(
            error.with_target_application_name(application_name.clone()),
        )
    })?;
    ensure_runtime_supported_backend(&selection, &application_name)?;
    if matches!(
        selection.backend_name,
        STRING_INJECTION_BACKEND_NAME | CLIPBOARD_PASTE_BACKEND_NAME
    ) {
        return Ok(FocusedFriendlyTarget {
            selection,
            application_name,
            target_class,
            text: None,
            editable_text: None,
            before_text: None,
            caret_offset: None,
        });
    }

    let text = text.ok_or_else(|| {
        with_selected_backend_failure(
            &selection,
            &application_name,
            FriendlyInsertRunError::Access(
                "friendly insertion target is missing Text support".into(),
            ),
        )
    })?;
    let editable_text = editable_text.ok_or_else(|| {
        with_selected_backend_failure(
            &selection,
            &application_name,
            FriendlyInsertRunError::Access(
                "friendly insertion target is missing EditableText support".into(),
            ),
        )
    })?;
    let before_text = unsafe { text_contents(text.as_ptr()) }
        .map_err(|error| with_selected_backend_failure(&selection, &application_name, error))?;

    Ok(FocusedFriendlyTarget {
        selection,
        application_name,
        target_class,
        text: Some(text),
        editable_text: Some(editable_text),
        before_text: Some(before_text),
        caret_offset: Some(caret_offset),
    })
}

fn friendly_application_id_from_executable_name(executable_name: &str) -> String {
    match executable_name {
        "gnome-text-editor" => "org.gnome.TextEditor".into(),
        "wine64-preloader" => "wine".into(),
        _ => executable_name.into(),
    }
}

unsafe fn friendly_application_id_from_process_id(
    accessible: *mut ffi::AtspiAccessible,
) -> Result<String, FriendlyInsertRunError> {
    let process_id = accessible_process_id(accessible)?;
    let executable_path =
        std::fs::read_link(format!("/proc/{process_id}/exe")).map_err(|error| {
            FriendlyInsertRunError::Access(format!(
                "failed to inspect focused target process {process_id}: {error}"
            ))
        })?;
    let executable_name = executable_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| {
            FriendlyInsertRunError::Access(format!(
                "focused target executable path has no file name: {}",
                executable_path.display()
            ))
        })?;

    Ok(friendly_application_id_from_executable_name(
        &executable_name,
    ))
}

fn apply_insert_at_char_offset(
    before_text: &str,
    inserted_text: &str,
    char_offset: usize,
) -> Option<String> {
    let byte_offset = byte_index_for_char_offset(before_text, char_offset)?;
    let mut after_text =
        String::with_capacity(before_text.len().saturating_add(inserted_text.len()));

    after_text.push_str(&before_text[..byte_offset]);
    after_text.push_str(inserted_text);
    after_text.push_str(&before_text[byte_offset..]);

    Some(after_text)
}

fn byte_index_for_char_offset(text: &str, char_offset: usize) -> Option<usize> {
    if char_offset == text.chars().count() {
        return Some(text.len());
    }

    text.char_indices()
        .nth(char_offset)
        .map(|(byte_offset, _)| byte_offset)
}

unsafe fn find_focused_accessible(
) -> Result<OwnedGObject<ffi::AtspiAccessible>, FriendlyInsertRunError> {
    let desktop_count = ffi::atspi_get_desktop_count();

    for desktop_index in 0..desktop_count {
        let Some(desktop) = OwnedGObject::new(ffi::atspi_get_desktop(desktop_index)) else {
            continue;
        };

        if let Some(focused) = find_focused_descendant(&desktop)? {
            return Ok(focused);
        }
    }

    Err(FriendlyInsertRunError::MissingFocusedTarget)
}

unsafe fn find_focused_descendant(
    accessible: &OwnedGObject<ffi::AtspiAccessible>,
) -> Result<Option<OwnedGObject<ffi::AtspiAccessible>>, FriendlyInsertRunError> {
    if accessible_has_state(accessible.as_ptr(), ffi::ATSPI_STATE_FOCUSED)
        .map_err(FriendlyInsertRunError::Access)?
    {
        return Ok(Some(accessible.clone_ref()));
    }

    let child_count = accessible_child_count(accessible.as_ptr())?;
    for child_index in 0..child_count {
        let Some(child) = accessible_child_at_index(accessible.as_ptr(), child_index)? else {
            continue;
        };

        if let Some(focused) = find_focused_descendant(&child)? {
            return Ok(Some(focused));
        }
    }

    Ok(None)
}

unsafe fn accessible_child_count(
    accessible: *mut ffi::AtspiAccessible,
) -> Result<i32, FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let child_count = ffi::atspi_accessible_get_child_count(accessible, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to enumerate accessibility children: {message}"
        )));
    }

    Ok(child_count)
}

unsafe fn accessible_child_at_index(
    accessible: *mut ffi::AtspiAccessible,
    child_index: i32,
) -> Result<Option<OwnedGObject<ffi::AtspiAccessible>>, FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let child = ffi::atspi_accessible_get_child_at_index(accessible, child_index, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to read accessibility child {child_index}: {message}"
        )));
    }

    Ok(OwnedGObject::new(child))
}

unsafe fn accessible_has_state(
    accessible: *mut ffi::AtspiAccessible,
    state: ffi::AtspiStateType,
) -> Result<bool, String> {
    let Some(state_set) = OwnedGObject::new(ffi::atspi_accessible_get_state_set(accessible)) else {
        return Err("failed to read accessibility state set".into());
    };

    Ok(ffi::atspi_state_set_contains(state_set.as_ptr(), state) != glib::ffi::GFALSE)
}

unsafe fn accessible_application(
    accessible: *mut ffi::AtspiAccessible,
) -> Result<OwnedGObject<ffi::AtspiAccessible>, FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let application = ffi::atspi_accessible_get_application(accessible, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to resolve accessibility application object: {message}"
        )));
    }

    OwnedGObject::new(application).ok_or_else(|| {
        FriendlyInsertRunError::Access(
            "focused accessibility target is missing an application object".into(),
        )
    })
}

unsafe fn accessible_name(
    accessible: *mut ffi::AtspiAccessible,
) -> Result<String, FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let raw_name = ffi::atspi_accessible_get_name(accessible, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to read accessibility object name: {message}"
        )));
    }

    Ok(take_glib_string(raw_name))
}

unsafe fn accessible_process_id(
    accessible: *mut ffi::AtspiAccessible,
) -> Result<u32, FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let process_id = ffi::atspi_accessible_get_process_id(accessible, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to read focused target process id: {message}"
        )));
    }

    Ok(process_id)
}

unsafe fn text_contents(text: *mut ffi::AtspiText) -> Result<String, FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let character_count = ffi::atspi_text_get_character_count(text, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to read accessibility text length: {message}"
        )));
    }

    let raw_text = ffi::atspi_text_get_text(text, 0, character_count, &mut error);
    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to read accessibility text: {message}"
        )));
    }

    Ok(take_glib_string(raw_text))
}

unsafe fn text_caret_offset(text: *mut ffi::AtspiText) -> Result<i32, FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let caret_offset = ffi::atspi_text_get_caret_offset(text, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to read accessibility caret offset: {message}"
        )));
    }

    Ok(caret_offset)
}

unsafe fn editable_text_insert_text(
    editable_text: *mut ffi::AtspiEditableText,
    position: i32,
    text: *const c_char,
    length: i32,
) -> Result<(), FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let inserted =
        ffi::atspi_editable_text_insert_text(editable_text, position, text, length, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to insert accessibility text: {message}"
        )));
    }

    if inserted == glib::ffi::GFALSE {
        return Err(FriendlyInsertRunError::Access(
            "AT-SPI editable text insertion returned FALSE".into(),
        ));
    }

    Ok(())
}

unsafe fn generate_keyboard_string(text: *const c_char) -> Result<(), FriendlyInsertRunError> {
    generate_keyboard_event(0, text, ffi::ATSPI_KEY_STRING)
}

unsafe fn generate_keyboard_key_press(
    keyval: glib::ffi::glong,
) -> Result<(), FriendlyInsertRunError> {
    generate_keyboard_event(keyval, std::ptr::null(), ffi::ATSPI_KEY_PRESS)
}

unsafe fn generate_keyboard_key_release(
    keyval: glib::ffi::glong,
) -> Result<(), FriendlyInsertRunError> {
    generate_keyboard_event(keyval, std::ptr::null(), ffi::ATSPI_KEY_RELEASE)
}

unsafe fn generate_keyboard_key_press_release(
    keyval: glib::ffi::glong,
) -> Result<(), FriendlyInsertRunError> {
    generate_keyboard_event(keyval, std::ptr::null(), ffi::ATSPI_KEY_PRESSRELEASE)
}

unsafe fn generate_keyboard_event(
    keyval: glib::ffi::glong,
    keystring: *const c_char,
    synth_type: ffi::AtspiKeySynthType,
) -> Result<(), FriendlyInsertRunError> {
    let mut error = std::ptr::null_mut();
    let generated = ffi::atspi_generate_keyboard_event(keyval, keystring, synth_type, &mut error);

    if let Some(message) = take_glib_error_message(error) {
        return Err(FriendlyInsertRunError::Access(format!(
            "failed to generate AT-SPI keyboard event: {message}"
        )));
    }

    if generated == glib::ffi::GFALSE {
        return Err(FriendlyInsertRunError::Access(
            "AT-SPI keyboard event generation returned FALSE".into(),
        ));
    }

    Ok(())
}

unsafe fn take_glib_error_message(error: *mut glib::ffi::GError) -> Option<String> {
    let error = NonNull::new(error)?;
    let message = if error.as_ref().message.is_null() {
        "unknown GLib error".into()
    } else {
        CStr::from_ptr(error.as_ref().message)
            .to_string_lossy()
            .into_owned()
    };

    glib::ffi::g_error_free(error.as_ptr());
    Some(message)
}

unsafe fn take_glib_string(raw: *mut c_char) -> String {
    let Some(raw) = NonNull::new(raw) else {
        return String::new();
    };
    let text = CStr::from_ptr(raw.as_ptr()).to_string_lossy().into_owned();
    glib::ffi::g_free(raw.as_ptr().cast());
    text
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
    use glib::ffi::{gboolean, gchar, gint, glong, gpointer, guint, GDestroyNotify};

    pub type AtspiStateType = gint;
    pub type AtspiKeySynthType = gint;

    #[repr(C)]
    pub struct AtspiDevice(c_void);

    #[repr(C)]
    pub struct AtspiAccessible(c_void);

    #[repr(C)]
    pub struct AtspiEditableText(c_void);

    #[repr(C)]
    pub struct AtspiText(c_void);

    #[repr(C)]
    pub struct AtspiStateSet(c_void);

    pub const ATSPI_STATE_EDITABLE: AtspiStateType = 7;
    pub const ATSPI_STATE_FOCUSED: AtspiStateType = 12;
    pub const ATSPI_KEY_PRESS: AtspiKeySynthType = 0;
    pub const ATSPI_KEY_RELEASE: AtspiKeySynthType = 1;
    pub const ATSPI_KEY_PRESSRELEASE: AtspiKeySynthType = 2;
    pub const ATSPI_KEY_STRING: AtspiKeySynthType = 4;

    #[link(name = "atspi")]
    unsafe extern "C" {
        pub fn atspi_init() -> gint;
        pub fn atspi_get_desktop_count() -> gint;
        pub fn atspi_get_desktop(i: gint) -> *mut AtspiAccessible;
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
        pub fn atspi_generate_keyboard_event(
            keyval: glong,
            keystring: *const gchar,
            synth_type: AtspiKeySynthType,
            error: *mut *mut glib::ffi::GError,
        ) -> gboolean;
        pub fn atspi_accessible_get_name(
            obj: *mut AtspiAccessible,
            error: *mut *mut glib::ffi::GError,
        ) -> *mut gchar;
        pub fn atspi_accessible_get_child_count(
            obj: *mut AtspiAccessible,
            error: *mut *mut glib::ffi::GError,
        ) -> gint;
        pub fn atspi_accessible_get_child_at_index(
            obj: *mut AtspiAccessible,
            child_index: gint,
            error: *mut *mut glib::ffi::GError,
        ) -> *mut AtspiAccessible;
        pub fn atspi_accessible_get_state_set(obj: *mut AtspiAccessible) -> *mut AtspiStateSet;
        pub fn atspi_accessible_get_application(
            obj: *mut AtspiAccessible,
            error: *mut *mut glib::ffi::GError,
        ) -> *mut AtspiAccessible;
        pub fn atspi_accessible_get_editable_text_iface(
            obj: *mut AtspiAccessible,
        ) -> *mut AtspiEditableText;
        pub fn atspi_accessible_get_text_iface(obj: *mut AtspiAccessible) -> *mut AtspiText;
        pub fn atspi_accessible_get_process_id(
            accessible: *mut AtspiAccessible,
            error: *mut *mut glib::ffi::GError,
        ) -> guint;
        pub fn atspi_state_set_contains(set: *mut AtspiStateSet, state: AtspiStateType)
            -> gboolean;
        pub fn atspi_text_get_character_count(
            obj: *mut AtspiText,
            error: *mut *mut glib::ffi::GError,
        ) -> gint;
        pub fn atspi_text_get_text(
            obj: *mut AtspiText,
            start_offset: gint,
            end_offset: gint,
            error: *mut *mut glib::ffi::GError,
        ) -> *mut gchar;
        pub fn atspi_text_get_caret_offset(
            obj: *mut AtspiText,
            error: *mut *mut glib::ffi::GError,
        ) -> gint;
        pub fn atspi_editable_text_insert_text(
            obj: *mut AtspiEditableText,
            position: gint,
            text: *const gchar,
            length: gint,
            error: *mut *mut glib::ffi::GError,
        ) -> gboolean;
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
mod accessible_insert_validation {
    use super::*;

    #[test]
    fn accessible_insert_rejects_unsupported_targets() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.Calculator".into(),
                is_editable: true,
                supports_text: true,
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
        assert_eq!(error.target_class, Some("unsupported"));
        assert!(error.attempted_backends.is_empty());
    }

    #[test]
    fn accessible_insert_rejects_non_editable_targets() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: false,
                supports_text: true,
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
        assert_eq!(error.target_class, Some("text-editor"));
        assert_eq!(error.attempted_backends, vec![FRIENDLY_INSERT_BACKEND_NAME]);
    }

    #[test]
    fn accessible_insert_rejects_targets_without_editable_text() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: true,
                supports_text: true,
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
    fn accessible_insert_rejects_targets_without_caret_surface() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: true,
                supports_text: true,
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
    fn accessible_insert_reports_stable_backend_name() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: true,
                supports_text: true,
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

    #[test]
    fn accessible_insert_accepts_browser_textarea_targets() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "firefox".into(),
                is_editable: true,
                supports_text: true,
                supports_editable_text: true,
                supports_caret: true,
            },
            &FriendlyInsertPolicy {
                target_application_id: "browser-textarea",
            },
        )
        .unwrap();

        assert_eq!(selection.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert_eq!(selection.target_application_id, "firefox");
    }

    #[test]
    fn fallback_insert_prefers_semantic_insertion_for_text_editor_targets() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "org.gnome.TextEditor".into(),
                is_editable: true,
                supports_text: true,
                supports_editable_text: true,
                supports_caret: true,
            },
            &FriendlyInsertPolicy {
                target_application_id: "org.gnome.TextEditor",
            },
        )
        .unwrap();

        let snapshot = format!("{selection:?}");

        assert!(snapshot.contains("backend_name: \"atspi-editable-text\""));
        assert!(snapshot.contains("target_class: \"text-editor\""));
        assert!(snapshot.contains("attempted_backends: [\"atspi-editable-text\"]"));
    }

    #[test]
    fn fallback_insert_promotes_terminal_targets_to_string_injection() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "ghostty".into(),
                is_editable: false,
                supports_text: true,
                supports_editable_text: false,
                supports_caret: false,
            },
            &FriendlyInsertPolicy {
                target_application_id: "ghostty",
            },
        )
        .unwrap();

        let snapshot = format!("{selection:?}");

        assert!(snapshot.contains("backend_name: \"atspi-key-string\""));
        assert!(snapshot.contains("target_class: \"terminal\""));
        assert!(snapshot
            .contains("attempted_backends: [\"atspi-editable-text\", \"atspi-key-string\"]"));
    }

    #[test]
    fn fallback_insert_selects_clipboard_after_text_paths_fail() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "firefox".into(),
                is_editable: true,
                supports_text: true,
                supports_editable_text: false,
                supports_caret: false,
            },
            &FriendlyInsertPolicy {
                target_application_id: "browser-textarea",
            },
        )
        .unwrap();

        let snapshot = format!("{selection:?}");

        assert!(snapshot.contains("backend_name: \"clipboard-paste\""));
        assert!(snapshot.contains("target_class: \"browser-textarea\""));
        assert!(
            snapshot.contains("attempted_backends: [\"atspi-editable-text\", \"clipboard-paste\"]")
        );
    }

    #[test]
    fn fallback_insert_keeps_uinput_last_for_hostile_targets() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "wine".into(),
                is_editable: false,
                supports_text: false,
                supports_editable_text: false,
                supports_caret: false,
            },
            &FriendlyInsertPolicy {
                target_application_id: "wine",
            },
        )
        .unwrap();

        let snapshot = format!("{selection:?}");

        assert!(snapshot.contains("backend_name: \"uinput-text\""));
        assert!(snapshot.contains("target_class: \"hostile\""));
        assert!(snapshot.contains(
            "attempted_backends: [\"atspi-editable-text\", \"atspi-key-string\", \"clipboard-paste\", \"uinput-text\"]"
        ));
    }

    #[test]
    fn fallback_insert_prefers_clipboard_before_uinput_for_editable_hostile_targets() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "wine".into(),
                is_editable: true,
                supports_text: false,
                supports_editable_text: false,
                supports_caret: false,
            },
            &FriendlyInsertPolicy {
                target_application_id: "wine",
            },
        )
        .unwrap();

        let snapshot = format!("{selection:?}");

        assert!(snapshot.contains("backend_name: \"clipboard-paste\""));
        assert!(snapshot.contains("target_class: \"hostile\""));
        assert!(snapshot.contains(
            "attempted_backends: [\"atspi-editable-text\", \"atspi-key-string\", \"clipboard-paste\"]"
        ));
    }

    #[test]
    fn fallback_insert_rejects_terminal_targets_without_text_surface() {
        let error = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "gnome-terminal-server".into(),
                is_editable: false,
                supports_text: false,
                supports_editable_text: false,
                supports_caret: false,
            },
            &FriendlyInsertPolicy {
                target_application_id: "gnome-terminal-server",
            },
        )
        .unwrap_err();

        assert_eq!(error.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert_eq!(error.reason, FriendlyInsertError::TargetNotEditable);
        assert_eq!(error.target_class, Some("terminal"));
        assert_eq!(
            error.attempted_backends,
            vec![
                FRIENDLY_INSERT_BACKEND_NAME,
                STRING_INJECTION_BACKEND_NAME,
                CLIPBOARD_PASTE_BACKEND_NAME,
            ]
        );
    }
}

#[cfg(test)]
mod accessible_insert_runtime_helpers {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeClipboardPasteAdapter {
        current_text: Option<String>,
        calls: Vec<String>,
    }

    impl FakeClipboardPasteAdapter {
        fn with_text(text: &str) -> Self {
            Self {
                current_text: Some(text.into()),
                calls: Vec::new(),
            }
        }
    }

    impl ClipboardPasteAdapter for FakeClipboardPasteAdapter {
        type Snapshot = String;

        fn snapshot(&mut self) -> Result<Option<Self::Snapshot>, FriendlyInsertRunError> {
            self.calls.push("snapshot".into());
            Ok(self.current_text.clone())
        }

        fn set_text(&mut self, text: &str) -> Result<(), FriendlyInsertRunError> {
            self.calls.push(format!("set_text:{text}"));
            self.current_text = Some(text.into());
            Ok(())
        }

        fn paste(&mut self) -> Result<(), FriendlyInsertRunError> {
            self.calls.push("paste".into());
            Ok(())
        }

        fn restore(
            &mut self,
            snapshot: Option<Self::Snapshot>,
        ) -> Result<(), FriendlyInsertRunError> {
            match snapshot {
                Some(snapshot) => {
                    self.calls.push(format!("restore:{snapshot}"));
                    self.current_text = Some(snapshot);
                }
                None => {
                    self.calls.push("restore:none".into());
                    self.current_text = None;
                }
            }

            Ok(())
        }
    }

    #[test]
    fn accessible_insert_runtime_helpers_map_text_editor_executable() {
        assert_eq!(
            friendly_application_id_from_executable_name("gnome-text-editor"),
            "org.gnome.TextEditor"
        );
    }

    #[test]
    fn accessible_insert_runtime_helpers_classify_text_editor_targets() {
        assert_eq!(
            friendly_insert_target_class_from_application_id("org.gnome.TextEditor"),
            FriendlyInsertTargetClass::TextEditor
        );
        assert_eq!(
            friendly_insert_target_class_from_application_id("gnome-text-editor"),
            FriendlyInsertTargetClass::TextEditor
        );
    }

    #[test]
    fn accessible_insert_runtime_helpers_classify_browser_textarea_targets() {
        assert_eq!(
            friendly_insert_target_class_from_application_id("browser-textarea"),
            FriendlyInsertTargetClass::BrowserTextarea
        );
        assert_eq!(
            friendly_insert_target_class_from_application_id("firefox"),
            FriendlyInsertTargetClass::BrowserTextarea
        );
    }

    #[test]
    fn accessible_insert_runtime_helpers_classify_terminal_targets() {
        assert_eq!(
            friendly_insert_target_class_from_application_id("gnome-terminal-server"),
            FriendlyInsertTargetClass::Terminal
        );
        assert_eq!(
            friendly_insert_target_class_from_application_id("ghostty"),
            FriendlyInsertTargetClass::Terminal
        );
    }

    #[test]
    fn accessible_insert_runtime_helpers_preserve_unknown_executable_names() {
        assert_eq!(
            friendly_application_id_from_executable_name("ghostty"),
            "ghostty"
        );
    }

    #[test]
    fn accessible_insert_runtime_helpers_apply_insert_at_char_offset() {
        assert_eq!(
            apply_insert_at_char_offset("ab🙂d", "X", 3),
            Some("ab🙂Xd".into())
        );
    }

    #[test]
    fn clipboard_insert_accepts_selected_backend() {
        ensure_runtime_supported_backend(
            &FriendlyInsertSelection {
                backend_name: CLIPBOARD_PASTE_BACKEND_NAME,
                target_application_id: "firefox".into(),
                target_class: "browser-textarea",
                attempted_backends: vec![
                    FRIENDLY_INSERT_BACKEND_NAME,
                    CLIPBOARD_PASTE_BACKEND_NAME,
                ],
            },
            "Firefox",
        )
        .expect("clipboard backend should be supported");
    }

    #[test]
    fn clipboard_insert_restores_previous_clipboard_text_after_paste() {
        let mut adapter = FakeClipboardPasteAdapter::with_text("original clipboard");

        let outcome = run_clipboard_paste_with_adapter(
            "pepper x transcript",
            &FriendlyInsertSelection {
                backend_name: CLIPBOARD_PASTE_BACKEND_NAME,
                target_application_id: "firefox".into(),
                target_class: "browser-textarea",
                attempted_backends: vec![
                    FRIENDLY_INSERT_BACKEND_NAME,
                    CLIPBOARD_PASTE_BACKEND_NAME,
                ],
            },
            "Firefox",
            "browser-textarea",
            &mut adapter,
        )
        .expect("clipboard paste should succeed");

        assert_eq!(outcome.selection.backend_name, CLIPBOARD_PASTE_BACKEND_NAME);
        assert_eq!(outcome.target_application_name, "Firefox");
        assert_eq!(outcome.target_class, "browser-textarea");
        assert_eq!(outcome.caret_offset, -1);
        assert_eq!(adapter.current_text.as_deref(), Some("original clipboard"));
        assert_eq!(
            adapter.calls,
            vec![
                "snapshot".to_string(),
                "set_text:pepper x transcript".to_string(),
                "paste".to_string(),
                "restore:original clipboard".to_string(),
            ]
        );
    }
}

#[cfg(test)]
mod accessible_insert_live {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    #[ignore = "requires a live GNOME Wayland session with GNOME Text Editor focused"]
    fn accessible_insert_live_text_editor_round_trip() {
        let inserted_text = std::env::var("PEPPERX_FRIENDLY_INSERT_TEXT").unwrap_or_else(|_| {
            format!(
                " pepperx-smoke-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock before unix epoch")
                    .as_nanos()
            )
        });
        assert_accessible_insert_live_round_trip("org.gnome.TextEditor", &inserted_text);
    }

    #[test]
    #[ignore = "requires a live GNOME Wayland session with GNOME Text Editor focused"]
    fn accessible_insert_live_text_editor_contains_expected_text() {
        let expected_text = std::env::var("PEPPERX_FRIENDLY_EXPECTED_TEXT")
            .expect("PEPPERX_FRIENDLY_EXPECTED_TEXT must contain the cleaned transcript");

        assert_accessible_insert_live_contains_text("org.gnome.TextEditor", &expected_text);
    }

    #[test]
    #[ignore = "requires a live GNOME Wayland session with a browser textarea focused"]
    fn accessible_insert_live_browser_textarea_round_trip() {
        let inserted_text = std::env::var("PEPPERX_FRIENDLY_INSERT_TEXT").unwrap_or_else(|_| {
            format!(
                " pepperx-smoke-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock before unix epoch")
                    .as_nanos()
            )
        });
        assert_accessible_insert_live_round_trip("browser-textarea", &inserted_text);
    }

    #[test]
    #[ignore = "requires a live GNOME Wayland session with GNOME Terminal focused"]
    fn accessible_insert_live_terminal_round_trip() {
        let smoke_file = std::env::var_os("PEPPERX_TERMINAL_SMOKE_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "pepper-x-terminal-smoke-{}-{}.txt",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("clock before unix epoch")
                        .as_nanos()
                ))
            });
        let expected_marker =
            std::env::var("PEPPERX_TERMINAL_EXPECTED_MARKER").unwrap_or_else(|_| {
                format!(
                    "pepperx-terminal-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("clock before unix epoch")
                        .as_nanos()
                )
            });
        let inserted_text = std::env::var("PEPPERX_TERMINAL_INSERT_TEXT")
            .unwrap_or_else(|_| terminal_smoke_command(&smoke_file, &expected_marker));

        assert_accessible_insert_live_terminal_round_trip(
            "gnome-terminal-server",
            &inserted_text,
            &smoke_file,
            &expected_marker,
        );
    }

    fn assert_accessible_insert_live_round_trip(
        target_application_id: &'static str,
        inserted_text: &str,
    ) {
        let outcome = insert_text_into_friendly_target(
            inserted_text,
            &FriendlyInsertPolicy {
                target_application_id,
            },
        )
        .expect("friendly insertion should succeed");

        assert_eq!(outcome.selection.backend_name, FRIENDLY_INSERT_BACKEND_NAME);
        assert!(
            !outcome.target_application_name.is_empty(),
            "friendly insertion should report the target application name"
        );
        assert!(!outcome.target_class.is_empty());
        assert_eq!(
            outcome.after_text,
            apply_insert_at_char_offset(
                &outcome.before_text,
                inserted_text,
                usize::try_from(outcome.caret_offset).expect("caret offset should be non-negative")
            )
            .expect("expected inserted text snapshot")
        );
    }

    fn assert_accessible_insert_live_contains_text(
        target_application_id: &'static str,
        expected_text: &str,
    ) {
        let target = focused_friendly_target(&FriendlyInsertPolicy {
            target_application_id,
        })
        .expect("focused target should be inspectable");

        assert_eq!(
            target.selection.target_application_id,
            target_application_id
        );
        assert_eq!(target.target_class, "text-editor");

        let current_text = target
            .before_text
            .expect("focused target should expose current text contents");

        assert!(
            current_text.contains(expected_text),
            "focused target text should contain the expected cleaned transcript"
        );
    }

    fn assert_accessible_insert_live_terminal_round_trip(
        target_application_id: &'static str,
        inserted_text: &str,
        smoke_file: &Path,
        expected_marker: &str,
    ) {
        let _ = std::fs::remove_file(smoke_file);

        let outcome = insert_text_into_friendly_target(
            inserted_text,
            &FriendlyInsertPolicy {
                target_application_id,
            },
        )
        .expect("terminal insertion should succeed");

        assert_eq!(
            outcome.selection.backend_name,
            STRING_INJECTION_BACKEND_NAME
        );
        assert_eq!(
            wait_for_terminal_smoke_file(smoke_file),
            expected_marker,
            "terminal smoke command should persist the expected marker"
        );
    }

    fn terminal_smoke_command(smoke_file: &Path, expected_marker: &str) -> String {
        format!(
            "printf '%s' {} > {}\n",
            shell_single_quote(expected_marker),
            shell_single_quote(&smoke_file.to_string_lossy())
        )
    }

    fn shell_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }

    fn wait_for_terminal_smoke_file(smoke_file: &Path) -> String {
        for _ in 0..40 {
            if let Ok(contents) = std::fs::read_to_string(smoke_file) {
                return contents;
            }

            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        panic!(
            "terminal smoke file did not appear in time: {}",
            smoke_file.display()
        );
    }
}
