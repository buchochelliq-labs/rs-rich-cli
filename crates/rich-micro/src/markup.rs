//! `:micro:name:` markup (#569, #570).
//!
//! `:micro:name:` is the one syntax (`[micro=…]` would collide with core's
//! markup tags). `\:micro:` escapes it: the backslash is dropped and the rest
//! stays literal. A well-formed token naming no known asset stays literal
//! too, and is reported as a [`Diagnostic`]; so is `:micro:` followed by
//! something that is not a name.
//!
//! Two ways in:
//!
//! - **Before parsing** ([`PreparedMarkup`], [`render_markup`],
//!   [`markup_text`]): tokens are swapped for private-use stand-ins, core
//!   parses the rest (markup tags, `:emoji:` codes), and the stand-ins become
//!   placeholders. This is the robust path: `:micro:rocket::fire:` gets both,
//!   and core's emoji pass never sees a micro token.
//! - **After parsing** ([`expand`], [`MicroTransform`], `MicroExt`): tokens in
//!   a [`Text`]'s plain string are replaced. Use it for text that was never
//!   markup, or in a transform pipeline. Core's emoji pass leaves
//!   `:micro:name:` alone, but it consumes the colons it scans, so a
//!   `:code:` written directly after a token (`:micro:rocket::fire:`) is not
//!   an emoji on this path.

use std::ops::Range;
use std::sync::Arc;

use rich::measure::Measurement;
use rich::text::Span;
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};
use rich_plugin_api::{PluginError, TextTransform};

use crate::model::MicroAsset;
use crate::name::is_valid_name;
use crate::registry::MicroRegistry;
use crate::render::{
    fallback_cells, is_sentinel, placeholder_with, FallbackPreference, MicroMeta, PAD_CELL,
    SENTINEL_BASE, SENTINEL_LAST,
};

/// What opens a token.
pub const TOKEN_PREFIX: &str = ":micro:";

/// Something in the input that was left as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Byte offset of the token in the input (the markup, or the text's
    /// plain string).
    pub offset: usize,
    /// The token as written.
    pub token: String,
    pub kind: DiagnosticKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// A well-formed name no layer has.
    UnknownAsset(String),
    /// `:micro:` not followed by `name:`.
    Malformed,
    /// Not expanded before parsing: the input already holds characters from
    /// U+100000..=U+10FFFD, which the pre-pass reserves as stand-ins (or
    /// more tokens than there are stand-ins). Use [`expand`] on such text.
    Reserved,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            DiagnosticKind::UnknownAsset(name) => {
                write!(f, "unknown micro asset {name:?} at byte {}", self.offset)
            }
            DiagnosticKind::Malformed => write!(
                f,
                "{:?} at byte {} is not a micro asset token (write :micro:name:)",
                self.token, self.offset
            ),
            DiagnosticKind::Reserved => write!(
                f,
                "{:?} at byte {} was left as written: the input holds reserved \
                 private-use characters",
                self.token, self.offset
            ),
        }
    }
}

enum Token {
    /// `\:micro:` — drop the backslash at this offset.
    Escape(usize),
    Asset {
        range: Range<usize>,
        name: String,
    },
    Malformed(Range<usize>),
}

/// Find tokens in `s`. With `skip_tags`, text inside core markup tags
/// (`[bold]`, `[link=…]`) is left alone.
fn scan(s: &str, skip_tags: bool) -> Vec<Token> {
    let bytes = s.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if skip_tags && bytes[at] == b'[' {
            let backslashes = bytes[..at]
                .iter()
                .rev()
                .take_while(|b| **b == b'\\')
                .count();
            let opens_tag = bytes
                .get(at + 1)
                .is_some_and(|b| b.is_ascii_lowercase() || matches!(b, b'#' | b'/' | b'@'));
            if backslashes % 2 == 0 && opens_tag {
                // Core's tag: `[` ... the first `]`, with no `[` between.
                if let Some(close) = s[at + 1..].find([']', '[']) {
                    if bytes[at + 1 + close] == b']' {
                        at += close + 2;
                        continue;
                    }
                }
            }
        }
        if s[at..].starts_with(TOKEN_PREFIX) {
            if at > 0 && bytes[at - 1] == b'\\' {
                tokens.push(Token::Escape(at - 1));
                at += TOKEN_PREFIX.len();
                continue;
            }
            let body = at + TOKEN_PREFIX.len();
            let length = bytes[body..]
                .iter()
                .take_while(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'/')
                })
                .count();
            let name = &s[body..body + length];
            if bytes.get(body + length) == Some(&b':') && is_valid_name(name) {
                tokens.push(Token::Asset {
                    range: at..body + length + 1,
                    name: name.to_string(),
                });
                at = body + length + 1;
            } else {
                tokens.push(Token::Malformed(at..body));
                at = body;
            }
            continue;
        }
        at += s[at..].chars().next().map_or(1, char::len_utf8);
    }
    tokens
}

/// One replacement of a byte range in a text.
struct Edit {
    range: Range<usize>,
    insert: Option<Text>,
}

/// Replace byte ranges of `text` (sorted, disjoint), moving every span with
/// them: a span edge inside a replaced range snaps to the replacement's
/// edge, so a style over a token covers its placeholder. Each inserted
/// text's spans (the placeholder tag) go on top.
fn splice(text: &Text, edits: &[Edit]) -> Text {
    if edits.is_empty() {
        return text.clone();
    }
    let plain = text.plain();
    let mut out = String::with_capacity(plain.len());
    // (old range, new range) per edit.
    let mut moved: Vec<(Range<usize>, Range<usize>)> = Vec::with_capacity(edits.len());
    let mut inserted: Vec<Span> = Vec::new();
    let mut cursor = 0;
    for edit in edits {
        out.push_str(&plain[cursor..edit.range.start]);
        let start = out.len();
        if let Some(insert) = &edit.insert {
            out.push_str(insert.plain());
            for span in insert.spans() {
                inserted.push(Span {
                    start: span.start + start,
                    end: span.end + start,
                    style: span.style.clone(),
                });
            }
        }
        moved.push((edit.range.clone(), start..out.len()));
        cursor = edit.range.end;
    }
    out.push_str(&plain[cursor..]);
    let map = |position: usize, is_end: bool| -> usize {
        let mut delta: isize = 0;
        for (old, new) in &moved {
            if position <= old.start {
                break;
            }
            if position < old.end {
                return if is_end { new.end } else { new.start };
            }
            delta = new.end as isize - old.end as isize;
        }
        (position as isize + delta) as usize
    };
    let mut result = text.blank_copy();
    result.append(&out, None);
    let mut spans: Vec<Span> = text
        .spans()
        .iter()
        .filter_map(|span| {
            let start = map(span.start, false);
            let end = map(span.end, true);
            (end > start).then(|| Span {
                start,
                end,
                style: span.style.clone(),
            })
        })
        .collect();
    spans.extend(inserted);
    result.set_spans(spans);
    result
}

/// Replace every `:micro:name:` token in `text`'s plain string with its
/// placeholder, and drop the backslash of each `\:micro:`. Unknown and
/// malformed tokens stay as written and are reported.
pub fn expand(
    text: &Text,
    registry: &MicroRegistry,
    preference: FallbackPreference,
) -> (Text, Vec<Diagnostic>) {
    let plain = text.plain();
    let mut edits = Vec::new();
    let mut diagnostics = Vec::new();
    for token in scan(plain, false) {
        match token {
            Token::Escape(at) => edits.push(Edit {
                range: at..at + 1,
                insert: None,
            }),
            Token::Asset { range, name } => match registry.resolve(&name) {
                Some(asset) => edits.push(Edit {
                    range,
                    insert: Some(placeholder_with(asset, preference, MicroMeta::new(asset))),
                }),
                None => diagnostics.push(Diagnostic {
                    offset: range.start,
                    token: plain[range].to_string(),
                    kind: DiagnosticKind::UnknownAsset(name),
                }),
            },
            Token::Malformed(range) => diagnostics.push(Diagnostic {
                offset: range.start,
                token: plain[range].to_string(),
                kind: DiagnosticKind::Malformed,
            }),
        }
    }
    (splice(text, &edits), diagnostics)
}

/// Markup with its micro tokens swapped for stand-ins, ready for core to
/// parse; [`finish`](Self::finish) turns the stand-ins into placeholders.
///
/// ```
/// use rich_micro::{FallbackPreference, MicroAsset, MicroRegistry, Layer, PreparedMarkup};
///
/// let mut registry = MicroRegistry::new();
/// registry.add(Layer::Inline, MicroAsset::new("ship", "rocket")?.with_emoji("🚀")?)?;
/// let prepared = PreparedMarkup::new("[b]Go[/b] :micro:ship::fire:", &registry, FallbackPreference::Emoji);
/// let parsed = rich::markup::render_emoji(prepared.markup()).unwrap();
/// let text = prepared.finish(&parsed);
/// assert_eq!(text.plain(), "Go 🚀🔥");
/// # Ok::<(), rich_micro::MicroError>(())
/// ```
#[derive(Clone, Debug)]
pub struct PreparedMarkup {
    markup: String,
    /// Stand-in, and the asset it stands for.
    tokens: Vec<(char, Arc<MicroAsset>)>,
    preference: FallbackPreference,
    diagnostics: Vec<Diagnostic>,
}

impl PreparedMarkup {
    pub fn new(markup: &str, registry: &MicroRegistry, preference: FallbackPreference) -> Self {
        let mut prepared = PreparedMarkup {
            markup: String::with_capacity(markup.len()),
            tokens: Vec::new(),
            preference,
            diagnostics: Vec::new(),
        };
        // Input that already holds stand-in characters, or more tokens than
        // there are stand-ins, is expanded after parsing instead.
        let usable = !markup.chars().any(is_sentinel);
        let mut cursor = 0;
        for token in scan(markup, true) {
            match token {
                Token::Escape(at) => {
                    prepared.markup.push_str(&markup[cursor..at]);
                    cursor = at + 1;
                }
                Token::Asset { range, name } => match registry.resolve(&name) {
                    Some(asset)
                        if usable
                            && prepared.tokens.len()
                                <= (SENTINEL_LAST - SENTINEL_BASE) as usize =>
                    {
                        let sentinel = char::from_u32(SENTINEL_BASE + prepared.tokens.len() as u32)
                            .expect("private-use code point");
                        prepared.markup.push_str(&markup[cursor..range.start]);
                        prepared.markup.push(sentinel);
                        prepared.tokens.push((sentinel, Arc::clone(asset)));
                        cursor = range.end;
                    }
                    Some(_) => prepared.diagnostics.push(Diagnostic {
                        offset: range.start,
                        token: markup[range].to_string(),
                        kind: DiagnosticKind::Reserved,
                    }),
                    None => prepared.diagnostics.push(Diagnostic {
                        offset: range.start,
                        token: markup[range].to_string(),
                        kind: DiagnosticKind::UnknownAsset(name),
                    }),
                },
                Token::Malformed(range) => prepared.diagnostics.push(Diagnostic {
                    offset: range.start,
                    token: markup[range].to_string(),
                    kind: DiagnosticKind::Malformed,
                }),
            }
        }
        prepared.markup.push_str(&markup[cursor..]);
        prepared
    }

    /// The markup to hand to core.
    pub fn markup(&self) -> &str {
        &self.markup
    }

    /// What was left as written.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Replace the stand-ins in `parsed` (core's rendering of
    /// [`markup`](Self::markup)) with placeholders.
    pub fn finish(&self, parsed: &Text) -> Text {
        let plain = parsed.plain();
        let mut edits = Vec::new();
        for (at, c) in plain.char_indices() {
            if !is_sentinel(c) {
                continue;
            }
            if let Some((_, asset)) = self.tokens.iter().find(|(s, _)| *s == c) {
                edits.push(Edit {
                    range: at..at + c.len_utf8(),
                    insert: Some(placeholder_with(
                        asset,
                        self.preference,
                        MicroMeta::new(asset),
                    )),
                });
            }
        }
        splice(parsed, &edits)
    }
}

/// Parse `markup` the way `console` parses printed strings (its emoji and
/// highlighting settings), with micro tokens as placeholders.
pub fn render_markup(
    console: &Console,
    markup: &str,
    registry: &MicroRegistry,
    preference: FallbackPreference,
) -> (Text, Vec<Diagnostic>) {
    let prepared = PreparedMarkup::new(markup, registry, preference);
    let parsed = console.render_str(prepared.markup(), None);
    (prepared.finish(&parsed), prepared.diagnostics)
}

/// Parse `markup` with core's markup parser (and, with `emoji`, its
/// `:emoji:` codes), with micro tokens as placeholders.
pub fn markup_text(
    markup: &str,
    registry: &MicroRegistry,
    emoji: bool,
    preference: FallbackPreference,
) -> rich::errors::Result<(Text, Vec<Diagnostic>)> {
    let prepared = PreparedMarkup::new(markup, registry, preference);
    let source = prepared.markup();
    let parsed = if !source.contains('[') {
        Text::new(if emoji {
            rich::emoji::replace(source)
        } else {
            source.to_string()
        })
    } else if emoji {
        rich::markup::render_emoji(source)?
    } else {
        rich::markup::render(source)?
    };
    Ok((prepared.finish(&parsed), prepared.diagnostics))
}

/// Byte ranges of `source` that Markdown shows as code: fenced blocks
/// (```` ``` ```` or `~~~`, to the matching close or the end) and inline
/// code spans (a run of backticks to the next run of the same length on the
/// same line). A token in one stays as written, as code does.
fn markdown_code(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    // (marker, length, start) of an open fence.
    let mut fence: Option<(u8, usize, usize)> = None;
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        let start = line_start;
        line_start += line.len();
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.bytes().next().filter(|b| matches!(b, b'`' | b'~'));
        let run = marker.map_or(0, |m| trimmed.bytes().take_while(|b| *b == m).count());
        if let Some((m, length, open)) = fence {
            if indent < 4 && marker == Some(m) && run >= length && trimmed[run..].trim().is_empty()
            {
                ranges.push(open..start + line.len());
                fence = None;
            }
            continue;
        }
        if let Some(m) = marker.filter(|_| indent < 4 && run >= 3) {
            fence = Some((m, run, start));
            continue;
        }
        // Inline code spans.
        let bytes = line.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] != b'`' {
                at += 1;
                continue;
            }
            let length = bytes[at..].iter().take_while(|b| **b == b'`').count();
            let mut close = at + length;
            let mut found = None;
            while close < bytes.len() {
                if bytes[close] == b'`' {
                    let n = bytes[close..].iter().take_while(|b| **b == b'`').count();
                    if n == length {
                        found = Some(close + n);
                        break;
                    }
                    close += n;
                } else {
                    close += 1;
                }
            }
            match found {
                Some(end) => {
                    ranges.push(start + at..start + end);
                    at = end;
                }
                None => at += length,
            }
        }
    }
    if let Some((_, _, open)) = fence {
        ranges.push(open..source.len());
    }
    ranges
}

/// A Markdown source with its micro tokens swapped for stand-in cells, ready
/// for a Markdown renderer; [`view`](Self::view) turns the stand-ins in its
/// output into placeholders.
///
/// Markdown has no place for a tag, so each token becomes as many copies of
/// one private-use character as the asset has columns (one cell each), and
/// the layout sizes it as the asset. Tokens in code (fenced blocks and
/// inline spans) stay as written; so does `\:micro:`, whose backslash
/// Markdown drops itself.
///
/// ```
/// use rich::markdown::Markdown;
/// use rich_micro::{FallbackPreference, Layer, MicroAsset, MicroRegistry, PreparedMarkdown};
///
/// let mut registry = MicroRegistry::new();
/// registry.add(Layer::Inline, MicroAsset::new("ship", "rocket")?.with_emoji("🚀")?)?;
/// let prepared = PreparedMarkdown::new(
///     "Go :micro:ship: `:micro:ship:`",
///     &registry,
///     FallbackPreference::Emoji,
/// );
/// let console = rich::Console::builder().width(40).build();
/// let shown = console.render_export(&prepared.view(Markdown::new(prepared.source())));
/// assert!(shown.starts_with("Go 🚀 "), "{shown:?}");
/// assert!(shown.contains(":micro:ship:"), "{shown:?}");
/// # Ok::<(), rich_micro::MicroError>(())
/// ```
#[derive(Clone, Debug)]
pub struct PreparedMarkdown {
    source: String,
    /// Stand-in, the asset, and its occurrence tag.
    tokens: Vec<(char, Arc<MicroAsset>, MicroMeta)>,
    preference: FallbackPreference,
    diagnostics: Vec<Diagnostic>,
}

impl PreparedMarkdown {
    pub fn new(source: &str, registry: &MicroRegistry, preference: FallbackPreference) -> Self {
        let mut prepared = PreparedMarkdown {
            source: String::with_capacity(source.len()),
            tokens: Vec::new(),
            preference,
            diagnostics: Vec::new(),
        };
        if !source.contains(TOKEN_PREFIX) {
            prepared.source.push_str(source);
            return prepared;
        }
        let usable = !source.chars().any(is_sentinel);
        let code = markdown_code(source);
        let in_code = |at: usize| code.iter().any(|range| range.contains(&at));
        let mut cursor = 0;
        for token in scan(source, false) {
            match token {
                // Markdown drops the backslash of `\:` itself.
                Token::Escape(_) => {}
                Token::Asset { range, .. } | Token::Malformed(range) if in_code(range.start) => {}
                Token::Asset { range, name } => match registry.resolve(&name) {
                    Some(asset)
                        if usable
                            && prepared.tokens.len()
                                <= (SENTINEL_LAST - SENTINEL_BASE) as usize =>
                    {
                        let sentinel = char::from_u32(SENTINEL_BASE + prepared.tokens.len() as u32)
                            .expect("private-use code point");
                        prepared.source.push_str(&source[cursor..range.start]);
                        prepared
                            .source
                            .extend(std::iter::repeat_n(sentinel, asset.cols()));
                        prepared
                            .tokens
                            .push((sentinel, Arc::clone(asset), MicroMeta::new(asset)));
                        cursor = range.end;
                    }
                    Some(_) => prepared.diagnostics.push(Diagnostic {
                        offset: range.start,
                        token: source[range].to_string(),
                        kind: DiagnosticKind::Reserved,
                    }),
                    None => prepared.diagnostics.push(Diagnostic {
                        offset: range.start,
                        token: source[range].to_string(),
                        kind: DiagnosticKind::UnknownAsset(name),
                    }),
                },
                Token::Malformed(range) => prepared.diagnostics.push(Diagnostic {
                    offset: range.start,
                    token: source[range].to_string(),
                    kind: DiagnosticKind::Malformed,
                }),
            }
        }
        prepared.source.push_str(&source[cursor..]);
        prepared
    }

    /// The Markdown to render.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Whether any token was swapped.
    pub fn has_assets(&self) -> bool {
        !self.tokens.is_empty()
    }

    /// What was left as written.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// `inner` (a renderer of [`source`](Self::source)) with each run of
    /// stand-ins drawn as its asset's placeholder cells, tagged, so a
    /// [`MicroView`](crate::MicroView) can draw it. A run the layout cut
    /// (wrapped or cropped) shows blank cells.
    pub fn view<R: Renderable>(&self, inner: R) -> MarkdownView<R> {
        MarkdownView {
            inner,
            tokens: self.tokens.clone(),
            preference: self.preference,
        }
    }
}

/// [`PreparedMarkdown::view`].
pub struct MarkdownView<R> {
    inner: R,
    tokens: Vec<(char, Arc<MicroAsset>, MicroMeta)>,
    preference: FallbackPreference,
}

impl<R> MarkdownView<R> {
    /// `segment` with each run of stand-ins replaced by placeholder cells.
    fn restore(&self, segment: Segment, out: &mut Vec<Segment>) {
        if segment.control || !segment.text.chars().any(is_sentinel) {
            out.push(segment);
            return;
        }
        let text = segment.text.as_str();
        let mut plain_start = 0;
        let mut chars = text.char_indices().peekable();
        while let Some((at, c)) = chars.next() {
            if !is_sentinel(c) {
                continue;
            }
            let mut count = 1;
            while chars.peek().is_some_and(|(_, next)| *next == c) {
                chars.next();
                count += 1;
            }
            let end = chars.peek().map_or(text.len(), |(next, _)| *next);
            if at > plain_start {
                out.push(Segment::new(&text[plain_start..at], segment.style.clone()));
            }
            plain_start = end;
            let blank = PAD_CELL.to_string().repeat(count);
            let Some((_, asset, meta)) = self.tokens.iter().find(|(s, _, _)| *s == c) else {
                out.push(Segment::new(blank, segment.style.clone()));
                continue;
            };
            let tag = meta.to_style();
            let style = Some(match &segment.style {
                Some(style) => style.combine(&tag),
                None => tag,
            });
            let cells = if count == meta.cols {
                fallback_cells(asset, self.preference)
            } else {
                blank
            };
            out.push(Segment::new(cells, style));
        }
        if plain_start < text.len() {
            out.push(Segment::new(&text[plain_start..], segment.style.clone()));
        }
    }
}

impl<R: Renderable> Renderable for MarkdownView<R> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let segments = self.inner.rich_render(console, options);
        if self.tokens.is_empty() {
            return segments;
        }
        let mut out = Vec::with_capacity(segments.len());
        for segment in segments {
            self.restore(segment, &mut out);
        }
        out
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.inner.measure(console, options)
    }
}

/// The substitution as a plugin [`TextTransform`] (after parsing; see the
/// module docs). Unknown names stay literal; a transform has no channel for
/// diagnostics, so use [`expand`] to see them.
#[derive(Clone, Debug)]
pub struct MicroTransform {
    registry: Arc<MicroRegistry>,
    preference: FallbackPreference,
}

impl MicroTransform {
    pub fn new(registry: Arc<MicroRegistry>) -> Self {
        MicroTransform {
            registry,
            preference: FallbackPreference::default(),
        }
    }

    pub fn preference(mut self, preference: FallbackPreference) -> Self {
        self.preference = preference;
        self
    }
}

impl TextTransform for MicroTransform {
    fn transform(&self, text: Text) -> Result<Text, PluginError> {
        Ok(expand(&text, &self.registry, self.preference).0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanning() {
        let tokens = scan(
            r"a :micro:x/y: \:micro:z: :micro:Bad: [link=:micro:q:]t[/]",
            true,
        );
        let kinds: Vec<String> = tokens
            .iter()
            .map(|t| match t {
                Token::Escape(at) => format!("esc@{at}"),
                Token::Asset { name, .. } => format!("asset {name}"),
                Token::Malformed(r) => format!("bad@{}", r.start),
            })
            .collect();
        assert_eq!(kinds, ["asset x/y", "esc@14", "bad@25"]);
    }

    #[test]
    fn splice_moves_spans() {
        let mut text = Text::new("ab:micro:x:cd");
        text.stylize("red", 0, 13);
        text.stylize("blue", 5, 7); // inside the token
        text.stylize("green", 11, 13); // after it
        let edits = [Edit {
            range: 2..11,
            insert: Some(Text::new("XY")),
        }];
        let out = splice(&text, &edits);
        assert_eq!(out.plain(), "abXYcd");
        let ranges: Vec<(usize, usize)> = out.spans().iter().map(|s| (s.start, s.end)).collect();
        assert_eq!(ranges, [(0, 6), (2, 4), (4, 6)]);
    }
}
