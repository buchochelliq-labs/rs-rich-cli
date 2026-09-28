//! Size limits the bindings enforce before handing values to core.
//!
//! Rich accepts any size and then runs out of memory or time; core would
//! abort the process or overflow the native stack instead, so the bindings
//! refuse such values up front (docs/python/compatibility.md lists them).

/// The widest `Console(width=...)` accepted. Core allocates lines of the
/// console's width, so an absurd width aborts the process on allocation;
/// 65536 columns is far beyond any real terminal.
pub(crate) const MAX_CONSOLE_WIDTH: usize = 1 << 16;

/// The largest column `width`, `min_width` or `max_width` accepted: the
/// widest console. Core lays out a column at its minimum width even when
/// that is far wider than the console, which takes minutes for a huge one.
pub(crate) const MAX_COLUMN_WIDTH: usize = MAX_CONSOLE_WIDTH;

/// The largest column `ratio` accepted. A ratio only divides free space, so
/// it may be larger than any width, but core multiplies with it in `usize`.
pub(crate) const MAX_COLUMN_RATIO: usize = u32::MAX as usize;

/// The largest padding accepted on any side: the widest console. Core
/// builds every padding line, so a huge padding never finishes.
pub(crate) const MAX_PADDING: usize = MAX_CONSOLE_WIDTH;

/// The most renderables a render may nest, whatever the recursion limit.
/// Below it, [`crate::renderable::Nesting`] stops where Rich runs out of
/// Python frames (about 123 levels at the default limit) or where this
/// thread's native stack would run out, whichever comes first.
pub(crate) const MAX_NESTING: usize = 5000;

/// How many prints may run inside each other (a print reached again from
/// what it prints: `__str__`, a highlighter, a hook).
pub(crate) const MAX_PRINT_NESTING: usize = 100;

/// Python frames Rich's render takes per nested renderable (a `Panel` in a
/// `Panel`: `render`, `__rich_console__`, `render_lines` and the rest):
/// measured on rich 15.0.0, where each 1000 more frames of recursion limit
/// buys 125 more levels.
pub(crate) const FRAMES_PER_LEVEL: usize = 8;

/// Frames Rich's print takes around the outermost renderable, counted from
/// what is left of the recursion limit where the caller prints: fitted to
/// rich 15.0.0 frame by frame, so the port stops at the same depth as Rich
/// from every call depth (`tests/test_panel.py`). CPython 3.12 and later
/// spend two fewer here ([`RENDER_BASE_FRAMES_312`]).
pub(crate) const RENDER_BASE_FRAMES: usize = 11;

/// [`RENDER_BASE_FRAMES`] on CPython 3.12 and later.
pub(crate) const RENDER_BASE_FRAMES_312: usize = 9;

/// Native stack each nested level may take when core renders it, and what
/// the render needs besides: a level is refused, with `RecursionError`,
/// while less than that is left on this thread for the levels so far.
/// Measured: a release build renders a nested `Panel` in about 10 KiB.
pub(crate) const NATIVE_STACK_PER_LEVEL: usize = 8 << 10;
pub(crate) const NATIVE_STACK_RESERVE: usize = 64 << 10;

/// Native stack an interactive run (`rs_rich.interact`) needs left on this
/// thread before it starts, and what each run inside it takes besides: a
/// validator or Python component that starts another run nests them on the
/// native stack (about 11 KiB a run in a release build, 31 KiB in a debug
/// one), so past this a run raises `RecursionError` instead of overflowing.
pub(crate) const INTERACT_STACK_RESERVE: usize = 256 << 10;
pub(crate) const INTERACT_STACK_PER_RUN: usize = 32 << 10;

/// The most interactive runs that may run inside each other on a thread,
/// whatever its stack: the guard when the stack's size is not known.
pub(crate) const MAX_INTERACT_NESTING: usize = 200;

/// The tallest console, or options `height`, accepted: core pads renders to
/// their height, so an absurd height aborts the process on allocation.
pub(crate) const MAX_CONSOLE_HEIGHT: usize = 1 << 16;

/// The largest `tab_size` accepted: the widest console.
pub(crate) const MAX_TAB_SIZE: usize = MAX_CONSOLE_WIDTH;

/// The longest `Text` the bindings build by padding (`pad`, `set_length`,
/// `extend_style`, `align`, `fit`, `truncate(pad=True)`, ...): 256 Mi
/// characters. Core aborts the process when such an allocation fails;
/// Rich raises `MemoryError`.
pub(crate) const MAX_TEXT_LENGTH: usize = 1 << 28;

/// The most blank lines `Console.line` writes at once.
pub(crate) const MAX_NEWLINES: usize = 1 << 24;

/// Refuse a size past `limit` as Rich's allocation of it would fail: with
/// `MemoryError`, instead of aborting the process in core.
pub(crate) fn check_size(what: &str, value: usize, limit: usize) -> pyo3::PyResult<usize> {
    if value > limit {
        return Err(pyo3::exceptions::PyMemoryError::new_err(format!(
            "{what} must be at most {limit}, got {value}"
        )));
    }
    Ok(value)
}

/// Refuse a size Rich would build a string or list of: past `isize::MAX`
/// (a Python `int` past `sys.maxsize` saturates to it here) with Rich's
/// `OverflowError`, past `limit` with its `MemoryError` ([`check_size`]).
pub(crate) fn check_alloc(what: &str, value: usize, limit: usize) -> pyo3::PyResult<usize> {
    if value >= isize::MAX as usize {
        return Err(pyo3::exceptions::PyOverflowError::new_err(
            "cannot fit 'int' into an index-sized integer",
        ));
    }
    check_size(what, value, limit)
}
