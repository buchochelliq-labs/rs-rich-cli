//! [`RatatuiComponent`]: ratatui widgets inside rs-rich-interact (the
//! `interact` feature).

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect;
use rich_interact::{Component, Context, Event, Flow, View};

use crate::buffer::buffer_to_lines;

type Draw<S> = Box<dyn Fn(&S, Rect, &mut Buffer)>;
type Handler<S, T> = Box<dyn FnMut(&mut S, &Event) -> Flow<T>>;

/// A [`rich_interact::Component`] whose view is drawn by ratatui.
///
/// It holds a state `S`, a draw function that renders it with ratatui
/// widgets into a buffer, and an optional event handler that updates it, so
/// the state lives in the component and not behind an `Rc<RefCell>`:
///
/// ```
/// use ratatui_core::widgets::Widget;
/// use ratatui_widgets::block::Block;
/// use ratatui_widgets::paragraph::Paragraph;
/// use rich_interact::headless::{self, Script};
/// use rich_interact::{Event, Flow, KeyCode, Outcome};
/// use rich_ratatui::RatatuiComponent;
///
/// let counter = RatatuiComponent::with_state(0u32, |n, area, buf| {
///     Paragraph::new(format!("pressed {n}"))
///         .block(Block::bordered().title("counter"))
///         .render(area, buf);
/// })
/// .on_event(|n, event| match event {
///     Event::Key(key) if key.code == KeyCode::Enter => Flow::Done(*n),
///     Event::Key(_) => {
///         *n += 1;
///         Flow::Continue
///     }
///     _ => Flow::Ignored,
/// })
/// .height(3);
///
/// // Under the headless driver; `rich_interact::run` drives a terminal.
/// let (outcome, record) = headless::run(counter, Script::new().keys("a b enter"), 30, 10);
/// assert!(matches!(outcome.unwrap(), Outcome::Done(2)));
/// assert!(record.last_frame().contains("pressed 2"));
/// ```
///
/// On render, the draw function gets a fresh [`Buffer`] the size of the
/// context (or [`height`](Self::height) rows), and the buffer becomes the
/// view through [`buffer_to_lines`], with its losses (the underline colour;
/// `Reset` read as unset). Without a handler every event is
/// [`Flow::Ignored`], so a container can route it elsewhere.
pub struct RatatuiComponent<S, T> {
    state: S,
    draw: Draw<S>,
    handler: Option<Handler<S, T>>,
    height: Option<u16>,
}

impl<T> RatatuiComponent<(), T> {
    /// A stateless component: `draw` gets only the area and buffer.
    pub fn new(draw: impl Fn(Rect, &mut Buffer) + 'static) -> RatatuiComponent<(), T> {
        RatatuiComponent::with_state((), move |_, area, buf| draw(area, buf))
    }
}

impl<S, T> RatatuiComponent<S, T> {
    /// A component drawing `state` with `draw`.
    pub fn with_state(
        state: S,
        draw: impl Fn(&S, Rect, &mut Buffer) + 'static,
    ) -> RatatuiComponent<S, T> {
        RatatuiComponent {
            state,
            draw: Box::new(draw),
            handler: None,
            height: None,
        }
    }

    /// Handle events with `handler`, which may update the state.
    pub fn on_event(
        mut self,
        handler: impl FnMut(&mut S, &Event) -> Flow<T> + 'static,
    ) -> RatatuiComponent<S, T> {
        self.handler = Some(Box::new(handler));
        self
    }

    /// Draw `rows` rows instead of the context's full height: for an inline
    /// widget that should not take the whole terminal.
    pub fn height(mut self, rows: u16) -> RatatuiComponent<S, T> {
        self.height = Some(rows);
        self
    }

    /// The component's state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// The component's state, to change between runs.
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Draw into a fresh buffer of `width` × `height` at the origin and
    /// return it.
    pub fn draw(&self, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        (self.draw)(&self.state, area, &mut buffer);
        buffer
    }
}

/// Clamp a context dimension to ratatui's `u16`.
fn dimension(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

impl<S, T> Component for RatatuiComponent<S, T> {
    type Output = T;

    fn handle(&mut self, event: &Event, _context: &Context<'_>) -> Flow<T> {
        match &mut self.handler {
            Some(handler) => handler(&mut self.state, event),
            None => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let height = self.height.unwrap_or(dimension(context.height));
        View::new(buffer_to_lines(
            &self.draw(dimension(context.width), height),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::widgets::Widget;
    use ratatui_widgets::block::Block;
    use ratatui_widgets::paragraph::Paragraph;
    use rich_interact::headless::{self, Script};
    use rich_interact::{KeyCode, Outcome};

    #[test]
    fn ratatui_paragraph_runs_headless() {
        let component = RatatuiComponent::new(|area, buf| {
            Paragraph::new("hello ratatui")
                .block(Block::bordered().title("demo"))
                .render(area, buf);
        })
        .on_event(|_, event| match event {
            Event::Key(key) if key.code == KeyCode::Enter => Flow::Done("done"),
            _ => Flow::Ignored,
        });
        let (outcome, record) = headless::run(component, Script::new().keys("x enter"), 30, 4);
        assert!(matches!(outcome.unwrap(), Outcome::Done("done")));
        let frame = record.last_frame();
        assert!(frame.contains("hello ratatui"), "{frame}");
        assert!(frame.contains("┌demo"), "{frame}");
        assert!(frame.contains('┘'), "{frame}");
    }

    #[test]
    fn stateful_component_updates() {
        let component = RatatuiComponent::with_state(0u32, |n, area, buf| {
            Paragraph::new(format!("count {n}")).render(area, buf);
        })
        .on_event(|n, event| match event {
            Event::Key(key) if key.code == KeyCode::Enter => Flow::Done(*n),
            Event::Key(_) => {
                *n += 1;
                Flow::Continue
            }
            _ => Flow::Ignored,
        })
        .height(1);
        let (outcome, record) = headless::run(component, Script::new().keys("a b c enter"), 20, 5);
        assert!(matches!(outcome.unwrap(), Outcome::Done(3)));
        assert!(record.last_frame().contains("count 3"));
    }
}
