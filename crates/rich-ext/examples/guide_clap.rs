//! Guide: CLI authoring (clap) — run: cargo run -p rs-rich-ext --example guide_clap --features clap [-- --svg docs/media/guide]
//!
//! A small clap program whose help, version and errors render through rich.
//! Without `--svg` it is the program: try `-- --help`, `-- --version`,
//! `-- build.tar -r eu-west-1` or a typo such as `-- build.tar --parralel 2`.
use std::path::PathBuf;

use rich::ansi::AnsiDecoder;
use rich::{ColorSystem, Console};
use rich_ext::cli_doc::CommandSpec;
use rich_ext::ConsoleExt;

// --8<-- [start:command]
use clap::{value_parser, Arg, ArgAction, Command};

fn command() -> Command {
    Command::new("deploy")
        .version("2.1.0")
        .about("Ship a build to one or more regions")
        .arg(
            Arg::new("artifact")
                .required(true)
                .help("The build archive to upload"),
        )
        .arg(
            Arg::new("region")
                .short('r')
                .long("region")
                .value_name("NAME")
                .value_parser(["eu-west-1", "us-east-2"])
                .action(ArgAction::Append)
                .env("DEPLOY_REGION")
                .help("Region to deploy to; repeat for several")
                .help_heading("Targets"),
        )
        .arg(
            Arg::new("parallel")
                .short('j')
                .long("parallel")
                .value_name("N")
                .value_parser(value_parser!(u8))
                .default_value("4")
                .help("Upload this many files at once")
                .help_heading("Targets"),
        )
        .arg(
            Arg::new("dry-run")
                .short('n')
                .long("dry-run")
                .action(ArgAction::SetTrue)
                .help("Show the plan without uploading"),
        )
}
// --8<-- [end:command]

// --8<-- [start:spec]
fn spec() -> CommandSpec {
    // clap has no notion of examples or config keys: add them to the spec
    // derived from the command.
    CommandSpec::from_clap(&command())
        .example("deploy build.tar -r eu-west-1", "Deploy to one region")
        .example(
            "deploy build.tar -n -r eu-west-1 -r us-east-2",
            "Preview a two-region deploy",
        )
}
// --8<-- [end:spec]

// --8<-- [start:main]
fn run() {
    use rich_ext::cli_doc::clap::parse_or_exit_with_spec;

    // Help, --version and errors render through rich, and the process exits
    // with 0 (help, version) or 2 (errors), as clap's own `get_matches` does.
    let matches = parse_or_exit_with_spec(command(), &spec());
    let regions: Vec<&String> = matches.get_many("region").into_iter().flatten().collect();
    let parallel: u8 = *matches.get_one("parallel").expect("has a default");
    let dry_run = matches.get_flag("dry-run");
    println!("deploying to {regions:?}, {parallel} at a time (dry run: {dry_run})");
}
// --8<-- [end:main]

// --8<-- [start:test]
fn render_error(console: &Console) {
    use rich_ext::cli_doc::clap::try_parse_with;

    // Nothing is printed and nothing exits, so this works in a test.
    let args = ["deploy", "build.tar", "--parralel", "2"];
    let exit = try_parse_with(console, command(), args).unwrap_err();
    assert_eq!(exit.code, 2);
    assert!(exit.use_stderr);
    assert!(exit
        .output
        .contains("a similar argument exists: '--parallel'"));
    // `exit.output` is the rendered text; replay it on this console.
    for line in AnsiDecoder::new().decode(&exit.output) {
        console.print(&line);
    }
}
// --8<-- [end:test]

fn render_help(console: &Console) {
    use rich_ext::cli_doc::clap::try_parse_with_spec;

    let exit = try_parse_with_spec(console, command(), &spec(), ["deploy", "--help"]).unwrap_err();
    assert_eq!(exit.code, 0);
    for line in AnsiDecoder::new().decode(&exit.output) {
        console.print(&line);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(i) = args.iter().position(|a| a == "--svg") else {
        return run();
    };
    let dir = PathBuf::from(args.get(i + 1).expect("--svg takes a directory"));
    shot(&dir, "help", 80, "deploy --help", render_help);
    shot(
        &dir,
        "error",
        72,
        "deploy build.tar --parralel 2",
        render_error,
    );
}

/// Write one shot as `DIR/guide_clap-<shot>.svg`.
fn shot(dir: &std::path::Path, name: &str, width: usize, title: &str, f: impl FnOnce(&Console)) {
    let mut console = Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .build();
    console.install_extensions();
    let id = format!("guide_clap-{name}");
    let svg = console.export_svg(title, &id, f);
    std::fs::create_dir_all(dir).expect("create the SVG directory");
    std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
}
