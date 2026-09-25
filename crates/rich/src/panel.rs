//! Panels — a box drawn around a renderable.
//!
//! Port of upstream `rich/panel.py`. A [`Panel`] frames a child renderable with
//! a box border, inner padding, and an optional centered title.
//!
//! Slice scope: title + subtitle (with alignment), box + border style +
//! padding, `expand` (and [`Panel::fit`]) and a fixed `width`.

use crate::align::HorizontalAlign;
use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::padding::join_rows;
use crate::protocol::Renderable;
use crate::r#box::{Box as BoxSet, ROUNDED};
use crate::segment::Segment;
use crate::style::Style;
use crate::text::{Text, DEFAULT_TAB_SIZE};

/// A bordered box around a renderable. Mirrors `rich.panel.Panel`.
pub struct Panel {
    child: Box<dyn Renderable>,
    box_set: BoxSet,
    title: Option<String>,
    title_align: HorizontalAlign,
    subtitle: Option<String>,
    subtitle_align: HorizontalAlign,
    padding: (usize, usize, usize, usize),
    border_style: Style,
    style: Style,
    expand: bool,
    width: Option<usize>,
    highlight: bool,
}

impl Panel {
    /// A panel around `child` with default box (`ROUNDED`) and padding `(0,1)`.
    pub fn new(child: Box<dyn Renderable>) -> Self {
        Panel {
            child,
            box_set: ROUNDED,
            title: None,
            title_align: HorizontalAlign::Center,
            subtitle: None,
            subtitle_align: HorizontalAlign::Center,
            padding: (0, 1, 0, 1),
            border_style: Style::new(),
            style: Style::new(),
            expand: true,
            width: None,
            highlight: false,
        }
    }

    /// A panel that fits its content rather than expanding to the available
    /// width. Port of `Panel.fit` (`expand=False`).
    pub fn fit(child: Box<dyn Renderable>) -> Self {
        Panel::new(child).expand(false)
    }

    /// Expand to the full available width (upstream `expand`, default on), or
    /// fit the measured width of the content and title.
    pub fn expand(mut self, expand: bool) -> Self {
        self.expand = expand;
        self
    }

    /// A fixed width for the whole panel, borders included (upstream `width`),
    /// capped at the available width.
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Set a title (drawn into the top border, centered by default).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the title alignment within the top border.
    pub fn title_align(mut self, align: HorizontalAlign) -> Self {
        self.title_align = align;
        self
    }

    /// Set a subtitle (drawn into the bottom border, centered by default).
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Set the subtitle alignment within the bottom border.
    pub fn subtitle_align(mut self, align: HorizontalAlign) -> Self {
        self.subtitle_align = align;
        self
    }

    /// Choose the box-drawing set.
    pub fn box_set(mut self, box_set: BoxSet) -> Self {
        self.box_set = box_set;
        self
    }

    /// Set the inner padding `(top, right, bottom, left)`.
    pub fn padding(mut self, padding: (usize, usize, usize, usize)) -> Self {
        self.padding = padding;
        self
    }

    /// Highlight strings rendered inside the panel (upstream
    /// `Panel(highlight=…)`, default off). Passed to the child as
    /// [`ConsoleOptions::highlight`].
    pub fn highlight(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }

    /// Set the border style.
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Build a top/bottom border. Port of `Panel._title`, `_subtitle` and
    /// `align_text`: markup is styled before its visible cell width is measured.
    fn border_line(
        &self,
        console: &Console,
        inner_width: usize,
        corners: (char, char, char),
        label: Option<&String>,
        align: HorizontalAlign,
    ) -> Vec<Segment> {
        let (left_corner, fill_char, right_corner) = corners;
        let border_style = Some(self.border_style.clone());
        let Some(label) = label.filter(|label| !label.is_empty() && inner_width > 2) else {
            return vec![Segment::new(
                format!(
                    "{left_corner}{}{right_corner}",
                    fill_char.to_string().repeat(inner_width)
                ),
                border_style,
            )];
        };

        let mut label = label_text(label);
        label.set_base_style(self.border_style.clone());
        let label_width = inner_width - 2;
        label.truncate(label_width, None, false);

        let fill = label_width.saturating_sub(label.cell_len());
        let (left, right) = match align {
            HorizontalAlign::Center => (fill / 2, fill - fill / 2),
            HorizontalAlign::Left => (0, fill),
            HorizontalAlign::Right => (fill, 0),
        };
        let mut text = Text::styled(
            fill_char.to_string().repeat(left),
            self.border_style.clone(),
        )
        .append_text(&label);
        text.append(
            &fill_char.to_string().repeat(right),
            Some(self.border_style.clone().into()),
        );
        let mut segments = vec![Segment::new(
            format!("{left_corner}{fill_char}"),
            border_style.clone(),
        )];
        segments.extend(text.render(console.theme(), console.base_style()));
        segments.push(Segment::new(
            format!("{fill_char}{right_corner}"),
            border_style,
        ));
        segments
    }
}

/// The title/subtitle `Text`: port of `Panel._title` / `_subtitle`.
/// Text.from_markup expands emoji independently of the console's emoji flag.
/// Preserve markup offsets while flattening newlines to spaces.
fn label_text(label: &str) -> Text {
    let expanded = crate::emoji::replace(label);
    let parsed = Text::from_markup(&expanded).unwrap_or_else(|_| Text::new(expanded));
    let mut text = parsed.blank_copy();
    text.append(&parsed.plain().replace('\n', " "), None);
    for span in parsed.spans() {
        text.push_span(span.clone());
    }
    text.expand_tabs(DEFAULT_TAB_SIZE);
    text.pad(1, ' ');
    text
}

impl Panel {
    /// The title as `Panel._title` builds it, when there is one.
    fn title_text(&self) -> Option<Text> {
        self.title
            .as_deref()
            .filter(|title| !title.is_empty())
            .map(label_text)
    }

    /// `Measurement.get` of the child wrapped in upstream's
    /// `Padding(renderable, padding)` (only when there is any padding).
    fn measure_padded_child(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let (top, right, bottom, left) = self.padding;
        let max_width = options.max_width;
        if max_width < 1 {
            return Measurement::new(0, 0);
        }
        if top == 0 && right == 0 && bottom == 0 && left == 0 {
            return Measurement::get(console, options, self.child.as_ref());
        }
        // `Padding.__rich_measure__`, then `Measurement.get`'s normalization.
        let extra_width = left + right;
        let width = if max_width < extra_width + 1 {
            Measurement::new(max_width, max_width)
        } else {
            let child = Measurement::get(console, options, self.child.as_ref());
            Measurement::new(child.minimum + extra_width, child.maximum + extra_width)
                .with_maximum(max_width)
        };
        let width = width.normalize().with_maximum(max_width);
        if width.maximum < 1 {
            Measurement::new(0, 0)
        } else {
            width.normalize()
        }
    }
}

impl Renderable for Panel {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = match self.width {
            Some(width) => width.min(options.max_width),
            None => options.max_width,
        };
        // Upstream renders nothing at all in no width, not two empty borders.
        if width == 0 {
            return Vec::new();
        }
        // Fall back to a terminal-safe box on legacy Windows / non-UTF-8.
        let box_set = self.box_set.substitute(
            console.legacy_windows(),
            console.safe_box(),
            console.ascii_only(),
        );
        // The padded child fills `width - 2`, or, when not expanding, its
        // measured width; a title may widen it up to the available width.
        let mut inner_width = if self.expand {
            width.saturating_sub(2)
        } else {
            self.measure_padded_child(console, &options.update_width(width.saturating_sub(2)))
                .maximum
        };
        if let Some(title) = self.title_text() {
            inner_width = options
                .max_width
                .saturating_sub(2)
                .min(inner_width.max(title.cell_len() + 2));
        }
        let (pt, pr, pb, pl) = self.padding;
        let child_width = inner_width.saturating_sub(pl).saturating_sub(pr);

        let mut child_options = options.update_width(child_width);
        // `options.update(width=…, height=…, highlight=self.highlight)`.
        child_options.highlight = Some(self.highlight);
        // When a height is imposed (e.g. as a Layout leaf), the child fills the
        // space left by the two borders and the top/bottom padding rows, so the
        // panel expands to exactly `height` rows. Port of `Panel`'s
        // `child_height = height - 2` (padding here lives outside the child).
        child_options.height = options.height.map(|h| h.saturating_sub(2 + pt + pb));
        // Upstream: `console.render_lines(renderable, child_options, style=style)`.
        let child_lines = console.render_lines_styled(
            self.child.as_ref(),
            &child_options,
            Some(&self.style),
            true,
        );

        let border = Some(self.border_style.clone());
        let inner_style = Some(self.style.clone());
        let left_border = || Segment::new(box_set.mid_left.to_string(), border.clone());
        let right_border = || Segment::new(box_set.mid_right.to_string(), border.clone());
        let blank_inner = || Segment::new(" ".repeat(inner_width), inner_style.clone());

        let mut rows: Vec<Vec<Segment>> = Vec::new();

        // Top border (with title if present).
        rows.push(self.border_line(
            console,
            inner_width,
            (box_set.top_left, box_set.top, box_set.top_right),
            self.title.as_ref(),
            self.title_align,
        ));

        // Top padding rows.
        for _ in 0..pt {
            rows.push(vec![left_border(), blank_inner(), right_border()]);
        }

        // Content rows: border + left pad + content + right pad + border.
        for line in child_lines {
            let mut row = vec![left_border()];
            if pl > 0 {
                row.push(Segment::new(" ".repeat(pl), inner_style.clone()));
            }
            row.extend(line);
            if pr > 0 {
                row.push(Segment::new(" ".repeat(pr), inner_style.clone()));
            }
            row.push(right_border());
            rows.push(row);
        }

        // Bottom padding rows.
        for _ in 0..pb {
            rows.push(vec![left_border(), blank_inner(), right_border()]);
        }

        // Bottom border (with subtitle if present).
        rows.push(self.border_line(
            console,
            inner_width,
            (box_set.bottom_left, box_set.bottom, box_set.bottom_right),
            self.subtitle.as_ref(),
            self.subtitle_align,
        ));

        join_rows(rows)
    }

    /// Port of `Panel.__rich_measure__`: the widest of the content and the
    /// title, measured inside the borders and padding, plus both; or the
    /// fixed `width`. Either way the panel asks for exactly one width.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let (_, right, _, left) = self.padding;
        let padding = left + right;
        let width = match self.width {
            Some(width) => width,
            None => {
                // `measure_renderables(console, options.update_width(...),
                // [renderable, _title])`, whose maximum is the widest maximum.
                let inner = options.update_width(options.max_width.saturating_sub(padding + 2));
                let child = Measurement::get(console, &inner, self.child.as_ref()).maximum;
                let title = self
                    .title_text()
                    .map_or(0, |title| Measurement::get(console, &inner, &title).maximum);
                child.max(title) + padding + 2
            }
        };
        Measurement::new(width, width)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn tiny_widths_match_upstream() {
        // Expected output captured from rich 15.0.0 (`Console.print`).
        let render = |panel: Panel, width| {
            let console = Console::builder().width(width).color_system(None).build();
            let out = console.render_to_string(&panel);
            if out.is_empty() {
                out
            } else {
                out + "\n"
            }
        };
        for width in [0, 1, 2] {
            let expected = ["", "╭\n╰\n", "╭╮\n╰╯\n"][width];
            assert_eq!(
                render(Panel::new(Box::new(crate::text::Text::new("hi"))), width),
                expected,
                "width {width}"
            );
            assert_eq!(
                render(Panel::fit(Box::new(crate::text::Text::new("hi"))), width),
                expected,
                "fit width {width}"
            );
        }
    }

    use super::*;
    use crate::r#box::SQUARE;
    use crate::text::Text;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(20)
            .build()
    }

    #[test]
    fn plain_panel() {
        let out = console().render_export(&Panel::new(Box::new(Text::new("hello"))));
        assert_eq!(
            out,
            "╭──────────────────╮\n│ hello            │\n╰──────────────────╯\n"
        );
    }

    #[test]
    fn titled_panel() {
        let out = console().render_export(&Panel::new(Box::new(Text::new("hello"))).title("T"));
        assert_eq!(
            out,
            "╭─────── T ────────╮\n│ hello            │\n╰──────────────────╯\n"
        );
    }

    #[test]
    fn square_box() {
        let out = console().render_export(&Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE));
        assert_eq!(
            out,
            "┌──────────────────┐\n│ hi               │\n└──────────────────┘\n"
        );
    }

    #[test]
    fn legacy_windows_substitutes_rounded_to_square() {
        // On a legacy Windows console, ROUNDED falls back to SQUARE. Captured
        // from real rich 15.0.0 (legacy_windows=True, width 12).
        let legacy = Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(12)
            .no_color(false)
            .legacy_windows(true)
            .build();
        let out = legacy.render_export(&Panel::new(Box::new(Text::new("hi"))));
        assert_eq!(out, "┌──────────┐\n│ hi       │\n└──────────┘\n");
    }

    #[test]
    fn zero_inner_width_renders_no_body_and_empty_text_one_row() {
        // Captured from real rich 15.0.0 (#449, #442).
        let narrow = Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::color::ColorSystem::Truecolor))
            .width(4)
            .highlight(false)
            .build();
        let panel = Panel::new(Box::new(Text::new("ab cd"))).box_set(crate::r#box::HEAVY);
        assert_eq!(narrow.render_export(&panel), "┏━━┓\n┗━━┛\n");
        let empty = Panel::new(Box::new(Text::new(""))).box_set(SQUARE);
        assert_eq!(
            Console::builder()
                .force_terminal(true)
                .color_system(Some(crate::color::ColorSystem::Truecolor))
                .width(10)
                .highlight(false)
                .build()
                .render_export(&empty),
            "┌────────┐\n│        │\n└────────┘\n"
        );
    }
}
