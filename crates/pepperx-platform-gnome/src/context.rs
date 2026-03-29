#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use zbus::blocking::Connection;

use crate::atspi::{inspect_focused_target, FocusedTargetSnapshot, FriendlyInsertRunError};
use crate::screenshot::{
    introspect_interface_xml, screenshot_window, validate_interface_xml, ScreenshotContractError,
    ScreenshotWindowError,
};

const SUPPORTING_CONTEXT_LIMIT: usize = 512;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportingContext {
    pub supporting_context_text: Option<String>,
    pub ocr_text: Option<String>,
    pub used_ocr: bool,
}

#[derive(Debug)]
pub enum ContextCaptureError {
    FocusedTarget(FriendlyInsertRunError),
    SessionBus(String),
    InvalidScreenshotContract(ScreenshotContractError),
    Screenshot(ScreenshotWindowError),
    Ocr(String),
}

impl std::fmt::Display for ContextCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FocusedTarget(error) => write!(f, "{error}"),
            Self::SessionBus(message) => f.write_str(message),
            Self::InvalidScreenshotContract(error) => write!(f, "{error}"),
            Self::Screenshot(error) => write!(f, "{error}"),
            Self::Ocr(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ContextCaptureError {}

pub fn capture_supporting_context() -> Result<SupportingContext, ContextCaptureError> {
    let snapshot = match inspect_focused_target() {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            eprintln!("[Pepper X] failed to inspect focused target for cleanup context: {error}");
            None
        }
    };
    let atspi_context =
        supporting_context_from_atspi(snapshot.as_ref(), None, SUPPORTING_CONTEXT_LIMIT);
    if atspi_context.supporting_context_text.is_some() {
        return Ok(atspi_context);
    }

    let connection = Connection::session()
        .map_err(|error| ContextCaptureError::SessionBus(error.to_string()))?;
    let screenshot_contract_xml =
        introspect_interface_xml(&connection).map_err(ContextCaptureError::Screenshot)?;
    let screenshot_path = temporary_screenshot_path();
    let result = capture_supporting_context_with(
        snapshot,
        &screenshot_contract_xml,
        || screenshot_window(&connection, &screenshot_path, false, false, false),
        ocr_png_with_tesseract,
        SUPPORTING_CONTEXT_LIMIT,
    );
    let _ = std::fs::remove_file(&screenshot_path);
    result
}

pub(crate) fn supporting_context_from_atspi(
    snapshot: Option<&FocusedTargetSnapshot>,
    ocr_text: Option<&str>,
    max_chars: usize,
) -> SupportingContext {
    if let Some(snapshot) = snapshot {
        if let Some(supporting_context_text) = snapshot.supporting_context_text(max_chars) {
            return SupportingContext {
                supporting_context_text: Some(supporting_context_text),
                ocr_text: None,
                used_ocr: false,
            };
        }
    }

    let ocr_text = ocr_text.map(|text| bound_text(text, max_chars));
    let used_ocr = ocr_text.is_some();

    SupportingContext {
        supporting_context_text: ocr_text.clone(),
        ocr_text,
        used_ocr,
    }
}

pub(crate) fn capture_supporting_context_with<S, O>(
    snapshot: Option<FocusedTargetSnapshot>,
    screenshot_contract_xml: &str,
    capture_screenshot: S,
    ocr_image: O,
    max_chars: usize,
) -> Result<SupportingContext, ContextCaptureError>
where
    S: FnOnce() -> Result<PathBuf, ScreenshotWindowError>,
    O: FnOnce(&Path) -> Result<Option<String>, ContextCaptureError>,
{
    let atspi_context = supporting_context_from_atspi(snapshot.as_ref(), None, max_chars);
    if atspi_context.supporting_context_text.is_some() {
        return Ok(atspi_context);
    }

    validate_interface_xml(screenshot_contract_xml)
        .map_err(ContextCaptureError::InvalidScreenshotContract)?;
    let screenshot_path = capture_screenshot().map_err(ContextCaptureError::Screenshot)?;
    let ocr_text = ocr_image(&screenshot_path)?;

    Ok(supporting_context_from_atspi(
        None,
        ocr_text.as_deref(),
        max_chars,
    ))
}

fn bound_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars.max(1)).collect()
}

fn ocr_png_with_tesseract(image_path: &Path) -> Result<Option<String>, ContextCaptureError> {
    let output = Command::new("tesseract")
        .arg(image_path)
        .arg("stdout")
        .arg("--psm")
        .arg("6")
        .output()
        .map_err(|error| ContextCaptureError::Ocr(error.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ContextCaptureError::Ocr(if stderr.is_empty() {
            String::from("tesseract OCR failed")
        } else {
            stderr
        }));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

fn temporary_screenshot_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "pepper-x-context-{}-{unique}.png",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        capture_supporting_context_with, supporting_context_from_atspi, ContextCaptureError,
    };
    use crate::atspi::FocusedTargetSnapshot;
    use std::path::{Path, PathBuf};

    #[test]
    fn context_prefers_atspi_text_over_ocr_and_bounds_it() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.TextEditor".into(),
            application_name: "Text Editor".into(),
            target_class: "text-editor",
            is_editable: true,
            supports_text: true,
            supports_editable_text: true,
            supports_caret: true,
            before_text: Some("abcdefghi".into()),
            caret_offset: Some(4),
        };

        let context = supporting_context_from_atspi(Some(&snapshot), Some("ocr fallback"), 4);

        assert_eq!(context.supporting_context_text.as_deref(), Some("cdef"));
        assert_eq!(context.ocr_text, None);
        assert!(!context.used_ocr);
    }

    #[test]
    fn context_uses_ocr_when_atspi_text_is_unavailable() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.TextEditor".into(),
            application_name: "Text Editor".into(),
            target_class: "text-editor",
            is_editable: true,
            supports_text: false,
            supports_editable_text: false,
            supports_caret: false,
            before_text: None,
            caret_offset: None,
        };

        let context = supporting_context_from_atspi(Some(&snapshot), Some("ocr fallback"), 4);

        assert_eq!(context.supporting_context_text.as_deref(), Some("ocr "));
        assert_eq!(context.ocr_text.as_deref(), Some("ocr "));
        assert!(context.used_ocr);
    }

    #[test]
    fn context_leaves_used_ocr_false_without_ocr_input() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.TextEditor".into(),
            application_name: "Text Editor".into(),
            target_class: "text-editor",
            is_editable: true,
            supports_text: false,
            supports_editable_text: false,
            supports_caret: false,
            before_text: None,
            caret_offset: None,
        };

        let context = supporting_context_from_atspi(Some(&snapshot), None, 4);

        assert_eq!(context.supporting_context_text, None);
        assert_eq!(context.ocr_text, None);
        assert!(!context.used_ocr);
    }

    #[test]
    fn context_requires_valid_gnome_shell_screenshot_contract_before_ocr_fallback() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.Terminal".into(),
            application_name: "Terminal".into(),
            target_class: "terminal",
            is_editable: true,
            supports_text: false,
            supports_editable_text: false,
            supports_caret: false,
            before_text: None,
            caret_offset: None,
        };

        let error = capture_supporting_context_with(
            Some(snapshot),
            "<node/>",
            || Ok(PathBuf::from("/tmp/pepperx-shot.png")),
            |_| Ok(Some("ocr fallback".into())),
            8,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ContextCaptureError::InvalidScreenshotContract(_)
        ));
    }

    #[test]
    fn context_falls_back_to_ocr_when_atspi_text_is_unavailable() {
        let snapshot = FocusedTargetSnapshot {
            application_id: "org.gnome.Terminal".into(),
            application_name: "Terminal".into(),
            target_class: "terminal",
            is_editable: true,
            supports_text: false,
            supports_editable_text: false,
            supports_caret: false,
            before_text: None,
            caret_offset: None,
        };

        let context = capture_supporting_context_with(
            Some(snapshot),
            r#"
                <node>
                  <interface name="org.gnome.Shell.Screenshot">
                    <method name="ScreenshotWindow">
                      <arg name="include_frame" type="b" direction="in"/>
                      <arg name="include_cursor" type="b" direction="in"/>
                      <arg name="flash" type="b" direction="in"/>
                      <arg name="filename" type="s" direction="in"/>
                      <arg name="success" type="b" direction="out"/>
                      <arg name="filename_used" type="s" direction="out"/>
                    </method>
                  </interface>
                </node>
            "#,
            || Ok(PathBuf::from("/tmp/pepperx-shot.png")),
            |path| {
                assert_eq!(path, Path::new("/tmp/pepperx-shot.png"));
                Ok(Some("ocr fallback".into()))
            },
            8,
        )
        .unwrap();

        assert_eq!(context.supporting_context_text.as_deref(), Some("ocr fall"));
        assert_eq!(context.ocr_text.as_deref(), Some("ocr fall"));
        assert!(context.used_ocr);
    }
}
