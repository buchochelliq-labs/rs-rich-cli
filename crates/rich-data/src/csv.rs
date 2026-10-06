//! CSV and TSV: a port of Python's `csv` sniffer and reader, and a
//! [`CsvReader`] that turns a file into [`Rows`].
//!
//! [`sniff`], [`has_header`] and [`read_rows`] are the reader `rich --csv`
//! has always used (moved here from `rs-rich-cli` in 0.0.16 so every caller
//! shares it): `csv.Sniffer.sniff`, `csv.Sniffer.has_header` and
//! `_csv.reader`, ported closely enough that `rich --csv` keeps rich-cli
//! 1.8.1's output byte for byte. They take text that has already been through
//! Python's universal newlines (no `\r`); [`CsvReader`] does that first.
//!
//! [`CsvReader`] is the adapter: it sniffs the dialect and the header the way
//! `--csv` does (or takes them as given), drops blank lines, and yields every
//! cell as [`Value::Str`] exactly as written. Nothing is typed until
//! [`infer`](crate::infer) is asked to.
//!
//! ```
//! use rich_data::csv::CsvReader;
//! use rich_data::Value;
//!
//! let rows = CsvReader::new().read("name;age\nAda;36\nAlan;41\n").unwrap();
//! assert_eq!(rows.columns(), ["name", "age"]);
//! assert_eq!(rows.rows()[1], [Value::from("Alan"), Value::from("41")]);
//!
//! let tsv = CsvReader::tsv().header(false).read("a\tb\n").unwrap();
//! assert_eq!(tsv.columns(), ["1", "2"]);
//! ```

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{BufRead, Read};

use crate::{DataError, Row, RowSource, Rows, Value};

/// The delimiters [`CsvReader`] (and `rich --csv`) lets the sniffer choose
/// from: rich-cli's `",\t|;"`.
pub const DELIMITERS: [char; 4] = [',', '\t', '|', ';'];

/// How many characters of the input the sniffer reads: rich-cli's
/// `csv_data[:1024]`.
pub const SAMPLE_CHARS: usize = 1024;

/// Whether the first row is a header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Header {
    /// Decide with [`has_header`], as `rich --csv` does; when the sniffer
    /// cannot tell, the first row is a header.
    #[default]
    Sniff,
    /// The first row is a header.
    Yes,
    /// There is no header; columns are named `1`, `2`, …
    No,
    /// The first row is a header unless every cell in it is a number (or
    /// empty), as `rich chart` and `rich profile` decide. Python's sniffer
    /// says "no header" whenever every column is ragged in length or type,
    /// which one empty cell can make it.
    UnlessNumeric,
}

/// Reads CSV or TSV text into [`Rows`]. See the [module docs](self).
#[derive(Clone, Copy, Debug, Default)]
pub struct CsvReader {
    dialect: Option<Dialect>,
    fallback: Option<char>,
    header: Header,
}

impl CsvReader {
    /// Sniff the dialect from [`DELIMITERS`] and the header, failing when the
    /// sniffer finds no delimiter (set a [`fallback`](Self::fallback) to
    /// read such input anyway).
    pub fn new() -> Self {
        CsvReader::default()
    }

    /// Tab-separated, with excel's quoting (`csv.get_dialect("excel-tab")`).
    pub fn tsv() -> Self {
        CsvReader::new().delimiter('\t')
    }

    /// Use this delimiter, with excel's quoting, instead of sniffing.
    pub fn delimiter(mut self, delimiter: char) -> Self {
        self.dialect = Some(Dialect::excel(delimiter));
        self
    }

    /// Use this dialect instead of sniffing.
    pub fn dialect(mut self, dialect: Dialect) -> Self {
        self.dialect = Some(dialect);
        self
    }

    /// The delimiter to fall back to when the sniffer finds none, as
    /// `rich --csv` falls back to `,` for a `.csv` file and a tab for a
    /// `.tsv` one.
    pub fn fallback(mut self, delimiter: char) -> Self {
        self.fallback = Some(delimiter);
        self
    }

    /// Whether the first row is a header (`true`/`false`), instead of
    /// sniffing it.
    pub fn header(mut self, header: bool) -> Self {
        self.header = if header { Header::Yes } else { Header::No };
        self
    }

    /// Set the header decision, [`Header::Sniff`] included.
    pub fn header_mode(mut self, header: Header) -> Self {
        self.header = header;
        self
    }

    /// The dialect and header decision for `content`, as [`read`](Self::read)
    /// makes them; `None` when the sniffer finds no delimiter and there is no
    /// fallback.
    pub fn detect(&self, content: &str) -> Option<(Dialect, bool)> {
        let sample: String = content.chars().take(SAMPLE_CHARS).collect();
        let dialect = match self.dialect {
            Some(dialect) => dialect,
            None => {
                sniff(&sample, Some(&DELIMITERS)).or_else(|| self.fallback.map(Dialect::excel))?
            }
        };
        let header = match self.header {
            Header::Yes => true,
            Header::No => false,
            Header::Sniff => has_header(&sample).unwrap_or(true),
            Header::UnlessNumeric => {
                let first = read_rows(&sample, &dialect)
                    .into_iter()
                    .find(|row| !row.is_empty())
                    .unwrap_or_default();
                first.iter().any(|cell| {
                    let cell = cell.trim();
                    !cell.is_empty() && cell.parse::<f64>().is_err()
                })
            }
        };
        Some((dialect, header))
    }

    /// Read `content`. Blank lines are skipped; a row longer than the header
    /// adds columns named by position, and a shorter one is padded with
    /// nulls, so no field is lost.
    pub fn read(&self, content: &str) -> Result<Rows, DataError> {
        let content = universal_newlines(content);
        // Sniffed past a byte-order mark, as `source` sniffs.
        let content = content.strip_prefix('\u{feff}').unwrap_or(&content);
        let (dialect, header) = self
            .detect(content)
            .ok_or_else(|| DataError::new("Could not determine delimiter"))?;
        let mut records = parse_rows(content, &dialect)
            .into_iter()
            .filter(|row| !row.is_empty());
        let names = if header {
            records.next().unwrap_or_default()
        } else {
            Vec::new()
        };
        let records: Vec<Vec<String>> = records.collect();
        let width = records
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .max(names.len());
        let columns: Vec<String> = (0..width)
            .map(|i| names.get(i).cloned().unwrap_or_else(|| (i + 1).to_string()))
            .collect();
        let mut rows = Rows::new(columns);
        for record in records {
            rows.push(record.into_iter().map(Value::Str));
        }
        Ok(rows)
    }
}

impl CsvReader {
    /// Stream rows from `reader` instead of reading the text whole, so an
    /// input larger than memory can be read (a [profile](crate::profile)
    /// samples it).
    ///
    /// The dialect and header are decided from the first [`SAMPLE_CHARS`]
    /// characters, as [`read`](Self::read) decides them, and records are
    /// parsed one at a time: a quoted field may span lines, up to
    /// [`MAX_RECORD`] bytes. The columns are decided from the rows the
    /// sniffer read, as [`read`](Self::read) decides them from every row (a
    /// row longer than the header adds columns named by position); a later
    /// row is padded with nulls or cut to fit, and
    /// [`longer_rows`](CsvSource::longer_rows) counts the cut ones. Bytes
    /// that are not UTF-8 are replaced.
    ///
    /// ```
    /// use rich_data::csv::CsvReader;
    /// use rich_data::{RowSource, Value};
    ///
    /// let text = "name,note\nweb,\"two\nlines\"\napi,ok\n";
    /// let mut source = CsvReader::new().source(text.as_bytes()).unwrap();
    /// assert_eq!(source.columns(), ["name", "note"]);
    /// let first = source.next_row().unwrap().unwrap();
    /// assert_eq!(first[1], Value::from("two\nlines"));
    /// source.next_row().unwrap().unwrap();
    /// assert!(source.next_row().is_none());
    /// ```
    pub fn source<R: BufRead>(&self, reader: R) -> Result<CsvSource<R>, DataError> {
        let mut source = CsvSource {
            reader,
            dialect: Dialect::excel(','),
            columns: Vec::new(),
            pending: VecDeque::new(),
            record: String::new(),
            state: State::StartRecord,
            line: 0,
            done: false,
            longer: 0,
        };
        // Whole lines until the sniffer has its sample.
        let mut prefix = String::new();
        while prefix.chars().count() < SAMPLE_CHARS {
            match source.read_line()? {
                Some(line) => prefix.push_str(&line),
                None => break,
            }
        }
        let prefix = universal_newlines(&prefix);
        let prefix = prefix.strip_prefix('\u{feff}').unwrap_or(&prefix);
        let (dialect, header) = self
            .detect(prefix)
            .ok_or_else(|| DataError::new("Could not determine delimiter"))?;
        source.dialect = dialect;
        source.feed(prefix)?;
        // The sample may end inside a quoted field: finish that record.
        if !matches!(source.state, State::StartRecord) {
            source.fill_record()?;
        }
        if source.done {
            source.flush_record();
        }
        let names = if header {
            source.pending.pop_front().unwrap_or_default()
        } else {
            Vec::new()
        };
        let width = source
            .pending
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0)
            .max(names.len());
        source.columns = (0..width)
            .map(|i| names.get(i).cloned().unwrap_or_else(|| (i + 1).to_string()))
            .collect();
        Ok(source)
    }
}

/// The longest record a [`CsvSource`] holds, in bytes (64 MiB): a line, or
/// a quoted field still open, past it is refused rather than read into
/// memory.
pub const MAX_RECORD: usize = 64 << 20;

/// A streaming CSV or TSV [`RowSource`], from [`CsvReader::source`]. Every
/// cell is [`Value::Str`] as written.
#[derive(Debug)]
pub struct CsvSource<R> {
    reader: R,
    dialect: Dialect,
    columns: Vec<String>,
    /// Records parsed and not yet yielded.
    pending: VecDeque<Vec<String>>,
    /// The record being read, while a quoted field spans lines.
    record: String,
    /// Where `record` ends in the reader's state machine.
    state: State,
    line: usize,
    done: bool,
    longer: usize,
}

impl<R: BufRead> CsvSource<R> {
    /// The dialect in use.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// How many rows so far had more cells than there are columns (their
    /// extra cells are dropped).
    pub fn longer_rows(&self) -> usize {
        self.longer
    }

    /// The next line, its `\n` included; `None` at the end.
    fn read_line(&mut self) -> Result<Option<String>, DataError> {
        if self.done {
            return Ok(None);
        }
        let mut buffer = Vec::new();
        let read = (&mut self.reader)
            .take(MAX_RECORD as u64 + 1)
            .read_until(b'\n', &mut buffer)
            .map_err(|e| {
                self.done = true;
                DataError::at(self.line + 1, e.to_string())
            })?;
        if read == 0 {
            self.done = true;
            return Ok(None);
        }
        self.line += 1;
        if buffer.len() > MAX_RECORD {
            self.done = true;
            return Err(DataError::at(
                self.line,
                format!("longer than {MAX_RECORD} bytes"),
            ));
        }
        Ok(Some(String::from_utf8_lossy(&buffer).into_owned()))
    }

    /// Add `text` (universal newlines applied) to the record being read,
    /// parsing each record it completes.
    fn feed(&mut self, text: &str) -> Result<(), DataError> {
        for c in text.chars() {
            self.state = advance(self.state, c, &self.dialect);
            self.record.push(c);
            if matches!(self.state, State::StartRecord) {
                self.flush_record();
            }
        }
        if self.record.len() > MAX_RECORD {
            self.done = true;
            return Err(DataError::at(
                self.line,
                format!("a quoted field runs past {MAX_RECORD} bytes"),
            ));
        }
        Ok(())
    }

    /// Read lines until the record being read ends, or the input does.
    fn fill_record(&mut self) -> Result<(), DataError> {
        while let Some(line) = self.read_line()? {
            self.feed(&universal_newlines(&line))?;
            if matches!(self.state, State::StartRecord) {
                break;
            }
        }
        Ok(())
    }

    /// Parse the record being read into `pending`, dropping blank lines.
    fn flush_record(&mut self) {
        if self.record.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.record);
        self.state = State::StartRecord;
        self.pending.extend(
            parse_rows(&text, &self.dialect)
                .into_iter()
                .filter(|row| !row.is_empty()),
        );
    }
}

impl<R: BufRead> RowSource for CsvSource<R> {
    fn columns(&self) -> &[String] {
        &self.columns
    }

    fn next_row(&mut self) -> Option<Result<Row, DataError>> {
        while self.pending.is_empty() {
            if self.done {
                // A last record without a newline, or a quoted field the
                // input never closed.
                self.flush_record();
                if self.pending.is_empty() {
                    return None;
                }
                break;
            }
            if let Err(error) = self.fill_record() {
                return Some(Err(error));
            }
        }
        let record = self.pending.pop_front()?;
        let width = self.columns.len();
        if record.len() > width {
            self.longer += 1;
        }
        let mut row: Row = record.into_iter().take(width).map(Value::Str).collect();
        row.resize(width, Value::Null);
        Some(Ok(row))
    }
}

/// One step of [`read_rows`]'s state machine without collecting fields: just
/// enough to know where a record ends.
fn advance(state: State, c: char, dialect: &Dialect) -> State {
    match state {
        State::StartRecord | State::StartField => {
            if c == '\n' {
                State::StartRecord
            } else if c == dialect.quotechar {
                State::InQuotedField
            } else if c == dialect.delimiter || (c == ' ' && dialect.skipinitialspace) {
                State::StartField
            } else {
                State::InField
            }
        }
        State::InField => match c {
            '\n' => State::StartRecord,
            c if c == dialect.delimiter => State::StartField,
            _ => State::InField,
        },
        State::InQuotedField => {
            if c != dialect.quotechar {
                State::InQuotedField
            } else if dialect.doublequote {
                State::QuoteInQuotedField
            } else {
                State::InField
            }
        }
        State::QuoteInQuotedField => match c {
            c if c == dialect.quotechar => State::InQuotedField,
            c if c == dialect.delimiter => State::StartField,
            '\n' => State::StartRecord,
            _ => State::InField,
        },
    }
}

/// Python's universal newlines: `\r\n` and a lone `\r` both become `\n`.
fn universal_newlines(content: &str) -> Cow<'_, str> {
    if content.contains('\r') {
        Cow::Owned(content.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(content)
    }
}

/// A CSV dialect: everything `csv.Sniffer` decides and `csv.reader` consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dialect {
    /// The field separator.
    pub delimiter: char,
    /// The quote character.
    pub quotechar: char,
    /// Whether a doubled quote inside a quoted field is one literal quote.
    pub doublequote: bool,
    /// Whether spaces straight after a delimiter are dropped.
    pub skipinitialspace: bool,
}

impl Dialect {
    /// `csv.get_dialect("excel")` (or `"excel-tab"` for a tab): upstream's
    /// fallback for a `.csv`/`.tsv` the sniffer cannot read.
    pub fn excel(delimiter: char) -> Self {
        Dialect {
            delimiter,
            quotechar: '"',
            doublequote: true,
            skipinitialspace: false,
        }
    }
}

/// Whether `c` is a `\w` character for CPython's `re` over `str`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The `[^\w\n"']` class the sniffer accepts as a candidate delimiter.
fn is_delimiter_char(c: char) -> bool {
    !is_word_char(c) && c != '\n' && c != '"' && c != '\''
}

/// The `["']` class the sniffer accepts as a candidate quote character.
fn is_quote_char(c: char) -> bool {
    c == '"' || c == '\''
}

/// One hit from the quote/delimiter scan: the quote, the delimiter bracketing
/// it (absent in the fourth, delimiter-free pattern), and whether a space sat
/// between the two.
struct QuoteHit {
    quote: char,
    delim: Option<char>,
    space: bool,
}

/// Port of `csv.Sniffer.sniff`, restricted to `delimiters` when given (rich-cli
/// passes `",\t|;"`). `None` is CPython's `csv.Error`: no delimiter found.
///
/// `doublequote` is **not** sniffed; it is left at excel's `true`. CPython
/// decides it with a regex that only fires when a `""` pair sits inside a
/// quoted field containing neither the delimiter nor a newline, and when it
/// does not fire `csv.reader` stops unescaping — `"he said ""hi"""` reads back
/// as `he said "hi"""`. Reproducing that can only ever make a well-formed file
/// render worse, and upstream's own fallback (`csv.get_dialect("excel")`, used
/// for every file the sniffer rejects) already sets it true. The divergence is
/// confined to files that use `""` escaping *and* defeat the heuristic, e.g. a
/// `""` inside a multi-line cell: there we unescape and upstream does not.
pub fn sniff(sample: &str, delimiters: Option<&[char]>) -> Option<Dialect> {
    let data: Vec<char> = sample.chars().collect();
    let (quote, mut delimiter, mut skipinitialspace) = guess_quote_and_delimiter(&data, delimiters);
    if delimiter.is_none() {
        let (guessed, spaced) = guess_delimiter(sample, delimiters);
        delimiter = guessed;
        skipinitialspace = spaced;
    }
    Some(Dialect {
        delimiter: delimiter?,
        // `_csv.reader` won't accept an empty quotechar, so upstream falls back
        // to `"` when the scan found no quotes at all.
        quotechar: quote.unwrap_or('"'),
        doublequote: true,
        skipinitialspace,
    })
}

/// Port of `csv.Sniffer._guess_quote_and_delimiter`: look for text enclosed in
/// two identical quotes that are themselves bracketed by the same character.
///
/// CPython uses four backreferencing regexes (`(?P=quote)`, `(?P=delim)`),
/// which no Rust regex engine can express, so they are scanned by hand below.
/// The first pattern that hits anywhere decides; the most frequent quote wins,
/// and so does the most frequent delimiter seen beside it.
fn guess_quote_and_delimiter(
    data: &[char],
    delimiters: Option<&[char]>,
) -> (Option<char>, Option<char>, bool) {
    let hits = quote_hits(data);
    if hits.is_empty() {
        return (None, None, false);
    }

    // Insertion-ordered tallies: Python's `max(dict, key=dict.get)` returns the
    // FIRST key holding the maximum, so the order these were first seen in
    // decides ties.
    let mut quotes: Vec<(char, usize)> = Vec::new();
    let mut delims: Vec<(char, usize)> = Vec::new();
    let mut spaces = 0usize;
    fn bump(table: &mut Vec<(char, usize)>, key: char) {
        match table.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 += 1,
            None => table.push((key, 1)),
        }
    }
    for hit in &hits {
        bump(&mut quotes, hit.quote);
        // The fourth pattern has no delimiter group at all, so it contributes
        // only a quote — upstream `continue`s past both tallies below.
        let Some(delim) = hit.delim else { continue };
        if delimiters.is_none_or(|allowed| allowed.contains(&delim)) {
            bump(&mut delims, delim);
        }
        if hit.space {
            spaces += 1;
        }
    }

    let first_max = |table: &[(char, usize)]| -> Option<(char, usize)> {
        let mut best: Option<(char, usize)> = None;
        for &(key, count) in table {
            if best.is_none_or(|(_, seen)| count > seen) {
                best = Some((key, count));
            }
        }
        best
    };
    let quotechar = first_max(&quotes).map(|(key, _)| key);
    match first_max(&delims) {
        Some((delim, count)) => (quotechar, Some(delim), count == spaces),
        // A single column of quoted data: quotes but nothing bracketing them.
        None => (quotechar, None, false),
    }
}

/// One of CPython's four quote/delimiter patterns, scanned over the sample.
type QuoteScan = fn(&[char]) -> Vec<QuoteHit>;

/// Run CPython's four quote/delimiter patterns in order, returning the hits of
/// the first that matches anything.
fn quote_hits(data: &[char]) -> Vec<QuoteHit> {
    let scans: [QuoteScan; 4] = [
        scan_delim_quote_delim,
        scan_line_quote_delim,
        scan_delim_quote_line,
        scan_line_quote_line,
    ];
    for scan in scans {
        let hits = scan(data);
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}

/// `(?P<delim>[^\w\n"'])(?P<space> ?)(?P<quote>["']).*?(?P=quote)(?P=delim)` —
/// `,"some text",`. The ` ?` needs no backtracking: if the space is there and
/// the next character is not a quote, dropping the space only offers the space
/// itself as the quote, which it is not.
fn scan_delim_quote_delim(data: &[char]) -> Vec<QuoteHit> {
    scan_delim_quote(data, |data, quote, delim, from| {
        let mut k = from;
        while k + 1 < data.len() {
            if data[k] == quote && data[k + 1] == delim {
                return Some(k + 2);
            }
            k += 1;
        }
        None
    })
}

/// `(?P<delim>[^\w\n"'])(?P<space> ?)(?P<quote>["']).*?(?P=quote)(?:$|\n)` —
/// `,"some text"` at the end of a line.
fn scan_delim_quote_line(data: &[char]) -> Vec<QuoteHit> {
    scan_delim_quote(data, |data, quote, _delim, from| {
        let mut k = from;
        while k < data.len() {
            if data[k] == quote && (k + 1 == data.len() || data[k + 1] == '\n') {
                // `$` is zero-width and the engine prefers it, so the match ends
                // at the closing quote either way.
                return Some(k + 1);
            }
            k += 1;
        }
        None
    })
}

/// The shared `<delim><space?><quote> … ` head of patterns one and three;
/// `close` finds the closing quote and reports where the match ends.
fn scan_delim_quote(
    data: &[char],
    close: fn(&[char], char, char, usize) -> Option<usize>,
) -> Vec<QuoteHit> {
    let mut hits = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if is_delimiter_char(data[i]) {
            let delim = data[i];
            let mut j = i + 1;
            let mut space = false;
            if j < data.len() && data[j] == ' ' {
                space = true;
                j += 1;
            }
            if j < data.len() && is_quote_char(data[j]) {
                let quote = data[j];
                if let Some(end) = close(data, quote, delim, j + 1) {
                    hits.push(QuoteHit {
                        quote,
                        delim: Some(delim),
                        space,
                    });
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    hits
}

/// `(?:^|\n)(?P<quote>["']).*?(?P=quote)(?P<delim>[^\w\n"'])(?P<space> ?)` —
/// `"some text",` at the start of a line.
fn scan_line_quote_delim(data: &[char]) -> Vec<QuoteHit> {
    scan_line_quote(data, |data, quote, from| {
        let mut k = from;
        while k + 1 < data.len() {
            if data[k] == quote && is_delimiter_char(data[k + 1]) {
                let space = k + 2 < data.len() && data[k + 2] == ' ';
                return Some((Some(data[k + 1]), space, if space { k + 3 } else { k + 2 }));
            }
            k += 1;
        }
        None
    })
}

/// `(?:^|\n)(?P<quote>["']).*?(?P=quote)(?:$|\n)` — a whole line that is one
/// quoted field, with no delimiter to learn from.
fn scan_line_quote_line(data: &[char]) -> Vec<QuoteHit> {
    scan_line_quote(data, |data, quote, from| {
        let mut k = from;
        while k < data.len() {
            if data[k] == quote && (k + 1 == data.len() || data[k + 1] == '\n') {
                return Some((None, false, k + 1));
            }
            k += 1;
        }
        None
    })
}

/// The shared `(?:^|\n)<quote> … ` head of patterns two and four; `close`
/// reports `(delimiter, saw a space, match end)`.
#[allow(clippy::type_complexity)]
fn scan_line_quote(
    data: &[char],
    close: fn(&[char], char, usize) -> Option<(Option<char>, bool, usize)>,
) -> Vec<QuoteHit> {
    let mut hits = Vec::new();
    let mut i = 0;
    while i < data.len() {
        // `^` is zero-width at a line start; the `\n` branch consumes the
        // newline and puts the quote on the character after it. The engine tries
        // them in that order at each position.
        let mut starts: Vec<usize> = Vec::new();
        if i == 0 || data[i - 1] == '\n' {
            starts.push(i);
        }
        if data[i] == '\n' {
            starts.push(i + 1);
        }
        let mut advanced = false;
        for quote_at in starts {
            if quote_at >= data.len() || !is_quote_char(data[quote_at]) {
                continue;
            }
            let quote = data[quote_at];
            if let Some((delim, space, end)) = close(data, quote, quote_at + 1) {
                hits.push(QuoteHit {
                    quote,
                    delim,
                    space,
                });
                i = end;
                advanced = true;
                break;
            }
        }
        if !advanced {
            i += 1;
        }
    }
    hits
}

/// Port of `csv.Sniffer._guess_delimiter`: the character whose per-line
/// occurrence count is most consistent across the sample wins.
fn guess_delimiter(sample: &str, delimiters: Option<&[char]>) -> (Option<char>, bool) {
    // `filter(None, data.split('\n'))` — blank lines carry no evidence.
    let data: Vec<&str> = sample.split('\n').filter(|line| !line.is_empty()).collect();
    if data.is_empty() {
        return (None, false);
    }
    /// CPython scans `[chr(c) for c in range(127)]` — 7-bit ASCII.
    const ASCII: usize = 127;

    let chunk_length = 10.min(data.len());
    let mut iteration = 0usize;
    // Per character, an insertion-ordered list of (occurrences on a line, how
    // many lines had exactly that many) — upstream's "meta-frequency".
    let mut char_frequency: Vec<Vec<(usize, usize)>> = vec![Vec::new(); ASCII];
    // The winning (frequency, confidence) per character. Confidence can go
    // negative once every competing frequency is subtracted, hence `isize`.
    let mut modes: Vec<Option<(usize, isize)>> = vec![None; ASCII];
    let mut delims: Vec<(char, (usize, isize))> = Vec::new();

    let (mut start, mut end) = (0usize, chunk_length);
    while start < data.len() {
        iteration += 1;
        for line in &data[start..end.min(data.len())] {
            for (code, table) in char_frequency.iter_mut().enumerate() {
                let c = code as u8 as char;
                // Counted even when zero: a character absent from a line is
                // evidence against it being the delimiter.
                let freq = line.matches(c).count();
                match table.iter_mut().find(|(seen, _)| *seen == freq) {
                    Some(entry) => entry.1 += 1,
                    None => table.push((freq, 1)),
                }
            }
        }

        for (code, items) in char_frequency.iter().enumerate() {
            if items.len() == 1 && items[0].0 == 0 {
                continue;
            }
            if items.len() > 1 {
                // The first frequency with the highest count, less the sum of
                // every other count.
                let mut best = 0usize;
                for (index, item) in items.iter().enumerate() {
                    if item.1 > items[best].1 {
                        best = index;
                    }
                }
                let others: usize = items
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != best)
                    .map(|(_, item)| item.1)
                    .sum();
                modes[code] = Some((items[best].0, items[best].1 as isize - others as isize));
            } else if let Some(&(freq, count)) = items.first() {
                modes[code] = Some((freq, count as isize));
            }
        }

        let total = (chunk_length * iteration).min(data.len()) as f64;
        let mut consistency = 1.0f64;
        let threshold = 0.9f64;
        while delims.is_empty() && consistency >= threshold {
            for (code, mode) in modes.iter().enumerate() {
                let Some((freq, count)) = *mode else { continue };
                let c = code as u8 as char;
                if freq > 0
                    && count > 0
                    && (count as f64 / total) >= consistency
                    && delimiters.is_none_or(|allowed| allowed.contains(&c))
                {
                    delims.push((c, (freq, count)));
                }
            }
            consistency -= 0.01;
        }

        if delims.len() == 1 {
            let delim = delims[0].0;
            return (Some(delim), saw_space_after(data[0], delim));
        }

        start = end;
        end += chunk_length;
    }

    if delims.is_empty() {
        return (None, false);
    }
    if delims.len() > 1 {
        for preferred in [',', '\t', ';', ' ', ':'] {
            if delims.iter().any(|(c, _)| *c == preferred) {
                return (Some(preferred), saw_space_after(data[0], preferred));
            }
        }
    }
    // `items = [(v, k) …]; items.sort()` — ordered by the mode, then the char.
    let best = delims
        .iter()
        .max_by_key(|(c, mode)| (*mode, *c))
        .expect("delims is non-empty");
    (Some(best.0), saw_space_after(data[0], best.0))
}

/// Upstream's `skipinitialspace` test: every delimiter on the first line is
/// followed by a space.
fn saw_space_after(line: &str, delimiter: char) -> bool {
    line.matches(delimiter).count() == line.matches(&format!("{delimiter} ")).count()
}

/// What `csv.Sniffer.has_header` decides a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnType {
    /// Every value so far parsed as a Python `complex`.
    Number,
    /// They did not, but all had this many characters.
    Length(usize),
}

/// Whether a column is still in play, and what it has looked like so far.
#[derive(Debug, Clone, Copy)]
enum ColumnVote {
    /// In `columnTypes` with the value `None`: no data row typed it yet.
    Untyped,
    Typed(ColumnType),
    /// `del columnTypes[col]` — the column was inconsistent.
    Dropped,
}

/// Port of `csv.Sniffer.has_header`: if a column is one consistent type in every
/// row *except* the first, the first row is a label. Each column casts one vote.
///
/// The typing is CPython's current one — a single `complex` attempt, falling
/// back to the string's length. The historical `for thisType in [int, float,
/// complex]` loop is NOT equivalent: it makes a column mixing `10` and `9.5`
/// read as inconsistent (int then float), so `price,note / 10,aa / 9.5,bbb`
/// loses its header.
///
/// `None` is CPython's `csv.Error` escaping from the re-sniff, which upstream
/// handles the same way as a failed `sniff`.
pub fn has_header(sample: &str) -> Option<bool> {
    // Note the missing `delimiters` argument: upstream re-sniffs here with the
    // full candidate set, not the four it renders with.
    let dialect = sniff(sample, None)?;
    let mut rows = read_rows(sample, &dialect).into_iter();
    let header = rows.next()?;
    let columns = header.len();
    let mut votes = vec![ColumnVote::Untyped; columns];

    for (checked, row) in rows.enumerate() {
        // An arbitrary cap, "to keep it sane" — the 22nd data row and beyond are
        // never looked at, however inconsistent they are.
        if checked > 20 {
            break;
        }
        if row.len() != columns {
            continue;
        }
        for (col, vote) in votes.iter_mut().enumerate() {
            if matches!(vote, ColumnVote::Dropped) {
                continue;
            }
            let this = if parses_as_complex(&row[col]) {
                ColumnType::Number
            } else {
                ColumnType::Length(row[col].chars().count())
            };
            match vote {
                ColumnVote::Untyped => *vote = ColumnVote::Typed(this),
                ColumnVote::Typed(known) if *known != this => *vote = ColumnVote::Dropped,
                _ => {}
            }
        }
    }

    let mut tally = 0isize;
    for (col, vote) in votes.iter().enumerate() {
        match vote {
            ColumnVote::Dropped => {}
            // `colType` is still `None`, and `None(header[col])` raises
            // TypeError — which counts as "the header does not fit the column".
            ColumnVote::Untyped => tally += 1,
            ColumnVote::Typed(ColumnType::Length(len)) => {
                if header[col].chars().count() == *len {
                    tally -= 1;
                } else {
                    tally += 1;
                }
            }
            ColumnVote::Typed(ColumnType::Number) => {
                if parses_as_complex(&header[col]) {
                    tally -= 1;
                } else {
                    tally += 1;
                }
            }
        }
    }
    Some(tally > 0)
}

/// Whether Python's `complex(value)` would succeed — the type test at the heart
/// of `has_header`.
///
/// `complex()` is much looser than "looks like a number": it takes surrounding
/// whitespace, one level of parentheses, a sign, `inf`/`infinity`/`nan` in any
/// case, an exponent, `_` digit separators and an imaginary `j` suffix. All of
/// that is honoured. The single narrowing is the digit set — Python accepts any
/// Unicode decimal digit, this accepts ASCII — which can only ever move one
/// has-header vote, and only for a column written in non-ASCII numerals.
fn parses_as_complex(value: &str) -> bool {
    let trimmed = value.trim();
    let body = match trimmed.strip_prefix('(') {
        Some(inner) => match inner.strip_suffix(')') {
            Some(inner) => inner.trim(),
            None => return false,
        },
        None if trimmed.ends_with(')') => return false,
        None => trimmed,
    };
    if body.is_empty() {
        return false;
    }
    // `a`, `aj`, or `a±bj`. The split is the first sign that is not an
    // exponent's, so `1e-5` stays one number.
    let chars: Vec<char> = body.chars().collect();
    let split = (1..chars.len())
        .find(|&i| matches!(chars[i], '+' | '-') && !matches!(chars[i - 1], 'e' | 'E'));
    match split {
        Some(index) => {
            let at: usize = chars[..index].iter().map(|c| c.len_utf8()).sum();
            let (real, imaginary) = body.split_at(at);
            is_float_literal(real) && is_imaginary_literal(imaginary)
        }
        None => is_float_literal(body) || is_imaginary_literal(body),
    }
}

/// A Python float literal: optional sign, then `inf`/`infinity`/`nan`, or
/// digits with an optional fraction and exponent.
fn is_float_literal(value: &str) -> bool {
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    if matches!(
        body.to_ascii_lowercase().as_str(),
        "inf" | "infinity" | "nan"
    ) {
        return true;
    }
    let chars: Vec<char> = body.chars().collect();
    let mut index = 0usize;
    let mut digits = 0usize;
    let take_digits = |index: &mut usize, digits: &mut usize| {
        while *index < chars.len() && (chars[*index].is_ascii_digit() || chars[*index] == '_') {
            if chars[*index].is_ascii_digit() {
                *digits += 1;
            }
            *index += 1;
        }
    };
    take_digits(&mut index, &mut digits);
    if index < chars.len() && chars[index] == '.' {
        index += 1;
        take_digits(&mut index, &mut digits);
    }
    if digits == 0 {
        return false;
    }
    if index < chars.len() && (chars[index] == 'e' || chars[index] == 'E') {
        index += 1;
        if index < chars.len() && matches!(chars[index], '+' | '-') {
            index += 1;
        }
        let mut exponent = 0usize;
        take_digits(&mut index, &mut exponent);
        if exponent == 0 {
            return false;
        }
    }
    index == chars.len()
}

/// `<number>j`, or a bare `j`/`+j`/`-j` (which Python reads as ±1j).
fn is_imaginary_literal(value: &str) -> bool {
    let Some(body) = value.strip_suffix(['j', 'J']) else {
        return false;
    };
    matches!(body, "" | "+" | "-") || is_float_literal(body)
}

/// Where [`read_rows`] is within a record. Port of `_csv.c`'s reader states
/// (its `EAT_CRNL` is unreachable here: universal newlines ran first).
#[derive(Clone, Copy, Debug)]
enum State {
    StartRecord,
    StartField,
    InField,
    InQuotedField,
    QuoteInQuotedField,
}

/// Parse `content` into rows of fields with `dialect`. Port of `_csv.reader`'s
/// state machine, minus the escape character (no sniffed dialect sets one) and
/// `strict` (always off, as `QUOTE_MINIMAL` leaves it).
///
/// A blank line yields an **empty** row, as `csv.reader` does — upstream drops
/// those before building the table, where a `[""]` row would have drawn a
/// spurious blank line in it. `\r` never reaches here: the reader has already
/// applied Python's universal newlines.
pub fn read_rows(content: &str, dialect: &Dialect) -> Vec<Vec<String>> {
    // Strip a leading UTF-8 BOM so it doesn't cling to the first header cell.
    parse_rows(content.strip_prefix('\u{feff}').unwrap_or(content), dialect)
}

/// [`read_rows`] without stripping a byte-order mark: a [`CsvSource`] parses
/// each record alone, and only the input's first may lose one.
fn parse_rows(content: &str, dialect: &Dialect) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut state = State::StartRecord;

    for c in content.chars() {
        match state {
            State::StartRecord => {
                if c == '\n' {
                    rows.push(Vec::new());
                } else {
                    state = State::StartField;
                    // Re-dispatch this character as the start of a field.
                    read_csv_char(c, dialect, &mut state, &mut field, &mut row, &mut rows);
                }
            }
            _ => read_csv_char(c, dialect, &mut state, &mut field, &mut row, &mut rows),
        }
    }
    if !matches!(state, State::StartRecord) {
        row.push(std::mem::take(&mut field));
        row.shrink_to_fit();
        rows.push(std::mem::take(&mut row));
    }
    rows
}

/// One character of [`read_rows`]'s state machine, for every state but
/// `StartRecord`.
fn read_csv_char(
    c: char,
    dialect: &Dialect,
    state: &mut State,
    field: &mut String,
    row: &mut Vec<String>,
    rows: &mut Vec<Vec<String>>,
) {
    let end_record = |field: &mut String, row: &mut Vec<String>, rows: &mut Vec<Vec<String>>| {
        row.push(std::mem::take(field));
        // Completed records keep only their fields, not Vec's spare growth slots.
        row.shrink_to_fit();
        rows.push(std::mem::take(row));
    };
    match state {
        State::StartRecord => unreachable!("handled by the caller"),
        State::StartField => {
            if c == '\n' {
                end_record(field, row, rows);
                *state = State::StartRecord;
            } else if c == dialect.quotechar {
                *state = State::InQuotedField;
            } else if c == ' ' && dialect.skipinitialspace {
                // Stay in StartField, swallowing the padding.
            } else if c == dialect.delimiter {
                row.push(std::mem::take(field));
            } else {
                field.push(c);
                *state = State::InField;
            }
        }
        State::InField => {
            if c == '\n' {
                end_record(field, row, rows);
                *state = State::StartRecord;
            } else if c == dialect.delimiter {
                row.push(std::mem::take(field));
                *state = State::StartField;
            } else {
                field.push(c);
            }
        }
        State::InQuotedField => {
            if c == dialect.quotechar {
                *state = if dialect.doublequote {
                    State::QuoteInQuotedField
                } else {
                    // Without doublequote the quote simply ends the quoted part;
                    // anything after it, quotes included, is literal.
                    State::InField
                };
            } else {
                field.push(c);
            }
        }
        State::QuoteInQuotedField => {
            if c == dialect.quotechar {
                field.push(c);
                *state = State::InQuotedField;
            } else if c == dialect.delimiter {
                row.push(std::mem::take(field));
                *state = State::StartField;
            } else if c == '\n' {
                end_record(field, row, rows);
                *state = State::StartRecord;
            } else {
                field.push(c);
                *state = State::InField;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_rows_handles_quotes_and_delimiters() {
        let comma = Dialect::excel(',');
        // Quoted field containing the delimiter, and `""` escaping.
        let rows = read_rows("a,\"b,c\",d\n\"he said \"\"hi\"\"\",2\n", &comma);
        assert_eq!(
            rows,
            vec![
                vec!["a".to_string(), "b,c".to_string(), "d".to_string()],
                vec!["he said \"hi\"".to_string(), "2".to_string()],
            ]
        );
        // No trailing empty row after a final newline (`\r` is gone by now:
        // the reader applies universal newlines first).
        assert_eq!(read_rows("x\ny\n", &comma), vec![vec!["x"], vec!["y"]]);
        // Tab delimiter.
        assert_eq!(
            read_rows("a\tb", &Dialect::excel('\t')),
            vec![vec!["a", "b"]]
        );
        // A leading UTF-8 BOM is stripped, not glued to the first cell.
        assert_eq!(read_rows("\u{feff}a,b", &comma), vec![vec!["a", "b"]]);
        // A blank line is an EMPTY record, as `csv.reader` reports it — upstream
        // then drops it, where a `[""]` row would have drawn a blank table row.
        assert_eq!(
            read_rows("a,b\n\nc,d\n", &comma),
            vec![vec!["a", "b"], vec![], vec!["c", "d"]]
        );
        // `skipinitialspace` eats the padding after a delimiter, but only there.
        let padded = Dialect {
            skipinitialspace: true,
            ..Dialect::excel(',')
        };
        assert_eq!(read_rows("a,  b , c", &padded), vec![vec!["a", "b ", "c"]]);
    }

    #[test]
    fn the_sniffer_finds_the_delimiter_and_the_header() {
        // Every expectation here was read out of CPython's own `csv.Sniffer`.
        let candidates = [',', '\t', '|', ';'];
        let sniffed =
            |sample: &str| sniff(sample, Some(&candidates)).map(|dialect| dialect.delimiter);
        assert_eq!(
            sniffed("name;age;city\nAlice;30;Paris\nBob;25;Lyon\n"),
            Some(';')
        );
        assert_eq!(sniffed("a|b|c\n1|2|3\n4|5|6\n"), Some('|'));
        assert_eq!(sniffed("a\tb\tc\n1\t2\t3\n4\t5\t6\n"), Some('\t'));
        assert_eq!(sniffed("a, b, c\n1, 2, 3\n4, 5, 6\n"), Some(','));
        // A quoted, multi-line cell: only the quote/delimiter scan can find the
        // comma here, because the line counts are inconsistent.
        assert_eq!(
            sniffed("name,bio\nAlice,\"line one\nline two\"\nBob,short\n"),
            Some(',')
        );
        // Ragged rows and a single column defeat the sniffer, exactly as they do
        // upstream — that is what the excel fallback is for.
        assert_eq!(sniffed("a,b,c\n1,2\n3,4,5,6\n"), None);
        assert_eq!(sniffed("alpha\nbeta\ngamma\n"), None);

        assert_eq!(has_header("name,age\nAlice,30\nBob,25\n"), Some(true));
        // A column mixing an int and a float must stay ONE type. The historical
        // `for thisType in [int, float, complex]` loop reads it as inconsistent
        // and loses the header.
        assert_eq!(has_header("price,note\n10,aa\n9.5,bbb\n"), Some(true));
        assert_eq!(has_header("1,2,3\n4,5,6\n7,8,9\n"), Some(false));
    }
    #[test]
    fn the_reader_sniffs_pads_and_names_columns() {
        let rows = CsvReader::new()
            .delimiter(',')
            .header(true)
            .read("a,b\r\n1,2,3\r\n\r\n4\r\n")
            .unwrap();
        assert_eq!(rows.columns(), ["a", "b", "3"]);
        assert_eq!(
            rows.rows()[0],
            [Value::from("1"), Value::from("2"), Value::from("3")]
        );
        assert_eq!(rows.rows()[1], [Value::from("4"), Value::Null, Value::Null]);
        // No header: the first row is data.
        let rows = CsvReader::new().read("1,2,3\n4,5,6\n7,8,9\n").unwrap();
        assert_eq!(rows.columns(), ["1", "2", "3"]);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn the_reader_fails_where_rich_csv_does_unless_given_a_fallback() {
        let error = CsvReader::new().read("alpha\nbeta\n").unwrap_err();
        assert_eq!(error.to_string(), "Could not determine delimiter");
        let rows = CsvReader::new()
            .fallback(',')
            .read("alpha\nbeta\n")
            .unwrap();
        assert_eq!(rows.columns(), ["alpha"]);
        assert_eq!(rows.len(), 1);
        let rows = CsvReader::tsv().read("x\ty\n1\t2\n").unwrap();
        assert_eq!(rows.columns(), ["x", "y"]);
    }

    /// A byte-order mark is stripped from the start of the input only:
    /// streaming used to strip one from the start of every record, so a
    /// cell after a lone `\r` lost it where `read` kept it.
    #[test]
    fn streaming_strips_only_a_leading_byte_order_mark() {
        let text = "\u{feff}a,b\n1,2\r\u{feff}x,y\n\u{feff}3,4\n";
        let read = CsvReader::new().header(true).read(text).unwrap();
        let streamed = CsvReader::new()
            .header(true)
            .source(text.as_bytes())
            .unwrap()
            .collect_rows()
            .unwrap();
        assert_eq!(streamed.columns(), ["a", "b"]);
        assert_eq!(streamed.rows()[1][0], Value::from("\u{feff}x"));
        assert_eq!(streamed.rows(), read.rows());
    }

    /// `read` sniffs the text after a leading byte-order mark, as `source`
    /// does: it sniffed the mark too, so the two could choose different
    /// dialects (and headers) for the same input.
    #[test]
    fn read_and_source_sniff_past_a_byte_order_mark_alike() {
        for text in [
            "\u{feff}\r\nx|y;z\tw w\n",
            "\u{feff}\nname,age\nAda,36\n",
            "\u{feff}ab,c\nxy,1\nzw,2\n",
            // Only the first mark is stripped.
            "\u{feff}\u{feff}ab,c\nxy,1\n",
        ] {
            let read = CsvReader::new().fallback(',').read(text).unwrap();
            let streamed = CsvReader::new()
                .fallback(',')
                .source(text.as_bytes())
                .unwrap()
                .collect_rows()
                .unwrap();
            assert_eq!(streamed.columns(), read.columns(), "{text:?}");
            assert_eq!(streamed.rows(), read.rows(), "{text:?}");
        }
    }
}
