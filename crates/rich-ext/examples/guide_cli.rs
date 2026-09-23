//! Guide: CLI authoring — run: cargo run -p rs-rich-ext --example guide_cli [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/cli-authoring.md` comes from this file.
use std::path::PathBuf;

use rich::{ColorSystem, Console};
use rich_ext::cli_doc::{
    generate, markdown_view, suggest, to_man, to_man_pages, to_markdown, ArgSpec, CliError,
    CommandSpec, CompletionCatalog, ConfigEntry, ConfigReference, HelpView, Layer, Precedence,
    Shell, ValueHint,
};
use rich_ext::ConsoleExt;

// --8<-- [start:spec]
fn deploy_spec() -> CommandSpec {
    CommandSpec::new("deploy")
        .version("2.1.0")
        .about("Ship a build to one or more regions")
        .arg(
            ArgSpec::positional("artifact")
                .value(ValueHint::File)
                .required(true)
                .help("The build archive to upload"),
        )
        .arg(
            ArgSpec::option("region")
                .short('r')
                .value_name("NAME")
                .choice("eu-west-1", "Ireland")
                .choice("us-east-2", "Ohio")
                .multiple(true)
                .env("DEPLOY_REGION")
                .config_key("deploy.region")
                .help("Region to deploy to; repeat for several")
                .heading("Targets"),
        )
        .arg(
            ArgSpec::option("parallel")
                .short('j')
                .value_name("N")
                .default_value("4")
                .config_key("deploy.parallel")
                .help("Upload this many files at once")
                .heading("Targets"),
        )
        .arg(
            ArgSpec::flag("dry-run")
                .short('n')
                .help("Show the plan without uploading"),
        )
        .arg(
            ArgSpec::flag("verbose")
                .short('v')
                .help("Log every request"),
        )
        .subcommand(CommandSpec::new("rollback").about("Restore the previous release"))
        .subcommand(CommandSpec::new("status").about("Show what is running where"))
        .example("deploy build.tar -r eu-west-1", "Deploy to one region")
        .example(
            "deploy build.tar -n -r eu-west-1 -r us-east-2",
            "Preview a two-region deploy",
        )
        .section(
            "Exit status",
            "0 on success, 1 when an upload fails, 2 on a usage error.",
        )
}
// --8<-- [end:spec]

// --8<-- [start:help]
fn show_help(console: &Console, spec: &CommandSpec) {
    // Two columns from STACK_BELOW (60) cells up; stacked below that.
    console.print(&HelpView::new(spec));
}
// --8<-- [end:help]

// --8<-- [start:help-sub]
fn show_subcommand_help(console: &Console, spec: &CommandSpec) {
    let view = HelpView::for_path(spec, &["rollback"]).expect("known subcommand");
    console.print(&view.long(true));
}
// --8<-- [end:help-sub]

// --8<-- [start:errors]
fn show_errors(console: &Console, spec: &CommandSpec) {
    // An unknown flag: suggestions come from every switch the spec knows,
    // and the usage line from the spec.
    let error = CliError::unknown_in(spec, "--paralel").help_flag("--help");
    console.print(&error.to_diagnostic());
    assert_eq!(error.exit_code(), 2); // the usage-error status clap uses too

    // A bad value, with the allowed ones listed and the closest suggested.
    let regions = ["eu-west-1", "us-east-2"];
    let error = CliError::invalid_value("--region <NAME>", "eu-west", regions);
    assert_eq!(error.suggestions, suggest("eu-west", regions));
    console.print(&error.to_diagnostic());
}
// --8<-- [end:errors]

// --8<-- [start:completions]
fn write_completions(spec: &CommandSpec) {
    for shell in Shell::ALL {
        let script = generate(spec, shell);
        // Typically: `deploy completions bash > /etc/bash_completion.d/deploy`.
        println!("{shell}: {} lines", script.lines().count());
    }
    let fish: Shell = "fish".parse().expect("known shell");
    print!("{}", generate(spec, fish));
}
// --8<-- [end:completions]

// --8<-- [start:catalog]
fn show_catalog(console: &Console, spec: &CommandSpec) {
    let catalog = CompletionCatalog::from_spec(spec);
    // Plain data for other completion systems...
    assert!(catalog.items.iter().any(|item| item.word == "--dry-run"));
    // ...and a renderable table.
    console.print(&catalog);
}
// --8<-- [end:catalog]

// --8<-- [start:docs]
fn write_docs(console: &Console, spec: &CommandSpec) {
    // Markdown for a docs site, or rendered in the terminal.
    let markdown: String = to_markdown(spec);
    assert!(markdown.starts_with("# deploy"));
    console.print(&markdown_view(spec));

    // Man pages: one page, or one per subcommand. Pass a date for
    // reproducible output; `None` leaves it out.
    let page = to_man(spec, "1", Some("2026-09-23"));
    assert!(page.starts_with(".TH \"DEPLOY\" \"1\" \"2026-09-23\""));
    for (file_name, _page) in to_man_pages(spec, "1", Some("2026-09-23")) {
        println!("would write {file_name}"); // deploy.1, deploy-rollback.1, …
    }
}
// --8<-- [end:docs]

// --8<-- [start:config-reference]
fn show_config_reference(console: &Console, spec: &CommandSpec) {
    // Every argument with a `config_key` becomes an entry...
    let reference = ConfigReference::from_spec(spec)
        .description("Settings can live in a file, the environment or on the command line.")
        // ...listed with its sources, lowest precedence first.
        .source("defaults", "", "Built in")
        .source("user", "~/.config/deploy.toml", "Your settings")
        .source("environment", "DEPLOY_*", "")
        .source("command line", "", "")
        .entry(
            ConfigEntry::new("deploy.timeout", "duration")
                .default_value("30s")
                .description("Give up on an upload after this long"),
        );
    console.print(&reference);
    let _markdown = reference.to_markdown();
}
// --8<-- [end:config-reference]

// --8<-- [start:precedence]
fn show_precedence(console: &Console) {
    let precedence = Precedence::new()
        .layer(
            Layer::new("defaults")
                .value("deploy.parallel", "4")
                .value("deploy.timeout", "30s"),
        )
        .layer(
            Layer::new("user")
                .origin("~/.config/deploy.toml")
                .value("deploy.parallel", "8")
                .value("deploy.region", "eu-west-1"),
        )
        .layer(
            Layer::new("env")
                .origin("DEPLOY_REGION")
                .value("deploy.region", "us-east-2"),
        )
        .layer(Layer::new("flags").value("deploy.parallel", "2"));
    console.print(&precedence.view());
    console.print(
        &precedence
            .explain("deploy.parallel")
            .expect("some layer sets it"),
    );
}
// --8<-- [end:precedence]

fn main() {
    let shots = Shots::from_args();
    let spec = deploy_spec();
    shots.shot("help-wide", 80, "deploy --help", |c| show_help(c, &spec));
    shots.shot("help-narrow", 48, "deploy --help (48 columns)", |c| {
        show_help(c, &spec)
    });
    shots.shot("help-sub", 80, "deploy rollback --help", |c| {
        show_subcommand_help(c, &spec)
    });
    shots.shot("errors", 72, "CliError", |c| show_errors(c, &spec));
    if !shots.svg() {
        write_completions(&spec);
    }
    shots.shot("catalog", 80, "CompletionCatalog", |c| {
        show_catalog(c, &spec)
    });
    shots.shot("docs", 80, "markdown_view", |c| write_docs(c, &spec));
    shots.shot("config-reference", 88, "ConfigReference", |c| {
        show_config_reference(c, &spec)
    });
    shots.shot("precedence", 80, "Precedence", show_precedence);
}

/// `--svg DIR` writes each shot as `DIR/guide_cli-<shot>.svg`; without it,
/// shots print to the terminal.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { dir }
    }

    fn svg(&self) -> bool {
        self.dir.is_some()
    }

    fn shot(&self, name: &str, width: usize, title: &str, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            let mut console = Console::new();
            console.install_extensions();
            return f(&console);
        };
        let mut console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        console.install_extensions();
        let id = format!("guide_cli-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}
