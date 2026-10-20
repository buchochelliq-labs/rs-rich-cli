//! A small SQL highlighter in pure Rust: no syntect, no C, no dependencies.
//!
//! [`SqlHighlighter`] is a [`CodeHighlighter`] built on a hand-written lexer
//! ([`tokenize`]). It knows standard SQL and the common words of PostgreSQL,
//! MySQL, SQLite, BigQuery, Snowflake and DuckDB, and treats Jinja blocks
//! (`{{ … }}`, `{% … %}`, `{# … #}`), as dbt models use them, as a token class
//! of their own. It answers to the language `sql` and the aliases in
//! [`LANGUAGES`]; any other language is plain text, or goes to a fallback
//! highlighter ([`SqlHighlighter::with_fallback`]).
//!
//! Turn on the `sql` feature. It needs neither `syntax` nor `markdown`:
//! [`highlight`] gives a styled [`Text`] in a lean build, and with `syntax`
//! the highlighter serves `Syntax` and Markdown's ```` ```sql ```` fences
//! through the console's code highlighting.
//!
//! ```
//! use rich::protocol::{CodeHighlighting, ConsoleCodeHighlighting};
//! use rich::{ColorSystem, Console};
//! use rich_ext::sql::{self, SqlHighlighter};
//!
//! // A styled `Text`, no `Syntax` needed.
//! let text = sql::highlight("SELECT 1 -- one", "ansi_dark");
//! assert_eq!(text.plain(), "SELECT 1 -- one");
//!
//! // Or every SQL code block a console renders.
//! let mut console = Console::builder()
//!     .force_terminal(true)
//!     .color_system(Some(ColorSystem::Standard))
//!     .build();
//! console.set_code_highlighting(Some(CodeHighlighting {
//!     highlighter: SqlHighlighter::shared(),
//!     theme: Some("ansi_dark".into()),
//! }));
//! # #[cfg(feature = "syntax")] {
//! let out = console.render_to_string(&rich::Syntax::new("SELECT 1", "sql"));
//! assert!(out.contains("\x1b[94mSELECT"), "{out:?}");
//! # }
//! ```
//!
//! # Themes
//!
//! `monokai` (the default, upstream's default code theme), and `ansi_dark`
//! and `ansi_light`, upstream's themes in the terminal's own 16 colours. A
//! theme this highlighter does not know highlights SQL with `monokai`, so a
//! console set up for another engine's theme still colours SQL.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use rich::color::Color;
use rich::protocol::{
    CodeHighlighter, HighlightError, HighlightSpan, HighlightedCode, HighlightedLine,
};
use rich::style::Style;
use rich::Text;
use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar};

/// The language names this highlighter answers to.
pub const LANGUAGES: &[&str] = &[
    "sql",
    "postgresql",
    "postgres",
    "psql",
    "plpgsql",
    "mysql",
    "mariadb",
    "sqlite",
    "sqlite3",
    "bigquery",
    "snowflake",
    "duckdb",
    "tsql",
    "plsql",
];

/// The themes this highlighter has.
pub const THEMES: &[&str] = &["ansi_dark", "ansi_light", "monokai"];

/// The theme used when none is chosen.
pub const DEFAULT_THEME: &str = "monokai";

/// What a token is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// Spaces, tabs and line breaks.
    Whitespace,
    /// A reserved or dialect keyword (`SELECT`, `QUALIFY`, `PRAGMA`).
    Keyword,
    /// A data type (`VARCHAR`, `INT64`, `TIMESTAMPTZ`).
    Type,
    /// `TRUE`, `FALSE`, `NULL`.
    Constant,
    /// A name followed by `(`: a function call.
    Function,
    /// A name: a table, column, alias or schema.
    Name,
    /// A quoted name: `"col"`, `` `col` ``, `[col]`.
    QuotedName,
    /// A string literal: `'it''s'`, `E'\n'`, `$$body$$`, `'''raw'''`.
    String,
    /// A number: `42`, `1.5e-3`, `.5`, `0xFF`.
    Number,
    /// A comment: `-- …` or `/* … */` (nested).
    Comment,
    /// A bind parameter: `:name`, `@name`, `@@var`, `$1`, `?`.
    Parameter,
    /// An operator: `=`, `<>`, `||`, `::`, `->>`.
    Operator,
    /// Punctuation: `( ) , ; . [ ] { }`.
    Punctuation,
    /// A Jinja block: `{{ ref('x') }}`, `{% if … %}`, `{# … #}`.
    Template,
    /// Anything else (a stray character).
    Other,
}

/// A token: its kind and its byte range in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
}

const KEYWORDS: &[&str] = &[
    "abort",
    "absolute",
    "action",
    "add",
    "after",
    "all",
    "alter",
    "always",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asof",
    "attach",
    "authorization",
    "autoincrement",
    "auto_increment",
    "before",
    "begin",
    "between",
    "both",
    "by",
    "cascade",
    "case",
    "cast",
    "chain",
    "check",
    "cluster",
    "collate",
    "column",
    "columns",
    "comment",
    "commit",
    "concurrently",
    "conflict",
    "connect",
    "constraint",
    "copy",
    "create",
    "cross",
    "cube",
    "current",
    "current_date",
    "current_time",
    "current_timestamp",
    "current_user",
    "cursor",
    "database",
    "deallocate",
    "declare",
    "default",
    "deferrable",
    "deferred",
    "delete",
    "desc",
    "describe",
    "detach",
    "distinct",
    "do",
    "drop",
    "each",
    "else",
    "elseif",
    "elsif",
    "end",
    "engine",
    "escape",
    "except",
    "exclude",
    "exclusive",
    "execute",
    "exists",
    "explain",
    "extension",
    "fetch",
    "filter",
    "first",
    "following",
    "for",
    "foreign",
    "format",
    "from",
    "full",
    "function",
    "glob",
    "grant",
    "group",
    "grouping",
    "having",
    "if",
    "ignore",
    "ilike",
    "immediate",
    "in",
    "include",
    "index",
    "inherits",
    "initially",
    "inner",
    "insert",
    "instead",
    "intersect",
    "interval",
    "into",
    "is",
    "isnull",
    "join",
    "key",
    "language",
    "last",
    "lateral",
    "leading",
    "left",
    "like",
    "limit",
    "load",
    "local",
    "lock",
    "loop",
    "match",
    "matched",
    "materialized",
    "merge",
    "minus",
    "natural",
    "next",
    "no",
    "not",
    "nothing",
    "notnull",
    "nulls",
    "of",
    "offset",
    "on",
    "only",
    "option",
    "options",
    "or",
    "order",
    "ordinality",
    "outer",
    "over",
    "overwrite",
    "owner",
    "partition",
    "pivot",
    "pragma",
    "preceding",
    "primary",
    "procedure",
    "qualify",
    "range",
    "recursive",
    "references",
    "regexp",
    "reindex",
    "release",
    "rename",
    "repeat",
    "replace",
    "restrict",
    "return",
    "returning",
    "returns",
    "revoke",
    "right",
    "rlike",
    "rollback",
    "rollup",
    "row",
    "rows",
    "sample",
    "savepoint",
    "schema",
    "select",
    "semi",
    "sequence",
    "set",
    "sets",
    "show",
    "similar",
    "some",
    "stage",
    "start",
    "stream",
    "struct",
    "summarize",
    "table",
    "tablesample",
    "temp",
    "temporary",
    "then",
    "ties",
    "to",
    "top",
    "trailing",
    "transaction",
    "trigger",
    "truncate",
    "unbounded",
    "union",
    "unique",
    "unnest",
    "unpivot",
    "until",
    "update",
    "upsert",
    "use",
    "user",
    "using",
    "vacuum",
    "values",
    "view",
    "virtual",
    "volatile",
    "warehouse",
    "when",
    "where",
    "while",
    "window",
    "with",
    "within",
    "without",
    "work",
];

const TYPES: &[&str] = &[
    "bigint",
    "bigserial",
    "binary",
    "bit",
    "blob",
    "bool",
    "boolean",
    "bpchar",
    "bytea",
    "bytes",
    "char",
    "character",
    "cidr",
    "clob",
    "date",
    "datetime",
    "datetime2",
    "dec",
    "decimal",
    "double",
    "enum",
    "float",
    "float4",
    "float8",
    "float64",
    "geography",
    "geometry",
    "hugeint",
    "inet",
    "int",
    "int2",
    "int4",
    "int8",
    "int64",
    "integer",
    "json",
    "jsonb",
    "list",
    "long",
    "map",
    "mediumint",
    "money",
    "nchar",
    "number",
    "numeric",
    "nvarchar",
    "object",
    "precision",
    "real",
    "serial",
    "smallint",
    "smallserial",
    "string",
    "text",
    "time",
    "timestamp",
    "timestamp_ltz",
    "timestamp_ntz",
    "timestamp_tz",
    "timestamptz",
    "timetz",
    "tinyint",
    "ubigint",
    "uinteger",
    "usmallint",
    "utinyint",
    "uuid",
    "varbinary",
    "varchar",
    "variant",
    "xml",
];

const CONSTANTS: &[&str] = &["false", "null", "true", "unknown"];

/// The kind a word list gives, by lower-case word.
fn word_kinds() -> &'static HashMap<&'static str, TokenKind> {
    static KINDS: OnceLock<HashMap<&'static str, TokenKind>> = OnceLock::new();
    KINDS.get_or_init(|| {
        let mut kinds = HashMap::new();
        for (list, kind) in [
            (KEYWORDS, TokenKind::Keyword),
            (TYPES, TokenKind::Type),
            (CONSTANTS, TokenKind::Constant),
        ] {
            for word in list {
                kinds.insert(*word, kind);
            }
        }
        kinds
    })
}

fn word_kind(word: &str) -> Option<TokenKind> {
    if word.len() > 24 {
        return None;
    }
    word_kinds()
        .get(word.to_ascii_lowercase().as_str())
        .copied()
}

fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn is_ident_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

const OPERATOR_CHARS: &str = "+-*/%=<>!~^&|#";

/// Split `code` into tokens covering every byte, in order.
///
/// Never fails: an unterminated string, comment, quoted name or Jinja block
/// runs to the end of the input.
///
/// ```
/// use rich_ext::sql::{tokenize, TokenKind};
///
/// let code = "select \"a b\" from t -- note";
/// let kinds: Vec<(TokenKind, &str)> = tokenize(code)
///     .into_iter()
///     .filter(|t| t.kind != TokenKind::Whitespace)
///     .map(|t| (t.kind, &code[t.range]))
///     .collect();
/// assert_eq!(kinds, [
///     (TokenKind::Keyword, "select"),
///     (TokenKind::QuotedName, "\"a b\""),
///     (TokenKind::Keyword, "from"),
///     (TokenKind::Name, "t"),
///     (TokenKind::Comment, "-- note"),
/// ]);
/// ```
pub fn tokenize(code: &str) -> Vec<Token> {
    Lexer {
        code,
        at: 0,
        tokens: Vec::new(),
    }
    .run()
}

struct Lexer<'a> {
    code: &'a str,
    at: usize,
    tokens: Vec<Token>,
}

impl Lexer<'_> {
    fn rest(&self) -> &str {
        &self.code[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    fn push(&mut self, kind: TokenKind, end: usize) {
        let end = end.clamp(self.at, self.code.len());
        if end > self.at {
            self.tokens.push(Token {
                kind,
                range: self.at..end,
            });
        }
        self.at = end;
    }

    /// The end of the first `close` at or after byte `from`, or the end of input.
    fn until(&self, from: usize, close: &str) -> usize {
        self.code
            .get(from..)
            .and_then(|rest| rest.find(close))
            .map_or(self.code.len(), |found| from + found + close.len())
    }

    /// The last token that is not whitespace or a comment.
    fn previous(&self) -> Option<&Token> {
        self.tokens
            .iter()
            .rev()
            .find(|t| !matches!(t.kind, TokenKind::Whitespace | TokenKind::Comment))
    }

    fn run(mut self) -> Vec<Token> {
        while let Some(ch) = self.peek() {
            let start = self.at;
            match ch {
                c if c.is_whitespace() => {
                    let len = self
                        .rest()
                        .find(|c: char| !c.is_whitespace())
                        .unwrap_or(self.rest().len());
                    self.push(TokenKind::Whitespace, start + len);
                }
                '-' if self.peek_at(1) == Some('-') => {
                    let end = self
                        .rest()
                        .find('\n')
                        .map_or(self.code.len(), |n| start + n);
                    self.push(TokenKind::Comment, end);
                }
                '/' if self.peek_at(1) == Some('*') => {
                    let end = self.block_comment();
                    self.push(TokenKind::Comment, end);
                }
                '{' if matches!(self.peek_at(1), Some('{' | '%' | '#')) => {
                    let close = match self.peek_at(1) {
                        Some('{') => "}}",
                        Some('%') => "%}",
                        _ => "#}",
                    };
                    let end = self.until(start + 2, close);
                    self.push(TokenKind::Template, end);
                }
                '\'' => {
                    let end = self.quoted_string(start, '\'', false);
                    self.push(TokenKind::String, end);
                }
                '"' => {
                    let body = start + 3;
                    if self.rest().starts_with("\"\"\"")
                        && !self.code[body..].starts_with('"')
                        && self.code[body..].contains("\"\"\"")
                    {
                        let end = self.until(body, "\"\"\"");
                        self.push(TokenKind::String, end);
                    } else {
                        let end = self.doubled(start, '"');
                        self.push(TokenKind::QuotedName, end);
                    }
                }
                '`' => {
                    let end = self.doubled(start, '`');
                    self.push(TokenKind::QuotedName, end);
                }
                '[' if self.bracket_is_name() => {
                    let end = self.doubled(start, ']');
                    self.push(TokenKind::QuotedName, end);
                }
                '$' => self.dollar(start),
                '0'..='9' => {
                    let end = self.number(start);
                    self.push(TokenKind::Number, end);
                }
                '.' if self.peek_at(1).is_some_and(|c| c.is_ascii_digit())
                    && !self.previous().is_some_and(|t| {
                        matches!(t.kind, TokenKind::Name | TokenKind::QuotedName)
                            && t.range.end == start
                    }) =>
                {
                    let end = self.number(start);
                    self.push(TokenKind::Number, end);
                }
                ':' if self.peek_at(1) == Some(':') => self.push(TokenKind::Operator, start + 2),
                ':' if self.peek_at(1) == Some('=') => self.push(TokenKind::Operator, start + 2),
                ':' if self.peek_at(1).is_some_and(is_ident_start) => {
                    let end = self.word_end(start + 1);
                    self.push(TokenKind::Parameter, end);
                }
                '@' if self
                    .peek_at(1)
                    .is_some_and(|c| is_ident_start(c) || c == '@') =>
                {
                    let skip = if self.peek_at(1) == Some('@') { 2 } else { 1 };
                    let end = self.word_end(start + skip);
                    self.push(TokenKind::Parameter, end);
                }
                '?' => self.push(TokenKind::Parameter, start + 1),
                c if is_ident_start(c) => self.word(start),
                c if OPERATOR_CHARS.contains(c) || c == '@' => {
                    let end = self.operator(start);
                    self.push(TokenKind::Operator, end);
                }
                '(' | ')' | ',' | ';' | '.' | '[' | ']' | '{' | '}' | ':' => {
                    self.push(TokenKind::Punctuation, start + 1)
                }
                c => self.push(TokenKind::Other, start + c.len_utf8()),
            }
        }
        self.tokens
    }

    /// `/* … */`, counting nested comments; the end of input if unclosed.
    fn block_comment(&self) -> usize {
        let bytes = self.code.as_bytes();
        let mut depth = 0usize;
        let mut i = self.at;
        while i + 1 < bytes.len() {
            match (bytes[i], bytes[i + 1]) {
                (b'/', b'*') => {
                    depth += 1;
                    i += 2;
                }
                (b'*', b'/') => {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        return i;
                    }
                }
                _ => i += 1,
            }
        }
        self.code.len()
    }

    /// A string opened by `quote` at `start` (or by a prefix before it), with
    /// the quote doubled to escape it and, for `backslash`, `\` escapes too.
    fn quoted_string(&self, start: usize, quote: char, backslash: bool) -> usize {
        let open = self.code[start..].find(quote).map_or(start, |n| start + n);
        // BigQuery's triple quotes, when three quotes are not a doubled quote
        // inside a string (`''''`) and the string is closed.
        let triple: String = std::iter::repeat_n(quote, 3).collect();
        let body = open + 3;
        if self.code[open..].starts_with(&triple)
            && !self.code[body..].starts_with(quote)
            && self.code[body..].contains(triple.as_str())
        {
            return self.until(body, &triple);
        }
        let mut chars = self.code[open + 1..].char_indices().peekable();
        while let Some((i, ch)) = chars.next() {
            if backslash && ch == '\\' {
                chars.next();
            } else if ch == quote {
                if chars.peek().is_some_and(|&(_, next)| next == quote) {
                    chars.next();
                } else {
                    return open + 1 + i + ch.len_utf8();
                }
            }
        }
        self.code.len()
    }

    /// A name quoted from `start` to `close`, which is escaped by doubling it.
    fn doubled(&self, start: usize, close: char) -> usize {
        let mut chars = self.code[start + 1..].char_indices().peekable();
        while let Some((i, ch)) = chars.next() {
            if ch == close {
                if chars.peek().is_some_and(|&(_, next)| next == close) {
                    chars.next();
                } else {
                    return start + 1 + i + ch.len_utf8();
                }
            }
        }
        self.code.len()
    }

    /// `[` opens a quoted name (SQL Server) where an expression could start,
    /// and is a subscript after a name, a closing bracket or `ARRAY`.
    fn bracket_is_name(&self) -> bool {
        let Some(previous) = self.previous() else {
            return true;
        };
        let text = &self.code[previous.range.clone()];
        let attached = previous.range.end == self.at;
        match previous.kind {
            TokenKind::Name | TokenKind::QuotedName | TokenKind::Function => !attached,
            TokenKind::Punctuation => !matches!(text, ")" | "]"),
            TokenKind::Keyword => !text.eq_ignore_ascii_case("array"),
            TokenKind::Number | TokenKind::String | TokenKind::Parameter => false,
            _ => true,
        }
    }

    /// `$1`, `$name`, or a dollar-quoted string `$$…$$` / `$tag$…$tag$`.
    fn dollar(&mut self, start: usize) {
        let after = &self.code[start + 1..];
        if after.starts_with(|c: char| c.is_ascii_digit()) {
            let len = after
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(after.len());
            self.push(TokenKind::Parameter, start + 1 + len);
            return;
        }
        let tag_len = after
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if after[tag_len..].starts_with('$') {
            let delimiter = &self.code[start..start + tag_len + 2];
            let end = self.until(start + delimiter.len(), delimiter);
            self.push(TokenKind::String, end);
        } else if tag_len > 0 {
            self.push(TokenKind::Parameter, start + 1 + tag_len);
        } else {
            self.push(TokenKind::Operator, start + 1);
        }
    }

    fn number(&self, start: usize) -> usize {
        let bytes = self.code.as_bytes();
        let mut i = start;
        if bytes[i] == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X')) {
            i += 2;
            while i < bytes.len() && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
                i += 1;
            }
            return i;
        }
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'.' && bytes.get(i + 1) != Some(&b'.') {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
        }
        if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
            let mut j = i + 1;
            if j < bytes.len() && matches!(bytes[j], b'+' | b'-') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_digit() {
                i = j;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        i
    }

    fn word_end(&self, from: usize) -> usize {
        self.code[from..]
            .find(|c: char| !is_ident_char(c))
            .map_or(self.code.len(), |n| from + n)
    }

    fn word(&mut self, start: usize) {
        let end = self.word_end(start);
        let word = &self.code[start..end];
        // A one-letter prefix opens a string: E'…' (backslash escapes),
        // N'…', B'…', X'…', R'…' (raw), and BigQuery's two-letter ones.
        if self.code[end..].starts_with(['\'', '"']) && word.len() <= 2 {
            let lower = word.to_ascii_lowercase();
            let quote = self.code[end..].chars().next().unwrap_or('\'');
            let string_prefix = matches!(lower.as_str(), "e" | "n" | "b" | "x" | "r" | "rb" | "br");
            if string_prefix && (quote == '\'' || matches!(lower.as_str(), "r" | "b" | "rb" | "br"))
            {
                let backslash = lower == "e" || (lower.contains('b') && !lower.contains('r'));
                let close = self.quoted_string(start, quote, backslash);
                self.push(TokenKind::String, close);
                return;
            }
        }
        let after_dot = self.previous().is_some_and(|t| {
            t.kind == TokenKind::Punctuation
                && &self.code[t.range.clone()] == "."
                && t.range.end == start
        });
        let call = self.code[end..]
            .trim_start_matches([' ', '\t'])
            .starts_with('(');
        let kind = if after_dot {
            if call {
                TokenKind::Function
            } else {
                TokenKind::Name
            }
        } else if let Some(kind) = word_kind(word) {
            kind
        } else if call {
            TokenKind::Function
        } else {
            TokenKind::Name
        };
        self.push(kind, end);
    }

    /// A run of operator characters, stopping before a comment.
    fn operator(&self, start: usize) -> usize {
        let mut end = start;
        for (i, ch) in self.code[start..].char_indices() {
            let here = &self.code[start + i..];
            if !(OPERATOR_CHARS.contains(ch) || (ch == '@' && i == 0))
                || (i > 0 && (here.starts_with("--") || here.starts_with("/*")))
            {
                break;
            }
            end = start + i + ch.len_utf8();
        }
        end
    }
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

/// The style of `kind` in `theme`; `None` for text left in the default style.
pub fn token_style(kind: TokenKind, theme: &str) -> Option<Style> {
    use TokenKind::*;
    let markup = match theme {
        "ansi_dark" | "ansi_light" => {
            let dark = theme == "ansi_dark";
            match (kind, dark) {
                (Keyword | Constant, true) | (Number, true) => "bright_blue",
                (Keyword | Constant, false) | (Number, false) => "blue",
                (Type, true) | (Template, true) => "bright_cyan",
                (Type, false) | (Template, false) => "cyan",
                (Function, true) => "bright_green",
                (Function, false) => "green",
                (Parameter, true) => "bright_red",
                (Parameter, false) => "red",
                (String, _) => "yellow",
                (Comment, _) => "dim",
                (Other, _) => "red underline",
                _ => return None,
            }
        }
        _ => match kind {
            Keyword | Type | Constant => "#66d9ef",
            Function => "#a6e22e",
            String => "#e6db74",
            Number => "#ae81ff",
            Comment => "#959077",
            Operator => "#ff4689",
            Parameter => "#fd971f",
            Template => "italic #fd971f",
            Other => "#ed007e on #1e0010",
            _ => return None,
        },
    };
    Style::parse(markup).ok()
}

/// The text and background colour of `theme`, if it has them.
fn theme_base(theme: &str) -> (Style, Option<Color>) {
    match theme {
        "ansi_dark" | "ansi_light" => (Style::new(), None),
        _ => {
            let background = Color::parse("#272822").ok();
            let mut base = Style::parse("#f8f8f2").unwrap_or_default();
            if let Some(background) = &background {
                base = base.with_bgcolor(background.clone());
            }
            (base, background)
        }
    }
}

/// Highlight `code` as SQL with `theme` (one of [`THEMES`]; anything else is
/// `monokai`), as a styled [`Text`]. Needs no `Syntax`, so it works in a
/// build without syntect.
///
/// ```
/// let text = rich_ext::sql::highlight("select 'x'", "ansi_light");
/// assert_eq!(text.spans().len(), 2);
/// ```
pub fn highlight(code: &str, theme: &str) -> Text {
    let theme = known_theme(theme);
    let (base, _) = theme_base(theme);
    let mut text = Text::new(code);
    if !base.is_null() {
        text.set_base_style(base.clone());
    }
    for token in tokenize(code) {
        if let Some(style) = token_style(token.kind, theme) {
            text.stylize(base.combine(&style), token.range.start, token.range.end);
        }
    }
    text
}

fn known_theme(theme: &str) -> &str {
    if THEMES.contains(&theme) {
        theme
    } else {
        DEFAULT_THEME
    }
}

/// Whether `language` (a name or a file extension) is SQL to this highlighter.
pub fn is_sql(language: &str) -> bool {
    LANGUAGES
        .iter()
        .any(|name| name.eq_ignore_ascii_case(language))
}

// ---------------------------------------------------------------------------
// The CodeHighlighter
// ---------------------------------------------------------------------------

/// SQL highlighting as a [`CodeHighlighter`], for `Syntax`, Markdown code
/// fences and the console's code highlighting.
#[derive(Clone, Default)]
pub struct SqlHighlighter {
    fallback: Option<Arc<dyn CodeHighlighter>>,
}

impl std::fmt::Debug for SqlHighlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlHighlighter")
            .field("fallback", &self.fallback.is_some())
            .finish()
    }
}

impl SqlHighlighter {
    /// SQL only: other languages are plain text.
    pub fn new() -> Self {
        Self::default()
    }

    /// SQL here, every other language through `fallback` (syntect, lumis or
    /// another engine).
    ///
    /// ```
    /// use std::sync::Arc;
    /// use rich::protocol::CodeHighlighter;
    /// use rich_ext::sql::SqlHighlighter;
    ///
    /// let both = SqlHighlighter::with_fallback(Arc::new(SqlHighlighter::new()));
    /// assert!(both.languages().contains(&"duckdb".to_string()));
    /// ```
    pub fn with_fallback(fallback: Arc<dyn CodeHighlighter>) -> Self {
        SqlHighlighter {
            fallback: Some(fallback),
        }
    }

    /// One shared instance with no fallback.
    pub fn shared() -> Arc<dyn CodeHighlighter> {
        static SHARED: OnceLock<Arc<SqlHighlighter>> = OnceLock::new();
        SHARED
            .get_or_init(|| Arc::new(SqlHighlighter::new()))
            .clone()
    }
}

impl CodeHighlighter for SqlHighlighter {
    fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Result<HighlightedCode, HighlightError> {
        if !language.is_some_and(is_sql) {
            return match &self.fallback {
                Some(fallback) => fallback.highlight(code, language, theme),
                None => {
                    let (default_style, background) = theme_base(known_theme(theme));
                    Ok(HighlightedCode {
                        lines: vec![HighlightedLine::default(); code.split('\n').count()],
                        background,
                        default_style,
                    })
                }
            };
        }
        let theme = known_theme(theme);
        let (default_style, background) = theme_base(theme);
        let mut lines = Lines::new(code);
        for token in tokenize(code) {
            if let Some(style) = token_style(token.kind, theme) {
                lines.push(token.range, &default_style.combine(&style));
            }
        }
        Ok(HighlightedCode {
            lines: lines.lines,
            background,
            default_style,
        })
    }

    fn default_theme(&self) -> &str {
        DEFAULT_THEME
    }

    fn themes(&self) -> Vec<String> {
        let mut names: Vec<String> = THEMES.iter().map(|name| name.to_string()).collect();
        if let Some(fallback) = &self.fallback {
            names.extend(fallback.themes());
            names.sort();
            names.dedup();
        }
        names
    }

    fn languages(&self) -> Vec<String> {
        let mut names: Vec<String> = LANGUAGES.iter().map(|name| name.to_string()).collect();
        if let Some(fallback) = &self.fallback {
            names.extend(fallback.languages());
            names.sort();
            names.dedup();
        }
        names
    }

    fn language_for_path(&self, path: &Path) -> Option<String> {
        let sql = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("sql"));
        if sql {
            return Some("sql".to_string());
        }
        self.fallback.as_ref()?.language_for_path(path)
    }

    fn token_style(&self, theme: &str, token: &str) -> Option<Style> {
        if !THEMES.contains(&theme) {
            if let Some(fallback) = &self.fallback {
                return fallback.token_style(theme, token);
            }
        }
        let theme = known_theme(theme);
        match token {
            "Comment" => token_style(TokenKind::Comment, theme),
            "Text" => Some(theme_base(theme).0).filter(|style| !style.is_null()),
            _ => None,
        }
    }
}

/// Collects token styles into per-line spans.
struct Lines {
    /// Byte offset where each line of `code.split('\n')` starts.
    starts: Vec<usize>,
    len: usize,
    lines: Vec<HighlightedLine>,
}

impl Lines {
    fn new(code: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(code.match_indices('\n').map(|(at, _)| at + 1));
        let lines = vec![HighlightedLine::default(); starts.len()];
        Lines {
            starts,
            len: code.len(),
            lines,
        }
    }

    fn line_of(&self, at: usize) -> usize {
        self.starts.partition_point(|&start| start <= at) - 1
    }

    /// Style `range`, splitting it at line breaks.
    fn push(&mut self, range: Range<usize>, style: &Style) {
        if range.is_empty() || range.end > self.len {
            return;
        }
        for line in self.line_of(range.start)..=self.line_of(range.end - 1) {
            let begin = self.starts[line];
            let end = self.starts.get(line + 1).map_or(self.len, |next| next - 1);
            let (from, to) = (range.start.max(begin), range.end.min(end));
            if from < to {
                self.lines[line].spans.push(HighlightSpan {
                    range: from - begin..to - begin,
                    style: style.clone(),
                });
            }
            if end < self.len && range.start <= end && end < range.end {
                self.lines[line].newline_style = Some(style.clone());
            }
        }
    }
}

/// Registers [`SqlHighlighter`] as the code highlighter `"sql"`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SqlPlugin;

impl Plugin for SqlPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("sql", "SQL highlighter", env!("CARGO_PKG_VERSION"))
            .description("a pure-Rust SQL lexer with Jinja blocks, for SQL code")
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.code_highlighter("sql", SqlHighlighter::shared());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
