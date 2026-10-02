//! Separate lab executable; never included in the application installation bundle.

#[path = "../src/command/mod.rs"]
mod command;

use clap::Parser;

#[derive(Parser)]
struct LabOptions {
    #[arg(long)]
    qualification_public_key: std::path::PathBuf,
    #[command(flatten)]
    options: command::Options,
}

fn main() -> std::process::ExitCode {
    let options = LabOptions::parse();
    let outcome = std::fs::read_to_string(options.qualification_public_key)
        .map_err(command::CommandError::from)
        .and_then(|key| command::run(options.options, &key));
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("qualification_error {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
