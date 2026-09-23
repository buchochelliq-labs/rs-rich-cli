//! The command description model: plain data with builder methods.

/// What an argument's value is, for help hints and shell completion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ValueHint {
    /// A flag: takes no value.
    #[default]
    None,
    /// Any value; completion offers nothing specific.
    Any,
    /// A file path.
    File,
    /// A directory path.
    Dir,
    /// A file or directory path.
    Path,
    /// A command name.
    Command,
    /// A URL.
    Url,
    /// One of a fixed set of values.
    Choices(Vec<Choice>),
}

/// One allowed value, with optional help.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    /// Empty when the value needs no explanation.
    pub help: String,
}

impl Choice {
    pub fn new(value: impl Into<String>) -> Self {
        Choice {
            value: value.into(),
            help: String::new(),
        }
    }
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }
}

impl From<&str> for Choice {
    fn from(value: &str) -> Self {
        Choice::new(value)
    }
}

impl<V: Into<String>, H: Into<String>> From<(V, H)> for Choice {
    fn from((value, help): (V, H)) -> Self {
        Choice::new(value).help(help)
    }
}

/// An option, flag or positional argument.
///
/// ```
/// use rich_ext::cli_doc::{ArgSpec, ValueHint};
///
/// let color = ArgSpec::option("color")
///     .value_name("WHEN")
///     .choices(["auto", "always", "never"])
///     .default_value("auto")
///     .env("RICH_COLOR")
///     .help("When to use colour");
/// assert_eq!(color.names(), "--color <WHEN>");
/// assert!(matches!(color.value, ValueHint::Choices(ref c) if c.len() == 3));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArgSpec {
    /// A stable identifier; also the default value name.
    pub id: String,
    /// The long name, without dashes.
    pub long: Option<String>,
    pub short: Option<char>,
    /// Extra long names, without dashes.
    pub aliases: Vec<String>,
    /// The metavar shown as `<VALUE>`. `None` makes the argument a flag.
    pub value_name: Option<String>,
    pub value: ValueHint,
    pub help: String,
    /// Help for `--help` (long) output; `help` is used when absent.
    pub long_help: Option<String>,
    pub default: Option<String>,
    /// The environment variable that sets this argument.
    pub env: Option<String>,
    /// The config file key that sets this argument.
    pub config_key: Option<String>,
    pub required: bool,
    /// Whether the argument may be given more than once (or takes a list).
    pub multiple: bool,
    /// Hidden arguments are left out of help, docs and completions.
    pub hidden: bool,
    /// The help group title; `None` means "Options" (or "Arguments" for a
    /// positional).
    pub heading: Option<String>,
    pub positional: bool,
}

impl ArgSpec {
    /// A bare argument with only an id; set the rest with the builders.
    pub fn new(id: impl Into<String>) -> Self {
        ArgSpec {
            id: id.into(),
            ..Default::default()
        }
    }

    /// A `--long` flag that takes no value.
    pub fn flag(long: impl Into<String>) -> Self {
        let long = long.into();
        ArgSpec {
            id: long.clone(),
            long: Some(long),
            ..Default::default()
        }
    }

    /// A `--long <LONG>` option taking any value.
    pub fn option(long: impl Into<String>) -> Self {
        let long = long.into();
        ArgSpec {
            id: long.clone(),
            value_name: Some(long.to_uppercase().replace('-', "_")),
            long: Some(long),
            value: ValueHint::Any,
            ..Default::default()
        }
    }

    /// A positional argument shown as `[NAME]`, or `<NAME>` when required.
    pub fn positional(name: impl Into<String>) -> Self {
        let name = name.into();
        ArgSpec {
            value_name: Some(name.to_uppercase().replace('-', "_")),
            id: name,
            value: ValueHint::Any,
            positional: true,
            ..Default::default()
        }
    }

    pub fn long(mut self, long: impl Into<String>) -> Self {
        self.long = Some(long.into());
        self
    }
    pub fn short(mut self, short: char) -> Self {
        self.short = Some(short);
        self
    }
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }
    /// Set the metavar; this also makes a flag take a value.
    pub fn value_name(mut self, name: impl Into<String>) -> Self {
        self.value_name = Some(name.into());
        if self.value == ValueHint::None {
            self.value = ValueHint::Any;
        }
        self
    }
    /// Set the value hint; a non-`None` hint makes a flag take a value.
    pub fn value(mut self, hint: ValueHint) -> Self {
        if hint != ValueHint::None && self.value_name.is_none() {
            self.value_name = Some(self.id.to_uppercase().replace('-', "_"));
        }
        self.value = hint;
        self
    }
    /// Restrict the value to `choices`.
    pub fn choices<C: Into<Choice>>(self, choices: impl IntoIterator<Item = C>) -> Self {
        self.value(ValueHint::Choices(
            choices.into_iter().map(Into::into).collect(),
        ))
    }
    /// Add one allowed value with help.
    pub fn choice(mut self, value: impl Into<String>, help: impl Into<String>) -> Self {
        let choice = Choice::new(value).help(help);
        if let ValueHint::Choices(choices) = &mut self.value {
            choices.push(choice);
            return self;
        }
        self.value(ValueHint::Choices(vec![choice]))
    }
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }
    pub fn long_help(mut self, help: impl Into<String>) -> Self {
        self.long_help = Some(help.into());
        self
    }
    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        self.default = Some(value.into());
        self
    }
    pub fn env(mut self, name: impl Into<String>) -> Self {
        self.env = Some(name.into());
        self
    }
    pub fn config_key(mut self, key: impl Into<String>) -> Self {
        self.config_key = Some(key.into());
        self
    }
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
    pub fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }
    pub fn heading(mut self, heading: impl Into<String>) -> Self {
        self.heading = Some(heading.into());
        self
    }

    /// Whether the argument takes a value.
    pub fn takes_value(&self) -> bool {
        self.value_name.is_some()
    }

    /// The allowed values, if restricted.
    pub fn choice_list(&self) -> &[Choice] {
        match &self.value {
            ValueHint::Choices(choices) => choices,
            _ => &[],
        }
    }

    /// The help group this argument belongs to.
    pub fn group(&self) -> &str {
        match (&self.heading, self.positional) {
            (Some(heading), _) => heading,
            (None, true) => "Arguments",
            (None, false) => "Options",
        }
    }

    /// The metavar as displayed: `<NAME>` (plus `...` when multiple), or the
    /// name unchanged when it already carries brackets.
    pub fn metavar(&self) -> Option<String> {
        let name = self.value_name.as_deref()?;
        let mut out = if name.starts_with(['<', '[']) {
            name.to_string()
        } else if self.positional && !self.required {
            format!("[{name}]")
        } else {
            format!("<{name}>")
        };
        if self.multiple {
            out.push_str("...");
        }
        Some(out)
    }

    /// Every command-line spelling: `-s`, `--long`, then `--alias`es.
    pub fn switches(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(short) = self.short {
            out.push(format!("-{short}"));
        }
        if let Some(long) = &self.long {
            out.push(format!("--{long}"));
        }
        out.extend(self.aliases.iter().map(|alias| format!("--{alias}")));
        out
    }

    /// The names as help shows them: `-w, --width <SIZE>`; a positional shows
    /// only its metavar.
    pub fn names(&self) -> String {
        if self.positional {
            return self.metavar().unwrap_or_else(|| self.id.clone());
        }
        let mut out = self.switches().join(", ");
        if let Some(metavar) = self.metavar() {
            out.push(' ');
            out.push_str(&metavar);
        }
        out
    }

    /// The primary switch: `--long`, else `-s`, else the id.
    pub fn primary(&self) -> String {
        match (&self.long, self.short) {
            (Some(long), _) => format!("--{long}"),
            (None, Some(short)) => format!("-{short}"),
            _ => self.id.clone(),
        }
    }
}

/// A usage example: a command line and what it does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Example {
    pub command: String,
    pub description: String,
}

/// A free-form help section. Its body is plain text; a blank line separates
/// paragraphs. An empty title renders the body without a heading.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub body: String,
}

/// A note shown under an option group's heading, such as "choose at most one".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeadingNote {
    pub heading: String,
    pub text: String,
}

/// A command (or subcommand) and everything its documentation needs.
///
/// ```
/// use rich_ext::cli_doc::{ArgSpec, CommandSpec};
///
/// let spec = CommandSpec::new("rich")
///     .version("1.0.0")
///     .about("Render files in the terminal")
///     .arg(ArgSpec::flag("pager").help("Page the output"))
///     .arg(ArgSpec::positional("resource"))
///     .subcommand(CommandSpec::new("config").about("Show settings"));
/// assert_eq!(spec.usage_lines(), vec!["rich [OPTIONS] [RESOURCE] [COMMAND]".to_string()]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: String,
    /// The executable name, when it differs from `name`.
    pub bin_name: Option<String>,
    pub version: Option<String>,
    pub about: String,
    pub long_about: Option<String>,
    /// Other names a subcommand answers to.
    pub aliases: Vec<String>,
    /// Explicit usage lines; one is generated from the arguments when empty.
    pub usage: Vec<String>,
    pub args: Vec<ArgSpec>,
    pub subcommands: Vec<CommandSpec>,
    pub examples: Vec<Example>,
    pub sections: Vec<Section>,
    pub heading_notes: Vec<HeadingNote>,
    /// The title of the subcommand list; `None` means "Commands".
    pub subcommand_heading: Option<String>,
    /// Whether a subcommand must be given (`<COMMAND>` rather than
    /// `[COMMAND]` in generated usage).
    pub subcommand_required: bool,
    /// Hidden subcommands are left out of help, docs and completions.
    pub hidden: bool,
}

impl CommandSpec {
    pub fn new(name: impl Into<String>) -> Self {
        CommandSpec {
            name: name.into(),
            ..Default::default()
        }
    }
    pub fn bin_name(mut self, name: impl Into<String>) -> Self {
        self.bin_name = Some(name.into());
        self
    }
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
    pub fn about(mut self, about: impl Into<String>) -> Self {
        self.about = about.into();
        self
    }
    pub fn long_about(mut self, about: impl Into<String>) -> Self {
        self.long_about = Some(about.into());
        self
    }
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }
    /// Add an explicit usage line (without the `Usage:` label).
    pub fn usage(mut self, line: impl Into<String>) -> Self {
        self.usage.push(line.into());
        self
    }
    pub fn arg(mut self, arg: ArgSpec) -> Self {
        self.args.push(arg);
        self
    }
    pub fn args(mut self, args: impl IntoIterator<Item = ArgSpec>) -> Self {
        self.args.extend(args);
        self
    }
    pub fn subcommand(mut self, command: CommandSpec) -> Self {
        self.subcommands.push(command);
        self
    }
    pub fn example(mut self, command: impl Into<String>, description: impl Into<String>) -> Self {
        self.examples.push(Example {
            command: command.into(),
            description: description.into(),
        });
        self
    }
    pub fn section(mut self, title: impl Into<String>, body: impl Into<String>) -> Self {
        self.sections.push(Section {
            title: title.into(),
            body: body.into(),
        });
        self
    }
    /// Add a note shown under the option group titled `heading`.
    pub fn heading_note(mut self, heading: impl Into<String>, text: impl Into<String>) -> Self {
        self.heading_notes.push(HeadingNote {
            heading: heading.into(),
            text: text.into(),
        });
        self
    }
    pub fn subcommand_heading(mut self, heading: impl Into<String>) -> Self {
        self.subcommand_heading = Some(heading.into());
        self
    }
    pub fn subcommand_required(mut self, required: bool) -> Self {
        self.subcommand_required = required;
        self
    }
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// The name the command is invoked by: `bin_name`, else `name`.
    pub fn display_name(&self) -> &str {
        self.bin_name.as_deref().unwrap_or(&self.name)
    }

    /// The arguments that are not hidden.
    pub fn visible_args(&self) -> impl Iterator<Item = &ArgSpec> {
        self.args.iter().filter(|arg| !arg.hidden)
    }

    /// The subcommands that are not hidden.
    pub fn visible_subcommands(&self) -> impl Iterator<Item = &CommandSpec> {
        self.subcommands.iter().filter(|command| !command.hidden)
    }

    /// The subcommand called (or aliased) `name`.
    pub fn find_subcommand(&self, name: &str) -> Option<&CommandSpec> {
        self.subcommands
            .iter()
            .find(|c| c.name == name || c.aliases.iter().any(|a| a == name))
    }

    /// The visible option groups in first-seen order, each with its arguments.
    /// Positionals without a heading form a trailing "Arguments" group.
    pub fn groups(&self) -> Vec<(String, Vec<&ArgSpec>)> {
        let mut groups: Vec<(String, Vec<&ArgSpec>)> = Vec::new();
        let mut positionals = Vec::new();
        for arg in self.visible_args() {
            if arg.positional && arg.heading.is_none() {
                positionals.push(arg);
                continue;
            }
            match groups.iter_mut().find(|(title, _)| title == arg.group()) {
                Some((_, members)) => members.push(arg),
                None => groups.push((arg.group().to_string(), vec![arg])),
            }
        }
        if !positionals.is_empty() {
            groups.push(("Arguments".to_string(), positionals));
        }
        groups
    }

    /// The note for the group titled `heading`, if any.
    pub fn note_for(&self, heading: &str) -> Option<&str> {
        self.heading_notes
            .iter()
            .find(|note| note.heading == heading)
            .map(|note| note.text.as_str())
    }

    /// The usage lines, generated when none were given.
    pub fn usage_lines(&self) -> Vec<String> {
        self.usage_lines_as(self.display_name())
    }

    /// The usage lines with `prefix` (such as `rich config`) as the command.
    /// Explicit lines starting with this command's name get the prefix
    /// substituted; generated lines are
    /// `prefix [OPTIONS] <REQUIRED OPTIONS> [POSITIONALS] [COMMAND]`.
    pub fn usage_lines_as(&self, prefix: &str) -> Vec<String> {
        if !self.usage.is_empty() {
            let own = self.display_name();
            return self
                .usage
                .iter()
                .map(|line| match line.strip_prefix(own) {
                    Some(rest) if rest.is_empty() || rest.starts_with(' ') => {
                        format!("{prefix}{rest}")
                    }
                    _ => line.clone(),
                })
                .collect();
        }
        let mut parts = vec![prefix.to_string()];
        if self
            .visible_args()
            .any(|arg| !arg.positional && !arg.required)
        {
            parts.push("[OPTIONS]".into());
        }
        for arg in self.visible_args().filter(|a| !a.positional && a.required) {
            match arg.metavar() {
                Some(metavar) => parts.push(format!("{} {metavar}", arg.primary())),
                None => parts.push(arg.primary()),
            }
        }
        for arg in self.visible_args().filter(|a| a.positional) {
            parts.push(arg.names());
        }
        if self.visible_subcommands().next().is_some() {
            parts.push(
                if self.subcommand_required {
                    "<COMMAND>"
                } else {
                    "[COMMAND]"
                }
                .into(),
            );
        }
        vec![parts.join(" ")]
    }

    /// Every visible `-s`/`--long` spelling, for suggestions.
    pub fn switch_names(&self) -> Vec<String> {
        self.visible_args()
            .filter(|arg| !arg.positional)
            .flat_map(ArgSpec::switches)
            .collect()
    }

    /// Every visible subcommand name and alias, for suggestions.
    pub fn subcommand_names(&self) -> Vec<String> {
        self.visible_subcommands()
            .flat_map(|c| std::iter::once(c.name.clone()).chain(c.aliases.iter().cloned()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_metavars() {
        let arg = ArgSpec::option("width").short('w').value_name("SIZE");
        assert_eq!(arg.names(), "-w, --width <SIZE>");
        let files = ArgSpec::positional("file").multiple(true).required(true);
        assert_eq!(files.names(), "<FILE>...");
        assert_eq!(ArgSpec::positional("x").names(), "[X]");
        assert_eq!(
            ArgSpec::flag("no-color").alias("no-colour").names(),
            "--no-color, --no-colour"
        );
    }

    #[test]
    fn groups_keep_first_seen_order() {
        let spec = CommandSpec::new("x")
            .arg(ArgSpec::flag("a").heading("B"))
            .arg(ArgSpec::positional("p"))
            .arg(ArgSpec::flag("b"))
            .arg(ArgSpec::flag("c").heading("B"))
            .arg(ArgSpec::flag("h").hidden(true));
        let titles: Vec<_> = spec
            .groups()
            .into_iter()
            .map(|(t, m)| (t, m.len()))
            .collect();
        assert_eq!(
            titles,
            vec![
                ("B".into(), 2),
                ("Options".into(), 1),
                ("Arguments".into(), 1)
            ]
        );
    }

    #[test]
    fn explicit_usage_takes_the_prefix() {
        let spec = CommandSpec::new("show").usage("show [KEY]").usage("other");
        assert_eq!(
            spec.usage_lines_as("rich config show"),
            vec!["rich config show [KEY]", "other"]
        );
    }

    #[test]
    fn generated_usage_lists_required_options() {
        let spec = CommandSpec::new("x")
            .arg(ArgSpec::option("out").short('o').required(true))
            .arg(ArgSpec::positional("in").required(true));
        assert_eq!(spec.usage_lines(), vec!["x --out <OUT> <IN>"]);
    }
}
