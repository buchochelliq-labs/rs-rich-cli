//! Clickable spans of markup in a [`text`](crate::text) or
//! [`label`](crate::label), after Textual's `[@click=…]`: a span tagged
//! `[@click=name]…[/]` runs the action `name` when it is clicked
//! ([`Ctx::run_action`]), and with [`Node::on_span`] or
//! [`Node::focus_spans`] a `[link=url]…[/]` span follows its link.
//! [`Node::focus_spans`] also lets the text take the focus, ← and → move
//! between its spans and Enter or Space runs the one picked.

use rich::{MetaValue, Segment, Style};
use rich_interact::{Button, KeyCode, MouseKind};

use crate::app::Ctx;
use crate::node::Node;
use crate::widget::{Canvas, DrawCx, EventCx, Used, WidgetEvent};

/// What a clickable span of markup does, as [`Node::on_span`] is told it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpanTarget {
    /// `[@click=name]` or `[@click=name('a', 2)]`: an action, with its
    /// arguments (a string as it is, anything else as Python writes it).
    Action { name: String, args: Vec<String> },
    /// `[link=url]`: a hyperlink.
    Link(String),
}

type OnSpan = Box<dyn FnMut(&mut Ctx, &SpanTarget)>;

/// A span's place on the text's last draw: its row, its first column and
/// the column after it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Placed {
    row: u16,
    start: u16,
    end: u16,
    target: SpanTarget,
}

/// What a text node keeps of its spans.
#[derive(Default)]
pub(crate) struct Spans {
    placed: Vec<Placed>,
    on_span: Option<OnSpan>,
    /// Links are followed and the text takes the focus.
    focus: bool,
    /// The span picked with the keys, while the text has the focus.
    picked: Option<usize>,
}

/// The action an `@click` meta value names: a handler call
/// (`("name", (args…))`), or a bare value.
fn action(value: &MetaValue) -> SpanTarget {
    let text = |value: &MetaValue| match value {
        MetaValue::Str(s) => s.clone(),
        other => other.repr(),
    };
    match value {
        MetaValue::Tuple(parts) if parts.len() == 2 => match (&parts[0], &parts[1]) {
            (MetaValue::Str(name), MetaValue::Tuple(args)) => SpanTarget::Action {
                name: name.clone(),
                args: args.iter().map(text).collect(),
            },
            _ => SpanTarget::Action {
                name: text(value),
                args: Vec::new(),
            },
        },
        other => SpanTarget::Action {
            name: text(other),
            args: Vec::new(),
        },
    }
}

/// The span a segment's style makes, if any: an `@click` first, else a
/// link when links are followed.
fn target(style: &Style, links: bool) -> Option<SpanTarget> {
    let meta = style.meta();
    if let Some(value) = meta.get("@click") {
        return Some(action(value));
    }
    match style.link() {
        Some(url) if links && !url.is_empty() => Some(SpanTarget::Link(url.to_string())),
        _ => None,
    }
}

impl Spans {
    fn links(&self) -> bool {
        self.focus || self.on_span.is_some()
    }

    /// Find the spans in the lines just drawn; with the focus, show the
    /// picked one in the theme's `link.focus` style (reversed).
    pub(crate) fn drawn(&mut self, cx: &DrawCx, canvas: &mut Canvas, lines: &[Vec<Segment>]) {
        let links = self.links();
        let mut placed: Vec<Placed> = Vec::new();
        for (row, line) in lines.iter().enumerate() {
            let row = row.min(u16::MAX as usize) as u16;
            let mut x = 0usize;
            for segment in line {
                let cells = segment.cell_length();
                if let Some(target) = segment.style.as_ref().and_then(|s| target(s, links)) {
                    let (start, end) = (x.min(65535) as u16, (x + cells).min(65535) as u16);
                    match placed.last_mut() {
                        // A span split into segments by the styles inside it.
                        Some(last)
                            if last.row == row && last.end == start && last.target == target =>
                        {
                            last.end = end;
                        }
                        _ => placed.push(Placed {
                            row,
                            start,
                            end,
                            target,
                        }),
                    }
                }
                x += cells;
            }
        }
        self.placed = placed;
        if self.placed.is_empty() {
            self.picked = None;
        } else if let Some(picked) = self.picked {
            self.picked = Some(picked.min(self.placed.len() - 1));
        }
        if !(self.focus && cx.focused()) {
            return;
        }
        let picked = *self.picked.get_or_insert(0);
        if let Some(span) = self.placed.get(picked) {
            let style = cx.style("link.focus", "reverse");
            canvas.restyle(span.start, span.row, span.end - span.start, 1, &style);
        }
    }

    /// Run `span`: the node's handler, else the action by name or the
    /// link through the system's opener.
    fn run(&mut self, cx: &mut Ctx, span: usize) {
        let Some(target) = self.placed.get(span).map(|p| p.target.clone()) else {
            return;
        };
        if let Some(handler) = &mut self.on_span {
            handler(cx, &target);
            return;
        }
        match target {
            SpanTarget::Action { name, .. } => cx.run_action(&name),
            SpanTarget::Link(url) => {
                let _ = rich_ext::open::system_opener().arg(url).spawn();
            }
        }
    }

    pub(crate) fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Down(Button::Left) | MouseKind::Up(Button::Left)
                ) =>
            {
                let at = self.placed.iter().position(|span| {
                    span.row == mouse.row && (span.start..span.end).contains(&mouse.column)
                });
                let Some(at) = at else {
                    return Used::No;
                };
                if mouse.kind == MouseKind::Up(Button::Left) {
                    if self.focus {
                        self.picked = Some(at);
                        cx.redraw();
                    }
                    self.run(cx.app(), at);
                }
                Used::Yes
            }
            WidgetEvent::Key(key) if self.focus && !key.modifiers.ctrl && !key.modifiers.alt => {
                let count = self.placed.len();
                if count == 0 {
                    return Used::No;
                }
                let picked = self.picked.unwrap_or(0).min(count - 1);
                match key.code {
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        self.run(cx.app(), picked);
                        Used::Yes
                    }
                    KeyCode::Right if picked + 1 < count => {
                        self.picked = Some(picked + 1);
                        cx.redraw();
                        Used::Yes
                    }
                    KeyCode::Left if picked > 0 => {
                        self.picked = Some(picked - 1);
                        cx.redraw();
                        Used::Yes
                    }
                    _ => Used::No,
                }
            }
            _ => Used::No,
        }
    }
}

impl Node {
    /// For a [`text`](crate::text) or [`label`](crate::label): run
    /// `handler` when one of its spans is clicked (or, with
    /// [`focus_spans`](Self::focus_spans), picked with Enter), with what
    /// the span does, in place of running the action or opening the link.
    /// With a handler, `[link=url]` spans are clickable too. Ignored on
    /// other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::SpanTarget;
    ///
    /// let app = App::new(|| {
    ///     let said = signal(String::new());
    ///     column([
    ///         label("[@click=greet('Ada')]hello[/] [link=https://example.com]docs[/]").on_span(
    ///             move |_, target| match target {
    ///                 SpanTarget::Action { name, args } => said.set(format!("{name} {}", args[0])),
    ///                 SpanTarget::Link(url) => said.set(url.clone()),
    ///                 _ => {}
    ///             },
    ///         ),
    ///         text!("{said}"),
    ///     ])
    /// });
    /// let mut driver = app.driver(30, 2);
    /// assert!(driver.click_text("hello"));
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "greet Ada");
    /// assert!(driver.click_text("docs"));
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "https://example.com");
    /// ```
    pub fn on_span(self, handler: impl FnMut(&mut Ctx, &SpanTarget) + 'static) -> Node {
        self.widget_mut(|text: &mut crate::builtin::TextNode| {
            text.spans.on_span = Some(Box::new(handler));
        });
        self
    }

    /// For a [`text`](crate::text) or [`label`](crate::label) with
    /// clickable spans: let it take the focus, as Textual's links do. With
    /// the focus, ← and → pick a span (drawn in the theme's `link.focus`
    /// style, reversed by default) and Enter or Space runs it; its
    /// `[link=url]` spans open through the system's opener (rs-rich-ext's
    /// `open::system_opener`) unless [`on_span`](Self::on_span) takes
    /// them. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let n = signal(0);
    ///     column([
    ///         label("[@click=add]add[/] [@click=reset]reset[/]").focus_spans(),
    ///         text!("{n}"),
    ///     ])
    ///     .bind_hidden("+", "add", move |_| n.update(|n| *n += 1))
    ///     .bind_hidden("0", "reset", move |_| n.set(0))
    /// });
    /// let mut driver = app.driver(20, 2);
    /// driver.press("tab enter enter");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "2");
    /// driver.press("right enter");
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "0");
    /// ```
    pub fn focus_spans(self) -> Node {
        let mut is_text = false;
        self.widget_mut(|text: &mut crate::builtin::TextNode| {
            text.spans.focus = true;
            is_text = true;
        });
        if is_text { self.focusable() } else { self }
    }
}
