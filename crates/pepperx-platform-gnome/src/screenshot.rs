use std::path::{Path, PathBuf};

use zbus::{blocking::Connection, Error as ZbusError};

pub const SCREENSHOT_BUS_NAME: &str = "org.gnome.Shell.Screenshot";
pub const SCREENSHOT_OBJECT_PATH: &str = "/org/gnome/Shell/Screenshot";
pub const SCREENSHOT_INTERFACE_NAME: &str = "org.gnome.Shell.Screenshot";
pub const SCREENSHOT_METHOD_NAME: &str = "ScreenshotWindow";

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

    let required_arguments = [
        r#"<arg name="include_frame" type="b" direction="in"/>"#,
        r#"<arg name="include_cursor" type="b" direction="in"/>"#,
        r#"<arg name="flash" type="b" direction="in"/>"#,
        r#"<arg name="filename" type="s" direction="in"/>"#,
        r#"<arg name="success" type="b" direction="out"/>"#,
        r#"<arg name="filename_used" type="s" direction="out"/>"#,
    ];

    let mut search_index = method_index;
    for argument in required_arguments {
        let offset =
            xml[search_index..]
                .find(argument)
                .ok_or(ScreenshotContractError::MissingArgument {
                    name: argument_name(argument),
                })?;
        search_index += offset + argument.len();
    }

    Ok(())
}

pub fn screenshot_window(
    connection: &Connection,
    filename: impl AsRef<Path>,
    include_frame: bool,
    include_cursor: bool,
    flash: bool,
) -> Result<PathBuf, ScreenshotWindowError> {
    let filename = validated_png_path(filename.as_ref())?;
    let filename = filename
        .to_str()
        .ok_or(ScreenshotWindowError::InvalidFilename)?;

    let reply = connection
        .call_method(
            Some(SCREENSHOT_BUS_NAME),
            SCREENSHOT_OBJECT_PATH,
            Some(SCREENSHOT_INTERFACE_NAME),
            SCREENSHOT_METHOD_NAME,
            &(include_frame, include_cursor, flash, filename),
        )
        .map_err(classify_dbus_error)?;

    let (success, filename_used): (bool, String) = reply
        .body()
        .deserialize()
        .map_err(|_| ScreenshotWindowError::InvalidReply)?;

    interpret_screenshot_reply(success, filename_used)
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
