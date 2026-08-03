//! Built-in highlighters.
//!
//! Port of upstream `rich/highlighter.py` — the [`RegexHighlighter`] base and
//! the [`ReprHighlighter`] / [`ISO8601Highlighter`] built-ins. Patterns
//! (`repr_patterns.rs` for repr) use lookbehind/alternation, so we compile them
//! with `fancy-regex`.
//!
//! Like upstream, a highlighter stylizes **every** matched named group with the
//! style named `{base_style}{group}`; unknown names resolve to a null style,
//! which still splits the text into spans (so e.g. an ISO date's sub-fields
//! create the same segment boundaries upstream produces).

use std::sync::OnceLock;

use fancy_regex::Regex;

use crate::protocol::Highlighter;
use crate::repr_patterns::REPR_PATTERNS;
use crate::style::Style;
use crate::text::Text;

/// The full ISO 8601 pattern set from upstream's `ISO8601Highlighter.highlights`,
/// in the same order (only `iso8601.date`/`time`/`timezone` carry a visible
/// style; the sub-groups split segments but resolve to null, as upstream). The
/// only rewrite: upstream's single conditional pattern (`(?(hyphen)…)`, which
/// `fancy-regex` can't compile) is split into two non-conditional alternatives —
/// the all-hyphen/colon form and the all-basic form — which together match
/// exactly the same strings the conditional does.
const ISO8601_PATTERNS: &[&str] = &[
    // Year-month (no visible style: year/month resolve to null).
    r"^(?P<year>[0-9]{4})-(?P<month>1[0-2]|0[1-9])$",
    // Basic (compact) calendar date, e.g. 20230615.
    r"^(?P<date>(?P<year>[0-9]{4})(?P<month>1[0-2]|0[1-9])(?P<day>3[01]|0[1-9]|[12][0-9]))$",
    // Ordinal date, e.g. 2023-166 / 2023166.
    r"^(?P<date>(?P<year>[0-9]{4})-?(?P<day>36[0-6]|3[0-5][0-9]|[12][0-9]{2}|0[1-9][0-9]|00[1-9]))$",
    // Week date, e.g. 2023-W36 / 2023W36.
    r"^(?P<date>(?P<year>[0-9]{4})-?W(?P<week>5[0-3]|[1-4][0-9]|0[1-9]))$",
    // Week date with weekday, e.g. 2023-W36-7 / 2023W367.
    r"^(?P<date>(?P<year>[0-9]{4})-?W(?P<week>5[0-3]|[1-4][0-9]|0[1-9])-?(?P<day>[1-7]))$",
    // Basic/extended time hh[:]mm.
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9]):?(?P<minute>[0-5][0-9]))$",
    // Basic time hhmmss.
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9])(?P<minute>[0-5][0-9])(?P<second>[0-5][0-9]))$",
    // Timezone alone.
    r"^(?P<timezone>(Z|[+-](?:2[0-3]|[01][0-9])(?::?(?:[0-5][0-9]))?))$",
    // Basic time hhmmss with timezone.
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9])(?P<minute>[0-5][0-9])(?P<second>[0-5][0-9]))(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9])(?::?(?:[0-5][0-9]))?)$",
    // Space-separated date + time — extended form (upstream conditional, part 1).
    r"^(?P<date>(?P<year>[0-9]{4})-(?P<month>1[0-2]|0[1-9])-(?P<day>3[01]|0[1-9]|[12][0-9])) (?P<time>(?P<hour>2[0-3]|[01][0-9]):(?P<minute>[0-5][0-9]):(?P<second>[0-5][0-9]))$",
    // Space-separated date + time — basic form (upstream conditional, part 2).
    r"^(?P<date>(?P<year>[0-9]{4})(?P<month>1[0-2]|0[1-9])(?P<day>3[01]|0[1-9]|[12][0-9])) (?P<time>(?P<hour>2[0-3]|[01][0-9])(?P<minute>[0-5][0-9])(?P<second>[0-5][0-9]))$",
    // Extended calendar date with optional timezone.
    r"^(?P<date>(?P<year>-?(?:[1-9][0-9]*)?[0-9]{4})-(?P<month>1[0-2]|0[1-9])-(?P<day>3[01]|0[1-9]|[12][0-9]))(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9]):[0-5][0-9])?$",
    // Extended clock time (with optional fraction) and timezone.
    r"^(?P<time>(?P<hour>2[0-3]|[01][0-9]):(?P<minute>[0-5][0-9]):(?P<second>[0-5][0-9])(?P<frac>\.[0-9]+)?)(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9]):[0-5][0-9])?$",
    // Full extended date-time (T-separated) with optional fraction and timezone.
    r"^(?P<date>(?P<year>-?(?:[1-9][0-9]*)?[0-9]{4})-(?P<month>1[0-2]|0[1-9])-(?P<day>3[01]|0[1-9]|[12][0-9]))T(?P<time>(?P<hour>2[0-3]|[01][0-9]):(?P<minute>[0-5][0-9]):(?P<second>[0-5][0-9])(?P<ms>\.[0-9]+)?)(?P<timezone>Z|[+-](?:2[0-3]|[01][0-9]):[0-5][0-9])?$",
];

/// The default style for a fully-qualified style name (the subset of
/// `default_styles.py` used by the built-in highlighters). Unknown names get a
/// null style — matching upstream's `get_style(name, default=Style.null())`.
fn default_style(name: &str) -> Style {
    // Resolved from the single upstream style table rather than a second copy
    // kept here — the two used to drift (the local copy was right, `theme.rs`'s
    // was lossy). An unknown name yields a null style, matching upstream's
    // `get_style(name, default=Style.null())`.
    crate::theme::Theme::default_shared()
        .get(name)
        .cloned()
        .unwrap_or_default()
}

/// Compile a slice of pattern strings.
fn compile(patterns: &[&str]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|pattern| Regex::new(pattern).expect("valid highlighter pattern"))
        .collect()
}

/// Apply `patterns` to `text`, stylizing each matched named group with
/// `{base_style}{group}`. Port of `RegexHighlighter.highlight`.
fn run_highlighter(text: &mut Text, base_style: &str, patterns: &[Regex]) {
    let plain = text.plain().to_string();
    for regex in patterns {
        let names: Vec<&str> = regex.capture_names().flatten().collect();
        for captures in regex.captures_iter(&plain) {
            let Ok(captures) = captures else { break };
            for &name in &names {
                if let Some(matched) = captures.name(name) {
                    let style = default_style(&format!("{base_style}{name}"));
                    text.stylize(matched.start(), matched.end(), style);
                }
            }
        }
    }
}

/// A generic regex highlighter — the extension point for custom highlighters.
/// Give it a `base_style` prefix and named-group patterns; each matched group is
/// styled with the built-in style named `{base_style}{group}`. Mirrors
/// `rich.highlighter.RegexHighlighter`. (Theme-driven custom style names are a
/// follow-up — see the Theme issue.)
pub struct RegexHighlighter {
    base_style: String,
    patterns: Vec<Regex>,
}

impl RegexHighlighter {
    pub fn new(base_style: impl Into<String>, patterns: &[&str]) -> Self {
        RegexHighlighter {
            base_style: base_style.into(),
            patterns: compile(patterns),
        }
    }
}

impl Highlighter for RegexHighlighter {
    fn highlight(&self, text: &mut Text) {
        run_highlighter(text, &self.base_style, &self.patterns);
    }
}

/// Highlights repr-style output — numbers, strings, bools, `None`, paths, URLs,
/// braces, calls, IP/UUID/EUI, and tags. Mirrors `rich.highlighter.ReprHighlighter`.
#[derive(Default)]
pub struct ReprHighlighter;

impl ReprHighlighter {
    pub fn new() -> Self {
        ReprHighlighter
    }
}

fn repr_patterns() -> &'static [Regex] {
    static COMPILED: OnceLock<Vec<Regex>> = OnceLock::new();
    COMPILED.get_or_init(|| compile(&REPR_PATTERNS))
}

impl Highlighter for ReprHighlighter {
    fn highlight(&self, text: &mut Text) {
        run_highlighter(text, "repr.", repr_patterns());
    }
}

/// Highlights ISO 8601 date/time strings (`iso8601.date`/`time`/`timezone`).
/// Mirrors `rich.highlighter.ISO8601Highlighter` for standard extended formats.
#[derive(Default)]
pub struct ISO8601Highlighter;

impl ISO8601Highlighter {
    pub fn new() -> Self {
        ISO8601Highlighter
    }
}

fn iso8601_patterns() -> &'static [Regex] {
    static COMPILED: OnceLock<Vec<Regex>> = OnceLock::new();
    COMPILED.get_or_init(|| compile(ISO8601_PATTERNS))
}

impl Highlighter for ISO8601Highlighter {
    fn highlight(&self, text: &mut Text) {
        run_highlighter(text, "iso8601.", iso8601_patterns());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::console::Console;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .no_color(false)
            .build()
    }

    fn highlight(input: &str) -> String {
        let mut text = Text::new(input);
        ReprHighlighter::new().highlight(&mut text);
        console().render_to_string(&text)
    }

    fn highlight_iso(input: &str) -> String {
        let mut text = Text::new(input);
        ISO8601Highlighter::new().highlight(&mut text);
        console().render_to_string(&text)
    }

    #[test]
    fn highlights_numbers_bools_none() {
        // Captured from real rich 15.0.0 Console(highlight=True).
        assert_eq!(
            highlight("value = 42 and 3.14"),
            "value = \x1b[1;36m42\x1b[0m and \x1b[1;36m3.14\x1b[0m"
        );
        assert_eq!(
            highlight("flag is True, x is None"),
            "flag is \x1b[3;92mTrue\x1b[0m, x is \x1b[3;35mNone\x1b[0m"
        );
    }

    #[test]
    fn highlights_paths_strings_urls_and_more() {
        // All captured from real rich 15.0.0.
        assert_eq!(
            highlight("path /usr/bin done"),
            "path \x1b[35m/usr/\x1b[0m\x1b[95mbin\x1b[0m done"
        );
        assert_eq!(
            highlight("s = 'hello world' end"),
            "s = \x1b[32m'hello world'\x1b[0m end"
        );
        assert_eq!(
            highlight("see https://example.com/x now"),
            "see \x1b[4;94mhttps://example.com/x\x1b[0m now"
        );
        assert_eq!(
            highlight("list [1, 2, 3] and (a, b)"),
            "list \x1b[1m[\x1b[0m\x1b[1;36m1\x1b[0m, \x1b[1;36m2\x1b[0m, \x1b[1;36m3\x1b[0m\x1b[1m]\x1b[0m and \x1b[1m(\x1b[0ma, b\x1b[1m)\x1b[0m"
        );
        assert_eq!(
            highlight("call func(x) here"),
            "call \x1b[1;35mfunc\x1b[0m\x1b[1m(\x1b[0mx\x1b[1m)\x1b[0m here"
        );
        assert_eq!(
            highlight("id 12345678-1234-1234-1234-123456789abc x"),
            "id \x1b[93m12345678-1234-1234-1234-123456789abc\x1b[0m x"
        );
        assert_eq!(
            highlight("ip 192.168.0.1 addr"),
            "ip \x1b[1;92m192.168.0.1\x1b[0m addr"
        );
        // "3:4" matches the ipv6 pattern in upstream; "..." is the ellipsis.
        assert_eq!(
            highlight("ratio 3:4 and dots ..."),
            "ratio \x1b[1;92m3:4\x1b[0m and dots \x1b[33m...\x1b[0m"
        );
    }

    #[test]
    fn iso8601_dates_times_and_zones() {
        // All captured from real rich 15.0.0 ISO8601Highlighter. Sub-fields
        // (year/month/…) split the run into per-field segments even though only
        // date/time/timezone carry color.
        assert_eq!(
            highlight_iso("2023-06-15"),
            "\x1b[34m2023\x1b[0m\x1b[34m-\x1b[0m\x1b[34m06\x1b[0m\x1b[34m-\x1b[0m\x1b[34m15\x1b[0m"
        );
        assert_eq!(
            highlight_iso("13:45:30"),
            "\x1b[35m13\x1b[0m\x1b[35m:\x1b[0m\x1b[35m45\x1b[0m\x1b[35m:\x1b[0m\x1b[35m30\x1b[0m"
        );
        assert_eq!(
            highlight_iso("2023-06-15T13:45:30.123+02:00"),
            "\x1b[34m2023\x1b[0m\x1b[34m-\x1b[0m\x1b[34m06\x1b[0m\x1b[34m-\x1b[0m\x1b[34m15\x1b[0mT\
             \x1b[35m13\x1b[0m\x1b[35m:\x1b[0m\x1b[35m45\x1b[0m\x1b[35m:\x1b[0m\x1b[35m30\x1b[0m\x1b[35m.123\x1b[0m\x1b[33m+02:00\x1b[0m"
        );
        assert_eq!(highlight_iso("not a date"), "not a date");
    }

    #[test]
    fn iso8601_compact_and_basic_forms() {
        // Compact/basic forms and the split of upstream's conditional pattern.
        // All captured from real rich 15.0.0 ISO8601Highlighter.
        assert_eq!(
            highlight_iso("20230615"),
            "\x1b[34m2023\x1b[0m\x1b[34m06\x1b[0m\x1b[34m15\x1b[0m"
        );
        assert_eq!(
            highlight_iso("2023-166"),
            "\x1b[34m2023\x1b[0m\x1b[34m-\x1b[0m\x1b[34m166\x1b[0m"
        );
        assert_eq!(
            highlight_iso("2023-W36"),
            "\x1b[34m2023\x1b[0m\x1b[34m-W\x1b[0m\x1b[34m36\x1b[0m"
        );
        assert_eq!(
            highlight_iso("2023-W36-7"),
            "\x1b[34m2023\x1b[0m\x1b[34m-W\x1b[0m\x1b[34m36\x1b[0m\x1b[34m-\x1b[0m\x1b[34m7\x1b[0m"
        );
        assert_eq!(highlight_iso("1345"), "\x1b[35m13\x1b[0m\x1b[35m45\x1b[0m");
        assert_eq!(
            highlight_iso("134530"),
            "\x1b[35m13\x1b[0m\x1b[35m45\x1b[0m\x1b[35m30\x1b[0m"
        );
        assert_eq!(highlight_iso("Z"), "\x1b[33mZ\x1b[0m");
        assert_eq!(highlight_iso("+02:00"), "\x1b[33m+02:00\x1b[0m");
        assert_eq!(
            highlight_iso("153000Z"),
            "\x1b[35m15\x1b[0m\x1b[35m30\x1b[0m\x1b[35m00\x1b[0m\x1b[33mZ\x1b[0m"
        );
        // The split conditional pattern: extended and basic space-separated forms.
        assert_eq!(
            highlight_iso("2023-06-15 13:45:30"),
            "\x1b[34m2023\x1b[0m\x1b[34m-\x1b[0m\x1b[34m06\x1b[0m\x1b[34m-\x1b[0m\x1b[34m15\x1b[0m \
             \x1b[35m13\x1b[0m\x1b[35m:\x1b[0m\x1b[35m45\x1b[0m\x1b[35m:\x1b[0m\x1b[35m30\x1b[0m"
        );
        assert_eq!(
            highlight_iso("20230615 134530"),
            "\x1b[34m2023\x1b[0m\x1b[34m06\x1b[0m\x1b[34m15\x1b[0m \
             \x1b[35m13\x1b[0m\x1b[35m45\x1b[0m\x1b[35m30\x1b[0m"
        );
    }
}
