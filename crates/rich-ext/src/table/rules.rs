//! Conditional styles (#215): rules that style a cell, a row or a column by
//! value.
//!
//! A [`StyleRule`] names a column, a condition on that column's value and a
//! style. The condition is either a [`Comparison`] against a value
//! (`status == "failed"`, `latency > 250`) or a Rust predicate; there is no
//! expression language. The rule's [`Target`] decides what the style lands
//! on: the matching cell (the default), the whole row, or the whole column
//! when any of its cells matches.
//!
//! [`TableData::style_rules`](super::TableData::style_rules) applies a set of
//! rules while rendering; any other renderer resolves them once with
//! [`StyleRules::resolve`] and asks the result for each row's and cell's
//! style. With the `toml` feature, [`StyleRules::from_toml`] reads the same
//! rules from TOML rule tables:
//!
//! ```toml
//! [[rules]]
//! column = "status"     # a header, or a 0-based index
//! op = "eq"             # eq ne lt le gt ge contains starts_with ends_with
//! value = "failed"      #   empty not_empty (or == != < <= > >=)
//! style = "bold red"
//! target = "row"        # cell (default), row or column
//! ```
//!
//! # Rule specs
//!
//! [`StyleRule::parse`] reads one rule from a line of text, as `rich
//! --style-rule` takes it: `COLUMN OP VALUE:STYLE`, or `COLUMN OP
//! VALUE:TARGET:STYLE`. Each part maps onto one of the rule's:
//!
//! | Part | Written as |
//! |---|---|
//! | `COLUMN` | a header (`p99 ms`), a header in double quotes when it holds a comparison, a `:` or a comparison word (`"a<b"`), or `#N` for the 0-based index `N` |
//! | `OP` | `==` or `=` (eq), `!=` (ne), `<` (lt), `<=` (le), `>` (gt), `>=` (ge), `*=` (contains), `^=` (starts_with), `$=` (ends_with), or any [`Comparison`] word after a space (`status eq failed`, `note empty`) |
//! | `VALUE` | the text up to the next `:`, trimmed: an integer or a float compares numerically, anything else as text; in double quotes it is always text and may hold `:` (`"12:30"`, `"200"`). `empty` and `not_empty` take none |
//! | `TARGET` | `cell` (the default), `row` or `column` |
//! | `STYLE` | the rest: a style definition, which may itself hold `:` (`link https://…`) |
//!
//! In quotes, `\"`, `\\`, `\n`, `\t` and `\r` are escapes. [`StyleRule::to_spec`]
//! writes a rule back as a spec, quoting where it must.
//!
//! ```
//! use rich_ext::table::{Comparison, StyleRule, Target, Value};
//!
//! let rule = StyleRule::parse("status=failed:row:on red").unwrap();
//! let (op, value) = rule.comparison().unwrap();
//! assert_eq!((op, value.plain().as_str()), (Comparison::Eq, "failed"));
//! assert_eq!(rule.target_kind(), Target::Row);
//! assert!(rule.matches(&Value::from("failed")));
//!
//! let rule: StyleRule = "latency>500:bold red".parse().unwrap();
//! assert!(rule.matches(&Value::from("612")));
//! assert_eq!(rule.to_spec().as_deref(), Some("latency>500:bold red"));
//!
//! let error = StyleRule::parse("latency>:red").unwrap_err();
//! assert!(error.to_string().starts_with("rule \"latency>:red\": no value after `>`"));
//! ```
//!
//! ```
//! use rich::{Console, Style};
//! use rich_ext::table::{Column, Comparison, StyleRule, StyleRules, TableData, Target, Value};
//!
//! let rules = StyleRules::new()
//!     .rule(StyleRule::new("p99 ms", Comparison::Gt, 100, Style::parse("red").unwrap()))
//!     .rule(StyleRule::when("service", |v| v.plain() == "db", Style::parse("dim").unwrap())
//!         .target(Target::Row));
//! let mut data = TableData::new([Column::new("service"), Column::new("p99 ms")]);
//! data.push(["web".into(), Value::Int(120)]);
//! data.push(["db".into(), Value::Int(35)]);
//!
//! let resolved = rules.resolve(&["service", "p99 ms"], data.rows());
//! assert!(resolved.cell_style(&data.rows()[0], 1).is_some());
//! assert!(resolved.cell_style(&data.rows()[1], 1).is_none());
//! assert!(resolved.row_style(&data.rows()[1]).is_some());
//!
//! // Rules restyle; they never change the text.
//! let data = data.style_rules(rules);
//! let out = Console::builder().width(40).build().render_export(&data);
//! assert!(out.contains("│ web     │ 120    │"));
//! ```

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use rich::Style;

use super::Value;

/// What a matching rule styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    /// The cell that matched.
    #[default]
    Cell,
    /// Every cell of the row the matching cell is in.
    Row,
    /// Every cell of the column, when any of its cells matches.
    Column,
}

impl Target {
    /// `cell`, `row` or `column`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "cell" => Some(Target::Cell),
            "row" => Some(Target::Row),
            "column" => Some(Target::Column),
            _ => None,
        }
    }

    /// `cell`, `row` or `column`, as [`parse`](Self::parse) reads it.
    pub fn name(self) -> &'static str {
        match self {
            Target::Cell => "cell",
            Target::Row => "row",
            Target::Column => "column",
        }
    }
}

/// The column a rule reads: a header, or a 0-based index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnRef {
    /// The first column with this header.
    Name(String),
    /// The column at this position.
    Index(usize),
}

impl From<&str> for ColumnRef {
    fn from(name: &str) -> Self {
        ColumnRef::Name(name.to_string())
    }
}

impl From<String> for ColumnRef {
    fn from(name: String) -> Self {
        ColumnRef::Name(name)
    }
}

impl From<usize> for ColumnRef {
    fn from(index: usize) -> Self {
        ColumnRef::Index(index)
    }
}

impl fmt::Display for ColumnRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnRef::Name(name) => f.write_str(name),
            ColumnRef::Index(index) => write!(f, "#{index}"),
        }
    }
}

/// How a cell is compared with a rule's value.
///
/// A numeric rule value ([`Value::Int`] or [`Value::Float`]) compares
/// numerically: a cell matches when it is a number, or text that parses as
/// one, and a cell that is neither never matches an ordering. A text value
/// compares as text, byte for byte. A [`Value::Null`] value matches empty
/// cells for [`Eq`](Comparison::Eq) and the rest for
/// [`Ne`](Comparison::Ne). [`Ne`](Comparison::Ne) is always the opposite of
/// [`Eq`](Comparison::Eq).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Comparison {
    /// Equal (`eq`, `==`).
    Eq,
    /// Not equal (`ne`, `!=`).
    Ne,
    /// Less than (`lt`, `<`).
    Lt,
    /// Less than or equal (`le`, `<=`).
    Le,
    /// Greater than (`gt`, `>`).
    Gt,
    /// Greater than or equal (`ge`, `>=`).
    Ge,
    /// The cell's text contains the value's (`contains`).
    Contains,
    /// The cell's text starts with the value's (`starts_with`).
    StartsWith,
    /// The cell's text ends with the value's (`ends_with`).
    EndsWith,
    /// The cell is null or empty text (`empty`); the value is ignored.
    Empty,
    /// The cell is not empty (`not_empty`); the value is ignored.
    NotEmpty,
}

impl Comparison {
    /// The comparison named `name`: a word (`eq`, `starts_with`) or a symbol
    /// (`==`, `<=`).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "eq" | "==" => Comparison::Eq,
            "ne" | "!=" => Comparison::Ne,
            "lt" | "<" => Comparison::Lt,
            "le" | "<=" => Comparison::Le,
            "gt" | ">" => Comparison::Gt,
            "ge" | ">=" => Comparison::Ge,
            "contains" => Comparison::Contains,
            "starts_with" => Comparison::StartsWith,
            "ends_with" => Comparison::EndsWith,
            "empty" => Comparison::Empty,
            "not_empty" => Comparison::NotEmpty,
            _ => return None,
        })
    }

    /// The comparison's word: `eq`, `ne`, `lt`, `le`, `gt`, `ge`, `contains`,
    /// `starts_with`, `ends_with`, `empty` or `not_empty`.
    pub fn name(self) -> &'static str {
        match self {
            Comparison::Eq => "eq",
            Comparison::Ne => "ne",
            Comparison::Lt => "lt",
            Comparison::Le => "le",
            Comparison::Gt => "gt",
            Comparison::Ge => "ge",
            Comparison::Contains => "contains",
            Comparison::StartsWith => "starts_with",
            Comparison::EndsWith => "ends_with",
            Comparison::Empty => "empty",
            Comparison::NotEmpty => "not_empty",
        }
    }

    /// The comparison's symbol in a rule spec ([`StyleRule::parse`]): `==`,
    /// `!=`, `<`, `<=`, `>`, `>=`, `*=` (contains), `^=` (starts with) or `$=`
    /// (ends with); `None` for `empty` and `not_empty`, which are words.
    pub fn symbol(self) -> Option<&'static str> {
        Some(match self {
            Comparison::Eq => "==",
            Comparison::Ne => "!=",
            Comparison::Lt => "<",
            Comparison::Le => "<=",
            Comparison::Gt => ">",
            Comparison::Ge => ">=",
            Comparison::Contains => "*=",
            Comparison::StartsWith => "^=",
            Comparison::EndsWith => "$=",
            Comparison::Empty | Comparison::NotEmpty => return None,
        })
    }

    /// Whether `cell` compares to `operand` this way.
    pub fn test(self, cell: &Value, operand: &Value) -> bool {
        match self {
            Comparison::Empty => cell.is_empty(),
            Comparison::NotEmpty => !cell.is_empty(),
            Comparison::Ne => !Comparison::Eq.test(cell, operand),
            Comparison::Contains => cell.plain().contains(&operand.plain()),
            Comparison::StartsWith => cell.plain().starts_with(&operand.plain()),
            Comparison::EndsWith => cell.plain().ends_with(&operand.plain()),
            Comparison::Eq => match operand {
                Value::Null => cell.is_empty(),
                Value::Int(_) | Value::Float(_) => order(cell, operand) == Some(Ordering::Equal),
                _ => cell.plain() == operand.plain(),
            },
            Comparison::Lt | Comparison::Le | Comparison::Gt | Comparison::Ge => {
                let Some(ordering) = order(cell, operand) else {
                    return false;
                };
                match self {
                    Comparison::Lt => ordering == Ordering::Less,
                    Comparison::Le => ordering != Ordering::Greater,
                    Comparison::Gt => ordering == Ordering::Greater,
                    _ => ordering != Ordering::Less,
                }
            }
        }
    }
}

/// `cell` against `operand`: numerically when the operand is a number, as
/// text when it is text, and not at all against null.
fn order(cell: &Value, operand: &Value) -> Option<Ordering> {
    match operand {
        Value::Null => None,
        // Two integers compare exactly: as `f64` they agree past 2^53.
        Value::Int(right) => {
            let left = match cell {
                Value::Int(left) => Some(*left),
                Value::Str(_) | Value::Text(_) => cell.plain().trim().parse::<i64>().ok(),
                _ => None,
            };
            match left {
                Some(left) => Some(left.cmp(right)),
                None => order(cell, &Value::Float(*right as f64)),
            }
        }
        Value::Float(_) => {
            let left = cell
                .as_f64()
                .or_else(|| match cell {
                    Value::Str(_) | Value::Text(_) => cell.plain().trim().parse::<f64>().ok(),
                    _ => None,
                })
                .filter(|n| !n.is_nan())?;
            left.partial_cmp(&operand.as_f64()?)
        }
        _ if matches!(cell, Value::Null) => None,
        _ => Some(cell.plain().cmp(&operand.plain())),
    }
}

/// A predicate over one cell's value.
pub type Predicate = Arc<dyn Fn(&Value) -> bool + Send + Sync>;

#[derive(Clone)]
enum Condition {
    Compare(Comparison, Box<Value>),
    Predicate(Predicate),
}

impl fmt::Debug for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Condition::Compare(op, value) => {
                f.debug_tuple("Compare").field(op).field(value).finish()
            }
            Condition::Predicate(_) => f.write_str("Predicate(..)"),
        }
    }
}

/// One conditional style. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct StyleRule {
    column: ColumnRef,
    condition: Condition,
    style: Style,
    target: Target,
}

impl StyleRule {
    /// Style a cell of `column` when it compares to `value` by `comparison`.
    pub fn new(
        column: impl Into<ColumnRef>,
        comparison: Comparison,
        value: impl Into<Value>,
        style: Style,
    ) -> Self {
        StyleRule {
            column: column.into(),
            condition: Condition::Compare(comparison, Box::new(value.into())),
            style,
            target: Target::Cell,
        }
    }

    /// Style a cell of `column` when `predicate` holds for its value.
    pub fn when(
        column: impl Into<ColumnRef>,
        predicate: impl Fn(&Value) -> bool + Send + Sync + 'static,
        style: Style,
    ) -> Self {
        StyleRule {
            column: column.into(),
            condition: Condition::Predicate(Arc::new(predicate)),
            style,
            target: Target::Cell,
        }
    }

    /// Style the row or the column instead of the cell.
    #[must_use]
    pub fn target(mut self, target: Target) -> Self {
        self.target = target;
        self
    }

    /// The column the rule reads.
    pub fn column(&self) -> &ColumnRef {
        &self.column
    }

    /// The style it applies.
    pub fn style(&self) -> &Style {
        &self.style
    }

    /// What it styles.
    pub fn target_kind(&self) -> Target {
        self.target
    }

    /// Whether `value` meets the rule's condition.
    pub fn matches(&self, value: &Value) -> bool {
        match &self.condition {
            Condition::Compare(op, operand) => op.test(value, operand),
            Condition::Predicate(predicate) => predicate(value),
        }
    }

    /// The comparison and the value it compares with; `None` for a rule with
    /// a Rust predicate.
    pub fn comparison(&self) -> Option<(Comparison, &Value)> {
        match &self.condition {
            Condition::Compare(op, operand) => Some((*op, &**operand)),
            Condition::Predicate(_) => None,
        }
    }

    /// Read a rule spec, `COLUMN OP VALUE:STYLE` or `COLUMN OP
    /// VALUE:TARGET:STYLE` (`latency>500:bold red`,
    /// `status=failed:row:on red`). See [Rule specs](self#rule-specs). The
    /// error names the spec.
    pub fn parse(spec: &str) -> Result<Self, RuleError> {
        parse_spec(spec).map_err(|message| RuleError(format!("rule {spec:?}: {message}")))
    }

    /// The rule as a spec [`parse`](Self::parse) reads back to the same
    /// rule: `None` for a Rust predicate, an ordering against
    /// [`Value::Null`] or a float that is not finite, which a spec cannot
    /// write.
    pub fn to_spec(&self) -> Option<String> {
        let (op, operand) = self.comparison()?;
        let column = match &self.column {
            ColumnRef::Index(index) => format!("#{index}"),
            ColumnRef::Name(name) if bare_column(name) => name.clone(),
            ColumnRef::Name(name) => quote(name),
        };
        let condition = match (op, operand) {
            (Comparison::Empty, _) | (Comparison::Eq, Value::Null) => format!("{column} empty"),
            (Comparison::NotEmpty, _) | (Comparison::Ne, Value::Null) => {
                format!("{column} not_empty")
            }
            (_, Value::Null) => return None,
            (op, operand) => format!("{column}{}{}", op.symbol()?, spec_value(operand)?),
        };
        let target = match self.target {
            Target::Cell => "",
            Target::Row => "row:",
            Target::Column => "column:",
        };
        Some(format!("{condition}:{target}{}", self.style.definition()))
    }
}

/// The spec ([`StyleRule::to_spec`]), or `COLUMN <predicate>:STYLE` for a
/// rule with a Rust predicate.
impl fmt::Display for StyleRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_spec() {
            Some(spec) => f.write_str(&spec),
            None => {
                let target = match self.target {
                    Target::Cell => "",
                    Target::Row => "row:",
                    Target::Column => "column:",
                };
                write!(
                    f,
                    "{} <predicate>:{target}{}",
                    self.column,
                    self.style.definition()
                )
            }
        }
    }
}

impl std::str::FromStr for StyleRule {
    type Err = RuleError;

    fn from_str(spec: &str) -> Result<Self, RuleError> {
        StyleRule::parse(spec)
    }
}

/// Comparison symbols, longest first so `<=` is not read as `<`.
const SYMBOLS: &[(&str, Comparison)] = &[
    ("==", Comparison::Eq),
    ("!=", Comparison::Ne),
    ("<=", Comparison::Le),
    (">=", Comparison::Ge),
    ("*=", Comparison::Contains),
    ("^=", Comparison::StartsWith),
    ("$=", Comparison::EndsWith),
    ("=", Comparison::Eq),
    ("<", Comparison::Lt),
    (">", Comparison::Gt),
];

/// Comparison words, written with whitespace before them.
const WORDS: &[&str] = &[
    "eq",
    "ne",
    "lt",
    "le",
    "gt",
    "ge",
    "contains",
    "starts_with",
    "ends_with",
    "empty",
    "not_empty",
];

/// The comparison `text` starts with and its length in bytes: a symbol, or
/// (when `word` allows it) a word followed by whitespace, a quote, `:` or
/// nothing.
fn operator_at(text: &str, word: bool) -> Option<(Comparison, usize)> {
    if let Some((symbol, op)) = SYMBOLS.iter().find(|(s, _)| text.starts_with(s)) {
        return Some((*op, symbol.len()));
    }
    if !word {
        return None;
    }
    WORDS.iter().find_map(|name| {
        let rest = text.strip_prefix(name)?;
        let ends = rest.is_empty()
            || rest.starts_with(|c: char| c.is_whitespace() || c == ':' || c == '"');
        ends.then(|| (Comparison::parse(name).expect("a known word"), name.len()))
    })
}

/// A quoted name or value at the start of `text` (which starts with `"`):
/// its text, with `\"`, `\\`, `\n`, `\t` and `\r` escapes, and what follows
/// the closing quote.
fn unquote(text: &str) -> Result<(String, &str), String> {
    let mut out = String::new();
    let mut chars = text.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, &text[i + 1..])),
            '\\' => match chars.next() {
                Some((_, '"')) => out.push('"'),
                Some((_, '\\')) => out.push('\\'),
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, 'r')) => out.push('\r'),
                Some((_, other)) => {
                    return Err(format!(
                        "unknown escape `\\{other}` in quotes (use \\\" \\\\ \\n \\t or \\r)"
                    ))
                }
                None => break,
            },
            c => out.push(c),
        }
    }
    Err("a quote is not closed".into())
}

/// `text` in quotes, escaped as [`unquote`] reads it.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An unquoted value: an integer, a float (any text with a digit that
/// parses as a finite one), else text.
fn spec_operand(text: &str) -> Value {
    if let Ok(n) = text.parse::<i64>() {
        return Value::Int(n);
    }
    if text.bytes().any(|b| b.is_ascii_digit()) {
        if let Ok(n) = text.parse::<f64>() {
            if n.is_finite() {
                return Value::Float(n);
            }
        }
    }
    Value::Str(text.to_string())
}

/// Whether a column name reads back unquoted: no comparison characters, `:`
/// or quotes, no leading `#` or `"`, no surrounding whitespace, and no
/// comparison word after whitespace.
fn bare_column(name: &str) -> bool {
    !name.is_empty()
        && name.trim() == name
        && !name.starts_with('#')
        && !name.contains(|c: char| "=<>!*^$:\"\\".contains(c) || c.is_control())
        && !name
            .split_whitespace()
            .skip(1)
            .any(|word| WORDS.contains(&word))
}

/// A value as a spec writes it: numbers as they are, text bare when it
/// reads back as the same text, else quoted. `None` for a float that is
/// not finite.
fn spec_value(value: &Value) -> Option<String> {
    Some(match value {
        Value::Null => return None,
        Value::Int(n) => n.to_string(),
        Value::Float(f) if f.is_finite() => format!("{f:?}"),
        Value::Float(_) => return None,
        Value::Str(_) | Value::Text(_) => {
            let text = value.plain();
            let bare = !text.is_empty()
                && text.trim() == text
                && !text.starts_with('"')
                && !text.contains(|c: char| c == ':' || c.is_control())
                && matches!(spec_operand(&text), Value::Str(_));
            if bare {
                text
            } else {
                quote(&text)
            }
        }
    })
}

/// [`StyleRule::parse`], with the message alone.
fn parse_spec(spec: &str) -> Result<StyleRule, String> {
    const HINT: &str = "expected COLUMN OP VALUE:STYLE, e.g. latency>500:bold red";
    let text = spec.trim();
    if text.is_empty() {
        return Err(format!("empty; {HINT}"));
    }
    // The column: quoted, or everything before the first comparison that
    // comes before the first `:`.
    let (column, op, rest) = if text.starts_with('"') {
        let (name, rest) = unquote(text)?;
        let after = rest.trim_start();
        let spaced = after.len() < rest.len();
        let Some((op, len)) = operator_at(after, spaced) else {
            return Err(format!("no comparison after the quoted column; {HINT}"));
        };
        (ColumnRef::Name(name), op, &after[len..])
    } else {
        let end = text.find(':').unwrap_or(text.len());
        let found = text[..end].char_indices().find_map(|(at, _)| {
            let word = text[..at].ends_with(char::is_whitespace);
            operator_at(&text[at..], word).map(|hit| (at, hit))
        });
        let Some((at, (op, len))) = found else {
            return Err(format!(
                "no comparison (== != < <= > >= *= ^= $=, or a word such as contains or \
                 empty) before the first `:`; quote a column name that holds one; {HINT}"
            ));
        };
        let name = text[..at].trim();
        if name.is_empty() {
            return Err(format!("no column before the comparison; {HINT}"));
        }
        let column = match name.strip_prefix('#') {
            Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
                ColumnRef::Index(
                    digits
                        .parse()
                        .map_err(|_| format!("column index {digits} is too large"))?,
                )
            }
            _ => ColumnRef::Name(name.to_string()),
        };
        (column, op, &text[at + len..])
    };
    // The value, up to the `:` before the style.
    let rest = rest.trim_start();
    let (operand, rest) = if matches!(op, Comparison::Empty | Comparison::NotEmpty) {
        match rest.strip_prefix(':') {
            Some(rest) => (Value::Null, rest),
            None if rest.is_empty() => return Err(format!("no `:` and style; {HINT}")),
            None => return Err(format!("`{}` takes no value", op.name())),
        }
    } else if rest.starts_with('"') {
        let (value, after) = unquote(rest)?;
        match after.trim_start().strip_prefix(':') {
            Some(after) => (Value::Str(value), after),
            None => return Err(format!("no `:` and style after the quoted value; {HINT}")),
        }
    } else {
        let Some((value, after)) = rest.split_once(':') else {
            return Err(format!("no `:` and style after the value; {HINT}"));
        };
        let value = value.trim();
        if value.is_empty() {
            return Err(format!(
                "no value after `{}` (quote an empty one: \"\")",
                op.symbol().unwrap_or(op.name())
            ));
        }
        (spec_operand(value), after)
    };
    // An optional target, then the style (which may hold `:`, as a link's
    // URL does).
    let rest = rest.trim();
    let (target, style) = match rest.split_once(':') {
        Some((head, tail)) => match Target::parse(head.trim()) {
            Some(target) => (target, tail.trim()),
            None => (Target::Cell, rest),
        },
        None => (Target::Cell, rest),
    };
    if style.is_empty() || Target::parse(style).is_some() {
        return Err(format!("no style after the `:`; {HINT}"));
    }
    let style = Style::parse(style).map_err(|e| format!("style {style:?}: {e}"))?;
    Ok(StyleRule::new(column, op, operand, style).target(target))
}

/// An ordered set of [`StyleRule`]s. Where several match, their styles
/// combine in order, a later rule winning wherever both set something.
#[derive(Clone, Debug, Default)]
pub struct StyleRules {
    rules: Vec<StyleRule>,
}

impl StyleRules {
    /// No rules.
    pub fn new() -> Self {
        StyleRules::default()
    }

    /// Add a rule (builder form).
    #[must_use]
    pub fn rule(mut self, rule: StyleRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add a rule.
    pub fn push(&mut self, rule: StyleRule) -> &mut Self {
        self.rules.push(rule);
        self
    }

    /// The rules, in order.
    pub fn rules(&self) -> &[StyleRule] {
        &self.rules
    }

    /// The number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether there are no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Resolve the rules against a table's `headers` and `rows`: each rule's
    /// column, and the styles of column rules (which need every row, so the
    /// rows are only read when a column rule exists).
    pub fn resolve<'a, R>(
        &'a self,
        headers: &[&str],
        rows: impl IntoIterator<Item = R>,
    ) -> ResolvedRules<'a>
    where
        R: AsRef<[Value]>,
    {
        let columns: Vec<Option<usize>> = self
            .rules
            .iter()
            .map(|rule| match &rule.column {
                ColumnRef::Name(name) => headers.iter().position(|h| h == name),
                ColumnRef::Index(index) => (*index < headers.len()).then_some(*index),
            })
            .collect();
        let mut column_styles: Vec<Option<Style>> = vec![None; headers.len()];
        let column_rules: Vec<usize> = (0..self.rules.len())
            .filter(|&i| self.rules[i].target == Target::Column && columns[i].is_some())
            .collect();
        if !column_rules.is_empty() {
            let mut hit = vec![false; self.rules.len()];
            for row in rows {
                let row = row.as_ref();
                for &i in &column_rules {
                    if !hit[i] {
                        let column = columns[i].expect("resolved above");
                        hit[i] = row.get(column).is_some_and(|v| self.rules[i].matches(v));
                    }
                }
                if column_rules.iter().all(|&i| hit[i]) {
                    break;
                }
            }
            for &i in column_rules.iter().filter(|&&i| hit[i]) {
                let column = columns[i].expect("resolved above");
                combine(&mut column_styles[column], &self.rules[i].style);
            }
        }
        ResolvedRules {
            rules: self,
            columns,
            column_styles,
        }
    }
}

fn combine(slot: &mut Option<Style>, style: &Style) {
    *slot = Some(match slot.take() {
        Some(previous) => previous.combine(style),
        None => style.clone(),
    });
}

/// [`StyleRules`] resolved against one table, from
/// [`StyleRules::resolve`].
#[derive(Clone, Debug)]
pub struct ResolvedRules<'a> {
    rules: &'a StyleRules,
    columns: Vec<Option<usize>>,
    column_styles: Vec<Option<Style>>,
}

impl ResolvedRules<'_> {
    /// The style for the whole of `row`: its row rules that match, combined.
    pub fn row_style(&self, row: &[Value]) -> Option<Style> {
        let mut style = None;
        for (rule, column) in self.rules.rules.iter().zip(&self.columns) {
            if rule.target != Target::Row {
                continue;
            }
            if let Some(value) = column.and_then(|c| row.get(c)) {
                if rule.matches(value) {
                    combine(&mut style, &rule.style);
                }
            }
        }
        style
    }

    /// The style for `row`'s cell in `column`: the column's style from
    /// column rules, then its cell rules that match, combined.
    pub fn cell_style(&self, row: &[Value], column: usize) -> Option<Style> {
        let mut style = self.column_styles.get(column).cloned().flatten();
        let Some(value) = row.get(column) else {
            return style;
        };
        for (rule, resolved) in self.rules.rules.iter().zip(&self.columns) {
            if rule.target == Target::Cell && *resolved == Some(column) && rule.matches(value) {
                combine(&mut style, &rule.style);
            }
        }
        style
    }

    /// The style column rules give `column`, if any matched.
    pub fn column_style(&self, column: usize) -> Option<&Style> {
        self.column_styles.get(column).and_then(Option::as_ref)
    }

    /// The rules whose column is not in the table (a header that is not
    /// there, an index past the end). They never match.
    pub fn unresolved(&self) -> Vec<&StyleRule> {
        self.rules
            .rules
            .iter()
            .zip(&self.columns)
            .filter(|(_, column)| column.is_none())
            .map(|(rule, _)| rule)
            .collect()
    }
}

/// Why rule tables could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleError(String);

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuleError {}

#[cfg(feature = "toml")]
impl StyleRules {
    /// Read the `[[rules]]` tables of a TOML document. See the
    /// [module docs](self) for the keys.
    pub fn from_toml(text: &str) -> Result<Self, RuleError> {
        let table: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| RuleError(format!("not TOML: {}", e.message())))?;
        match table.get("rules") {
            Some(rules) => Self::from_toml_value(rules),
            None => Ok(StyleRules::new()),
        }
    }

    /// Read rule tables from an array of tables, wherever a configuration
    /// keeps them (a profile's `rules` key, say).
    pub fn from_toml_value(value: &toml::Value) -> Result<Self, RuleError> {
        let toml::Value::Array(items) = value else {
            return Err(RuleError("rules: expected an array of tables".into()));
        };
        let mut rules = StyleRules::new();
        for (index, item) in items.iter().enumerate() {
            let at = |message: String| RuleError(format!("rules[{index}]: {message}"));
            let toml::Value::Table(table) = item else {
                return Err(at("expected a table".into()));
            };
            if let Some(key) = table
                .keys()
                .find(|k| !matches!(k.as_str(), "column" | "op" | "value" | "style" | "target"))
            {
                return Err(at(format!("unknown key `{key}`")));
            }
            let column = match table.get("column") {
                Some(toml::Value::String(name)) => ColumnRef::Name(name.clone()),
                Some(toml::Value::Integer(n)) => usize::try_from(*n)
                    .map(ColumnRef::Index)
                    .map_err(|_| at(format!("column index {n} is negative")))?,
                Some(_) => return Err(at("`column` is a header or an index".into())),
                None => return Err(at("missing `column`".into())),
            };
            let op = match table.get("op") {
                None => Comparison::Eq,
                Some(toml::Value::String(name)) => {
                    Comparison::parse(name).ok_or_else(|| at(format!("unknown op `{name}`")))?
                }
                Some(_) => return Err(at("`op` is a string".into())),
            };
            let operand = match table.get("value") {
                None if matches!(op, Comparison::Empty | Comparison::NotEmpty) => Value::Null,
                None => return Err(at("missing `value`".into())),
                Some(toml::Value::String(s)) => Value::Str(s.clone()),
                Some(toml::Value::Integer(n)) => Value::Int(*n),
                Some(toml::Value::Float(f)) => Value::Float(*f),
                Some(toml::Value::Boolean(b)) => Value::Str(b.to_string()),
                Some(_) => return Err(at("`value` is a string, number or boolean".into())),
            };
            let style = match table.get("style") {
                Some(toml::Value::String(spec)) => {
                    Style::parse(spec).map_err(|e| at(format!("style `{spec}`: {e}")))?
                }
                Some(_) => return Err(at("`style` is a string".into())),
                None => return Err(at("missing `style`".into())),
            };
            let target = match table.get("target") {
                None => Target::Cell,
                Some(toml::Value::String(name)) => {
                    Target::parse(name).ok_or_else(|| at(format!("unknown target `{name}`")))?
                }
                Some(_) => return Err(at("`target` is a string".into())),
            };
            rules.push(StyleRule::new(column, op, operand, style).target(target));
        }
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Style {
        Style::parse("red").unwrap()
    }

    #[test]
    fn comparisons_read_numbers_from_text() {
        let cell = Value::Str("12.5".into());
        assert!(Comparison::Gt.test(&cell, &Value::Int(10)));
        assert!(Comparison::Le.test(&cell, &Value::Float(12.5)));
        assert!(Comparison::Eq.test(&Value::Int(3), &Value::Float(3.0)));
        assert!(!Comparison::Lt.test(&Value::Str("n/a".into()), &Value::Int(1)));
        assert!(!Comparison::Gt.test(&Value::Null, &Value::Int(-1)));
        assert!(Comparison::Ne.test(&Value::Str("n/a".into()), &Value::Int(1)));
        // Integers past 2^53 are told apart, as cells and as text.
        let big = 9_007_199_254_740_993_i64;
        assert!(!Comparison::Eq.test(&Value::Int(big), &Value::Int(big - 1)));
        assert!(Comparison::Gt.test(&Value::Int(big), &Value::Int(big - 1)));
        assert!(Comparison::Gt.test(&Value::Str(big.to_string()), &Value::Int(big - 1)));
        assert!(Comparison::Eq.test(&Value::Str(" 7 ".into()), &Value::Int(7)));
        assert!(Comparison::Lt.test(&Value::Float(6.5), &Value::Int(7)));
        assert!(Comparison::Gt.test(&Value::Str("7.5".into()), &Value::Int(7)));
    }

    #[test]
    fn comparisons_on_text_and_null() {
        let cell = Value::Str("failed: timeout".into());
        assert!(Comparison::StartsWith.test(&cell, &"failed".into()));
        assert!(Comparison::EndsWith.test(&cell, &"timeout".into()));
        assert!(Comparison::Contains.test(&cell, &": ".into()));
        assert!(!Comparison::Eq.test(&cell, &"failed".into()));
        assert!(Comparison::Lt.test(&Value::Str("a".into()), &"b".into()));
        assert!(Comparison::Eq.test(&Value::Null, &Value::Null));
        assert!(Comparison::Eq.test(&Value::Str(String::new()), &Value::Null));
        assert!(Comparison::Ne.test(&Value::Int(0), &Value::Null));
        assert!(Comparison::Empty.test(&Value::Null, &Value::Null));
        assert!(Comparison::NotEmpty.test(&Value::Int(0), &Value::Null));
        for name in ["==", "!=", "<", "<=", ">", ">=", "starts_with", "not_empty"] {
            assert!(Comparison::parse(name).is_some(), "{name}");
        }
        assert_eq!(Comparison::parse("like"), None);
    }

    #[test]
    fn resolution_targets_cells_rows_and_columns() {
        let rows = vec![
            vec![Value::from("a"), Value::Int(1)],
            vec![Value::from("b"), Value::Int(-1)],
        ];
        let rules = StyleRules::new()
            .rule(StyleRule::new(1, Comparison::Lt, 0, red()))
            .rule(StyleRule::new("name", Comparison::Eq, "a", red()).target(Target::Row))
            .rule(
                StyleRule::when(
                    "n",
                    |v| v.as_f64() == Some(-1.0),
                    Style::parse("bold").unwrap(),
                )
                .target(Target::Column),
            )
            .rule(StyleRule::new(
                "missing",
                Comparison::NotEmpty,
                Value::Null,
                red(),
            ));
        let resolved = rules.resolve(&["name", "n"], &rows);
        assert_eq!(resolved.cell_style(&rows[0], 0), None);
        // Column rule only.
        assert_eq!(
            resolved.cell_style(&rows[0], 1),
            Some(Style::parse("bold").unwrap())
        );
        // Column rule, then the cell rule on top.
        assert_eq!(
            resolved.cell_style(&rows[1], 1),
            Some(Style::parse("bold red").unwrap())
        );
        assert_eq!(resolved.row_style(&rows[0]), Some(red()));
        assert_eq!(resolved.row_style(&rows[1]), None);
        assert_eq!(resolved.unresolved().len(), 1);
        assert_eq!(resolved.unresolved()[0].column().to_string(), "missing");
    }

    /// The parts of a parsed spec: column, comparison, operand (as text, with
    /// its kind), target and style.
    fn parts(spec: &str) -> (String, &'static str, String, &'static str, String) {
        let rule = StyleRule::parse(spec).unwrap_or_else(|e| panic!("{e}"));
        let (op, value) = rule.comparison().unwrap();
        let operand = match value {
            Value::Null => "null".to_string(),
            Value::Int(n) => format!("int {n}"),
            Value::Float(f) => format!("float {f}"),
            other => format!("text {}", other.plain()),
        };
        (
            rule.column().to_string(),
            op.name(),
            operand,
            rule.target_kind().name(),
            rule.style().definition(),
        )
    }

    #[test]
    fn specs_map_onto_comparisons_and_targets() {
        let p = |spec| parts(spec);
        assert_eq!(
            p("latency>500:bold red"),
            (
                "latency".into(),
                "gt",
                "int 500".into(),
                "cell",
                "bold red".into()
            )
        );
        assert_eq!(
            p("status=failed:row:on red"),
            (
                "status".into(),
                "eq",
                "text failed".into(),
                "row",
                "on red".into()
            )
        );
        assert_eq!(
            p("  p99 ms >= 12.5 : column : italic "),
            (
                "p99 ms".into(),
                "ge",
                "float 12.5".into(),
                "column",
                "italic".into()
            )
        );
        assert_eq!(p("delta<-5:red").2, "int -5");
        assert_eq!(p("a<=1:red").1, "le");
        assert_eq!(p("a==1:red").1, "eq");
        assert_eq!(p("a!=1:red").1, "ne");
        assert_eq!(p("a*=err:red").1, "contains");
        assert_eq!(p("a^=err:red").1, "starts_with");
        assert_eq!(p("a$=err:red").1, "ends_with");
        assert_eq!(p("#2>0:red").0, "#2");
        // Words, after whitespace.
        assert_eq!(
            p("msg contains a=b:red"),
            (
                "msg".into(),
                "contains",
                "text a=b".into(),
                "cell",
                "red".into()
            )
        );
        assert_eq!(p("status eq failed:red").1, "eq");
        assert_eq!(p("note empty:dim").1, "empty");
        assert_eq!(p("note not_empty:row:dim").3, "row");
        // A word inside a name is part of the name.
        assert_eq!(p("is empty=yes:red").0, "is empty");
        // Quotes: a value with `:` or that looks like a number stays text,
        // and a column with a comparison in it.
        assert_eq!(p("when=\"12:30\":red").2, "text 12:30");
        assert_eq!(p("code=\"200\":red").2, "text 200");
        assert_eq!(p("\"a<b\">1:red").0, "a<b");
        assert_eq!(p("\"p99: ms\" gt 1:red").0, "p99: ms");
        assert_eq!(p("x=\"say \\\"hi\\\"\":red").2, "text say \"hi\"");
        // A style may hold `:`.
        assert_eq!(
            p("site=docs:link https://example.com").4,
            "link https://example.com"
        );
        // A rule from a spec behaves as one built in code.
        let rule = StyleRule::parse("latency>500:red").unwrap();
        assert!(rule.matches(&Value::from("612")));
        assert!(!rule.matches(&Value::Int(500)));
        assert!(StyleRule::parse("code=\"200\":red")
            .unwrap()
            .matches(&Value::from("200")));
    }

    #[test]
    fn bad_specs_say_what_is_wrong_and_name_the_spec() {
        let error = |spec: &str| StyleRule::parse(spec).unwrap_err().to_string();
        assert!(error("").contains("empty"));
        assert!(
            error("latency:red").contains("no comparison"),
            "{}",
            error("latency:red")
        );
        assert!(error(">5:red").contains("no column"));
        assert!(error("latency>:red").starts_with("rule \"latency>:red\": no value after `>`"));
        assert!(error("latency>5").contains("no `:` and style"));
        assert!(error("latency>5:").contains("no style"));
        assert!(error("latency>5:row:").contains("no style"));
        assert!(error("latency>5:row").contains("no style"));
        assert!(error("latency>5:nonsense colour").contains("style \"nonsense colour\""));
        assert!(error("note empty x:red").contains("`empty` takes no value"));
        assert!(error("x=\"open:red").contains("not closed"));
        assert!(error("x=\"a\\q\":red").contains("unknown escape"));
        assert!(error("\"x\" ? 1:red").contains("no comparison after the quoted column"));
    }

    #[test]
    fn specs_round_trip() {
        let red = || Style::parse("bold red").unwrap();
        let rules = [
            StyleRule::new("latency", Comparison::Gt, 500, red()),
            StyleRule::new("status", Comparison::Eq, "failed", red()).target(Target::Row),
            StyleRule::new("p99 ms", Comparison::Ge, 12.5, red()).target(Target::Column),
            StyleRule::new(2, Comparison::Lt, 1.0, red()),
            StyleRule::new("a<b", Comparison::Contains, "x: y", red()),
            StyleRule::new("count empty", Comparison::Eq, "200", red()),
            StyleRule::new("#tag", Comparison::Ne, " padded ", red()),
            StyleRule::new("q", Comparison::EndsWith, "say \"hi\"\n", red()),
            StyleRule::new("note", Comparison::Empty, Value::Null, red()),
            StyleRule::new("note", Comparison::Eq, Value::Null, red()),
            StyleRule::new("note", Comparison::NotEmpty, Value::Null, red()),
            StyleRule::new("", Comparison::StartsWith, "", red()),
        ];
        for rule in rules {
            let spec = rule.to_spec().unwrap();
            let back = StyleRule::parse(&spec).unwrap_or_else(|e| panic!("{spec}: {e}"));
            assert_eq!(back.to_spec().as_deref(), Some(spec.as_str()));
            assert_eq!(back.column(), rule.column(), "{spec}");
            assert_eq!(back.target_kind(), rule.target_kind(), "{spec}");
            assert_eq!(back.style(), rule.style(), "{spec}");
            for cell in [
                "",
                "200",
                "failed",
                "x: y",
                " padded ",
                "13",
                "0.5",
                "say \"hi\"\n",
            ] {
                let cell = Value::from(cell);
                assert_eq!(
                    back.matches(&cell),
                    rule.matches(&cell),
                    "{spec} on {cell:?}"
                );
            }
        }
        assert_eq!(
            StyleRule::new("p99 ms", Comparison::Ge, 12.5, red())
                .to_spec()
                .as_deref(),
            Some("p99 ms>=12.5:bold red")
        );
        assert_eq!(
            StyleRule::new(1, Comparison::Eq, 2.0, red())
                .to_spec()
                .as_deref(),
            Some("#1==2.0:bold red")
        );
        // What a spec cannot write.
        assert_eq!(StyleRule::when("a", |_| true, red()).to_spec(), None);
        assert_eq!(
            StyleRule::when("a", |_| true, red()).to_string(),
            "a <predicate>:bold red"
        );
        assert_eq!(
            StyleRule::new("a", Comparison::Lt, Value::Null, red()).to_spec(),
            None
        );
        assert_eq!(
            StyleRule::new("a", Comparison::Lt, f64::NAN, red()).to_spec(),
            None
        );
    }

    #[cfg(feature = "toml")]
    #[test]
    fn rule_tables_from_toml() {
        let rules = StyleRules::from_toml(
            "[[rules]]\ncolumn = 'status'\nvalue = 'failed'\nstyle = 'bold red'\ntarget = 'row'\n\
             [[rules]]\ncolumn = 2\nop = '>='\nvalue = 0.5\nstyle = 'yellow'\n\
             [[rules]]\ncolumn = 'note'\nop = 'empty'\nstyle = 'dim'\n",
        )
        .unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules.rules()[0].target_kind(), Target::Row);
        assert_eq!(rules.rules()[1].column(), &ColumnRef::Index(2));
        assert!(rules.rules()[1].matches(&Value::Float(0.75)));
        assert!(rules.rules()[2].matches(&Value::Null));
        assert!(StyleRules::from_toml("").unwrap().is_empty());

        let error = |text: &str| StyleRules::from_toml(text).unwrap_err().to_string();
        assert_eq!(
            error("[[rules]]\ncolumn='a'\nvalue=1\nstyle='red'\nwhen='x'"),
            "rules[0]: unknown key `when`"
        );
        assert_eq!(
            error("[[rules]]\ncolumn='a'\nop='like'\nvalue=1\nstyle='red'"),
            "rules[0]: unknown op `like`"
        );
        assert_eq!(
            error("[[rules]]\ncolumn='a'\nstyle='red'"),
            "rules[0]: missing `value`"
        );
        assert_eq!(error("rules = 3"), "rules: expected an array of tables");
        assert!(
            error("[[rules]]\ncolumn='a'\nvalue=1\nstyle='nonsense colour'")
                .starts_with("rules[0]: style `nonsense colour`")
        );
    }
}
