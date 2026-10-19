//! intuiTUIve: a reactive, retained terminal UI framework built on rs-rich.
//!
//! You describe the screen once, as a tree of nodes, and keep your state in
//! [signals](signal). When a signal changes, the nodes that read it draw
//! again, and only the cells that changed are sent to the terminal. There
//! is no draw loop to write, no layout pass to call, and no "what changed"
//! to track.
//!
//! ```no_run
//! use intuituive::prelude::*;
//!
//! fn main() -> std::io::Result<()> {
//!     App::new(|| {
//!         let count = signal(0);
//!         column([
//!             text!("[b]Count:[/] {count}").panel("Counter"),
//!             label("[dim]+ adds one · q quits"),
//!         ])
//!         .on_key("+", move |_| count.update(|c| *c += 1))
//!         .on_key("q", |cx| cx.quit())
//!     })
//!     .run()
//! }
//! ```
//!
//! - **Nodes** ([`text`](fn@text), [`label`], [`renderable`], [`column`](fn@column), [`row`],
//!   [`grid`], [`each`], [`switch`], [`Node::panel`]) are built once and
//!   kept. Any rich renderable is a node: a `Table`, `Markdown`, `Syntax`, a
//!   chart.
//! - **Layout** sizes children by [`Size`] (fixed, percentage, flexible or
//!   their content), within minimums and maximums, with gaps and padding.
//! - **State** is [`signal`]s and [`memo`]s. Reading one while drawing
//!   subscribes the node; writing one redraws exactly its readers.
//! - **Input**: [`Node::on_key`] bindings bubble from the focused node up;
//!   Tab moves the focus; [`Node::on_click`] gets clicks, routed by the
//!   layout the last frame kept.
//! - **Screens**: [`Ctx::push`], [`Ctx::modal`] and [`Ctx::pop`] keep a
//!   stack of screens, each with its own state and focus.
//! - **Background work**: [`spawn`], [`spawn_future`] and [`resource`] run
//!   slow work off the app's thread and write signals with the result; a
//!   [`Proxy`] lets any thread do the same.
//! - **Themes**: [`Theme`] styles the framework's parts and names styles
//!   for markup; [`Ctx::set_theme`] switches at run time.
//! - **Inline**: [`App::inline`] runs in a few rows below the prompt
//!   instead of the alternate screen.
//! - **Responsive**: [`Node::hide_below`] and a stylesheet's `@media`
//!   rules follow the terminal's size; [`viewport`] reads it.
//! - **The outside world**, behind off-by-default features (plugins: the
//!   core stays generic): `audio::pcm` reads raw PCM a buffer at a time,
//!   `audio::play` plays it at its rate and `audio::Capture::simulated`
//!   stands in for a sound card in tests (`pcm`), `audio::input` captures
//!   a sound card into a signal
//!   (`audio`, with PulseAudio sources under `pulse`),
//!   `audio::wav` reads and records WAV files (`wav`), `watch_path` wakes
//!   the app when files change (`watch`), `image` draws an image file
//!   in half blocks (`art`), `meter`, `sparkline`, `spectrogram` and
//!   `bar_spectrum` show levels and spectra (`viz`), and [`files`] has a
//!   file manager's components: Miller columns, file lists, previews,
//!   background file jobs and a streaming finder (`files`).
//! - **Generic building blocks**: [`miller_of`] (Miller columns over any
//!   [`MillerSource`] tree), [`jobs`](fn@jobs) and [`jobs_panel`]
//!   (cancellable work with progress), [`fuzzy_list`] and [`finder_of`]
//!   (a fuzzy list over any stream of items), [`preview_of`] (a value
//!   loaded off the app's thread for the current key), [`multi_select`]
//!   (a selection by key with a visual range), [`list_search`] (find next
//!   and previous over a list), [`tab_set`](fn@tab_set) (tabs' states with one
//!   active), [`facts`] (a description list), [`digits`] (big digits
//!   in any build), and [`state`]'s files and [`FrecencyStore`] (an
//!   app's state between sessions). The core is generic only; the domain
//!   plugins above build on these.
//! - **Tests**: [`App::render_with`] and [`App::run_on`] with
//!   `rich-interact`'s headless driver run an app without a terminal;
//!   [`App::driver`] steps one, and [`assert_screen!`] compares its screen
//!   with a snapshot (see [`testing`]).
//!
//! The design, and how it compares with ratatui, is in
//! `docs/design/intuituive.md`.

pub mod a11y;
pub mod app;
#[cfg(feature = "pcm")]
pub mod audio;
mod bars;
#[cfg(feature = "art")]
mod big_text;
mod bindings;
pub mod border;
mod builtin;
mod calendar;
mod datetime;
mod dialog;
mod digits;
mod effect;
mod facts;
mod feed;
#[cfg(feature = "files")]
pub mod files;
pub mod finder;
mod help;
mod hex;
#[cfg(feature = "art")]
mod image;
mod input;
pub mod inspect;
pub mod jobs;
pub mod layout;
mod list;
pub mod log;
pub mod menu;
pub mod miller;
pub mod node;
mod notify;
mod place;
pub mod preview;
pub mod reactive;
mod reorder;
mod rule;
pub mod screen;
mod scrollbar;
pub mod select;
mod sequence;
mod sheet;
mod signals;
mod solver;
mod split;
pub mod state;
pub mod tab_set;
mod table_scroll;
pub mod task;
pub mod testing;
mod text_area;
pub mod transport;
mod tree;
#[cfg(feature = "viz")]
mod viz;
#[cfg(feature = "watch")]
mod watch;
pub mod widget;
pub mod widgets;

/// rs-rich, for renderables, styles and markup helpers, so an app needs
/// only this crate as a dependency.
pub use rich;
/// rs-rich-interact, for its components (`interact::Input`,
/// `interact::Select`, …) and the headless test driver.
pub use rich_interact as interact;

pub use app::{
    after, every, rate, Anchor, App, Ctx, Driver, Easing, FrameStats, Handoff, ModalOptions,
    Placement, Theme,
};
#[cfg(feature = "art")]
pub use big_text::big_text;
pub use bindings::{ModePage, Scaled, Stepped};
pub use border::{Border, BorderSet, Edge, Shadow, Sides};
pub use dialog::{ConfirmOptions, PickOptions, PromptOptions};
pub use digits::digits;
pub use effect::{effect, fill};
pub use facts::{facts, facts_of};
pub use feed::{
    latest_feed, latest_iter, latest_with_state, Feed, FeedEnd, FeedState, FeedTeller, Pace, Play,
};
pub use finder::{finder_of, fuzzy_list, fuzzy_list_with, FuzzyOptions, FuzzySource};
pub use help::{HelpOptions, KeyFormat};
#[cfg(feature = "art")]
pub use image::{image, image_of, image_with, ImageAnchor, ImageFit, ImageMode, ImageView};
pub use input::{input, number_input};
pub use jobs::{
    jobs, jobs_panel, jobs_panel_with, Job, JobOutcome, JobProgress, JobState, Jobs,
    JobsPanelOptions, ProcessOutcome, ProcessOutput,
};
pub use layout::{Align, AlignContent, Justify, Size, StackLayout, TextAlign};
pub use list::list_optional;
pub use log::Log;
pub use miller::{miller_in, miller_of, MillerSource};
pub use node::described;
pub use node::{
    column, component, each, grid, label, layers, leaf, list, renderable, repeating, row, scroll,
    scroll_both, scroll_both_with, scroll_with, scroll_x, switch, text, Node,
};
pub use notify::{Corner, Level};
pub use place::{Offset, Origin, RectHandle};
pub use preview::{preview_of, preview_of_with, PreviewOfOptions};
pub use reactive::{memo, on_write, on_write_ref, signal, viewport, watch, Memo, Proxy, Signal};
/// What a text does with what does not fit a line: rich's.
pub use rich::Overflow;
pub use rule::rule;
pub use select::{list_search, multi_select, ListSearch, Mark, MultiSelect};
pub use sheet::{SheetError, Stylesheet};
pub use signals::{falling, history, recent, recent_of, recent_timed, smoothed, Count, History};
pub use solver::Solver;
pub use state::{
    frecency_in, frecency_session, frecency_store, read_state, state_dir, state_file, update_file,
    update_state, write_atomic, write_state, FrecencyStore,
};
pub use tab_set::{tab_set, tabs_for, tabs_for_indexed, Scoped, TabSet};
pub use task::{
    latest, latest_every, resource, resource_for, spawn, spawn_future, spawn_stream, spawn_then,
    Latest, Load, Notifier, Resource, Stream, Task,
};
pub use text_area::Caret;
pub use transport::{transport, Transport};
#[cfg(feature = "viz")]
pub use viz::{
    bar_spectrum, bar_spectrum_with, meter, meter_with, persistence, persistence_with, sparkline,
    sparkline_with, spectrogram, spectrogram_with, tuner,
};
#[cfg(feature = "watch")]
pub use watch::{
    watch_path, watch_path_with, ChangeKind, PathChange, PathEvent, PathWatch, WatchOptions,
};
pub use widget::{widget, Widget};
pub use widgets::text_area;

/// Everything an app usually needs.
pub mod prelude {
    // `text` is both the function and the `text!` macro.
    pub use crate::described;
    pub use crate::{
        after, column, component, each, effect, every, fill, grid, input, label, latest,
        latest_every, layers, leaf, list, list_optional, memo, number_input, on_write,
        on_write_ref, rate, renderable, repeating, resource, resource_for, row, scroll,
        scroll_both, scroll_with, scroll_x, signal, spawn, switch, text, text_area, viewport,
        watch, widget, Align, App, Border, Ctx, Edge, Justify, Load, Log, Memo, Node, Overflow,
        Proxy, Resource, Shadow, Sides, Signal, Size, Task, TextAlign, Theme, Widget,
    };
}

/// Console markup with `format!` arguments, read again whenever a signal it
/// names changes: `text!("{count} items")`. Signals format as their value.
#[macro_export]
macro_rules! text {
    ($($arg:tt)*) => {
        $crate::node::text(move || format!($($arg)*))
    };
}

impl<T: std::fmt::Display + 'static> std::fmt::Display for Signal<T> {
    /// The value, subscribing the node that is drawing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|value| value.fmt(f))
    }
}

impl<T: std::fmt::Display + 'static> std::fmt::Display for Memo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|value| value.fmt(f))
    }
}
