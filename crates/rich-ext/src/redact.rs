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
//!   ([`Redactor::redact_chunks`], or [`Redactor::redact_byte_chunks`] for
//!   raw bytes from a pipe);
//! * command lines ([`Redactor::redact_args`]);
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
//!
//! Hidden text is searched too: the bodies of control strings (an OSC 8
//! hyperlink's URL, a window title, DCS and APC payloads) and a segment's
//! link target. There the mask is the plain one, with control characters
//! made `*`, so the escape stays well formed; masking inside a binary
//! payload (an inline image) can spoil it. Eight-bit C1 controls are read
//! as text, not as escapes.
//!
//! Matching fails closed: when a regex gives up (a user pattern that hits
//! the backtracking limit), the rest of the line is masked. The built-in
//! detectors run in time linear in the line's length.

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
    // Called for every key a line holds, so lowercase once and glob only
    // when a glob could match (every glob entry names `auth`). No entry
    // matches fewer than four characters.
    if key.chars().nth(3).is_none() {
        return false;
    }
    let lower = key.to_lowercase();
    let normalized = lower.replace('-', "_");
    let globs = lower.contains("auth");
    SECRET_KEYS.iter().any(|pattern| {
        if pattern.contains(['*', '?']) {
            globs && crate::env_inspect::name_matches(pattern, &lower)
        } else {
            lower.contains(pattern) || normalized.contains(pattern)
        }
    })
}

/// A built-in secret detector. [`Redactor::secrets`] enables all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detector {
    /// The value in `key=value`, `key: value`, `"key": "value"` or
    /// `--key=value` when the key is secret by [`is_secret_key`]. An
    /// `Authorization: Bearer …` scheme word is kept and only the credential
    /// masked. A value in quotes is masked up to the closing quote, spaces
    /// and commas included (`\"` does not close a `"…"` value); an unquoted
    /// value, or one whose quote never closes, ends at whitespace or one of
    /// `"',;&`.
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
    /// The password in a URL's `user:password@` (`scheme://` required), up
    /// to the last `@` before the host.
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
            // Only the key and its separator: the value is read by
            // `key_value_secret`, and only for a secret key, so a line of
            // ordinary keys is scanned once. A key starts a word (leading
            // dashes of a flag included), so no suffix of a key is retried.
            Detector::KeyValue => concat!(
                r#"(?i)(?<![\w.-])-*(?P<key>[a-z_][\w.-]*)["']?[ \t]*[:=][ \t]*"#,
                r#"(?:(?:bearer|basic|token)[ \t]+)?"#,
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
            // The password runs to the last `@` of the authority (which ends
            // at `/`, `?`, `#` or whitespace), so a password holding `@` is
            // masked whole. A scheme starts a run of scheme characters.
            Detector::UrlCredentials => {
                r"(?i)(?<![a-z0-9+.-])[a-z][a-z0-9+.-]*://[^\s/?#:@]*:(?P<secret>[^\s/?#]+)@"
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
    /// otherwise the whole match is. Its `{kind}` is `pattern`. A pattern
    /// that fails at run time (too much backtracking) masks the rest of the
    /// line rather than none of it; see [`find`](Self::find).
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
    ///
    /// Matching fails closed: when a rule's regex cannot finish (a pattern
    /// that exceeds the backtracking limit, say), everything from where
    /// that search started to the end of the line is masked.
    pub fn find(&self, line: &str) -> Vec<Match> {
        let mut found: Vec<Match> = Vec::new();
        for rule in &self.rules {
            let regex = rule.regex();
            let has_secret = regex.capture_names().any(|n| n == Some("secret"));
            let key_value = matches!(rule, Rule::Detector(Detector::KeyValue));
            // Where a quoted value's search for its closing quote failed:
            // a later search for the same quote fails too.
            let mut unclosed = [usize::MAX; 2];
            let mut pos = 0;
            while pos <= line.len() {
                let captures = match regex.captures_from_pos(line, pos) {
                    Ok(Some(captures)) => captures,
                    Ok(None) => break,
                    Err(_) => {
                        found.push(Match {
                            start: pos,
                            end: line.len(),
                            kind: rule.name().to_string(),
                        });
                        break;
                    }
                };
                let whole = captures.get(0).expect("group 0 always matches");
                let next = if whole.end() > whole.start() {
                    whole.end()
                } else {
                    // An empty match: step past one character.
                    whole.end() + line[whole.end()..].chars().next().map_or(1, char::len_utf8)
                };
                if key_value {
                    // After a secret value, search on after it; otherwise
                    // after the separator, as a value that is not secret
                    // may still hold a key that is (`url=https://x/?token=…`).
                    pos = next;
                    let secret = captures
                        .name("key")
                        .is_some_and(|key| is_secret_key(key.as_str()));
                    if let Some((start, end)) = secret
                        .then(|| key_value_secret(line, whole.end(), &mut unclosed))
                        .flatten()
                    {
                        pos = end;
                        found.push(Match {
                            start,
                            end,
                            kind: rule.name().to_string(),
                        });
                    }
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
    /// still found; the escape sequences themselves are kept, with secrets
    /// in their bodies (an OSC 8 link's URL) masked in place.
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
        let ends = chunk_ends(chunks.iter().map(|c| c.as_ref().len()));
        split_edited(joined.as_bytes(), &edits, &ends)
            .into_iter()
            .map(|bytes| String::from_utf8(bytes).expect("edits keep char boundaries"))
            .collect()
    }

    /// As [`redact_chunks`](Self::redact_chunks) for raw bytes, as read
    /// from a pipe: a character may be split between chunks, and the bytes
    /// need not be valid UTF-8. The stream is decoded once, so a split
    /// character stays whole; rules see each invalid sequence as `U+FFFD`,
    /// and it is kept byte for byte unless a mask covers it. When nothing
    /// matches, the chunks come back unchanged.
    pub fn redact_byte_chunks<B: AsRef<[u8]>>(&self, chunks: &[B]) -> Vec<Vec<u8>> {
        let unchanged = || chunks.iter().map(|c| c.as_ref().to_vec()).collect();
        if self.is_empty() {
            return unchanged();
        }
        let joined: Vec<u8> = chunks.iter().flat_map(|c| c.as_ref()).copied().collect();
        // The decoded text, and for each of its bytes (and its end) the
        // offset in `joined`; a replacement character maps to the start of
        // the invalid bytes it stands for.
        let mut text = String::with_capacity(joined.len());
        let mut origin = Vec::with_capacity(joined.len() + 1);
        let mut at = 0;
        for piece in joined.utf8_chunks() {
            let valid = piece.valid();
            text.push_str(valid);
            origin.extend(at..at + valid.len());
            at += valid.len();
            if !piece.invalid().is_empty() {
                text.push(char::REPLACEMENT_CHARACTER);
                origin.extend([at; 3]);
                at += piece.invalid().len();
            }
        }
        origin.push(at);
        let edits: Vec<Edit> = self
            .ansi_edits(&text)
            .into_iter()
            .map(|edit| Edit {
                start: origin[edit.start],
                end: origin[edit.end],
                text: edit.text,
            })
            .collect();
        if edits.is_empty() {
            return unchanged();
        }
        let ends = chunk_ends(chunks.iter().map(|c| c.as_ref().len()));
        split_edited(&joined, &edits, &ends)
    }

    /// A command line with every match masked word by word. With the
    /// [`KeyValue`](Detector::KeyValue) detector, the value of a flag whose
    /// name is secret by [`is_secret_key`] is masked too, whether it is the
    /// next word (`--token X`) or after `=` (`--api-key=X`). One-letter
    /// flags (`-p`, `-u`) are not: they mean a password in one tool and a
    /// port or a user in the next.
    ///
    /// ```
    /// use rich_ext::redact::Redactor;
    ///
    /// let argv = ["deploy", "--token", "abc123", "--api-key=xyz", "-p", "80"];
    /// assert_eq!(
    ///     Redactor::secrets().redact_args(&argv),
    ///     ["deploy", "--token", "********", "--api-key=********", "-p", "80"]
    /// );
    /// ```
    pub fn redact_args<S: AsRef<str>>(&self, args: &[S]) -> Vec<String> {
        let flags = self
            .rules
            .iter()
            .any(|rule| matches!(rule, Rule::Detector(Detector::KeyValue)));
        let kind = Detector::KeyValue.name();
        let mut out = Vec::with_capacity(args.len());
        let mut mask_next = false;
        for arg in args {
            let arg = arg.as_ref();
            let flag = if flags { secret_flag(arg) } else { None };
            let word = if std::mem::take(&mut mask_next) && !arg.is_empty() {
                self.mask_text(kind, arg, self.preserve_width)
            } else {
                match flag {
                    Some(Some(at)) if at < arg.len() => format!(
                        "{}{}",
                        &arg[..at],
                        self.mask_text(kind, &arg[at..], self.preserve_width)
                    ),
                    _ => self.redact_str(arg),
                }
            };
            mask_next = flag == Some(None);
            out.push(word);
        }
        out
    }

    fn mask_text(&self, kind: &str, original: &str, fit: bool) -> String {
        let mask = self.mask_for(kind);
        if fit {
            fit_mask(&mask, cell_len(original))
        } else {
            mask
        }
    }

    /// `text` (a URL or an escape's body) with every match replaced by the
    /// plain mask, control characters in it made `*` so an escape stays
    /// well formed; `None` when nothing matched.
    fn redact_invisible(&self, text: &str) -> Option<String> {
        let matches = self.find(text);
        if matches.is_empty() {
            return None;
        }
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for m in matches {
            out.push_str(&text[at..m.start]);
            out.push_str(&control_free(&self.mask_for(&m.kind)));
            at = m.end;
        }
        out.push_str(&text[at..]);
        Some(out)
    }

    fn ansi_edits(&self, text: &str) -> Vec<Edit> {
        let mut edits = Vec::new();
        if self.is_empty() {
            return edits;
        }
        let mut line_start = 0;
        for line in text.split('\n') {
            let scan = scan_line(line);
            let (visible, offsets) = (scan.visible, scan.offsets);
            // Control-string bodies (an OSC 8 link's URL, a window title)
            // are not shown but still land in recordings: search them too.
            for (start, end) in scan.bodies {
                let body = &line[start..end];
                for m in self.find(body) {
                    edits.push(Edit {
                        start: line_start + start + m.start,
                        end: line_start + start + m.end,
                        text: control_free(&self.mask_for(&m.kind)),
                    });
                }
            }
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
        edits.sort_by_key(|edit| edit.start);
        edits
    }

    /// Rendered segments with every match masked, line by line.
    ///
    /// Each line keeps its cell width: the mask is fitted to the width of
    /// what it covers and spread over the covered segments, so every part
    /// keeps its segment's style. A hyperlink target (`Style::link`) is
    /// redacted with the plain mask. Control segments are kept, with any
    /// secret in an escape's body (a window title, say) masked.
    pub fn redact_segments(&self, segments: &[Segment]) -> Vec<Segment> {
        if self.is_empty() {
            return segments.to_vec();
        }
        let mut changed = false;
        // Pieces: segment text split at newlines, a newline its own piece.
        let mut lines: Vec<Vec<Segment>> = vec![Vec::new()];
        for segment in segments {
            let redacted = self.redact_segment_escapes(segment);
            changed |= redacted.is_some();
            let segment = redacted.as_ref().unwrap_or(segment);
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

    /// `segment` with its link target, or a control segment's escape
    /// bodies, redacted; `None` when there was nothing to mask.
    fn redact_segment_escapes(&self, segment: &Segment) -> Option<Segment> {
        if segment.control {
            let text = self.redact_ansi(&segment.text);
            return (text != segment.text).then(|| Segment {
                text,
                ..segment.clone()
            });
        }
        let style = segment.style.as_ref()?;
        let link = self.redact_invisible(style.link()?)?;
        Some(Segment::new(
            segment.text.clone(),
            Some(style.update_link(Some(link))),
        ))
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
    let mut taken = 0;
    for &c in mask.iter() {
        let w = char_cell_width(c);
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
        taken += 1;
    }
    mask.drain(..taken);
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

/// The running totals of chunk lengths: where each chunk ends.
fn chunk_ends(lengths: impl Iterator<Item = usize>) -> Vec<usize> {
    lengths
        .scan(0, |total, len| {
            *total += len;
            Some(*total)
        })
        .collect()
}

/// `text` with `edits` applied, cut where the original chunks ended
/// (`ends`, ascending). A chunk end inside an edit lands after its
/// replacement, so a mask goes in the chunk where the secret starts. One
/// pass over the edits.
fn split_edited(text: &[u8], edits: &[Edit], ends: &[usize]) -> Vec<Vec<u8>> {
    let mut edited = Vec::with_capacity(text.len());
    let mut at = 0;
    for edit in edits {
        edited.extend_from_slice(&text[at..edit.start]);
        edited.extend_from_slice(edit.text.as_bytes());
        at = edit.end;
    }
    edited.extend_from_slice(&text[at..]);
    let mut out = Vec::with_capacity(ends.len());
    let (mut next, mut shift, mut from) = (0, 0isize, 0);
    for &end in ends {
        let to = loop {
            match edits.get(next) {
                Some(edit) if edit.start < end => {
                    if edit.end > end {
                        break (edit.start as isize + shift) as usize + edit.text.len();
                    }
                    shift += edit.text.len() as isize - (edit.end - edit.start) as isize;
                    next += 1;
                }
                _ => break (end as isize + shift) as usize,
            }
        };
        let to = to.max(from);
        out.push(edited[from..to].to_vec());
        from = to;
    }
    out
}

/// `mask` with control characters (which could end or corrupt an escape
/// sequence it is written into) replaced by `*`.
fn control_free(mask: &str) -> String {
    mask.chars()
        .map(|c| if c.is_control() { '*' } else { c })
        .collect()
}

/// The secret value of a secret key whose separator ends at `at`, as a
/// byte range of `line`. A quoted value runs to its closing quote (a
/// backslash escapes the next character in `"…"`); an unquoted value, or
/// one whose quote never closes, to whitespace or one of `"',;&`.
/// `unclosed` remembers, per quote, where a search for the closing quote
/// found none, so no stretch of the line is searched twice.
fn key_value_secret(line: &str, at: usize, unclosed: &mut [usize; 2]) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut start = at;
    if let Some(&quote @ (b'"' | b'\'')) = bytes.get(at) {
        start = at + 1;
        let slot = usize::from(quote == b'\'');
        if start < unclosed[slot] {
            let mut i = start;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' if quote == b'"' => i += 2,
                    b if b == quote => return (i > start).then_some((start, i)),
                    _ => i += 1,
                }
            }
            unclosed[slot] = start;
        }
    }
    let end = line[start..]
        .find(|c: char| c.is_whitespace() || "\"',;&".contains(c))
        .map_or(line.len(), |len| start + len);
    (end > start).then_some((start, end))
}

/// For a command-line word that is a flag named like a secret key: `Some`
/// with the offset of its value after `=`, or `Some(None)` when the value
/// is the next word.
fn secret_flag(word: &str) -> Option<Option<usize>> {
    let name = word.strip_prefix('-')?.trim_start_matches('-');
    let (name, value) = match name.find('=') {
        Some(eq) => (&name[..eq], Some(word.len() - name.len() + eq + 1)),
        None => (name, None),
    };
    let valid = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    (valid && is_secret_key(name)).then_some(value)
}

/// A line split into what a terminal shows and what it does not.
struct Scan {
    /// The text without escape sequences.
    visible: String,
    /// For each byte of `visible`, its offset in the line.
    offsets: Vec<usize>,
    /// The bodies of control strings (OSC, DCS, SOS, PM, APC), as byte
    /// ranges of the line.
    bodies: Vec<(usize, usize)>,
}

fn scan_line(line: &str) -> Scan {
    let bytes = line.as_bytes();
    let mut scan = Scan {
        visible: String::with_capacity(line.len()),
        offsets: Vec::with_capacity(line.len()),
        bodies: Vec::new(),
    };
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            let (end, body) = skip_escape(bytes, i);
            scan.bodies.extend(body.filter(|(start, end)| start < end));
            i = end;
            continue;
        }
        let c = line[i..].chars().next().expect("char boundary");
        let len = c.len_utf8();
        scan.visible.push(c);
        scan.offsets.extend(i..i + len);
        i += len;
    }
    scan
}

/// The ECMA-48 escape sequence starting with the ESC at `i`: the index
/// after it and, for a control string, the byte range of its body.
///
/// * CSI (`ESC [`): parameter and intermediate bytes (0x20–0x3F), then a
///   final byte (0x40–0x7E).
/// * OSC (`ESC ]`), ended by ST (`ESC \`) or BEL; DCS (`ESC P`), SOS
///   (`ESC X`), PM (`ESC ^`) and APC (`ESC _`), ended by ST. CAN, SUB or
///   an ESC that does not start ST cut a string short, as in a terminal;
///   an unended string runs to the end of the line.
/// * Other escapes: intermediate bytes (0x20–0x2F, as in `ESC ( B`), then
///   a final byte (0x30–0x7E).
///
/// A sequence cut short by a byte it cannot hold (a control, DEL,
/// non-ASCII) ends before that byte, which is then read as text; an ESC
/// followed by such a byte, or by nothing, is a lone ESC. Eight-bit C1
/// controls (U+0080–U+009F) are not read as escapes: they are text, so
/// what follows them is searched like any other text.
fn skip_escape(bytes: &[u8], i: usize) -> (usize, Option<(usize, usize)>) {
    let Some(&kind) = bytes.get(i + 1) else {
        return (i + 1, None);
    };
    match kind {
        b'[' => {
            let mut j = i + 2;
            while let Some(&b) = bytes.get(j) {
                match b {
                    0x20..=0x3f => j += 1,
                    0x40..=0x7e => return (j + 1, None),
                    _ => break,
                }
            }
            (j, None)
        }
        b']' | b'P' | b'X' | b'^' | b'_' => {
            let start = i + 2;
            let mut j = start;
            while let Some(&b) = bytes.get(j) {
                match b {
                    0x07 if kind == b']' => return (j + 1, Some((start, j))),
                    0x1b if bytes.get(j + 1) == Some(&b'\\') => return (j + 2, Some((start, j))),
                    0x1b | 0x18 | 0x1a => return (j, Some((start, j))),
                    _ => j += 1,
                }
            }
            (j, Some((start, j)))
        }
        0x20..=0x2f => {
            let mut j = i + 1;
            while let Some(&b) = bytes.get(j) {
                match b {
                    0x20..=0x2f => j += 1,
                    0x30..=0x7e => return (j + 1, None),
                    _ => break,
                }
            }
            (j, None)
        }
        0x30..=0x7e => (i + 2, None),
        _ => (i + 1, None),
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
    fn chunk_ends_map_through_edits() {
        let edits = vec![Edit {
            start: 2,
            end: 6,
            text: "*".into(),
        }];
        // Chunk ends before, inside, at the end of and after the edit.
        assert_eq!(
            split_edited(b"abcdefgh", &edits, &[1, 4, 6, 8]),
            [&b"a"[..], b"b*", b"", b"gh"]
        );
        assert_eq!(chunk_ends([1, 3, 0].into_iter()), [1, 4, 4]);
    }

    #[test]
    fn key_values_and_flags() {
        let mut unclosed = [usize::MAX; 2];
        assert_eq!(
            key_value_secret("k=\"a b\" c", 2, &mut unclosed),
            Some((3, 6))
        );
        assert_eq!(key_value_secret("k=\"\"", 2, &mut unclosed), None);
        assert_eq!(key_value_secret("k=\"a b", 2, &mut unclosed), Some((3, 4)));
        assert_eq!(unclosed, [3, usize::MAX]);
        assert_eq!(secret_flag("--token"), Some(None));
        assert_eq!(secret_flag("--api-key=x"), Some(Some(10)));
        assert_eq!(secret_flag("-p"), None);
        assert_eq!(secret_flag("--"), None);
        assert_eq!(secret_flag("--author"), None);
        assert_eq!(secret_flag("token"), None);
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
        let scan = scan_line("a\x1b[1mb\x1b]8;;u\x1b\\c");
        assert_eq!(scan.visible, "abc");
        assert_eq!(scan.offsets, [0, 5, 14]);
        assert_eq!(scan.bodies, [(8, 12)]);
        // Input, end of the escape, body.
        type Case = (&'static [u8], usize, Option<(usize, usize)>);
        let cases: [Case; 14] = [
            (b"\x1b", 1, None),
            (b"\x1b[", 2, None),
            (b"\x1b[?25l", 6, None),
            (b"\x1b[1 q", 5, None),
            (b"\x1b[31\x1bx", 4, None),
            (b"\x1b(B", 3, None),
            (b"\x1b$(Bx", 4, None),
            (b"\x1b(\x07", 2, None),
            (b"\x1b7x", 2, None),
            (b"\x1b\x1b", 1, None),
            (b"\x1b]0;t\x07x", 6, Some((2, 5))),
            (b"\x1bPq\x07x\x1b\\", 7, Some((2, 5))),
            (b"\x1b_a\x1b[m", 3, Some((2, 3))),
            (b"\x1b^a\x18b", 3, Some((2, 3))),
        ];
        for (bytes, end, body) in cases {
            assert_eq!(skip_escape(bytes, 0), (end, body), "{bytes:?}");
        }
    }
}
