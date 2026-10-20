//! A section that opens and closes: [`collapsible`].

use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::{AccessState, Role};
use crate::controls::{look, plain};
use crate::node::{column, Axis, Node};
use crate::reactive::Signal;
use crate::widget::{
    markup_width, widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent,
};

/// What a [`collapsible`]'s header is.
struct Header {
    title: String,
    open: Signal<bool>,
}

/// A section titled `title` (console markup) over `child`, shown while
/// `open` holds: a header with a disclosure marker, `▾ Advanced` open and
/// `▸ Advanced` closed (`[-]` and `[+]` in text mode). Enter, Space or a
/// click on the header opens or closes it; → opens it and ← closes it.
/// Closed, the child takes no room and leaves the Tab order. The title is
/// drawn in the theme's `collapsible.title` style (else `bold`), with
/// `control.focus` over it while the header has the focus. The header's
/// role is [`Button`](crate::a11y::Role::Button), named by the title, with
/// its expanded state. The section is as tall as what it shows.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::collapsible;
///
/// let app = App::new(|| {
///     collapsible("Advanced", signal(false), label("Cache size: 64 MB"))
/// });
/// let mut driver = app.driver(20, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "▸ Advanced");
/// assert_eq!(driver.accessibility()[0].describe(), "Advanced, button, collapsed");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "Cache size: 64 MB");
/// assert_eq!(driver.accessibility()[0].describe(), "Advanced, button, expanded");
/// ```
pub fn collapsible(title: impl Into<String>, open: Signal<bool>, child: Node) -> Node {
    let header = widget(Header {
        title: title.into(),
        open,
    })
    .auto();
    column([header, child.shown_when(move || open.get())])
        .auto()
        .name("collapsible")
}

impl Widget for Header {
    fn name(&self) -> &'static str {
        "collapsible header"
    }

    fn describe(&self) -> Option<String> {
        Some(plain(&self.title))
    }

    fn role(&self) -> Role {
        Role::Button
    }

    fn access_text(&self) -> Option<String> {
        Some(plain(&self.title))
    }

    fn access_state(&self) -> AccessState {
        AccessState {
            expanded: Some(self.open.get_untracked()),
            ..AccessState::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => markup_width(&self.title).saturating_add(Self::marker(true).1 + 1),
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let open = self.open.get();
        let (marker, cells) = Self::marker(open);
        let style = look(cx, &["collapsible.title"], "bold");
        canvas.print(0, 0, marker, Some(&style));
        let room = canvas.width().saturating_sub(cells + 1);
        let shown = canvas.markup(cx.console(), cells + 1, 0, room, &self.title, Some(&style));
        let over = match (cx.disabled(), cx.focused()) {
            (true, _) => Some(cx.style("control.disabled", "dim")),
            (false, true) => Some(cx.style("control.focus", "reverse")),
            (false, false) => None,
        };
        if let Some(over) = over {
            canvas.restyle(0, 0, cells + 1 + shown, 1, &over);
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let open = self.open.get_untracked();
        let to = match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Enter | KeyCode::Char(' ') => !open,
                KeyCode::Right if !open => true,
                KeyCode::Left if open => false,
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => !open,
            _ => return Used::No,
        };
        self.open.set(to);
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Header {
    /// The disclosure marker, and its cells.
    fn marker(open: bool) -> (&'static str, u16) {
        match (crate::a11y::text_mode(), open) {
            (false, true) => ("▾", 1),
            (false, false) => ("▸", 1),
            (true, true) => ("[-]", 3),
            (true, false) => ("[+]", 3),
        }
    }
}
