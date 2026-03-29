use std::path::Path;

use crate::settings::LAUNCH_AT_LOGIN_DESKTOP_FILE_NAME;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupLaunchPolicy {
    Interactive,
    Background,
}

pub fn startup_launch_policy() -> StartupLaunchPolicy {
    if launched_from_autostart_desktop_file() {
        StartupLaunchPolicy::Background
    } else {
        StartupLaunchPolicy::Interactive
    }
}

fn launched_from_autostart_desktop_file() -> bool {
    std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE")
        .and_then(|value| {
            Path::new(&value)
                .file_name()
                .map(|file_name| file_name == LAUNCH_AT_LOGIN_DESKTOP_FILE_NAME)
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn startup_policy_treats_the_packaged_autostart_desktop_file_as_background_launch() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE");
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE",
            "/usr/share/applications/pepper-x-autostart.desktop",
        );

        assert_eq!(startup_launch_policy(), StartupLaunchPolicy::Background);

        match previous {
            Some(previous) => std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", previous),
            None => std::env::remove_var("GIO_LAUNCHED_DESKTOP_FILE"),
        }
    }

    #[test]
    fn startup_policy_treats_regular_launcher_desktop_files_as_interactive_launches() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE");
        std::env::set_var(
            "GIO_LAUNCHED_DESKTOP_FILE",
            "/usr/share/applications/com.obra.PepperX.desktop",
        );

        assert_eq!(startup_launch_policy(), StartupLaunchPolicy::Interactive);

        match previous {
            Some(previous) => std::env::set_var("GIO_LAUNCHED_DESKTOP_FILE", previous),
            None => std::env::remove_var("GIO_LAUNCHED_DESKTOP_FILE"),
        }
    }
}
