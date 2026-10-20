//! `rs_rich.ext.sql`: the SQL highlighter (`rich_ext::sql`), a pure-Rust
//! lexer with dialect keywords and Jinja blocks (for dbt), no syntect.
//!
//! `SqlHighlighter` is a code highlighter for `Syntax(highlighter=...)`,
//! `Markdown(highlighter=...)` and the plugin host; the name `"sql"` picks it
//! too. `sql_highlight` styles code into a `Text` without a `Syntax`, and
//! `sql_tokenize` lists the tokens.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use rich::protocol::CodeHighlighter as CoreCodeHighlighter;
use rich_ext::sql::{self, SqlHighlighter as CoreSqlHighlighter, TokenKind};

use super::common::names;
use crate::art::repr_str;
use crate::code::code_highlighter_value;
use crate::plugins::CodeHighlighter;
use crate::style::Style;
use crate::text::Text;

names!(token_kind, token_kind_name, TokenKind, "token kind", {
    "whitespace" => TokenKind::Whitespace,
    "keyword" => TokenKind::Keyword,
    "type" => TokenKind::Type,
    "constant" => TokenKind::Constant,
    "function" => TokenKind::Function,
    "name" => TokenKind::Name,
    "quoted_name" => TokenKind::QuotedName,
    "string" => TokenKind::String,
    "number" => TokenKind::Number,
    "comment" => TokenKind::Comment,
    "parameter" => TokenKind::Parameter,
    "operator" => TokenKind::Operator,
    "punctuation" => TokenKind::Punctuation,
    "template" => TokenKind::Template,
    "other" => TokenKind::Other,
});

/// Every token kind's name, in `TokenKind`'s order.
const KINDS: [&str; 15] = [
    "whitespace",
    "keyword",
    "type",
    "constant",
    "function",
    "name",
    "quoted_name",
    "string",
    "number",
    "comment",
    "parameter",
    "operator",
    "punctuation",
    "template",
    "other",
];

/// A token of SQL source (`rich_ext::sql::Token`): its kind, and where it is
/// as `str` indices (`code[start:end] == text`).
#[pyclass(name = "SqlToken", module = "rs_rich.ext.sql", frozen)]
pub(crate) struct SqlToken {
    #[pyo3(get)]
    kind: &'static str,
    #[pyo3(get)]
    start: usize,
    #[pyo3(get)]
    end: usize,
    #[pyo3(get)]
    text: String,
}

#[pymethods]
impl SqlToken {
    fn __repr__(&self) -> String {
        format!(
            "SqlToken(kind={}, start={}, end={}, text={})",
            repr_str(self.kind),
            self.start,
            self.end,
            repr_str(&self.text)
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<PyRef<'_, SqlToken>>().is_ok_and(|other| {
            (other.kind, other.start, other.end, &other.text)
                == (self.kind, self.start, self.end, &self.text)
        })
    }
}

/// Split SQL source into tokens (`rich_ext::sql::tokenize`). The tokens
/// cover the source end to end, whitespace included.
#[pyfunction]
fn sql_tokenize(code: &str) -> Vec<SqlToken> {
    // Byte offsets to `str` (code point) indices.
    let mut index = vec![0usize; code.len() + 1];
    let mut chars = 0;
    for (at, ch) in code.char_indices() {
        index[at] = chars;
        chars += 1;
        for slot in &mut index[at + 1..at + ch.len_utf8()] {
            *slot = chars;
        }
    }
    index[code.len()] = chars;
    sql::tokenize(code)
        .into_iter()
        .map(|token| SqlToken {
            kind: token_kind_name(token.kind),
            start: index[token.range.start],
            end: index[token.range.end],
            text: code[token.range].to_string(),
        })
        .collect()
}

/// `code` highlighted as SQL with `theme` (`"monokai"`, `"ansi_dark"` or
/// `"ansi_light"`; any other name is `monokai`), as a `Text`
/// (`rich_ext::sql::highlight`).
#[pyfunction]
#[pyo3(signature = (code, theme=sql::DEFAULT_THEME))]
fn sql_highlight(code: &str, theme: &str) -> Text {
    Text::from_core(sql::highlight(code, theme))
}

/// The style of a token `kind` in `theme`, or `None` for text left in the
/// theme's default style (`rich_ext::sql::token_style`).
#[pyfunction]
#[pyo3(signature = (kind, theme=sql::DEFAULT_THEME))]
fn sql_token_style(kind: &str, theme: &str) -> PyResult<Option<Style>> {
    Ok(sql::token_style(token_kind(kind)?, theme).map(Style::from_core))
}

/// Whether the SQL highlighter answers to `language` (`sql`, `postgresql`,
/// `duckdb`, ..., any case).
#[pyfunction]
fn is_sql(language: &str) -> bool {
    sql::is_sql(language)
}

/// The SQL highlighter as a code highlighter
/// (`rich_ext::sql::SqlHighlighter`). With `fallback` (a highlighter name
/// such as `"syntect"`, or a code highlighter), other languages are
/// highlighted by it; without, they are plain.
#[pyclass(
    name = "SqlHighlighter",
    module = "rs_rich.ext.sql",
    extends = CodeHighlighter,
    frozen
)]
pub(crate) struct SqlHighlighter;

#[pymethods]
impl SqlHighlighter {
    #[new]
    #[pyo3(signature = (fallback=None))]
    fn new(fallback: Option<&Bound<'_, PyAny>>) -> PyResult<PyClassInitializer<Self>> {
        let inner: Arc<dyn CoreCodeHighlighter> = match code_highlighter_value(fallback)? {
            Some(fallback) => Arc::new(CoreSqlHighlighter::with_fallback(fallback)),
            None if fallback.is_some_and(|f| !f.is_none()) => {
                return Err(PyValueError::new_err("the fallback highlighter is unknown"))
            }
            None => CoreSqlHighlighter::shared(),
        };
        Ok(PyClassInitializer::from(CodeHighlighter::wrap(inner)).add_subclass(SqlHighlighter))
    }

    fn __repr__(&self) -> &'static str {
        "SqlHighlighter()"
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SqlToken>()?;
    m.add_class::<SqlHighlighter>()?;
    m.add_function(pyo3::wrap_pyfunction!(sql_tokenize, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(sql_highlight, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(sql_token_style, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(is_sql, m)?)?;
    m.add("SQL_LANGUAGES", sql::LANGUAGES.to_vec())?;
    m.add("SQL_THEMES", sql::THEMES.to_vec())?;
    m.add("SQL_DEFAULT_THEME", sql::DEFAULT_THEME)?;
    m.add("SQL_TOKEN_KINDS", KINDS.to_vec())?;
    Ok(())
}
