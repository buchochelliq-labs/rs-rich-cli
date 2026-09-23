//! Reference documentation (#408): Markdown and man pages from a
//! [`CommandSpec`]. Output is deterministic: no date unless one is given.

use super::paragraphs;
use super::spec::{ArgSpec, CommandSpec};
use rich::markdown::Markdown;

/// Markdown reference for `spec` and, under deeper headings, every visible
/// subcommand. Each option group is a two-column table; hints (default,
/// environment, config key, possible values) follow the help in its cell.
///
/// ```
/// use rich_ext::cli_doc::{to_markdown, ArgSpec, CommandSpec};
///
/// let spec = CommandSpec::new("tool")
///     .about("Does things.")
///     .arg(ArgSpec::option("mode").choices(["a|b", "c"]).help("The mode"));
/// assert_eq!(
///     to_markdown(&spec),
///     "# tool\n\nDoes things.\n\n## Usage\n\n```text\ntool [OPTIONS]\n```\n\n## Options\n\n\
///      | Option | Description |\n| --- | --- |\n\
///      | `--mode <MODE>` | The mode. Possible values: `a\\|b`, `c`. |\n"
/// );
/// ```
pub fn to_markdown(spec: &CommandSpec) -> String {
    let mut out = String::new();
    markdown_command(&mut out, spec, spec.display_name(), 1);
    let mut out = out.trim_end().to_string();
    out.push('\n');
    out
}

/// [`to_markdown`] as a core [`Markdown`] renderable, to print through rich.
pub fn markdown_view(spec: &CommandSpec) -> Markdown {
    Markdown::new(&to_markdown(spec))
}

/// Inline code, fenced with enough backticks for the content.
fn code(text: &str) -> String {
    if text.contains('`') {
        format!("`` {text} ``")
    } else {
        format!("`{text}`")
    }
}

/// A table cell: pipes escaped, line breaks as `<br>`.
fn cell(text: &str) -> String {
    text.trim()
        .replace('|', r"\|")
        .replace("\r\n", "\n")
        .replace("\n\n", "<br><br>")
        .replace('\n', " ")
}

/// Help text as a sentence: a full stop added when it has none.
fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() || text.ends_with(['.', '!', '?', ':', ')']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

fn arg_description(arg: &ArgSpec) -> String {
    let mut parts = Vec::new();
    let help = arg.long_help.as_deref().unwrap_or(&arg.help);
    if !help.trim().is_empty() {
        parts.push(sentence(help));
    }
    if let Some(default) = &arg.default {
        parts.push(format!("Default: {}.", code(default)));
    }
    if let Some(env) = &arg.env {
        parts.push(format!("Environment: {}.", code(env)));
    }
    if let Some(key) = &arg.config_key {
        parts.push(format!("Config: {}.", code(key)));
    }
    let choices = arg.choice_list();
    if !choices.is_empty() {
        let values: Vec<String> = choices
            .iter()
            .map(|c| match c.help.trim() {
                "" => code(&c.value),
                help => format!("{} ({help})", code(&c.value)),
            })
            .collect();
        parts.push(format!("Possible values: {}.", values.join(", ")));
    }
    cell(&parts.join(" "))
}

fn markdown_command(out: &mut String, spec: &CommandSpec, path: &str, level: usize) {
    let title = "#".repeat(level.min(6));
    let sub = "#".repeat((level + 1).min(6));
    out.push_str(&format!("{title} {path}\n\n"));
    let about = spec.long_about.as_deref().unwrap_or(&spec.about);
    for paragraph in paragraphs(about) {
        out.push_str(&paragraph);
        out.push_str("\n\n");
    }
    out.push_str(&format!("{sub} Usage\n\n```text\n"));
    for line in spec.usage_lines_as(path) {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("```\n\n");
    for (heading, args) in spec.groups() {
        out.push_str(&format!("{sub} {heading}\n\n"));
        if let Some(note) = spec.note_for(&heading) {
            out.push_str(note.trim());
            out.push_str("\n\n");
        }
        let column = if args.iter().all(|a| a.positional) {
            "Argument"
        } else {
            "Option"
        };
        out.push_str(&format!("| {column} | Description |\n| --- | --- |\n"));
        for arg in args {
            let names = if arg.positional {
                code(&arg.names())
            } else {
                let mut switches: Vec<String> = arg.switches();
                if let (Some(last), Some(metavar)) = (switches.last_mut(), arg.metavar()) {
                    last.push(' ');
                    last.push_str(&metavar);
                }
                switches
                    .iter()
                    .map(|s| code(s))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push_str(&format!(
                "| {} | {} |\n",
                cell(&names),
                arg_description(arg)
            ));
        }
        out.push('\n');
    }
    let commands: Vec<&CommandSpec> = spec.visible_subcommands().collect();
    if !commands.is_empty() {
        let heading = spec.subcommand_heading.as_deref().unwrap_or("Commands");
        out.push_str(&format!(
            "{sub} {heading}\n\n| Command | Description |\n| --- | --- |\n"
        ));
        for command in &commands {
            let mut names = code(&command.name);
            for alias in &command.aliases {
                names.push_str(", ");
                names.push_str(&code(alias));
            }
            let about = paragraphs(&command.about)
                .into_iter()
                .next()
                .unwrap_or_default();
            out.push_str(&format!("| {} | {} |\n", cell(&names), cell(&about)));
        }
        out.push('\n');
    }
    if !spec.examples.is_empty() {
        out.push_str(&format!("{sub} Examples\n\n"));
        for example in &spec.examples {
            if !example.description.trim().is_empty() {
                out.push_str(example.description.trim());
                out.push_str("\n\n");
            }
            out.push_str(&format!("```sh\n{}\n```\n\n", example.command));
        }
    }
    for section in &spec.sections {
        if !section.title.is_empty() {
            out.push_str(&format!("{sub} {}\n\n", section.title));
        }
        for paragraph in paragraphs(&section.body) {
            out.push_str(&paragraph);
            out.push_str("\n\n");
        }
    }
    for command in commands {
        markdown_command(out, command, &format!("{path} {}", command.name), level + 1);
    }
}

// ---------------------------------------------------------------- man

/// Escape one line of running text for roff.
fn roff(text: &str) -> String {
    let escaped = text.trim().replace('\\', r"\e").replace('-', r"\-");
    if escaped.starts_with(['.', '\'']) {
        format!(r"\&{escaped}")
    } else {
        escaped
    }
}

/// A quoted macro argument.
fn roff_arg(text: &str) -> String {
    format!("\"{}\"", roff(text).replace('"', r"\(dq"))
}

/// Paragraphs of running text, `.PP` between them.
fn roff_paragraphs(out: &mut String, text: &str) {
    roff_paragraphs_with(out, text, ".PP");
}

/// Paragraphs inside a `.TP` item, `.IP` between them to keep the indent.
fn roff_item(out: &mut String, text: &str) {
    roff_paragraphs_with(out, text, ".IP");
}

fn roff_paragraphs_with(out: &mut String, text: &str, separator: &str) {
    for (i, paragraph) in paragraphs(text).into_iter().enumerate() {
        if i > 0 {
            out.push_str(separator);
            out.push('\n');
        }
        for line in paragraph.lines() {
            out.push_str(&roff(line));
            out.push('\n');
        }
    }
}

fn man_names(arg: &ArgSpec) -> String {
    let metavar = arg.value_name.as_deref().map(|name| {
        let name = name.trim_matches(['<', '>', '[', ']']);
        let dots = if arg.multiple { "..." } else { "" };
        format!(r"\fI{}\fR{dots}", roff(name))
    });
    if arg.positional {
        return metavar.unwrap_or_else(|| roff(&arg.id));
    }
    let mut out = arg
        .switches()
        .iter()
        .map(|s| format!(r"\fB{}\fR", roff(s)))
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(metavar) = metavar {
        out.push(' ');
        out.push_str(&metavar);
    }
    out
}

/// A man page for `spec`. `section` is the manual section (`"1"`); `date`
/// goes in the header only when given. Subcommands are listed under
/// COMMANDS and SEE ALSO; [`to_man_pages`] writes their own pages.
///
/// ```
/// use rich_ext::cli_doc::{to_man, ArgSpec, CommandSpec};
///
/// let spec = CommandSpec::new("tool")
///     .about("Does things")
///     .arg(ArgSpec::option("out-dir").short('o').help("Where to write"));
/// let page = to_man(&spec, "1", Some("2026-01-01"));
/// assert!(page.starts_with(".TH \"TOOL\" \"1\" \"2026-01-01\" \"tool\" \"User Commands\"\n"));
/// assert!(page.contains(".TP\n\\fB\\-o\\fR, \\fB\\-\\-out\\-dir\\fR \\fIOUT_DIR\\fR\nWhere to write\n"));
/// ```
pub fn to_man(spec: &CommandSpec, section: &str, date: Option<&str>) -> String {
    man_page(spec, spec.display_name(), None, section, date)
}

/// Man pages for `spec` and every visible subcommand, as
/// `(file name, page)`: `rich.1`, `rich-config.1`, …
pub fn to_man_pages(
    spec: &CommandSpec,
    section: &str,
    date: Option<&str>,
) -> Vec<(String, String)> {
    let mut pages = Vec::new();
    collect_pages(&mut pages, spec, spec.display_name(), None, section, date);
    pages
}

fn collect_pages(
    pages: &mut Vec<(String, String)>,
    spec: &CommandSpec,
    path: &str,
    parent: Option<&str>,
    section: &str,
    date: Option<&str>,
) {
    let page = page_name(path);
    pages.push((
        format!("{page}.{section}"),
        man_page(spec, path, parent, section, date),
    ));
    for command in spec.visible_subcommands() {
        let child = format!("{path} {}", command.name);
        collect_pages(pages, command, &child, Some(path), section, date);
    }
}

fn page_name(path: &str) -> String {
    path.split(' ').collect::<Vec<_>>().join("-")
}

fn man_page(
    spec: &CommandSpec,
    path: &str,
    parent: Option<&str>,
    section: &str,
    date: Option<&str>,
) -> String {
    let page = page_name(path);
    let mut out = String::new();
    let source = match &spec.version {
        Some(version) => format!("{path} {version}"),
        None => path.to_string(),
    };
    let manual = if section.starts_with('1') {
        "User Commands"
    } else {
        ""
    };
    // The date stays unescaped: mandoc parses `YYYY-MM-DD` only without `\-`.
    out.push_str(&format!(
        ".TH {} {} \"{}\" {} {}\n",
        roff_arg(&page.to_uppercase()),
        roff_arg(section),
        date.unwrap_or("").replace('"', ""),
        roff_arg(&source),
        roff_arg(manual)
    ));

    out.push_str(".SH NAME\n");
    let summary = paragraphs(&spec.about)
        .into_iter()
        .next()
        .unwrap_or_default();
    if summary.is_empty() {
        out.push_str(&format!("{}\n", roff(&page)));
    } else {
        out.push_str(&format!(
            "{} \\- {}\n",
            roff(&page),
            roff(&summary.replace('\n', " "))
        ));
    }

    out.push_str(".SH SYNOPSIS\n.nf\n");
    for line in spec.usage_lines_as(path) {
        match line.strip_prefix(path) {
            Some(rest) => out.push_str(&format!("\\fB{}\\fR{}\n", roff(path), roff_keep(rest))),
            None => out.push_str(&format!("{}\n", roff(&line))),
        }
    }
    out.push_str(".fi\n");

    let about = spec.long_about.as_deref().unwrap_or(&spec.about);
    if !paragraphs(about).is_empty() {
        out.push_str(".SH DESCRIPTION\n");
        roff_paragraphs(&mut out, about);
    }

    let groups = spec.groups();
    if !groups.is_empty() {
        out.push_str(".SH OPTIONS\n");
        let titled = groups.len() > 1 || groups[0].0 != "Options";
        for (heading, args) in &groups {
            if titled {
                out.push_str(&format!(".SS {}\n", roff_arg(heading)));
            }
            if let Some(note) = spec.note_for(heading) {
                roff_paragraphs(&mut out, note);
            }
            for arg in args {
                out.push_str(".TP\n");
                out.push_str(&man_names(arg));
                out.push('\n');
                let help = arg.long_help.as_deref().unwrap_or(&arg.help);
                roff_item(&mut out, help);
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
                let described = choices.iter().any(|c| !c.help.trim().is_empty());
                if !choices.is_empty() && !described {
                    let values: Vec<&str> = choices.iter().map(|c| c.value.as_str()).collect();
                    hints.push(format!("[possible values: {}]", values.join(", ")));
                }
                if !hints.is_empty() {
                    if !paragraphs(help).is_empty() {
                        out.push_str(".br\n");
                    }
                    out.push_str(&roff(&hints.join(" ")));
                    out.push('\n');
                }
                if described {
                    out.push_str(".RS\n");
                    for choice in choices {
                        out.push_str(&format!(".TP\n\\fB{}\\fR\n", roff(&choice.value)));
                        roff_item(&mut out, &choice.help);
                    }
                    out.push_str(".RE\n");
                }
            }
        }
    }

    let commands: Vec<&CommandSpec> = spec.visible_subcommands().collect();
    if !commands.is_empty() {
        out.push_str(".SH COMMANDS\n");
        for command in &commands {
            let mut names = format!(r"\fB{}\fR", roff(&command.name));
            for alias in &command.aliases {
                names.push_str(&format!(r", \fB{}\fR", roff(alias)));
            }
            out.push_str(&format!(".TP\n{names}\n"));
            let about = paragraphs(&command.about)
                .into_iter()
                .next()
                .unwrap_or_default();
            roff_item(&mut out, &sentence(&about));
            out.push_str(&format!(
                "See \\fB{}\\fR({}).\n",
                roff(&page_name(&format!("{path} {}", command.name))),
                roff(section)
            ));
        }
    }

    let env: Vec<&ArgSpec> = spec.visible_args().filter(|a| a.env.is_some()).collect();
    if !env.is_empty() {
        out.push_str(".SH ENVIRONMENT\n");
        for arg in env {
            let name = arg.env.as_deref().unwrap_or_default();
            out.push_str(&format!(".TP\n\\fB{}\\fR\n", roff(name)));
            let target = if arg.positional {
                format!(
                    r"\fI{}\fR",
                    roff(arg.value_name.as_deref().unwrap_or(&arg.id))
                )
            } else {
                format!(r"\fB{}\fR", roff(&arg.primary()))
            };
            out.push_str(&format!("Sets {target}.\n"));
        }
    }

    if !spec.examples.is_empty() {
        out.push_str(".SH EXAMPLES\n");
        for example in &spec.examples {
            if example.description.trim().is_empty() {
                out.push_str(&format!(".PP\n\\fB{}\\fR\n", roff(&example.command)));
            } else {
                out.push_str(&format!(".TP\n\\fB{}\\fR\n", roff(&example.command)));
                roff_item(&mut out, &example.description);
            }
        }
    }

    for section_spec in &spec.sections {
        let title = if section_spec.title.is_empty() {
            "NOTES".to_string()
        } else {
            section_spec.title.to_uppercase()
        };
        out.push_str(&format!(".SH {}\n", roff_arg(&title)));
        roff_paragraphs(&mut out, &section_spec.body);
    }

    let mut see_also: Vec<String> = Vec::new();
    if let Some(parent) = parent {
        see_also.push(format!(
            r"\fB{}\fR({})",
            roff(&page_name(parent)),
            roff(section)
        ));
    }
    for command in &commands {
        let name = page_name(&format!("{path} {}", command.name));
        see_also.push(format!(r"\fB{}\fR({})", roff(&name), roff(section)));
    }
    if !see_also.is_empty() {
        out.push_str(".SH \"SEE ALSO\"\n");
        out.push_str(&see_also.join(",\n"));
        out.push('\n');
    }
    out
}

/// [`roff`] without trimming, for the tail of a line.
fn roff_keep(text: &str) -> String {
    text.replace('\\', r"\e").replace('-', r"\-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roff_escapes_control_lines_and_dashes() {
        assert_eq!(roff(".hidden"), r"\&.hidden");
        assert_eq!(roff("'quote"), r"\&'quote");
        assert_eq!(roff(r"a-b\c"), r"a\-b\ec");
        assert_eq!(roff_arg("say \"hi\""), r#""say \(dqhi\(dq""#);
    }

    #[test]
    fn code_spans_fence_backticks() {
        assert_eq!(code("a`b"), "`` a`b ``");
        assert_eq!(cell("a | b\nc"), r"a \| b c");
    }
}
