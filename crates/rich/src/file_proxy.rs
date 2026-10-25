//! Port of upstream `rich/file_proxy.py`.
//!
//! Upstream's `FileProxy` stands in for `sys.stdout` or `sys.stderr` while a
//! `Live` display runs: whole lines written to it are decoded from ANSI and
//! printed through the console, so they land above the display instead of
//! tearing it, and a partial line waits in a buffer for its newline.
//!
//! Rust cannot swap the process's standard streams the way Python reassigns
//! `sys.stdout`: `print!` and [`std::io::stdout`] always reach file
//! descriptor 1, and redirecting that descriptor needs unsafe, platform
//! specific calls that would also capture the display's own output. So a
//! [`FileProxy`] is a writer the program writes to *instead of* stdout
//! (see [`Live::stdout`](crate::live::Live::stdout) and
//! [`AutoLive::stdout`](crate::live::AutoLive::stdout)). Everything else
//! follows upstream: the line buffering, the ANSI decoding, `flush` printing
//! a partial line as console markup, and writes going straight to the
//! proxied file whenever the display is not redirecting (it is stopped,
//! redirection is off, or the console is not a terminal).

use std::io::{self, Write};

use crate::ansi::AnsiDecoder;
use crate::text::Text;

/// Which standard stream a [`FileProxy`] stands in for. Upstream's
/// `redirect_stdout` and `redirect_stderr` switch each one separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProxyStream {
    /// Standard output (`sys.stdout`).
    Stdout,
    /// Standard error (`sys.stderr`).
    Stderr,
}

/// A live display a [`FileProxy`] can print through.
///
/// Implemented for `&mut Live<W>` and for the [`LiveHandle`] of a display on
/// a refresh thread. Each method returns `false` when the display is not
/// redirecting `stream`, and the proxy then writes to its file instead.
///
/// [`LiveHandle`]: crate::live::LiveHandle
pub trait ProxyTarget {
    /// Upstream's `FileProxy.write`: buffer `bytes` and print every completed
    /// line through the display.
    fn proxy_write(&mut self, stream: ProxyStream, bytes: &[u8]) -> bool;

    /// Upstream's `FileProxy.flush`: print a buffered partial line.
    fn proxy_flush(&mut self, stream: ProxyStream) -> bool;
}

/// A writer that sends text through a live display. Mirrors
/// `rich.file_proxy.FileProxy`; the line buffer lives in the display, so a
/// partial line survives from one proxy to the next, as it does on the one
/// `sys.stdout` upstream.
pub struct FileProxy<T: ProxyTarget, F: Write> {
    target: T,
    stream: ProxyStream,
    file: F,
}

impl<T: ProxyTarget, F: Write> FileProxy<T, F> {
    /// A proxy for `stream` that prints through `target`, writing to `file`
    /// when the display is not redirecting.
    pub fn new(target: T, stream: ProxyStream, file: F) -> Self {
        FileProxy {
            target,
            stream,
            file,
        }
    }

    /// The proxied file. Port of `FileProxy.rich_proxied_file`; Python's
    /// `fileno` and `isatty` forward to it, which here is the caller's to do.
    pub fn rich_proxied_file(&self) -> &F {
        &self.file
    }

    /// Which stream this proxy stands in for.
    pub fn stream(&self) -> ProxyStream {
        self.stream
    }

    /// Consume the proxy, returning the proxied file.
    pub fn into_inner(self) -> F {
        self.file
    }
}

impl<T: ProxyTarget, F: Write> Write for FileProxy<T, F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.target.proxy_write(self.stream, bytes) {
            Ok(bytes.len())
        } else {
            self.file.write(bytes)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.target.proxy_flush(self.stream) {
            Ok(())
        } else {
            self.file.flush()
        }
    }
}

/// The state upstream keeps on each `FileProxy`: the partial line and the
/// ANSI decoder, whose style carries from one line to the next.
#[derive(Default)]
pub(crate) struct ProxyBuffer {
    pending: Vec<u8>,
    decoder: AnsiDecoder,
}

impl ProxyBuffer {
    /// Upstream's `write`: split off every completed line (the buffer joined
    /// to the first), keep the rest, and return the completed lines decoded
    /// and joined with newlines, or `None` when no line was completed.
    pub(crate) fn write(&mut self, bytes: &[u8]) -> Option<Text> {
        let mut lines = Vec::new();
        let mut rest = bytes;
        while let Some(end) = rest.iter().position(|&byte| byte == b'\n') {
            let mut line = std::mem::take(&mut self.pending);
            line.extend_from_slice(&rest[..end]);
            lines.push(String::from_utf8_lossy(&line).into_owned());
            rest = &rest[end + 1..];
        }
        self.pending.extend_from_slice(rest);
        if lines.is_empty() {
            return None;
        }
        let decoded: Vec<Text> = lines
            .iter()
            .map(|line| self.decoder.decode_line(line))
            .collect();
        Some(Text::new("\n").join(&decoded))
    }

    /// Upstream's `flush`: take the partial line, if any (printed as a `str`,
    /// so as console markup).
    pub(crate) fn flush(&mut self) -> Option<String> {
        let pending = std::mem::take(&mut self.pending);
        (!pending.is_empty()).then(|| String::from_utf8_lossy(&pending).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_splits_lines_like_upstream() {
        let mut buffer = ProxyBuffer::default();
        assert!(buffer.write(b"part").is_none());
        let text = buffer.write(b"ial\nline2\nrest").expect("two lines");
        assert_eq!(text.plain(), "partial\nline2");
        assert_eq!(buffer.flush().as_deref(), Some("rest"));
        assert_eq!(buffer.flush(), None);
    }

    /// A target that never redirects, as a stopped display.
    struct Stopped;

    impl ProxyTarget for Stopped {
        fn proxy_write(&mut self, _: ProxyStream, _: &[u8]) -> bool {
            false
        }
        fn proxy_flush(&mut self, _: ProxyStream) -> bool {
            false
        }
    }

    #[test]
    fn a_stopped_display_writes_to_the_file() {
        let mut proxy = FileProxy::new(Stopped, ProxyStream::Stdout, Vec::new());
        writeln!(proxy, "plain").unwrap();
        proxy.flush().unwrap();
        assert_eq!(proxy.into_inner(), b"plain\n");
    }
}
