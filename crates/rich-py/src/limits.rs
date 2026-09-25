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
