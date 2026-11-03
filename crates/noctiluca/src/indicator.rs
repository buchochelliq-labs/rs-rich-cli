//! Drawing an [`Indicator`] (a meter, a progress bar, a rating the page
//! states) as lines of cells, for the projection and reader mode alike.
//!
//! One entry, [`lines`], chooses the shape from the indicator and the
//! cells it has ([`shape`]) and draws it:
//!
//! - **Bar** ([`bar`]): wider than tall, most of them. Drawn as
//!   intuiTUIve's `progress_bar` draws, `━` in the bar's colour over a
//!   `grey23` track with the share after it, the label before; a
//!   `<meter>`'s value is green, yellow or red by its `low`, `high` and
//!   `optimum`, as browsers draw it.
//! - **Steps** ([`steps`]): a meter of a few whole steps in a narrow box
//!   (a signal meter's "3 of 5 bars"): rising eighths blocks, as many as
//!   its steps, lit to its value.
//! - **Dial** ([`dial`]): roughly square and at least
//!   [`DIAL_COLUMNS`] x [`DIAL_ROWS`] cells (a speed test's gauge): an arc
//!   lit to its share around its readout.
//! - **Stars** ([`stars`]): a rating, `★★★★½ 4.5`.
//!
//! The projection draws lines, not intuiTUIve nodes, so the bar is drawn
//! here in the same cells and styles as `progress_bar`. The dial and the
//! stars each live in one function, so intuiTUIve's `dial` and `rating`
//! can replace them where they are.

use rich::cells::cell_len;

use crate::adapt::{Adapt, Rule};
use crate::layout::{Line, Span};
use crate::model::{Indicator, IndicatorKind, Page, number_text};

/// A dial needs at least this many columns.
pub const DIAL_COLUMNS: usize = 10;
/// A dial needs at least this many rows.
pub const DIAL_ROWS: usize = 5;
/// The most rows a dial takes.
pub const MAX_DIAL_ROWS: usize = 12;
/// The most columns a dial takes.
pub const MAX_DIAL_COLUMNS: usize = 40;
/// A bar is at least this many cells long.
pub const MIN_BAR: usize = 8;

/// The style of the lit part, as intuiTUIve's `progress.bar`.
const BAR: &str = "magenta";
/// The style of the unlit part, as intuiTUIve's `progress.track`.
const TRACK: &str = "grey23";

/// How an indicator is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A bar, one row: [`bar`].
    Bar,
    /// Rising blocks, one a step: [`steps`].
    Steps,
    /// An arc around its readout: [`dial`].
    Dial,
    /// Stars and the number: [`stars`].
    Stars,
}

/// The shape of `indicator` in a box of `cols` x `rows` cells, `cell`
/// CSS pixels each (width, height).
pub fn shape(indicator: &Indicator, cols: usize, rows: usize, cell: (f64, f64)) -> Shape {
    if indicator.kind == IndicatorKind::Rating {
        return Shape::Stars;
    }
    let (w, h) = (indicator.rect.width, indicator.rect.height);
    let square = h > 0.0 && (0.6..=1.7).contains(&(w / h));
    if square && cols >= DIAL_COLUMNS && rows >= DIAL_ROWS && h >= DIAL_ROWS as f64 * cell.1 {
        return Shape::Dial;
    }
    if indicator.kind == IndicatorKind::Meter
        && rows <= 1
        && cols <= MIN_BAR
        && steps_of(indicator).is_some()
    {
        return Shape::Steps;
    }
    Shape::Bar
}

/// `indicator` drawn in `cols` x `rows` cells (`cell` CSS pixels each):
/// one line for a bar, steps or stars, `rows` lines (within the dial's
/// bounds) for a dial. A bar's line may be wider than `cols`: its label
/// and readout are around it.
pub fn lines(indicator: &Indicator, cols: usize, rows: usize, cell: (f64, f64)) -> Vec<Line> {
    let mut line = match shape(indicator, cols, rows, cell) {
        Shape::Bar => bar(indicator, cols),
        Shape::Steps => steps(indicator),
        Shape::Dial => {
            return dial(
                indicator,
                cols.min(MAX_DIAL_COLUMNS),
                rows.min(MAX_DIAL_ROWS),
            );
        }
        Shape::Stars => stars(indicator),
    };
    // Text before it on its line ("Rating ★★★★½"): a space between them
    // that wrapping keeps, as terminal glyphs are wider than a page's.
    if indicator.after_text {
        line.spans.insert(0, Span::new("\u{a0}", ""));
    }
    vec![line]
}

/// Whether media region `media` of `page` is drawn as the indicator it
/// also is, not as its picture: by the [indicators](Rule::Indicators)
/// rule.
pub fn instead(page: &Page, media: usize, adapt: Adapt) -> bool {
    adapt.on(Rule::Indicators) && page.indicators.iter().any(|i| i.media == Some(media))
}

/// Indicator `index` of `page` in reader mode, `width` cells wide: its
/// box's size on an 8 x 16 pixel cell, within the width. `None` when its
/// picture is drawn instead.
pub fn reader_lines(page: &Page, index: usize, width: usize, adapt: Adapt) -> Option<Vec<Line>> {
    let indicator = page.indicators.get(index)?;
    if indicator.media.is_some() && !adapt.on(Rule::Indicators) {
        return None;
    }
    let cell = (8.0, 16.0);
    let label = cell_len(&indicator.label) + 8;
    let cols = ((indicator.rect.width / cell.0).round() as usize)
        .clamp(MIN_BAR, MAX_DIAL_COLUMNS)
        .min(width.saturating_sub(label).max(MIN_BAR));
    let rows = ((indicator.rect.height / cell.1).round() as usize).max(1);
    Some(lines(indicator, cols, rows, cell))
}

/// The colour of a value: the bar's, or for a `<meter>` with bands,
/// green in its optimum region, yellow next to it, red past that.
pub fn colour(indicator: &Indicator) -> &'static str {
    let (Some((low, high, optimum)), Some(value)) = (indicator.bands, indicator.value) else {
        return BAR;
    };
    // The regions, low to high: below low, low to high, above high.
    let region = |v: f64| -> u8 {
        if v < low {
            0
        } else if v > high {
            2
        } else {
            1
        }
    };
    match region(optimum).abs_diff(region(value)) {
        0 => "green",
        1 => "yellow",
        _ => "red",
    }
}

/// The label in front, when it is not in the text before it already.
fn label(indicator: &Indicator) -> Option<Span> {
    (!indicator.label_in_text && !indicator.label.is_empty())
        .then(|| Span::new(format!("{} ", indicator.label), "bold"))
}

/// A bar of `cols` cells (at least [`MIN_BAR`]) with its label before it
/// and its readout after, as intuiTUIve's `progress_bar` draws: a busy
/// one's track holds a quarter of it lit, at its start.
pub fn bar(indicator: &Indicator, cols: usize) -> Line {
    let len = cols.max(MIN_BAR);
    let mut spans: Vec<Span> = label(indicator).into_iter().collect();
    let lit = match indicator.share() {
        Some(share) => (share * len as f64).round() as usize,
        None => (len / 4).max(1),
    };
    let lit = lit.min(len);
    if lit > 0 {
        spans.push(Span::new("━".repeat(lit), colour(indicator)));
    }
    if lit < len {
        spans.push(Span::new("━".repeat(len - lit), TRACK));
    }
    spans.push(Span::new(format!(" {}", indicator.readout()), ""));
    Line { spans }
}

/// A meter's whole steps: `max - min` when it is a whole number from 2 to
/// 10.
fn steps_of(indicator: &Indicator) -> Option<usize> {
    let span = indicator.max - indicator.min;
    (span.fract() == 0.0 && (2.0..=10.0).contains(&span)).then_some(span as usize)
}

/// A meter of a few steps as rising blocks, lit to its value, with its
/// label and readout.
pub fn steps(indicator: &Indicator) -> Line {
    const RISING: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let count = steps_of(indicator).unwrap_or(5);
    let lit = indicator
        .value
        .map_or(0, |v| (v - indicator.min).round().max(0.0) as usize);
    let mut spans: Vec<Span> = label(indicator).into_iter().collect();
    for k in 0..count {
        let glyph = RISING[((k + 1) * 8 / count).clamp(1, 8) - 1];
        let style = if k < lit { colour(indicator) } else { TRACK };
        spans.push(Span::new(glyph.to_string(), style));
    }
    spans.push(Span::new(format!(" {}", indicator.readout()), ""));
    Line { spans }
}

/// A dial of `cols` x `rows` cells (at least [`DIAL_COLUMNS`] x
/// [`DIAL_ROWS`]): an arc up the left side, over the top and down the
/// right, lit to its share as a needle sweeps 270°, its readout in the
/// middle and its label on the bottom row.
///
/// TODO(intuiTUIve `dial`): this is the one place a dial is drawn; when
/// intuiTUIve's core `dial` widget lands, draw it here (its value from
/// [`Indicator::share`], its readout from [`Indicator::readout`], its
/// bands from [`Indicator::bands`]), and keep [`shape`] choosing it.
pub fn dial(indicator: &Indicator, cols: usize, rows: usize) -> Vec<Line> {
    let (cols, rows) = (cols.max(DIAL_COLUMNS), rows.max(DIAL_ROWS));
    // The arc's cells, in sweep order, with their thin and thick glyphs.
    let mut arc: Vec<(usize, usize, char, char)> = Vec::new();
    for r in (1..rows - 1).rev() {
        arc.push((r, 0, '│', '┃'));
    }
    arc.push((0, 0, '╭', '┏'));
    for c in 1..cols - 1 {
        arc.push((0, c, '─', '━'));
    }
    arc.push((0, cols - 1, '╮', '┓'));
    for r in 1..rows - 1 {
        arc.push((r, cols - 1, '│', '┃'));
    }
    let lit = indicator
        .share()
        .map_or(0, |s| (s * arc.len() as f64).round() as usize);
    let mut grid: Vec<Vec<(String, &str)>> = vec![vec![(" ".into(), ""); cols]; rows];
    for (k, (r, c, thin, thick)) in arc.into_iter().enumerate() {
        grid[r][c] = if k < lit {
            (thick.to_string(), colour(indicator))
        } else {
            (thin.to_string(), TRACK)
        };
    }
    let mut lines: Vec<Line> = grid
        .into_iter()
        .map(|row| {
            let mut spans: Vec<Span> = Vec::new();
            for (text, style) in row {
                match spans.last_mut() {
                    Some(last) if last.style == style => last.text.push_str(&text),
                    _ => spans.push(Span::new(text, style)),
                }
            }
            Line { spans }
        })
        .collect();
    // The readout in the middle, the label on the bottom row, each
    // centred inside the arc.
    let inner = cols - 2;
    let centred = |text: &str, style: &str| -> Line {
        let text = crate::clean::truncate(text, inner);
        let pad = inner.saturating_sub(cell_len(&text));
        let left = pad / 2;
        vec![
            Span::new(" ".repeat(left), ""),
            Span::new(text, style.to_string()),
            Span::new(" ".repeat(pad - left), ""),
        ]
        .into_iter()
        .fold(Line::default(), |mut line, span| {
            line.spans.push(span);
            line
        })
    };
    let middle = (rows - 1) / 2;
    let put = |line: &mut Line, inside: Line| {
        // The arc's first and last cells stay; the inside is replaced.
        let first = line.spans.first().map(|s| {
            let mut s = s.clone();
            s.text = s.text.chars().take(1).collect();
            s
        });
        let last = line.spans.last().map(|s| {
            let mut s = s.clone();
            s.text = s.text.chars().last().into_iter().collect();
            s
        });
        let mut spans: Vec<Span> = first.into_iter().collect();
        spans.extend(inside.spans);
        spans.extend(last);
        line.spans = spans;
    };
    put(&mut lines[middle], centred(&indicator.readout(), "bold"));
    let bottom = rows - 1;
    let text = if indicator.label.is_empty() {
        String::new()
    } else {
        indicator.label.clone()
    };
    lines[bottom] = {
        let mut line = centred(&text, "");
        line.spans.insert(0, Span::new(" ", ""));
        line.spans.push(Span::new(" ", ""));
        line
    };
    lines
}

/// A rating as stars, `★★★★½☆`-like, one a point of its scale (five for
/// a scale over ten), then its number: `★★★★½ 4.5`.
///
/// TODO(intuiTUIve `rating`): this is the one place a rating is drawn;
/// when intuiTUIve's core `rating` widget lands, draw it here with
/// [`Indicator::value`] and [`Indicator::max`].
pub fn stars(indicator: &Indicator) -> Line {
    let value = indicator.value.unwrap_or(0.0);
    let scale = indicator.max.max(1.0);
    let (count, value) = if scale <= 10.0 && scale.fract() == 0.0 {
        (scale as usize, value)
    } else {
        (5, value / scale * 5.0)
    };
    let full = value.floor() as usize;
    let frac = value - value.floor();
    let (full, half) = match frac {
        f if f >= 0.75 => (full + 1, false),
        f if f >= 0.25 => (full, true),
        _ => (full, false),
    };
    let full = full.min(count);
    let mut lit = "★".repeat(full);
    if half && full < count {
        lit.push('½');
    }
    let empty = count.saturating_sub(full + usize::from(half && full < count));
    let mut spans = vec![Span::new(lit, "yellow")];
    if empty > 0 {
        spans.push(Span::new("☆".repeat(empty), "bright_black"));
    }
    spans.push(Span::new(
        format!(" {}", number_text(indicator.value.unwrap_or(0.0))),
        "",
    ));
    Line { spans }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Rect;

    fn indicator(kind: IndicatorKind, value: Option<f64>, max: f64, rect: Rect) -> Indicator {
        Indicator {
            node: 1,
            kind,
            label: "Disk".into(),
            value,
            min: 0.0,
            max,
            text: None,
            rect,
            bands: None,
            media: None,
            label_in_text: false,
            after_text: false,
        }
    }

    #[test]
    fn a_bar_is_lit_to_its_share_with_its_label_and_readout() {
        let bar = bar(
            &indicator(
                IndicatorKind::Progress,
                Some(25.0),
                100.0,
                Rect::new(0.0, 0.0, 80.0, 16.0),
            ),
            8,
        );
        assert_eq!(bar.plain(), "Disk ━━━━━━━━ 25%");
        assert_eq!(bar.spans[1].text, "━━");
        assert_eq!(bar.spans[1].style, "magenta");
        assert_eq!(bar.spans[2].style, "grey23");
    }

    #[test]
    fn a_meter_takes_its_bands_colour() {
        let mut meter = indicator(IndicatorKind::Meter, Some(72.0), 100.0, Rect::default());
        meter.bands = Some((60.0, 85.0, 20.0));
        assert_eq!(colour(&meter), "yellow");
        meter.value = Some(90.0);
        assert_eq!(colour(&meter), "red");
        meter.value = Some(10.0);
        assert_eq!(colour(&meter), "green");
    }

    #[test]
    fn a_square_box_is_a_dial_with_its_readout_inside() {
        let mut gauge = indicator(
            IndicatorKind::Meter,
            Some(250.0),
            500.0,
            Rect::new(0.0, 0.0, 240.0, 200.0),
        );
        gauge.text = Some("250.0 Mbps".into());
        gauge.label = "Upload".into();
        assert_eq!(shape(&gauge, 30, 12, (8.0, 16.0)), Shape::Dial);
        let lines = lines(&gauge, 30, 12, (8.0, 16.0));
        assert_eq!(lines.len(), 12);
        assert!(lines.iter().all(|l| l.width() == 30), "{lines:?}");
        assert!(lines[5].plain().contains("250.0 Mbps"));
        assert!(lines[11].plain().contains("Upload"));
        assert!(
            lines[0].plain().starts_with('┏'),
            "half lit: the left side and corner"
        );
        assert!(lines[0].plain().ends_with('╮'));
    }

    #[test]
    fn a_rating_is_stars_and_its_number() {
        let rating = indicator(IndicatorKind::Rating, Some(4.5), 5.0, Rect::default());
        assert_eq!(stars(&rating).plain(), "★★★★½ 4.5");
        let rating = indicator(IndicatorKind::Rating, Some(4.1), 5.0, Rect::default());
        assert_eq!(stars(&rating).plain(), "★★★★☆ 4.1");
        let rating = indicator(IndicatorKind::Rating, Some(4.8), 5.0, Rect::default());
        assert_eq!(stars(&rating).plain(), "★★★★★ 4.8");
    }

    #[test]
    fn a_narrow_meter_of_a_few_steps_rises() {
        let mut signal = indicator(
            IndicatorKind::Meter,
            Some(3.0),
            5.0,
            Rect::new(0.0, 0.0, 30.0, 16.0),
        );
        signal.text = Some("3 of 5 bars".into());
        signal.label = "Signal".into();
        assert_eq!(shape(&signal, 4, 1, (8.0, 16.0)), Shape::Steps);
        assert_eq!(steps(&signal).plain(), "Signal ▁▃▄▆█ 3 of 5 bars");
    }
}
