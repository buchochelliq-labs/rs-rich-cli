//! CLI authoring (#38, #403, #407, #408, #409, #413): help, errors,
//! completions, docs, config reference and precedence from one spec.
use rich::measure::Measurement;
use rich::{ColorSystem, Console, Renderable};
use rich_ext::cli_doc::*;
use std::path::PathBuf;
use std::process::Command;

fn sample() -> CommandSpec {
    CommandSpec::new("rich")
        .version("1.0.0")
        .about("Render rich text in the terminal")
        .long_about(
            "Render rich text in the terminal.\n\nRESOURCE is a file path, an http(s) URL, or - for stdin.",
        )
        .arg(
            ArgSpec::option("width")
                .short('w')
                .value_name("SIZE")
                .help("Render the output this many columns wide")
                .default_value("80")
                .env("RICH_WIDTH")
                .config_key("layout.width")
                .heading("Layout"),
        )
        .arg(
            ArgSpec::option("panel")
                .value_name("BOX")
                .choice("rounded", "Rounded corners")
                .choice("heavy", "Thick lines")
                .help("Wrap the output in a panel")
                .heading("Layout"),
        )
        .heading_note("Layout", "How the output is sized and framed.")
        .arg(
            ArgSpec::option("color")
                .value_name("WHEN")
                .choices(["auto", "always", "never"])
                .default_value("auto")
                .help("When to use colour"),
        )
        .arg(ArgSpec::flag("verbose").short('v').multiple(true).help("More output"))
        .arg(ArgSpec::flag("secret").hidden(true).help("Never shown"))
        .arg(
            ArgSpec::positional("resource")
                .value(ValueHint::File)
                .help("File, URL or - for stdin"),
        )
        .subcommand(
            CommandSpec::new("config")
                .about("Show and check settings")
                .alias("cfg")
                .arg(ArgSpec::flag("json").help("Print JSON"))
                .subcommand(CommandSpec::new("show").about("Show the effective settings"))
                .subcommand(CommandSpec::new("validate").about("Validate the config file")),
        )
        .subcommand(CommandSpec::new("print").about("Render literal markup"))
        .subcommand(CommandSpec::new("internal").hidden(true))
        .example("rich README.md", "Render a Markdown file")
        .example("rich -w 60 data.json", "")
        .section("Exit status", "0 on success.\n\n2 on a usage error.")
}

fn render(renderable: &dyn Renderable, width: usize) -> String {
    let out = Console::builder()
        .width(width)
        .build()
        .render_to_string(renderable);
    // Rows carry no trailing padding.
    for line in out.lines() {
        assert_eq!(line, line.trim_end(), "trailing spaces in {line:?}");
    }
    out
}

fn colored(renderable: &dyn Renderable, width: usize) -> String {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build()
        .render_to_string(renderable)
}

// ---------------------------------------------------------------- help

const WIDE: &str = "\
Usage: rich [OPTIONS] [RESOURCE] [COMMAND]

Render rich text in the terminal

Layout:
  How the output is sized and framed.
  -w, --width <SIZE>  Render the output this many columns wide [default: 80] [env: RICH_WIDTH]
                      [config: layout.width]
      --panel <BOX>   Wrap the output in a panel
                      Possible values:
                      - rounded: Rounded corners
                      - heavy: Thick lines

Options:
      --color <WHEN>  When to use colour [default: auto] [possible values: auto, always, never]
  -v, --verbose       More output

Arguments:
  [RESOURCE]          File, URL or - for stdin

Commands:
  config              Show and check settings [aliases: cfg]
  print               Render literal markup

Examples:
  Render a Markdown file
    $ rich README.md
  $ rich -w 60 data.json

Exit status:
  0 on success.

  2 on a usage error.";

const NARROW: &str = "\
Usage: rich [OPTIONS] [RESOURCE] [COMMAND]

Render rich text in the terminal

Layout:
  How the output is sized and framed.
  -w, --width <SIZE>
      Render the output this many columns wide
      [default: 80] [env: RICH_WIDTH] [config:
      layout.width]
      --panel <BOX>
      Wrap the output in a panel
      Possible values:
      - rounded: Rounded corners
      - heavy: Thick lines

Options:
      --color <WHEN>
      When to use colour [default: auto] [possible
      values: auto, always, never]
  -v, --verbose
      More output

Arguments:
  [RESOURCE]
      File, URL or - for stdin

Commands:
  config
      Show and check settings [aliases: cfg]
  print
      Render literal markup

Examples:
  Render a Markdown file
    $ rich README.md
  $ rich -w 60 data.json

Exit status:
  0 on success.

  2 on a usage error.";

#[test]
fn help_uses_two_columns_when_wide() {
    assert_eq!(render(&HelpView::new(&sample()), 100), WIDE);
}

#[test]
fn help_stacks_when_narrow() {
    const { assert!(50 < STACK_BELOW) };
    assert_eq!(render(&HelpView::new(&sample()), 50), NARROW);
}

#[test]
fn help_wraps_in_the_help_column_between_the_two() {
    let out = render(&HelpView::new(&sample()), 72);
    assert!(
        out.contains(
            "  -w, --width <SIZE>  Render the output this many columns wide [default:\n\
             \x20                     80] [env: RICH_WIDTH] [config: layout.width]\n"
        ),
        "{out}"
    );
}

#[test]
fn help_leaves_out_hidden_arguments_and_subcommands() {
    let out = render(&HelpView::new(&sample()), 100);
    assert!(!out.contains("secret") && !out.contains("Never shown"));
    assert!(!out.contains("internal"));
}

#[test]
fn long_help_uses_long_about_and_spaces_entries() {
    let out = render(&HelpView::new(&sample()).long(true), 100);
    assert!(out.contains("Render rich text in the terminal.\n\nRESOURCE is a file path"));
    assert!(
        out.contains("[config: layout.width]\n\n      --panel <BOX>"),
        "{out}"
    );
}

#[test]
fn subcommand_help_shows_the_full_path() {
    let view = HelpView::for_path(&sample(), &["cfg"]).unwrap();
    assert_eq!(
        render(&view, 60),
        "Usage: rich config [OPTIONS] [COMMAND]\n\n\
         Show and check settings\n\n\
         Options:\n  --json    Print JSON\n\n\
         Commands:\n  show      Show the effective settings\n  validate  Validate the config file"
    );
    assert!(HelpView::for_path(&sample(), &["nope"]).is_none());
}

#[test]
fn help_measures_its_natural_width() {
    let console = Console::builder().width(200).build();
    let view = HelpView::new(&sample());
    let Measurement { minimum, maximum } = view.measure(&console, &console.options());
    let wide = render(&view, 200);
    let widest = wide.lines().map(|l| l.chars().count()).max().unwrap();
    assert_eq!(maximum, widest);
    assert!(minimum <= maximum && minimum >= "  -w, --width <SIZE>".len());
    // Rendered at its measured width, it lays out exactly as it did at 200.
    assert_eq!(render(&view, maximum), wide);
    // Narrower than natural, the measurement is the available width.
    let narrow = Console::builder().width(70).build();
    assert_eq!(view.measure(&narrow, &narrow.options()).maximum, 70);
}

#[test]
fn help_styles_come_from_the_theme_with_defaults() {
    let out = colored(&HelpView::new(&sample()), 100);
    // help.heading (bold yellow) and help.option (bold cyan).
    assert!(out.contains("\x1b[1;33mLayout:\x1b[0m"), "{out:?}");
    assert!(out.contains("\x1b[1;36m--width\x1b[0m"), "{out:?}");
    // extended_theme registers every key, so markup can use them too.
    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .theme(rich_ext::theme::extended_theme())
        .build();
    assert_eq!(
        console.render_str_to_string("[help.hint]x[/]"),
        "\x1b[2mx\x1b[0m"
    );
}

// ---------------------------------------------------------------- errors

#[test]
fn unknown_argument_suggests_a_similar_one() {
    let error = CliError::unknown_in(&sample(), "--colr");
    assert_eq!(error.kind, CliErrorKind::UnknownArgument);
    assert_eq!(error.suggestions, ["--color"]);
    assert_eq!(
        render(&error.help_flag("--help").to_diagnostic(), 80),
        "error: unexpected argument '--colr'\n\
         note: usage: rich [OPTIONS] [RESOURCE] [COMMAND]\n\
         help: a similar argument exists: '--color'\n\
         help: for more information, try '--help'"
    );
}

#[test]
fn flag_typos_are_compared_with_flags_of_the_same_kind() {
    let spec = CommandSpec::new("tool")
        .arg(ArgSpec::flag("parallel").short('r').help("Run in parallel"))
        .arg(ArgSpec::flag("dry-run").short('n').help("Change nothing"))
        .arg(ArgSpec::flag("quiet").short('q').help("Less output"))
        .subcommand(CommandSpec::new("report").about("Summarise"));
    // A long-flag typo never draws an unrelated one-letter short flag.
    assert_eq!(
        CliError::unknown_in(&spec, "--paralel").suggestions,
        ["--parallel"]
    );
    assert_eq!(
        CliError::unknown_in(&spec, "--dry-rn").suggestions,
        ["--dry-run"]
    );
    assert!(CliError::unknown_in(&spec, "--r")
        .suggestions
        .iter()
        .all(|s| s.starts_with("--")));
    // A short-flag typo is compared with short flags only.
    assert!(CliError::unknown_in(&spec, "-x")
        .suggestions
        .iter()
        .all(|s| s.len() == 2 && !s.starts_with("--")));
    // Subcommands with subcommands.
    assert_eq!(CliError::unknown_in(&spec, "reprt").suggestions, ["report"]);
}

#[test]
fn unknown_subcommand_and_invalid_value() {
    let error = CliError::unknown_in(&sample(), "confg");
    assert_eq!(
        render(&error.to_diagnostic(), 80),
        "error: unrecognized subcommand 'confg'\n\
         note: usage: rich [OPTIONS] [RESOURCE] [COMMAND]\n\
         help: a similar subcommand exists: 'config'"
    );
    let error = CliError::invalid_value("--color <WHEN>", "alwys", ["auto", "always", "never"]);
    assert_eq!(
        render(&error.to_diagnostic(), 80),
        "error: invalid value 'alwys' for '--color <WHEN>'\n\
         note: possible values: auto, always, never\n\
         help: a similar value exists: 'always'"
    );
}

#[test]
fn missing_values_and_required_arguments() {
    assert_eq!(
        CliError::missing_value("--width <SIZE>").to_string(),
        "a value is required for '--width <SIZE>' but none was supplied"
    );
    assert_eq!(
        CliError::missing_required(["--out <OUT>", "<IN>"]).to_string(),
        "the following required arguments were not provided: --out <OUT>, <IN>"
    );
    let error = CliError::new(CliErrorKind::Conflict, "")
        .argument("--json")
        .value("--pretty");
    assert_eq!(
        error.to_string(),
        "the argument '--json' cannot be used with '--pretty'"
    );
    assert_eq!(error.exit_code(), 2);
    // Several close candidates are listed together.
    let error = CliError::unknown_argument("--colr", ["--color", "--colour"]);
    let out = render(&error.to_diagnostic(), 80);
    assert!(
        out.ends_with("help: similar arguments exist: '--color', '--colour'"),
        "{out}"
    );
}

// ---------------------------------------------------------------- completion

fn scratch(name: &str, contents: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-ext-cli-doc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

fn have(binary: &str) -> bool {
    let found = Command::new("which")
        .arg(binary)
        .output()
        .is_ok_and(|o| o.status.success());
    if !found {
        println!("note: `{binary}` not found; skipping its check");
    }
    found
}

/// A spec whose words need quoting in every shell.
fn awkward() -> CommandSpec {
    CommandSpec::new("my-tool")
        .arg(
            ArgSpec::option("mode")
                .choice("it's", "a 'quoted' $HOME `cmd` [x]: y")
                .choice("a b", "space \"double\" \\ back")
                .help("Say \"it's\" [ok]: $PATH `id` ‘smart’"),
        )
        .arg(
            ArgSpec::option("dir")
                .value(ValueHint::Dir)
                .help("A directory"),
        )
        .arg(ArgSpec::option("run").value(ValueHint::Command))
        .arg(
            ArgSpec::option("url")
                .value(ValueHint::Url)
                .heading("Net: remote"),
        )
        .arg(ArgSpec::positional("target").choices(["x:1", "y(2)"]))
        .subcommand(CommandSpec::new("sub-cmd").about("It's $HOME").alias("sc"))
}

#[test]
fn completion_scripts_pass_syntax_checks() {
    for spec in [sample(), awkward()] {
        let checks: [(Shell, &str, &[&str]); 4] = [
            (Shell::Bash, "bash", &["-n"]),
            (Shell::Zsh, "zsh", &["-n"]),
            (Shell::Fish, "fish", &["--no-execute"]),
            (
                Shell::PowerShell,
                "pwsh",
                &["-NoProfile", "-NonInteractive", "-File"],
            ),
        ];
        for (shell, binary, args) in checks {
            let script = generate(&spec, shell);
            if !have(binary) {
                continue;
            }
            let ext = if shell == Shell::PowerShell {
                "ps1"
            } else {
                "sh"
            };
            let path = scratch(&format!("{}-{shell}.{ext}", spec.name), &script);
            let out = Command::new(binary).args(args).arg(&path).output().unwrap();
            assert!(
                out.status.success() && out.stderr.is_empty(),
                "{shell} rejected its script:\n{}\n{script}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

/// Run the bash completion function for `words` (the last one is being
/// completed) and return COMPREPLY.
fn bash_complete(script: &std::path::Path, words: &[&str]) -> Vec<String> {
    let quoted: Vec<String> = words
        .iter()
        .map(|w| format!("'{}'", w.replace('\'', r"'\''")))
        .collect();
    let program = format!(
        "source '{}'; COMP_WORDS=({}); COMP_CWORD={}; _rich; printf '%s\\n' \"${{COMPREPLY[@]}}\"",
        script.display(),
        quoted.join(" "),
        words.len() - 1
    );
    let out = Command::new("bash")
        .args(["--norc", "--noprofile", "-c", &program])
        .current_dir(script.parent().unwrap())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn bash_completion_behaves() {
    if !have("bash") {
        return;
    }
    let script = scratch("rich.bash", &generate(&sample(), Shell::Bash));
    assert_eq!(bash_complete(&script, &["rich", "--col"]), ["--color"]);
    assert_eq!(bash_complete(&script, &["rich", "-"]).len(), 6);
    assert_eq!(
        bash_complete(&script, &["rich", "--color", "a"]),
        ["auto", "always"]
    );
    assert_eq!(
        bash_complete(&script, &["rich", "--panel", ""]),
        ["rounded", "heavy"]
    );
    // `--color=` as bash splits it by default, and kept whole.
    assert_eq!(
        bash_complete(&script, &["rich", "--color", "=", "n"]),
        ["never"]
    );
    assert_eq!(
        bash_complete(&script, &["rich", "--color=n"]),
        ["--color=never"]
    );
    // Subcommands, their aliases and nested subcommands.
    assert_eq!(
        bash_complete(&script, &["rich", "c"])[..2],
        ["config", "cfg"]
    );
    assert_eq!(bash_complete(&script, &["rich", "cfg", "--j"]), ["--json"]);
    assert_eq!(
        bash_complete(&script, &["rich", "config", "v"]),
        ["validate"]
    );
    // The file positional offers files (the script's own directory here).
    assert!(bash_complete(&script, &["rich", "rich.b"]).contains(&"rich.bash".to_string()));
}

#[test]
fn fish_completion_behaves() {
    if !have("fish") {
        return;
    }
    let script = scratch("rich.fish", &generate(&sample(), Shell::Fish));
    let complete = |line: &str| {
        let program = format!("source '{}'; complete -C '{line}'", script.display());
        let out = Command::new("fish")
            .args(["--no-config", "-c", &program])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap()
    };
    assert_eq!(
        complete("rich --col"),
        "--color\tOptions: When to use colour\n"
    );
    assert_eq!(
        complete("rich --panel "),
        "heavy\tThick lines\nrounded\tRounded corners\n"
    );
    let sub = complete("rich cfg ");
    assert!(sub.contains("show\tShow the effective settings"), "{sub}");
    assert!(complete("rich config --").contains("--json\tPrint JSON"));
}

#[test]
fn zsh_script_groups_options_by_heading() {
    let script = generate(&sample(), Shell::Zsh);
    assert!(script.starts_with("#compdef rich\n"));
    assert!(script.contains("_describe -t 'layout' 'Layout' group"));
    assert!(script.contains("_describe -t 'options' 'Options' group"));
    assert!(script.contains(
        "'(--panel)--panel=[Wrap the output in a panel]:BOX:((rounded\\:\"Rounded corners\" heavy\\:\"Thick lines\"))'"
    ));
    assert!(script.contains("'*-v[More output]'"));
    let awkward = generate(&awkward(), Shell::Zsh);
    assert!(
        awkward.contains(r#"[Say "it'\''s" \[ok\]\: \$PATH \`id\` ‘smart’]"#),
        "{awkward}"
    );
}

#[test]
fn powershell_script_escapes_quotes() {
    let script = generate(&awkward(), Shell::PowerShell);
    assert!(script.contains("Register-ArgumentCompleter -Native -CommandName 'my-tool'"));
    assert!(
        script.contains("'Options: Say \"it''s\" [ok]: $PATH `id` ‘‘smart’’'"),
        "{script}"
    );
    assert!(script.contains(
        "[CompletionResult]::new('it''s', 'it''s', [CompletionResultType]::ParameterValue"
    ));
    // Every line's single quotes balance once '' escapes are removed.
    for line in script.lines() {
        let bare = line.replace("''", "").replace("‘‘", "").replace("’’", "");
        let quotes = bare
            .chars()
            .filter(|c| matches!(c, '\'' | '‘' | '’'))
            .count();
        assert_eq!(quotes % 2, 0, "unbalanced: {line}");
    }
}

#[test]
fn catalog_lists_every_word() {
    let catalog = CompletionCatalog::from_spec(&sample());
    let config: Vec<(&str, CompletionKind, &str)> = catalog
        .items
        .iter()
        .filter(|i| i.path == ["config"])
        .map(|i| (i.word.as_str(), i.kind, i.group.as_str()))
        .collect();
    assert_eq!(
        config,
        [
            ("--json", CompletionKind::Option, "Options"),
            ("show", CompletionKind::Subcommand, "Commands"),
            ("validate", CompletionKind::Subcommand, "Commands"),
        ]
    );
    assert!(!catalog
        .items
        .iter()
        .any(|i| i.word == "--secret" || i.word == "internal"));
    let table = render(&catalog, 100);
    assert!(
        table.contains("│ rich config │ --json    │ option     │ Options        │ Print JSON"),
        "{table}"
    );
}

// ---------------------------------------------------------------- docs

#[test]
fn markdown_reference() {
    let spec = CommandSpec::new("tool")
        .about("Does things.")
        .arg(
            ArgSpec::option("level")
                .short('l')
                .choice("low", "Gently")
                .choice("high", "")
                .default_value("low")
                .env("TOOL_LEVEL")
                .help("How hard | how fast"),
        )
        .arg(ArgSpec::positional("input").required(true).multiple(true))
        .subcommand(CommandSpec::new("init").about("Create a config"))
        .example("tool -l high a b", "Work hard");
    assert_eq!(
        to_markdown(&spec),
        "# tool

Does things.

## Usage

```text
tool [OPTIONS] <INPUT>... [COMMAND]
```

## Options

| Option | Description |
| --- | --- |
| `-l`, `--level <LEVEL>` | How hard \\| how fast. Default: `low`. Environment: `TOOL_LEVEL`. Possible values: `low` (Gently), `high`. |

## Arguments

| Argument | Description |
| --- | --- |
| `<INPUT>...` |  |

## Commands

| Command | Description |
| --- | --- |
| `init` | Create a config |

## Examples

Work hard

```sh
tool -l high a b
```

## tool init

Create a config

### Usage

```text
tool init
```
"
    );
    // And through rich's Markdown renderer.
    let out = Console::builder()
        .width(80)
        .build()
        .render_to_string(&markdown_view(&spec));
    assert!(out.contains("Does things."), "{out}");
}

#[test]
fn man_page_passes_lint() {
    let pages = to_man_pages(&sample(), "1", Some("2026-09-23"));
    let names: Vec<&str> = pages.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "rich.1",
            "rich-config.1",
            "rich-config-show.1",
            "rich-config-validate.1",
            "rich-print.1"
        ]
    );
    let root = &pages[0].1;
    assert_eq!(root, &to_man(&sample(), "1", Some("2026-09-23")));
    assert!(root.starts_with(".TH \"RICH\" \"1\" \"2026-09-23\" \"rich 1.0.0\" \"User Commands\"\n.SH NAME\nrich \\- Render rich text in the terminal\n"));
    assert!(root.contains(".SH ENVIRONMENT\n.TP\n\\fBRICH_WIDTH\\fR\nSets \\fB\\-\\-width\\fR.\n"));
    assert!(root.contains(".SH \"SEE ALSO\"\n\\fBrich\\-config\\fR(1),\n\\fBrich\\-print\\fR(1)\n"));
    assert!(pages[1].1.contains(".SH \"SEE ALSO\"\n\\fBrich\\fR(1),\n"));
    // Deterministic, and dated only when asked.
    assert_eq!(to_man(&sample(), "1", None), to_man(&sample(), "1", None));
    assert!(to_man(&sample(), "1", None).starts_with(".TH \"RICH\" \"1\" \"\" "));
    // A line starting with a control character is escaped.
    let dotty = CommandSpec::new("x")
        .about("x")
        .section("Notes", ".not a macro\n'nor this");
    assert!(to_man(&dotty, "1", None).contains("\\&.not a macro\n\\&'nor this\n"));

    for (name, page) in &pages {
        let path = scratch(name, page);
        if have("mandoc") {
            let out = Command::new("mandoc")
                .args(["-T", "lint", "-W", "warning"])
                .arg(&path)
                .output()
                .unwrap();
            let report = String::from_utf8_lossy(&out.stdout).to_string()
                + &String::from_utf8_lossy(&out.stderr);
            assert!(
                report.trim().is_empty(),
                "mandoc on {name}:\n{report}\n{page}"
            );
        }
        if have("groff") {
            let out = Command::new("groff")
                .args(["-man", "-Tutf8", "-ww", "-z"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                out.stderr.is_empty(),
                "groff on {name}:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

// ---------------------------------------------------------------- config

fn reference() -> ConfigReference {
    ConfigReference::from_spec(&sample())
        .description("Every setting has a default; later sources override earlier ones.")
        .source("defaults", "", "Built into rich")
        .source("global", "~/.config/rich/config.toml", "Your settings")
        .source("env", "RICH_*", "Environment variables")
        .source("command line", "", "Flags")
        .entry(
            ConfigEntry::new("theme", "enum")
                .choices(["dark", "light"])
                .default_value("dark")
                .description("The colour theme | palette"),
        )
}

#[test]
fn config_reference_renders_sources_then_keys() {
    assert_eq!(
        render(&reference(), 90),
        "\
rich configuration

Every setting has a default; later sources override earlier ones.

Settings are read from these sources, lowest precedence first; a later source overrides an
earlier one:
  1. defaults — Built into rich
  2. global (~/.config/rich/config.toml) — Your settings
  3. env (RICH_*) — Environment variables
  4. command line — Flags

┏━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━┓
┃ Key          ┃ Type              ┃ Default ┃ Env        ┃ Flag    ┃ Description        ┃
┡━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━┩
│ layout.width │ string            │ 80      │ RICH_WIDTH │ --width │ Render the output  │
│              │                   │         │            │         │ this many columns  │
│              │                   │         │            │         │ wide               │
│ theme        │ enum: dark, light │ dark    │            │         │ The colour theme | │
│              │                   │         │            │         │ palette            │
└──────────────┴───────────────────┴─────────┴────────────┴─────────┴────────────────────┘"
    );
}

#[test]
fn config_reference_markdown() {
    assert_eq!(
        reference().to_markdown(),
        "\
# rich configuration

Every setting has a default; later sources override earlier ones.

## Sources

Settings are read from these sources, lowest precedence first; a later source overrides an earlier one:

1. **defaults** — Built into rich
2. **global** (`~/.config/rich/config.toml`) — Your settings
3. **env** (`RICH_*`) — Environment variables
4. **command line** — Flags

## Keys

| Key | Type | Default | Environment | Flag | Description |
| --- | --- | --- | --- | --- | --- |
| `layout.width` | string | `80` | `RICH_WIDTH` | `--width` | Render the output this many columns wide |
| `theme` | enum: `dark`, `light` | `dark` | | | The colour theme \\| palette |
"
    );
}

// ---------------------------------------------------------------- precedence

fn layers() -> Precedence {
    Precedence::new()
        .layer(
            Layer::new("defaults")
                .value("width", "80")
                .value("theme", "dark")
                .value("pager", "off"),
        )
        .layer(
            Layer::new("global")
                .origin("~/.config/rich/config.toml")
                .value("theme", "light"),
        )
        .layer(
            Layer::new("project")
                .origin("./rich.toml")
                .value("width", "100")
                .value("theme", "mono"),
        )
        .layer(Layer::new("env").origin("RICH_WIDTH").value("width", "90"))
        .layer(
            Layer::new("command line")
                .value("width", "120")
                .value("wrap", "on"),
        )
}

#[test]
fn precedence_resolves_later_layers_first() {
    let resolved = layers().resolve();
    let summary: Vec<(&str, &str, usize, Vec<usize>)> = resolved
        .iter()
        .map(|r| {
            (
                r.key.as_str(),
                r.value.as_str(),
                r.winner,
                r.shadowed.iter().map(|(i, _)| *i).collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("width", "120", 4, vec![0, 2, 3]),
            ("theme", "mono", 2, vec![0, 1]),
            ("pager", "off", 0, vec![]),
            ("wrap", "on", 4, vec![]),
        ]
    );
    assert!(Precedence::new().resolve().is_empty());
    assert!(layers().explain("missing").is_none());
}

#[test]
fn precedence_view_marks_winners_without_colour() {
    assert_eq!(
        render(&layers().view(), 100),
        "\
┏━━━━━━━┳━━━━━━━━━━┳━━━━━━━━━┳━━━━━━━━━┳━━━━━━┳━━━━━━━━━━━━━━┳━━━━━━━━━━━┓
┃ Key   ┃ defaults ┃ global  ┃ project ┃ env  ┃ command line ┃ Effective ┃
┡━━━━━━━╇━━━━━━━━━━╇━━━━━━━━━╇━━━━━━━━━╇━━━━━━╇━━━━━━━━━━━━━━╇━━━━━━━━━━━┩
│ width │ ✗ 80     │         │ ✗ 100   │ ✗ 90 │ ✔ 120        │ 120       │
│ theme │ ✗ dark   │ ✗ light │ ✔ mono  │      │              │ mono      │
│ pager │ ✔ off    │         │         │      │              │ off       │
│ wrap  │          │         │         │      │ ✔ on         │ on        │
└───────┴──────────┴─────────┴─────────┴──────┴──────────────┴───────────┘
global: ~/.config/rich/config.toml, project: ./rich.toml, env: RICH_WIDTH"
    );
}

#[test]
fn precedence_view_styles_the_winner() {
    let out = colored(&layers().view(), 100);
    // config.winner (bold green) and config.shadowed (dim strike).
    assert!(out.contains("\x1b[1;32m✔ 120 "), "{out:?}");
    assert!(out.contains("\x1b[2;9m✗ 80 "), "{out:?}");
}

#[test]
fn explain_walks_the_chain() {
    let explanation = layers().explain("width").unwrap();
    assert_eq!(explanation.resolved().winner, 4);
    assert_eq!(
        render(&explanation, 80),
        "\
width = 120
  ✔ command line           120  effective
  ✗ env (RICH_WIDTH)       90   overridden by command line
  ✗ project (./rich.toml)  100  overridden by env
  ✗ defaults               80   overridden by project"
    );
    let ascii = Console::builder().width(80).ascii_only(true).build();
    assert!(ascii
        .render_to_string(&explanation)
        .contains("  * command line"));
}

// ---------------------------------------------------------------- clap

#[cfg(feature = "clap")]
mod clap_adapter {
    use super::*;
    use ::clap::builder::PossibleValue;
    use ::clap::{Arg, ArgAction, Command as Clap};
    use rich_ext::cli_doc::clap::{try_parse_with, try_parse_with_spec};

    fn app() -> Clap {
        Clap::new("tool")
            .version("2.1.0")
            .about("A tool")
            .arg(
                Arg::new("input")
                    .help("Input file")
                    .value_hint(::clap::ValueHint::FilePath),
            )
            .arg(
                Arg::new("width")
                    .short('w')
                    .long("width")
                    .value_name("SIZE")
                    .env("TOOL_WIDTH")
                    .default_value("80")
                    .help("Output width")
                    .help_heading("Layout"),
            )
            .arg(
                Arg::new("color")
                    .long("color")
                    .visible_alias("colour")
                    .value_parser([
                        PossibleValue::new("auto").help("Detect"),
                        PossibleValue::new("never"),
                    ])
                    .help("When to colour"),
            )
            .arg(
                Arg::new("verbose")
                    .short('v')
                    .action(ArgAction::Count)
                    .help("Louder"),
            )
            .arg(Arg::new("token").long("token").hide(true))
            .subcommand(
                Clap::new("init")
                    .about("Create a config")
                    .arg(Arg::new("name").required(true).help("Project name")),
            )
            .after_help("See the manual for more.")
    }

    #[test]
    fn from_clap_maps_arguments_and_subcommands() {
        let spec = CommandSpec::from_clap(&app());
        assert_eq!(spec.version.as_deref(), Some("2.1.0"));
        let arg = |id: &str| spec.args.iter().find(|a| a.id == id).unwrap();
        let width = arg("width");
        assert_eq!(width.names(), "-w, --width <SIZE>");
        assert_eq!(arg("input").names(), "[input]");
        assert_eq!(width.heading.as_deref(), Some("Layout"));
        assert_eq!(width.env.as_deref(), Some("TOOL_WIDTH"));
        assert_eq!(width.default.as_deref(), Some("80"));
        let color = arg("color");
        assert_eq!(color.aliases, ["colour"]);
        assert_eq!(
            color.value,
            ValueHint::Choices(vec![
                Choice::new("auto").help("Detect"),
                Choice::new("never")
            ])
        );
        let input = arg("input");
        assert!(input.positional && !input.required);
        assert_eq!(input.value, ValueHint::File);
        let verbose = arg("verbose");
        assert!(verbose.multiple && !verbose.takes_value());
        assert!(arg("token").hidden);
        assert!(arg("help").short == Some('h') && arg("version").short == Some('V'));
        let init = spec.find_subcommand("init").unwrap();
        assert_eq!(init.about, "Create a config");
        let name = init.args.iter().find(|a| a.id == "name").unwrap();
        assert!(name.positional && name.required);
        assert_eq!(spec.sections[0].body, "See the manual for more.");
        assert!(spec.find_subcommand("help").is_some());
    }

    fn console() -> Console {
        Console::builder().width(80).build()
    }

    #[test]
    fn help_and_version_render_through_rich() {
        let exit = try_parse_with(&console(), app(), ["tool", "-h"]).unwrap_err();
        assert_eq!((exit.code, exit.use_stderr), (0, false));
        assert!(exit
            .output
            .starts_with("Usage: tool [OPTIONS] [input] [COMMAND]\n\nA tool\n\nLayout:\n"));
        assert!(
            exit.output.contains(
                "  -w, --width <SIZE>             Output width [default: 80] [env: TOOL_WIDTH]\n"
            ),
            "{}",
            exit.output
        );
        assert!(exit
            .output
            .contains("      --color, --colour <color>  When to colour\n"));
        assert!(exit
            .output
            .contains("  -v                             Louder\n"));
        assert!(exit.output.ends_with("See the manual for more.\n"));
        let exit = try_parse_with(&console(), app(), ["tool", "init", "--help"]).unwrap_err();
        assert!(
            exit.output
                .starts_with("Usage: tool init [OPTIONS] <name>\n"),
            "{}",
            exit.output
        );
        let exit = try_parse_with(&console(), app(), ["tool", "help", "init"]).unwrap_err();
        assert!(
            exit.output
                .starts_with("Usage: tool init [OPTIONS] <name>\n"),
            "{}",
            exit.output
        );
        let exit = try_parse_with(&console(), app(), ["tool", "--version"]).unwrap_err();
        assert_eq!((exit.code, exit.output.as_str()), (0, "tool 2.1.0\n"));
        // A spec can add what clap cannot express.
        let spec = CommandSpec::from_clap(&app()).example("tool -w 60 in.txt", "Narrow");
        let exit = try_parse_with_spec(&console(), app(), &spec, ["tool", "--help"]).unwrap_err();
        assert!(exit
            .output
            .contains("Examples:\n  Narrow\n    $ tool -w 60 in.txt\n"));
        assert!(try_parse_with(&console(), app(), ["tool", "-vv", "x"]).is_ok());
    }

    #[test]
    fn errors_map_kinds_and_suggestions() {
        let error = |args: &[&str]| {
            let err = app().try_get_matches_from(args).unwrap_err();
            CliError::from_clap(&err, Some(&app()))
        };
        let unknown = error(&["tool", "--colr"]);
        assert_eq!(unknown.kind, CliErrorKind::UnknownArgument);
        assert_eq!(unknown.argument.as_deref(), Some("--colr"));
        assert_eq!(unknown.suggestions, ["--color"]);
        let invalid = error(&["tool", "--color", "nevr"]);
        assert_eq!(invalid.kind, CliErrorKind::InvalidValue);
        assert_eq!(invalid.value.as_deref(), Some("nevr"));
        assert_eq!(invalid.possible_values, ["auto", "never"]);
        assert_eq!(invalid.suggestions, ["never"]);
        let missing = error(&["tool", "init"]);
        assert_eq!(missing.kind, CliErrorKind::MissingRequired);
        assert_eq!(missing.argument.as_deref(), Some("<name>"));
        let value = error(&["tool", "--width"]);
        assert_eq!(value.kind, CliErrorKind::MissingValue);

        let exit = try_parse_with(&console(), app(), ["tool", "--colr"]).unwrap_err();
        assert_eq!((exit.code, exit.use_stderr), (2, true));
        assert_eq!(
            exit.output,
            "error: unexpected argument '--colr'\n\
             note: usage: tool [OPTIONS] [input] [COMMAND]\n\
             help: a similar argument exists: '--color'\n\
             help: for more information, try '--help'\n"
        );
        let exit = try_parse_with(&console(), app(), ["tool", "init"]).unwrap_err();
        assert_eq!(
            exit.output,
            "error: the following required arguments were not provided: <name>\n\
             note: usage: tool init [OPTIONS] <name>\n\
             help: for more information, try '--help'\n"
        );
    }
}
