//! Configuration reference (#409): where settings come from, in precedence
//! order, and every key with its type, default, environment variable and
//! flag.

use super::spec::{ArgSpec, CommandSpec, ValueHint};
use super::{join_lines, paragraphs, style, wrap_indented};
use rich::table::Table;
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};

/// A place settings are read from, such as a config file or the environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigSource {
    pub name: String,
    /// A path, variable prefix or other locator; may be empty.
    pub location: String,
    pub description: String,
}

/// One configurable setting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigEntry {
    pub key: String,
    /// The value type as users write it: `integer`, `string`, `bool`,
    /// `enum`, `path`, `list`, ...
    pub kind: String,
    pub default: Option<String>,
    /// Allowed values, for an `enum`.
    pub choices: Vec<String>,
    pub env: Option<String>,
    /// The command-line flag that sets it.
    pub flag: Option<String>,
    pub description: String,
}

impl ConfigEntry {
    pub fn new(key: impl Into<String>, kind: impl Into<String>) -> Self {
        ConfigEntry {
            key: key.into(),
            kind: kind.into(),
            ..Default::default()
        }
    }
    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        self.default = Some(value.into());
        self
    }
    pub fn choices<S: Into<String>>(mut self, choices: impl IntoIterator<Item = S>) -> Self {
        self.choices = choices.into_iter().map(Into::into).collect();
        self
    }
    pub fn env(mut self, name: impl Into<String>) -> Self {
        self.env = Some(name.into());
        self
    }
    pub fn flag(mut self, flag: impl Into<String>) -> Self {
        self.flag = Some(flag.into());
        self
    }
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = text.into();
        self
    }

    /// The type column: the kind, plus the choices for an enum.
    fn kind_text(&self) -> String {
        if self.choices.is_empty() {
            self.kind.clone()
        } else {
            format!("{}: {}", self.kind, self.choices.join(", "))
        }
    }
}

/// A configuration reference: sources in precedence order, then every key.
///
/// ```
/// use rich::Console;
/// use rich_ext::cli_doc::{ConfigEntry, ConfigReference};
///
/// let reference = ConfigReference::new("Settings")
///     .source("defaults", "", "Built in")
///     .source("user", "~/.config/tool.toml", "Your settings")
///     .entry(ConfigEntry::new("width", "integer").default_value("80").flag("--width"));
/// let out = Console::builder().width(80).build().render_to_string(&reference);
/// assert!(out.contains("  1. defaults — Built in"));
/// assert!(out.contains("  2. user (~/.config/tool.toml) — Your settings"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigReference {
    pub title: String,
    pub description: String,
    /// Lowest precedence first: a later source overrides an earlier one.
    pub sources: Vec<ConfigSource>,
    pub entries: Vec<ConfigEntry>,
}

impl ConfigReference {
    pub fn new(title: impl Into<String>) -> Self {
        ConfigReference {
            title: title.into(),
            ..Default::default()
        }
    }
    pub fn description(mut self, text: impl Into<String>) -> Self {
        self.description = text.into();
        self
    }
    /// Add a source above every source added so far.
    pub fn source(
        mut self,
        name: impl Into<String>,
        location: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.sources.push(ConfigSource {
            name: name.into(),
            location: location.into(),
            description: description.into(),
        });
        self
    }
    pub fn entry(mut self, entry: ConfigEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Entries for every argument of `spec` and its subcommands that has a
    /// `config_key`, the first definition of a key winning. The kind is
    /// `bool` for flags, `enum` for choices, `path`/`url`/`command` by hint,
    /// else `string`; `list` wraps it for a repeatable argument.
    pub fn from_spec(spec: &CommandSpec) -> Self {
        let mut reference = ConfigReference::new(format!("{} configuration", spec.display_name()));
        collect(spec, &mut reference.entries);
        reference
    }

    fn sources_intro(&self) -> &'static str {
        "Settings are read from these sources, lowest precedence first; a later source overrides an earlier one:"
    }

    fn source_line(index: usize, source: &ConfigSource) -> String {
        let mut line = format!("{}. {}", index + 1, source.name);
        if !source.location.is_empty() {
            line.push_str(&format!(" ({})", source.location));
        }
        if !source.description.is_empty() {
            line.push_str(&format!(" — {}", source.description));
        }
        line
    }

    /// The reference as Markdown: sources as an ordered list, keys as a table.
    ///
    /// ```
    /// use rich_ext::cli_doc::{ConfigEntry, ConfigReference};
    ///
    /// let md = ConfigReference::new("Settings")
    ///     .entry(ConfigEntry::new("mode", "enum").choices(["a", "b"]).env("MODE"))
    ///     .to_markdown();
    /// assert!(md.ends_with("| `mode` | enum: `a`, `b` | | `MODE` | | |\n"));
    /// ```
    pub fn to_markdown(&self) -> String {
        let code = |s: &str| format!("`{}`", s.replace('|', r"\|"));
        let cell = |s: &str| s.trim().replace('|', r"\|").replace('\n', " ");
        let mut out = format!("# {}\n\n", self.title);
        for paragraph in paragraphs(&self.description) {
            out.push_str(&paragraph);
            out.push_str("\n\n");
        }
        if !self.sources.is_empty() {
            out.push_str("## Sources\n\n");
            out.push_str(self.sources_intro());
            out.push_str("\n\n");
            for (i, source) in self.sources.iter().enumerate() {
                let mut line = format!("{}. **{}**", i + 1, source.name);
                if !source.location.is_empty() {
                    line.push_str(&format!(" ({})", code(&source.location)));
                }
                if !source.description.is_empty() {
                    line.push_str(&format!(" — {}", source.description.trim()));
                }
                out.push_str(&line);
                out.push('\n');
            }
            out.push('\n');
        }
        if !self.entries.is_empty() {
            out.push_str(
                "## Keys\n\n| Key | Type | Default | Environment | Flag | Description |\n",
            );
            out.push_str("| --- | --- | --- | --- | --- | --- |\n");
            for entry in &self.entries {
                let kind = if entry.choices.is_empty() {
                    cell(&entry.kind)
                } else {
                    let values: Vec<String> = entry.choices.iter().map(|c| code(c)).collect();
                    format!("{}: {}", cell(&entry.kind), values.join(", "))
                };
                let optional =
                    |value: &Option<String>| value.as_deref().map(code).unwrap_or_default();
                let row = [
                    code(&entry.key),
                    kind,
                    optional(&entry.default),
                    optional(&entry.env),
                    optional(&entry.flag),
                    cell(&entry.description),
                ];
                let row: Vec<String> = row
                    .into_iter()
                    .map(|c| {
                        if c.is_empty() {
                            " ".to_string()
                        } else {
                            format!(" {c} ")
                        }
                    })
                    .collect();
                out.push_str(&format!("|{}|\n", row.join("|")));
            }
        }
        let mut out = out.trim_end().to_string();
        out.push('\n');
        out
    }
}

fn collect(spec: &CommandSpec, entries: &mut Vec<ConfigEntry>) {
    for arg in spec.visible_args() {
        let Some(key) = &arg.config_key else { continue };
        if entries.iter().any(|e| &e.key == key) {
            continue;
        }
        entries.push(entry_for(arg, key));
    }
    for command in spec.visible_subcommands() {
        collect(command, entries);
    }
}

fn entry_for(arg: &ArgSpec, key: &str) -> ConfigEntry {
    let kind = match (&arg.value, arg.takes_value()) {
        (_, false) => "bool",
        (ValueHint::Choices(_), _) => "enum",
        (ValueHint::File | ValueHint::Dir | ValueHint::Path, _) => "path",
        (ValueHint::Url, _) => "url",
        (ValueHint::Command, _) => "command",
        _ => "string",
    };
    let kind = if arg.multiple && arg.takes_value() {
        format!("list of {kind}")
    } else {
        kind.to_string()
    };
    ConfigEntry {
        key: key.to_string(),
        kind,
        default: arg.default.clone(),
        choices: arg.choice_list().iter().map(|c| c.value.clone()).collect(),
        env: arg.env.clone(),
        flag: (!arg.positional).then(|| arg.primary()),
        description: arg.help.trim().to_string(),
    }
}

impl Renderable for ConfigReference {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        let width = o.max_width;
        if width == 0 {
            return Vec::new();
        }
        let mut rows: Vec<Vec<Segment>> = Vec::new();
        let heading = style(c, "help.heading");
        if !self.title.is_empty() {
            rows.extend(wrap_indented(
                c,
                &Text::styled(self.title.clone(), heading.clone()),
                0,
                width,
            ));
        }
        for paragraph in paragraphs(&self.description) {
            if !rows.is_empty() {
                rows.push(Vec::new());
            }
            rows.extend(wrap_indented(c, &Text::new(paragraph), 0, width));
        }
        if !self.sources.is_empty() {
            if !rows.is_empty() {
                rows.push(Vec::new());
            }
            rows.extend(wrap_indented(c, &Text::new(self.sources_intro()), 0, width));
            for (i, source) in self.sources.iter().enumerate() {
                rows.extend(wrap_indented(
                    c,
                    &Text::new(Self::source_line(i, source)),
                    2,
                    width,
                ));
            }
        }
        if !self.entries.is_empty() {
            if !rows.is_empty() {
                rows.push(Vec::new());
            }
            let mut table = Table::new();
            for header in ["Key", "Type", "Default", "Env", "Flag", "Description"] {
                table.add_column(header);
            }
            let key = style(c, "config.key");
            let option = style(c, "help.option");
            for entry in &self.entries {
                table.add_row_text(vec![
                    Text::styled(entry.key.clone(), key.clone()),
                    Text::new(entry.kind_text()),
                    Text::new(entry.default.clone().unwrap_or_default()),
                    Text::new(entry.env.clone().unwrap_or_default()),
                    Text::styled(entry.flag.clone().unwrap_or_default(), option.clone()),
                    Text::new(entry.description.clone()),
                ]);
            }
            rows.extend(c.render_lines(&table, &o.update_width(width), false));
        }
        join_lines(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_spec_derives_kinds_and_skips_duplicates() {
        let spec = CommandSpec::new("x")
            .arg(ArgSpec::flag("pager").config_key("pager"))
            .arg(
                ArgSpec::option("theme")
                    .choices(["dark", "light"])
                    .config_key("theme"),
            )
            .arg(
                ArgSpec::option("include")
                    .value(ValueHint::Path)
                    .multiple(true)
                    .config_key("include"),
            )
            .arg(ArgSpec::option("plain"))
            .subcommand(CommandSpec::new("y").arg(ArgSpec::flag("pager2").config_key("pager")));
        let reference = ConfigReference::from_spec(&spec);
        let kinds: Vec<(&str, &str)> = reference
            .entries
            .iter()
            .map(|e| (e.key.as_str(), e.kind.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("pager", "bool"),
                ("theme", "enum"),
                ("include", "list of path")
            ]
        );
        assert_eq!(reference.entries[0].flag.as_deref(), Some("--pager"));
        assert_eq!(reference.title, "x configuration");
    }
}
