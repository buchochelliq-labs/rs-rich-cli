//! SQL DDL in the format-neutral model: a small `CREATE TABLE` subset.
//!
//! [`parse`] reads a file of statements into a [`Schema`] of tables, one
//! per `CREATE TABLE`, each column a [`Field`] with its type as written
//! (`VARCHAR(20)`, kept as the [native type](Field::native_type)) and the
//! model's type for it (`string`). It reads:
//!
//! - column names and types, quoted (`"x"`, `` `x` ``, `[x]`) or not, with
//!   their parameters (`NUMERIC(10, 2)`), several words
//!   (`DOUBLE PRECISION`, `TIMESTAMP WITH TIME ZONE`) and arrays (`INT[]`);
//! - `NOT NULL` and `NULL`, `DEFAULT` (any expression, kept as written),
//!   `PRIMARY KEY`, `UNIQUE` and `REFERENCES table (column)`, on a column
//!   or, for one or several columns, on the table (`PRIMARY KEY (a, b)`,
//!   `UNIQUE (a, b)`, `FOREIGN KEY (a) REFERENCES t (x)`), named with
//!   `CONSTRAINT name` or not, and MySQL's `ENUM('a', 'b')` and `COMMENT`;
//! - `--` and `/* … */` comments, and any number of statements.
//!
//! `CHECK` constraints, indexes, generated columns and other clauses are
//! skipped, as is every statement but `CREATE TABLE` (`CREATE INDEX`,
//! `ALTER TABLE`, `INSERT`, …); each skip is a [`Note`] with its line, so
//! nothing is dropped silently. Text that cannot be read (an unclosed
//! string, comment or parenthesis, a `CREATE TABLE` without a column list)
//! is a [`SqlError`] with its line. Input is bounded by [`MAX_INPUT`],
//! [`MAX_STATEMENTS`], [`MAX_COLUMNS`] and [`MAX_NESTING`], and the reader
//! never panics.
//!
//! ```
//! use rich_ext::schema::{sql, DataType};
//!
//! let ddl = "
//!     -- The people.
//!     CREATE TABLE users (
//!         id    BIGINT PRIMARY KEY,
//!         email VARCHAR(255) NOT NULL UNIQUE,
//!         team  INT REFERENCES teams (id),
//!         CHECK (email <> '')
//!     );
//!     CREATE INDEX users_email ON users (email);
//! ";
//! let parsed = sql::parse(ddl).unwrap();
//! let users = parsed.schema.table("users").unwrap();
//! let email = users.field("email").unwrap();
//! assert_eq!(email.native_type(), Some("VARCHAR(255)"));
//! assert_eq!(email.data_type(), &DataType::String);
//! assert!(email.is_required() && email.is_unique());
//! assert_eq!(users.primary_key(), ["id"]);
//! assert_eq!(users.foreign_keys()[0].to_string(), "team → teams.id");
//! let notes: Vec<String> = parsed.notes.iter().map(ToString::to_string).collect();
//! assert_eq!(
//!     notes,
//!     [
//!         "line 7: users: CHECK constraint skipped",
//!         "line 9: CREATE INDEX statement skipped",
//!     ]
//! );
//! ```

use std::fmt;

use super::model::{Constraint, DataType, Field, ForeignKey, Literal, Schema};

/// The most bytes of DDL read.
pub const MAX_INPUT: usize = 16 * 1024 * 1024;

/// The most statements read.
pub const MAX_STATEMENTS: usize = 100_000;

/// The most columns (and table constraints) in one table.
pub const MAX_COLUMNS: usize = 4_096;

/// The deepest parentheses nest.
pub const MAX_NESTING: usize = 64;

/// The most notes kept; past this they are counted in a last one.
pub const MAX_NOTES: usize = 1_000;

/// Why DDL could not be read, and on which line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlError {
    line: usize,
    message: String,
}

impl SqlError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        SqlError {
            line,
            message: message.into(),
        }
    }

    /// The line (from 1).
    pub fn line(&self) -> usize {
        self.line
    }

    /// What was wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for SqlError {}

/// Something skipped, and on which line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The line (from 1).
    pub line: usize,
    /// What was skipped.
    pub message: String,
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// DDL read into the model: a schema of tables, and what was skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Parsed {
    /// One table per `CREATE TABLE`, in order.
    pub schema: Schema,
    /// What was skipped, in order.
    pub notes: Vec<Note>,
}

/// Read DDL. See the [module docs](self).
pub fn parse(text: &str) -> Result<Parsed, SqlError> {
    if text.len() > MAX_INPUT {
        return Err(SqlError::new(
            1,
            format!(
                "the DDL is {} bytes, past the limit of {MAX_INPUT}",
                text.len()
            ),
        ));
    }
    let tokens = tokenize(text)?;
    let mut reader = Reader {
        schema: Schema::default(),
        notes: Vec::new(),
        dropped: 0,
    };
    let mut statements = 0;
    for statement in statements_of(&tokens)? {
        statements += 1;
        if statements > MAX_STATEMENTS {
            return Err(SqlError::new(
                statement[0].line,
                format!("more than {MAX_STATEMENTS} statements"),
            ));
        }
        reader.statement(statement)?;
    }
    if reader.dropped > 0 {
        let line = tokens.last().map_or(1, |t| t.line);
        reader.notes.push(Note {
            line,
            message: format!("… and {} more notes", reader.dropped),
        });
    }
    Ok(Parsed {
        schema: reader.schema,
        notes: reader.notes,
    })
}

// --------------------------------------------------------------- tokens

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A bare word: a keyword or an unquoted name.
    Word,
    /// A quoted name.
    Quoted,
    /// A string literal, kept with its quotes.
    Str,
    /// A number.
    Number,
    /// Punctuation or an operator.
    Symbol,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    /// As written.
    text: String,
    /// A name's value: unquoted, escapes undone.
    value: String,
    line: usize,
}

impl Token {
    /// Whether this is the keyword `word` (in any case).
    fn is(&self, word: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(word)
    }

    fn is_symbol(&self, symbol: &str) -> bool {
        self.kind == Kind::Symbol && self.text == symbol
    }

    fn is_name(&self) -> bool {
        matches!(self.kind, Kind::Word | Kind::Quoted)
    }
}

fn tokenize(text: &str) -> Result<Vec<Token>, SqlError> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let take = |from: usize, to: usize| -> String { chars[from..to].iter().collect() };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '-' && next == Some('-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let start = line;
            i += 2;
            loop {
                match chars.get(i) {
                    None => return Err(SqlError::new(start, "a /* comment is not closed")),
                    Some('*') if chars.get(i + 1) == Some(&'/') => {
                        i += 2;
                        break;
                    }
                    Some('\n') => line += 1,
                    _ => {}
                }
                i += 1;
            }
        } else if c == '[' && next.is_some_and(|n| n == ']' || n.is_ascii_digit()) {
            // An array type's brackets (`INT[]`, `INT[3]`), not a name.
            let len = if next == Some(']') { 2 } else { 1 };
            let symbol = take(i, i + len);
            tokens.push(Token {
                kind: Kind::Symbol,
                value: symbol.clone(),
                text: symbol,
                line,
            });
            i += len;
        } else if matches!(c, '\'' | '"' | '`' | '[') {
            let close = if c == '[' { ']' } else { c };
            let start = (i, line);
            let mut value = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => {
                        let what = if c == '\'' { "string" } else { "quoted name" };
                        return Err(SqlError::new(start.1, format!("a {what} is not closed")));
                    }
                    Some(&ch) if ch == close => {
                        // A doubled quote is one quote.
                        if close != ']' && chars.get(i + 1) == Some(&close) {
                            value.push(ch);
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        if ch == '\n' {
                            line += 1;
                        }
                        value.push(ch);
                        i += 1;
                    }
                }
            }
            let kind = if c == '\'' { Kind::Str } else { Kind::Quoted };
            tokens.push(Token {
                kind,
                text: take(start.0, i),
                value,
                line: start.1,
            });
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || matches!(chars[i], '_' | '$')) {
                i += 1;
            }
            let word = take(start, i);
            tokens.push(Token {
                kind: Kind::Word,
                value: word.clone(),
                text: word,
                line,
            });
        } else if c.is_ascii_digit() || (c == '.' && next.is_some_and(|n| n.is_ascii_digit())) {
            let start = i;
            while i < chars.len() {
                let ch = chars[i];
                let exponent_sign =
                    matches!(ch, '+' | '-') && i > start && matches!(chars[i - 1], 'e' | 'E');
                if ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E') || exponent_sign {
                    i += 1;
                } else {
                    break;
                }
            }
            let number = take(start, i);
            tokens.push(Token {
                kind: Kind::Number,
                value: number.clone(),
                text: number,
                line,
            });
        } else {
            // `::` (a cast) is one symbol; anything else is one character.
            let len = if c == ':' && next == Some(':') { 2 } else { 1 };
            let symbol = take(i, i + len);
            tokens.push(Token {
                kind: Kind::Symbol,
                value: symbol.clone(),
                text: symbol,
                line,
            });
            i += len;
        }
    }
    Ok(tokens)
}

/// The statements: tokens up to each `;` outside parentheses.
fn statements_of(tokens: &[Token]) -> Result<Vec<&[Token]>, SqlError> {
    let mut out = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if token.is_symbol("(") {
            open.push(token.line);
            if open.len() > MAX_NESTING {
                return Err(SqlError::new(
                    token.line,
                    format!("parentheses nest more than {MAX_NESTING} deep"),
                ));
            }
        } else if token.is_symbol(")") {
            if open.pop().is_none() {
                return Err(SqlError::new(token.line, "a `)` closes nothing"));
            }
        } else if token.is_symbol(";") && open.is_empty() {
            if index > start {
                out.push(&tokens[start..index]);
            }
            start = index + 1;
        }
    }
    if let Some(line) = open.first() {
        return Err(SqlError::new(*line, "a `(` is not closed"));
    }
    if start < tokens.len() {
        out.push(&tokens[start..]);
    }
    Ok(out)
}

/// `tokens` split at commas outside parentheses.
fn split_commas(tokens: &[Token]) -> Vec<&[Token]> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if token.is_symbol("(") {
            depth += 1;
        } else if token.is_symbol(")") {
            depth = depth.saturating_sub(1);
        } else if token.is_symbol(",") && depth == 0 {
            out.push(&tokens[start..index]);
            start = index + 1;
        }
    }
    out.push(&tokens[start..]);
    out
}

/// The index just past the parenthesis that closes the one at `open`.
fn closing(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is_symbol("(") {
            depth += 1;
        } else if token.is_symbol(")") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return index + 1;
            }
        }
    }
    tokens.len()
}

/// Tokens written back as SQL: words spaced, punctuation close.
fn write(tokens: &[Token]) -> String {
    let mut out = String::new();
    for (index, token) in tokens.iter().enumerate() {
        if index > 0 && space_before(tokens, index) {
            out.push(' ');
        }
        out.push_str(&token.text);
    }
    out
}

/// Whether [`write`] puts a space before token `index`.
fn space_before(tokens: &[Token], index: usize) -> bool {
    let (prev, token) = (&tokens[index - 1], &tokens[index]);
    let symbol = |t: &Token, any: &[&str]| t.kind == Kind::Symbol && any.contains(&t.text.as_str());
    if symbol(token, &[")", ",", "[", "]", "[]", "::", "."]) || symbol(prev, &["(", "[", "::", "."])
    {
        return false;
    }
    // A call or a type's parameters: `now()`, `NUMERIC(10, 2)`.
    if token.is_symbol("(") && prev.is_name() {
        return false;
    }
    // A sign that starts a value stays with its number: `-1`, `(-1`.
    let starts = index == 1 || symbol(&tokens[index - 2], &["(", ","]);
    !(symbol(prev, &["-", "+"]) && token.kind == Kind::Number && starts)
}

/// The names in a parenthesised list starting at `open`, and the index past
/// it.
fn name_list(tokens: &[Token], open: usize) -> (Vec<String>, usize) {
    let end = closing(tokens, open);
    let inner = &tokens[(open + 1).min(end)..end.saturating_sub(1).max(open + 1)];
    let names = split_commas(inner)
        .into_iter()
        .filter_map(|part| part.iter().find(|t| t.is_name()).map(|t| t.value.clone()))
        .collect();
    (names, end)
}

/// A possibly qualified name (`a.b.c`) at `at`, and the index past it.
fn qualified(tokens: &[Token], mut at: usize) -> Option<(String, usize)> {
    let mut parts = Vec::new();
    loop {
        let token = tokens.get(at).filter(|t| t.is_name())?;
        parts.push(token.value.clone());
        at += 1;
        if tokens.get(at).is_some_and(|t| t.is_symbol(".")) {
            at += 1;
        } else {
            return Some((parts.join("."), at));
        }
    }
}

// --------------------------------------------------------------- statements

struct Reader {
    schema: Schema,
    notes: Vec<Note>,
    dropped: usize,
}

/// Words that end a column's type and start its constraints.
const COLUMN_KEYWORDS: &[&str] = &[
    "CONSTRAINT",
    "NOT",
    "NULL",
    "DEFAULT",
    "PRIMARY",
    "UNIQUE",
    "REFERENCES",
    "CHECK",
    "COLLATE",
    "GENERATED",
    "AS",
    "AUTO_INCREMENT",
    "AUTOINCREMENT",
    "IDENTITY",
    "COMMENT",
    "ON",
    "CHARACTER",
    "CHARSET",
];

impl Reader {
    fn note(&mut self, line: usize, message: impl Into<String>) {
        if self.notes.len() < MAX_NOTES {
            self.notes.push(Note {
                line,
                message: message.into(),
            });
        } else {
            self.dropped += 1;
        }
    }

    fn statement(&mut self, tokens: &[Token]) -> Result<(), SqlError> {
        let line = tokens[0].line;
        let mut at = 0;
        let word = |at: usize, w: &str| tokens.get(at).is_some_and(|t| t.is(w));
        if !word(0, "CREATE") {
            let what = tokens[0].text.to_uppercase();
            self.note(line, format!("{what} statement skipped"));
            return Ok(());
        }
        at += 1;
        if word(at, "OR") && word(at + 1, "REPLACE") {
            at += 2;
        }
        while [
            "GLOBAL",
            "LOCAL",
            "TEMP",
            "TEMPORARY",
            "UNLOGGED",
            "VIRTUAL",
        ]
        .iter()
        .any(|w| word(at, w))
        {
            at += 1;
        }
        if !word(at, "TABLE") {
            // `CREATE UNIQUE INDEX`: the words up to the kind of object.
            let objects = [
                "INDEX",
                "VIEW",
                "SEQUENCE",
                "TYPE",
                "FUNCTION",
                "TRIGGER",
                "SCHEMA",
                "EXTENSION",
                "DATABASE",
                "PROCEDURE",
                "DOMAIN",
            ];
            let mut what: Vec<String> = Vec::new();
            for token in tokens.iter().take_while(|t| t.kind == Kind::Word).take(5) {
                what.push(token.text.to_uppercase());
                if objects.iter().any(|w| token.is(w)) {
                    break;
                }
            }
            self.note(line, format!("{} statement skipped", what.join(" ")));
            return Ok(());
        }
        at += 1;
        if word(at, "IF") && word(at + 1, "NOT") && word(at + 2, "EXISTS") {
            at += 3;
        }
        let Some((name, after)) = qualified(tokens, at) else {
            let line = tokens.get(at).map_or(line, |t| t.line);
            return Err(SqlError::new(line, "CREATE TABLE needs a table name"));
        };
        at = after;
        if word(at, "AS") || word(at, "LIKE") {
            let what = tokens[at].text.to_uppercase();
            self.note(line, format!("{name}: CREATE TABLE … {what} skipped"));
            return Ok(());
        }
        if !tokens.get(at).is_some_and(|t| t.is_symbol("(")) {
            let line = tokens.get(at).map_or(line, |t| t.line);
            return Err(SqlError::new(
                line,
                format!("CREATE TABLE {name} needs a column list in parentheses"),
            ));
        }
        let end = closing(tokens, at);
        let body = &tokens[at + 1..end - 1];
        let table = self.table(&name, body, line)?;
        if end < tokens.len() {
            // Table options (`ENGINE=InnoDB`, `WITHOUT ROWID`, `STRICT`):
            // nothing the model holds.
            let options = &tokens[end..];
            let comment = options
                .iter()
                .position(|t| t.is("COMMENT"))
                .and_then(|at| options[at + 1..].iter().find(|t| !t.is_symbol("=")))
                .filter(|t| t.kind == Kind::Str)
                .map(|t| t.value.clone());
            let table = match comment {
                Some(comment) => table.with_description(comment),
                None => table,
            };
            self.add(table, line);
        } else {
            self.add(table, line);
        }
        Ok(())
    }

    fn add(&mut self, table: Schema, line: usize) {
        let name = table.name().unwrap_or_default().to_string();
        if let Some(index) = self
            .schema
            .tables()
            .iter()
            .position(|t| t.name() == Some(&name))
        {
            self.note(
                line,
                format!("{name}: defined again; the later definition is kept"),
            );
            self.schema.tables_mut()[index] = table;
        } else {
            self.schema.push_table(table);
        }
    }

    fn table(&mut self, name: &str, body: &[Token], line: usize) -> Result<Schema, SqlError> {
        let parts = split_commas(body);
        if parts.len() > MAX_COLUMNS {
            return Err(SqlError::new(
                line,
                format!("{name}: more than {MAX_COLUMNS} columns"),
            ));
        }
        let mut table = Schema::default().named(name);
        let mut keys: Vec<(usize, TableKey)> = Vec::new();
        for part in parts {
            let Some(first) = part.first() else {
                continue;
            };
            let mut at = 0;
            if first.is("CONSTRAINT") {
                at = 2;
            }
            let Some(head) = part.get(at) else {
                return Err(SqlError::new(
                    first.line,
                    format!("{name}: CONSTRAINT needs a rule"),
                ));
            };
            let word = |at: usize, w: &str| part.get(at).is_some_and(|t| t.is(w));
            let open = |at: usize| part.get(at).is_some_and(|t| t.is_symbol("("));
            if head.is("PRIMARY") && word(at + 1, "KEY") {
                let open_at = at + 2;
                if !open(open_at) {
                    return Err(SqlError::new(
                        head.line,
                        format!("{name}: PRIMARY KEY needs its columns"),
                    ));
                }
                let (columns, _) = name_list(part, open_at);
                keys.push((head.line, TableKey::Primary(columns)));
            } else if head.is("UNIQUE") {
                let mut open_at = at + 1;
                while part.get(open_at).is_some_and(|t| !t.is_symbol("(")) {
                    open_at += 1;
                }
                if !open(open_at) {
                    return Err(SqlError::new(
                        head.line,
                        format!("{name}: UNIQUE needs its columns"),
                    ));
                }
                let (columns, _) = name_list(part, open_at);
                keys.push((head.line, TableKey::Unique(columns)));
            } else if head.is("FOREIGN") && word(at + 1, "KEY") {
                let mut open_at = at + 2;
                while part.get(open_at).is_some_and(|t| !t.is_symbol("(")) {
                    open_at += 1;
                }
                let (columns, after) = name_list(part, open_at);
                let Some(key) = references(part, after) else {
                    return Err(SqlError::new(
                        head.line,
                        format!("{name}: FOREIGN KEY needs REFERENCES and a table"),
                    ));
                };
                keys.push((head.line, TableKey::Foreign(ForeignKey { columns, ..key })));
            } else if head.is("CHECK") {
                self.note(head.line, format!("{name}: CHECK constraint skipped"));
            } else if [
                "KEY", "INDEX", "FULLTEXT", "SPATIAL", "EXCLUDE", "LIKE", "PERIOD",
            ]
            .iter()
            .any(|w| head.is(w))
            {
                let what = head.text.to_uppercase();
                self.note(head.line, format!("{name}: {what} clause skipped"));
            } else if at == 0 && head.is_name() {
                let field = self.column(name, part)?;
                table.push(field);
            } else {
                return Err(SqlError::new(
                    head.line,
                    format!(
                        "{name}: expected a column or a constraint, found `{}`",
                        head.text
                    ),
                ));
            }
        }
        for (line, key) in keys {
            self.apply(&mut table, key, line);
        }
        Ok(table)
    }

    /// Put a table-level key on its column, or on the table when it spans
    /// several.
    fn apply(&mut self, table: &mut Schema, key: TableKey, line: usize) {
        let columns = key.columns().to_vec();
        let name = table.name().unwrap_or_default().to_string();
        for column in &columns {
            if table.field(column).is_none() {
                self.note(line, format!("{name}: a key names no column {column:?}"));
                return;
            }
        }
        let single = columns.len() == 1;
        let index = |table: &Schema, column: &str| table.index_of(column).unwrap_or(0);
        match key {
            TableKey::Primary(columns) => {
                for column in &columns {
                    let i = index(table, column);
                    let field = &mut table.fields_mut()[i];
                    *field = field.clone().required(true).nullable(false);
                    if single && !field.is_primary_key() {
                        field
                            .constraints_mut()
                            .push(Constraint::PrimaryKey(Vec::new()));
                    }
                }
                if !single {
                    table
                        .constraints_mut()
                        .push(Constraint::PrimaryKey(columns));
                }
            }
            TableKey::Unique(columns) if single => {
                let i = index(table, &columns[0]);
                let field = &mut table.fields_mut()[i];
                if !field.is_unique() {
                    field.constraints_mut().push(Constraint::Unique(Vec::new()));
                }
            }
            TableKey::Unique(columns) => table.constraints_mut().push(Constraint::Unique(columns)),
            TableKey::Foreign(key) if single => {
                let i = index(table, &key.columns[0]);
                let field = &mut table.fields_mut()[i];
                field
                    .constraints_mut()
                    .push(Constraint::References(ForeignKey {
                        columns: Vec::new(),
                        ..key
                    }));
            }
            TableKey::Foreign(key) => table.constraints_mut().push(Constraint::References(key)),
        }
    }

    fn column(&mut self, table: &str, part: &[Token]) -> Result<Field, SqlError> {
        let name = &part[0];
        // The type: everything up to the first constraint keyword.
        let mut at = 1;
        while at < part.len() {
            let token = &part[at];
            if token.is_symbol("(") {
                at = closing(part, at);
                continue;
            }
            if COLUMN_KEYWORDS.iter().any(|w| token.is(w)) {
                // `CHARACTER VARYING` and `DOUBLE PRECISION` are types; a
                // `CHARACTER SET` after a type is not.
                let character_type = token.is("CHARACTER") && at == 1;
                if !character_type {
                    break;
                }
            }
            at += 1;
        }
        let type_tokens = &part[1..at];
        let native = write(type_tokens);
        let mut field = Field::new(name.value.clone(), sql_type(type_tokens));
        if !native.is_empty() {
            field = field.with_native_type(native.clone());
        }
        if let Some(values) = enum_values(type_tokens) {
            field = field.with_constraint(Constraint::Enum(values));
        }
        while at < part.len() {
            let token = &part[at];
            let word = |offset: usize, w: &str| part.get(at + offset).is_some_and(|t| t.is(w));
            if token.is("CONSTRAINT") {
                at += 2;
            } else if token.is("NOT") && word(1, "NULL") {
                field = field.nullable(false).required(true);
                at += 2;
            } else if token.is("NULL") {
                field = field.nullable(true);
                at += 1;
            } else if token.is("DEFAULT") {
                let end = expression_end(part, at + 1);
                let text = write(&part[at + 1..end]);
                field = field.with_constraint(Constraint::Default(Literal::new(text)));
                at = end;
            } else if token.is("PRIMARY") && word(1, "KEY") {
                field = field
                    .nullable(false)
                    .required(true)
                    .with_constraint(Constraint::PrimaryKey(Vec::new()));
                at += 2;
            } else if token.is("UNIQUE") {
                field = field.with_constraint(Constraint::Unique(Vec::new()));
                at += if word(1, "KEY") { 2 } else { 1 };
            } else if token.is("REFERENCES") {
                match references(part, at) {
                    Some(key) => field = field.with_constraint(Constraint::References(key)),
                    None => {
                        return Err(SqlError::new(
                            token.line,
                            format!("{table}.{}: REFERENCES needs a table", name.value),
                        ))
                    }
                }
                at = skip_reference(part, at);
            } else if token.is("CHECK") {
                self.note(
                    token.line,
                    format!("{table}.{}: CHECK constraint skipped", name.value),
                );
                at += 1;
                if part.get(at).is_some_and(|t| t.is_symbol("(")) {
                    at = closing(part, at);
                }
            } else if token.is("COMMENT") && part.get(at + 1).is_some_and(|t| t.kind == Kind::Str) {
                field = field.with_description(part[at + 1].value.clone());
                at += 2;
            } else if token.is_symbol("(") {
                at = closing(part, at);
            } else {
                // `COLLATE x`, `GENERATED … AS (…)`, `AUTO_INCREMENT`,
                // `ON UPDATE …`: nothing the model holds.
                at += 1;
            }
        }
        Ok(field)
    }
}

/// A table-level key, before it is placed.
enum TableKey {
    Primary(Vec<String>),
    Unique(Vec<String>),
    Foreign(ForeignKey),
}

impl TableKey {
    fn columns(&self) -> &[String] {
        match self {
            TableKey::Primary(columns) | TableKey::Unique(columns) => columns,
            TableKey::Foreign(key) => &key.columns,
        }
    }
}

/// `REFERENCES table [(columns)]` at `at` (the `REFERENCES` keyword, or
/// the token before it).
fn references(tokens: &[Token], mut at: usize) -> Option<ForeignKey> {
    while tokens.get(at).is_some_and(|t| !t.is("REFERENCES")) {
        at += 1;
    }
    let (table, after) = qualified(tokens, at + 1)?;
    let references = if tokens.get(after).is_some_and(|t| t.is_symbol("(")) {
        name_list(tokens, after).0
    } else {
        Vec::new()
    };
    Some(ForeignKey {
        columns: Vec::new(),
        table,
        references,
    })
}

/// The index past a `REFERENCES` clause and its `ON DELETE …`, `MATCH …`
/// and `DEFERRABLE …` options.
fn skip_reference(tokens: &[Token], at: usize) -> usize {
    let Some((_, mut at)) = qualified(tokens, at + 1) else {
        return tokens.len();
    };
    if tokens.get(at).is_some_and(|t| t.is_symbol("(")) {
        at = closing(tokens, at);
    }
    let options = [
        "ON",
        "DELETE",
        "UPDATE",
        "CASCADE",
        "RESTRICT",
        "SET",
        "NULL",
        "DEFAULT",
        "NO",
        "ACTION",
        "MATCH",
        "FULL",
        "PARTIAL",
        "SIMPLE",
        "DEFERRABLE",
        "INITIALLY",
        "DEFERRED",
        "IMMEDIATE",
        "NOT",
    ];
    while let Some(token) = tokens.get(at) {
        // `NOT NULL` after a reference is the column's, not the reference's.
        if token.is("NOT") && tokens.get(at + 1).is_some_and(|t| t.is("NULL")) {
            break;
        }
        // So are a `NULL` and a `DEFAULT` that do not follow `SET`.
        let after_set = tokens.get(at.wrapping_sub(1)).is_some_and(|t| t.is("SET"));
        if (token.is("NULL") || token.is("DEFAULT")) && !after_set {
            break;
        }
        if options.iter().any(|w| token.is(w)) {
            at += 1;
        } else {
            break;
        }
    }
    at
}

/// The index past a `DEFAULT` expression starting at `at`: up to the next
/// constraint keyword outside parentheses.
fn expression_end(tokens: &[Token], mut at: usize) -> usize {
    let start = at;
    while let Some(token) = tokens.get(at) {
        if token.is_symbol("(") {
            at = closing(tokens, at);
            continue;
        }
        let keyword = [
            "NOT",
            "NULL",
            "PRIMARY",
            "UNIQUE",
            "REFERENCES",
            "CHECK",
            "CONSTRAINT",
            "COLLATE",
            "GENERATED",
            "COMMENT",
            "AUTO_INCREMENT",
            "AUTOINCREMENT",
            "ON",
        ]
        .iter()
        .any(|w| token.is(w));
        // `DEFAULT NULL` is the value NULL.
        if keyword && at > start {
            break;
        }
        at += 1;
    }
    at
}

/// MySQL's `ENUM('a', 'b')` values.
fn enum_values(type_tokens: &[Token]) -> Option<Vec<Literal>> {
    let first = type_tokens.first()?;
    if !(first.is("ENUM") || first.is("SET")) {
        return None;
    }
    let values: Vec<Literal> = type_tokens
        .iter()
        .filter(|t| t.kind == Kind::Str)
        .map(|t| Literal::new(t.text.clone()))
        .collect();
    (!values.is_empty()).then_some(values)
}

/// The model's type for a SQL type.
fn sql_type(type_tokens: &[Token]) -> DataType {
    let words: Vec<String> = type_tokens
        .iter()
        .filter(|t| t.kind == Kind::Word)
        .map(|t| t.text.to_uppercase())
        .collect();
    let Some(base) = words.first() else {
        return DataType::Unknown;
    };
    let array = words.iter().any(|w| w == "ARRAY")
        || type_tokens
            .iter()
            .any(|t| t.is_symbol("[]") || t.is_symbol("["));
    let has = |word: &str| words.iter().any(|w| w == word);
    let numbers: Vec<i64> = type_tokens
        .iter()
        .filter(|t| t.kind == Kind::Number)
        .filter_map(|t| t.text.parse().ok())
        .collect();
    let scalar = match base.as_str() {
        "INT" | "INTEGER" | "SMALLINT" | "BIGINT" | "TINYINT" | "MEDIUMINT" | "SERIAL"
        | "BIGSERIAL" | "SMALLSERIAL" | "INT2" | "INT4" | "INT8" | "SERIAL4" | "SERIAL8"
        | "UNSIGNED" => DataType::Integer,
        "REAL" | "FLOAT" | "FLOAT4" | "FLOAT8" | "DOUBLE" => DataType::Float,
        "DECIMAL" | "NUMERIC" | "DEC" | "NUMBER" | "MONEY" | "SMALLMONEY" => DataType::Decimal {
            precision: numbers.first().and_then(|&p| u32::try_from(p).ok()),
            scale: numbers.get(1).and_then(|&s| i32::try_from(s).ok()),
        },
        "CHAR" | "VARCHAR" | "CHARACTER" | "NCHAR" | "NVARCHAR" | "VARCHAR2" | "NVARCHAR2"
        | "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "NTEXT" | "CLOB" | "NCLOB"
        | "STRING" | "CITEXT" | "UUID" | "UNIQUEIDENTIFIER" | "ENUM" | "SET" | "INET" | "CIDR"
        | "MACADDR" | "XML" => DataType::String,
        "BOOLEAN" | "BOOL" => DataType::Boolean,
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BYTEA" | "BINARY" | "VARBINARY"
        | "IMAGE" | "RAW" => DataType::Binary,
        "DATE" => DataType::Date,
        "TIMESTAMPTZ" => DataType::Timestamp {
            timezone: Some("UTC".into()),
        },
        "TIMESTAMP" | "DATETIME" | "DATETIME2" | "SMALLDATETIME" | "DATETIMEOFFSET" => {
            let zoned = (has("WITH") && !has("WITHOUT")) || base == "DATETIMEOFFSET";
            DataType::Timestamp {
                timezone: zoned.then(|| "UTC".to_string()),
            }
        }
        "JSON" | "JSONB" | "VARIANT" => DataType::Any,
        _ => DataType::Unknown,
    };
    if array {
        DataType::list(scalar)
    } else {
        scalar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(parsed: &Parsed) -> Vec<String> {
        parsed.notes.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn columns_types_and_keys() {
        let parsed = parse(
            r#"
            CREATE TABLE IF NOT EXISTS "public"."order items" (
                "order" BIGINT NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
                line INT NOT NULL,
                price NUMERIC(10, 2) DEFAULT 0.00 NOT NULL,
                status ENUM('new', 'paid') DEFAULT 'new' COMMENT 'Where it is',
                tags TEXT[],
                at TIMESTAMP WITH TIME ZONE DEFAULT now(),
                weight DOUBLE PRECISION NULL,
                code CHARACTER VARYING(8) UNIQUE,
                PRIMARY KEY ("order", line),
                UNIQUE (line, code),
                CONSTRAINT fk FOREIGN KEY (line, code) REFERENCES lines (n, c)
            ) ENGINE=InnoDB COMMENT 'Lines';
            "#,
        )
        .unwrap();
        assert!(parsed.notes.is_empty(), "{:?}", parsed.notes);
        let table = &parsed.schema.tables()[0];
        assert_eq!(table.name(), Some("public.order items"));
        assert_eq!(table.description(), Some("Lines"));
        let types: Vec<(String, String)> = table
            .fields()
            .iter()
            .map(|f| {
                (
                    f.native_type().unwrap().to_string(),
                    f.data_type().to_string(),
                )
            })
            .collect();
        let types: Vec<(&str, &str)> = types
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        assert_eq!(
            types,
            [
                ("BIGINT", "integer"),
                ("INT", "integer"),
                ("NUMERIC(10, 2)", "decimal(10,2)"),
                ("ENUM('new', 'paid')", "string"),
                ("TEXT[]", "list<string>"),
                ("TIMESTAMP WITH TIME ZONE", "timestamp[UTC]"),
                ("DOUBLE PRECISION", "float"),
                ("CHARACTER VARYING(8)", "string"),
            ]
        );
        let order = table.field("order").unwrap();
        assert!(order.is_required() && !order.is_nullable());
        assert_eq!(order.references().unwrap().to_string(), "orders.id");
        assert_eq!(table.primary_key(), ["order", "line"]);
        let price = table.field("price").unwrap();
        assert_eq!(price.default_value().unwrap().as_str(), "0.00");
        assert!(price.is_required());
        let status = table.field("status").unwrap();
        assert_eq!(status.default_value().unwrap().as_str(), "'new'");
        assert_eq!(status.description(), Some("Where it is"));
        assert_eq!(status.enum_values().unwrap().len(), 2);
        assert_eq!(
            table.field("at").unwrap().default_value().unwrap().as_str(),
            "now()"
        );
        assert!(table.field("code").unwrap().is_unique());
        let shown: Vec<String> = table
            .constraints()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            shown,
            [
                "primary key (order, line)",
                "unique (line, code)",
                "→ (line, code) → lines.(n, c)",
            ]
        );
    }

    #[test]
    fn a_default_after_a_reference_is_the_columns() {
        let parsed = parse(
            "CREATE TABLE t (\
             a INT REFERENCES p (id) DEFAULT 0, \
             b INT REFERENCES p (id) ON DELETE SET DEFAULT DEFAULT 1 NOT NULL, \
             c INT REFERENCES p ON UPDATE SET NULL NULL);",
        )
        .unwrap();
        let t = &parsed.schema.tables()[0];
        let a = t.field("a").unwrap();
        assert_eq!(a.default_value().unwrap().as_str(), "0");
        assert_eq!(a.references().unwrap().to_string(), "p.id");
        let b = t.field("b").unwrap();
        assert_eq!(b.default_value().unwrap().as_str(), "1");
        assert!(b.is_required());
        let c = t.field("c").unwrap();
        assert!(c.default_value().is_none() && c.is_nullable());
    }

    #[test]
    fn single_column_table_keys_go_on_the_column() {
        let parsed = parse(
            "CREATE TABLE t (a INT, b INT, PRIMARY KEY (a), UNIQUE (b), \
             FOREIGN KEY (b) REFERENCES u);",
        )
        .unwrap();
        let t = &parsed.schema.tables()[0];
        assert!(t.constraints().is_empty());
        assert!(t.field("a").unwrap().is_primary_key());
        assert!(t.field("a").unwrap().is_required());
        assert!(t.field("b").unwrap().is_unique());
        assert_eq!(t.foreign_keys()[0].to_string(), "b → u");
    }

    #[test]
    fn other_statements_and_clauses_are_noted() {
        let parsed = parse(
            "/* a dump */\n\
             SET NAMES utf8;\n\
             CREATE TABLE a (x INT CHECK (x > 0), KEY ix (x));\n\
             CREATE UNIQUE INDEX i ON a (x);\n\
             CREATE TABLE b AS SELECT * FROM a;\n\
             CREATE TABLE a (y TEXT);\n\
             INSERT INTO a VALUES (1);",
        )
        .unwrap();
        assert_eq!(
            notes(&parsed),
            [
                "line 2: SET statement skipped",
                "line 3: a.x: CHECK constraint skipped",
                "line 3: a: KEY clause skipped",
                "line 4: CREATE UNIQUE INDEX statement skipped",
                "line 5: b: CREATE TABLE … AS skipped",
                "line 6: a: defined again; the later definition is kept",
                "line 7: INSERT statement skipped",
            ]
        );
        assert_eq!(parsed.schema.tables().len(), 1);
        assert_eq!(parsed.schema.tables()[0].fields()[0].name(), "y");
    }

    #[test]
    fn errors_name_their_line() {
        let cases = [
            ("CREATE TABLE t (\n a INT", "line 1: a `(` is not closed"),
            ("\n\nSELECT 'oops", "line 3: a string is not closed"),
            ("/* never\nclosed", "line 1: a /* comment is not closed"),
            (
                "CREATE TABLE t\n;",
                "line 1: CREATE TABLE t needs a column list in parentheses",
            ),
            (
                "CREATE TABLE (a INT);",
                "line 1: CREATE TABLE needs a table name",
            ),
            ("x)", "line 1: a `)` closes nothing"),
            (
                "CREATE TABLE t (a INT REFERENCES);",
                "line 1: t.a: REFERENCES needs a table",
            ),
            (
                "CREATE TABLE t (\n, PRIMARY KEY);",
                "line 2: t: PRIMARY KEY needs its columns",
            ),
        ];
        for (ddl, expected) in cases {
            assert_eq!(parse(ddl).unwrap_err().to_string(), expected, "{ddl}");
        }
    }

    #[test]
    fn limits_hold() {
        let deep = format!(
            "CREATE TABLE t (a INT DEFAULT {}1{});",
            "(".repeat(100),
            ")".repeat(100)
        );
        assert!(parse(&deep).unwrap_err().message().contains("nest"));
        let wide: Vec<String> = (0..=MAX_COLUMNS).map(|i| format!("c{i} INT")).collect();
        let wide = format!("CREATE TABLE t ({});", wide.join(", "));
        assert!(parse(&wide).unwrap_err().message().contains("columns"));
        let mut many = String::new();
        for i in 0..1_200 {
            many.push_str(&format!("INSERT INTO t VALUES ({i});\n"));
        }
        let parsed = parse(&many).unwrap();
        assert_eq!(parsed.notes.len(), MAX_NOTES + 1);
        assert_eq!(parsed.notes.last().unwrap().message, "… and 200 more notes");
    }

    #[test]
    fn odd_input_never_panics() {
        for ddl in [
            "",
            ";;;",
            "CREATE",
            "CREATE TABLE",
            "CREATE TABLE t ()",
            "CREATE TABLE t (CONSTRAINT)",
            "CREATE TABLE t (a)",
            "CREATE TABLE t (a INT, , b INT)",
            "CREATE TABLE t (FOREIGN KEY (a) REFERENCES)",
            "CREATE TABLE t (UNIQUE)",
            "CREATE TABLE t (a INT DEFAULT)",
            "CREATE TABLE t (a INT REFERENCES u (",
            "CREATE TABLE [t (a INT)",
            "CREATE TABLE \"t\" (\"a\"\"b\" INT)",
            "ÄÖ ü 1e+ .5 :: ::: \u{0}",
        ] {
            let _ = parse(ddl);
        }
        let parsed = parse("CREATE TABLE \"t\" (\"a\"\"b\" INT, c)").unwrap();
        let t = &parsed.schema.tables()[0];
        assert_eq!(t.fields()[0].name(), "a\"b");
        assert_eq!(t.fields()[1].data_type(), &DataType::Unknown);
    }
}
