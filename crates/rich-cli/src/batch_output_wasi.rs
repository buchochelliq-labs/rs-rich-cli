//! `batch_output.rs` under WASI, where cap-std does not build: `--batch` is
//! refused before it gets here (`host::unavailable`), so every call reports
//! that the export directory is not available.
use std::io::{self, Read};
use std::path::Path;

fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        crate::host::unavailable("--batch's export directory"),
    )
}

pub(super) struct OutputRoot;

impl OutputRoot {
    pub fn capture(_path: &Path) -> io::Result<Self> {
        Err(unavailable())
    }

    pub fn destination(&self, _path: &Path, _create_parents: bool) -> io::Result<Destination> {
        Err(unavailable())
    }
}

pub(super) struct Destination;

impl Destination {
    pub fn publish(&self, _source: &mut impl Read, _overwrite: bool) -> io::Result<()> {
        Err(unavailable())
    }
}
