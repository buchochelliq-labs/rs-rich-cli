//! Where the focus is, as state, and where it goes next by geometry: the
//! reactive reads [`focused`] and [`has_focus`], [`Direction`] for
//! [`Ctx::focus_direction`](crate::Ctx::focus_direction), and the pure
//! [`neighbour`] search under it, for widgets that place parts of their
//! own.

use crate::reactive::{NodeId, Runtime};
use crate::screen::Rect;

/// The node that has the focus on the top screen, read so that what reads
/// it (a text closure, a memo, a [`watch`](crate::watch), a size) follows
/// the focus as it moves. `None` with nothing focused, or outside an app.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::focus::focused;
///
/// let app = App::new(|| {
///     let list = label("list").focusable();
///     let id = list.id();
///     column([
///         label("menu").focusable(),
///         list,
///         text(move || match focused() == Some(id) {
///             true => "on the list".into(),
///             false => "elsewhere".into(),
///         }),
///     ])
/// });
/// let mut driver = app.driver(12, 3);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[2].trim_end(), "elsewhere");
/// driver.press("tab");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "on the list");
/// ```
pub fn focused() -> Option<NodeId> {
    let path = Runtime::try_current().and_then(|runtime| runtime.focus.get())?;
    path.with(|path| path.last().copied())
}

/// Whether the focus is on node `id` or on a node inside it, read as
/// [`focused`] is: a pane's title lit while the focus is anywhere in the
/// pane, a diff that follows the list last focused.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::focus::has_focus;
///
/// let app = App::new(|| {
///     let pane = column([label("a").focusable(), label("b").focusable()]);
///     let id = pane.id();
///     column([
///         label("top").focusable(),
///         pane,
///         text(move || format!("in pane: {}", has_focus(id))),
///     ])
/// });
/// let mut driver = app.driver(16, 4);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[3].trim_end(), "in pane: false");
/// driver.press("tab tab");
/// assert_eq!(driver.screen().plain()[3].trim_end(), "in pane: true");
/// ```
pub fn has_focus(id: NodeId) -> bool {
    Runtime::try_current()
        .and_then(|runtime| runtime.focus.get())
        .is_some_and(|path| path.with(|path| path.contains(&id)))
}
