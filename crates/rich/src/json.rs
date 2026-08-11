//! JSON pretty-printing.
//!
//! Port of upstream `rich/json.py`. [`Json`] parses a JSON string and renders it
//! with 2-space indentation (matching Python's `json.dumps(indent=2)`) and the
//! default JSON highlight colors.
//!
//! Non-ASCII strings render as UTF-8, matching upstream (`rich.json.JSON`
//! defaults to `ensure_ascii=False`); object keys keep input order, and a
//! repeated key keeps its first position but its last value — what both
//! `dict` and serde_json's `preserve_order` do. The one remaining caveat is
//! **number formatting** for exotic values — exponent notation (`1e+20`,
//! `1e-07`) and integers beyond i64/u64 can differ from CPython's `repr`.
//! Custom indent/sort options are deferred — see docs/DIVERGENCES.md.
//!
//! ## Why the parser is hand-written
//!
//! Upstream's parser is Python's `json`, which differs from `serde_json` in two
//! ways that this module has to reproduce:
//!
//! * `json.loads` accepts (and `json.dumps(allow_nan=True)` emits) the
//!   non-finite literals `NaN`, `Infinity` and `-Infinity`. `serde_json` has no
//!   `Value` that can hold them and rejects the documents outright.
//! * `serde_json` caps nesting at 128 levels, so a 200-deep document — which
//!   CPython parses without complaint — came back as "invalid JSON".
//!
//! Raising a recursion limit only moves the failure to a stack overflow, so
//! parsing, rendering and *dropping* the tree here are all iterative: nesting
//! depth costs heap, never stack, and no document can crash the process. Scalar
//! *decoding* is still delegated to `serde_json`, so string escapes and number
//! formatting stay exactly as they were.
//!
//! That leaves nesting *unbounded* where CPython eventually raises
//! `RecursionError` — somewhere past 10 000 levels, at a depth that depends on
//! the interpreter's C stack rather than on anything in the format. Reproducing
//! a number that moves between machines would be a made-up divergence of its
//! own, so this accepts every document CPython would and then some.

use std::collections::HashMap;

use crate::console::{Console, ConsoleOptions};
use crate::errors::{Result, RichError};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;

/// A parsed JSON document, rendered with syntax highlighting. Mirrors `rich.json.JSON`.
pub struct Json {
    value: Node,
    styles: JsonStyles,
    /// See [`Json::no_wrap`].
    no_wrap: bool,
}

/// A parsed JSON value.
///
/// Scalars keep the form they will be printed in: numbers are stored already
/// formatted by `serde_json`, strings already decoded (upstream re-encodes them
/// through `json.dumps`, so `"A"` prints as `"A"`).
#[derive(Debug)]
enum Node {
    Null,
    Bool(bool),
    Number(String),
    /// `NaN`, `Infinity` or `-Infinity`. Python's `json` round-trips these;
    /// rich's `JSONHighlighter` has no rule that matches them, so they print
    /// unstyled.
    NonFinite(&'static str),
    Str(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Drop for Node {
    /// Dismantle the tree with an explicit stack.
    ///
    /// The compiler's drop glue recurses once per nesting level, so a document
    /// deep enough to parse (parsing has no depth limit here) would overflow
    /// the stack on the way out — a crash with no error message at all, which
    /// is worse than the rejection this module used to hand out.
    fn drop(&mut self) {
        let mut pending: Vec<Node> = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut node) = pending.pop() {
            take_children(&mut node, &mut pending);
            // `node` drops here with its children already moved out, so this
            // same `drop` runs against an empty container and stops.
        }
    }
}

/// Move a node's children into `out`, leaving the node childless.
fn take_children(node: &mut Node, out: &mut Vec<Node>) {
    match node {
        Node::Array(items) => out.append(items),
        Node::Object(entries) => out.extend(entries.drain(..).map(|(_, value)| value)),
        _ => {}
    }
}

struct JsonStyles {
    brace: Style,
    key: Style,
    string: Style,
    number: Style,
    bool_true: Style,
    bool_false: Style,
    null: Style,
}

impl JsonStyles {
    fn defaults() -> Self {
        let s = |spec: &str| Style::parse(spec).expect("valid built-in style");
        JsonStyles {
            brace: s("bold"),
            key: s("bold blue"),
            string: s("green"),
            number: s("bold cyan"),
            bool_true: s("italic bright_green"),
            bool_false: s("italic bright_red"),
            null: s("italic magenta"),
        }
    }
}

/// One entry of the render work list, consumed newest-first.
enum Task<'a> {
    /// Render this value indented `usize` levels deep.
    Value(&'a Node, usize),
    /// Emit a segment that has already been decided.
    Emit(Segment),
}

impl Json {
    /// Parse `text` as JSON. Returns an error if it is not valid JSON.
    pub fn new(text: &str) -> Result<Self> {
        Ok(Json {
            value: Parser::new(text).parse_document()?,
            styles: JsonStyles::defaults(),
            no_wrap: false,
        })
    }

    /// Keep `rich.json.JSON`'s `self.text.no_wrap = True`, which **crops** each
    /// line at the available width instead of wrapping it.
    ///
    /// Defaults to `false`, because that is what a *top-level*
    /// `Console.print(JSON(...))` produces and that is how this renderable is
    /// normally reached. Upstream's flag really is set, but `Console.print`
    /// never renders the `Text` it is set on: `_collect_renderables` sees a
    /// `Text`, buffers it, and `check_text` hands back
    /// `Text(sep, justify=…, end=…).join(buffered)` — and `Text.join` starts
    /// from `self.blank_copy()`, i.e. from the *separator's* metadata. The
    /// separator has `no_wrap=None`, so the copy that actually renders wraps
    /// with the default `fold` overflow.
    ///
    /// Turn it on whenever the document is **nested** inside another
    /// renderable — a `Panel`, `Padding`, `Styled`, `Constrain`, or rich-cli's
    /// `ForceWidth`. Those are `ConsoleRenderable`s, so `_collect_renderables`
    /// appends them untouched and the inner `Text` reaches
    /// `Text.__rich_console__` with the flag intact; `Text.wrap` then skips
    /// `divide_line` and only `truncate`s to the width.
    ///
    /// The two are not interchangeable. Wrapping keeps every character;
    /// cropping discards what does not fit, which for JSON means the printed
    /// document no longer parses — so the choice has to follow upstream's,
    /// not taste.
    #[must_use]
    pub fn no_wrap(mut self, no_wrap: bool) -> Self {
        self.no_wrap = no_wrap;
        self
    }

    /// Flatten the document into segments, iteratively.
    ///
    /// A recursive walk would descend once per nesting level and overflow the
    /// stack on the deep documents the parser now accepts.
    fn render_value(&self) -> Vec<Segment> {
        let brace = |text: &str| Segment::new(text.to_string(), Some(self.styles.brace.clone()));
        let plain = |text: String| Segment::new(text, None);

        let mut out = Vec::new();
        let mut stack = vec![Task::Value(&self.value, 0)];
        while let Some(task) = stack.pop() {
            let (node, level) = match task {
                Task::Emit(segment) => {
                    out.push(segment);
                    continue;
                }
                Task::Value(node, level) => (node, level),
            };
            match node {
                Node::Null => out.push(Segment::new(
                    "null".to_string(),
                    Some(self.styles.null.clone()),
                )),
                Node::Bool(true) => out.push(Segment::new(
                    "true".to_string(),
                    Some(self.styles.bool_true.clone()),
                )),
                Node::Bool(false) => out.push(Segment::new(
                    "false".to_string(),
                    Some(self.styles.bool_false.clone()),
                )),
                Node::Number(number) => out.push(Segment::new(
                    number.clone(),
                    Some(self.styles.number.clone()),
                )),
                Node::NonFinite(literal) => out.push(plain((*literal).to_string())),
                Node::Str(string) => out.push(Segment::new(
                    quote(string),
                    Some(self.styles.string.clone()),
                )),
                Node::Array(items) => {
                    out.push(brace("["));
                    if items.is_empty() {
                        out.push(brace("]"));
                        continue;
                    }
                    out.push(plain("\n".to_string()));
                    // Pushed back-to-front, so they pop in document order.
                    stack.push(Task::Emit(brace("]")));
                    stack.push(Task::Emit(plain("  ".repeat(level))));
                    let last = items.len() - 1;
                    for (index, item) in items.iter().enumerate().rev() {
                        stack.push(Task::Emit(plain("\n".to_string())));
                        if index != last {
                            stack.push(Task::Emit(plain(",".to_string())));
                        }
                        stack.push(Task::Value(item, level + 1));
                        stack.push(Task::Emit(plain("  ".repeat(level + 1))));
                    }
                }
                Node::Object(entries) => {
                    out.push(brace("{"));
                    if entries.is_empty() {
                        out.push(brace("}"));
                        continue;
                    }
                    out.push(plain("\n".to_string()));
                    stack.push(Task::Emit(brace("}")));
                    stack.push(Task::Emit(plain("  ".repeat(level))));
                    let last = entries.len() - 1;
                    for (index, (key, item)) in entries.iter().enumerate().rev() {
                        stack.push(Task::Emit(plain("\n".to_string())));
                        if index != last {
                            stack.push(Task::Emit(plain(",".to_string())));
                        }
                        stack.push(Task::Value(item, level + 1));
                        stack.push(Task::Emit(plain(": ".to_string())));
                        stack.push(Task::Emit(Segment::new(
                            quote(key),
                            Some(self.styles.key.clone()),
                        )));
                        stack.push(Task::Emit(plain("  ".repeat(level + 1))));
                    }
                }
            }
        }
        out
    }
}

impl Renderable for Json {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let segments = self.render_value();
        if self.no_wrap {
            // `Text.wrap` with `no_wrap` keeps the line whole and then calls
            // `line.truncate(width, overflow="fold")`, which is a crop. See
            // [`Json::no_wrap`] for when upstream gets here.
            Segment::crop_lines(&segments, options.max_width)
        } else {
            // The break must land at a *word* boundary: the joined copy carries
            // no overflow either, so upstream wraps with the default `fold`
            // overflow and only splits mid-word when a single token is wider
            // than the line.
            Segment::fold_lines_words(&segments, options.max_width)
        }
    }
}

/// Serialize a string as a JSON string literal (quoted + escaped).
fn quote(string: &str) -> String {
    serde_json::to_string(string).unwrap_or_else(|_| format!("{string:?}"))
}

/// A container being filled in, held on the parser's explicit stack.
enum Frame {
    Array(Vec<Node>),
    Object {
        entries: Vec<(String, Node)>,
        /// Key -> position in `entries`, so a repeated key overwrites in place
        /// (`{"a": 1, "a": 2}` is one entry) without an O(n^2) rescan. Dropped
        /// with the frame, so only the objects on the current path pay for it.
        seen: HashMap<String, usize>,
        /// The key whose value is currently being parsed.
        key: String,
    },
}

/// A non-recursive JSON reader. Structure is walked with an explicit stack;
/// scalar tokens are handed to `serde_json` for decoding so escapes, number
/// formats and their rejections stay identical to the rest of the workspace.
struct Parser<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Parser {
            src,
            bytes: src.as_bytes(),
            pos: 0,
        }
    }

    fn parse_document(&mut self) -> Result<Node> {
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.pos != self.bytes.len() {
            return Err(self.error("trailing characters"));
        }
        Ok(value)
    }

    /// Parse one value, descending into containers with an explicit stack.
    fn parse_value(&mut self) -> Result<Node> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut node: Node;

        'descend: loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'[') => {
                    self.pos += 1;
                    self.skip_whitespace();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        node = Node::Array(Vec::new());
                    } else {
                        stack.push(Frame::Array(Vec::new()));
                        continue 'descend;
                    }
                }
                Some(b'{') => {
                    self.pos += 1;
                    self.skip_whitespace();
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                        node = Node::Object(Vec::new());
                    } else {
                        let key = self.parse_key()?;
                        stack.push(Frame::Object {
                            entries: Vec::new(),
                            seen: HashMap::new(),
                            key,
                        });
                        continue 'descend;
                    }
                }
                _ => node = self.parse_scalar()?,
            }

            // `node` is finished: hand it to its parent, then close as many
            // containers as end here.
            loop {
                let Some(frame) = stack.last_mut() else {
                    return Ok(node);
                };
                let closing = match frame {
                    Frame::Array(items) => {
                        items.push(node);
                        b']'
                    }
                    Frame::Object { entries, seen, key } => {
                        let key = std::mem::take(key);
                        match seen.get(&key) {
                            Some(&at) => entries[at].1 = node,
                            None => {
                                seen.insert(key.clone(), entries.len());
                                entries.push((key, node));
                            }
                        }
                        b'}'
                    }
                };
                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => {
                        self.pos += 1;
                        if closing == b'}' {
                            let next_key = self.parse_key()?;
                            if let Some(Frame::Object { key, .. }) = stack.last_mut() {
                                *key = next_key;
                            }
                        }
                        continue 'descend;
                    }
                    Some(byte) if byte == closing => {
                        self.pos += 1;
                        node = match stack.pop() {
                            Some(Frame::Array(items)) => Node::Array(items),
                            Some(Frame::Object { entries, .. }) => Node::Object(entries),
                            None => unreachable!("the frame was just borrowed"),
                        };
                    }
                    _ if closing == b']' => return Err(self.error("expected `,` or `]`")),
                    _ => return Err(self.error("expected `,` or `}`")),
                }
            }
        }
    }

    /// Parse `"key" :`, leaving the parser on the value.
    fn parse_key(&mut self) -> Result<String> {
        self.skip_whitespace();
        if self.peek() != Some(b'"') {
            return Err(self.error("key must be a string"));
        }
        let key = self.parse_string()?;
        self.skip_whitespace();
        if self.peek() != Some(b':') {
            return Err(self.error("expected `:`"));
        }
        self.pos += 1;
        Ok(key)
    }

    fn parse_scalar(&mut self) -> Result<Node> {
        match self.peek() {
            Some(b'"') => Ok(Node::Str(self.parse_string()?)),
            Some(b't') => {
                self.expect_literal("true")?;
                Ok(Node::Bool(true))
            }
            Some(b'f') => {
                self.expect_literal("false")?;
                Ok(Node::Bool(false))
            }
            Some(b'n') => {
                self.expect_literal("null")?;
                Ok(Node::Null)
            }
            // Python's json emits and accepts these three (`allow_nan=True` is
            // the default both ways), so upstream renders documents containing
            // them instead of rejecting the file.
            Some(b'N') => {
                self.expect_literal("NaN")?;
                Ok(Node::NonFinite("NaN"))
            }
            Some(b'I') => {
                self.expect_literal("Infinity")?;
                Ok(Node::NonFinite("Infinity"))
            }
            Some(b'-') if self.src[self.pos..].starts_with("-Infinity") => {
                self.pos += "-Infinity".len();
                Ok(Node::NonFinite("-Infinity"))
            }
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(_) => Err(self.error("expected value")),
            None => Err(self.error("EOF while parsing a value")),
        }
    }

    /// Read a string token and decode it with `serde_json`, so escapes, lone
    /// surrogates and raw control characters behave exactly as before.
    fn parse_string(&mut self) -> Result<String> {
        let start = self.pos;
        let mut end = self.pos + 1;
        loop {
            match self.bytes.get(end) {
                None => return Err(self.error_at(self.bytes.len(), "EOF while parsing a string")),
                Some(b'"') => {
                    end += 1;
                    break;
                }
                Some(b'\\') => {
                    end += 1;
                    // Step over the escaped character whole. A multi-byte
                    // character after a backslash is invalid JSON, but `end`
                    // must still land on a UTF-8 boundary or slicing panics
                    // before `serde_json` gets to reject it.
                    match self.src[end..].chars().next() {
                        Some(ch) => end += ch.len_utf8(),
                        None => {
                            return Err(
                                self.error_at(self.bytes.len(), "EOF while parsing a string")
                            )
                        }
                    }
                }
                // Continuation bytes are never `"` or `\`, so scanning byte by
                // byte cannot mistake one for a delimiter.
                Some(_) => end += 1,
            }
        }
        let decoded: String = serde_json::from_str(&self.src[start..end])
            .map_err(|error| self.error_at(start, &describe(&error)))?;
        self.pos = end;
        Ok(decoded)
    }

    /// Read a number token and let `serde_json` decide whether it is one — this
    /// keeps `float_roundtrip` parsing and `Number`'s formatting.
    fn parse_number(&mut self) -> Result<Node> {
        let start = self.pos;
        let mut end = self.pos;
        while matches!(
            self.bytes.get(end),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            end += 1;
        }
        let number: serde_json::Number = serde_json::from_str(&self.src[start..end])
            .map_err(|error| self.error_at(start, &describe(&error)))?;
        self.pos = end;
        Ok(Node::Number(number.to_string()))
    }

    fn expect_literal(&mut self, literal: &str) -> Result<()> {
        if self.src[self.pos..].starts_with(literal) {
            self.pos += literal.len();
            Ok(())
        } else {
            Err(self.error("expected value"))
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn error(&self, message: &str) -> RichError {
        self.error_at(self.pos, message)
    }

    fn error_at(&self, pos: usize, message: &str) -> RichError {
        let (line, column) = self.line_column(pos);
        RichError::Json(format!("{message} at line {line} column {column}"))
    }

    fn line_column(&self, pos: usize) -> (usize, usize) {
        let mut pos = pos.min(self.src.len());
        while !self.src.is_char_boundary(pos) {
            pos -= 1;
        }
        let before = &self.src[..pos];
        let line = 1 + before.matches('\n').count();
        let column = before
            .rsplit('\n')
            .next()
            .map_or(0, |tail| tail.chars().count())
            + 1;
        (line, column)
    }
}

/// `serde_json`'s message without its own `at line … column …` suffix, which
/// counts from the start of the token slice rather than the document.
fn describe(error: &serde_json::Error) -> String {
    let text = error.to_string();
    match text.find(" at line ") {
        Some(at) => text[..at].to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;

    fn render(text: &str) -> String {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .build();
        console.render_to_string(&Json::new(text).unwrap())
    }

    fn render_plain(text: &str, width: usize) -> String {
        let console = Console::builder().width(width).no_color(true).build();
        console.render_to_string(&Json::new(text).expect("valid json"))
    }

    #[test]
    fn empty_collections_stay_inline() {
        assert_eq!(render("{}"), "\x1b[1m{\x1b[0m\x1b[1m}\x1b[0m");
        assert_eq!(render("[]"), "\x1b[1m[\x1b[0m\x1b[1m]\x1b[0m");
    }

    #[test]
    fn object_with_scalars() {
        assert_eq!(
            render(r#"{"ok": false}"#),
            "\x1b[1m{\x1b[0m\n  \x1b[1;34m\"ok\"\x1b[0m: \x1b[3;91mfalse\x1b[0m\n\x1b[1m}\x1b[0m"
        );
    }

    #[test]
    fn invalid_json_errors() {
        assert!(Json::new("{not json}").is_err());
    }

    #[test]
    fn non_ascii_stays_utf8_in_input_order() {
        // Upstream's JSON defaults to ensure_ascii=False, so accented/symbol
        // characters render as UTF-8 (not \uXXXX), and keys keep input order.
        // (Byte-parity is guaranteed by the `json_unicode` golden.)
        let out = render("{\"name\": \"caf\u{e9}\", \"emoji\": \"\u{2764}\"}");
        assert!(out.contains("caf\u{e9}"), "café stays UTF-8: {out:?}");
        assert!(out.contains('\u{2764}'), "heart stays UTF-8");
        let name_at = out.find("name").expect("name key present");
        let emoji_at = out.find("emoji").expect("emoji key present");
        assert!(name_at < emoji_at, "keys keep input order");
    }

    #[test]
    fn a_long_value_is_wrapped_rather_than_cropped() {
        // A long string value used to be cut mid-token, so the printed document
        // was missing data -- and, for JSON, no longer parseable -- at exit 0.
        let payload = format!("{{\"k\": \"{}\"}}", "y".repeat(120));
        let out = render_plain(&payload, 40);
        assert_eq!(
            out.matches('y').count(),
            120,
            "characters were dropped:
{out}"
        );
    }

    /// Upstream wraps the JSON at word boundaries: `JSON.text` asks for
    /// `no_wrap`, but `Console.print` re-joins it through `Text(sep).join(...)`
    /// and the joined copy carries neither the flag nor an overflow, so the
    /// default `fold` word wrap applies. Character folding split words
    /// (`over t` / `he lazy`), which no upstream output ever shows.
    ///
    /// Captured from rich 15.0.0:
    /// `Console(width=40).print(JSON(...))`.
    #[test]
    fn wrapping_breaks_at_word_boundaries() {
        let payload = r#"{"k": "the quick brown fox jumps over the lazy dog and keeps running for a very long time indeed"}"#;
        assert_eq!(
            render_plain(payload, 40),
            "{\n  \"k\": \"the quick brown fox jumps over \n\
             the lazy dog and keeps running for a \n\
             very long time indeed\"\n}"
        );
    }

    /// Nested inside another renderable, `JSON.text.no_wrap` survives and each
    /// line is **cropped** at the width rather than wrapped — see
    /// [`Json::no_wrap`]. Wrapping here instead was silent content loss: with
    /// `rich -j doc.json -w 120` on an 80-column console the document was laid
    /// out at 120 and then cropped to 80 by `Console.print`, so whole runs
    /// vanished and the surviving text read as if it were contiguous.
    ///
    /// Captured from rich-cli 1.8.1 driven by rich 15.0.0:
    /// `COLUMNS=80 rich -j long.json -w 40`.
    #[test]
    fn a_nested_document_is_cropped_rather_than_wrapped() {
        let payload = r#"{"k": "the quick brown fox jumps over the lazy dog and keeps running for a very long time indeed"}"#;
        let console = Console::builder().width(40).no_color(true).build();
        let json = Json::new(payload).expect("valid json").no_wrap(true);
        assert_eq!(
            console.render_to_string(&json),
            "{\n  \"k\": \"the quick brown fox jumps over t\n}"
        );

        // …and the wrap is still the default, because a bare
        // `Console.print(JSON(...))` loses the flag in `Text.join`.
        assert_eq!(
            render_plain(payload, 40),
            "{\n  \"k\": \"the quick brown fox jumps over \n\
             the lazy dog and keeps running for a \n\
             very long time indeed\"\n}"
        );
    }

    /// A crop must not cut a double-width character in half: `set_cell_size`
    /// drops the straddling character and the line comes out one cell short,
    /// never one cell over.
    #[test]
    fn cropping_never_splits_a_wide_character() {
        let console = Console::builder().width(12).no_color(true).build();
        let json = Json::new("{\"k\": \"\u{1f306}\u{1f306}\u{1f306}\"}")
            .expect("valid json")
            .no_wrap(true);
        for line in console.render_to_string(&json).lines() {
            assert!(
                crate::cells::cell_len(line) <= 12,
                "line {line:?} overflows the crop"
            );
        }
    }

    /// serde_json's default float parser takes a fast path that can land 1 ULP
    /// from the value in the file, so the rendered number parsed back to a
    /// *different* double. The `float_roundtrip` feature makes parsing exact.
    #[test]
    fn floats_round_trip_exactly() {
        for literal in [
            "-938371.9565467801",
            "0.1",
            "1.7976931348623157e308",
            "5e-324",
            "3.141592653589793",
        ] {
            let out = render_plain(&format!("{{\"v\": {literal}}}"), 120);
            let rendered: String = out
                .split(':')
                .nth(1)
                .expect("a value after the key")
                .trim()
                .trim_end_matches(['}', ' ', '\n'])
                .to_string();
            let want: f64 = literal.parse().expect("literal parses");
            let got: f64 = rendered
                .parse()
                .unwrap_or_else(|_| panic!("rendered {rendered:?}"));
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "{literal} rendered as {rendered} — a different double"
            );
        }
    }

    /// serde_json stops at 128 levels, so a 200-deep document — which CPython
    /// parses without complaint — was reported as invalid JSON and the CLI
    /// exited 1 on a file upstream renders.
    #[test]
    fn deep_nesting_is_not_rejected() {
        for depth in [128, 129, 200, 1000] {
            let payload = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
            let json = Json::new(&payload)
                .unwrap_or_else(|error| panic!("depth {depth} rejected: {error}"));
            let console = Console::builder()
                .width(4 * depth + 8)
                .no_color(true)
                .build();
            let out = console.render_to_string(&json);
            assert_eq!(
                out.matches('[').count(),
                depth,
                "depth {depth} did not render every level"
            );
        }
    }

    /// Parsing and dropping must cost heap, not stack: a recursive parser (or
    /// the compiler's recursive drop glue) turns a deep document into a stack
    /// overflow, which kills the process without even an error message.
    #[test]
    fn very_deep_nesting_does_not_overflow_the_stack() {
        let depth = 100_000;
        let payload = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
        let json = Json::new(&payload).expect("deep document parses");
        drop(json);
    }

    /// Python's json accepts and emits `NaN` / `Infinity` / `-Infinity`
    /// (`allow_nan=True` is the default), and rich's JSONHighlighter has no
    /// rule that matches them, so upstream prints them *unstyled*. serde_json
    /// rejected the whole document.
    ///
    /// Captured from rich 15.0.0:
    /// `Console(width=40, force_terminal=True).print(JSON(...))`.
    #[test]
    fn non_finite_numbers_render_unstyled() {
        assert_eq!(
            render(r#"{"a": NaN, "b": Infinity, "c": -Infinity, "d": 1.5}"#),
            "\x1b[1m{\x1b[0m\n  \x1b[1;34m\"a\"\x1b[0m: NaN,\n  \x1b[1;34m\"b\"\x1b[0m: \
             Infinity,\n  \x1b[1;34m\"c\"\x1b[0m: -Infinity,\n  \x1b[1;34m\"d\"\x1b[0m: \
             \x1b[1;36m1.5\x1b[0m\n\x1b[1m}\x1b[0m"
        );
        // Python spells them with those exact capitalisations and nothing else.
        for rejected in [r#"{"a": nan}"#, r#"{"a": inf}"#, r#"{"a": -inf}"#] {
            assert!(Json::new(rejected).is_err(), "{rejected} should not parse");
        }
    }

    /// The hand-written reader must accept and reject exactly what serde_json
    /// does — apart from the three Python constants it exists to add.
    #[test]
    fn acceptance_matches_serde_json() {
        let samples = [
            "{}",
            "[]",
            "  {\t\"a\" :\n1 }  ",
            r#"{"a": 1, "a": 2}"#,
            r#"{"a": [1, {"b": null}], "c": "x"}"#,
            "0",
            "-0",
            "0.0",
            "1e10",
            "1E+10",
            "1e-7",
            "1e999",
            "12345678901234567890",
            "-12345678901234567890123456789012345",
            "01",
            "1.",
            ".1",
            "+1",
            "1e",
            "-",
            "--1",
            "-i",
            "-Inf",
            "Infinit",
            "NAN",
            "1 2",
            "",
            "   ",
            "{",
            "[",
            "]",
            "}",
            "[,]",
            "[1,]",
            r#"{"a": 1,}"#,
            r#"{a: 1}"#,
            r#"{'a': 1}"#,
            r#"{"a" 1}"#,
            "[1 2]",
            "truex",
            "tru",
            "nul",
            r#""unterminated"#,
            r#""\q""#,
            r#""é""#,
            r#""😀""#,
            r#""\ud800""#,
            "\"raw\nnewline\"",
            r#""café ❤""#,
            "\"\u{e9}\\\"",
            "\u{feff}{}",
            "[[[[1]]]]",
        ];
        for sample in samples {
            let ours = Json::new(sample).is_ok();
            let theirs = serde_json::from_str::<serde_json::Value>(sample).is_ok();
            assert_eq!(ours, theirs, "disagreed about {sample:?}");
        }
    }

    /// And the tree it builds must be the tree serde_json would have built.
    /// `serde_json::to_string_pretty` happens to use the very layout upstream's
    /// `json.dumps(indent=2)` does, so it doubles as a reference dump: key
    /// order, repeated-key collapsing, string escaping and number formatting
    /// all have to agree.
    #[test]
    fn the_parsed_tree_matches_serde_json() {
        let samples = [
            r#"{"name": "Alice", "age": 30, "admin": true, "tags": ["a", "b"], "meta": null}"#,
            r#"{"a": 1, "b": 2, "a": 3}"#,
            r#"{"a": {"b": {"c": [1, [], {}, [[2]]]}}}"#,
            r#"{"k": "A\t\"x\"A\\\/é"}"#,
            r#"[0, -0.5, 1e10, 1E+10, 1e-7, 12345678901234567890, 1.7976931348623157e308]"#,
            r#"{"café": "❤", "": ""}"#,
            "[]",
            "{}",
            "\"top level\"",
            "1234",
        ];
        for sample in samples {
            let reference: serde_json::Value =
                serde_json::from_str(sample).expect("sample is valid JSON");
            assert_eq!(
                render_plain(sample, 10_000),
                serde_json::to_string_pretty(&reference).expect("value re-serialises"),
                "diverged on {sample}"
            );
        }
    }

    /// A repeated key collapses to one entry — first position, last value —
    /// which is what both `dict` and serde_json's `preserve_order` produce.
    #[test]
    fn a_repeated_key_keeps_its_position_and_last_value() {
        assert_eq!(
            render_plain(r#"{"a": 1, "b": 2, "a": 3}"#, 40),
            "{\n  \"a\": 3,\n  \"b\": 2\n}"
        );
    }

    /// Escapes are decoded and re-encoded, because upstream re-serialises the
    /// parsed data with `json.dumps`.
    #[test]
    fn escapes_are_re_encoded_like_dumps() {
        assert_eq!(
            render_plain(r#"{"k": "A\t\"x\""}"#, 60),
            "{\n  \"k\": \"A\\t\\\"x\\\"\"\n}"
        );
    }
}
