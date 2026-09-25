//! The `rich` binary: a Rust port of the `rich-cli` terminal toolbox.
//!
//! The command line itself lives in the library target (`lib.rs`), so the
//! Python wheel can run it in-process; this is only the process entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    // `args()` (not `args_os()`): a non-Unicode argument stops the binary
    // exactly as it always has.
    rich_cli::run(std::env::args().skip(1).map(Into::into).collect())
}
