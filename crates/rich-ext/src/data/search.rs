//! Search a document by key, path glob or value, and render the hits with
//! the matched text highlighted.

use std::ops::Range;

use rich::{Console, ConsoleOptions, Overflow, Renderable, Segment, Style, Text};

use super::explorer::fit_quoted;
use super::{
    escape_controls, scalar_text, segment_text, style, summary, Node, Path, PathSegment, Value,
};
use crate::event::flatten as join_lines;

/// What a hit matched on. With several criteria, the most specific one
/// (value, then key, then path) is reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchKind {
    Key,
    Path,
    Value,
}

/// What to look for. Every criterion given must match (they combine with
/// AND); key and value patterns match substrings, the path pattern is a glob.
///
/// Path globs: `*` is any one segment, `[*]` any index, `**` any number of
/// segments (including none), `[2]` an index, and `*`/`?` inside a key are
/// wildcards: `servers[*].name`, `**.port`, `db_*.host`.
///
/// ```
/// use rich_ext::data::{parse, search, Format, SearchQuery};
///
/// let node = parse(Format::Json, r#"{"servers": [{"name": "a", "port": 80}]}"#).unwrap();
/// let hits = search(&node, &SearchQuery::path("**.port"));
/// assert_eq!(hits[0].path.to_string(), "servers[0].port");
/// let hits = search(&node, &SearchQuery::key("NAME").case_insensitive(true));
/// assert_eq!(hits.len(), 1);
/// ```
#[derive(Clone, Debug, Default)]
pub struct SearchQuery {
    key: Option<String>,
    path: Option<String>,
    value: Option<String>,
    case_insensitive: bool,
}

impl SearchQuery {
    /// Nodes whose own key contains `pattern`.
    pub fn key(pattern: impl Into<String>) -> Self {
        Self::default().and_key(pattern)
    }
    /// Nodes whose path matches the glob `pattern`.
    pub fn path(pattern: impl Into<String>) -> Self {
        Self::default().and_path(pattern)
    }
    /// Scalars whose text contains `pattern`.
    pub fn value(pattern: impl Into<String>) -> Self {
        Self::default().and_value(pattern)
    }
    pub fn and_key(mut self, pattern: impl Into<String>) -> Self {
        self.key = Some(pattern.into());
        self
    }
    pub fn and_path(mut self, pattern: impl Into<String>) -> Self {
        self.path = Some(pattern.into());
        self
    }
    pub fn and_value(mut self, pattern: impl Into<String>) -> Self {
        self.value = Some(pattern.into());
        self
    }
    /// Ignore case in keys, values and path globs.
    pub fn case_insensitive(mut self, yes: bool) -> Self {
        self.case_insensitive = yes;
        self
    }
    fn is_empty(&self) -> bool {
        self.key.is_none() && self.path.is_none() && self.value.is_none()
    }
}

/// One hit.
#[derive(Clone, Debug)]
pub struct SearchMatch<'a> {
    pub path: Path,
    pub node: &'a Node,
    pub matched_on: MatchKind,
}

/// A scalar's searchable text: strings unquoted, others as displayed.
fn value_text(node: &Node) -> Option<String> {
    match &node.value {
        Value::String(s) | Value::DateTime(s) => Some(s.clone()),
        Value::Seq(_) | Value::Map(_) => None,
        other => Some(scalar_text(other, false).0),
    }
}

fn chars_eq(a: char, b: char, ci: bool) -> bool {
    a == b || (ci && a.to_lowercase().eq(b.to_lowercase()))
}

/// The first occurrence of `pattern` in `text`, as a byte range.
pub(crate) fn find(text: &str, pattern: &str, ci: bool) -> Option<Range<usize>> {
    if !ci {
        return text.find(pattern).map(|i| i..i + pattern.len());
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let starts = text.char_indices().map(|(i, _)| i).chain([text.len()]);
    for start in starts {
        let mut end = start;
        let mut chars = text[start..].chars();
        if pattern.iter().all(|p| match chars.next() {
            Some(c) if chars_eq(c, *p, true) => {
                end += c.len_utf8();
                true
            }
            _ => false,
        }) {
            return Some(start..end);
        }
    }
    None
}

/// `*` and `?` wildcards over characters.
pub(crate) fn wildcard(pattern: &str, text: &str, ci: bool) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || (p[pi] != '*' && chars_eq(p[pi], t[ti], ci))) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

#[derive(Clone, Debug, PartialEq)]
enum Glob {
    /// `*`: any one segment.
    Any,
    /// `[*]`: any index.
    AnyIndex,
    /// `**`: any number of segments.
    Deep,
    Key(String),
    Index(usize),
}

fn parse_glob(pattern: &str) -> Vec<Glob> {
    let mut out = Vec::new();
    let chars: Vec<char> = pattern.trim_start_matches('$').chars().collect();
    let mut i = 0;
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<Glob>| {
        if !word.is_empty() {
            out.push(match word.as_str() {
                "**" => Glob::Deep,
                "*" => Glob::Any,
                _ => Glob::Key(std::mem::take(word)),
            });
            word.clear();
        }
    };
    while i < chars.len() {
        match chars[i] {
            '.' => flush(&mut word, &mut out),
            '[' => {
                flush(&mut word, &mut out);
                let close = chars[i..].iter().position(|c| *c == ']').map(|p| i + p);
                let inner: String = chars[i + 1..close.unwrap_or(chars.len())].iter().collect();
                let inner = inner.trim();
                out.push(if inner == "*" {
                    Glob::AnyIndex
                } else if let Ok(index) = inner.parse() {
                    Glob::Index(index)
                } else {
                    let key = serde_json::from_str::<String>(inner)
                        .unwrap_or_else(|_| inner.trim_matches(['\'', '"']).to_string());
                    Glob::Key(key)
                });
                i = close.unwrap_or(chars.len());
            }
            c => word.push(c),
        }
        i += 1;
    }
    flush(&mut word, &mut out);
    out
}

fn glob_match(glob: &[Glob], path: &[PathSegment], ci: bool) -> bool {
    match glob.split_first() {
        None => path.is_empty(),
        Some((Glob::Deep, rest)) => {
            (0..=path.len()).any(|skip| glob_match(rest, &path[skip..], ci))
        }
        Some((first, rest)) => {
            let Some((segment, tail)) = path.split_first() else {
                return false;
            };
            let ok = match (first, segment) {
                (Glob::Any, _) => true,
                (Glob::AnyIndex, PathSegment::Index(_)) => true,
                (Glob::Index(a), PathSegment::Index(b)) => a == b,
                (Glob::Key(pattern), PathSegment::Key(key)) => wildcard(pattern, key, ci),
                _ => false,
            };
            ok && glob_match(rest, tail, ci)
        }
    }
}

/// Every node matching `query`, in document order. The root itself never
/// matches; an empty query matches nothing.
pub fn search<'a>(node: &'a Node, query: &SearchQuery) -> Vec<SearchMatch<'a>> {
    let mut hits = Vec::new();
    if query.is_empty() {
        return hits;
    }
    let ci = query.case_insensitive;
    let glob = query.path.as_deref().map(parse_glob);
    node.walk(|path, node| {
        if path.is_root() {
            return;
        }
        if let Some(glob) = &glob {
            if !glob_match(glob, path.segments(), ci) {
                return;
            }
        }
        if let Some(pattern) = &query.key {
            let Some(PathSegment::Key(key)) = path.last() else {
                return;
            };
            if find(key, pattern, ci).is_none() {
                return;
            }
        }
        if let Some(pattern) = &query.value {
            if !value_text(node).is_some_and(|text| find(&text, pattern, ci).is_some()) {
                return;
            }
        }
        let matched_on = if query.value.is_some() {
            MatchKind::Value
        } else if query.key.is_some() {
            MatchKind::Key
        } else {
            MatchKind::Path
        };
        hits.push(SearchMatch {
            path: path.clone(),
            node,
            matched_on,
        });
    });
    hits
}

/// Search hits, each as its path and value with the matched text
/// highlighted (`data.match`), followed by up to `context` sibling entries
/// from the same container and a match count.
///
/// ```
/// use rich::Console;
/// use rich_ext::data::{parse, Format, SearchQuery, SearchResults};
///
/// let node = parse(Format::Json, r#"{"db": {"host": "x", "port": 5432, "user": "u"}}"#).unwrap();
/// let results = SearchResults::new(&node, &SearchQuery::key("port")).context(1);
/// let out = Console::builder().width(40).build().render_export(&results);
/// assert_eq!(out, "db.port: 5432\n    host: \"x\"\n    … 1 more\n1 match\n");
/// ```
#[derive(Clone, Debug)]
pub struct SearchResults<'a> {
    root: &'a Node,
    hits: Vec<SearchMatch<'a>>,
    query: SearchQuery,
    context: usize,
    max_string: Option<usize>,
}

impl<'a> SearchResults<'a> {
    /// Search `root` for `query`.
    pub fn new(root: &'a Node, query: &SearchQuery) -> Self {
        SearchResults {
            root,
            hits: search(root, query),
            query: query.clone(),
            context: 0,
            max_string: Some(80),
        }
    }
    /// Show up to `siblings` other entries of each hit's container.
    pub fn context(mut self, siblings: usize) -> Self {
        self.context = siblings;
        self
    }
    /// Cut values to this many characters (default 80).
    pub fn max_string(mut self, length: usize) -> Self {
        self.max_string = Some(length);
        self
    }
    /// The hits.
    pub fn matches(&self) -> &[SearchMatch<'a>] {
        &self.hits
    }

    /// `text` with every occurrence of `pattern` (if any) highlighted.
    fn highlighted(&self, text: &mut Text, from: usize, pattern: Option<&str>, hit: &Style) {
        let Some(pattern) = pattern.filter(|p| !p.is_empty()) else {
            return;
        };
        let plain = text.plain().to_string();
        let mut at = from;
        while let Some(range) = find(&plain[at..], pattern, self.query.case_insensitive) {
            let (start, end) = (at + range.start, at + range.end);
            text.stylize(hit.clone(), start, end);
            at = end.max(start + 1);
            while at < plain.len() && !plain.is_char_boundary(at) {
                at += 1;
            }
            if at >= plain.len() {
                break;
            }
        }
    }

    fn value_display(&self, console: &Console, node: &Node) -> Text {
        match &node.value {
            Value::Seq(_) | Value::Map(_) => {
                Text::styled(summary(node), style(console, "data.summary"))
            }
            Value::String(s) => Text::styled(
                fit_quoted(s, self.max_string, None),
                style(console, "json.str"),
            ),
            other => {
                let (text, key) = scalar_text(other, true);
                Text::styled(text, style(console, key))
            }
        }
    }

    fn hit_line(&self, console: &Console, hit: &SearchMatch<'_>) -> Text {
        let highlight = style(console, "data.match");
        let key_style = style(console, "json.key");
        let segments = hit.path.segments();
        let mut text = Text::new("");
        for (i, segment) in segments.iter().enumerate() {
            let shown = escape_controls(&segment_text(segment, i == 0));
            let start = text.plain().len();
            text.append(&shown, Some(key_style.clone().into()));
            if i + 1 == segments.len() && hit.matched_on == MatchKind::Key {
                self.highlighted(&mut text, start, self.query.key.as_deref(), &highlight);
            }
        }
        if hit.matched_on == MatchKind::Path {
            let end = text.plain().len();
            text.stylize(highlight.clone(), 0, end);
        }
        text.append(": ", None);
        let start = text.plain().len();
        let text = text.append_text(&self.value_display(console, hit.node));
        let mut text = text;
        if hit.matched_on == MatchKind::Value {
            self.highlighted(&mut text, start, self.query.value.as_deref(), &highlight);
        }
        text
    }

    fn context_lines(&self, console: &Console, hit: &SearchMatch<'_>) -> Vec<Text> {
        let (Some(parent), Some(own)) = (hit.path.parent(), hit.path.last()) else {
            return Vec::new();
        };
        let Some(container) = self.root.at(&parent) else {
            return Vec::new();
        };
        let siblings: Vec<(String, &Node)> = match &container.value {
            Value::Map(entries) => entries
                .iter()
                .filter(|(k, _)| !matches!(own, PathSegment::Key(own) if own == k))
                .map(|(k, v)| (escape_controls(k), v))
                .collect(),
            Value::Seq(items) => items
                .iter()
                .enumerate()
                .filter(|(i, _)| !matches!(own, PathSegment::Index(own) if own == i))
                .map(|(i, v)| (format!("[{i}]"), v))
                .collect(),
            _ => Vec::new(),
        };
        let mut lines = Vec::new();
        for (key, node) in siblings.iter().take(self.context) {
            let text = Text::new(format!("    {key}: "));
            lines.push(text.append_text(&self.value_display(console, node)));
        }
        if siblings.len() > self.context {
            lines.push(Text::styled(
                format!("    … {} more", siblings.len() - self.context),
                style(console, "data.summary"),
            ));
        }
        lines
    }
}

impl Renderable for SearchResults<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut texts = Vec::new();
        for hit in &self.hits {
            texts.push(self.hit_line(console, hit));
            if self.context > 0 {
                texts.extend(self.context_lines(console, hit));
            }
        }
        let count = match self.hits.len() {
            0 => "No matches".to_string(),
            1 => "1 match".to_string(),
            n => format!("{n} matches"),
        };
        texts.push(Text::styled(count, style(console, "data.summary")));
        let rows = texts
            .into_iter()
            .map(|mut text| {
                text.truncate(options.max_width, Some(Overflow::Ellipsis), false);
                text.render(console.theme(), &Style::new())
            })
            .collect();
        join_lines(rows)
    }
}
