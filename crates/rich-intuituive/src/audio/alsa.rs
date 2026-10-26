//! Keeping ALSA's messages off the screen. alsa-lib writes what it finds
//! wrong (`ALSA lib conf.c:5180:(_snd_config_evaluate) …`) to stderr,
//! where a full-screen app would show it over its cells. While a device is
//! listed or opened, a handler of our own takes them instead, on that
//! thread only (`snd_lib_error_set_local`, so nothing else that uses ALSA
//! in the process is changed), and keeps them for
//! [`messages`](super::messages). A stream's own thread (cpal's, where an
//! underrun or a failed write is reported) is given the handler by the
//! stream's first callback, for the thread's life: the stream's life.
//!
//! The one module of the crate with `unsafe`: three calls into alsa-lib
//! and the C library, each with its contract stated where it is made.
#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::ffi::{c_char, c_int, CStr};
use std::sync::Mutex;

use alsa_sys::{__va_list_tag, snd_lib_error_set_local, snd_local_error_handler_t};

/// How many messages are kept.
const KEPT: usize = 200;

/// Every message taken, oldest first, with how many were ever taken (so a
/// reader can tell which are new after older ones have gone).
static MESSAGES: Mutex<(VecDeque<String>, usize)> = Mutex::new((VecDeque::new(), 0));

unsafe extern "C" {
    // The C library's: formats `format` with the arguments in `args`. A
    // `va_list` is passed on as the handler was given it, which is a
    // pointer (or pointer-sized) on every target ALSA runs on.
    fn vsnprintf(
        buffer: *mut c_char,
        size: usize,
        format: *const c_char,
        args: *mut __va_list_tag,
    ) -> c_int;
}

/// The handler: `file:line:(function) message`, as ALSA's own would write
/// it, kept instead of written.
unsafe extern "C" fn keep(
    file: *const c_char,
    line: c_int,
    function: *const c_char,
    err: c_int,
    format: *const c_char,
    args: *mut __va_list_tag,
) {
    let text = |p: *const c_char| {
        if p.is_null() {
            String::new()
        } else {
            // SAFETY: ALSA passes NUL-terminated strings (or null).
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    };
    let mut buffer = [0 as c_char; 512];
    let message = if format.is_null() {
        String::new()
    } else {
        // SAFETY: the buffer's size is passed, so vsnprintf writes within
        // it and ends it with a NUL; the format and arguments are ALSA's.
        unsafe { vsnprintf(buffer.as_mut_ptr(), buffer.len(), format, args) };
        text(buffer.as_ptr())
    };
    let file = text(file);
    let file = file.rsplit('/').next().unwrap_or_default();
    let mut line = format!("ALSA lib {file}:{line}:({}) {message}", text(function));
    if err != 0 {
        // SAFETY: snd_strerror returns a static string for any number.
        let reason = text(unsafe { alsa_sys::snd_strerror(err) });
        line.push_str(&format!(": {reason}"));
    }
    keep_message(line);
}

fn keep_message(line: String) {
    if let Ok(mut kept) = MESSAGES.lock() {
        if kept.0.len() == KEPT {
            kept.0.pop_front();
        }
        kept.0.push_back(line);
        kept.1 += 1;
    }
}

/// The messages kept, oldest first.
pub(super) fn messages() -> Vec<String> {
    MESSAGES
        .lock()
        .map(|kept| kept.0.iter().cloned().collect())
        .unwrap_or_default()
}

/// The messages taken after the first `seen` ever taken, and the count to
/// pass next time.
pub(super) fn messages_since(seen: usize) -> (Vec<String>, usize) {
    let Ok(kept) = MESSAGES.lock() else {
        return (Vec::new(), seen);
    };
    let (lines, total) = (&kept.0, kept.1);
    let new = total.saturating_sub(seen).min(lines.len());
    (
        lines.iter().skip(lines.len() - new).cloned().collect(),
        total,
    )
}

/// While it lives, ALSA's messages on this thread are kept rather than
/// written to stderr.
pub(super) struct Quiet {
    before: snd_local_error_handler_t,
}

impl Quiet {
    pub(super) fn new() -> Quiet {
        // SAFETY: sets this thread's handler, returning the one before.
        let before = unsafe { snd_lib_error_set_local(Some(keep)) };
        Quiet { before }
    }
}

std::thread_local! {
    /// The handler is in force on this thread for its life.
    static KEPT_HERE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Keep ALSA's messages on this thread from now on, for the thread's
/// life: what a stream's callbacks call, on the thread that runs the
/// stream. Cheap after the first call.
pub(super) fn quiet_thread() {
    KEPT_HERE.with(|kept| {
        if !kept.get() {
            // SAFETY: sets this thread's handler; the thread keeps it
            // until it ends.
            unsafe { snd_lib_error_set_local(Some(keep)) };
            kept.set(true);
        }
    });
}

impl Drop for Quiet {
    fn drop(&mut self) {
        // SAFETY: puts back this thread's handler from before.
        unsafe { snd_lib_error_set_local(self.before) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_messages_are_counted_past_old_ones() {
        let (_, start) = messages_since(usize::MAX);
        keep_message("one".into());
        keep_message("two".into());
        let (new, seen) = messages_since(start);
        // Other tests may keep messages meanwhile: these two, in order.
        let at = |m: &str| new.iter().position(|n| n == m);
        assert!(at("one") < at("two") && at("one").is_some(), "{new:?}");
        assert!(!messages_since(seen).0.contains(&"one".to_string()));
        assert!(messages().contains(&"two".to_string()));
    }

    #[test]
    fn opening_a_missing_device_writes_nothing_to_stderr() {
        // A PCM that does not exist makes alsa-lib complain through the
        // handler; with it installed the complaint is kept.
        let _quiet = Quiet::new();
        let (_, start) = messages_since(usize::MAX);
        let name = c"intuituive_no_such_pcm";
        let mut pcm = std::ptr::null_mut();
        // SAFETY: a valid name and an out-pointer; a failure returns < 0.
        let result = unsafe {
            alsa_sys::snd_pcm_open(&mut pcm, name.as_ptr(), alsa_sys::SND_PCM_STREAM_CAPTURE, 0)
        };
        assert!(result < 0);
        let (new, _) = messages_since(start);
        assert!(
            new.iter().any(|m| m.contains("intuituive_no_such_pcm")),
            "{new:?}"
        );
    }
}
