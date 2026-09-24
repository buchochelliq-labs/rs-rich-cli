//! The `rich` command line described once, as a [`rich_ext::cli_doc`]
//! [`CommandSpec`], and rendered from there: `--help`, `rich <command>
//! --help`, shell completions, Markdown and man pages, and the configuration
//! reference.
//!
//! This is a binary-boundary convenience (see `docs/PORTING.md`): upstream
//! `rich-cli` prints click's help. The spec composes public `rich-ext` APIs
//! only, and the drift test below keeps it and the hand-written parser in
//! step.

use rich::{Console, Renderable};
use rich_ext::cli_doc::{
    ArgSpec, Choice, CommandSpec, ConfigEntry, ConfigReference, HelpView, Layer, ValueHint,
};

const ARGUMENTS: &str = "Arguments";
const RENDER_MODE: &str = "Render mode";
const INPUT: &str = "Input";
const LAYOUT: &str = "Layout";
const MODE_OPTIONS: &str = "Mode options";
const IMAGE: &str = "Image";
const INSPECT: &str = "Inspect";
const DIFF: &str = "Diff & ANSI";
const VIEWERS: &str = "Viewers";
const EXPORT: &str = "Export";
const PAGING: &str = "Paging";
const WATCH: &str = "Watch";
const BATCH: &str = "Batch";
const CONFIG: &str = "Config & theme";
const OUTPUT: &str = "Output & reports";
const DEMO: &str = "Demo";
const GENERAL: &str = "General";

/// The render modes a `mode` config value (and the command word) may name.
pub(crate) const MODES: &[&str] = &[
    "print", "markdown", "json", "syntax", "csv", "ipynb", "jsonl", "log", "rule", "image", "gif",
    "diff", "inspect", "ansi",
];

fn flag(long: &str, heading: &str, help: &str) -> ArgSpec {
    ArgSpec::flag(long).heading(heading).help(help)
}

fn option(long: &str, metavar: &str, heading: &str, help: &str) -> ArgSpec {
    ArgSpec::option(long)
        .value_name(metavar)
        .heading(heading)
        .help(help)
}

/// A boolean config flag with its `--no-…` inverse as an alias.
fn switch(long: &str, key: &str, heading: &str, help: &str) -> ArgSpec {
    flag(long, heading, help)
        .alias(format!("no-{long}"))
        .config_key(key)
}

fn mode_command(name: &str, aliases: &[&str], about: &str) -> CommandSpec {
    let mut command =
        CommandSpec::new(name)
            .about(about)
            .usage(format!("{name} [OPTIONS] [RESOURCE]"))
            .arg(ArgSpec::positional("resource").value(ValueHint::Path).help(
                "A file path, an http(s) URL, or `-` for stdin; every `rich` option applies",
            ));
    for alias in aliases {
        command = command.alias(*alias);
    }
    command
}

fn render_modes() -> Vec<ArgSpec> {
    let mode = |long: &str, short: Option<char>, help: &str| {
        let arg = flag(long, RENDER_MODE, help);
        match short {
            Some(short) => arg.short(short),
            None => arg,
        }
    };
    vec![
        mode(
            "print",
            Some('p'),
            "Treat RESOURCE as literal markup TEXT, not a file path",
        ),
        mode("markdown", Some('m'), "Render RESOURCE as Markdown"),
        mode("json", Some('j'), "Pretty-print RESOURCE as JSON"),
        mode(
            "syntax",
            Some('x'),
            "Syntax-highlight RESOURCE (language from its extension)",
        ),
        mode("csv", None, "Render RESOURCE as a CSV/TSV table"),
        mode("ipynb", None, "Render RESOURCE as a Jupyter notebook"),
        mode("jsonl", None, "Stream JSON Lines / NDJSON records").alias("ndjson"),
        mode("log", None, "Stream common structured-log JSONL records"),
        mode(
            "gif",
            None,
            "Animate GIFs side by side; pipes receive the first frame",
        ),
        mode(
            "rule",
            None,
            "Draw a horizontal rule (RESOURCE is its title)",
        ),
        mode(
            "diff",
            None,
            "Compare two images perceptually, two text files as a diff (syntax-aware; ANSI \
             captures by visible text and style), or render one patch such as `git diff` output",
        ),
        mode(
            "image",
            None,
            "Render RESOURCE as a still image (ASCII/Braille/blocks/Sixel)",
        ),
        mode(
            "inspect",
            None,
            "Explore structured data (JSON, YAML, TOML, XML, INI, dotenv) as a tree",
        ),
        mode(
            "ansi-explain",
            None,
            "Decode every escape sequence in RESOURCE and show the visible text",
        ),
    ]
}

fn diff_options() -> Vec<ArgSpec> {
    vec![
        flag(
            "side-by-side",
            DIFF,
            "With a text --diff, show old and new in two columns",
        ),
        option(
            "context",
            "N",
            DIFF,
            "With a text --diff, unchanged lines around each change",
        )
        .default_value("3"),
        option(
            "language",
            "NAME",
            DIFF,
            "With a text --diff, highlight as this language instead of guessing from the \
             file name",
        ),
        flag(
            "ansi-inline",
            DIFF,
            "With --ansi-explain, mark escapes inline in the text instead of a table",
        ),
        flag(
            "escapes-only",
            DIFF,
            "With --ansi-explain, list only escape sequences, not text runs",
        ),
    ]
}

fn viewer_options() -> Vec<ArgSpec> {
    vec![
        option(
            "search",
            "TEXT",
            VIEWERS,
            "With `rich view`, highlight every case-insensitive match and mark its line; with \
             `rich hex`, highlight a byte string: hex such as `de ad` or `0xDEAD`, or quoted \
             text such as '\"PNG\"'",
        ),
        flag(
            "no-line-numbers",
            VIEWERS,
            "With `rich view`, leave out the line-number gutter",
        ),
        option(
            "offset",
            "N",
            VIEWERS,
            "With `rich hex`, start N bytes in (decimal or 0x hex)",
        ),
        option(
            "length",
            "N",
            VIEWERS,
            "With `rich hex`, show at most N bytes",
        ),
        option(
            "bytes-per-line",
            "N",
            VIEWERS,
            "With `rich hex`, bytes on each line (default: fit the width, up to 16)",
        ),
        option("group", "N", VIEWERS, "With `rich hex`, bytes per group").default_value("8"),
        option(
            "limit",
            "N",
            VIEWERS,
            "With `rich unicode`, show at most N graphemes",
        ),
        flag(
            "show-secrets",
            VIEWERS,
            "With `rich env`, show values whose names look secret instead of masking them",
        ),
        option(
            "cast",
            "FILE",
            VIEWERS,
            "With `rich capture`, also write an asciicast v2 recording (`asciinema play FILE`)",
        ),
    ]
}

fn input_options() -> Vec<ArgSpec> {
    let mut args = vec![option(
        "format",
        "F",
        INPUT,
        "Input format. With --inspect, the parser (default auto-detect); otherwise `auto` \
         detects piped or extensionless input and routes it (JSON to --json, other formats \
         to highlighting, anything else to plain text), and a named format overrides the \
         extension",
    )
    .choices(["auto", "json", "yaml", "toml", "xml", "ini", "env"])
    .default_value("auto")
    .config_key("format")];
    args.extend(
        rich_ext::cli::arg_specs()
            .into_iter()
            .filter(|arg| arg.id == "encoding")
            .map(|arg| arg.heading(INPUT)),
    );
    args
}

fn layout_options() -> Vec<ArgSpec> {
    vec![
        option(
            "width",
            "N",
            LAYOUT,
            "Render the output N columns wide (the console keeps its own width, so \
             --left/--center/--right still use it)",
        )
        .short('w')
        .config_key("width"),
        flag("left", LAYOUT, "Left-justify output"),
        flag("center", LAYOUT, "Center output"),
        flag("right", LAYOUT, "Right-justify output"),
        option(
            "panel",
            "BOX",
            LAYOUT,
            "Wrap output in a panel, shrunk to fit its content (none = no panel)",
        )
        .choices([
            "ascii", "ascii2", "square", "rounded", "heavy", "double", "none",
        ])
        .config_key("panel"),
        option(
            "padding",
            "P",
            LAYOUT,
            "Wrap output in padding (1, 2, or 4 comma-separated ints)",
        )
        .config_key("padding"),
        flag(
            "expand",
            LAYOUT,
            "Make --panel/--padding fill the width instead of fitting (implied by --width)",
        )
        .short('e'),
        option(
            "title",
            "T",
            LAYOUT,
            "Panel title; also the CSV table's title",
        ),
        option(
            "caption",
            "T",
            LAYOUT,
            "Panel subtitle; also the CSV table's caption",
        ),
        option(
            "style",
            "S",
            LAYOUT,
            "Style laid under the whole output, e.g. \"bold red\"",
        )
        .short('s'),
        option(
            "panel-style",
            "S",
            LAYOUT,
            "Panel border style, e.g. \"dim\" (with --panel)",
        )
        .short('S'),
        flag(
            "hyperlinks",
            LAYOUT,
            "Render a Markdown link as a clickable OSC 8 hyperlink. Off by default, which \
             shows the URL as `text (url)`",
        )
        .short('y'),
    ]
}

fn mode_options() -> Vec<ArgSpec> {
    let mut args = vec![
        option(
            "log-presentation",
            "MODE",
            MODE_OPTIONS,
            "With --log, select log presentation",
        )
        .choices(["plain", "rich"])
        .default_value("plain")
        .config_key("log_presentation"),
        option(
            "loop",
            "N",
            MODE_OPTIONS,
            "With --gif, repeat N times (default 1; 0 = forever)",
        ),
    ];
    args.extend(
        rich_ext::cli::arg_specs()
            .into_iter()
            .filter(|arg| arg.id == "gif-mode")
            .map(|arg| arg.heading(MODE_OPTIONS)),
    );
    args
}

fn image_options() -> Vec<ArgSpec> {
    let tone = "Tone adjustment (1.0 = unchanged)";
    vec![
        option(
            "image-mode",
            "M",
            IMAGE,
            "With --diff/--image, how to draw the picture: sixel draws real pixels; blocks, \
             quadrants, braille and ascii draw characters (--image rejects none: there would \
             be nothing to draw)",
        )
        .choices([
            "auto",
            "sixel",
            "blocks",
            "quadrants",
            "braille",
            "ascii",
            "none",
        ])
        .default_value("auto"),
        option(
            "height",
            "N",
            IMAGE,
            "With --image, render this many rows instead of the backend's default",
        )
        .config_key("height"),
        option(
            "image-fit",
            "M",
            IMAGE,
            "With --image and --height: contain letterboxes, cover crops at --image-anchor, \
             stretch fills ignoring aspect",
        )
        .choices(["contain", "cover", "stretch"])
        .config_key("image_fit"),
        option("image-anchor", "A", IMAGE, "Cover crop anchor")
            .choices([
                "center",
                "top",
                "bottom",
                "left",
                "right",
                "top-left",
                "top-right",
                "bottom-left",
                "bottom-right",
            ])
            .default_value("center")
            .config_key("image_anchor"),
        option(
            "image-max-width",
            "N",
            IMAGE,
            "With --image, never exceed N columns (aspect kept)",
        )
        .config_key("image_max_width"),
        option(
            "image-max-height",
            "N",
            IMAGE,
            "With --image, never exceed N rows (aspect kept)",
        )
        .config_key("image_max_height"),
        option(
            "image-background",
            "#RRGGBB",
            IMAGE,
            "With --image: flatten transparency onto this RGB colour (also colours contain \
             padding; quote the # in your shell)",
        )
        .config_key("image_background"),
        option(
            "image-color",
            "M",
            IMAGE,
            "Colour depth for --image (not Braille, which is monochrome) and --gif frames",
        )
        .choices(["truecolor", "ansi256", "ansi16", "grayscale"])
        .default_value("truecolor")
        .config_key("image_color"),
        option(
            "image-dither",
            "M",
            IMAGE,
            "Dithering (needs a non-truecolor --image-color)",
        )
        .choices(["none", "floyd-steinberg", "bayer4x4", "atkinson"])
        .default_value("none")
        .config_key("image_dither"),
        option(
            "image-color-distance",
            "M",
            IMAGE,
            "How the nearest palette colour is measured: encoded RGB, or perceptual OKLab \
             (needs a non-truecolor --image-color)",
        )
        .choices(["rgb", "oklab"])
        .default_value("rgb")
        .config_key("image_color_distance"),
        option(
            "image-brightness",
            "F",
            IMAGE,
            "Tone adjustment (1.0 = unchanged). Brightness, contrast and gamma apply in that \
             order, after rotation/flips and before grayscale and colour",
        )
        .default_value("1.0")
        .config_key("image_brightness"),
        option("image-contrast", "F", IMAGE, tone)
            .default_value("1.0")
            .config_key("image_contrast"),
        option("image-gamma", "F", IMAGE, tone)
            .default_value("1.0")
            .config_key("image_gamma"),
        option("image-rotate", "N", IMAGE, "Rotate still images clockwise")
            .choices(["0", "90", "180", "270"])
            .default_value("0")
            .config_key("image_rotate"),
        switch(
            "image-flip-horizontal",
            "image_flip_horizontal",
            IMAGE,
            "Flip still images horizontally, after rotation",
        ),
        switch(
            "image-flip-vertical",
            "image_flip_vertical",
            IMAGE,
            "Flip still images vertically, after rotation",
        ),
        switch(
            "image-grayscale",
            "image_grayscale",
            IMAGE,
            "Composite and convert still images to grayscale",
        ),
        option(
            "threshold",
            "PCT",
            IMAGE,
            "With --diff, exit non-zero above PCT% changed (pixels for images, lines for \
             text). Also sets the exit code: 0 within, 5 over.",
        ),
    ]
}

fn inspect_options() -> Vec<ArgSpec> {
    vec![
        option(
            "select",
            "EXPR",
            INSPECT,
            "Show only what a JSONPath expression selects, e.g. `$.servers[*].name`",
        ),
        option(
            "find",
            "TEXT",
            INSPECT,
            "Search keys and values (case-insensitive), highlighting matches",
        ),
        flag(
            "flatten",
            INSPECT,
            "Show `path = value` rows instead of a tree",
        ),
        flag(
            "table",
            INSPECT,
            "Show records, or a path/value table, instead of a tree",
        ),
        option(
            "max-depth",
            "N",
            INSPECT,
            "Fold containers deeper than N levels",
        ),
        option(
            "max-length",
            "N",
            INSPECT,
            "Show at most N items per container",
        ),
        flag("show-paths", INSPECT, "Append each value's path"),
        flag(
            "redact",
            INSPECT,
            "Mask secret-looking keys such as password, token and api_key",
        ),
        option(
            "compare",
            "PATH",
            INSPECT,
            "Show added, removed and changed values against another document",
        )
        .value(ValueHint::File),
    ]
}

fn export_options() -> Vec<ArgSpec> {
    vec![
        option(
            "export-html",
            "PATH",
            EXPORT,
            "Also write a self-contained HTML document to PATH",
        )
        .short('o')
        .value(ValueHint::File)
        .config_key("export_html"),
        option(
            "export-svg",
            "PATH",
            EXPORT,
            "Also write an SVG document to PATH. Unlike the HTML, it references its font \
             from a CDN, so it is not self-contained offline.",
        )
        .value(ValueHint::File)
        .config_key("export_svg"),
    ]
}

fn paging_options() -> Vec<ArgSpec> {
    vec![
        flag(
            "pager",
            PAGING,
            "Page terminal output via MANPAGER, PAGER, then less/more.com",
        )
        .config_key("pager"),
        flag("no-pager", PAGING, "Disable explicit and automatic paging"),
        flag(
            "auto-pager",
            PAGING,
            "Page only terminal output taller than the viewport",
        )
        .config_key("auto_pager"),
        flag("no-auto-pager", PAGING, "Disable automatic paging"),
    ]
}

fn watch_options() -> Vec<ArgSpec> {
    vec![
        switch(
            "watch",
            "watch",
            WATCH,
            "Re-render changing files (several allowed) or one URL while stdout is a \
             terminal; each file gets its own live region",
        ),
        option("watch-interval", "SEC", WATCH, "Poll interval in seconds")
            .alias("interval")
            .default_value("1")
            .config_key("watch_interval"),
        option(
            "watch-debounce",
            "SEC",
            WATCH,
            "Quiet period collapsing a burst of file events",
        )
        .default_value("0.1")
        .config_key("watch_debounce"),
        switch(
            "watch-poll",
            "watch_poll",
            WATCH,
            "Poll local files at --watch-interval instead of file events",
        ),
        switch(
            "watch-exit-on-error",
            "watch_exit_on_error",
            WATCH,
            "End the watch with a non-zero exit when a render fails",
        ),
        switch(
            "watch-cache",
            "watch_cache",
            WATCH,
            "With URLs, render only when the response body changes",
        ),
    ]
}

fn batch_options() -> Vec<ArgSpec> {
    vec![
        switch(
            "batch",
            "batch",
            BATCH,
            "Convert explicit files, directories, or globs deterministically",
        ),
        option(
            "batch-input-root",
            "PATH",
            BATCH,
            "The directory --batch-preserve-dirs keeps paths relative to",
        )
        .value(ValueHint::Dir)
        .config_key("batch_input_root"),
        switch(
            "batch-preserve-dirs",
            "batch_preserve_dirs",
            BATCH,
            "Preserve paths under --batch-input-root PATH",
        ),
        option(
            "batch-name-template",
            "TEMPLATE",
            BATCH,
            "Name export leaves; export paths become directories",
        )
        .config_key("batch_name_template"),
        option(
            "jobs",
            "N",
            BATCH,
            "Parallel file-export workers; requires --batch. Terminal output stays in input \
             order; active jobs finish on error.",
        )
        .default_value("1")
        .config_key("jobs"),
        switch(
            "progress",
            "progress",
            BATCH,
            "Enable/disable batch counts on terminal stderr; hidden for redirected stderr, \
             JSON reports and dry runs",
        )
        .default_value("true"),
        flag(
            "dry-run",
            BATCH,
            "Validate and show the batch plan without writing files",
        ),
        switch(
            "continue-on-error",
            "continue_on_error",
            BATCH,
            "Process all planned inputs and aggregate failures",
        ),
        switch(
            "overwrite",
            "overwrite",
            BATCH,
            "Allow existing batch export destinations",
        ),
        option(
            "collision",
            "P",
            BATCH,
            "Batch policy for existing destinations",
        )
        .choices(["error", "overwrite", "suffix"])
        .default_value("error")
        .config_key("collision"),
    ]
}

fn config_options() -> Vec<ArgSpec> {
    vec![
        option(
            "config",
            "PATH",
            CONFIG,
            "Read versioned TOML defaults from PATH",
        )
        .value(ValueHint::File),
        option("profile", "NAME", CONFIG, "Select a config profile").default_value("default"),
        flag("no-config", CONFIG, "Disable config discovery"),
        option("theme", "NAME", CONFIG, "Select a named theme from config").config_key("theme"),
        option(
            "theme-style",
            "NAME=STYLE",
            CONFIG,
            "Override a theme binding; repeatable and worker-safe",
        )
        .multiple(true),
    ]
}

fn output_options() -> Vec<ArgSpec> {
    vec![
        flag(
            "no-color",
            OUTPUT,
            "Disable colored output (as does a non-empty NO_COLOR)",
        )
        .env("NO_COLOR")
        .config_key("no_color"),
        flag(
            "color",
            OUTPUT,
            "Override a config no_color setting (pipes remain plain)",
        ),
        switch(
            "sanitize",
            "sanitize",
            OUTPUT,
            "Replace input terminal controls, JSON/notebook strings, titles and captions \
             with visible inert text",
        ),
        option(
            "report",
            "F",
            OUTPUT,
            "Emit a result/error envelope on stderr",
        )
        .choices(["human", "json"])
        .default_value("human"),
        flag("machine-json", OUTPUT, "Alias for --report json"),
    ]
}

fn demo_options() -> Vec<ArgSpec> {
    vec![
        flag(
            "demo",
            DEMO,
            "Guided suite tour; pauses 3 seconds between sections on a TTY",
        ),
        flag(
            "demo-list",
            DEMO,
            "List stable tour sections: core, workflows, art",
        ),
        option(
            "demo-section",
            "NAME",
            DEMO,
            "With --demo, play one section only",
        )
        .choices(["core", "workflows", "art"]),
        option(
            "demo-delay",
            "SECONDS",
            DEMO,
            "Tour pause (0–60); no pauses when redirected; Ctrl+C stops",
        )
        .default_value("3"),
    ]
}

fn general_options() -> Vec<ArgSpec> {
    vec![
        flag("help", GENERAL, "Show this help").short('h'),
        flag("version", GENERAL, "Show the rs-rich-cli package version").short('V'),
    ]
}

/// Every config key, for `config explain` completion and suggestions.
fn config_keys() -> Vec<String> {
    let mut keys = vec!["mode".to_string()];
    keys.extend(
        root_args()
            .into_iter()
            .filter_map(|arg| arg.config_key)
            .filter(|key| key != "mode"),
    );
    keys
}

fn root_args() -> Vec<ArgSpec> {
    let mut args = vec![ArgSpec::positional("resource")
        .value(ValueHint::Path)
        .multiple(true)
        .heading(ARGUMENTS)
        .help(
            "A file path, an http(s) URL, or `-` for stdin (several with --batch, --watch \
             or --gif)",
        )];
    for group in [
        render_modes(),
        input_options(),
        layout_options(),
        mode_options(),
        image_options(),
        inspect_options(),
        diff_options(),
        viewer_options(),
        export_options(),
        paging_options(),
        watch_options(),
        batch_options(),
        config_options(),
        output_options(),
        demo_options(),
        general_options(),
    ] {
        args.extend(group);
    }
    args
}

fn config_command() -> CommandSpec {
    let same = "[OPTIONS]";
    CommandSpec::new("config")
        .about("Show, validate, explain or document configuration")
        .long_about(
            "Show, validate, explain or document configuration.\n\n\
             Each subcommand accepts --config PATH, --profile NAME, --no-config, --theme NAME \
             and the setting flags (--width, --pager, ...) that `rich` itself accepts, as \
             command-line overrides.",
        )
        .subcommand_required(true)
        .subcommand(
            CommandSpec::new("show")
                .about("Show configured settings with CLI overrides as JSON")
                .usage(format!("show {same}")),
        )
        .subcommand(
            CommandSpec::new("validate")
                .about("Validate TOML, all profiles and explicit setting values")
                .usage(format!("validate {same}")),
        )
        .subcommand(
            CommandSpec::new("explain")
                .about(
                    "Show which layer sets each setting — defaults, environment, config file, \
                     profile, command line — and what it overrides",
                )
                .usage(format!("explain {same} [KEY]"))
                .arg(
                    ArgSpec::positional("key")
                        .help("Explain one setting instead of all of them")
                        .value(ValueHint::Choices(
                            config_keys().into_iter().map(Choice::new).collect(),
                        )),
                ),
        )
        .subcommand(
            CommandSpec::new("reference")
                .about("List every configuration source and key")
                .usage(format!("reference {same}")),
        )
}

fn authoring_commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("completions")
            .about("Print a shell completion script")
            .arg(
                ArgSpec::positional("shell")
                    .required(true)
                    .help("The shell to complete for (pwsh is accepted for powershell)")
                    .choices(["bash", "zsh", "fish", "powershell"]),
            )
            .example(
                "rich completions bash > ~/.local/share/bash-completion/completions/rich",
                "Bash",
            )
            .example("rich completions zsh > \"${fpath[1]}/_rich\"", "Zsh")
            .example(
                "rich completions fish > ~/.config/fish/completions/rich.fish",
                "Fish",
            )
            .example(
                "rich completions powershell | Out-String | Invoke-Expression",
                "PowerShell",
            ),
        CommandSpec::new("docs")
            .about("Print reference documentation generated from this help")
            .subcommand_required(true)
            .subcommand(
                CommandSpec::new("markdown").about("Print the command reference as Markdown"),
            )
            .subcommand(
                CommandSpec::new("man")
                    .about("Print the rich(1) man page, or write every page to a directory")
                    .arg(
                        ArgSpec::option("output")
                            .value_name("DIR")
                            .value(ValueHint::Dir)
                            .help(
                                "Write rich.1 and one page per subcommand into DIR (created \
                                 if missing) and list them",
                            ),
                    ),
            )
            .subcommand(
                CommandSpec::new("config").about("Print the configuration reference as Markdown"),
            ),
        CommandSpec::new("bench")
            .about("Compare benchmark runs")
            .subcommand_required(true)
            .subcommand(
                CommandSpec::new("compare")
                    .about(
                        "Compare a candidate benchmark run with a baseline; exits 5 when any \
                         benchmark regressed",
                    )
                    .arg(
                        ArgSpec::positional("baseline")
                            .required(true)
                            .value(ValueHint::Path)
                            .help("A rich_ext::qa::bench JSON run, or a criterion directory"),
                    )
                    .arg(
                        ArgSpec::positional("candidate")
                            .required(true)
                            .value(ValueHint::Path)
                            .help("The run to judge, in the same form"),
                    )
                    .arg(
                        ArgSpec::option("threshold")
                            .value_name("PCT")
                            .default_value("5")
                            .help("Changes within ±PCT% (beyond noise) count as unchanged"),
                    )
                    .example(
                        "rich bench compare baseline.json candidate.json --threshold 10",
                        "Gate CI on a 10% slowdown",
                    ),
            ),
    ]
}

/// The whole `rich` command line.
pub(crate) fn spec() -> CommandSpec {
    let mut spec = CommandSpec::new("rich")
        .version(super::VERSION)
        .about(format!(
            "rich {} — Rust port of the rich-cli terminal toolbox\n\n\
             RESOURCE is a file path, an http(s) URL, or `-` for stdin. Everything after a \
             bare `--` is a RESOURCE, however much it looks like an option. Input modes with \
             no RESOURCE read stdin until EOF; `-p -` reads markup from stdin too. Terminal \
             stdin shows an input hint. Repeated scalar options use their last value.\n\n\
             A `--no-…` spelling (such as --no-watch) disables the corresponding \
             config/default boolean.",
            super::VERSION
        ))
        .usage("rich [OPTIONS] [RESOURCE]")
        .usage("rich [OPTIONS] <COMMAND> [RESOURCE]")
        .usage("rich --batch [OPTIONS] RESOURCE...")
        .usage("rich --watch [OPTIONS] FILE...")
        .args(root_args())
        .heading_note(
            RENDER_MODE,
            "Choose at most one; the default auto-detects .md/.json/.csv/.tsv/.ipynb by \
             extension — anything else with a file extension is syntax-highlighted.",
        )
        .heading_note(
            DEMO,
            "Self-contained examples; ignores config; accepts --no-color.",
        );
    for (name, aliases, about) in [
        (
            "print",
            &[][..],
            "Treat RESOURCE as literal markup TEXT (`--print`)",
        ),
        ("markdown", &["md"][..], "Render Markdown (`--markdown`)"),
        (
            "syntax",
            &["code"][..],
            "Syntax-highlight source (`--syntax`)",
        ),
        ("json", &[][..], "Pretty-print JSON (`--json`)"),
        ("csv", &["tsv"][..], "Render CSV/TSV as a table (`--csv`)"),
        (
            "ipynb",
            &["notebook"][..],
            "Render a Jupyter notebook (`--ipynb`)",
        ),
        (
            "jsonl",
            &["ndjson"][..],
            "Stream JSON Lines / NDJSON records (`--jsonl`)",
        ),
        (
            "log",
            &["logs"][..],
            "Stream common structured-log JSONL records (`--log`)",
        ),
        ("gif", &[][..], "Animate GIFs (`--gif`)"),
        (
            "diff",
            &[][..],
            "Compare two images, two text files, or render one patch (`--diff`)",
        ),
        (
            "image",
            &[][..],
            "Render a still image as ASCII/Braille/blocks/Sixel (`--image`)",
        ),
        ("rule", &[][..], "Draw a horizontal rule (`--rule`)"),
        (
            "inspect",
            &[][..],
            "Explore structured data (JSON, YAML, TOML, XML, INI, dotenv) as a tree \
             (`--inspect`)",
        ),
        (
            "ansi",
            &["ansi-explain"][..],
            "Decode escape sequences: `rich ansi explain FILE` (`--ansi-explain`)",
        ),
        (
            "view",
            &[][..],
            "Show any file: Markdown, CSV, notebooks, images and patches rendered, source and \
             data numbered and highlighted, binary as hex; paged, searchable with --search",
        ),
        (
            "hex",
            &["hexdump"][..],
            "Hex dump with offsets, byte groups and an ASCII panel (--search, --offset, --length)",
        ),
        (
            "unicode",
            &[][..],
            "Show graphemes, code points, UTF-8 bytes, widths and invalid sequences",
        ),
        (
            "env",
            &[][..],
            "List environment variables, secrets masked; `rich env PATH` checks each PATH entry",
        ),
        (
            "capture",
            &[][..],
            "Run `rich capture -- COMMAND ARGS…` and show or export its output (--cast FILE \
             records it), then exit with the command's status",
        ),
    ] {
        spec = spec.subcommand(mode_command(name, aliases, about));
    }
    spec = spec.subcommand(config_command());
    for command in authoring_commands() {
        spec = spec.subcommand(command);
    }
    spec.subcommand(
        CommandSpec::new("doctor")
            .about(
                "Read-only build, terminal, config and pager diagnostics; --report json writes \
                 diagnostic data to stdout",
            )
            .usage(
                "doctor [--report json] [--config PATH] [--profile NAME] [--no-config] \
                 [--no-color]",
            ),
    )
    .section(
        "Environment",
        "- NO_COLOR: any non-empty value disables colour\n\
         - COLUMNS: console width (default 80 when unavailable)\n\
         - MANPAGER, PAGER: pager command; fallback is less (Unix), more.com (Windows)\n\
         - FORCE_COLOR: not supported; redirected stdout stays plain\n\
         - RICH_SIXEL: 0/1 overrides Sixel detection for --image-mode auto",
    )
    .section(
        "",
        "With no RESOURCE and no mode flag, a capability demo is shown. Layout, style, \
         paging, hyperlinks and export options require a resource or render mode.",
    )
    .section(
        "Exit codes",
        "- 0: success\n\
         - 2: usage/config error\n\
         - 3: input/read/write error\n\
         - 4: parse/render data error\n\
         - 5: threshold/gate failure",
    )
}

/// The configuration reference: every source, lowest precedence first, and
/// every key.
pub(crate) fn config_reference() -> ConfigReference {
    let mut reference = ConfigReference::from_spec(&spec())
        .description(
            "Settings are TOML: `version = 1`, a `[defaults]` table, optional \
             `[profile.NAME]` tables and `[themes.NAME]` style tables. `rich config explain` \
             shows where each effective value comes from.",
        )
        .source("defaults", "", "Built-in defaults")
        .source(
            "environment",
            "NO_COLOR",
            "A non-empty value sets no_color = true. A no_color = false in ./rich.toml \
             cannot undo it; ~/.config/rich/config.toml, --config PATH and --color can",
        )
        .source(
            "config file",
            "./rich.toml, else ~/.config/rich/config.toml",
            "The [defaults] table. Only the first file found is read; --config PATH reads \
             that file instead and --no-config reads none",
        )
        .source(
            "profile",
            "[profile.NAME]",
            "The profile selected with --profile (default: default) overrides the [defaults] \
             table",
        )
        .source(
            "command line",
            "",
            "Explicit flags override every other source",
        );
    // The spec knows values only as text; these are what `validate_value`
    // accepts.
    for entry in &mut reference.entries {
        let kind = match entry.key.as_str() {
            "width" | "height" | "jobs" | "image_max_width" | "image_max_height" => {
                "positive integer"
            }
            "watch_interval" | "watch_debounce" | "image_brightness" | "image_contrast"
            | "image_gamma" => "number",
            _ => continue,
        };
        entry.kind = kind.to_string();
    }
    reference.entries.insert(
        0,
        ConfigEntry::new("mode", "enum")
            .choices(MODES.iter().copied())
            .default_value("auto")
            .flag("--print, --markdown, ... or a command word")
            .description("Render mode, as the flag or command of the same name"),
    );
    reference
}

/// The `defaults` precedence layer: each config key's built-in value. A flag
/// with no stated default is off.
pub(crate) fn default_layer() -> Layer {
    let mut layer = Layer::new("defaults").value("mode", "auto");
    for arg in root_args() {
        let Some(key) = &arg.config_key else { continue };
        match (&arg.default, arg.takes_value()) {
            (Some(value), _) => layer = layer.value(key.clone(), value.clone()),
            (None, false) => layer = layer.value(key.clone(), "false"),
            (None, true) => {}
        }
    }
    layer
}

/// Every known config key.
pub(crate) fn known_config_keys() -> Vec<String> {
    config_keys()
}

/// A console for stdout: colour only on a terminal, and never with
/// `no_color`.
pub(crate) fn console(no_color: bool) -> Console {
    Console::builder().no_color(no_color).build()
}

/// Render through a stdout console to a string (no trailing newline).
pub(crate) fn render(renderable: &dyn Renderable, no_color: bool) -> String {
    console(no_color).render_to_string(renderable)
}

/// `rich --help`.
pub(crate) fn print_help(no_color: bool) {
    console(no_color).print(&HelpView::new(&spec()));
}

/// Help for the subcommand at `path`, such as `["config", "explain"]`.
pub(crate) fn subcommand_help(path: &[&str], no_color: bool) -> Option<String> {
    let spec = spec();
    HelpView::for_path(&spec, path).map(|view| render(&view, no_color))
}

/// Colour off for NO_COLOR or `--no-color` (the last of `--no-color` and
/// `--color` wins), as the rest of the binary decides it before config.
pub(crate) fn no_color_requested(args: &[String]) -> bool {
    let mut no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    for arg in args.iter().take_while(|arg| *arg != "--") {
        match arg.as_str() {
            "--no-color" => no_color = true,
            "--color" => no_color = false,
            _ => {}
        }
    }
    no_color
}

#[cfg(test)]
mod tests {
    //! Drift between this spec and the hand-written parser.
    //!
    //! Heuristic for "what the parser recognises": every string literal that
    //! is exactly an option spelling (`"--name"` or `"-x"`) inside the parser
    //! functions — `parse_inner`, `VALUE_OPTIONS`, `mode_flag_alias` in
    //! `main.rs`, `arguments` and `boolean_flags` in `config.rs`, `options`
    //! in `demo.rs`, and the extension options in `rich_ext::cli`. Error
    //! messages and table labels such as `"--diff/--image"` never match a
    //! whole literal, so no exclusions are needed. The spec side is every
    //! short, long and alias of every argument, including subcommands.
    use super::*;
    use crate::config::{config_args, ConfigRoots};
    use std::collections::BTreeSet;

    const MAIN: &str = include_str!("main.rs");
    const CONFIG: &str = include_str!("config.rs");
    const DEMO: &str = include_str!("demo.rs");
    const INSPECT: &str = include_str!("inspect.rs");
    const TOOLS: &str = include_str!("tools.rs");
    const VIEWERS: &str = include_str!("viewers.rs");

    /// The source of the item that starts with `start`, up to the next
    /// top-level item.
    fn item<'a>(source: &'a str, start: &str) -> &'a str {
        let from = source
            .find(start)
            .unwrap_or_else(|| panic!("{start} not found"));
        let rest = &source[from..];
        let end = rest[1..]
            .find("\nfn ")
            .into_iter()
            .chain(rest[1..].find("\nconst "))
            .chain(rest[1..].find("\npub(crate) fn "))
            .chain(rest[1..].find("\npub(super) fn "))
            .min()
            .map_or(rest.len(), |i| i + 1);
        &rest[..end]
    }

    /// Whole string literals that are option spellings.
    fn option_literals(source: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for piece in source.split('"').skip(1).step_by(2) {
            let long = piece.len() > 2
                && piece.starts_with("--")
                && piece[2..].starts_with(|c: char| c.is_ascii_lowercase())
                && piece[2..]
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            let short = piece.len() == 2
                && piece.starts_with('-')
                && piece[1..].starts_with(|c: char| c.is_ascii_alphabetic());
            if long || short {
                out.insert(piece.to_string());
            }
        }
        out
    }

    fn parser_options() -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for source in [
            item(MAIN, "fn parse_inner("),
            item(MAIN, "const VALUE_OPTIONS"),
            item(MAIN, "fn mode_flag_alias("),
            item(CONFIG, "fn arguments("),
            item(CONFIG, "fn boolean_flags("),
            item(DEMO, "fn options("),
            item(INSPECT, "impl DataOptions {"),
            item(TOOLS, "impl ToolOptions {"),
            item(VIEWERS, "impl ViewerOptions {"),
        ] {
            out.extend(option_literals(source));
        }
        for arg in rich_ext::cli::arg_specs() {
            out.extend(arg.switches());
        }
        out
    }

    fn all_args(spec: &CommandSpec) -> Vec<ArgSpec> {
        let mut out: Vec<ArgSpec> = spec.args.clone();
        for command in &spec.subcommands {
            out.extend(all_args(command));
        }
        out
    }

    fn spec_switches() -> BTreeSet<String> {
        all_args(&spec())
            .iter()
            .filter(|arg| !arg.positional)
            .flat_map(ArgSpec::switches)
            .collect()
    }

    #[test]
    fn the_heuristic_sees_the_parser() {
        let found = parser_options();
        for known in [
            "--width",
            "-w",
            "--no-watch",
            "--config",
            "--demo-delay",
            "--gif-mode",
        ] {
            assert!(found.contains(known), "{known} not found: {found:?}");
        }
        assert!(!found.iter().any(|o| o.contains('/')));
        assert!(
            found.len() > 90,
            "the heuristic found too little: {found:?}"
        );
    }

    #[test]
    fn every_parser_option_is_in_the_spec() {
        let documented = spec_switches();
        let missing: Vec<_> = parser_options()
            .into_iter()
            .filter(|option| !documented.contains(option))
            .collect();
        assert!(missing.is_empty(), "undocumented options: {missing:?}");
    }

    #[test]
    fn every_spec_option_has_a_parser() {
        let parser = parser_options();
        let unknown: Vec<_> = spec_switches()
            .into_iter()
            .filter(|s| !parser.contains(s))
            // Subcommand-local options are parsed by their subcommand.
            .filter(|s| s != "--output")
            .collect();
        assert!(
            unknown.is_empty(),
            "spec options with no parser: {unknown:?}"
        );
    }

    /// A value the option accepts, or at least one that reaches its own
    /// validation rather than the unknown-option arm.
    fn sample(arg: &ArgSpec) -> String {
        arg.choice_list()
            .first()
            .map_or_else(|| "1".to_string(), |choice| choice.value.clone())
    }

    #[test]
    fn every_spec_switch_is_accepted_by_the_parser() {
        let root = tempfile::tempdir().unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        let demo = demo_options();
        for arg in root_args().iter().filter(|arg| !arg.positional) {
            for switch in arg.switches() {
                if demo.iter().any(|d| d.id == arg.id) {
                    // The tour has its own parser; it is covered by
                    // `every_parser_option_is_in_the_spec` via `demo.rs`.
                    assert!(DEMO.contains(&format!("\"{switch}\" =>")), "{switch}");
                    continue;
                }
                let mut args = vec![switch.clone()];
                if arg.takes_value() {
                    args.push(sample(arg));
                }
                args.push("input.txt".into());
                let result = config_args(&args, &roots).and_then(|merged| {
                    // `--help`/`--version` print; that is acceptance too.
                    super::super::parse_inner(&merged).map(|_| ())
                });
                if let Err(message) = result {
                    assert!(
                        !message.starts_with("unknown option"),
                        "{switch} rejected: {message}"
                    );
                }
            }
        }
    }

    #[test]
    fn value_taking_agrees_with_the_parser() {
        for arg in root_args().iter().filter(|arg| !arg.positional) {
            for switch in arg.switches() {
                let consumes = super::super::VALUE_OPTIONS.contains(&switch.as_str());
                assert_eq!(
                    arg.takes_value(),
                    consumes,
                    "{switch}: spec and VALUE_OPTIONS disagree about taking a value"
                );
            }
        }
    }

    #[test]
    fn config_keys_match_the_validator() {
        let keys = config_keys();
        for key in &keys {
            // Every documented key is one `validate_value` knows.
            let probe = toml::Value::Boolean(true);
            if let Err(message) = crate::config::validate_value(key, &probe) {
                assert!(!message.starts_with("unknown key"), "{key}: {message}");
            }
        }
        for key in crate::config::all_keys() {
            assert!(
                keys.iter().any(|k| k == key),
                "{key} has no documented flag"
            );
        }
    }

    #[test]
    fn subcommands_cover_every_mode_word() {
        let spec = spec();
        for mode in super::super::MODE_SPECS.iter().skip(1) {
            for alias in mode.aliases {
                assert!(spec.find_subcommand(alias).is_some(), "{alias}");
            }
        }
        for name in ["config", "completions", "docs", "doctor", "inspect"] {
            assert!(spec.find_subcommand(name).is_some(), "{name}");
        }
    }

    #[test]
    fn help_keeps_the_documented_phrases() {
        let out = Console::builder()
            .width(100)
            .build()
            .render_to_string(&HelpView::new(&spec()));
        let flat = out.split_whitespace().collect::<Vec<_>>().join(" ");
        for phrase in [
            "Usage: rich [OPTIONS] [RESOURCE]",
            "rich --watch [OPTIONS] FILE...",
            "stdin until EOF",
            "Choose at most one",
            "-y, --hyperlinks",
            "default 1; 0 = forever",
            "--jobs <N>",
            "[config: watch_interval]",
            "[env: NO_COLOR]",
            "RICH_SIXEL",
            "5: threshold/gate failure",
            "a capability demo is shown",
        ] {
            assert!(flat.contains(phrase), "missing {phrase:?}:\n{out}");
        }
    }
}
