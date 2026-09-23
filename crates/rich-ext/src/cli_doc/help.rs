//! Help output (#38, #407): usage, about, option groups, arguments,
//! subcommands, examples and extra sections.
//!
//! ## Layout
//!
//! Entries (`-w, --width <SIZE>` plus help) are laid out in two columns when
//! they fit, else stacked:
//!
//! - **Natural:** when the width holds the widest names, a two-cell gap and
//!   the widest help line, the names column is exactly as wide as the widest
//!   names and nothing wraps.
//! - **Two-column:** from [`STACK_BELOW`] cells, the names column is capped at
//!   40% of the width; wider names sit on their own line with the help below,
//!   in the help column. Help wraps within its column.
//! - **Stacked:** below [`STACK_BELOW`] cells each entry's names take a line
//!   and its help follows, indented six cells.
//!
//! The layout depends only on the width and the content, and measuring
//! reports the natural width, so a help view rendered at its measured width
//! (inside a fitted panel, for example) lays out exactly as it did at full
//! width.

use super::spec::{ArgSpec, CommandSpec};
use super::{join_lines, paragraphs, style, wrap_indented};
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

/// Widths below this many cells use the stacked layout (unless every entry
/// fits naturally).
pub const STACK_BELOW: usize = 60;
const INDENT: usize = 2;
const GAP: usize = 2;
const STACK_INDENT: usize = 6;
const USAGE_LABEL: &str = "Usage: ";

/// A renderable help page for a [`CommandSpec`].
///
/// ```
/// use rich::Console;
/// use rich_ext::cli_doc::{ArgSpec, CommandSpec, HelpView};
///
/// let spec = CommandSpec::new("tool")
///     .arg(ArgSpec::option("level").choices(["low", "high"]).default_value("low").help("How much"));
/// let out = Console::builder().width(80).build().render_to_string(&HelpView::new(&spec));
/// assert!(out.contains("--level <LEVEL>  How much [default: low] [possible values: low, high]"));
/// ```
#[derive(Clone, Debug)]
pub struct HelpView {
    spec: CommandSpec,
    long: bool,
    command_path: Option<String>,
}

/// One help row: names, help (with hints) and an optional list of choices.
struct Entry {
    names: Text,
    help: Text,
    choices: Vec<Text>,
}

enum Mode {
    /// Two columns; the names column is this wide.
    Columns(usize),
    Stacked,
}

impl HelpView {
    pub fn new(spec: &CommandSpec) -> Self {
        HelpView {
            spec: spec.clone(),
            long: false,
            command_path: None,
        }
    }

    /// The help for the subcommand at `path` below `root` (such as
    /// `["config", "show"]`), with the full command path in its usage.
    pub fn for_path(root: &CommandSpec, path: &[&str]) -> Option<Self> {
        let mut spec = root;
        let mut shown = root.display_name().to_string();
        for name in path {
            spec = spec.find_subcommand(name)?;
            shown.push(' ');
            shown.push_str(&spec.name);
        }
        Some(HelpView::new(spec).command_path(shown))
    }

    /// Long help (`--help`): long about and long argument help where given,
    /// and a blank line between entries.
    pub fn long(mut self, long: bool) -> Self {
        self.long = long;
        self
    }

    /// The command as usage shows it, such as `rich config`.
    pub fn command_path(mut self, path: impl Into<String>) -> Self {
        self.command_path = Some(path.into());
        self
    }

    fn usage_lines(&self) -> Vec<String> {
        match &self.command_path {
            Some(path) => self.spec.usage_lines_as(path),
            None => self.spec.usage_lines(),
        }
    }

    fn about(&self) -> &str {
        match (&self.spec.long_about, self.long) {
            (Some(long), true) => long,
            _ => &self.spec.about,
        }
    }

    /// `indent_long`: pad a long-only option so its `--` lines up with the
    /// longs of options that have a short, as clap does.
    fn arg_entry(&self, c: &Console, arg: &ArgSpec, indent_long: bool) -> Entry {
        let option = style(c, "help.option");
        let metavar = style(c, "help.metavar");
        let hint = style(c, "help.hint");
        let mut names = Text::new("");
        if arg.positional {
            append(&mut names, &arg.names(), &metavar);
        } else {
            if indent_long && arg.short.is_none() {
                names.append("    ", None);
            }
            for (i, switch) in arg.switches().iter().enumerate() {
                if i > 0 {
                    names.append(", ", None);
                }
                append(&mut names, switch, &option);
            }
            if let Some(value) = arg.metavar() {
                names.append(" ", None);
                append(&mut names, &value, &metavar);
            }
        }
        let text = match (&arg.long_help, self.long) {
            (Some(long), true) => long.as_str(),
            _ => arg.help.as_str(),
        };
        let mut help = Text::new("");
        append(&mut help, text.trim_end(), &style(c, "help.description"));
        let mut hints = Vec::new();
        if let Some(default) = &arg.default {
            hints.push(format!("[default: {default}]"));
        }
        if let Some(env) = &arg.env {
            hints.push(format!("[env: {env}]"));
        }
        if let Some(key) = &arg.config_key {
            hints.push(format!("[config: {key}]"));
        }
        let choices = arg.choice_list();
        let described = choices.iter().any(|choice| !choice.help.is_empty());
        if !choices.is_empty() && !described {
            let values: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
            hints.push(format!("[possible values: {}]", values.join(", ")));
        }
        for hint_text in hints {
            if !help.is_empty() {
                help.append(" ", None);
            }
            append(&mut help, &hint_text, &hint);
        }
        let mut list = Vec::new();
        if described {
            list.push(Text::styled("Possible values:", hint.clone()));
            for choice in choices {
                let mut line = Text::new("- ");
                append(&mut line, &choice.value, &metavar);
                if !choice.help.is_empty() {
                    line.append(": ", None);
                    append(&mut line, &choice.help, &style(c, "help.description"));
                }
                list.push(line);
            }
        }
        Entry {
            names,
            help,
            choices: list,
        }
    }

    fn command_entry(&self, c: &Console, command: &CommandSpec) -> Entry {
        let mut names = Text::new("");
        append(&mut names, &command.name, &style(c, "help.command"));
        let mut help = Text::new("");
        let about = paragraphs(&command.about)
            .into_iter()
            .next()
            .unwrap_or_default();
        append(&mut help, &about, &style(c, "help.description"));
        if !command.aliases.is_empty() {
            if !help.is_empty() {
                help.append(" ", None);
            }
            let aliases = format!("[aliases: {}]", command.aliases.join(", "));
            append(&mut help, &aliases, &style(c, "help.hint"));
        }
        Entry {
            names,
            help,
            choices: Vec::new(),
        }
    }

    /// Titled groups of entries, in display order.
    fn groups(&self, c: &Console) -> Vec<(String, Option<String>, Vec<Entry>)> {
        let indent_long = self
            .spec
            .visible_args()
            .any(|a| !a.positional && a.short.is_some());
        let mut out: Vec<(String, Option<String>, Vec<Entry>)> = self
            .spec
            .groups()
            .into_iter()
            .map(|(title, args)| {
                let note = self.spec.note_for(&title).map(str::to_string);
                let entries = args
                    .into_iter()
                    .map(|a| self.arg_entry(c, a, indent_long))
                    .collect();
                (title, note, entries)
            })
            .collect();
        let commands: Vec<Entry> = self
            .spec
            .visible_subcommands()
            .map(|command| self.command_entry(c, command))
            .collect();
        if !commands.is_empty() {
            let title = self
                .spec
                .subcommand_heading
                .clone()
                .unwrap_or_else(|| "Commands".into());
            let note = self.spec.note_for(&title).map(str::to_string);
            out.push((title, note, commands));
        }
        out
    }

    /// The width at which every entry fits in two columns without wrapping,
    /// and the widest names.
    fn natural_columns(groups: &[(String, Option<String>, Vec<Entry>)]) -> (usize, usize) {
        let entries = groups.iter().flat_map(|(_, _, entries)| entries);
        let names = entries
            .clone()
            .map(|e| e.names.cell_len())
            .max()
            .unwrap_or(0);
        let help = entries
            .flat_map(|e| std::iter::once(&e.help).chain(&e.choices))
            .map(|t| t.measurement().1)
            .max()
            .unwrap_or(0);
        (INDENT + names + GAP + help, names)
    }

    /// The widest line of everything outside the entries, unwrapped.
    fn natural_prose(&self, groups: &[(String, Option<String>, Vec<Entry>)]) -> usize {
        let widest = |text: &str| text.lines().map(rich::cells::cell_len).max().unwrap_or(0);
        let mut width = self
            .usage_lines()
            .iter()
            .map(|line| USAGE_LABEL.len() + widest(line))
            .max()
            .unwrap_or(0);
        width = width.max(widest(self.about()));
        for (title, note, _) in groups {
            width = width.max(heading_text(title).len());
            if let Some(note) = note {
                width = width.max(INDENT + widest(note));
            }
        }
        for example in &self.spec.examples {
            width = width
                .max(INDENT + widest(&example.description))
                .max(2 * INDENT + 2 + widest(&example.command));
        }
        for section in &self.spec.sections {
            let indent = if section.title.is_empty() { 0 } else { INDENT };
            width = width
                .max(heading_text(&section.title).len())
                .max(indent + widest(&section.body));
        }
        width
    }

    fn mode(width: usize, natural: usize, names: usize) -> Mode {
        if width >= natural {
            Mode::Columns(names)
        } else if width >= STACK_BELOW {
            Mode::Columns(names.min(width * 2 / 5))
        } else {
            Mode::Stacked
        }
    }

    fn render_entry(
        &self,
        c: &Console,
        entry: &Entry,
        mode: &Mode,
        width: usize,
    ) -> Vec<Vec<Segment>> {
        let theme = c.theme();
        let plain = Style::new();
        let mut rows = Vec::new();
        let (help_indent, names_inline) = match mode {
            Mode::Columns(col) => (INDENT + col + GAP, entry.names.cell_len() <= *col),
            Mode::Stacked => (STACK_INDENT, false),
        };
        let mut help_lines = if entry.help.is_empty() {
            Vec::new()
        } else {
            wrap_indented(c, &entry.help, help_indent, width)
        };
        for choice in &entry.choices {
            let inner = width.saturating_sub(help_indent + 2).max(1);
            let lines = choice.render_lines(theme, &plain, Some(inner));
            for (i, line) in lines.into_iter().enumerate() {
                let pad = help_indent + if i == 0 { 0 } else { 2 };
                let mut row = vec![Segment::new(" ".repeat(pad), None)];
                row.extend(line);
                help_lines.push(row);
            }
        }
        if names_inline && !help_lines.is_empty() {
            let mut first = vec![Segment::new(" ".repeat(INDENT), None)];
            first.extend(entry.names.render(theme, &plain));
            let used = INDENT + entry.names.cell_len();
            // The help line already carries its full indent; keep only the
            // part past the names.
            let mut help = help_lines.remove(0);
            if let Some(lead) = help.first_mut() {
                lead.text = " ".repeat(help_indent - used);
            }
            first.extend(help);
            rows.push(first);
        } else {
            rows.extend(wrap_indented(c, &entry.names, INDENT, width));
        }
        rows.extend(help_lines);
        rows
    }
}

fn append(text: &mut Text, value: &str, style: &Style) {
    if style.is_null() {
        text.append(value, None);
    } else {
        text.append(value, Some(style.clone().into()));
    }
}

fn heading_text(title: &str) -> String {
    if title.ends_with(':') {
        title.to_string()
    } else {
        format!("{title}:")
    }
}

impl Renderable for HelpView {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        let width = o.max_width;
        if width == 0 {
            return Vec::new();
        }
        let groups = self.groups(c);
        let (natural, names) = Self::natural_columns(&groups);
        let mode = Self::mode(width, natural, names);
        let heading = style(c, "help.heading");
        let heading_row =
            |title: &str| vec![Segment::new(heading_text(title), Some(heading.clone()))];
        let mut rows: Vec<Vec<Segment>> = Vec::new();

        // Usage: the label, then each line aligned after it.
        let usage_style = style(c, "help.usage");
        let command = self
            .command_path
            .clone()
            .unwrap_or_else(|| self.spec.display_name().to_string());
        for (i, line) in self.usage_lines().iter().enumerate() {
            let mut text = Text::new("");
            match line.strip_prefix(&command) {
                Some(rest) => {
                    append(&mut text, &command, &usage_style);
                    text.append(rest, None);
                }
                None => text.append(line, None),
            }
            let mut lines = wrap_indented(c, &text, USAGE_LABEL.len(), width);
            if i == 0 {
                if let Some(first) = lines.first_mut() {
                    first[0] = Segment::new(USAGE_LABEL.trim_end(), Some(heading.clone()));
                    first.insert(1, Segment::new(" ", None));
                }
            }
            rows.extend(lines);
        }

        let about = paragraphs(self.about());
        if !about.is_empty() {
            for paragraph in about {
                rows.push(Vec::new());
                rows.extend(wrap_indented(c, &Text::new(paragraph), 0, width));
            }
        }

        for (title, note, entries) in &groups {
            rows.push(Vec::new());
            rows.push(heading_row(title));
            if let Some(note) = note {
                rows.extend(wrap_indented(c, &Text::new(note.as_str()), INDENT, width));
            }
            for (i, entry) in entries.iter().enumerate() {
                if self.long && i > 0 {
                    rows.push(Vec::new());
                }
                rows.extend(self.render_entry(c, entry, &mode, width));
            }
        }

        if !self.spec.examples.is_empty() {
            rows.push(Vec::new());
            rows.push(heading_row("Examples"));
            let example = style(c, "help.example");
            for item in &self.spec.examples {
                let mut indent = INDENT;
                if !item.description.is_empty() {
                    rows.extend(wrap_indented(
                        c,
                        &Text::new(item.description.as_str()),
                        INDENT,
                        width,
                    ));
                    indent += INDENT;
                }
                let mut command = Text::new("");
                append(&mut command, &format!("$ {}", item.command), &example);
                rows.extend(wrap_indented(c, &command, indent, width));
            }
        }

        for section in &self.spec.sections {
            let indent = if section.title.is_empty() {
                0
            } else {
                rows.push(Vec::new());
                rows.push(heading_row(&section.title));
                INDENT
            };
            for (i, paragraph) in paragraphs(&section.body).into_iter().enumerate() {
                if i > 0 || section.title.is_empty() {
                    rows.push(Vec::new());
                }
                rows.extend(wrap_indented(c, &Text::new(paragraph), indent, width));
            }
        }

        // A leading blank row appears only when there is no usage at all.
        while rows.first().is_some_and(Vec::is_empty) {
            rows.remove(0);
        }
        if let Some(height) = o.height {
            rows.truncate(height);
        }
        join_lines(rows)
    }

    fn measure(&self, c: &Console, o: &ConsoleOptions) -> Measurement {
        let groups = self.groups(c);
        let (columns, names) = Self::natural_columns(&groups);
        let natural = columns.max(self.natural_prose(&groups));
        let maximum = natural.min(o.max_width);
        let word = |text: &str| {
            text.split_whitespace()
                .map(rich::cells::cell_len)
                .max()
                .unwrap_or(0)
        };
        let longest_word = groups
            .iter()
            .flat_map(|(_, _, entries)| entries)
            .map(|e| word(e.help.plain()))
            .chain(std::iter::once(word(self.about())))
            .max()
            .unwrap_or(0);
        let minimum = (INDENT + names).max(STACK_INDENT + longest_word);
        Measurement::new(minimum.min(maximum), maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_doc::{ArgSpec, ValueHint};

    fn render(view: &HelpView, width: usize) -> String {
        Console::builder()
            .width(width)
            .build()
            .render_to_string(view)
    }

    #[test]
    fn entries_without_help_render_names_only() {
        let spec = CommandSpec::new("x").arg(ArgSpec::flag("quiet"));
        assert_eq!(
            render(&HelpView::new(&spec), 40),
            "Usage: x [OPTIONS]\n\nOptions:\n  --quiet"
        );
    }

    #[test]
    fn value_hint_none_is_a_flag() {
        assert!(!ArgSpec::flag("x").takes_value());
        assert!(ArgSpec::flag("x").value(ValueHint::File).takes_value());
    }
}
