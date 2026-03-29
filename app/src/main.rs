mod app;
mod background;
mod cli;
mod settings;
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

    match cli::run(startup_mode) {
        Ok(Some(entry)) => println!("{}", entry.display_text()),
        Ok(None) => app::run(),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
