//! `rich chart`: a chart from CSV, JSON or stdin (0.0.15 workstream 5). Not
//! upstream: a binary-boundary convenience that composes `rich_ext::chart`;
//! see `docs/PORTING.md`.
//!
//! Reading the data into columns lives here rather than in `rich-ext`: CSV
//! goes through the CLI's own port of Python's `csv` sniffer and reader (the
//! one `--csv` uses), and picking the columns from `--x` and `--y` is command
//! routing. The charts themselves are `rich_ext::chart`'s, unchanged.
use super::*;

use rich::table::Cell as TableCell;
use rich_ext::chart::{Bar, BarChart, Heatmap, LineChart, Series, Sparkline};

/// `--kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ChartKind {
    /// One sparkline per series.
    Spark,
    /// A bar per row, labelled by `--x`.
    Bar,
    /// Each series as a line against `--x` (or the row number). The default.
    #[default]
    Line,
    /// Each series as points against `--x` (or the row number).
    Scatter,
    /// Rows by series, each value a shade.
    Heatmap,
}

impl ChartKind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "spark" | "sparkline" => Ok(Self::Spark),
            "bar" => Ok(Self::Bar),
            "line" => Ok(Self::Line),
            "scatter" => Ok(Self::Scatter),
            "heatmap" => Ok(Self::Heatmap),
            other => Err(format!(
                "unknown chart kind {other:?} (spark, bar, line, scatter, heatmap)"
            )),
        }
    }

    /// Whether `--x` is a number (a position) rather than a label.
    fn numeric_x(self) -> bool {
        matches!(self, Self::Line | Self::Scatter)
    }
}

/// The options of `rich chart`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChartOptions {
    /// `--kind`; `None` when not given (a line chart).
    kind: Option<ChartKind>,
    /// `--x COLUMN`.
    x: Option<String>,
    /// `--y COLUMN`, repeatable: one series each.
    y: Vec<String>,
}

impl ChartOptions {
    /// Consume one of these options; anything else stays with the main parser.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match arg {
            "--kind" => {
                let value = rest
                    .next()
                    .ok_or("--kind requires spark, bar, line, scatter or heatmap")?;
                self.kind = Some(ChartKind::parse(value)?);
            }
            "--x" => {
                let value = rest.next().ok_or("--x requires a COLUMN")?;
                if self.x.is_some() {
                    return Err("--x can be given once; a chart has one x column".into());
                }
                self.x = Some(value.clone());
            }
            "--y" => {
                let value = rest.next().ok_or("--y requires a COLUMN")?;
                self.y.push(value.clone());
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Each option given with the commands it applies to, for the "only has
    /// an effect" check.
    pub(crate) fn given(&self) -> Vec<(&'static str, &'static [&'static str])> {
        [
            ("--kind", self.kind.is_some()),
            ("--x", self.x.is_some()),
            ("--y", !self.y.is_empty()),
        ]
        .into_iter()
        .filter(|(_, given)| *given)
        .map(|(flag, _)| (flag, &["chart"][..]))
        .collect()
    }
}

type Failure = (ExitClass, String);

/// One value read from the data, before a chart asks for it as a number.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    /// An empty CSV cell, a JSON `null` or a key a record lacks: a gap.
    Missing,
    Number(f64),
    Text(String),
}

impl Value {
    /// The value as a label: a number as written, a gap as nothing.
    fn label(&self) -> String {
        match self {
            Value::Missing => String::new(),
            Value::Number(n) => n.to_string(),
            Value::Text(text) => text.clone(),
        }
    }

    /// The value as a number: `Ok(NaN)` for a gap, `Err` for text that does
    /// not read as one.
    fn number(&self) -> Result<f64, &str> {
        match self {
            Value::Missing => Ok(f64::NAN),
            Value::Number(n) => Ok(*n),
            Value::Text(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    Ok(f64::NAN)
                } else {
                    trimmed.parse().map_err(|_| text.as_str())
                }
            }
        }
    }
}

/// The data as named columns of rows.
#[derive(Debug, Clone, PartialEq)]
struct Data {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

impl Data {
    fn cell(&self, row: usize, column: usize) -> &Value {
        self.rows[row].get(column).unwrap_or(&Value::Missing)
    }

    /// The column `name` names: its header exactly, else a 1-based position.
    fn column(&self, name: &str) -> Result<usize, Failure> {
        if let Some(index) = self.columns.iter().position(|c| c == name) {
            return Ok(index);
        }
        if let Ok(n) = name.parse::<usize>() {
            if (1..=self.columns.len()).contains(&n) {
                return Ok(n - 1);
            }
        }
        let shown: Vec<String> = self.columns.iter().map(|c| format!("{c:?}")).collect();
        Err((
            ExitClass::Data,
            format!(
                "no column {name:?}; the columns are {}",
                if shown.is_empty() {
                    "none".to_string()
                } else {
                    shown.join(", ")
                }
            ),
        ))
    }

    /// Whether every value in `column` reads as a number (gaps aside), and
    /// at least one is there.
    fn is_numeric(&self, column: usize) -> bool {
        let mut any = false;
        for row in 0..self.rows.len() {
            match self.cell(row, column).number() {
                Ok(n) => any |= !n.is_nan(),
                Err(_) => return false,
            }
        }
        any
    }

    /// `column` as numbers; text that is not one is an error naming the row
    /// (counted from 1, after any header) and the column.
    fn numbers(&self, column: usize) -> Result<Vec<f64>, Failure> {
        (0..self.rows.len())
            .map(|row| {
                self.cell(row, column).number().map_err(|text| {
                    (
                        ExitClass::Data,
                        format!(
                            "row {}, column {:?}: {text:?} is not a number",
                            row + 1,
                            self.columns[column]
                        ),
                    )
                })
            })
            .collect()
    }
}

/// `rich chart [FILE]`: read the data, pick the columns, draw the chart.
pub(crate) fn chart(cli: &Cli) -> Result<Box<dyn Renderable>, Failure> {
    let resource = cli.resource.as_deref().unwrap_or("-");
    let content = if is_url(resource) {
        fetch_url(resource, cli.extensions.encoding)
            .map(|(content, _)| content)
            .map_err(|err| (ExitClass::Input, err))?
    } else {
        read_resource(Some(resource), cli.extensions.encoding).map_err(|err| {
            let name = if resource == "-" { "<stdin>" } else { resource };
            (ExitClass::Input, format!("cannot read {name}: {err}"))
        })?
    };
    let name = if resource == "-" { "<stdin>" } else { resource };
    let data =
        parse(&content, resource).map_err(|err| (ExitClass::Data, format!("{name}: {err}")))?;
    build(&data, &cli.chart)
}

/// Read `content` as JSON or JSON Lines, CSV/TSV, or bare numbers: by the
/// resource's extension, else by its first character.
fn parse(content: &str, resource: &str) -> Result<Data, String> {
    let extension = resource
        .rsplit(['/', '\\'])
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase());
    let trimmed = content.trim_start_matches('\u{feff}').trim_start();
    let data = match extension.as_deref() {
        Some("json" | "jsonl" | "ndjson") => parse_json(content)?,
        Some("csv") => parse_csv(content, Some(','))?,
        Some("tsv") => parse_csv(content, Some('\t'))?,
        _ if trimmed.starts_with(['[', '{']) => parse_json(content)?,
        _ => match parse_numbers(content) {
            Some(data) => data,
            None => parse_csv(content, None)?,
        },
    };
    if data.rows.is_empty() {
        return Err("no rows to chart".into());
    }
    Ok(data)
}

/// Whitespace-separated numbers (`seq 10 | rich chart`): one column, `value`.
fn parse_numbers(content: &str) -> Option<Data> {
    let rows: Option<Vec<Vec<Value>>> = content
        .split_whitespace()
        .map(|token| token.parse().ok().map(|n| vec![Value::Number(n)]))
        .collect();
    let rows = rows?;
    (!rows.is_empty()).then(|| Data {
        columns: vec!["value".into()],
        rows,
    })
}

/// CSV or TSV, the dialect sniffed as `--csv` sniffs it (falling back to
/// `fallback`, or a comma). The first row is a header unless every cell in it
/// is a number; without one the columns are named `1`, `2`, …
fn parse_csv(content: &str, fallback: Option<char>) -> Result<Data, String> {
    let sample: String = content.chars().take(1024).collect();
    let dialect = sniff(&sample, Some(&[',', '\t', '|', ';']))
        .unwrap_or_else(|| Dialect::excel(fallback.unwrap_or(',')));
    let mut rows: Vec<Vec<String>> = read_csv_rows(content, &dialect)
        .into_iter()
        .filter(|row| !row.is_empty() && !(row.len() == 1 && row[0].trim().is_empty()))
        .collect();
    if rows.is_empty() {
        return Err("no rows to chart".into());
    }
    let header = rows[0]
        .iter()
        .any(|cell| !cell.trim().is_empty() && cell.trim().parse::<f64>().is_err());
    let columns = if header {
        rows.remove(0)
            .into_iter()
            .map(|c| c.trim().to_string())
            .collect()
    } else {
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        (1..=width).map(|n| n.to_string()).collect()
    };
    let rows = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| {
                    if cell.trim().is_empty() {
                        Value::Missing
                    } else {
                        Value::Text(cell)
                    }
                })
                .collect()
        })
        .collect();
    Ok(Data { columns, rows })
}

/// JSON: an array of records, of numbers, or of arrays (the first a header
/// when it is all strings), or an object of columns. JSON Lines: one record
/// per line.
fn parse_json(content: &str) -> Result<Data, String> {
    use serde_json::Value as Json;
    let document = match serde_json::from_str::<Json>(content) {
        Ok(document) => document,
        Err(whole) => {
            // JSON Lines: every non-blank line a record.
            let mut records = Vec::new();
            for (n, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Json>(line) {
                    Ok(record @ Json::Object(_)) => records.push(record),
                    Ok(_) => return Err(format!("line {}: expected a JSON object", n + 1)),
                    Err(err) if records.is_empty() => {
                        let _ = err;
                        return Err(format!("not valid JSON: {whole}"));
                    }
                    Err(err) => return Err(format!("line {}: {err}", n + 1)),
                }
            }
            Json::Array(records)
        }
    };
    let value = |json: &Json| match json {
        Json::Null => Value::Missing,
        Json::Number(n) => n.as_f64().map_or(Value::Missing, Value::Number),
        Json::String(s) => Value::Text(s.clone()),
        other => Value::Text(other.to_string()),
    };
    match document {
        Json::Array(items) if items.iter().all(Json::is_object) => {
            let mut columns: Vec<String> = Vec::new();
            for item in &items {
                for key in item.as_object().into_iter().flat_map(|o| o.keys()) {
                    if !columns.contains(key) {
                        columns.push(key.clone());
                    }
                }
            }
            let rows = items
                .iter()
                .map(|item| {
                    let object = item.as_object().expect("checked above");
                    columns
                        .iter()
                        .map(|c| object.get(c).map_or(Value::Missing, value))
                        .collect()
                })
                .collect();
            Ok(Data { columns, rows })
        }
        Json::Array(items) if items.iter().all(Json::is_array) => {
            let mut rows: Vec<Vec<Value>> = items
                .iter()
                .map(|row| {
                    row.as_array()
                        .expect("checked above")
                        .iter()
                        .map(value)
                        .collect()
                })
                .collect();
            let header = items
                .first()
                .and_then(Json::as_array)
                .is_some_and(|first| !first.is_empty() && first.iter().all(Json::is_string));
            let columns = if header {
                rows.remove(0).iter().map(Value::label).collect()
            } else {
                let width = rows.iter().map(Vec::len).max().unwrap_or(0);
                (1..=width).map(|n| n.to_string()).collect()
            };
            Ok(Data { columns, rows })
        }
        Json::Array(items) if items.iter().all(|i| !i.is_object() && !i.is_array()) => Ok(Data {
            columns: vec!["value".into()],
            rows: items.iter().map(|i| vec![value(i)]).collect(),
        }),
        Json::Object(object) if object.values().all(Json::is_array) && !object.is_empty() => {
            let columns: Vec<String> = object.keys().cloned().collect();
            let length = object
                .values()
                .filter_map(Json::as_array)
                .map(Vec::len)
                .max()
                .unwrap_or(0);
            let rows = (0..length)
                .map(|row| {
                    object
                        .values()
                        .map(|column| column.get(row).map_or(Value::Missing, value))
                        .collect()
                })
                .collect();
            Ok(Data { columns, rows })
        }
        _ => Err(
            "expected an array of records, of numbers or of rows, or an object of columns".into(),
        ),
    }
}

/// The chart `options` ask for, drawn from `data`.
fn build(data: &Data, options: &ChartOptions) -> Result<Box<dyn Renderable>, Failure> {
    let kind = options.kind.unwrap_or_default();
    let x = match &options.x {
        Some(name) => Some(data.column(name)?),
        // Labels default to the first column that is not all numbers.
        None if !kind.numeric_x() && kind != ChartKind::Spark => {
            (0..data.columns.len()).find(|&c| !data.is_numeric(c))
        }
        None => None,
    };
    let ys: Vec<usize> = if options.y.is_empty() {
        (0..data.columns.len())
            .filter(|&c| Some(c) != x && data.is_numeric(c))
            .collect()
    } else {
        options
            .y
            .iter()
            .map(|name| data.column(name))
            .collect::<Result<_, _>>()?
    };
    if ys.is_empty() {
        return Err((
            ExitClass::Data,
            format!(
                "no numeric column to chart; name one with --y (the columns are {})",
                data.columns
                    .iter()
                    .map(|c| format!("{c:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    let series: Vec<(&str, Vec<f64>)> = ys
        .iter()
        .map(|&c| Ok((data.columns[c].as_str(), data.numbers(c)?)))
        .collect::<Result<_, Failure>>()?;
    let labels: Vec<String> = (0..data.rows.len())
        .map(|row| match x {
            Some(x) => data.cell(row, x).label(),
            None => (row + 1).to_string(),
        })
        .collect();
    Ok(match kind {
        ChartKind::Spark => {
            if let [(_, values)] = series.as_slice() {
                Box::new(Sparkline::new(values.iter().copied()))
            } else {
                let mut grid = Table::grid();
                grid.add_column("");
                grid.add_column("");
                for (name, values) in &series {
                    grid.add_row_cells(vec![
                        TableCell::Text(Text::new(*name)),
                        TableCell::Renderable(std::sync::Arc::new(Sparkline::new(
                            values.iter().copied(),
                        ))),
                    ]);
                }
                Box::new(grid.padding(0, 1, 0, 0))
            }
        }
        ChartKind::Bar => {
            let mut chart = BarChart::new();
            let several = series.len() > 1;
            for (row, label) in labels.iter().enumerate() {
                for (n, (name, values)) in series.iter().enumerate() {
                    let bar = if several {
                        Bar::new(format!("{label} {name}"), values[row])
                            .style(format!("chart.series.{}", n % 5 + 1))
                    } else {
                        Bar::new(label.clone(), values[row])
                    };
                    chart = chart.push(bar);
                }
            }
            Box::new(chart)
        }
        ChartKind::Line | ChartKind::Scatter => {
            let xs: Vec<f64> = match x {
                Some(x) => data.numbers(x).map_err(|(class, err)| {
                    (
                        class,
                        format!("{err} (a line or scatter chart's --x is a position; --kind bar takes labels)"),
                    )
                })?,
                None => (1..=data.rows.len()).map(|n| n as f64).collect(),
            };
            let mut chart = LineChart::new();
            for (name, values) in &series {
                let points = xs.iter().copied().zip(values.iter().copied());
                chart = chart.series(if kind == ChartKind::Scatter {
                    Series::scatter(*name, points)
                } else {
                    Series::line(*name, points)
                });
            }
            Box::new(chart)
        }
        ChartKind::Heatmap => {
            // Each cell as wide as the widest header and a space, so every
            // header shows (the heatmap still shrinks to the width it gets).
            let header = series.iter().map(|(name, _)| cell_len(name)).max();
            let mut heatmap = Heatmap::new()
                .columns(series.iter().map(|(name, _)| *name))
                .cell_width(header.unwrap_or(1).clamp(1, 11) + 1);
            for (row, label) in labels.iter().enumerate() {
                heatmap = heatmap.row(label.clone(), series.iter().map(|(_, values)| values[row]));
            }
            Box::new(heatmap)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(content: &str, resource: &str) -> Data {
        parse(content, resource).unwrap()
    }

    #[test]
    fn csv_with_a_header_names_its_columns() {
        let d = data("name,count\napi,3\nweb,\n", "x.csv");
        assert_eq!(d.columns, ["name", "count"]);
        assert_eq!(d.rows[1][1], Value::Missing);
        assert!(d.is_numeric(1));
        assert!(!d.is_numeric(0));
    }

    #[test]
    fn csv_without_a_header_numbers_its_columns() {
        let d = data("1,2\n3,4\n", "-");
        assert_eq!(d.columns, ["1", "2"]);
        assert_eq!(d.column("2").unwrap(), 1);
    }

    #[test]
    fn bare_numbers_are_one_series() {
        let d = data("1\n2 3\n", "-");
        assert_eq!(d.columns, ["value"]);
        assert_eq!(d.rows.len(), 3);
    }

    #[test]
    fn json_shapes() {
        let records = data(r#"[{"a":1,"b":2},{"a":3,"c":null}]"#, "-");
        assert_eq!(records.columns, ["a", "b", "c"]);
        assert_eq!(records.rows[1][1], Value::Missing);
        let numbers = data("[1, 2, null]", "-");
        assert_eq!(numbers.columns, ["value"]);
        let rows = data(r#"[["x","y"],[1,2]]"#, "-");
        assert_eq!(rows.columns, ["x", "y"]);
        let columns = data(r#"{"a":[1,2],"b":[3]}"#, "-");
        assert_eq!(columns.rows.len(), 2);
        assert_eq!(columns.rows[1][1], Value::Missing);
        let lines = data("{\"a\":1}\n{\"a\":2}\n", "x.jsonl");
        assert_eq!(lines.rows.len(), 2);
    }

    #[test]
    fn errors_name_the_row_and_the_column() {
        let d = data("t,v\n1,2\n2,oops\n", "x.csv");
        let err = d.numbers(1).unwrap_err();
        assert_eq!(err.1, "row 2, column \"v\": \"oops\" is not a number");
        let err = d.column("w").unwrap_err();
        assert_eq!(err.1, "no column \"w\"; the columns are \"t\", \"v\"");
        assert!(parse("", "-").is_err());
        assert!(parse("{\"a\": 1}", "-").is_err());
    }

    #[test]
    fn labels_write_numbers_back_as_read() {
        assert_eq!(Value::Number(2024.0).label(), "2024");
        assert_eq!(Value::Number(1.25).label(), "1.25");
    }
}
