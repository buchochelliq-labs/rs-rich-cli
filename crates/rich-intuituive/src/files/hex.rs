//! A hex dump as a node.

use rich_ext::hex::HexView;

use crate::node::{renderable, scroll_with, Node};
use crate::reactive::Signal;

/// The bytes `bytes` gives (which may read signals) as a hex dump,
/// `hexdump -C`'s layout through rs-rich-ext's [`HexView`]: offsets, the
/// bytes in groups, an ASCII panel, as many bytes a line as the width
/// fits, runs of equal lines collapsed. It scrolls; `top` is its first
/// line in view (both ways).
///
/// ```
/// use intuituive::files::hex_view;
/// use intuituive::prelude::*;
///
/// let app = App::new(|| hex_view(|| b"PNG\x00".to_vec(), signal(0)).on_key("q", |cx| cx.quit()));
/// let screen = app.render_with(&["q"], 80, 2).unwrap();
/// assert!(screen[0].starts_with("00000000  50 4e 47 00"), "{screen:?}");
/// assert!(screen[0].contains("│PNG.│"), "{screen:?}");
/// ```
pub fn hex_view(bytes: impl Fn() -> Vec<u8> + 'static, top: Signal<u16>) -> Node {
    scroll_with(
        renderable(move || HexView::new(bytes()).collapse(true)),
        top,
    )
}
