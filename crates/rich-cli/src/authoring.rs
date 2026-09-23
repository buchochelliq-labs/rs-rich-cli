//! `rich completions` and `rich docs`: shell completion scripts, Markdown and
//! man pages generated from [`cli_spec::spec`]. A binary-boundary convenience
//! (see `docs/PORTING.md`); `rich config explain|reference` live in
//! `config.rs` next to `config show|validate`.

use super::cli_spec;
use super::{emit_error, mode_flag_alias, wants_json_report, ExitClass, VALUE_OPTIONS};
use rich_ext::cli_doc::{generate, suggest, to_man, to_man_pages, to_markdown, Shell};
use std::path::{Path, PathBuf};

/// A parsed authoring command.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Help(Vec<&'static str>),
    Completions(Shell),
    Markdown,
    ConfigMarkdown,
    Man(Option<PathBuf>),
}

/// The authoring command word, when it is the first positional and no render
/// mode is selected (`rich -p docs` still prints the word "docs").
fn command_word(args: &[String]) -> Option<&'static str> {
    let mut iter = args.iter();
    let mut word = None;
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
            continue;
        }
        if mode_flag_alias(arg).is_some() {
            return None;
        }
        if word.is_none() && (!arg.starts_with('-') || arg == "-") {
            word = Some(arg.as_str());
        }
    }
    match word? {
        "completions" => Some("completions"),
        "docs" => Some("docs"),
        _ => None,
    }
}

/// Options every authoring command accepts and ignores (they matter to the
/// rest of the binary): `--report` is read by the error path.
fn common(arg: &str) -> bool {
    matches!(
        arg,
        "--no-color" | "--color" | "--no-config" | "--machine-json" | "--report"
    )
}

fn unknown(arg: &str, path: &str, allowed: &[&str]) -> String {
    let mut candidates: Vec<&str> = allowed.to_vec();
    candidates.extend(["--help", "--no-color", "--color", "--report"]);
    let close = suggest(arg, candidates);
    let hint = match close.first() {
        Some(close) => format!("; did you mean {close}?"),
        None => String::new(),
    };
    format!("unknown option {arg:?} for rich {path}{hint}")
}

/// Parse an authoring command line, or `Ok(None)` when it is not one.
pub(crate) fn parse(args: &[String]) -> Result<Option<Command>, String> {
    let Some(word) = command_word(args) else {
        return Ok(None);
    };
    let mut positionals: Vec<&str> = Vec::new();
    let mut output = None;
    let mut help = false;
    let mut local = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--" => {
                positionals.extend(iter.by_ref().map(String::as_str));
                break;
            }
            "-h" | "--help" => help = true,
            "--report" => {
                match iter.next().map(String::as_str) {
                    Some("human" | "json") => {}
                    _ => return Err("--report requires human or json".into()),
                };
            }
            "--output" => {
                output = Some(PathBuf::from(
                    iter.next().ok_or("--output requires a directory")?,
                ));
                local.push("--output");
            }
            flag if common(flag) => {}
            flag if flag.starts_with('-') && flag != "-" => local.push(flag),
            word => positionals.push(word),
        }
    }
    let path: Vec<&'static str> = match (word, positionals.get(1).copied()) {
        ("docs", Some("markdown")) => vec!["docs", "markdown"],
        ("docs", Some("man")) => vec!["docs", "man"],
        ("docs", Some("config")) => vec!["docs", "config"],
        (word, _) => vec![word],
    };
    if help {
        return Ok(Some(Command::Help(path)));
    }
    let allowed: &[&str] = if path == ["docs", "man"] {
        &["--output"]
    } else {
        &[]
    };
    if let Some(flag) = local.iter().find(|flag| !allowed.contains(flag)) {
        return Err(unknown(flag, &path.join(" "), allowed));
    }
    let extra = positionals.len() > path.len() + usize::from(word == "completions");
    const FORMATS: [&str; 3] = ["markdown", "man", "config"];
    match (word, positionals.get(1).copied()) {
        ("completions", None) => {
            Err("completions requires a shell: bash, zsh, fish or powershell".into())
        }
        ("completions", Some(_)) if extra => Err("completions takes exactly one shell".into()),
        ("completions", Some(shell)) => {
            shell.parse().map(|shell| Some(Command::Completions(shell)))
        }
        ("docs", Some(other)) if !FORMATS.contains(&other) => {
            Err(match suggest(other, FORMATS).first() {
                Some(close) => format!("unknown docs format {other:?}; did you mean {close}?"),
                None => format!("unknown docs format {other:?}; use markdown, man or config"),
            })
        }
        ("docs", None) => Err("docs requires markdown, man or config".into()),
        _ if extra => Err(format!(
            "rich {} takes no arguments; unexpected {:?}",
            path.join(" "),
            positionals[path.len()]
        )),
        (_, Some("markdown")) => Ok(Some(Command::Markdown)),
        (_, Some("config")) => Ok(Some(Command::ConfigMarkdown)),
        _ => Ok(Some(Command::Man(output))),
    }
}

/// Run an authoring command. `Ok(false)` means the arguments are not one;
/// `Err` is a usage error for the caller to report. A failure to write man
/// pages is an input/output error, reported here with its own exit code.
pub(crate) fn dispatch(args: &[String]) -> Result<bool, String> {
    let Some(command) = parse(args)? else {
        return Ok(false);
    };
    let spec = cli_spec::spec();
    match command {
        Command::Help(path) => {
            let no_color = cli_spec::no_color_requested(args);
            if let Some(help) = cli_spec::subcommand_help(&path, no_color) {
                out(&format!("{help}\n"));
            }
        }
        Command::Completions(shell) => out(&generate(&spec, shell)),
        Command::Markdown => out(&to_markdown(&spec)),
        Command::ConfigMarkdown => out(&cli_spec::config_reference().to_markdown()),
        Command::Man(None) => out(&to_man(&spec, "1", None)),
        Command::Man(Some(dir)) => {
            if let Err(message) = write_man_pages(&spec, &dir) {
                let _ = emit_error(wants_json_report(args), ExitClass::Input, &message);
                std::process::exit(i32::from(ExitClass::Input.code()));
            }
        }
    }
    Ok(true)
}

/// Write to stdout, ignoring a closed pipe (`rich docs markdown | head`).
fn out(text: &str) {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

fn write_man_pages(spec: &rich_ext::cli_doc::CommandSpec, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for (name, page) in to_man_pages(spec, "1", None) {
        let path = dir.join(&name);
        std::fs::write(&path, page).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        out(&format!("{}\n", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Option<Command>, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn recognises_only_the_first_positional_outside_render_modes() {
        assert_eq!(parsed(&["-p", "docs"]), Ok(None));
        assert_eq!(parsed(&["--title", "docs", "file.md"]), Ok(None));
        assert_eq!(parsed(&["readme.md", "docs"]), Ok(None));
        assert_eq!(parsed(&["--", "docs", "markdown"]), Ok(None));
        assert_eq!(
            parsed(&["--no-color", "docs", "markdown"]),
            Ok(Some(Command::Markdown))
        );
    }

    #[test]
    fn parses_shells_formats_and_help() {
        assert_eq!(
            parsed(&["completions", "pwsh"]),
            Ok(Some(Command::Completions(Shell::PowerShell)))
        );
        assert!(parsed(&["completions", "tcsh"])
            .unwrap_err()
            .contains("unknown shell"));
        assert!(parsed(&["completions"]).is_err());
        assert!(parsed(&["completions", "bash", "zsh"]).is_err());
        assert_eq!(
            parsed(&["docs", "man", "--output", "out"]),
            Ok(Some(Command::Man(Some("out".into()))))
        );
        assert_eq!(
            parsed(&["docs", "config"]),
            Ok(Some(Command::ConfigMarkdown))
        );
        assert!(parsed(&["docs", "mark"])
            .unwrap_err()
            .contains("did you mean markdown"));
        assert!(parsed(&["docs", "markdown", "--outpt", "x"])
            .unwrap_err()
            .contains("unknown option"));
        assert!(parsed(&["docs", "markdown", "--output", "x"]).is_err());
        assert_eq!(
            parsed(&["docs", "man", "--help"]),
            Ok(Some(Command::Help(vec!["docs", "man"])))
        );
        assert_eq!(
            parsed(&["completions", "-h"]),
            Ok(Some(Command::Help(vec!["completions"])))
        );
    }

    #[test]
    fn every_help_path_exists_in_the_spec() {
        let spec = cli_spec::spec();
        for path in [
            &["completions"][..],
            &["docs"],
            &["docs", "markdown"],
            &["docs", "man"],
            &["docs", "config"],
            &["config"],
            &["config", "explain"],
            &["config", "reference"],
        ] {
            assert!(
                rich_ext::cli_doc::HelpView::for_path(&spec, path).is_some(),
                "{path:?}"
            );
        }
    }
}
