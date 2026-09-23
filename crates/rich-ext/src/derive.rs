//! The runtime behind `#[derive(Rich)]` (the `macros` feature).
//!
//! The derive implements [`RichRecord`]: a title and a list of labelled
//! [`Field`]s. [`render`] draws one record as a label/value grid, a titled
//! panel or a one-row table, and [`table`] lays many records out as rows, one
//! column per field label. The functions work for hand-written `RichRecord`
//! impls too.

use rich::{
    Console, ConsoleOptions, Highlighter, Justify, Panel, Renderable, ReprHighlighter, Segment,
    Style, Table, Text,
};

/// One labelled value of a record.
#[derive(Clone, Debug)]
pub struct Field {
    /// The label (the field name, or `#[rich(label = "…")]`).
    pub label: String,
    /// The formatted value.
    pub value: String,
    /// The value's style, if the field sets one.
    pub style: Option<Style>,
    /// The value's justification in a table.
    pub justify: Justify,
    /// Whether to colour the value with `ReprHighlighter` (`Debug` values
    /// without a style).
    pub highlight: bool,
}

impl Field {
    /// A plain field with `Debug`-style highlighting.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Field {
            label: label.into(),
            value: value.into(),
            style: None,
            justify: Justify::Left,
            highlight: true,
        }
    }

    /// The value as styled text.
    pub fn text(&self) -> Text {
        let mut text = match &self.style {
            Some(style) => Text::styled(self.value.clone(), style.clone()),
            None => Text::new(self.value.clone()),
        };
        if self.highlight {
            ReprHighlighter::new().highlight(&mut text);
        }
        text
    }
}

/// How a single record renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    /// A title line, then a label/value grid.
    Fields,
    /// A label/value grid in a titled panel.
    Panel,
    /// A one-row table with the labels as headers.
    Table,
}

/// A value that renders as labelled fields. `#[derive(Rich)]` implements it.
pub trait RichRecord {
    /// The title (a type's `#[rich(title)]`, or an enum's variant name) and
    /// the fields, in display order.
    fn rich_record(&self) -> (Option<String>, Vec<Field>);

    /// How a single record renders (default [`Presentation::Fields`]).
    fn rich_presentation(&self) -> Presentation {
        Presentation::Fields
    }
}

fn grid(fields: &[Field]) -> Table {
    let mut grid = Table::grid().padding(0, 1, 0, 1);
    grid.add_column("")
        .column_style(Style::parse("bold").expect("valid style"));
    grid.add_column("");
    for field in fields {
        grid.add_row_text(vec![Text::new(format!("{}:", field.label)), field.text()]);
    }
    grid
}

/// Render one record as its [`Presentation`] asks.
pub fn render<T: RichRecord + ?Sized>(
    value: &T,
    console: &Console,
    options: &ConsoleOptions,
) -> Vec<Segment> {
    let (title, fields) = value.rich_record();
    match value.rich_presentation() {
        Presentation::Table => {
            let mut table = rows(std::iter::once((title.clone(), fields)));
            if let Some(title) = title {
                table = table.title(title);
            }
            table.rich_render(console, options)
        }
        Presentation::Panel => {
            let mut panel = Panel::new(Box::new(grid(&fields)));
            if let Some(title) = title {
                panel = panel.title(title);
            }
            panel.rich_render(console, options)
        }
        Presentation::Fields => {
            let mut lines = Vec::new();
            if let Some(title) = title {
                let heading = Text::styled(title, Style::parse("bold").expect("valid style"));
                lines.extend(console.render_lines(&heading, options, false));
            }
            if !fields.is_empty() {
                lines.extend(console.render_lines(&grid(&fields), options, false));
            }
            let last = lines.len().saturating_sub(1);
            let mut segments = Vec::new();
            for (index, line) in lines.into_iter().enumerate() {
                segments.extend(line);
                if index != last {
                    segments.push(Segment::line());
                }
            }
            segments
        }
    }
}

fn rows(records: impl IntoIterator<Item = (Option<String>, Vec<Field>)>) -> Table {
    let records: Vec<_> = records.into_iter().collect();
    // Columns are the union of the labels, in first-seen order; each takes
    // the justification of its first field.
    let mut columns: Vec<(String, Justify)> = Vec::new();
    for (_, fields) in &records {
        for field in fields {
            if !columns.iter().any(|(label, _)| *label == field.label) {
                columns.push((field.label.clone(), field.justify));
            }
        }
    }
    let mut table = Table::new();
    for (label, justify) in &columns {
        table.add_column_justify(label.clone(), *justify);
    }
    for (_, fields) in records {
        let row = columns
            .iter()
            .map(|(label, _)| {
                fields
                    .iter()
                    .find(|field| field.label == *label)
                    .map(Field::text)
                    .unwrap_or_default()
            })
            .collect();
        table.add_row_text(row);
    }
    table
}

/// A table with one row per record and one column per field label. Enum
/// variants with different fields leave the missing cells empty.
pub fn table<'a, T: RichRecord + 'a>(records: impl IntoIterator<Item = &'a T>) -> Table {
    rows(records.into_iter().map(RichRecord::rich_record))
}
