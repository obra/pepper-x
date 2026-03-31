mod app;
mod app_model;
mod background;
mod cli;
mod diagnostics_view;
mod history_store;
mod history_view;
mod onboarding;
mod overlay;
mod session_runtime;
mod settings;
mod settings_view;
mod startup_policy;
mod transcript_log;
mod transcription;
mod window;

fn main() {
    let startup_mode = match cli::parse_args(std::env::args_os()) {
        Ok(startup_mode) => startup_mode,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let should_launch_gui = matches!(startup_mode, cli::StartupMode::Gui);

    match cli::run(startup_mode) {
        Ok(Some(entry)) => println!("{}", entry.display_text()),
        Ok(None) if should_launch_gui => app::run(),
        Ok(None) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
