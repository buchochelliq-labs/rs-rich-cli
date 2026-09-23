//! A dashboard over many diagnostics: counts by level, the most frequent
//! codes, and every diagnostic grouped by file in location order.
//!
//! [`DiagnosticsDashboard`] reads each [`Diagnostic`]'s level, code and
//! location (its own, or its first snippet's), so a compiler run, a linter or
//! a config validator can pour its results in and print one overview. Paths
//! link through a [`Hyperlinker`] when one is set.

use std::collections::BTreeMap;

use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Style, Table, Text};

use crate::diagnostic::{Diagnostic, Level, Location};
use crate::event::theme_style;
use crate::hyperlink::Hyperlinker;

/// Aggregates diagnostics for one overview.
#[derive(Clone, Debug, Default)]
pub struct DiagnosticsDashboard {
    diagnostics: Vec<Diagnostic>,
    min_level: Option<Level>,
    linker: Option<Hyperlinker>,
    top_codes: usize,
}

const LEVELS: [Level; 5] = [
    Level::Error,
    Level::Warning,
    Level::Info,
    Level::Note,
    Level::Help,
];

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("{count} {word}")
    } else {
        format!("{count} {word}s")
    }
}

impl DiagnosticsDashboard {
    /// An empty dashboard showing the 5 most frequent codes.
    pub fn new() -> Self {
        DiagnosticsDashboard {
            top_codes: 5,
            ..Default::default()
        }
    }

    /// Add one diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) -> &mut Self {
        self.diagnostics.push(diagnostic);
        self
    }

    /// Add many diagnostics.
    pub fn extend(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) -> &mut Self {
        self.diagnostics.extend(diagnostics);
        self
    }

    /// Hide diagnostics less serious than `level` (error is the most serious).
    pub fn min_level(mut self, level: Level) -> Self {
        self.min_level = Some(level);
        self
    }

    /// Link file headings and locations with `linker`.
    pub fn hyperlinker(mut self, linker: Hyperlinker) -> Self {
        self.linker = Some(linker);
        self
    }

    /// How many codes the frequency table lists (0 hides it).
    pub fn top_codes(mut self, count: usize) -> Self {
        self.top_codes = count;
        self
    }

    fn shown(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(move |d| match (self.min_level, d.get_level()) {
                (Some(min), Some(level)) => level <= min,
                _ => true,
            })
    }

    /// How many shown diagnostics have each level; unlevelled ones count as
    /// errors.
    pub fn counts(&self) -> BTreeMap<Level, usize> {
        let mut counts = BTreeMap::new();
        for diagnostic in self.shown() {
            *counts
                .entry(diagnostic.get_level().unwrap_or(Level::Error))
                .or_insert(0) += 1;
        }
        counts
    }

    fn summary(&self, console: &Console, files: usize) -> Text {
        let counts = self.counts();
        let mut text = Text::new("");
        if counts.is_empty() {
            text.append(
                "No diagnostics",
                Some(theme_style(console, "diagnostic.note", "bold green").into()),
            );
            return text;
        }
        let mut first = true;
        for level in LEVELS {
            let Some(&count) = counts.get(&level) else {
                continue;
            };
            if !first {
                text.append(", ", None);
            }
            first = false;
            text.append(
                &plural(count, level.name()),
                Some(level.style(console).into()),
            );
        }
        if files > 0 {
            text.append(&format!(" in {}", plural(files, "file")), None);
        }
        text
    }

    fn codes_table(&self, console: &Console) -> Option<Table> {
        let mut by_code: BTreeMap<&str, (usize, Level, Option<Location>)> = BTreeMap::new();
        for diagnostic in self.shown() {
            let Some(code) = diagnostic.get_code() else {
                continue;
            };
            let entry = by_code.entry(code).or_insert((
                0,
                diagnostic.get_level().unwrap_or(Level::Error),
                diagnostic.get_location(),
            ));
            entry.0 += 1;
        }
        if by_code.is_empty() || self.top_codes == 0 {
            return None;
        }
        let mut rows: Vec<_> = by_code.into_iter().collect();
        // Most frequent first; ties by level, then code.
        rows.sort_by(|a, b| {
            b.1 .0
                .cmp(&a.1 .0)
                .then(a.1 .1.cmp(&b.1 .1))
                .then(a.0.cmp(b.0))
        });
        let mut table = Table::new().title("Top codes");
        table.add_column("Code");
        table.add_column_justify("Count", Justify::Right);
        table.add_column("First seen");
        for (code, (count, level, location)) in rows.into_iter().take(self.top_codes) {
            let first_seen = match location {
                Some(location) => self.location_text(&location, true),
                None => Text::new(""),
            };
            table.add_row_text(vec![
                Text::styled(code.to_string(), level.style(console)),
                Text::new(count.to_string()),
                first_seen,
            ]);
        }
        Some(table)
    }

    fn location_text(&self, location: &Location, with_path: bool) -> Text {
        let linker = self.linker.clone().unwrap_or_else(Hyperlinker::disabled);
        let mut text = linker.location(&location.path, location.line, location.column, "");
        if !with_path {
            // Only `line:column` is shown under a file heading; the link stays.
            let label = match (location.line, location.column) {
                (Some(line), Some(column)) => format!("{line}:{column}"),
                (Some(line), None) => line.to_string(),
                _ => "-".into(),
            };
            let url = linker.file_url(&location.path, location.line, location.column);
            text = Text::new(label);
            if let Some(url) = url {
                let end = text.plain().len();
                text.stylize(Style::new().with_link(url), 0, end);
            }
        }
        text
    }
}

impl Renderable for DiagnosticsDashboard {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut groups: BTreeMap<Option<String>, Vec<(Option<Location>, &Diagnostic)>> =
            BTreeMap::new();
        for diagnostic in self.shown() {
            let location = diagnostic.get_location();
            groups
                .entry(location.as_ref().map(|l| l.path.clone()))
                .or_default()
                .push((location, diagnostic));
        }
        let files = groups.keys().filter(|key| key.is_some()).count();

        let mut lines: Vec<Vec<Segment>> = Vec::new();
        let mut push = |renderable: &dyn Renderable| {
            lines.extend(console.render_lines(renderable, options, false));
        };
        push(&self.summary(console, files));
        if let Some(table) = self.codes_table(console) {
            push(&Text::new(""));
            push(&table);
        }

        let heading = theme_style(console, "diagnostic.file", "bold underline");
        // Files in path order; diagnostics without a location last.
        let mut ordered: Vec<_> = groups.into_iter().collect();
        ordered.sort_by_key(|(path, _)| (path.is_none(), path.clone()));
        for (path, mut entries) in ordered {
            entries.sort_by_key(|(location, _)| location.as_ref().map(|l| (l.line, l.column)));
            push(&Text::new(""));
            let title = match &path {
                Some(path) => {
                    let linker = self.linker.clone().unwrap_or_else(Hyperlinker::disabled);
                    let mut title = linker.location(path, None, None, heading.clone());
                    title.append(
                        &format!("  {}", plural(entries.len(), "diagnostic")),
                        Some(theme_style(console, "diagnostic.count", "dim").into()),
                    );
                    title
                }
                None => Text::styled("(no location)", heading.clone()),
            };
            push(&title);
            let mut grid = Table::grid().padding(0, 1, 0, 1);
            grid.add_column_justify("", Justify::Right);
            grid.add_column("");
            grid.add_column("");
            for (location, diagnostic) in entries {
                let level = diagnostic.get_level().unwrap_or(Level::Error);
                let mut label = Text::styled(level.name().to_string(), level.style(console));
                if let Some(code) = diagnostic.get_code() {
                    label.append(&format!("[{code}]"), Some(level.style(console).into()));
                }
                let position = match &location {
                    Some(location) => self.location_text(location, false),
                    None => Text::new("-"),
                };
                grid.add_row_text(vec![
                    position,
                    label,
                    Text::new(diagnostic.message().to_string()),
                ]);
            }
            push(&grid);
        }

        let last = lines.len().saturating_sub(1);
        let mut segments = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }
}
