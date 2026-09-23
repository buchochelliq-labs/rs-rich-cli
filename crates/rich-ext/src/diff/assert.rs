//! Assertions that fail with a rendered diff: [`assert_rich_eq!`],
//! [`assert_rich_json_eq!`], [`assert_render_eq!`] and
//! [`assert_snapshot_eq!`].
//!
//! The diff is unified unless `RICH_ASSERT_LAYOUT=side-by-side`. It is
//! coloured only when `RICH_ASSERT_COLOR=1`, or when stdout is a terminal and
//! `CI` is unset (`RICH_ASSERT_COLOR=0` forces it off). Without colour the
//! output is plain ASCII: `-`/`+`/`~` markers and a `|` divider.
//!
//! [`assert_rich_eq!`]: crate::assert_rich_eq
//! [`assert_rich_json_eq!`]: crate::assert_rich_json_eq
//! [`assert_render_eq!`]: crate::assert_render_eq
//! [`assert_snapshot_eq!`]: crate::assert_snapshot_eq

use std::io::IsTerminal;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Renderable};
use serde::Serialize;

use super::{DiffView, Layout};
use crate::target::{RenderTarget, TargetKind};
use crate::testing::RenderSnapshot;

/// How a failing assertion renders its diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub layout: Layout,
    pub color: bool,
    pub width: usize,
}

impl Report {
    /// From `RICH_ASSERT_LAYOUT`, `RICH_ASSERT_COLOR`, `CI` and whether
    /// stdout is a terminal; 100 columns.
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok();
        let layout = match var("RICH_ASSERT_LAYOUT").as_deref() {
            Some("side-by-side" | "side_by_side" | "sbs") => Layout::SideBySide,
            _ => Layout::Unified,
        };
        let color = match var("RICH_ASSERT_COLOR").as_deref() {
            Some("1") => true,
            Some("0") => false,
            _ => std::io::stdout().is_terminal() && var("CI").is_none(),
        };
        Report {
            layout,
            color,
            width: 100,
        }
    }

    fn console(&self) -> Console {
        let builder = Console::builder().width(self.width).height(10_000);
        if self.color {
            builder
                .force_terminal(true)
                .color_system(Some(ColorSystem::Standard))
                .theme(crate::theme::extended_theme())
                .build()
        } else {
            builder
                .force_terminal(false)
                .no_color(true)
                .ascii_only(true)
                .build()
        }
    }

    /// Render `view` for a failure message.
    pub fn render(&self, view: &DiffView) -> String {
        let console = self.console();
        let view = view.clone().layout(self.layout);
        let segments = view.rich_render(&console, &console.options());
        console.segments_to_string(&segments)
    }
}

#[track_caller]
fn fail(what: &str, view: DiffView, message: Option<String>) -> ! {
    let rendered = Report::from_env().render(&view);
    match message {
        Some(message) => panic!("assertion `{what}` failed: {message}\n{rendered}"),
        None => panic!("assertion `{what}` failed\n{rendered}"),
    }
}

/// [`assert_rich_eq!`](crate::assert_rich_eq)'s body.
#[track_caller]
pub fn assert_str_eq(left: &str, right: &str, message: Option<String>) {
    if left != right {
        fail(
            "left == right",
            DiffView::new(left, right).titles("left", "right"),
            message,
        );
    }
}

/// [`assert_rich_json_eq!`](crate::assert_rich_json_eq)'s body: both values
/// as pretty JSON.
#[track_caller]
pub fn assert_json_eq<L: Serialize + ?Sized, R: Serialize + ?Sized>(
    left: &L,
    right: &R,
    message: Option<String>,
) {
    let json =
        |v: serde_json::Result<String>| v.unwrap_or_else(|e| format!("<not serializable: {e}>"));
    let (l, r) = (
        serde_json::to_value(left).and_then(|v| serde_json::to_string_pretty(&v)),
        serde_json::to_value(right).and_then(|v| serde_json::to_string_pretty(&v)),
    );
    let (l, r) = (json(l) + "\n", json(r) + "\n");
    if l != r {
        fail(
            "left == right",
            DiffView::new(&l, &r).titles("left", "right"),
            message,
        );
    }
}

/// Render `renderable` at `width` through a plain, deterministic target.
/// Trailing whitespace on each line and trailing blank lines are dropped.
pub fn render_plain(renderable: &dyn Renderable, width: usize) -> String {
    let target = RenderTarget::new(
        TargetKind::PlainStream,
        TargetCapabilities {
            width,
            height: 10_000,
            color_system: None,
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        crate::theme::extended_theme(),
    );
    normalize(&target.console().render_to_string(renderable))
}

fn normalize(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |i| i + 1);
    let mut out = lines[..end].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// [`assert_render_eq!`](crate::assert_render_eq)'s body.
#[track_caller]
pub fn assert_render_eq(
    renderable: &dyn Renderable,
    expected: &str,
    width: usize,
    message: Option<String>,
) {
    let actual = render_plain(renderable, width);
    let expected = normalize(expected);
    if actual != expected {
        fail(
            "rendered == expected",
            DiffView::new(&expected, &actual).titles("expected", "rendered"),
            message,
        );
    }
}

/// [`assert_snapshot_eq!`](crate::assert_snapshot_eq)'s body: the ANSI
/// diff (style-only lines marked `~`) and the first differing field.
#[track_caller]
pub fn assert_snapshot_eq(left: &RenderSnapshot, right: &RenderSnapshot, message: Option<String>) {
    if let Some(difference) = left.diff(right) {
        let message = match message {
            Some(m) => format!("{m}\n{difference}"),
            None => difference,
        };
        fail(
            "left == right",
            DiffView::snapshots(left, right).titles("left", "right"),
            Some(message),
        );
    }
}

/// Assert two strings are equal; on failure, panic with a rendered line diff.
///
/// ```should_panic
/// rich_ext::assert_rich_eq!("a\nb\n", "a\nc\n", "values for {}", "case 1");
/// ```
#[macro_export]
macro_rules! assert_rich_eq {
    ($left:expr, $right:expr $(,)?) => {
        $crate::diff::assert::assert_str_eq(
            ::core::convert::AsRef::<str>::as_ref(&$left),
            ::core::convert::AsRef::<str>::as_ref(&$right),
            ::core::option::Option::None,
        )
    };
    ($left:expr, $right:expr, $($arg:tt)+) => {
        $crate::diff::assert::assert_str_eq(
            ::core::convert::AsRef::<str>::as_ref(&$left),
            ::core::convert::AsRef::<str>::as_ref(&$right),
            ::core::option::Option::Some(::std::format!($($arg)+)),
        )
    };
}

/// Assert two `Serialize` values are equal as JSON; on failure, panic with a
/// diff of both pretty-printed.
///
/// ```
/// rich_ext::assert_rich_json_eq!(vec![1, 2], [1, 2]);
/// ```
#[macro_export]
macro_rules! assert_rich_json_eq {
    ($left:expr, $right:expr $(,)?) => {
        $crate::diff::assert::assert_json_eq(&$left, &$right, ::core::option::Option::None)
    };
    ($left:expr, $right:expr, $($arg:tt)+) => {
        $crate::diff::assert::assert_json_eq(
            &$left,
            &$right,
            ::core::option::Option::Some(::std::format!($($arg)+)),
        )
    };
}

/// Assert a renderable renders as `expected` at `width` (default 80)
/// through a plain, deterministic target; trailing whitespace is ignored.
///
/// ```
/// rich_ext::assert_render_eq!(rich::Text::new("hello world"), "hello\nworld\n", width = 6);
/// ```
#[macro_export]
macro_rules! assert_render_eq {
    ($renderable:expr, $expected:expr, width = $width:expr $(,)?) => {
        $crate::diff::assert::assert_render_eq(
            &$renderable,
            ::core::convert::AsRef::<str>::as_ref(&$expected),
            $width,
            ::core::option::Option::None,
        )
    };
    ($renderable:expr, $expected:expr, width = $width:expr, $($arg:tt)+) => {
        $crate::diff::assert::assert_render_eq(
            &$renderable,
            ::core::convert::AsRef::<str>::as_ref(&$expected),
            $width,
            ::core::option::Option::Some(::std::format!($($arg)+)),
        )
    };
    ($renderable:expr, $expected:expr $(,)?) => {
        $crate::assert_render_eq!($renderable, $expected, width = 80)
    };
    ($renderable:expr, $expected:expr, $($arg:tt)+) => {
        $crate::diff::assert::assert_render_eq(
            &$renderable,
            ::core::convert::AsRef::<str>::as_ref(&$expected),
            80,
            ::core::option::Option::Some(::std::format!($($arg)+)),
        )
    };
}

/// Assert two [`RenderSnapshot`]s are equal; on failure, panic with their
/// ANSI diff (style-only changes marked `~`) and the first differing field.
#[macro_export]
macro_rules! assert_snapshot_eq {
    ($left:expr, $right:expr $(,)?) => {
        $crate::diff::assert::assert_snapshot_eq(&$left, &$right, ::core::option::Option::None)
    };
    ($left:expr, $right:expr, $($arg:tt)+) => {
        $crate::diff::assert::assert_snapshot_eq(
            &$left,
            &$right,
            ::core::option::Option::Some(::std::format!($($arg)+)),
        )
    };
}
