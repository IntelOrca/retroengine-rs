//! `retro-engine` binary entry point.
#![forbid(unsafe_code)]

use clap::Parser;
use retro_engine::{Args, run};

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
