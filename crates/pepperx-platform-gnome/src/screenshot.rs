use std::path::{Path, PathBuf};

use zbus::{blocking::Connection, Error as ZbusError};

pub const SCREENSHOT_BUS_NAME: &str = "org.gnome.Shell.Screenshot";
pub const SCREENSHOT_OBJECT_PATH: &str = "/org/gnome/Shell/Screenshot";
pub const SCREENSHOT_INTERFACE_NAME: &str = "org.gnome.Shell.Screenshot";
pub const SCREENSHOT_METHOD_NAME: &str = "ScreenshotWindow";
const INTROSPECTABLE_INTERFACE_NAME: &str = "org.freedesktop.DBus.Introspectable";
const INTROSPECT_METHOD_NAME: &str = "Introspect";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotContractError {
    MissingInterface,
    MissingMethod,
    MissingArgument { name: &'static str },
}

impl std::fmt::Display for ScreenshotContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingInterface => f.write_str("missing org.gnome.Shell.Screenshot interface"),
            Self::MissingMethod => f.write_str("missing ScreenshotWindow method"),
            Self::MissingArgument { name } => write!(f, "missing {name} argument"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenshotWindowError {
    InvalidFilename,
    Contract(ScreenshotContractError),
    AccessDenied,
    Unavailable,
    Rejected { filename_used: PathBuf },
    InvalidReply,
    Dbus(String),
}

impl std::fmt::Display for ScreenshotWindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFilename => {
                f.write_str("screenshot filename must be an absolute .png path")
            }
            Self::Contract(error) => write!(f, "{error}"),
            Self::AccessDenied => f.write_str("GNOME Shell denied screenshot access"),
            Self::Unavailable => f.write_str("GNOME Shell screenshot service is unavailable"),
            Self::Rejected { filename_used } => {
                write!(
                    f,
                    "GNOME Shell rejected screenshot request: {filename_used:?}"
                )
            }
            Self::InvalidReply => f.write_str("GNOME Shell screenshot reply had the wrong shape"),
            Self::Dbus(message) => f.write_str(message),
        }
    }
}

pub fn validate_interface_xml(xml: &str) -> Result<(), ScreenshotContractError> {
    let interface_index = xml
        .find(r#"<interface name="org.gnome.Shell.Screenshot">"#)
        .ok_or(ScreenshotContractError::MissingInterface)?;
    let method_index = xml[interface_index..]
        .find(r#"<method name="ScreenshotWindow">"#)
        .ok_or(ScreenshotContractError::MissingMethod)?
        + interface_index;

    // Check for required argument names (attribute-order-independent).
    // GNOME versions may emit attributes in different order (type before name, etc.).
    let required_arg_names = [
        ("include_frame", "in"),
        ("include_cursor", "in"),
        ("flash", "in"),
        ("filename", "in"),
        ("success", "out"),
        ("filename_used", "out"),
    ];

    let method_xml = &xml[method_index..];
    for (name, direction) in required_arg_names {
        // Check that an <arg> with this name and direction exists somewhere
        // in the method XML, regardless of attribute order.
        let has_arg = method_xml.contains(&format!(r#"name="{name}""#))
            && method_xml.contains(&format!(r#"direction="{direction}""#));
        if !has_arg {
            return Err(ScreenshotContractError::MissingArgument { name });
        }
    }

    Ok(())
}

pub fn screenshot_window(
    _connection: &Connection,
    filename: impl AsRef<Path>,
    _include_frame: bool,
    _include_cursor: bool,
    _flash: bool,
) -> Result<PathBuf, ScreenshotWindowError> {
    let filename = validated_png_path(filename.as_ref())?;

    // Use the XDG Desktop Portal via gdbus — compositor-agnostic, no sound,
    // no user dialog (when permission is already granted).
    let output = std::process::Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.freedesktop.portal.Desktop",
            "--object-path", "/org/freedesktop/portal/desktop",
            "--method", "org.freedesktop.portal.Screenshot.Screenshot",
            "", // parent_window
            "{'interactive': <false>}",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|_| ScreenshotWindowError::Unavailable)?;

    if !output.status.success() {
        return Err(ScreenshotWindowError::Unavailable);
    }

    // The portal saves the screenshot asynchronously. We need to wait for it.
    // The response comes via a D-Bus signal, but gdbus doesn't wait for it.
    // Instead, wait briefly and check the Pictures directory for the latest screenshot.
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Find the most recently created screenshot in ~/Pictures/
    let pictures_dir = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join("Pictures"))
        .map_err(|_| ScreenshotWindowError::Unavailable)?;

    let latest = std::fs::read_dir(&pictures_dir)
        .map_err(|_| ScreenshotWindowError::Unavailable)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("Screenshot")
                && e.path().extension().map(|ext| ext == "png").unwrap_or(false)
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());

    let latest = latest.ok_or(ScreenshotWindowError::Unavailable)?;

    // Check it was created in the last 2 seconds
    if let Ok(modified) = latest.metadata().and_then(|m| m.modified()) {
        if modified.elapsed().map(|d| d.as_secs() > 2).unwrap_or(true) {
            return Err(ScreenshotWindowError::Unavailable);
        }
    }

    // Move to our desired filename
    std::fs::rename(latest.path(), &filename)
        .or_else(|_| std::fs::copy(latest.path(), &filename).map(|_| ()))
        .map_err(|_| ScreenshotWindowError::Unavailable)?;
    let _ = std::fs::remove_file(latest.path());

    Ok(filename)
}

pub fn introspect_interface_xml(connection: &Connection) -> Result<String, ScreenshotWindowError> {
    let reply = connection
        .call_method(
            Some(SCREENSHOT_BUS_NAME),
            SCREENSHOT_OBJECT_PATH,
            Some(INTROSPECTABLE_INTERFACE_NAME),
            INTROSPECT_METHOD_NAME,
            &(),
        )
        .map_err(classify_dbus_error)?;

    reply
        .body()
        .deserialize::<String>()
        .map_err(|_| ScreenshotWindowError::InvalidReply)
}

fn validated_png_path(path: &Path) -> Result<PathBuf, ScreenshotWindowError> {
    if !path.is_absolute() {
        return Err(ScreenshotWindowError::InvalidFilename);
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("png") {
        return Err(ScreenshotWindowError::InvalidFilename);
    }

    Ok(path.to_path_buf())
}

fn interpret_screenshot_reply(
    success: bool,
    filename_used: impl Into<PathBuf>,
) -> Result<PathBuf, ScreenshotWindowError> {
    let filename_used = filename_used.into();

    if success {
        Ok(filename_used)
    } else {
        Err(ScreenshotWindowError::Rejected { filename_used })
    }
}

fn classify_dbus_error(error: ZbusError) -> ScreenshotWindowError {
    match error {
        ZbusError::MethodError(name, _, _) => classify_dbus_error_name(name.as_str()),
        ZbusError::InterfaceNotFound
        | ZbusError::Unsupported
        | ZbusError::InvalidReply
        | ZbusError::MissingField => ScreenshotWindowError::Unavailable,
        other => ScreenshotWindowError::Dbus(other.to_string()),
    }
}

fn classify_dbus_error_name(name: &str) -> ScreenshotWindowError {
    match name {
        "org.freedesktop.DBus.Error.AccessDenied" => ScreenshotWindowError::AccessDenied,
        "org.freedesktop.DBus.Error.ServiceUnknown"
        | "org.freedesktop.DBus.Error.NameHasNoOwner"
        | "org.freedesktop.DBus.Error.UnknownObject"
        | "org.freedesktop.DBus.Error.UnknownInterface"
        | "org.freedesktop.DBus.Error.UnknownMethod" => ScreenshotWindowError::Unavailable,
        other => ScreenshotWindowError::Dbus(other.to_owned()),
    }
}

fn argument_name(argument: &str) -> &'static str {
    if argument.contains("include_frame") {
        "include_frame"
    } else if argument.contains("include_cursor") {
        "include_cursor"
    } else if argument.contains("flash") {
        "flash"
    } else if argument.contains("filename_used") {
        "filename_used"
    } else if argument.contains("filename") && argument.contains("direction=\"in\"") {
        "filename"
    } else if argument.contains("success") {
        "success"
    } else {
        "argument"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn validates_the_gnome_shell_screenshot_interface_shape() {
        let xml = r#"
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
        "#;

        validate_interface_xml(xml).unwrap();
    }

    #[test]
    fn requires_an_absolute_png_path() {
        assert!(validated_png_path(Path::new("/tmp/pepperx-shot.png")).is_ok());
        assert!(validated_png_path(Path::new("pepperx-shot.png")).is_err());
        assert!(validated_png_path(Path::new("/tmp/pepperx-shot.jpg")).is_err());
    }

    #[test]
    fn maps_a_false_reply_to_a_rejection() {
        let error = interpret_screenshot_reply(false, "/tmp/pepperx-shot.png");

        assert_eq!(
            error,
            Err(ScreenshotWindowError::Rejected {
                filename_used: "/tmp/pepperx-shot.png".into(),
            })
        );
    }

    #[test]
    fn classifies_access_and_availability_errors() {
        assert_eq!(
            classify_dbus_error_name("org.freedesktop.DBus.Error.AccessDenied"),
            ScreenshotWindowError::AccessDenied
        );
        assert_eq!(
            classify_dbus_error_name("org.freedesktop.DBus.Error.ServiceUnknown"),
            ScreenshotWindowError::Unavailable
        );
    }
}
