//! What a [`Redactor`] would change, shown before anything is written
//! (#722): [`Redactor::preview`], [`Redactor::preview_ansi`] and
//! [`Redactor::preview_args`] list every match with its rule, where it is,
//! the text it covers and its replacement, and [`Preview`] draws them as
//! the lines they are on, each match highlighted, with a summary.
//!
//! A preview shows the secrets it found, unmasked: it is for the person
//! about to share the output, not for sharing itself.

use std::collections::BTreeMap;

use rich::cells::cell_len;
use rich::text::Text;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::{control_free, scan_line, secret_flag, Detector, Match, Redactor, Rule};

/// One span a [`Redactor`] would mask, found by a preview.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Finding {
    /// The line it is on, from 0.
    pub line: usize,
    /// The cell column it starts at, from 0.
    pub column: usize,
    /// Its start in the line's visible text, in bytes.
    pub start: usize,
    /// Its end (exclusive) in the line's visible text, in bytes. For a
    /// [`hidden`](Self::hidden) finding, `start == end`: where the escape
    /// sequence holding it sits.
    pub end: usize,
    /// The rule that matched: a [`Detector::name`] or a pattern's name.
    pub kind: String,
    /// The text that would be masked.
    pub original: String,
    /// What would replace it.
    pub replacement: String,
    /// Whether it is inside an escape sequence (an OSC 8 link's URL, a
    /// window title) rather than in the visible text.
    pub hidden: bool,
}

/// Every match a [`Redactor`] finds in a text, and the lines they are on.
///
/// Rendered, each line with a match is shown with its number, the matches
/// highlighted, and one line per match naming its rule and replacement,
/// then a summary line ([`Preview::summary`]).
///
/// ```
/// use rich_ext::redact::Redactor;
///
/// let preview = Redactor::secrets().preview("user=ann\npassword=hunter2\n");
/// let finding = &preview.findings()[0];
/// assert_eq!((finding.line, finding.column), (1, 9));
/// assert_eq!((finding.kind.as_str(), finding.original.as_str()), ("key-value", "hunter2"));
/// assert_eq!(finding.replacement, "********");
/// assert_eq!(preview.summary(), "1 match on 1 of 2 lines (key-value 1)");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    title: Option<String>,
    findings: Vec<Finding>,
    /// The visible text of each line with a finding, by line number.
    lines: BTreeMap<usize, String>,
    total_lines: usize,
}

impl Preview {
    /// A heading drawn above the lines (`"output"`, `"command"`), and the
    /// summary's first word.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Every finding, in line order and, on a line, left to right.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Whether nothing would be masked.
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// The number of lines previewed.
    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    /// The number of lines with at least one finding.
    pub fn lines_affected(&self) -> usize {
        self.lines.len()
    }

    /// The visible text of line `index`, when it has a finding.
    pub fn line(&self, index: usize) -> Option<&str> {
        self.lines.get(&index).map(String::as_str)
    }

    /// How many findings each rule made, in the order the rules first
    /// matched.
    pub fn counts(&self) -> Vec<(String, usize)> {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for finding in &self.findings {
            match counts.iter_mut().find(|(kind, _)| *kind == finding.kind) {
                Some((_, count)) => *count += 1,
                None => counts.push((finding.kind.clone(), 1)),
            }
        }
        counts
    }

    /// One line: `3 matches on 2 of 40 lines (key-value 2, token 1)`, or
    /// `no matches in 40 lines`; after the title and a colon when there is
    /// one.
    pub fn summary(&self) -> String {
        let plural = |n: usize, word: &str| {
            if n == 1 {
                format!("{n} {word}")
            } else if word == "match" {
                format!("{n} matches")
            } else {
                format!("{n} {word}s")
            }
        };
        let body = if self.findings.is_empty() {
            format!("no matches in {}", plural(self.total_lines, "line"))
        } else {
            let counts: Vec<String> = self
                .counts()
                .into_iter()
                .map(|(kind, n)| format!("{kind} {n}"))
                .collect();
            format!(
                "{} on {} of {} ({})",
                plural(self.findings.len(), "match"),
                self.lines.len(),
                plural(self.total_lines, "line"),
                counts.join(", ")
            )
        };
        match &self.title {
            Some(title) => format!("{title}: {body}"),
            None => body,
        }
    }

    fn push(&mut self, line: usize, visible: &str, finding: Finding) {
        self.lines
            .entry(line)
            .or_insert_with(|| visible.to_string());
        self.findings.push(finding);
    }

    fn sort(&mut self) {
        self.findings
            .sort_by_key(|f| (f.line, f.start, f.hidden, f.end));
    }

    fn text(&self) -> Text {
        let location = Style::parse("dim").expect("valid style");
        let highlight = Style::parse("bold reverse red").expect("valid style");
        let kind_style = Style::parse("cyan").expect("valid style");
        let replacement = Style::parse("green").expect("valid style");
        let safe = crate::sanitize_terminal_controls;
        let digits = self
            .lines
            .keys()
            .last()
            .map_or(1, |last| (last + 1).to_string().len());
        let mut text = Text::new("");
        if let Some(title) = &self.title {
            text.append(
                &safe(title),
                Some(Style::parse("bold").expect("valid").into()),
            );
            text.append("\n", None);
        }
        for (&number, line) in &self.lines {
            text.append(
                &format!("{:>digits$} │ ", number + 1),
                Some(location.clone().into()),
            );
            let mut at = 0;
            let findings: Vec<&Finding> =
                self.findings.iter().filter(|f| f.line == number).collect();
            for finding in findings.iter().filter(|f| !f.hidden) {
                if finding.start < at {
                    continue;
                }
                text.append(&safe(&line[at..finding.start]), None);
                text.append(
                    &safe(&line[finding.start..finding.end]),
                    Some(highlight.clone().into()),
                );
                at = finding.end;
            }
            text.append(&safe(&line[at..]), None);
            text.append("\n", None);
            for finding in findings {
                text.append(
                    &format!("{:>digits$} ╰ {}:{} ", "", number + 1, finding.column + 1),
                    Some(location.clone().into()),
                );
                text.append(&safe(&finding.kind), Some(kind_style.clone().into()));
                if finding.hidden {
                    text.append(" (in an escape sequence)", Some(location.clone().into()));
                }
                text.append(" ", None);
                text.append(&safe(&finding.original), Some(highlight.clone().into()));
                text.append(" → ", Some(location.clone().into()));
                text.append(
                    &safe(&finding.replacement),
                    Some(replacement.clone().into()),
                );
                text.append("\n", None);
            }
        }
        text.append(&safe(&self.summary()), Some(location.into()));
        text
    }
}

impl Renderable for Preview {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.text().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.text().measure(console, options)
    }
}

/// The number of lines in `text`, a final line break ending the last line
/// rather than starting another.
fn line_count(text: &str) -> usize {
    let lines = text.split('\n').count();
    if text.ends_with('\n') {
        lines - 1
    } else {
        lines
    }
}

impl Redactor {
    fn finding(&self, line: usize, visible: &str, m: Match) -> Finding {
        let original = visible[m.start..m.end].to_string();
        Finding {
            line,
            column: cell_len(&visible[..m.start]),
            start: m.start,
            end: m.end,
            replacement: self.replacement(&m, &original, self.preserve_width),
            kind: m.kind,
            original,
            hidden: false,
        }
    }

    /// What [`redact_str`](Self::redact_str) would mask in `text`, line by
    /// line, without masking it.
    pub fn preview(&self, text: &str) -> Preview {
        let mut preview = Preview {
            total_lines: line_count(text),
            ..Preview::default()
        };
        for (index, line) in text.split('\n').enumerate() {
            for m in self.find(line) {
                let finding = self.finding(index, line, m);
                preview.push(index, line, finding);
            }
        }
        preview
    }

    /// What [`redact_ansi`](Self::redact_ansi) would mask in `text`, which
    /// holds ANSI escape sequences. Positions are in each line's visible
    /// text (escapes removed, which is what [`Preview::line`] holds); a
    /// secret in an escape's body (an OSC 8 link's URL) is a
    /// [`hidden`](Finding::hidden) finding at the escape's position.
    pub fn preview_ansi(&self, text: &str) -> Preview {
        let mut preview = Preview {
            total_lines: line_count(text),
            ..Preview::default()
        };
        if self.is_empty() {
            return preview;
        }
        for (index, line) in text.split('\n').enumerate() {
            let scan = scan_line(line);
            for (start, end) in &scan.bodies {
                let body = &line[*start..*end];
                // Where the escape sits in the visible text.
                let at = scan.offsets.partition_point(|&offset| offset < *start);
                for m in self.find(body) {
                    let finding = Finding {
                        line: index,
                        column: cell_len(&scan.visible[..at]),
                        start: at,
                        end: at,
                        original: body[m.start..m.end].to_string(),
                        replacement: control_free(&self.mask_for(&m.kind)),
                        kind: m.kind,
                        hidden: true,
                    };
                    preview.push(index, &scan.visible, finding);
                }
            }
            for m in self.find(&scan.visible) {
                let finding = self.finding(index, &scan.visible, m);
                preview.push(index, &scan.visible, finding);
            }
        }
        preview.sort();
        preview
    }

    /// What [`redact_args`](Self::redact_args) would mask in a command
    /// line, as one line: the words joined by spaces. A secret-named flag's
    /// value (`--token X`, `--api-key=X`) is a
    /// [`key-value`](Detector::KeyValue) finding, as there.
    pub fn preview_args<S: AsRef<str>>(&self, args: &[S]) -> Preview {
        let flags = self
            .rules
            .iter()
            .any(|rule| matches!(rule, Rule::Detector(Detector::KeyValue)));
        let kind = Detector::KeyValue.name();
        let line = args.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" ");
        let mut preview = Preview {
            total_lines: 1,
            ..Preview::default()
        };
        let mut offset = 0;
        let mut mask_next = false;
        for arg in args {
            let arg = arg.as_ref();
            let flag = if flags { secret_flag(arg) } else { None };
            let whole = |start: usize| Match {
                start: offset + start,
                end: offset + arg.len(),
                kind: kind.to_string(),
            };
            let found = if std::mem::take(&mut mask_next) && !arg.is_empty() {
                vec![whole(0)]
            } else {
                match flag {
                    Some(Some(at)) if at < arg.len() => vec![whole(at)],
                    _ => self
                        .find(arg)
                        .into_iter()
                        .map(|m| Match {
                            start: offset + m.start,
                            end: offset + m.end,
                            kind: m.kind,
                        })
                        .collect(),
                }
            };
            for m in found {
                let finding = self.finding(0, &line, m);
                preview.push(0, &line, finding);
            }
            mask_next = flag == Some(None);
            offset += arg.len() + 1;
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_preview_matches_redact_str() {
        let redactor = Redactor::secrets();
        let text = "a\nGITHUB_TOKEN=ghp_0123456789abcdefghijklmnopqrstuvwxyzAB x=1 password=pw\nb";
        let preview = redactor.preview(text);
        assert_eq!(preview.total_lines(), 3);
        assert_eq!(preview.lines_affected(), 1);
        // Applying the findings gives what redact_str gives.
        let line = preview.line(1).unwrap();
        let mut out = String::new();
        let mut at = 0;
        for f in preview.findings() {
            out.push_str(&line[at..f.start]);
            out.push_str(&f.replacement);
            at = f.end;
        }
        out.push_str(&line[at..]);
        assert_eq!(out, redactor.redact_str(text).lines().nth(1).unwrap());
        assert_eq!(preview.counts(), [("key-value".to_string(), 2)]);
    }

    #[test]
    fn ansi_preview_uses_visible_positions_and_finds_hidden_secrets() {
        let redactor = Redactor::secrets();
        let text = "\x1b[31mtoken=\x1b[1mabc123\x1b[0m \x1b]8;;https://u:pw@host/\x1b\\link\x1b]8;;\x1b\\\n";
        let preview = redactor.preview_ansi(text);
        assert_eq!(preview.total_lines(), 1);
        assert_eq!(preview.line(0), Some("token=abc123 link"));
        let found: Vec<_> = preview
            .findings()
            .iter()
            .map(|f| (f.column, f.original.as_str(), f.hidden))
            .collect();
        assert_eq!(found, [(6, "abc123", false), (13, "pw", true)]);
    }

    #[test]
    fn args_preview_follows_redact_args() {
        let redactor = Redactor::secrets().preserve_width(true);
        let argv = ["deploy", "--token", "abc123", "--api-key=xyz", "-p", "80"];
        let preview = redactor.preview_args(&argv);
        let found: Vec<_> = preview
            .findings()
            .iter()
            .map(|f| (f.column, f.original.as_str(), f.replacement.as_str()))
            .collect();
        assert_eq!(found, [(15, "abc123", "******"), (32, "xyz", "***")]);
    }

    #[test]
    fn renders_highlighted_lines_and_a_summary() {
        let console = Console::builder().width(60).build();
        let preview = Redactor::secrets()
            .preview("ok\npassword=hunter2\n")
            .title("output");
        let shown = console.render_to_string(&preview);
        assert_eq!(
            shown,
            "output\n2 │ password=hunter2\n  ╰ 2:10 key-value hunter2 → ********\n\
             output: 1 match on 1 of 2 lines (key-value 1)"
        );
        let none = Redactor::secrets().preview("fine\n");
        assert_eq!(none.summary(), "no matches in 1 line");
    }
}
