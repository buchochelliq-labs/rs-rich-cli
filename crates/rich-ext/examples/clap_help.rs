//! A clap CLI whose help, version and errors render through rich.
//!
//! ```text
//! cargo run -p rs-rich-ext --example clap_help --features clap -- --help
//! cargo run -p rs-rich-ext --example clap_help --features clap -- --colr
//! cargo run -p rs-rich-ext --example clap_help --features clap -- completions fish
//! ```
use clap::builder::PossibleValue;
use clap::{value_parser, Arg, ArgAction, Command, ValueHint};
use rich_ext::cli_doc::clap::parse_or_exit_with_spec;
use rich_ext::cli_doc::{generate, CommandSpec, Shell};

fn command() -> Command {
    Command::new("render")
        .version("1.2.0")
        .about("Render files beautifully in the terminal")
        .long_about(
            "Render Markdown, JSON, CSV and source code in the terminal.\n\n\
             RESOURCE is a file path, an http(s) URL, or `-` for stdin.",
        )
        .arg(
            Arg::new("resource")
                .help("File, URL or - for stdin")
                .value_hint(ValueHint::FilePath),
        )
        .arg(
            Arg::new("width")
                .short('w')
                .long("width")
                .value_name("SIZE")
                .value_parser(value_parser!(u16))
                .env("RENDER_WIDTH")
                .help("Render this many columns wide")
                .help_heading("Layout"),
        )
        .arg(
            Arg::new("panel")
                .long("panel")
                .value_name("BOX")
                .value_parser([
                    PossibleValue::new("rounded").help("Rounded corners"),
                    PossibleValue::new("heavy").help("Thick lines"),
                    PossibleValue::new("none").help("No panel"),
                ])
                .default_value("none")
                .help("Wrap the output in a panel")
                .help_heading("Layout"),
        )
        .arg(
            Arg::new("color")
                .long("color")
                .value_name("WHEN")
                .value_parser(["auto", "always", "never"])
                .default_value("auto")
                .env("RENDER_COLOR")
                .help("When to use colour"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::Count)
                .help("More output; repeat for more"),
        )
        .subcommand(
            Command::new("completions")
                .about("Print a shell completion script")
                .arg(
                    Arg::new("shell")
                        .required(true)
                        .value_parser(["bash", "zsh", "fish", "powershell"])
                        .help("The shell to complete for"),
                ),
        )
        .after_help("Configuration lives in ~/.config/render/config.toml.")
}

fn main() {
    // clap has no notion of examples; add them to the spec it produces.
    let spec = CommandSpec::from_clap(&command())
        .example("render README.md", "Render a Markdown file")
        .example(
            "render -w 60 --panel rounded data.json",
            "Pretty-print JSON in a panel",
        )
        .example("curl -s https://example.com/api | render -", "Render stdin");
    let matches = parse_or_exit_with_spec(command(), &spec);
    if let Some(sub) = matches.subcommand_matches("completions") {
        let shell: Shell = sub
            .get_one::<String>("shell")
            .and_then(|s| s.parse().ok())
            .unwrap_or(Shell::Bash);
        print!("{}", generate(&CommandSpec::from_clap(&command()), shell));
        return;
    }
    let width = matches.get_one::<u16>("width");
    let resource = matches.get_one::<String>("resource");
    println!("would render {resource:?} at width {width:?}");
}
