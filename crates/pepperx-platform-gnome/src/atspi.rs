use crate::service::PepperXService;
use std::ffi::{c_char, c_void, CStr, CString};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FriendlyInsertTargetClass {
    TextEditor,
    BrowserTextarea,
    Unsupported,
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
            Self::Access(message) => f.write_str(message),
            Self::ReadbackMismatch => {
                f.write_str("friendly insertion readback did not match the requested text")
            }
        }
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
        _ => FriendlyInsertTargetClass::Unsupported,
    }
}

fn friendly_insert_target_class_name(
    target_class: FriendlyInsertTargetClass,
) -> Option<&'static str> {
    match target_class {
        FriendlyInsertTargetClass::TextEditor => Some("text-editor"),
        FriendlyInsertTargetClass::BrowserTextarea => Some("browser-textarea"),
        FriendlyInsertTargetClass::Unsupported => None,
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

    unsafe {
        editable_text_insert_text(
            target.editable_text.as_ptr(),
            target.caret_offset,
            insert_text.as_ptr(),
            insert_length,
        )?;
    }

    let after_text = unsafe { text_contents(target.text.as_ptr())? };
    let expected_after_text = apply_insert_at_char_offset(
        &target.before_text,
        text,
        usize::try_from(target.caret_offset)
            .map_err(|_| FriendlyInsertRunError::ReadbackMismatch)?,
    )
    .ok_or(FriendlyInsertRunError::ReadbackMismatch)?;

    if after_text != expected_after_text {
        return Err(FriendlyInsertRunError::ReadbackMismatch);
    }

    Ok(FriendlyInsertOutcome {
        selection: target.selection,
        target_application_name: target.application_name,
        target_class: target.target_class.into(),
        caret_offset: target.caret_offset,
        before_text: target.before_text,
        after_text,
    })
}

fn control_bit(keysym: u32) -> Option<u8> {
    match keysym {
        CONTROL_LEFT_KEYSYM => Some(0b01),
        CONTROL_RIGHT_KEYSYM => Some(0b10),
        _ => None,
    }
}

struct FocusedFriendlyTarget {
    selection: FriendlyInsertSelection,
    application_name: String,
    target_class: &'static str,
    text: OwnedGObject<ffi::AtspiText>,
    editable_text: OwnedGObject<ffi::AtspiEditableText>,
    before_text: String,
    caret_offset: i32,
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
    let caret_offset = match text.as_ref() {
        Some(text) => unsafe { text_caret_offset(text.as_ptr())? },
        None => -1,
    };
    let target = FriendlyFocusedTarget {
        application_id,
        is_editable,
        supports_editable_text: editable_text.is_some(),
        supports_caret: caret_offset >= 0,
    };
    let target_class = friendly_insert_target_class_name(
        friendly_insert_target_class_from_application_id(&target.application_id),
    )
    .ok_or_else(|| {
        FriendlyInsertRunError::Access("friendly insertion target class is unsupported".into())
    })?;
    let selection = select_friendly_insert_backend(&target, policy)
        .map_err(FriendlyInsertRunError::UnsupportedTarget)?;
    let text = text.ok_or_else(|| {
        FriendlyInsertRunError::Access("friendly insertion target is missing Text support".into())
    })?;
    let editable_text = editable_text.ok_or_else(|| {
        FriendlyInsertRunError::Access(
            "friendly insertion target is missing EditableText support".into(),
        )
    })?;
    let before_text = unsafe { text_contents(text.as_ptr())? };

    Ok(FocusedFriendlyTarget {
        selection,
        application_name,
        target_class,
        text,
        editable_text,
        before_text,
        caret_offset,
    })
}

fn friendly_application_id_from_executable_name(executable_name: &str) -> String {
    match executable_name {
        "gnome-text-editor" => "org.gnome.TextEditor".into(),
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
    use glib::ffi::{gboolean, gchar, gint, gpointer, guint, GDestroyNotify};

    pub type AtspiStateType = gint;

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
    fn accessible_insert_rejects_non_editable_targets() {
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
    fn accessible_insert_rejects_targets_without_editable_text() {
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
    fn accessible_insert_rejects_targets_without_caret_surface() {
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
    fn accessible_insert_reports_stable_backend_name() {
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

    #[test]
    fn accessible_insert_accepts_browser_textarea_targets() {
        let selection = select_friendly_insert_backend(
            &FriendlyFocusedTarget {
                application_id: "firefox".into(),
                is_editable: true,
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
}

#[cfg(test)]
mod accessible_insert_runtime_helpers {
    use super::*;

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
}

#[cfg(test)]
mod accessible_insert_live {
    use super::*;

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
}
