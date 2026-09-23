//! Shell completion scripts (#403) and a reusable completion catalogue.
//!
//! Every script completes subcommands (recursively, aliases included), long
//! and short options, the values of options with [`ValueHint::Choices`],
//! paths for [`ValueHint::File`], [`ValueHint::Dir`] and [`ValueHint::Path`],
//! and positionals by their hint. Hidden arguments and subcommands are left
//! out. Descriptions are included where the shell shows them:
//!
//! - **bash** lists words only, and falls back to file names when it has
//!   nothing to offer (`complete -o default`).
//! - **zsh** offers option names through `_describe`, one group per heading
//!   (the heading is the group's description, so `zstyle ':completion:*'
//!   group-name ''` separates them); `_arguments` completes values and
//!   positionals.
//! - **fish** and **PowerShell** show descriptions, prefixed with the group
//!   when a command has more than one option group.
//!
//! Commands are tracked by internal node ids (`n0`, `n1`, …) so names never
//! need escaping into patterns or function names.

use super::spec::{ArgSpec, CommandSpec, ValueHint};
use super::{join_lines, style};
use rich::table::Table;
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};
use std::fmt::Write as _;

/// A shell that [`generate`] writes a completion script for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
}

impl Shell {
    pub const ALL: [Shell; 4] = [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell];
}

impl std::fmt::Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::PowerShell => "powershell",
        })
    }
}

impl std::str::FromStr for Shell {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            "powershell" | "pwsh" => Ok(Shell::PowerShell),
            _ => Err(format!(
                "unknown shell '{s}' (expected bash, zsh, fish, powershell or pwsh)"
            )),
        }
    }
}

/// A completion script for `spec` in `shell`.
///
/// ```
/// use rich_ext::cli_doc::{generate, ArgSpec, CommandSpec, Shell};
///
/// let spec = CommandSpec::new("rich").arg(ArgSpec::flag("pager"));
/// let script = generate(&spec, Shell::Bash);
/// assert!(script.contains("complete -F _rich -o bashdefault -o default 'rich'"));
/// ```
pub fn generate(spec: &CommandSpec, shell: Shell) -> String {
    let nodes = nodes(spec);
    match shell {
        Shell::Bash => bash(spec, &nodes),
        Shell::Zsh => zsh(spec, &nodes),
        Shell::Fish => fish(spec, &nodes),
        Shell::PowerShell => powershell(spec, &nodes),
    }
}

/// A command in the tree, with its id and parent.
struct Node<'a> {
    id: String,
    spec: &'a CommandSpec,
    /// Canonical subcommand names below the root.
    path: Vec<String>,
    /// `(word, child id)` for every visible subcommand name and alias.
    children: Vec<(String, String)>,
}

impl Node<'_> {
    fn options(&self) -> impl Iterator<Item = &ArgSpec> {
        self.spec.visible_args().filter(|a| !a.positional)
    }
    fn positionals(&self) -> impl Iterator<Item = &ArgSpec> {
        self.spec.visible_args().filter(|a| a.positional)
    }
    /// Whether descriptions should name their group: more than one group.
    fn grouped(&self) -> bool {
        let first = self.options().next().map(ArgSpec::group);
        self.options().any(|arg| Some(arg.group()) != first)
    }
    fn describe(&self, arg: &ArgSpec) -> String {
        let help = one_line(&arg.help);
        if self.grouped() {
            if help.is_empty() {
                arg.group().to_string()
            } else {
                format!("{}: {help}", arg.group())
            }
        } else {
            help
        }
    }
}

fn nodes(root: &CommandSpec) -> Vec<Node<'_>> {
    // Breadth-first, numbering each node as it is discovered.
    let mut out = vec![Node {
        id: "n0".into(),
        spec: root,
        path: Vec::new(),
        children: Vec::new(),
    }];
    let mut i = 0;
    while i < out.len() {
        let spec = out[i].spec;
        for child in spec.visible_subcommands() {
            let id = format!("n{}", out.len());
            for word in std::iter::once(&child.name).chain(&child.aliases) {
                out[i].children.push((word.clone(), id.clone()));
            }
            let mut path = out[i].path.clone();
            path.push(child.name.clone());
            out.push(Node {
                id,
                spec: child,
                path,
                children: Vec::new(),
            });
        }
        i += 1;
    }
    out
}

/// Collapse whitespace (and newlines) into single spaces.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A name usable in a shell function name.
fn ident(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------- bash

/// Single-quote for POSIX shells.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn bash_words<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    words
        .into_iter()
        .map(sh_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The bash statement that completes a value with `hint`.
fn bash_value(hint: &ValueHint, f: &str) -> String {
    match hint {
        ValueHint::Choices(choices) => format!(
            "{f}__match \"$cur\" {}",
            bash_words(choices.iter().map(|c| c.value.as_str()))
        ),
        ValueHint::File | ValueHint::Path => format!("{f}__compgen -f \"$cur\""),
        ValueHint::Dir => format!("{f}__compgen -d \"$cur\""),
        ValueHint::Command => format!("{f}__compgen -c \"$cur\""),
        ValueHint::None | ValueHint::Any | ValueHint::Url => ":".into(),
    }
}

fn bash(root: &CommandSpec, nodes: &[Node<'_>]) -> String {
    let f = format!("_{}", ident(root.display_name()));
    let mut s = String::new();
    let _ = writeln!(s, "# bash completion for {}", one_line(root.display_name()));
    let _ = writeln!(
        s,
        "# Generated by rich_ext::cli_doc; source it or install it under"
    );
    let _ = writeln!(s, "# ~/.local/share/bash-completion/completions/.");
    let _ = writeln!(
        s,
        r#"
{f}__match() {{
    # Add the words that start with the current word.
    local cur="$1" word
    shift
    for word in "$@"; do
        if [[ "$word" == "$cur"* ]]; then COMPREPLY+=("$word"); fi
    done
}}

{f}__compgen() {{
    # Add compgen's matches, one per line, so names with spaces survive.
    local line
    compopt -o filenames 2>/dev/null
    while IFS= read -r line; do COMPREPLY+=("$line"); done < <(compgen "$1" -- "$2")
}}

{f}() {{
    local cur="${{COMP_WORDS[COMP_CWORD]}}" prev="" node=n0 prefix="" i
    COMPREPLY=()
    if (( COMP_CWORD > 0 )); then prev="${{COMP_WORDS[COMP_CWORD-1]}}"; fi
    # `--option=value`: bash splits at `=` when it is in COMP_WORDBREAKS...
    if [[ "$cur" == "=" ]]; then cur=""; fi
    if [[ "$prev" == "=" ]] && (( COMP_CWORD > 1 )); then prev="${{COMP_WORDS[COMP_CWORD-2]}}"; fi
    # ...and keeps the word whole when it is not.
    if [[ "$cur" == --*=* ]]; then prev="${{cur%%=*}}"; prefix="$prev="; cur="${{cur#*=}}"; fi
    for (( i = 1; i < COMP_CWORD; i++ )); do
        case "$node:${{COMP_WORDS[i]}}" in"#
    );
    for node in nodes {
        for (word, child) in &node.children {
            let _ = writeln!(
                s,
                "            {}) node={child} ;;",
                sh_quote(&format!("{}:{word}", node.id))
            );
        }
    }
    s.push_str("        esac\n    done\n    case \"$node\" in\n");
    for node in nodes {
        let _ = writeln!(s, "        {})", node.id);
        s.push_str("            case \"$prev\" in\n");
        for arg in node.options().filter(|a| a.takes_value()) {
            let _ = writeln!(
                s,
                "                {}) {} ;;",
                arg.switches()
                    .iter()
                    .map(|w| sh_quote(w))
                    .collect::<Vec<_>>()
                    .join("|"),
                bash_value(&arg.value, &f)
            );
        }
        s.push_str("                *)\n");
        let switches: Vec<String> = node.options().flat_map(ArgSpec::switches).collect();
        let options = if switches.is_empty() {
            ":".to_string()
        } else {
            format!(
                "{f}__match \"$cur\" {}",
                bash_words(switches.iter().map(String::as_str))
            )
        };
        let _ = writeln!(
            s,
            "                    if [[ \"$cur\" == -* ]]; then\n                        {options}\n                    else"
        );
        let mut rest = Vec::new();
        if !node.children.is_empty() {
            rest.push(format!(
                "{f}__match \"$cur\" {}",
                bash_words(node.children.iter().map(|(w, _)| w.as_str()))
            ));
        }
        // Positionals are offered by the union of their hints.
        let mut seen = Vec::new();
        for arg in node.positionals() {
            let line = bash_value(&arg.value, &f);
            if line != ":" && !seen.contains(&line) {
                seen.push(line.clone());
                rest.push(line);
            }
        }
        if rest.is_empty() {
            rest.push(":".into());
        }
        for line in rest {
            let _ = writeln!(s, "                        {line}");
        }
        s.push_str("                    fi ;;\n            esac ;;\n");
    }
    let _ = writeln!(
        s,
        r#"    esac
    if [[ -n "$prefix" ]]; then
        for i in "${{!COMPREPLY[@]}}"; do COMPREPLY[i]="$prefix${{COMPREPLY[i]}}"; done
    fi
    return 0
}}

complete -F {f} -o bashdefault -o default {}"#,
        sh_quote(root.display_name())
    );
    s
}

// ---------------------------------------------------------------- zsh

fn zsh_help(s: &str) -> String {
    one_line(s)
        .replace('\\', r"\\")
        .replace('[', r"\[")
        .replace(']', r"\]")
        .replace(':', r"\:")
        .replace('$', r"\$")
        .replace('`', r"\`")
}

fn zsh_value(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '[' | ']' | ':' | '$' | '`' | '(' | ')' | ' ' | '"' | '\''
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A `_describe` entry: `name:description`, the name's colons escaped.
fn zsh_describe_entry(name: &str, help: &str) -> String {
    let name = name.replace('\\', r"\\").replace(':', r"\:");
    let help = one_line(help);
    if help.is_empty() {
        sh_quote(&name)
    } else {
        sh_quote(&format!("{name}:{help}"))
    }
}

/// The action part of an `_arguments` spec.
fn zsh_action(hint: &ValueHint) -> String {
    match hint {
        ValueHint::Choices(choices) if choices.iter().any(|c| !c.help.is_empty()) => {
            let items: Vec<String> = choices
                .iter()
                .map(|c| {
                    let help = one_line(&c.help)
                        .replace('\\', r"\\")
                        .replace('"', "\\\"")
                        .replace('$', r"\$")
                        .replace('`', r"\`");
                    format!("{}\\:\"{help}\"", zsh_value(&c.value))
                })
                .collect();
            format!("(({}))", items.join(" "))
        }
        ValueHint::Choices(choices) => {
            let items: Vec<String> = choices.iter().map(|c| zsh_value(&c.value)).collect();
            format!("({})", items.join(" "))
        }
        ValueHint::File | ValueHint::Path => "_files".into(),
        ValueHint::Dir => "_files -/".into(),
        ValueHint::Command => "_command_names -e".into(),
        ValueHint::Url => "_urls".into(),
        ValueHint::None | ValueHint::Any => " ".into(),
    }
}

/// Statements completing a value with `hint`, for use inside a state.
fn zsh_statement(hint: &ValueHint, message: &str) -> Option<String> {
    Some(match hint {
        ValueHint::Choices(choices) => {
            let items: Vec<String> = choices
                .iter()
                .map(|c| zsh_describe_entry(&c.value, &c.help))
                .collect();
            format!(
                "local -a values=({}); _describe -t values {} values && ret=0",
                items.join(" "),
                sh_quote(message)
            )
        }
        ValueHint::File | ValueHint::Path => "_files && ret=0".into(),
        ValueHint::Dir => "_files -/ && ret=0".into(),
        ValueHint::Command => "_command_names -e && ret=0".into(),
        ValueHint::Url => "_urls && ret=0".into(),
        ValueHint::None | ValueHint::Any => return None,
    })
}

fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn zsh(root: &CommandSpec, nodes: &[Node<'_>]) -> String {
    let base = format!("_{}", ident(root.display_name()));
    let fname = |node: &Node<'_>| {
        let mut name = base.clone();
        for part in &node.path {
            name.push_str("__");
            name.push_str(&ident(part));
        }
        // Distinct paths can share an identifier ("a-b" and "a_b").
        if node.id != "n0" {
            name.push_str("__");
            name.push_str(&node.id);
        }
        name
    };
    let mut s = String::new();
    let _ = writeln!(s, "#compdef {}", root.display_name());
    let _ = writeln!(
        s,
        "# zsh completion, generated by rich_ext::cli_doc. Put it on $fpath as"
    );
    let _ = writeln!(s, "# `{base}`, or source it after compinit.");
    for node in nodes {
        let name = fname(node);
        let _ = writeln!(s, "\n{name}() {{");
        s.push_str("    local cur=${words[CURRENT]} prev=${words[CURRENT-1]} ret=1\n");
        let values: Vec<String> = node
            .options()
            .filter(|a| a.takes_value())
            .flat_map(ArgSpec::switches)
            .map(|w| sh_quote(&w))
            .collect();
        let _ = writeln!(s, "    local -a value_options=({})", values.join(" "));
        // Option names, one `_describe` group per heading.
        s.push_str(
            "    if [[ $cur == -* && $cur != *=* ]] && (( ! ${value_options[(Ie)$prev]} )); then\n",
        );
        s.push_str("        local -a group\n");
        let mut groups: Vec<(&str, Vec<&ArgSpec>)> = Vec::new();
        for arg in node.options() {
            match groups.iter_mut().find(|(title, _)| *title == arg.group()) {
                Some((_, members)) => members.push(arg),
                None => groups.push((arg.group(), vec![arg])),
            }
        }
        for (title, members) in &groups {
            let entries: Vec<String> = members
                .iter()
                .flat_map(|arg| {
                    arg.switches()
                        .into_iter()
                        .map(|w| zsh_describe_entry(&w, &arg.help))
                        .collect::<Vec<_>>()
                })
                .collect();
            let _ = writeln!(s, "        group=({})", entries.join(" "));
            let tag = slug(title);
            let _ = writeln!(
                s,
                "        _describe -t {} {} group && ret=0",
                sh_quote(&tag),
                sh_quote(title)
            );
        }
        s.push_str("        return ret\n    fi\n");
        s.push_str("    local curcontext=$curcontext state state_descr line\n");
        s.push_str("    typeset -A opt_args\n");
        s.push_str("    _arguments -s -S -C");
        let mut specs: Vec<String> = Vec::new();
        for arg in node.options() {
            let switches = arg.switches();
            let exclusion = if arg.multiple {
                "*".to_string()
            } else {
                format!("({})", switches.join(" "))
            };
            let help = zsh_help(&arg.help);
            let desc = if help.is_empty() {
                String::new()
            } else {
                format!("[{help}]")
            };
            for switch in &switches {
                let spec = if arg.takes_value() {
                    let suffix = if switch.starts_with("--") { "=" } else { "+" };
                    let message = zsh_help(arg.value_name.as_deref().unwrap_or(&arg.id));
                    format!(
                        "{exclusion}{switch}{suffix}{desc}:{message}:{}",
                        zsh_action(&arg.value)
                    )
                } else {
                    format!("{exclusion}{switch}{desc}")
                };
                specs.push(sh_quote(&spec));
            }
        }
        let positionals: Vec<&ArgSpec> = node.positionals().collect();
        let message = |arg: &ArgSpec| {
            if arg.help.is_empty() {
                zsh_help(arg.value_name.as_deref().unwrap_or(&arg.id))
            } else {
                zsh_help(&arg.help)
            }
        };
        if node.children.is_empty() {
            for arg in &positionals {
                let lead = if arg.multiple {
                    "*:"
                } else if arg.required {
                    ":"
                } else {
                    "::"
                };
                specs.push(sh_quote(&format!(
                    "{lead}{}:{}",
                    message(arg),
                    zsh_action(&arg.value)
                )));
            }
        } else {
            specs.push(sh_quote(": :->command"));
            specs.push(sh_quote("*::: :->args"));
        }
        for spec in &specs {
            let _ = write!(s, " \\\n        {spec}");
        }
        s.push_str(" && ret=0\n");
        if !node.children.is_empty() {
            s.push_str("    case $state in\n        (command)\n");
            let commands: Vec<String> = node
                .spec
                .visible_subcommands()
                .flat_map(|c| {
                    std::iter::once(&c.name)
                        .chain(&c.aliases)
                        .map(|w| zsh_describe_entry(w, &c.about))
                        .collect::<Vec<_>>()
                })
                .collect();
            let heading = node
                .spec
                .subcommand_heading
                .as_deref()
                .unwrap_or("Commands");
            let _ = writeln!(s, "            local -a commands=({})", commands.join(" "));
            let _ = writeln!(
                s,
                "            _describe -t commands {} commands && ret=0",
                sh_quote(heading)
            );
            if let Some(line) = positionals
                .first()
                .and_then(|a| zsh_statement(&a.value, &message(a)))
            {
                let _ = writeln!(s, "            {line}");
            }
            s.push_str("            ;;\n        (args)\n");
            s.push_str("            words=($line[1] \"${words[@]}\")\n");
            s.push_str("            (( CURRENT += 1 ))\n");
            let _ = writeln!(
                s,
                "            curcontext=\"${{curcontext%:*:*}}:{}-$line[1]:\"",
                ident(root.display_name())
            );
            s.push_str("            case $line[1] in\n");
            for child in node.spec.visible_subcommands() {
                let id = &node
                    .children
                    .iter()
                    .find(|(w, _)| *w == child.name)
                    .map(|(_, id)| id.clone())
                    .unwrap_or_default();
                let target = nodes
                    .iter()
                    .find(|n| &n.id == id)
                    .map(&fname)
                    .unwrap_or_default();
                let words: Vec<String> = std::iter::once(&child.name)
                    .chain(&child.aliases)
                    .map(|w| sh_quote(w))
                    .collect();
                let _ = writeln!(
                    s,
                    "                ({}) {target} && ret=0 ;;",
                    words.join("|")
                );
            }
            // A word that is no subcommand was a positional; complete the rest.
            let rest = positionals
                .get(1)
                .or(positionals.first().filter(|a| a.multiple))
                .and_then(|a| zsh_statement(&a.value, &message(a)));
            if let Some(line) = rest {
                let _ = writeln!(s, "                (*) {line} ;;");
            }
            s.push_str("            esac\n            ;;\n    esac\n");
        }
        s.push_str("    return ret\n}\n");
    }
    let _ = writeln!(
        s,
        "\nif [[ $zsh_eval_context[-1] == loadautofunc ]]; then\n    {base} \"$@\"\nelse\n    compdef {base} {}\nfi",
        sh_quote(root.display_name())
    );
    s
}

// ---------------------------------------------------------------- fish

fn fish_quote(s: &str) -> String {
    format!(
        "'{}'",
        one_line(s).replace('\\', r"\\").replace('\'', r"\'")
    )
}

fn fish(root: &CommandSpec, nodes: &[Node<'_>]) -> String {
    let base = format!("__fish_{}", ident(root.display_name()));
    let bin = fish_quote(root.display_name());
    let mut s = String::new();
    let _ = writeln!(s, "# fish completion for {}", one_line(root.display_name()));
    let _ = writeln!(s, "# Generated by rich_ext::cli_doc; install it under");
    let _ = writeln!(s, "# ~/.config/fish/completions/.");
    let _ = writeln!(s, "\nfunction {base}_node");
    s.push_str("    # The subcommand node the command line is in.\n");
    s.push_str("    set -l tokens (commandline -opc)\n    set -e tokens[1]\n    set -l node n0\n");
    s.push_str("    for token in $tokens\n");
    let mut first = true;
    for node in nodes {
        for (word, child) in &node.children {
            let _ = writeln!(
                s,
                "        {} test \"$node:$token\" = {}\n            set node {child}",
                if first { "if" } else { "else if" },
                fish_quote(&format!("{}:{word}", node.id))
            );
            first = false;
        }
    }
    if !first {
        s.push_str("        end\n");
    }
    s.push_str("    end\n    echo $node\nend\n");
    let _ = writeln!(
        s,
        "\nfunction {base}_at\n    test ({base}_node) = \"$argv[1]\"\nend"
    );
    // Value lists live in functions: `printf` lines of `value<TAB>help`.
    let choice_fn = |node: &Node<'_>, index: usize| format!("{base}_{}_{index}", node.id);
    for node in nodes {
        for (index, arg) in node.spec.args.iter().enumerate() {
            let choices = arg.choice_list();
            if arg.hidden || choices.is_empty() {
                continue;
            }
            let items: Vec<String> = choices
                .iter()
                .map(|c| format!("{} {}", fish_quote(&c.value), fish_quote(&c.help)))
                .collect();
            let _ = writeln!(
                s,
                "\nfunction {}\n    printf '%s\\t%s\\n' {}\nend",
                choice_fn(node, index),
                items.join(" ")
            );
        }
    }
    for node in nodes {
        let condition = fish_quote(&format!("{base}_at {}", node.id));
        let head = format!("complete -c {bin} -n {condition}");
        s.push('\n');
        let wants_files = node
            .positionals()
            .any(|a| matches!(a.value, ValueHint::File | ValueHint::Path | ValueHint::Any));
        if !wants_files {
            let _ = writeln!(s, "{head} -f");
        }
        for (index, arg) in node.spec.args.iter().enumerate() {
            if arg.hidden {
                continue;
            }
            let value = match &arg.value {
                ValueHint::Choices(_) => format!(" -x -a '({})'", choice_fn(node, index)),
                ValueHint::File | ValueHint::Path => " -r -F".into(),
                ValueHint::Dir => " -x -a '(__fish_complete_directories)'".into(),
                ValueHint::Command => " -x -a '(__fish_complete_command)'".into(),
                ValueHint::Url => " -x".into(),
                ValueHint::Any => " -r".into(),
                ValueHint::None => String::new(),
            };
            if arg.positional {
                // Only positionals with something specific to offer.
                if matches!(
                    arg.value,
                    ValueHint::Choices(_) | ValueHint::Dir | ValueHint::Command
                ) {
                    let value = value.replace(" -x", "");
                    let _ = writeln!(s, "{head}{value} -d {}", fish_quote(&node.describe(arg)));
                }
                continue;
            }
            let mut names = String::new();
            if let Some(short) = arg.short {
                let _ = write!(names, " -s {}", fish_quote(&short.to_string()));
            }
            for long in arg.long.iter().chain(&arg.aliases) {
                let _ = write!(names, " -l {}", fish_quote(long));
            }
            let _ = writeln!(
                s,
                "{head}{names}{value} -d {}",
                fish_quote(&node.describe(arg))
            );
        }
        for command in node.spec.visible_subcommands() {
            for word in std::iter::once(&command.name).chain(&command.aliases) {
                let _ = writeln!(
                    s,
                    "{head} -a {} -d {}",
                    fish_quote(word),
                    fish_quote(
                        &super::paragraphs(&command.about)
                            .into_iter()
                            .next()
                            .unwrap_or_default()
                    )
                );
            }
        }
    }
    s
}

// ---------------------------------------------------------------- PowerShell

fn ps_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in one_line(s).chars() {
        // PowerShell also treats the typographic single quotes as quotes.
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

fn ps_result(word: &str, kind: &str, tooltip: &str) -> String {
    let tooltip = if tooltip.trim().is_empty() {
        word
    } else {
        tooltip
    };
    format!(
        "[CompletionResult]::new({w}, {w}, [CompletionResultType]::{kind}, {})",
        ps_quote(tooltip),
        w = ps_quote(word)
    )
}

fn powershell(root: &CommandSpec, nodes: &[Node<'_>]) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# PowerShell completion for {}",
        one_line(root.display_name())
    );
    s.push_str("# Generated by rich_ext::cli_doc; dot-source it from your $PROFILE.\n\n");
    s.push_str("using namespace System.Management.Automation\n");
    s.push_str("using namespace System.Management.Automation.Language\n\n");
    let _ = writeln!(
        s,
        "Register-ArgumentCompleter -Native -CommandName {} -ScriptBlock {{",
        ps_quote(root.display_name())
    );
    s.push_str("    param($wordToComplete, $commandAst, $cursorPosition)\n\n");
    s.push_str("    # Subcommand transitions, case-sensitive: 'node:word' -> node.\n");
    s.push_str(
        "    $transitions = [System.Collections.Generic.Dictionary[string, string]]::new()\n",
    );
    for node in nodes {
        for (word, child) in &node.children {
            let _ = writeln!(
                s,
                "    $transitions[{}] = '{child}'",
                ps_quote(&format!("{}:{word}", node.id))
            );
        }
    }
    s.push_str(
        r#"    $node = 'n0'
    $prev = ''
    foreach ($element in @($commandAst.CommandElements | Select-Object -Skip 1)) {
        if ($element.Extent.EndOffset -ge $cursorPosition) { break }
        $text = $element.Extent.Text
        if ($transitions.ContainsKey("${node}:$text")) { $node = $transitions["${node}:$text"] }
        $prev = $text
    }

    $results = switch ($node) {
"#,
    );
    for node in nodes {
        let _ = writeln!(s, "        '{}' {{", node.id);
        for arg in node.options().filter(|a| a.takes_value()) {
            let test: Vec<String> = arg
                .switches()
                .iter()
                .map(|w| format!("$prev -ceq {}", ps_quote(w)))
                .collect();
            let _ = writeln!(s, "            if ({}) {{", test.join(" -or "));
            match &arg.value {
                ValueHint::Choices(choices) => {
                    for choice in choices {
                        let _ = writeln!(
                            s,
                            "                {}",
                            ps_result(&choice.value, "ParameterValue", &choice.help)
                        );
                    }
                    s.push_str("                break\n");
                }
                // Returning nothing falls back to PowerShell's path completion.
                _ => s.push_str("                return\n"),
            }
            s.push_str("            }\n");
        }
        s.push_str("            if ($wordToComplete.StartsWith('-')) {\n");
        for arg in node.options() {
            let description = node.describe(arg);
            for switch in arg.switches() {
                let _ = writeln!(
                    s,
                    "                {}",
                    ps_result(&switch, "ParameterName", &description)
                );
            }
        }
        s.push_str("                break\n            }\n");
        for command in node.spec.visible_subcommands() {
            let about = super::paragraphs(&command.about)
                .into_iter()
                .next()
                .unwrap_or_default();
            for word in std::iter::once(&command.name).chain(&command.aliases) {
                let _ = writeln!(
                    s,
                    "            {}",
                    ps_result(word, "ParameterValue", &about)
                );
            }
        }
        for arg in node.positionals() {
            for choice in arg.choice_list() {
                let _ = writeln!(
                    s,
                    "            {}",
                    ps_result(&choice.value, "ParameterValue", &choice.help)
                );
            }
        }
        s.push_str("            break\n        }\n");
    }
    s.push_str(
        r#"    }
    $results | Where-Object { $_.CompletionText.StartsWith($wordToComplete, [System.StringComparison]::Ordinal) }
}
"#,
    );
    s
}

// ---------------------------------------------------------------- catalogue

/// What a completion word is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    Subcommand,
    Option,
    Value,
}

impl CompletionKind {
    fn name(self) -> &'static str {
        match self {
            CompletionKind::Subcommand => "subcommand",
            CompletionKind::Option => "option",
            CompletionKind::Value => "value",
        }
    }
}

/// One word a shell could complete, with where and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    /// The subcommand path below the root (empty for the root).
    pub path: Vec<String>,
    pub word: String,
    pub description: String,
    /// The heading (options), the subcommand heading (subcommands), or the
    /// option or positional the value belongs to (values).
    pub group: String,
    pub kind: CompletionKind,
}

/// Every completion word of a command tree: reusable metadata for other
/// completion systems, and a renderable table.
///
/// ```
/// use rich_ext::cli_doc::{ArgSpec, CommandSpec, CompletionCatalog, CompletionKind};
///
/// let spec = CommandSpec::new("rich")
///     .arg(ArgSpec::option("color").choice("always", "Always colour"))
///     .subcommand(CommandSpec::new("config").about("Show settings"));
/// let catalog = CompletionCatalog::from_spec(&spec);
/// let words: Vec<&str> = catalog.items.iter().map(|i| i.word.as_str()).collect();
/// assert_eq!(words, ["--color", "always", "config"]);
/// assert_eq!(catalog.items[1].kind, CompletionKind::Value);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionCatalog {
    /// The root command's name, shown before each path.
    pub name: String,
    pub items: Vec<CompletionItem>,
}

impl CompletionCatalog {
    pub fn from_spec(spec: &CommandSpec) -> Self {
        let mut items = Vec::new();
        collect(spec, &mut Vec::new(), &mut items);
        CompletionCatalog {
            name: spec.display_name().to_string(),
            items,
        }
    }
}

fn collect(spec: &CommandSpec, path: &mut Vec<String>, items: &mut Vec<CompletionItem>) {
    let mut push = |word: String, description: String, group: String, kind| {
        items.push(CompletionItem {
            path: path.clone(),
            word,
            description,
            group,
            kind,
        })
    };
    for arg in spec.visible_args() {
        if !arg.positional {
            for switch in arg.switches() {
                push(
                    switch,
                    one_line(&arg.help),
                    arg.group().to_string(),
                    CompletionKind::Option,
                );
            }
        }
        for choice in arg.choice_list() {
            push(
                choice.value.clone(),
                one_line(&choice.help),
                arg.names(),
                CompletionKind::Value,
            );
        }
    }
    let heading = spec.subcommand_heading.as_deref().unwrap_or("Commands");
    for command in spec.visible_subcommands() {
        let about = super::paragraphs(&command.about)
            .into_iter()
            .next()
            .unwrap_or_default();
        for word in std::iter::once(&command.name).chain(&command.aliases) {
            push(
                word.clone(),
                one_line(&about),
                heading.to_string(),
                CompletionKind::Subcommand,
            );
        }
    }
    for command in spec.visible_subcommands() {
        path.push(command.name.clone());
        collect(command, path, items);
        path.pop();
    }
}

impl Renderable for CompletionCatalog {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        let mut table = Table::new();
        for header in ["Command", "Word", "Kind", "Group", "Description"] {
            table.add_column(header);
        }
        for item in &self.items {
            let command = std::iter::once(self.name.as_str())
                .chain(item.path.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let key = match item.kind {
                CompletionKind::Subcommand => "help.command",
                CompletionKind::Option => "help.option",
                CompletionKind::Value => "help.metavar",
            };
            table.add_row_text(vec![
                Text::new(command),
                Text::styled(item.word.clone(), style(c, key)),
                Text::new(item.kind.name()),
                Text::new(item.group.clone()),
                Text::new(item.description.clone()),
            ]);
        }
        join_lines(c.render_lines(&table, o, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_are_numbered_breadth_first() {
        let spec = CommandSpec::new("a")
            .subcommand(CommandSpec::new("b").subcommand(CommandSpec::new("d")))
            .subcommand(CommandSpec::new("c").alias("cc"));
        let nodes = nodes(&spec);
        let ids: Vec<(String, String)> = nodes
            .iter()
            .map(|n| (n.id.clone(), n.path.join(" ")))
            .collect();
        assert_eq!(
            ids,
            vec![
                ("n0".into(), "".into()),
                ("n1".into(), "b".into()),
                ("n2".into(), "c".into()),
                ("n3".into(), "b d".into()),
            ]
        );
        assert_eq!(
            nodes[0].children,
            vec![
                ("b".into(), "n1".into()),
                ("c".into(), "n2".into()),
                ("cc".into(), "n2".into())
            ]
        );
    }

    #[test]
    fn shells_parse_and_display() {
        for shell in Shell::ALL {
            assert_eq!(shell.to_string().parse::<Shell>(), Ok(shell));
        }
        assert_eq!("pwsh".parse::<Shell>(), Ok(Shell::PowerShell));
        assert!("tcsh".parse::<Shell>().is_err());
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(fish_quote(r"a\b'c"), r"'a\\b\'c'");
        assert_eq!(ps_quote("it’s"), "'it’’s'");
        assert_eq!(zsh_help("a [b]: $x `y`"), r"a \[b\]\: \$x \`y\`");
    }
}
