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

/// How many renderables deep a render may nest. Core renders recursively,
/// so without a limit a deep enough chain overflows the native stack and
/// kills the interpreter; upstream raises `RecursionError` a little past this
/// depth (it renders 100 nested panels and fails before 150). Enforced by
/// [`crate::renderable::Nesting`].
pub(crate) const MAX_NESTING: usize = 100;

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
