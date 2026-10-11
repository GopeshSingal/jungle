mod cli;
mod core;

use std::process::ExitCode;
fn main() -> ExitCode {
    match cli::run() {
        Ok(code) => code,
        Err(e)   => {
            eprintln!("jungle: {e:#}");
            ExitCode::FAILURE
        }
    }
}
