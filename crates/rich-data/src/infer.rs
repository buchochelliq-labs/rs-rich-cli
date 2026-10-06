//! Column type inference (#267), on request and with its evidence.
//!
//! [`Inferrer::infer`] reads every cell of every column and counts what each
//! parses as: an integer, a float, a boolean (`true`/`false`, any case), a
//! date (`YYYY-MM-DD`), a timestamp (a date, `T` or a space, `HH:MM`, optional
//! seconds and fraction, optional `Z` or offset), or a null token (by default
//! an empty cell, `null`, `NULL`, `NA`, `N/A`). A column is the narrowest
//! type every non-null cell parses as (integers widen to floats, dates to
//! timestamps), `null` when every cell is null, and `text` otherwise; the
//! [`Evidence`] says how many cells parsed as each type, so a column that is
//! text because of one stray cell is easy to spot. An
//! [`override`](Inferrer::override_type) fixes a column's type whatever its
//! cells say.
//!
//! Nothing here runs unless asked: `rich --csv` never infers, and a reader's
//! cells stay as written until [`Inference::apply`] converts them.
//!
//! ```
//! use rich_data::infer::{InferredType, Inferrer};
//! use rich_data::{Rows, Value};
//!
//! let mut rows = Rows::new(["id", "when", "note"]);
//! rows.push(["1".into(), "2026-10-06".into(), "".into()]);
//! rows.push(["2".into(), "2026-10-06T09:30:00Z".into(), "n/a".into()]);
//! rows.push(["NA".into(), "2026-10-07".into(), "7".into()]);
//!
//! let inference = Inferrer::new().infer(&rows);
//! let types: Vec<InferredType> = inference.columns().iter().map(|c| c.data_type()).collect();
//! assert_eq!(types, [InferredType::Integer, InferredType::Timestamp, InferredType::Text]);
//! let note = &inference.columns()[2];
//! assert_eq!((note.evidence().nulls, note.evidence().integers), (1, 1));
//! assert_eq!(note.summary(), "text: 1 of 2 values parse as integer, 1 null");
//!
//! inference.apply(&mut rows);
//! assert_eq!(rows.rows()[2][0], Value::Null);
//! assert_eq!(rows.rows()[1][0], Value::Int(2));
//! ```

use std::collections::HashMap;
use std::fmt;

use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Table};

use crate::{DataType, Field, Rows, Schema, Value};

/// A column's inferred type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InferredType {
    /// Every cell is null.
    Null,
    /// `true` or `false`.
    Boolean,
    /// Whole numbers that fit in an `i64`.
    Integer,
    /// Numbers.
    Float,
    /// `YYYY-MM-DD` dates.
    Date,
    /// Dates with a time.
    Timestamp,
    /// Anything else.
    Text,
}

impl InferredType {
    /// The lowercase name.
    pub fn name(self) -> &'static str {
        match self {
            InferredType::Null => "null",
            InferredType::Boolean => "boolean",
            InferredType::Integer => "integer",
            InferredType::Float => "float",
            InferredType::Date => "date",
            InferredType::Timestamp => "timestamp",
            InferredType::Text => "text",
        }
    }

    /// The schema type: `null` is [`DataType::Unknown`], `text` is
    /// [`DataType::String`].
    pub fn data_type(self) -> DataType {
        match self {
            InferredType::Null => DataType::Unknown,
            InferredType::Boolean => DataType::Boolean,
            InferredType::Integer => DataType::Integer,
            InferredType::Float => DataType::Float,
            InferredType::Date => DataType::Date,
            InferredType::Timestamp => DataType::timestamp(),
            InferredType::Text => DataType::String,
        }
    }
}

impl fmt::Display for InferredType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How many of a column's cells parsed as each type. A cell counts for
/// every type it parses as: `7` is an integer and a float, a date is also a
/// timestamp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    /// Every cell.
    pub cells: usize,
    /// Null tokens.
    pub nulls: usize,
    /// Booleans.
    pub booleans: usize,
    /// Integers.
    pub integers: usize,
    /// Numbers (integers included).
    pub floats: usize,
    /// Dates.
    pub dates: usize,
    /// Timestamps (dates included).
    pub timestamps: usize,
}

impl Evidence {
    /// The non-null cells.
    pub fn values(&self) -> usize {
        self.cells - self.nulls
    }

    /// How many non-null cells parse as `data_type`.
    pub fn parsed(&self, data_type: InferredType) -> usize {
        match data_type {
            InferredType::Null => self.nulls,
            InferredType::Boolean => self.booleans,
            InferredType::Integer => self.integers,
            InferredType::Float => self.floats,
            InferredType::Date => self.dates,
            InferredType::Timestamp => self.timestamps,
            InferredType::Text => self.values(),
        }
    }

    /// The narrowest type every non-null cell parses as.
    pub fn best(&self) -> InferredType {
        let values = self.values();
        if values == 0 {
            InferredType::Null
        } else if self.booleans == values {
            InferredType::Boolean
        } else if self.integers == values {
            InferredType::Integer
        } else if self.floats == values {
            InferredType::Float
        } else if self.dates == values {
            InferredType::Date
        } else if self.timestamps == values {
            InferredType::Timestamp
        } else {
            InferredType::Text
        }
    }

    /// The typed reading most cells support, for a text column: `(type,
    /// count)`, or `None` when no cell parses as anything.
    pub fn closest(&self) -> Option<(InferredType, usize)> {
        [
            (InferredType::Integer, self.integers),
            (InferredType::Float, self.floats),
            (InferredType::Boolean, self.booleans),
            (InferredType::Date, self.dates),
            (InferredType::Timestamp, self.timestamps),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        // The first of the largest: integers before floats, dates before
        // timestamps.
        .fold(
            None,
            |best: Option<(InferredType, usize)>, (t, n)| match best {
                Some((_, m)) if m >= n => best,
                _ => Some((t, n)),
            },
        )
    }
}

/// One column's inferred type and the evidence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnInference {
    name: String,
    data_type: InferredType,
    evidence: Evidence,
    overridden: bool,
}

impl ColumnInference {
    /// The column name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type: inferred, or the override.
    pub fn data_type(&self) -> InferredType {
        self.data_type
    }

    /// The counts behind it.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    /// Whether the type was given, not inferred.
    pub fn is_overridden(&self) -> bool {
        self.overridden
    }

    /// The evidence in words: `integer: 298 of 300 values, 2 null`,
    /// `text: 12 of 13 values parse as float`, or `date (override): 3 of 4
    /// values`.
    pub fn summary(&self) -> String {
        let e = &self.evidence;
        let mut out = self.data_type.name().to_string();
        if self.overridden {
            out.push_str(" (override)");
        }
        let values = e.values();
        let plural = |n: usize| if n == 1 { "value" } else { "values" };
        match self.data_type {
            InferredType::Null => out.push_str(&format!(": {} null", e.nulls)),
            InferredType::Text if !self.overridden => match e.closest() {
                Some((t, n)) => out.push_str(&format!(
                    ": {n} of {values} {} parse as {t}",
                    plural(values)
                )),
                None => out.push_str(&format!(": {values} {}", plural(values))),
            },
            t => out.push_str(&format!(": {} of {values} {}", e.parsed(t), plural(values))),
        }
        if e.nulls > 0 && self.data_type != InferredType::Null {
            out.push_str(&format!(", {} null", e.nulls));
        }
        out
    }
}

/// The null tokens [`Inferrer::new`] starts with: an empty cell, `null`,
/// `NULL`, `NA` and `N/A`.
pub const DEFAULT_NULL_TOKENS: [&str; 5] = ["", "null", "NULL", "NA", "N/A"];

/// Infers column types. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Inferrer {
    nulls: Vec<String>,
    overrides: HashMap<String, InferredType>,
}

impl Default for Inferrer {
    fn default() -> Self {
        Inferrer {
            nulls: DEFAULT_NULL_TOKENS.map(String::from).to_vec(),
            overrides: HashMap::new(),
        }
    }
}

impl Inferrer {
    /// The default null tokens and no overrides.
    pub fn new() -> Self {
        Inferrer::default()
    }

    /// Replace the null tokens (compared after trimming whitespace).
    pub fn null_tokens(mut self, tokens: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.nulls = tokens.into_iter().map(Into::into).collect();
        self
    }

    /// Give the column called `column` this type, whatever its cells say.
    pub fn override_type(mut self, column: impl Into<String>, data_type: InferredType) -> Self {
        self.overrides.insert(column.into(), data_type);
        self
    }

    /// Infer every column of `rows`.
    pub fn infer(&self, rows: &Rows) -> Inference {
        let columns = rows
            .columns()
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let mut evidence = Evidence::default();
                for value in rows.column(index) {
                    self.count(value, &mut evidence);
                }
                let (data_type, overridden) = match self.overrides.get(name) {
                    Some(&t) => (t, true),
                    None => (evidence.best(), false),
                };
                ColumnInference {
                    name: name.clone(),
                    data_type,
                    evidence,
                    overridden,
                }
            })
            .collect();
        Inference {
            columns,
            nulls: self.nulls.clone(),
        }
    }

    fn count(&self, value: &Value, e: &mut Evidence) {
        e.cells += 1;
        match value {
            Value::Null => e.nulls += 1,
            Value::Int(_) => {
                e.integers += 1;
                e.floats += 1;
            }
            Value::Float(_) => e.floats += 1,
            Value::Str(_) | Value::Text(_) => {
                let text = value.plain();
                let text = text.trim();
                if is_null(&self.nulls, text) {
                    e.nulls += 1;
                    return;
                }
                if parse_bool(text).is_some() {
                    e.booleans += 1;
                }
                if parse_int(text).is_some() {
                    e.integers += 1;
                }
                if parse_float(text).is_some() {
                    e.floats += 1;
                }
                if is_date(text) {
                    e.dates += 1;
                    e.timestamps += 1;
                } else if is_timestamp(text) {
                    e.timestamps += 1;
                }
            }
        }
    }
}

fn is_null(tokens: &[String], text: &str) -> bool {
    tokens.iter().any(|t| t == text)
}

/// Every column's inferred type, from [`Inferrer::infer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inference {
    columns: Vec<ColumnInference>,
    nulls: Vec<String>,
}

impl Inference {
    /// One entry per column, in order.
    pub fn columns(&self) -> &[ColumnInference] {
        &self.columns
    }

    /// The schema these types describe: a field per column, nullable when
    /// any cell was null.
    pub fn schema(&self) -> Schema {
        Schema::new(self.columns.iter().map(|c| {
            Field::new(c.name.clone(), c.data_type.data_type()).nullable(c.evidence.nulls > 0)
        }))
    }

    /// Convert `rows`' cells to their columns' types and attach
    /// [`schema`](Self::schema): null tokens become [`Value::Null`], integer
    /// columns [`Value::Int`], float columns [`Value::Float`]. Booleans,
    /// dates and timestamps stay as written ([`Value`] has no variant for
    /// them) but are typed in the schema. A cell that does not parse as its
    /// column's (overridden) type is left as it was.
    pub fn apply(&self, rows: &mut Rows) {
        let types: Vec<InferredType> = self.columns.iter().map(|c| c.data_type).collect();
        for row in rows.rows_mut() {
            for (cell, &data_type) in row.iter_mut().zip(&types) {
                if data_type == InferredType::Text {
                    continue;
                }
                let converted = match &*cell {
                    Value::Str(_) | Value::Text(_) => {
                        let plain = cell.plain();
                        let text = plain.trim();
                        if is_null(&self.nulls, text) {
                            Some(Value::Null)
                        } else {
                            match data_type {
                                InferredType::Integer => parse_int(text).map(Value::Int),
                                InferredType::Float => parse_float(text).map(Value::Float),
                                _ => None,
                            }
                        }
                    }
                    Value::Int(n) if data_type == InferredType::Float => {
                        Some(Value::Float(*n as f64))
                    }
                    _ => None,
                };
                if let Some(converted) = converted {
                    *cell = converted;
                }
            }
        }
        rows.set_schema(Some(self.schema()));
    }
}

/// A table of column, type and evidence.
impl Renderable for Inference {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.to_table().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.to_table().measure(console, options)
    }
}

impl Inference {
    /// The table this renders as: column, type, parsed, nulls.
    pub fn to_table(&self) -> Table {
        let mut table = Table::new();
        table.add_column("column");
        table.add_column("type");
        table.add_column_justify("parsed", Justify::Right);
        table.add_column_justify("nulls", Justify::Right);
        for c in &self.columns {
            let mut kind = c.data_type.name().to_string();
            if c.overridden {
                kind.push_str(" (override)");
            }
            let parsed = match c.data_type {
                InferredType::Null => String::new(),
                InferredType::Text => match c.evidence.closest() {
                    Some((t, n)) if !c.overridden => format!("{n}/{} {t}", c.evidence.values()),
                    _ => format!("{}/{}", c.evidence.values(), c.evidence.values()),
                },
                t => format!("{}/{}", c.evidence.parsed(t), c.evidence.values()),
            };
            table.add_row_text(vec![
                rich::Text::new(c.name.clone()),
                rich::Text::new(kind),
                rich::Text::new(parsed),
                rich::Text::new(c.evidence.nulls.to_string()),
            ]);
        }
        table
    }
}

/// `true` or `false`, in any case.
pub fn parse_bool(text: &str) -> Option<bool> {
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// An optional sign and ASCII digits that fit an `i64`.
pub fn parse_int(text: &str) -> Option<i64> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// A decimal number: an optional sign, digits with an optional point, and
/// an optional exponent. `inf` and `nan` are text.
pub fn parse_float(text: &str) -> Option<f64> {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.len() + fraction.len() == 0 || !digits(whole) || !digits(fraction) {
        return None;
    }
    if let Some(exponent) = exponent {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if exponent.is_empty() || !digits(exponent) {
            return None;
        }
    }
    text.parse().ok()
}

/// `YYYY-MM-DD`, a real calendar date.
pub fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let number = |s: &[u8]| -> Option<u32> {
        s.iter().try_fold(0u32, |n, &c| {
            c.is_ascii_digit().then(|| n * 10 + u32::from(c - b'0'))
        })
    };
    let (Some(year), Some(month), Some(day)) = (number(&b[..4]), number(&b[5..7]), number(&b[8..]))
    else {
        return false;
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// A date, `T` or a space, `HH:MM`, optional `:SS` and fraction, and an
/// optional `Z` or `±HH:MM` / `±HHMM` offset.
pub fn is_timestamp(text: &str) -> bool {
    if text.len() < 16 || !text.is_char_boundary(10) || !is_date(&text[..10]) {
        return false;
    }
    let rest = &text.as_bytes()[10..];
    if rest[0] != b'T' && rest[0] != b't' && rest[0] != b' ' {
        return false;
    }
    let mut time = &rest[1..];
    let two = |s: &[u8], max: u32| -> bool {
        s.len() >= 2
            && s[0].is_ascii_digit()
            && s[1].is_ascii_digit()
            && u32::from(s[0] - b'0') * 10 + u32::from(s[1] - b'0') <= max
    };
    if !(two(time, 23) && time.get(2) == Some(&b':') && two(&time[3..], 59)) {
        return false;
    }
    time = &time[5..];
    if time.first() == Some(&b':') {
        if !two(&time[1..], 60) {
            return false;
        }
        time = &time[3..];
        if time.first() == Some(&b'.') {
            let digits = time[1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 {
                return false;
            }
            time = &time[1 + digits..];
        }
    }
    match time {
        [] | [b'Z'] | [b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] | [b'+' | b'-', h1, h2, m1, m2] => {
            two(&[*h1, *h2], 23) && two(&[*m1, *m2], 59)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_parse_strictly() {
        assert_eq!(parse_int("-42"), Some(-42));
        assert_eq!(parse_int("+7"), Some(7));
        assert_eq!(parse_int("1.0"), None);
        assert_eq!(parse_int("99999999999999999999"), None);
        assert_eq!(parse_float("99999999999999999999"), Some(1e20));
        assert_eq!(parse_float(".5"), Some(0.5));
        assert_eq!(parse_float("5."), Some(5.0));
        assert_eq!(parse_float("-1.5e-3"), Some(-0.0015));
        for text in ["inf", "NaN", ".", "-", "1e", "1_000", "0x10", " 1", "1.2.3"] {
            assert_eq!(parse_float(text), None, "{text}");
        }
        assert_eq!(parse_bool("TRUE"), Some(true));
        assert_eq!(parse_bool("yes"), None);
    }

    #[test]
    fn dates_and_timestamps_are_checked() {
        assert!(is_date("2024-02-29"));
        assert!(!is_date("2023-02-29"));
        assert!(!is_date("2024-13-01"));
        assert!(!is_date("2024-1-01"));
        for ok in [
            "2024-02-29T23:59",
            "2024-02-29 23:59:60",
            "2024-02-29T00:00:00.123456Z",
            "2024-02-29T00:00:00+05:30",
            "2024-02-29T00:00-0800",
        ] {
            assert!(is_timestamp(ok), "{ok}");
        }
        for bad in [
            "2024-02-29",
            "2024-02-29T24:00",
            "2024-02-29T10:00:00.",
            "2024-02-29T10:00+5",
            "2024-02-29X10:00",
        ] {
            assert!(!is_timestamp(bad), "{bad}");
        }
    }

    #[test]
    fn columns_take_the_narrowest_type_and_overrides_win() {
        let mut rows = Rows::new(["n", "x", "b", "empty", "mixed"]);
        rows.push([
            "1".into(),
            "1".into(),
            "true".into(),
            "".into(),
            Value::Int(3),
        ]);
        rows.push([
            "2".into(),
            "2.5".into(),
            "False".into(),
            "NA".into(),
            "x".into(),
        ]);
        let inference = Inferrer::new()
            .override_type("n", InferredType::Float)
            .infer(&rows);
        let types: Vec<InferredType> = inference.columns().iter().map(|c| c.data_type()).collect();
        assert_eq!(
            types,
            [
                InferredType::Float,
                InferredType::Float,
                InferredType::Boolean,
                InferredType::Null,
                InferredType::Text
            ]
        );
        assert!(inference.columns()[0].is_overridden());
        assert_eq!(
            inference.columns()[0].summary(),
            "float (override): 2 of 2 values"
        );
        assert_eq!(inference.columns()[3].summary(), "null: 2 null");
        let schema = inference.schema();
        assert!(schema.fields()[3].is_nullable());
        assert!(!schema.fields()[0].is_nullable());

        inference.apply(&mut rows);
        assert_eq!(rows.rows()[0][0], Value::Float(1.0));
        assert_eq!(rows.rows()[1][1], Value::Float(2.5));
        assert_eq!(rows.rows()[1][2], Value::from("False"));
        assert_eq!(rows.rows()[1][3], Value::Null);
        assert_eq!(rows.rows()[0][4], Value::Int(3));
        assert_eq!(
            rows.schema().unwrap().fields()[2].data_type(),
            &DataType::Boolean
        );
    }

    #[test]
    fn the_report_is_a_table() {
        let mut rows = Rows::new(["id"]);
        rows.push(["1".into()]);
        rows.push(["x".into()]);
        let inference = Inferrer::new().infer(&rows);
        let out = Console::builder()
            .width(40)
            .build()
            .render_export(&inference);
        assert!(
            out.contains("│ id     │ text │ 1/2 integer │     0 │"),
            "{out}"
        );
    }
}
