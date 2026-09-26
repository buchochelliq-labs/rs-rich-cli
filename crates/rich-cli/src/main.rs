//! The `rich` binary: a Rust port of the `rich-cli` terminal toolbox.
//!
//! The command line itself lives in the library target (`lib.rs`), so the
//! Python wheel can run it in-process; this is only the process entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    // `args_os()`: `args()` panics on an argument that is not valid Unicode,
    // and such an argument may well name a file (`rich $'\xff.txt'`).
    rich_cli::run(std::env::args_os().skip(1).collect())
}
