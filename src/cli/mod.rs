//! CLI entry point: parse args, dispatch subcommands, map errors to
//! exit codes and stable output.

pub mod args;
pub mod batch;
pub mod capabilities;
pub mod inspect;
pub mod json;
pub mod render;

use clap::Parser;

use crate::cli::args::{CapabilitiesArgs, Cli, Command};
use crate::diagnostics::error::Result;

/// Exit codes:
/// - 0: success (including batches with per-file failures, which report
///   `failed > 0` in their result)
/// - 1: a command failed (fatal error or abort-on-error batch)
/// - 2: usage error (clap handles this itself)
pub const EXIT_OK: i32 = 0;
pub const EXIT_ERROR: i32 = 1;

/// Run the CLI from `std::env::args`. Returns the process exit code.
pub fn main_entry() -> i32 {
    let cli = Cli::parse();
    let want_json = cli.wants_json();
    match dispatch(cli) {
        Ok(None) => EXIT_OK,
        Ok(Some(response)) => {
            print_json(&response);
            EXIT_OK
        }
        Err(e) => {
            if want_json {
                // Structured error envelope on stdout for programmatic
                // consumers; the human-readable message still goes to
                // stderr and the exit code is 1.
                print_json(&json::Response::err(&e));
            }
            eprintln!(
                "mesh-thumb: {}",
                crate::diagnostics::error::ErrorReport::from(&e)
            );
            EXIT_ERROR
        }
    }
}

fn dispatch(cli: Cli) -> Result<Option<json::Response>> {
    match cli.command {
        Command::Render(a) => render::execute(&a),
        Command::Inspect(a) => inspect::execute(&a),
        Command::Capabilities(a) => capabilities::execute(&a),
        Command::Batch(a) => batch::execute(&a),
    }
}

fn print_json(response: &json::Response) {
    match serde_json::to_string_pretty(response) {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("mesh-thumb: failed to serialize JSON response: {e}");
            std::process::exit(EXIT_ERROR);
        }
    }
}

/// Library-accessible run for tests (per-command).
pub fn run_capabilities(args: &CapabilitiesArgs) -> Result<Option<json::Response>> {
    capabilities::execute(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_error_shape() {
        let e = crate::diagnostics::error::Error::unsupported_format(
            std::path::Path::new("x.xyz"),
            "no magic",
        );
        let r = json::Response::err(&e);
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"success\":false"));
        assert!(s.contains("\"unsupported_format\""));
    }
}
