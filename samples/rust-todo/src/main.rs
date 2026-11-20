//! The binary: read the arguments and the store path, call the library, and set the exit code.

use std::process::ExitCode;

use rust_todo::run;
use rust_todo::store::{self, FILE_VAR};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = store::resolve_path(std::env::var(FILE_VAR).ok().as_deref());
    match run(&args, &path) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
