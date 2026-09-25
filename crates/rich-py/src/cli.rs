//! Running the `rich` CLI from Python: `python -m rs_rich` and the `rich-rs`
//! console script (`rs_rich/cli.py` calls [`cli_main`]).
//!
//! The command line is `rich_cli::run_embedded`, which is `rich_cli::run` (all
//! the `rich` binary's `main` does) told how to start itself again, so output
//! and exit codes are the binary's. It writes to the
//! process's real stdout and stderr (file descriptors 1 and 2) and reads its
//! stdin; Python's `sys.stdout` is flushed around it by `cli.py`.

use std::ffi::OsString;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::ExitCode;

use pyo3::prelude::*;

/// Run the `rich` command line with `argv` (the arguments after the program
/// name), with the GIL released, and return its exit status. `program` is the
/// command that starts the command line again (`[sys.executable, "-m",
/// "rs_rich"]`), for `--batch` workers and the demo's child `--watch`.
#[pyfunction]
fn cli_main(py: Python<'_>, program: Vec<OsString>, argv: Vec<OsString>) -> i32 {
    py.detach(|| {
        let status = catch_unwind(AssertUnwindSafe(|| rich_cli::run_embedded(program, argv)));
        // The binary's stdout is flushed when the process ends; in-process,
        // Rust's line buffer must be emptied before Python writes again.
        let _ = std::io::stdout().flush();
        match status {
            Ok(code) => exit_status(code),
            // A panic has already been reported on stderr by the panic hook;
            // the binary would end with Rust's panic status.
            Err(_) => 101,
        }
    })
}

/// The number an [`ExitCode`] carries. `ExitCode` has no accessor, but every
/// status the CLI returns is built from a `u8` (or is `SUCCESS`/`FAILURE`,
/// which are 0 and 1), so it equals exactly one `ExitCode::from(n)`.
fn exit_status(code: ExitCode) -> i32 {
    (0..=u8::MAX)
        .find(|&n| ExitCode::from(n) == code)
        .map_or(1, i32::from)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(cli_main, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_status_round_trips_every_code() {
        assert_eq!(exit_status(ExitCode::SUCCESS), 0);
        assert_eq!(exit_status(ExitCode::FAILURE), 1);
        for n in 0..=u8::MAX {
            assert_eq!(exit_status(ExitCode::from(n)), i32::from(n));
        }
    }
}
