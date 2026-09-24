//! Redact secrets from strings, terminal output, rendered segments and
//! exports.
//!
//! A [`Redactor`] holds rules: built-in [`Detector`]s for common secret
//! shapes and your own regular expressions. It replaces what they match with
//! a mask (`********` by default) in
//!
//! * plain strings and log lines ([`Redactor::redact_str`]);
//! * text with ANSI escape sequences, keeping the escapes
//!   ([`Redactor::redact_ansi`]), or a stream of such chunks
//!   ([`Redactor::redact_chunks`]);
//! * rendered [`Segment`]s, between [`Console::record_output`] and an export
//!   ([`Redactor::redact_segments`]), with helpers that record, redact and
//!   export in one call ([`Redactor::export_svg`], [`Redactor::export_html`],
//!   [`Redactor::export_text`], …) and a [`Redacted`] renderable.
//!
//! Rules match on a line's text, so a secret split over several segments with
//! different styles is still found. In segments each part of the mask keeps
//! the style of the cells it covers and the line keeps its width, so borders
//! and columns stay aligned.
//!
//! ```
//! use rich::{ColorSystem, Console, Text};
//! use rich_ext::redact::Redactor;
//!
//! let redactor = Redactor::secrets();
//! assert_eq!(
//!     redactor.redact_str("GITHUB_TOKEN=ghp_0123456789abcdefghijklmnopqrstuvwxyzAB user=ann"),
//!     "GITHUB_TOKEN=******** user=ann"
//! );
//!
//! let console = Console::builder()
//!     .width(40)
//!     .force_terminal(true)
//!     .color_system(Some(ColorSystem::Truecolor))
//!     .build();
//! let text = redactor.export_text(&console, |c| {
//!     c.print(&Text::new("password: hunter2"));
//! });
//! assert_eq!(text, "password: *******\n");
//! ```
//!
//! # Limits
//!
//! Detection is pattern matching, not proof: the built-in set is kept small
//! to avoid false positives, and it will miss secrets that look like
//! ordinary words. Rules match within one line. In rendered output a secret
//! that a renderable wrapped onto two lines is not found; redact the input
//! with [`Redactor::redact_str`] before rendering when that can happen.
//! Masks that keep the width (in segments, or with
//! [`preserve_width`](Redactor::preserve_width)) reveal the secret's length.

use std::fmt;
use std::sync::OnceLock;

use fancy_regex::Regex;
use rich::cells::{cell_len, char_cell_width};
use rich::{Console, ConsoleOptions, Renderable, Segment};

/// Key-name fragments that mark a `key=value` or `key: value` value as
/// secret, and the fields [`data::Redaction::secrets`] masks.
///
/// Matching is case-insensitive: a plain entry matches anywhere in the key, an
/// entry with `*` or `?` must match the whole key (so `author` is not masked
/// but `auth`, `gh_auth` and `auth_header` are).
///
/// [`data::Redaction::secrets`]: https://docs.rs/rs-rich-ext/latest/rich_ext/data/struct.Redaction.html#method.secrets
pub const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "credential",
    // Whole-key globs: a bare `auth` substring would also mask `author`.
    "*auth",
    "auth_*",
    "auth-*",
    "authorization",
];

/// Whether `key` names a secret by [`SECRET_KEYS`]. `-` and `_` are
/// interchangeable for the plain fragments, so `api-key` matches `api_key`.
///
/// ```
/// use rich_ext::redact::is_secret_key;
///
/// assert!(is_secret_key("GITHUB_TOKEN"));
/// assert!(is_secret_key("api-key"));
/// assert!(is_secret_key("gh_auth"));
/// assert!(!is_secret_key("author"));
/// ```
pub fn is_secret_key(key: &str) -> bool {
    let normalized = key.replace('-', "_");
    SECRET_KEYS.iter().any(|pattern| {
        crate::env_inspect::name_matches(pattern, key)
            || (!pattern.contains(['*', '?'])
                && crate::env_inspect::name_matches(pattern, &normalized))
    })
}

/// A built-in secret detector. [`Redactor::secrets`] enables all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detector {
    /// The value in `key=value`, `key: value`, `"key": "value"` or
    /// `--key=value` when the key is secret by [`is_secret_key`]. An
    /// `Authorization: Bearer …` scheme word is kept and only the credential
    /// masked.
    KeyValue,
    /// The token after `Bearer ` (at least 12 characters with a digit).
    Bearer,
    /// Tokens with a well-known prefix: GitHub (`ghp_`, `gho_`, `ghu_`,
    /// `ghs_`, `ghr_`, `github_pat_`), GitLab (`glpat-`), Slack (`xoxb-` and
    /// the other `xox?-` kinds), Stripe secret and restricted keys
    /// (`sk_live_`, `rk_test_`, …), npm (`npm_`) and `sk-` API keys (OpenAI,
    /// Anthropic and others), each with a minimum length.
    TokenPrefix,
    /// AWS access key ids: `AKIA` or `ASIA` and 16 upper-case letters or
    /// digits.
    AwsAccessKey,
    /// JSON Web Tokens: three base64url parts, the first two starting `eyJ`.
    Jwt,
    /// The password in a URL's `user:password@`.
    UrlCredentials,
}

impl Detector {
    /// Every detector, in the order they are tried.
    pub const ALL: [Detector; 6] = [
        Detector::KeyValue,
        Detector::Bearer,
        Detector::TokenPrefix,
        Detector::AwsAccessKey,
        Detector::Jwt,
        Detector::UrlCredentials,
    ];

    /// A short kebab-case name, used for `{kind}` in a mask.
    pub fn name(self) -> &'static str {
        match self {
            Detector::KeyValue => "key-value",
            Detector::Bearer => "bearer",
            Detector::TokenPrefix => "token",
            Detector::AwsAccessKey => "aws-access-key",
            Detector::Jwt => "jwt",
            Detector::UrlCredentials => "url-credentials",
        }
    }

    fn pattern(self) -> &'static str {
        match self {
            Detector::KeyValue => concat!(
                r#"(?i)(?<![\w.])(?P<key>[a-z_][\w.-]*)["']?[ \t]*[:=][ \t]*"#,
                r#"(?:(?:bearer|basic|token)[ \t]+)?["']?(?P<secret>[^\s"',;&]+)"#,
            ),
            Detector::Bearer => {
                r"(?i)\bbearer[ \t]+(?P<secret>(?=[a-z._~+/=-]*[0-9])[a-z0-9._~+/=-]{12,})"
            }
            Detector::TokenPrefix => concat!(
                r"(?<![A-Za-z0-9_-])(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{22,}",
                r"|glpat-[A-Za-z0-9_-]{20,}|xox[abposr]-[A-Za-z0-9-]{10,}",
                r"|[sr]k_(?:live|test)_[A-Za-z0-9]{16,}|npm_[A-Za-z0-9]{36}",
                r"|sk-[A-Za-z0-9_-]{20,})",
            ),
            Detector::AwsAccessKey => r"(?<![A-Z0-9])(?:AKIA|ASIA)[A-Z0-9]{16}(?![A-Z0-9])",
            Detector::Jwt => {
                r"(?<![A-Za-z0-9_-])eyJ[A-Za-z0-9_-]{5,}\.eyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]*"
            }
            Detector::UrlCredentials => {
                r"(?i)\b[a-z][a-z0-9+.-]*://[^\s/:@]+:(?P<secret>[^\s/@]+)@"
            }
        }
    }

    fn regex(self) -> &'static Regex {
        static COMPILED: OnceLock<Vec<Regex>> = OnceLock::new();
        let all = COMPILED.get_or_init(|| {
            Detector::ALL
                .iter()
                .map(|d| Regex::new(d.pattern()).expect("valid detector pattern"))
                .collect()
        });
        &all[Detector::ALL
            .iter()
            .position(|d| *d == self)
            .expect("listed")]
    }
}

/// A pattern that did not compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    /// The pattern as given.
    pub pattern: String,
    /// Why it did not compile.
    pub message: String,
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid pattern {:?}: {}", self.pattern, self.message)
    }
}

impl std::error::Error for PatternError {}

#[derive(Clone, Debug)]
enum Rule {
    Detector(Detector),
    Pattern { name: String, regex: Box<Regex> },
}

impl Rule {
    fn name(&self) -> &str {
        match self {
            Rule::Detector(d) => d.name(),
            Rule::Pattern { name, .. } => name,
        }
    }

    fn regex(&self) -> &Regex {
        match self {
            Rule::Detector(d) => d.regex(),
            Rule::Pattern { regex, .. } => regex,
        }
    }
}

/// One redacted span of a line, as byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Start byte.
    pub start: usize,
    /// End byte (exclusive).
    pub end: usize,
    /// The rule that matched: a [`Detector::name`] or a pattern's name.
    pub kind: String,
}

/// Replaces secrets with a mask. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Redactor {
    rules: Vec<Rule>,
    mask: String,
    preserve_width: bool,
}

impl Default for Redactor {
    fn default() -> Self {
        Redactor {
            rules: Vec::new(),
            mask: "********".into(),
            preserve_width: false,
        }
    }
}

impl Redactor {
    /// No rules; mask `********`. Add [`detector`](Self::detector)s or
    /// [`pattern`](Self::pattern)s.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every built-in [`Detector`].
    pub fn secrets() -> Self {
        Detector::ALL
            .into_iter()
            .fold(Self::new(), |redactor, detector| {
                redactor.detector(detector)
            })
    }

    /// Add a built-in detector.
    pub fn detector(mut self, detector: Detector) -> Self {
        self.rules.push(Rule::Detector(detector));
        self
    }

    /// Add a regular expression (fancy-regex syntax, so lookaround works).
    /// When it has a group named `secret`, only that group is masked;
    /// otherwise the whole match is. Its `{kind}` is `pattern`.
    pub fn pattern(self, pattern: &str) -> Result<Self, PatternError> {
        self.named_pattern("pattern", pattern)
    }

    /// As [`pattern`](Self::pattern), with `name` as its `{kind}`.
    pub fn named_pattern(
        mut self,
        name: impl Into<String>,
        pattern: &str,
    ) -> Result<Self, PatternError> {
        let regex = Regex::new(pattern).map_err(|e| PatternError {
            pattern: pattern.to_string(),
            message: e.to_string(),
        })?;
        self.rules.push(Rule::Pattern {
            name: name.into(),
            regex: Box::new(regex),
        });
        Ok(self)
    }

    /// The replacement text (default `********`). `{kind}` is replaced by
    /// the matching rule's name, so `[{kind}]` gives `[jwt]`.
    pub fn mask(mut self, mask: impl Into<String>) -> Self {
        self.mask = mask.into();
        self
    }

    /// Fit each mask to the cell width of what it replaces in
    /// [`redact_str`](Self::redact_str), [`redact_ansi`](Self::redact_ansi)
    /// and [`redact_chunks`](Self::redact_chunks) (default off; segments
    /// always keep their width). A mask of one repeated character, like the
    /// default, fills the width; any other mask is cut, or padded with
    /// spaces.
    pub fn preserve_width(mut self, on: bool) -> Self {
        self.preserve_width = on;
        self
    }

    /// Whether there are no rules, so nothing is ever redacted.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The spans of `line` to mask, sorted and merged where they overlap.
    /// Rules match within one line; pass lines, not whole documents, when
    /// a pattern could otherwise cross a line break.
    pub fn find(&self, line: &str) -> Vec<Match> {
        let mut found: Vec<Match> = Vec::new();
        for rule in &self.rules {
            let regex = rule.regex();
            let has_secret = regex.capture_names().any(|n| n == Some("secret"));
            let key_value = matches!(rule, Rule::Detector(Detector::KeyValue));
            let mut pos = 0;
            while pos <= line.len() {
                let Ok(Some(captures)) = regex.captures_from_pos(line, pos) else {
                    break;
                };
                let whole = captures.get(0).expect("group 0 always matches");
                let mut next = if whole.end() > whole.start() {
                    whole.end()
                } else {
                    // An empty match: step past one character.
                    whole.end() + line[whole.end()..].chars().next().map_or(1, char::len_utf8)
                };
                let key = captures.name("key");
                if key_value && !key.is_some_and(|key| is_secret_key(key.as_str())) {
                    // Not a secret key; its value may still hold one
                    // (`url=https://x/?token=…`), so search on after the key.
                    next = key.map_or(next, |key| key.end());
                    pos = next;
                    continue;
                }
                let span = if has_secret {
                    captures.name("secret")
                } else {
                    Some(whole)
                };
                if let Some(span) = span.filter(|s| s.start() < s.end()) {
                    found.push(Match {
                        start: span.start(),
                        end: span.end(),
                        kind: rule.name().to_string(),
                    });
                }
                pos = next;
            }
        }
        found.sort_by_key(|m| (m.start, std::cmp::Reverse(m.end)));
        let mut merged: Vec<Match> = Vec::with_capacity(found.len());
        for m in found {
            match merged.last_mut() {
                Some(last) if m.start < last.end => last.end = last.end.max(m.end),
                _ => merged.push(m),
            }
        }
        merged
    }

    fn mask_for(&self, kind: &str) -> String {
        self.mask.replace("{kind}", kind)
    }

    fn replacement(&self, m: &Match, original: &str, fit: bool) -> String {
        let mask = self.mask_for(&m.kind);
        if fit {
            fit_mask(&mask, cell_len(original))
        } else {
            mask
        }
    }

    /// `text` with every match masked, line by line.
    pub fn redact_str(&self, text: &str) -> String {
        if self.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                out.push('\n');
            }
            let mut at = 0;
            for m in self.find(line) {
                out.push_str(&line[at..m.start]);
                out.push_str(&self.replacement(&m, &line[m.start..m.end], self.preserve_width));
                at = m.end;
            }
            out.push_str(&line[at..]);
        }
        out
    }

    /// `text` holding ANSI escape sequences with every match masked. Rules
    /// see the visible text, so a secret interrupted by a colour change is
    /// still found; the escape sequences themselves are kept.
    pub fn redact_ansi(&self, text: &str) -> String {
        apply_edits(text, &self.ansi_edits(text))
    }

    /// A stream of ANSI text chunks (a recording, say) with every match
    /// masked, returned chunk for chunk. A secret split across chunks is
    /// found, and its mask lands in the chunk where it starts.
    pub fn redact_chunks<S: AsRef<str>>(&self, chunks: &[S]) -> Vec<String> {
        let joined: String = chunks.iter().map(AsRef::as_ref).collect();
        let edits = self.ansi_edits(&joined);
        if edits.is_empty() {
            return chunks.iter().map(|c| c.as_ref().to_string()).collect();
        }
        let redacted = apply_edits(&joined, &edits);
        let mut out = Vec::with_capacity(chunks.len());
        let (mut original, mut from) = (0, 0);
        for chunk in chunks {
            original += chunk.as_ref().len();
            let to = map_offset(original, &edits);
            out.push(redacted[from..to].to_string());
            from = to;
        }
        out
    }

    fn ansi_edits(&self, text: &str) -> Vec<Edit> {
        let mut edits = Vec::new();
        if self.is_empty() {
            return edits;
        }
        let mut line_start = 0;
        for line in text.split('\n') {
            let (visible, offsets) = visible_text(line);
            for m in self.find(&visible) {
                // Contiguous runs of visible bytes in the original; escapes
                // between them are kept.
                let mut runs: Vec<(usize, usize)> = Vec::new();
                for &offset in &offsets[m.start..m.end] {
                    match runs.last_mut() {
                        Some(run) if run.1 == offset => run.1 = offset + 1,
                        _ => runs.push((offset, offset + 1)),
                    }
                }
                let covered = &visible[m.start..m.end];
                let replacement = self.replacement(&m, covered, self.preserve_width);
                // A fitted mask is spread over the runs cell for cell, so
                // each part keeps its colour; otherwise it goes in the first.
                let mut mask: Vec<char> = replacement.chars().collect();
                let last = runs.len().saturating_sub(1);
                for (index, (start, end)) in runs.into_iter().enumerate() {
                    let text = if self.preserve_width {
                        let width = cell_len(&line[start..end]);
                        take_cells(&mut mask, width, index == last)
                    } else if index == 0 {
                        replacement.clone()
                    } else {
                        String::new()
                    };
                    edits.push(Edit {
                        start: line_start + start,
                        end: line_start + end,
                        text,
                    });
                }
            }
            line_start += line.len() + 1;
        }
        edits
    }

    /// Rendered segments with every match masked, line by line.
    ///
    /// Each line keeps its cell width: the mask is fitted to the width of
    /// what it covers and spread over the covered segments, so every part
    /// keeps its segment's style. Control segments are kept.
    pub fn redact_segments(&self, segments: &[Segment]) -> Vec<Segment> {
        if self.is_empty() {
            return segments.to_vec();
        }
        // Pieces: segment text split at newlines, a newline its own piece.
        let mut lines: Vec<Vec<Segment>> = vec![Vec::new()];
        for segment in segments {
            if segment.control || !segment.text.contains('\n') {
                lines
                    .last_mut()
                    .expect("at least one line")
                    .push(segment.clone());
                continue;
            }
            for (index, part) in segment.text.split('\n').enumerate() {
                if index > 0 {
                    let line = lines.last_mut().expect("at least one line");
                    line.push(Segment::new("\n", segment.style.clone()));
                    lines.push(Vec::new());
                }
                if !part.is_empty() {
                    lines
                        .last_mut()
                        .expect("at least one line")
                        .push(Segment::new(part, segment.style.clone()));
                }
            }
        }
        let mut changed = false;
        let mut out = Vec::with_capacity(segments.len());
        for line in lines {
            let plain: String = line
                .iter()
                .filter(|s| !s.control && s.text != "\n")
                .map(|s| s.text.as_str())
                .collect();
            let matches = self.find(&plain);
            if matches.is_empty() {
                out.extend(line);
                continue;
            }
            changed = true;
            out.extend(self.mask_line(line, &plain, &matches));
        }
        if changed {
            out
        } else {
            segments.to_vec()
        }
    }

    fn mask_line(&self, line: Vec<Segment>, plain: &str, matches: &[Match]) -> Vec<Segment> {
        // Each match's fitted mask, consumed cell by cell as pieces cover it.
        let mut masks: Vec<Vec<char>> = matches
            .iter()
            .map(|m| {
                self.replacement(m, &plain[m.start..m.end], true)
                    .chars()
                    .collect()
            })
            .collect();
        let mut out = Vec::with_capacity(line.len() + 2);
        let mut at = 0;
        for segment in line {
            if segment.control || segment.text == "\n" {
                out.push(segment);
                continue;
            }
            let (start, end) = (at, at + segment.text.len());
            at = end;
            let mut text = String::with_capacity(segment.text.len());
            let mut cursor = start;
            for (index, m) in matches.iter().enumerate() {
                if m.end <= cursor || m.start >= end {
                    continue;
                }
                let from = m.start.max(cursor);
                let to = m.end.min(end);
                text.push_str(&plain[cursor..from]);
                let width = cell_len(&plain[from..to]);
                text.push_str(&take_cells(&mut masks[index], width, to == m.end));
                cursor = to;
            }
            text.push_str(&plain[cursor..end]);
            if !text.is_empty() {
                out.push(Segment::new(text, segment.style));
            }
        }
        out
    }

    /// Record what `f` prints and return it as terminal text (ANSI codes as
    /// the console renders them), redacted. The redacted form of
    /// [`Console::capture`].
    pub fn capture(&self, console: &Console, f: impl FnOnce(&Console)) -> String {
        console.segments_to_string(&self.record(console, f))
    }

    /// Record what `f` prints and return it as plain text, redacted. The
    /// redacted form of [`Console::export_text`].
    pub fn export_text(&self, console: &Console, f: impl FnOnce(&Console)) -> String {
        self.record(console, f)
            .iter()
            .filter(|s| !s.control)
            .map(|s| s.text.as_str())
            .collect()
    }

    /// Record what `f` prints and export it as HTML with inline styles,
    /// redacted. The redacted form of [`Console::export_html`].
    pub fn export_html(&self, console: &Console, f: impl FnOnce(&Console)) -> String {
        rich::export::export_html_inline(
            &self.record(console, f),
            &rich::terminal_theme::DEFAULT_TERMINAL_THEME,
        )
    }

    /// As [`export_html`](Self::export_html) with a CSS-class stylesheet. The
    /// redacted form of [`Console::export_html_classes`].
    pub fn export_html_classes(&self, console: &Console, f: impl FnOnce(&Console)) -> String {
        rich::export::export_html_classes(
            &self.record(console, f),
            &rich::terminal_theme::DEFAULT_TERMINAL_THEME,
        )
    }

    /// Record what `f` prints and export it as an SVG image, redacted. The
    /// redacted form of [`Console::export_svg`].
    pub fn export_svg(
        &self,
        console: &Console,
        title: &str,
        unique_id: &str,
        f: impl FnOnce(&Console),
    ) -> String {
        rich::svg::export_svg(
            &self.record(console, f),
            &rich::terminal_theme::SVG_EXPORT_THEME,
            title,
            unique_id,
            console.width(),
        )
    }

    fn record(&self, console: &Console, f: impl FnOnce(&Console)) -> Vec<Segment> {
        self.redact_segments(&console.record_output(f))
    }
}

/// A renderable whose output is redacted.
///
/// ```
/// use rich::{Console, Text};
/// use rich_ext::redact::{Redacted, Redactor};
///
/// let console = Console::builder().width(30).build();
/// let safe = Redacted::new(Text::new("token=abc123"), Redactor::secrets());
/// assert_eq!(console.render_to_string(&safe).trim_end(), "token=******");
/// ```
pub struct Redacted<R> {
    inner: R,
    redactor: Redactor,
}

impl<R: Renderable> Redacted<R> {
    /// Render `inner` through `redactor`.
    pub fn new(inner: R, redactor: Redactor) -> Self {
        Redacted { inner, redactor }
    }
}

impl<R: Renderable> Renderable for Redacted<R> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.redactor
            .redact_segments(&self.inner.rich_render(console, options))
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.inner.measure(console, options)
    }
}

/// `mask` fitted to exactly `width` cells.
fn fit_mask(mask: &str, width: usize) -> String {
    let mut chars = mask.chars();
    if let Some(first) = chars.next() {
        if char_cell_width(first) == 1 && chars.all(|c| c == first) {
            return first.to_string().repeat(width);
        }
    }
    let mut out = String::new();
    let mut used = 0;
    for c in mask.chars() {
        let w = char_cell_width(c);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.extend(std::iter::repeat_n(' ', width - used));
    out
}

/// The next `width` cells of `mask`; `last` takes the rest, padded to width.
fn take_cells(mask: &mut Vec<char>, width: usize, last: bool) -> String {
    let mut out = String::new();
    let mut used = 0;
    while let Some(&c) = mask.first() {
        let w = char_cell_width(c);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
        mask.remove(0);
    }
    out.extend(std::iter::repeat_n(' ', width - used));
    if last {
        mask.clear();
    }
    out
}

/// A replacement of `start..end` in the original text.
#[derive(Clone, Debug)]
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

fn apply_edits(text: &str, edits: &[Edit]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for edit in edits {
        out.push_str(&text[at..edit.start]);
        out.push_str(&edit.text);
        at = edit.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Where `offset` in the original lands after `edits`; an offset inside an
/// edit lands after its replacement.
fn map_offset(offset: usize, edits: &[Edit]) -> usize {
    let mut shift: isize = 0;
    for edit in edits {
        if edit.start >= offset {
            break;
        }
        let delta = edit.text.len() as isize - (edit.end - edit.start) as isize;
        if edit.end > offset {
            return (edit.start as isize + shift) as usize + edit.text.len();
        }
        shift += delta;
    }
    (offset as isize + shift) as usize
}

/// The text of `line` without escape sequences, and for each of its bytes
/// the byte offset in `line`.
fn visible_text(line: &str) -> (String, Vec<usize>) {
    let bytes = line.as_bytes();
    let mut visible = String::with_capacity(line.len());
    let mut offsets = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            i = skip_escape(bytes, i);
            continue;
        }
        let c = line[i..].chars().next().expect("char boundary");
        let len = c.len_utf8();
        visible.push(c);
        offsets.extend(i..i + len);
        i += len;
    }
    (visible, offsets)
}

/// The index after the escape sequence starting at `i` (CSI, OSC, other
/// string sequences, or a two-byte escape).
fn skip_escape(bytes: &[u8], i: usize) -> usize {
    let Some(&kind) = bytes.get(i + 1) else {
        return i + 1;
    };
    match kind {
        b'[' => {
            let mut j = i + 2;
            while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                j += 1;
            }
            (j + 1).min(bytes.len())
        }
        b']' | b'P' | b'_' | b'^' | b'X' => {
            let mut j = i + 2;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    return j + 1;
                }
                if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                    return j + 2;
                }
                j += 1;
            }
            j
        }
        // A two-byte escape; the byte after ESC is ASCII here only when it
        // is a real sequence, so stepping one past it stays on a boundary.
        c if c.is_ascii() => i + 2,
        _ => i + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert!(is_secret_key("DB_PASSWORD"));
        assert!(is_secret_key("auth"));
        assert!(is_secret_key("x-auth"));
        assert!(!is_secret_key("author"));
        assert!(!is_secret_key("user"));
    }

    #[test]
    fn offsets_map_through_edits() {
        let edits = vec![Edit {
            start: 2,
            end: 6,
            text: "*".into(),
        }];
        assert_eq!(map_offset(1, &edits), 1);
        assert_eq!(map_offset(4, &edits), 3);
        assert_eq!(map_offset(6, &edits), 3);
        assert_eq!(map_offset(8, &edits), 5);
    }

    #[test]
    fn masks_fit() {
        assert_eq!(fit_mask("********", 3), "***");
        assert_eq!(fit_mask("********", 10), "**********");
        assert_eq!(fit_mask("[x]", 5), "[x]  ");
        assert_eq!(fit_mask("[REDACTED]", 4), "[RED");
    }

    #[test]
    fn escapes_are_skipped() {
        let (visible, offsets) = visible_text("a\x1b[1mb\x1b]8;;u\x1b\\c");
        assert_eq!(visible, "abc");
        assert_eq!(offsets, [0, 5, 14]);
    }
}
