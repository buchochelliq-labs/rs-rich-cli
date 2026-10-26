//! Inline markup: docutils' `Inliner`, for the markup `rich-rst` renders.
//!
//! The recognition rules are docutils': a start-string follows whitespace,
//! an opening bracket or a delimiter and is followed by non-whitespace; an
//! end-string follows non-whitespace and is followed by whitespace, a closing
//! bracket or punctuation. A start-string with no end-string is plain text,
//! as docutils' `problematic` node shows it. Backslash escapes are docutils'
//! too: an escaped character is never markup, and the backslash disappears
//! everywhere but inside inline literals.

use std::collections::HashMap;

use super::doctree::{Element, Node};

/// Inline markup as the inliner finds it, before it becomes nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Emphasis(String),
    Strong(String),
    /// A literal, with its classes (`:code:` adds `code`).
    Literal(String, Vec<String>),
    TitleReference(String),
    Subscript(String),
    Superscript(String),
    Math(String),
    /// An abbreviation and its explanation.
    Abbreviation(String, String),
    Acronym(String),
    /// A hyperlink: `` `text <uri>`_ ``, a standalone URI, or `name_` (a
    /// `refname` until the transforms resolve it).
    Reference {
        text: String,
        uri: Option<String>,
        name: Option<String>,
        anonymous: bool,
        /// Its source, shown when it resolves to nothing.
        raw: String,
    },
    /// `[1]_` (a label), `[#]_` or `[#name]_` (auto-numbered) and `[*]_`.
    FootnoteReference {
        label: Option<String>,
        auto: Option<String>,
        name: Option<String>,
    },
    CitationReference(String),
    /// `|name|`, and `|name|_` (a reference around it) or `|name|__`.
    SubstitutionReference {
        name: String,
        refend: usize,
    },
    /// `` _`text` ``: an inline target, whose text the visitor skips.
    Target(String),
    /// Markup docutils cannot read, shown as its source.
    Problematic(String),
}

/// Link names compare lowercased with whitespace collapsed.
pub(super) fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The inline markup as document nodes.
pub(super) fn inline_nodes(inlines: Vec<Inline>) -> Vec<Node> {
    inlines.into_iter().map(inline_node).collect()
}

fn inline_node(inline: Inline) -> Node {
    let text = |tag: &'static str, text: String| Element::text(tag, text).into();
    match inline {
        Inline::Text(text) => Node::Text(text),
        Inline::Emphasis(t) => text("emphasis", t),
        Inline::Strong(t) => text("strong", t),
        Inline::Literal(t, classes) => {
            let mut literal = Element::text("literal", t);
            literal.classes = classes;
            literal.into()
        }
        Inline::TitleReference(t) => text("title_reference", t),
        Inline::Subscript(t) => text("subscript", t),
        Inline::Superscript(t) => text("superscript", t),
        Inline::Math(t) => text("math", t),
        Inline::Abbreviation(t, explanation) => {
            let mut node = Element::text("abbreviation", t);
            if !explanation.is_empty() {
                node = node.with("explanation", explanation);
            }
            node.into()
        }
        Inline::Acronym(t) => text("acronym", t),
        Inline::Reference {
            text,
            uri,
            name,
            anonymous,
            raw,
        } => {
            let mut reference = Element::text("reference", text.clone()).with("name", text);
            match (uri, name) {
                (Some(uri), name) => {
                    reference = reference.with("refuri", uri);
                    // A named embedded URI is also a target for its name.
                    if let Some(name) = name {
                        reference = reference.with("__target", name);
                    }
                }
                (None, Some(name)) => reference = reference.with("refname", name),
                (None, None) => {}
            }
            if anonymous {
                reference = reference.with("anonymous", "1");
            }
            if !raw.is_empty() {
                reference = reference.with("__raw", raw);
            }
            reference.into()
        }
        Inline::FootnoteReference { label, auto, name } => {
            let mut node = Element::new("footnote_reference");
            if let Some(label) = label {
                node = node.with("refname", label.clone()).child(Node::Text(label));
            }
            if let Some(auto) = auto {
                node = node.with("auto", auto);
            }
            if let Some(name) = name {
                node = node.with("refname", name);
            }
            node.into()
        }
        Inline::CitationReference(label) => Element::text("citation_reference", label.clone())
            .with("refname", normalize_name(&label))
            .into(),
        Inline::SubstitutionReference { name, refend } => {
            let reference = Element::text("substitution_reference", name.clone())
                .with("refname", normalize_name(&name));
            match refend {
                0 => reference.into(),
                1 => Element::new("reference")
                    .with("refname", normalize_name(&name))
                    .child(reference)
                    .into(),
                _ => Element::new("reference")
                    .with("anonymous", "1")
                    .child(reference)
                    .into(),
            }
        }
        Inline::Target(t) => {
            let mut target = Element::text("target", t.clone());
            target.names = vec![normalize_name(&t)];
            target.into()
        }
        Inline::Problematic(t) => text("problematic", t),
    }
}

/// Stands in for an escaping backslash, as docutils' `escape2null` does.
const ESCAPE: char = '\0';

const OPENERS: &str = "\"'(<[{\u{f3a}\u{f3c}\u{169b}\u{2045}\u{207d}\u{208d}\u{2329}\u{2768}\u{276a}\u{276c}\u{276e}\u{2770}\u{2772}\u{2774}\u{27c5}\u{27e6}\u{27e8}\u{27ea}\u{27ec}\u{27ee}\u{2983}\u{2985}\u{2987}\u{2989}\u{298b}\u{298d}\u{298f}\u{2991}\u{2993}\u{2995}\u{2997}\u{29d8}\u{29da}\u{ab}\u{2018}\u{201c}\u{2039}\u{201a}\u{201e}";
const CLOSERS: &str = "\"')>]}\u{f3b}\u{f3d}\u{169c}\u{2046}\u{207e}\u{208e}\u{232a}\u{2769}\u{276b}\u{276d}\u{276f}\u{2771}\u{2773}\u{2775}\u{27c6}\u{27e7}\u{27e9}\u{27eb}\u{27ed}\u{27ef}\u{2984}\u{2986}\u{2988}\u{298a}\u{298c}\u{298e}\u{2990}\u{2992}\u{2994}\u{2996}\u{2998}\u{29d9}\u{29db}\u{bb}\u{2019}\u{201d}\u{203a}";
const CLOSING_DELIMITERS: &str = "\\.,;!?";

/// docutils' `delimiters`: the ASCII ones and the common Unicode dashes and
/// punctuation.
fn is_delimiter(c: char) -> bool {
    matches!(
        c,
        '\\' | '-' | '/' | ':' | '\u{58a}' | '\u{a1}' | '\u{b7}' | '\u{bf}'
    ) || ('\u{2010}'..='\u{2027}').contains(&c)
        || ('\u{2030}'..='\u{2043}').contains(&c)
        || ('\u{3001}'..='\u{3003}').contains(&c)
}

/// Whether markup may start after `prev`.
fn start_prefix(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c.is_whitespace() || OPENERS.contains(c) || is_delimiter(c),
    }
}

/// Whether markup may end before `next`.
fn end_suffix(next: Option<char>) -> bool {
    match next {
        None => true,
        Some(c) => {
            c.is_whitespace()
                || c == ESCAPE
                || CLOSING_DELIMITERS.contains(c)
                || is_delimiter(c)
                || CLOSERS.contains(c)
        }
    }
}

/// docutils' `quoted_start`: markup between matching quotes is not markup.
fn quoted(prev: Option<char>, next: Option<char>) -> bool {
    let (Some(prev), Some(next)) = (prev, next) else {
        return prev.is_some();
    };
    let Some(index) = OPENERS.chars().position(|c| c == prev) else {
        return false;
    };
    // The ASCII and bracket openers pair with the closer at the same index;
    // the quotation marks also pair with themselves and their mirrors.
    CLOSERS.chars().nth(index) == Some(next)
        || (prev == next && "\"'".contains(prev))
        || matches!(
            (prev, next),
            ('\u{ab}', '\u{bb}')
                | ('\u{2018}', '\u{2019}')
                | ('\u{201c}', '\u{201d}')
                | ('\u{2039}', '\u{203a}')
                | ('\u{201a}', '\u{2018}' | '\u{2019}')
                | ('\u{201e}', '\u{201c}' | '\u{201d}')
        )
}

/// `escape2null`: each backslash becomes [`ESCAPE`], keeping the character
/// it escapes.
fn escape_to_null(text: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(ESCAPE);
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `unescape`: an escaped space or newline disappears with its backslash;
/// any other backslash just disappears.
fn unescape(chars: &[char]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ESCAPE {
            if matches!(chars.get(i + 1), Some(' ' | '\n')) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `unescape(restore_backslashes=True)`, for inline literals.
fn restore(chars: &[char]) -> String {
    chars
        .iter()
        .map(|&c| if c == ESCAPE { '\\' } else { c })
        .collect()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

/// docutils' `simplename` at `start`: words joined by single `-._+:`.
/// Returns where it ends.
fn simplename(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    if !chars.get(i).copied().is_some_and(is_word) {
        return None;
    }
    loop {
        while chars.get(i).copied().is_some_and(is_word) {
            i += 1;
        }
        let joined = chars.get(i).is_some_and(|c| "-._+:".contains(*c))
            && chars.get(i + 1).copied().is_some_and(is_word);
        if !joined {
            return Some(i);
        }
        i += 1;
    }
}

/// Parse a paragraph's text into inline nodes.
pub(super) fn parse_inline(text: &str) -> Vec<Inline> {
    let chars = escape_to_null(text);
    let mut misses = Misses::default();
    let mut inlines = Vec::new();
    let mut plain_start = 0;
    let mut i = 0;
    while i < chars.len() {
        let prev = i.checked_sub(1).map(|p| chars[p]);
        if start_prefix(prev) {
            if let Some((found, end)) = markup_at(&chars, i, &mut misses) {
                implicit(&chars[plain_start..i], &mut inlines);
                inlines.extend(found);
                i = end;
                plain_start = end;
                continue;
            }
            // A start-string with no end-string is docutils' `problematic`
            // node: its own text node, which nothing inside starts again.
            if let Some(len) = problematic(&chars, i, &mut misses) {
                implicit(&chars[plain_start..i], &mut inlines);
                inlines.push(Inline::Problematic(unescape(&chars[i..i + len])));
                i += len;
                plain_start = i;
                continue;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    implicit(&chars[plain_start..], &mut inlines);
    // Each text node stays its own, as docutils' do: the visitor appends
    // them one by one, and each becomes its own span.
    inlines.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
    inlines
}

/// The length of the start-string at `i` when it is one (followed by
/// non-whitespace, not between quotes), for when it finds no end-string.
fn problematic(chars: &[char], i: usize, misses: &mut Misses) -> Option<usize> {
    let at = |offset: usize| chars.get(i + offset).copied();
    let len = match at(0)? {
        '*' if at(1) == Some('*') => 2,
        '`' if at(1) == Some('`') => 2,
        '_' if at(1) == Some('`') => 2,
        '*' | '`' => 1,
        '|' if at(1) != Some('|') => 1,
        ':' => {
            let name_end = misses.simplename(chars, i + 1)?;
            if chars.get(name_end) != Some(&':') || chars.get(name_end + 1) != Some(&'`') {
                return None;
            }
            // `:role:` before ``` `` ``` starts nothing: the role is text and
            // the inline literal after it starts on its own.
            if chars.get(name_end + 2) == Some(&'`') {
                return None;
            }
            name_end + 2 - i
        }
        _ => return None,
    };
    let prev = i.checked_sub(1).map(|p| chars[p]);
    let next = at(len);
    (next.is_some_and(|c| !c.is_whitespace()) && !quoted(prev, next)).then_some(len)
}

/// Where each kind of end-string was last looked for in vain. An end-string
/// qualifies by what surrounds it, not by where the search began, so a search
/// from further on fails too; skipping it keeps a paragraph full of unmatched
/// start-strings linear.
///
/// It also remembers the last `simplename` run and the last `]` found, which
/// every start inside the run (or before the `]`) shares: without them a
/// paragraph like `a-a-a-…` or `[1 [1 [1 …` rescans its rest at each word.
#[derive(Default)]
struct Misses(
    HashMap<&'static str, usize>,
    Option<(usize, usize)>,
    Option<(usize, Option<usize>)>,
);

impl Misses {
    /// [`simplename`] at `start`, reusing the run it last found: a name that
    /// starts inside that run ends where it ends.
    fn simplename(&mut self, chars: &[char], start: usize) -> Option<usize> {
        if !chars.get(start).copied().is_some_and(is_word) {
            return None;
        }
        if let Some((from, end)) = self.1 {
            if from <= start && start < end {
                return Some(end);
            }
        }
        let end = simplename(chars, start)?;
        self.1 = Some((start, end));
        Some(end)
    }

    /// The first `]` at or after `from`, reusing the last search: nothing
    /// between where it started and what it found is a `]`.
    fn close_bracket(&mut self, chars: &[char], from: usize) -> Option<usize> {
        if let Some((searched, found)) = self.2 {
            if searched <= from && found.is_none_or(|found| from <= found) {
                return found;
            }
        }
        let found = (from..chars.len()).find(|&j| chars[j] == ']');
        self.2 = Some((from, found));
        found
    }

    fn search<T>(
        &mut self,
        kind: &'static str,
        from: usize,
        search: impl FnOnce() -> Option<T>,
    ) -> Option<T> {
        if self.0.get(kind).is_some_and(|&missed| missed <= from) {
            return None;
        }
        let found = search();
        if found.is_none() {
            self.0.insert(kind, from);
        }
        found
    }
}

/// The first `end` at or after `from` that follows non-whitespace (and no
/// escape) and precedes an end-suffix. Returns its index.
fn find_end(chars: &[char], from: usize, end: &str, literal: bool) -> Option<usize> {
    let end: Vec<char> = end.chars().collect();
    let mut j = from;
    while j + end.len() <= chars.len() {
        if chars[j..j + end.len()] == end[..] && j > 0 {
            let before = chars[j - 1];
            let ok_before = !before.is_whitespace() && (literal || before != ESCAPE);
            if ok_before && end_suffix(chars.get(j + end.len()).copied()) {
                return Some(j);
            }
        }
        j += 1;
    }
    None
}

/// Markup that starts at `i`: its nodes and where it ends.
fn markup_at(chars: &[char], i: usize, misses: &mut Misses) -> Option<(Vec<Inline>, usize)> {
    let at = |offset: usize| chars.get(i + offset).copied();
    let prev = i.checked_sub(1).map(|p| chars[p]);
    // Content must start with non-whitespace, and not close a quote.
    let opens = |len: usize| at(len).is_some_and(|c| !c.is_whitespace()) && !quoted(prev, at(len));
    match at(0)? {
        '*' if at(1) == Some('*') => {
            if !opens(2) {
                return None;
            }
            let end = misses.search("**", i + 3, || find_end(chars, i + 3, "**", false))?;
            Some((vec![Inline::Strong(unescape(&chars[i + 2..end]))], end + 2))
        }
        '*' => {
            if !opens(1) {
                return None;
            }
            let end = misses.search("*", i + 2, || find_end(chars, i + 2, "*", false))?;
            Some((
                vec![Inline::Emphasis(unescape(&chars[i + 1..end]))],
                end + 1,
            ))
        }
        '`' if at(1) == Some('`') => {
            if !opens(2) {
                return None;
            }
            let end = misses.search("``", i + 3, || find_end(chars, i + 3, "``", true))?;
            Some((
                vec![Inline::Literal(restore(&chars[i + 2..end]), Vec::new())],
                end + 2,
            ))
        }
        '`' => interpreted(chars, i, i + 1, None, misses),
        '_' if at(1) == Some('`') => {
            if !opens(2) {
                return None;
            }
            let end = misses.search("_`", i + 3, || find_end(chars, i + 3, "`", false))?;
            Some((vec![Inline::Target(unescape(&chars[i + 2..end]))], end + 1))
        }
        '|' if at(1) != Some('|') => {
            if !opens(1) {
                return None;
            }
            misses.search("|", i + 2, || substitution(chars, i))
        }
        '[' => footnote_reference(chars, i, misses),
        ':' => {
            let name_end = misses.simplename(chars, i + 1)?;
            if chars.get(name_end) != Some(&':') || chars.get(name_end + 1) != Some(&'`') {
                return None;
            }
            if chars.get(name_end + 2) == Some(&'`') {
                return None;
            }
            let role: String = chars[i + 1..name_end].iter().collect();
            interpreted(chars, i, name_end + 2, Some(role), misses)
        }
        c if is_word(c) => reference_name(chars, i, misses),
        _ => None,
    }
}

/// Interpreted text or a phrase reference: `` `text` ``, `` :role:`text` ``,
/// `` `text`:role: ``, `` `text`_ ``, `` `text <uri>`_ ``.
fn interpreted(
    chars: &[char],
    start: usize,
    content: usize,
    prefix_role: Option<String>,
    misses: &mut Misses,
) -> Option<(Vec<Inline>, usize)> {
    let prev = start.checked_sub(1).map(|p| chars[p]);
    let first = chars.get(content).copied();
    if first.is_none_or(char::is_whitespace) || first == Some('`') {
        return None;
    }
    if prefix_role.is_none() && quoted(prev, first) {
        return None;
    }
    // The end: a backquote, then an optional `:role:`, then `_` or `__`.
    let (j, k, suffix_role, refend) = misses.search("`", content + 1, || {
        (content + 1..chars.len()).find_map(|j| interpreted_end(chars, j))
    })?;
    let body = &chars[content..j];
    // docutils shows markup it cannot read as its source, backslashes kept.
    let raw = restore(&chars[start..k]);
    let nodes = if refend > 0 {
        if prefix_role.is_some() || suffix_role.is_some() {
            vec![Inline::Problematic(raw)]
        } else {
            let mut nodes = phrase_reference(body, refend == 2);
            for node in &mut nodes {
                if let Inline::Reference { raw: source, .. } = node {
                    source.clone_from(&raw);
                }
            }
            nodes
        }
    } else if prefix_role.is_some() && suffix_role.is_some() {
        vec![Inline::Problematic(raw)]
    } else {
        role(prefix_role.or(suffix_role), body, &raw)
    };
    Some((nodes, k))
}

/// An interpreted text end-string at `j`: its backquote's index, where it
/// ends, its role suffix and how many underscores follow.
fn interpreted_end(chars: &[char], j: usize) -> Option<(usize, usize, Option<String>, usize)> {
    if chars[j] != '`' || chars[j - 1].is_whitespace() || chars[j - 1] == ESCAPE {
        return None;
    }
    let mut k = j + 1;
    let mut suffix_role = None;
    if chars.get(k) == Some(&':') {
        if let Some(name_end) = simplename(chars, k + 1) {
            if chars.get(name_end) == Some(&':') {
                suffix_role = Some(chars[k + 1..name_end].iter().collect::<String>());
                k = name_end + 1;
            }
        }
    }
    let mut refend = 0;
    while refend < 2 && chars.get(k) == Some(&'_') {
        refend += 1;
        k += 1;
    }
    end_suffix(chars.get(k).copied()).then_some((j, k, suffix_role, refend))
}

/// `` `text`_ `` and `` `text <uri>`_ ``.
fn phrase_reference(body: &[char], anonymous: bool) -> Vec<Inline> {
    // An embedded URI or alias: `<...>` at the end, after whitespace or alone.
    if body.last() == Some(&'>') {
        if let Some(open) = body.iter().rposition(|&c| c == '<') {
            let before_ok = open == 0 || matches!(body[open - 1], ' ' | '\n');
            let inner = &body[open + 1..body.len() - 1];
            let inner_ok = !inner.is_empty()
                && !matches!(inner[0], ' ' | '\n')
                && !matches!(inner[inner.len() - 1], ' ' | '\n' | ESCAPE);
            if before_ok && inner_ok {
                let alias = unescape(&body[..open]).trim().to_string();
                let target = unescape(inner);
                let text_or = |fallback: &str| {
                    if alias.is_empty() {
                        fallback.to_string()
                    } else {
                        alias.clone()
                    }
                };
                if target.ends_with('_') && inner.get(inner.len() - 2) != Some(&ESCAPE) {
                    // An alias of another name: `` `text <name_>`_ ``.
                    let name = &target[..target.len() - 1];
                    return vec![Inline::Reference {
                        text: text_or(name),
                        uri: None,
                        name: Some(normalize_name(name)),
                        anonymous,
                        raw: String::new(),
                    }];
                }
                let uri: String = target.split_whitespace().collect();
                let text = text_or(&uri);
                // A named embedded URI is also a target for that name.
                let name = (!anonymous).then(|| normalize_name(&text));
                return vec![Inline::Reference {
                    text,
                    uri: Some(uri),
                    name,
                    anonymous,
                    raw: String::new(),
                }];
            }
        }
    }
    let text = unescape(body);
    let name = (!anonymous).then(|| normalize_name(&text));
    vec![Inline::Reference {
        text,
        uri: None,
        name,
        anonymous,
        raw: String::new(),
    }]
}

/// The Sphinx roles `rich-rst` registers to render as literals.
const SPHINX_ROLES: &[&str] = &[
    "func", "function", "meth", "method", "class", "mod", "module", "attr", "attribute", "obj",
    "object", "data", "const", "constant", "exc", "exception", "var", "variable", "type",
    "py:func", "py:meth", "py:class", "py:mod", "py:attr", "py:obj", "py:data", "py:const",
    "py:exc", "envvar", "token", "option", "term", "ref", "doc", "any", "numref", "download",
    "mailheader", "mimetype", "newsgroup", "makevar", "regexp", "kbd", "guilabel", "manpage",
    "py:variable", "py:type", "py:property", "py:parameter", "py:typevar", "c:func",
    "c:function", "c:type", "c:struct", "c:union", "c:enum", "c:enumerator", "c:member", "c:var",
    "c:macro", "c:expr", "c:texpr", "cpp:func", "cpp:function", "cpp:class", "cpp:type",
    "cpp:member", "cpp:var", "cpp:enum", "cpp:enumerator", "cpp:concept", "cpp:expr",
    "cpp:texpr", "cpp:alias", "js:mod", "js:module", "js:func", "js:function", "js:data",
    "js:attr", "js:attribute", "js:class", "js:meth", "js:method",
];

/// `title <target>` shows its title, as the Sphinx roles do.
fn display_title(text: &str) -> String {
    if text.contains('<') && text.ends_with('>') {
        let open = text.rfind('<').unwrap_or(0);
        let title = text[..open].trim();
        if !title.is_empty() {
            return title.to_string();
        }
    }
    text.to_string()
}

/// A reference with a URI.
fn link(text: String, uri: String) -> Inline {
    Inline::Reference {
        text,
        uri: Some(uri),
        name: None,
        anonymous: false,
        raw: String::new(),
    }
}

/// Interpreted text under `role` (the default, `title-reference`, when none).
fn role(role: Option<String>, body: &[char], raw: &str) -> Vec<Inline> {
    let text = unescape(body);
    // `math` and `code` keep their backslashes.
    let verbatim = restore(body);
    let role = role.map(|role| role.to_lowercase());
    // The PEP and RFC roles take a number and an optional `#anchor`.
    let numbered = |text: &str| {
        let (number, anchor) = match text.split_once('#') {
            Some((number, anchor)) => (number.trim().to_string(), format!("#{anchor}")),
            None => (text.trim().to_string(), String::new()),
        };
        (number.parse::<u32>().ok(), number, anchor)
    };
    let node = match role.as_deref() {
        None | Some("title-reference" | "title" | "t") => Inline::TitleReference(text),
        Some("emphasis" | "dfn") => Inline::Emphasis(text),
        Some("strong") => Inline::Strong(text),
        Some("command" | "program") => Inline::Strong(display_title(&text)),
        Some("literal") => Inline::Literal(text, Vec::new()),
        Some("code") => Inline::Literal(verbatim, vec!["code".into()]),
        Some("subscript" | "sub") => Inline::Subscript(text),
        Some("superscript" | "sup") => Inline::Superscript(text),
        Some("math") => Inline::Math(verbatim),
        Some("abbreviation" | "ab") => Inline::Abbreviation(text, String::new()),
        Some("acronym" | "ac") => Inline::Acronym(text),
        Some("abbr") => match text.rfind('(').filter(|_| text.ends_with(')')) {
            Some(open) => Inline::Abbreviation(
                text[..open].trim().to_string(),
                text[open + 1..text.len() - 1].to_string(),
            ),
            None => Inline::Abbreviation(text, String::new()),
        },
        Some("menuselection") => Inline::Literal(text.replace("-->", "\u{25b6}"), Vec::new()),
        Some("samp" | "file") => {
            Inline::Literal(text.replace(['{', '}'], ""), Vec::new())
        }
        Some("pep" | "pep-reference") => {
            let (number, shown, anchor) = numbered(&text);
            let uri = match number {
                Some(number) => format!("https://peps.python.org/pep-{number:04}/{anchor}"),
                None => "https://peps.python.org/".into(),
            };
            link(format!("PEP {shown}"), uri)
        }
        Some("rfc" | "rfc-reference") => {
            let (number, shown, anchor) = numbered(&text);
            let uri = match number {
                Some(number) => format!("https://datatracker.ietf.org/doc/html/rfc{number}{anchor}"),
                None => "https://datatracker.ietf.org/".into(),
            };
            link(format!("RFC {shown}"), uri)
        }
        Some("cve") => {
            let id = text.trim().trim_start_matches(['C', 'V', 'E', '-']);
            link(
                format!("CVE-{id}"),
                format!("https://www.cve.org/CVERecord?id=CVE-{id}"),
            )
        }
        Some("cwe") => {
            let number = text.trim();
            link(
                format!("CWE-{number}"),
                format!("https://cwe.mitre.org/data/definitions/{number}.html"),
            )
        }
        Some("pypi") => {
            let project = text.trim();
            link(project.to_string(), format!("https://pypi.org/project/{project}/"))
        }
        Some(name) if SPHINX_ROLES.contains(&name) => {
            Inline::Literal(display_title(&text), Vec::new())
        }
        // An unknown role is an error, shown as its source.
        Some(_) => Inline::Problematic(raw.to_string()),
    };
    vec![node]
}

/// `|name|`, `|name|_` and `|name|__`.
fn substitution(chars: &[char], start: usize) -> Option<(Vec<Inline>, usize)> {
    let mut j = start + 2;
    while j < chars.len() {
        if chars[j] == '|' && !chars[j - 1].is_whitespace() && chars[j - 1] != ESCAPE {
            let mut k = j + 1;
            let mut refend = 0;
            while refend < 2 && chars.get(k) == Some(&'_') {
                refend += 1;
                k += 1;
            }
            if end_suffix(chars.get(k).copied()) {
                let name = unescape(&chars[start + 1..j]);
                return Some((vec![Inline::SubstitutionReference { name, refend }], k));
            }
        }
        j += 1;
    }
    None
}

/// `[1]_`, `[#]_`, `[#name]_`, `[*]_` (footnotes) and `[name]_` (citations).
fn footnote_reference(
    chars: &[char],
    start: usize,
    misses: &mut Misses,
) -> Option<(Vec<Inline>, usize)> {
    let close = misses.close_bracket(chars, start + 1)?;
    if chars.get(close + 1) != Some(&'_') || !end_suffix(chars.get(close + 2).copied()) {
        return None;
    }
    let label: String = chars[start + 1..close].iter().collect();
    let node = if !label.is_empty() && label.chars().all(|c| c.is_ascii_digit()) {
        Inline::FootnoteReference {
            label: Some(label),
            auto: None,
            name: None,
        }
    } else if label == "*" {
        Inline::FootnoteReference {
            label: None,
            auto: Some("*".into()),
            name: None,
        }
    } else if let Some(name) = label.strip_prefix('#') {
        let label_chars: Vec<char> = label.chars().collect();
        if !name.is_empty() && simplename(&label_chars, 1) != Some(label_chars.len()) {
            return None;
        }
        Inline::FootnoteReference {
            label: None,
            auto: Some("1".into()),
            name: (!name.is_empty()).then(|| normalize_name(name)),
        }
    } else {
        let label_chars: Vec<char> = label.chars().collect();
        if simplename(&label_chars, 0) != Some(label_chars.len()) {
            return None;
        }
        Inline::CitationReference(label)
    };
    Some((vec![node], close + 2))
}

/// `name_` and `name__`.
fn reference_name(
    chars: &[char],
    start: usize,
    misses: &mut Misses,
) -> Option<(Vec<Inline>, usize)> {
    let end = misses.simplename(chars, start)?;
    let mut k = end;
    let mut refend = 0;
    while refend < 2 && chars.get(k) == Some(&'_') {
        refend += 1;
        k += 1;
    }
    if refend == 0 || !end_suffix(chars.get(k).copied()) {
        return None;
    }
    let text = unescape(&chars[start..end]);
    let name = (refend == 1).then(|| normalize_name(&text));
    Some((
        vec![Inline::Reference {
            text,
            uri: None,
            name,
            anonymous: refend == 2,
            raw: unescape(&chars[start..k]),
        }],
        k,
    ))
}

/// Registered URI schemes (docutils' `urischemes`), the common ones.
const SCHEMES: &[&str] = &[
    "about",
    "data",
    "file",
    "ftp",
    "git",
    "gopher",
    "http",
    "https",
    "imap",
    "irc",
    "javascript",
    "ldap",
    "mailto",
    "news",
    "nfs",
    "nntp",
    "pop",
    "rtsp",
    "sftp",
    "sip",
    "sips",
    "smb",
    "ssh",
    "tag",
    "tel",
    "telnet",
    "tftp",
    "urn",
    "uuid",
    "view-source",
    "wais",
];

fn uric(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.!~*'()[];/:@&=+$,%".contains(c) || c == ESCAPE
}

fn urilast(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_~*/=+".contains(c)
}

fn emailc(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_!~*'{|}/#?^`&=+$%".contains(c) || c == ESCAPE
}

/// Standalone URIs and email addresses in plain text: docutils' implicit
/// inline markup.
fn implicit(chars: &[char], inlines: &mut Vec<Inline>) {
    let mut runs = Runs::default();
    let mut plain_start = 0;
    let mut i = 0;
    while i < chars.len() {
        let prev = i.checked_sub(1).map(|p| chars[p]);
        if start_prefix(prev) && chars[i].is_ascii_alphanumeric() {
            if let Some((node, end)) =
                uri_at(chars, i, &mut runs).or_else(|| email_at(chars, i, &mut runs))
            {
                inlines.push(Inline::Text(unescape(&chars[plain_start..i])));
                inlines.push(node);
                i = end;
                plain_start = end;
                continue;
            }
        }
        i += 1;
    }
    inlines.push(Inline::Text(unescape(&chars[plain_start..])));
}

/// The ends of the last scheme and email-name runs scanned: every start
/// inside a run shares its end, so a paragraph like `a-a-a-…` scans once.
#[derive(Default)]
struct Runs {
    scheme: Option<(usize, usize)>,
    name: Option<(usize, usize)>,
}

/// The end of the run of `pred` characters from `start`, reusing `last`.
fn run_end(
    last: &mut Option<(usize, usize)>,
    chars: &[char],
    start: usize,
    pred: impl Fn(char) -> bool,
) -> usize {
    if let Some((from, end)) = *last {
        if from <= start && start <= end {
            return end;
        }
    }
    let mut end = start;
    while chars.get(end).is_some_and(|&c| pred(c)) {
        end += 1;
    }
    *last = Some((start, end));
    end
}

/// The longest end in `start..=max` at which `last` holds and the URI may
/// end.
fn uri_end(chars: &[char], min: usize, max: usize, last: impl Fn(char) -> bool) -> Option<usize> {
    (min..max).rev().find(|&j| {
        (last(chars[j]) || (uric(chars[j]) && chars.get(j + 1) == Some(&'>')))
            && end_suffix(chars.get(j + 1).copied())
    })
}

fn uri_at(chars: &[char], start: usize, runs: &mut Runs) -> Option<(Inline, usize)> {
    let i = run_end(&mut runs.scheme, chars, start + 1, |c| {
        c.is_ascii_alphanumeric() || ".+-".contains(c)
    });
    if chars.get(i) != Some(&':') {
        return None;
    }
    let scheme: String = chars[start..i].iter().collect::<String>().to_lowercase();
    if !SCHEMES.contains(&scheme.as_str()) {
        return None;
    }
    let body = i + 1;
    let mut max = body;
    while chars
        .get(max)
        .is_some_and(|&c| uric(c) || c == '?' || c == '#')
    {
        max += 1;
    }
    let end = uri_end(chars, body, max, urilast)? + 1;
    let text = unescape(&chars[start..end]);
    Some((
        Inline::Reference {
            uri: Some(text.clone()),
            text,
            name: None,
            anonymous: false,
            raw: String::new(),
        },
        end,
    ))
}

fn email_at(chars: &[char], start: usize, runs: &mut Runs) -> Option<(Inline, usize)> {
    let at = run_end(&mut runs.name, chars, start, |c| emailc(c) || c == '.');
    if chars.get(at) != Some(&'@') || at == start || chars[at - 1] == ESCAPE {
        return None;
    }
    let host = at + 1;
    let mut max = host;
    while chars.get(max).is_some_and(|&c| emailc(c) || c == '.') {
        max += 1;
    }
    if max == host {
        return None;
    }
    let end = uri_end(chars, host, max, urilast)? + 1;
    let text = unescape(&chars[start..end]);
    Some((
        Inline::Reference {
            uri: Some(format!("mailto:{text}")),
            text,
            name: None,
            anonymous: false,
            raw: String::new(),
        },
        end,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Inline {
        Inline::Text(s.into())
    }

    #[test]
    fn emphasis_strong_and_literals() {
        assert_eq!(
            parse_inline("a *b* **c** ``d\\e``"),
            [
                text("a "),
                Inline::Emphasis("b".into()),
                text(" "),
                Inline::Strong("c".into()),
                text(" "),
                Inline::Literal("d\\e".into(), Vec::new()),
            ]
        );
    }

    #[test]
    fn markup_needs_its_start_and_end_conditions() {
        assert_eq!(parse_inline("2 * 3 * 4"), [text("2 * 3 * 4")]);
        assert_eq!(parse_inline("a*b*"), [text("a*b*")]);
        assert_eq!(parse_inline("\\*not\\*"), [text("*not*")]);
        assert_eq!(parse_inline("\"*\""), [text("\"*\"")]);
    }

    #[test]
    fn references() {
        assert_eq!(
            parse_inline("see `Rust <https://rust-lang.org>`_ and Python_."),
            [
                text("see "),
                Inline::Reference {
                    text: "Rust".into(),
                    uri: Some("https://rust-lang.org".into()),
                    name: Some("rust".into()),
                    anonymous: false,
                    raw: String::new(),
                },
                text(" and "),
                Inline::Reference {
                    text: "Python".into(),
                    uri: None,
                    name: Some("python".into()),
                    anonymous: false,
                    raw: String::new(),
                },
                text("."),
            ]
        );
    }

    #[test]
    fn roles_and_footnotes() {
        assert_eq!(
            parse_inline(":sub:`2` :func:`f` [CIT]_ :bogus:`x`"),
            [
                Inline::Subscript("2".into()),
                text(" "),
                Inline::Literal("f".into(), Vec::new()),
                text(" "),
                Inline::CitationReference("CIT".into()),
                text(" "),
                Inline::Problematic(":bogus:`x`".into()),
            ]
        );
    }
}
