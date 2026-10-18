//! Assets two rows high (#711): reserving their rows in any layout.
//!
//! A micro asset in a [`Text`](rich::Text) is always its **first row**: its
//! columns of placeholder cells, in the line it sits on, so it measures,
//! wraps and crops as one unbreakable word of exactly its width. An asset
//! two rows high (a 2×2 icon, a 3×2 animation) also needs the row below
//! that line, and core's layout knows nothing of it. [`MicroRows`] adds it:
//! it renders what it wraps, and after every line holding the first row of a
//! two-row asset it inserts a **reserved row**:
//!
//! - under each such asset, its second row: [`PAD_CELL`] blanks at the same
//!   columns, tagged with the asset's occurrence and row 1, so a
//!   [`MicroView`](crate::MicroView) can draw the image over both rows;
//! - everywhere else, blank: spaces under the text beside the asset (the
//!   text stays on the first row and never flows into the reserved row),
//!   keeping only the background colour of the cells above, and with no
//!   blanks after the last asset unless they have a background.
//!
//! So a two-row asset never overlaps the line below it: that line moves
//! down. The rule is the same at every width and in every container, and the
//! output depends only on the input, so the same input renders the same way
//! every time. [`MicroRows`] reports the extra row through its rendered
//! height, which is what tables and panels lay out by, so a table row or a
//! panel grows to hold it; its width is the inner renderable's.
//!
//! Wrap the [`Text`](rich::Text) that holds the asset (or a renderable that
//! lays out like text), not a table or panel around it: a reserved row is
//! blank across the whole line, borders included. A row already reserved is
//! never reserved again, so wrapping twice is harmless.
//! [`MicroAssetRef`](crate::MicroAssetRef) reserves its own rows.
//!
//! Where nothing reserves the rows (a bare `Text`, Markdown, a table or
//! panel title), a two-row asset shows its first row only, as its fallback:
//! the renderers draw an image over both rows only when both are there.

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use crate::render::{MicroMeta, PAD_CELL};

/// One run of cells tagged with one occurrence's row, found in rendered
/// output.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub meta: MicroMeta,
    /// The run's segments: `start..end`.
    pub start: usize,
    pub end: usize,
    /// The line it is on (counting line breaks from 0) and its first column.
    pub line: usize,
    pub column: usize,
    /// Its width in cells.
    pub width: usize,
}

/// Every tagged run in `segments`, in order. A run is the segments tagged
/// with one occurrence's row, next to each other; a control segment ends it.
pub(crate) fn runs(segments: &[Segment]) -> Vec<Run> {
    let mut out = Vec::new();
    let (mut line, mut column) = (0, 0);
    let mut index = 0;
    while index < segments.len() {
        let segment = &segments[index];
        let Some(meta) = MicroMeta::from_segment(segment) else {
            if !segment.control {
                match segment.text.rfind('\n') {
                    Some(last) => {
                        line += segment.text.matches('\n').count();
                        column = cell_len(&segment.text[last + 1..]);
                    }
                    None => column += segment.cell_length(),
                }
            }
            index += 1;
            continue;
        };
        let start = index;
        let mut width = 0;
        while index < segments.len()
            && MicroMeta::from_segment(&segments[index])
                .is_some_and(|m| m.id == meta.id && m.row == meta.row)
        {
            width += segments[index].cell_length();
            index += 1;
        }
        out.push(Run {
            meta,
            start,
            end: index,
            line,
            column,
            width,
        });
        column += width;
    }
    out
}

/// `segments` split at line breaks: the lines, and the break after each but
/// the last (its own segment, as it was styled).
fn split(segments: Vec<Segment>) -> (Vec<Vec<Segment>>, Vec<Segment>) {
    let mut lines = vec![Vec::new()];
    let mut breaks = Vec::new();
    for segment in segments {
        if segment.control || !segment.text.contains('\n') {
            lines.last_mut().expect("a line").push(segment);
            continue;
        }
        for (index, part) in segment.text.split('\n').enumerate() {
            if index > 0 {
                breaks.push(Segment::new("\n", segment.style.clone()));
                lines.push(Vec::new());
            }
            if !part.is_empty() {
                lines
                    .last_mut()
                    .expect("a line")
                    .push(Segment::new(part, segment.style.clone()));
            }
        }
    }
    (lines, breaks)
}

/// Only the background colour of `style`, for a blank under it.
fn background(style: Option<&Style>) -> Option<Style> {
    let color = style?.bgcolor()?.clone();
    Some(Style::new().with_bgcolor(color))
}

/// Append `width` blank cells in `style`, merged into a blank before them.
fn blank(out: &mut Vec<Segment>, width: usize, style: Option<Style>) {
    if width == 0 {
        return;
    }
    if let Some(last) = out.last_mut() {
        let is_blank = !last.control
            && last.style == style
            && MicroMeta::from_segment(last).is_none()
            && last.text.bytes().all(|b| b == b' ');
        if is_blank {
            last.text.push_str(&" ".repeat(width));
            return;
        }
    }
    out.push(Segment::new(" ".repeat(width), style));
}

/// Reserved row `row` (1 for the first below the line) of `line`, under the
/// two-row assets in `pending` (runs of `line`).
fn reserved(line: &[Segment], pending: &[&Run], row: usize) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut index = 0;
    while index < line.len() {
        if let Some(run) = pending.iter().find(|run| run.start == index) {
            let under = background(line[run.start].style.as_ref());
            if run.meta.rows > row {
                let tag = run.meta.with_row(row).to_style();
                let style = under.map_or(tag.clone(), |under| under.combine(&tag));
                out.push(Segment::new(
                    PAD_CELL.to_string().repeat(run.meta.cols),
                    Some(style),
                ));
            } else {
                blank(&mut out, run.width, under);
            }
            index = run.end;
            continue;
        }
        let segment = &line[index];
        if !segment.control {
            blank(
                &mut out,
                segment.cell_length(),
                background(segment.style.as_ref()),
            );
        }
        index += 1;
    }
    // Nothing after the last asset but blanks with no background: drop them.
    if out
        .last()
        .is_some_and(|last| last.style.is_none() && last.text.bytes().all(|b| b == b' '))
    {
        out.pop();
    }
    out
}

/// Reserve the rows of every two-row asset in `segments` (rendered output):
/// see the [module docs](self). Output with no two-row asset is returned as
/// it is.
pub fn reserve_rows(segments: Vec<Segment>) -> Vec<Segment> {
    let multi_row = segments.iter().any(|segment| {
        MicroMeta::from_segment(segment).is_some_and(|meta| meta.row == 0 && meta.rows > 1)
    });
    if !multi_row {
        return segments;
    }
    let (lines, breaks) = split(segments);
    let found: Vec<Vec<Run>> = lines.iter().map(|line| runs(line)).collect();
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        // The two-row assets whole on this line, unless the line below
        // already holds their second row.
        let below = found.get(index + 1);
        let pending: Vec<&Run> = found[index]
            .iter()
            .filter(|run| {
                run.meta.row == 0
                    && run.meta.rows > 1
                    && run.width == run.meta.cols
                    && !below.is_some_and(|below| {
                        below.iter().any(|other| {
                            other.meta.id == run.meta.id
                                && other.meta.row == 1
                                && other.column == run.column
                        })
                    })
            })
            .collect();
        out.extend(line.iter().cloned());
        let extra = pending.iter().map(|run| run.meta.rows - 1).max();
        for row in 1..=extra.unwrap_or(0) {
            out.push(Segment::line());
            out.extend(reserved(line, &pending, row));
        }
        if let Some(line_break) = breaks.get(index) {
            out.push(line_break.clone());
        }
    }
    out
}

/// A renderable with the rows of its two-row micro assets reserved: see the
/// [module docs](self).
///
/// ```
/// use rich::{Console, Text};
/// use rich_micro::{CellSize, MicroAsset, MicroExt, MicroRows};
///
/// let icon = MicroAsset::new("team/icon", "blue square")?
///     .with_size(CellSize::new(2, 2)?)?
///     .with_text("[]")?;
/// let mut text = Text::new("Deploying ");
/// text.append_micro_asset(&icon).append(" done", None);
/// let console = Console::builder().width(40).build();
/// let shown = console.render_export(&MicroRows::new(text));
/// // The line the icon sits on, then the row it reserves below it.
/// assert_eq!(shown, "Deploying [] done\n          \u{2800}\u{2800}\n");
/// # Ok::<(), rich_micro::MicroError>(())
/// ```
#[derive(Clone, Debug)]
pub struct MicroRows<R> {
    inner: R,
}

impl<R> MicroRows<R> {
    pub fn new(inner: R) -> Self {
        MicroRows { inner }
    }

    /// What it wraps.
    pub fn inner(&self) -> &R {
        &self.inner
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Renderable> Renderable for MicroRows<R> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        reserve_rows(self.inner.rich_render(console, options))
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.inner.measure(console, options)
    }

    fn fit_to_measurement(&self) -> bool {
        self.inner.fit_to_measurement()
    }

    fn vertical(&self) -> Option<rich::align::VerticalAlign> {
        self.inner.vertical()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(text: &str, meta: &MicroMeta) -> Segment {
        Segment::new(text, Some(meta.to_style()))
    }

    fn meta(rows: usize, id: u64) -> MicroMeta {
        MicroMeta {
            name: "a".into(),
            cols: 2,
            rows,
            id,
            row: 0,
        }
    }

    fn plain(segments: &[Segment]) -> String {
        segments.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn runs_know_their_line_and_column() {
        let one = meta(1, 1);
        let two = meta(2, 2);
        let segments = vec![
            Segment::new("ab ", None),
            tagged("x", &one),
            tagged("y", &one),
            Segment::new(" c\nd", None),
            tagged("zz", &two),
            Segment::control("\x1b[0m"),
            tagged("zz", &two),
        ];
        let found = runs(&segments);
        let shape: Vec<(u64, usize, usize, usize, usize)> = found
            .iter()
            .map(|r| (r.meta.id, r.line, r.column, r.width, r.end - r.start))
            .collect();
        // A control segment ends a run.
        assert_eq!(
            shape,
            [(1, 0, 3, 2, 2), (2, 1, 1, 2, 1), (2, 1, 3, 2, 1)],
            "{found:#?}"
        );
    }

    #[test]
    fn a_line_with_a_two_row_asset_gets_a_row_below_it() {
        let two = meta(2, 7);
        let red = Style::parse("red on blue").unwrap();
        let segments = vec![
            Segment::new("go ", None),
            tagged("[]", &two),
            Segment::new(" now", Some(red)),
            Segment::line(),
            Segment::new("next", None),
        ];
        let out = reserve_rows(segments);
        assert_eq!(plain(&out), "go [] now\n   \u{2800}\u{2800}    \nnext");
        // The blank under " now" keeps its background, nothing else.
        let under = out.iter().rev().nth(2).unwrap();
        assert_eq!(under.text, "    ");
        assert_eq!(
            under.style.as_ref().and_then(|s| s.bgcolor()).cloned(),
            Style::parse("on blue").unwrap().bgcolor().cloned()
        );
        assert!(under.style.as_ref().unwrap().color().is_none());
        // The second row is tagged as row 1 of the same occurrence.
        let second: Vec<MicroMeta> = out.iter().filter_map(MicroMeta::from_segment).collect();
        assert_eq!(second.iter().map(|m| m.row).collect::<Vec<_>>(), [0, 1]);
        assert!(second.iter().all(|m| m.id == 7));
        // Reserving again changes nothing.
        assert_eq!(reserve_rows(out.clone()), out);
    }

    #[test]
    fn one_row_output_is_untouched() {
        let one = meta(1, 3);
        let segments = vec![Segment::new("a\n", None), tagged("xx", &one)];
        assert_eq!(reserve_rows(segments.clone()), segments);
    }

    #[test]
    fn a_cut_asset_reserves_nothing() {
        let two = meta(2, 4);
        let segments = vec![Segment::new("ab ", None), tagged("x", &two)];
        assert_eq!(plain(&reserve_rows(segments)), "ab x");
    }
}
