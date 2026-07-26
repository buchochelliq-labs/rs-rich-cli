//! `rich` — a Rust port of the `rich-cli` terminal toolbox.
//!
//! This binary mirrors the upstream `rich-cli` command-line tool and is built on
//! the [`rich`] library crate. It implements a slice of upstream's rendering
//! flags — `--print`, `--markdown`, `--json`, `--rule` — plus width/justify
//! options, a plain-file printer (with extension auto-detection), and a
//! capability demo. Syntax, CSV, and export flags are tracked as roadmap issues.

use std::io::Read;
use std::process::ExitCode;

use rich::markdown::Markdown;
use rich::r#box::{DOUBLE, SQUARE};
use rich::text::Text;
use rich::{
    filesize, Align, Bar, ColorSystem, Columns, Console, Constrain, Control, HorizontalAlign, Json,
    Justify, Layout, Padding, Panel, ProgressBar, Renderable, Rule, Spinner, Table, Tree,
};
use rich_ext::ConsoleExt;

/// Boxed `Text` helper to cut down on `Box::new(Text::new(...))` noise.
fn text(content: &str) -> Box<dyn Renderable> {
    Box::new(Text::new(content))
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How to render the resource. Mirrors the mutually-exclusive rich-cli flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// No explicit flag: auto-detect by file extension, else print plain.
    Auto,
    /// `-p/--print`: interpret the resource as console markup.
    Print,
    /// `-m/--markdown`: render the resource as Markdown.
    Markdown,
    /// `-j/--json`: pretty-print the resource as JSON.
    Json,
    /// `--rule`: draw a horizontal rule (the resource, if any, is its title).
    Rule,
}

/// Parsed command line.
struct Cli {
    mode: Mode,
    resource: Option<String>,
    width: Option<usize>,
    justify: Option<Justify>,
    no_color: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(None) => ExitCode::SUCCESS, // help/version already printed
        Ok(Some(cli)) => run(cli),
        Err(message) => {
            eprintln!("rich: {message} (try --help)");
            ExitCode::FAILURE
        }
    }
}

/// Set the render mode, rejecting a second, conflicting mode flag.
fn set_mode(current: &mut Mode, mode: Mode) -> Result<(), String> {
    if *current != Mode::Auto && *current != mode {
        return Err("only one of --print/--markdown/--json/--rule may be given".into());
    }
    *current = mode;
    Ok(())
}

/// Parse args into a [`Cli`], or `Ok(None)` when `--help`/`--version` handled it.
fn parse(args: &[String]) -> Result<Option<Cli>, String> {
    let mut mode = Mode::Auto;
    let mut resource = None;
    let mut width = None;
    let mut justify = None;
    let mut no_color = false;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("rich (rs-rich-cli) {VERSION}");
                return Ok(None);
            }
            "-p" | "--print" => set_mode(&mut mode, Mode::Print)?,
            "-m" | "--markdown" => set_mode(&mut mode, Mode::Markdown)?,
            "-j" | "--json" => set_mode(&mut mode, Mode::Json)?,
            "--rule" => set_mode(&mut mode, Mode::Rule)?,
            "--left" => justify = Some(Justify::Left),
            "--right" => justify = Some(Justify::Right),
            "--center" => justify = Some(Justify::Center),
            "--no-color" => no_color = true,
            "-w" | "--width" => {
                let value = iter.next().ok_or("--width requires a number")?;
                width = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid width {value:?}"))?,
                );
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other:?}"));
            }
            other => {
                if resource.is_some() {
                    return Err("only one resource may be given".into());
                }
                resource = Some(other.to_string());
            }
        }
    }

    Ok(Some(Cli {
        mode,
        resource,
        width,
        justify,
        no_color,
    }))
}

/// Read a resource: `-` (or `None`) means stdin, otherwise a file path.
fn read_resource(resource: Option<&str>) -> std::io::Result<String> {
    match resource {
        Some(path) if path != "-" => std::fs::read_to_string(path),
        _ => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            Ok(buffer)
        }
    }
}

/// Resolve `Mode::Auto` to a concrete mode from the resource's file extension.
fn detect_mode(resource: Option<&str>) -> Mode {
    match resource.and_then(|r| r.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())) {
        Some(ext) if ext == "md" || ext == "markdown" => Mode::Markdown,
        Some(ext) if ext == "json" => Mode::Json,
        _ => Mode::Auto,
    }
}

fn run(cli: Cli) -> ExitCode {
    // With no flags and no resource, show the capability demo.
    if cli.mode == Mode::Auto && cli.resource.is_none() {
        run_demo();
        return ExitCode::SUCCESS;
    }

    let mut builder = Console::builder().no_color(cli.no_color);
    if let Some(width) = cli.width {
        builder = builder.width(width);
    }
    let mut console = builder.build();
    console.install_extensions();

    let mode = match cli.mode {
        Mode::Auto => detect_mode(cli.resource.as_deref()),
        other => other,
    };

    // A rule takes its optional title from the resource string directly.
    if mode == Mode::Rule {
        match cli.resource.as_deref() {
            Some(title) if title != "-" => console.print(&Rule::new(title)),
            _ => console.print(&Rule::line()),
        }
        return ExitCode::SUCCESS;
    }

    // `--print` treats the resource as a literal markup string, not a file path
    // (stdin when it's `-` or absent). Other modes read the resource as a file.
    let content = if mode == Mode::Print && matches!(cli.resource.as_deref(), Some(r) if r != "-") {
        cli.resource.clone().unwrap()
    } else {
        match read_resource(cli.resource.as_deref()) {
            Ok(content) => content,
            Err(err) => {
                eprintln!(
                    "rich: cannot read {}: {err}",
                    cli.resource.as_deref().unwrap_or("<stdin>")
                );
                return ExitCode::FAILURE;
            }
        }
    };

    match mode {
        Mode::Markdown => console.print(&Markdown::new(&content)),
        Mode::Json => match Json::new(content.trim()) {
            Ok(json) => console.print(&json),
            Err(err) => {
                eprintln!("rich: invalid JSON: {err}");
                return ExitCode::FAILURE;
            }
        },
        Mode::Print => match cli.justify {
            Some(justify) => console.print_justified(&content, justify),
            None => console.print_str(&content),
        },
        // Auto with no detected type: print the file as plain text.
        _ => match cli.justify {
            Some(justify) => console.print_justified(&content, justify),
            None => console.print(&Text::new(content)),
        },
    }
    ExitCode::SUCCESS
}

fn print_help() {
    println!(
        "rich {VERSION} — Rust port of the rich-cli terminal toolbox\n\
\n\
USAGE:\n\
    rich [OPTIONS] [RESOURCE]\n\
\n\
RESOURCE is a file path, or `-` for stdin.\n\
\n\
RENDER MODE (choose at most one; default auto-detects by extension):\n\
    -p, --print      Interpret RESOURCE as console markup\n\
    -m, --markdown   Render RESOURCE as Markdown\n\
    -j, --json       Pretty-print RESOURCE as JSON\n\
        --rule       Draw a horizontal rule (RESOURCE is its title)\n\
\n\
OPTIONS:\n\
    -w, --width N    Set the output width\n\
        --left       Left-justify output\n\
        --center     Center output\n\
        --right      Right-justify output\n\
        --no-color   Disable colored output\n\
    -h, --help       Show this help\n\
    -V, --version    Show the version (mirrors upstream rich-cli)\n\
\n\
With no RESOURCE and no mode flag, a capability demo is shown.\n\
\n\
NOT YET PORTED (tracked as roadmap issues): syntax, csv/tsv, ipynb, export-html.\n"
    );
}

fn run_demo() {
    // Force truecolor so the demo looks the same regardless of TERM.
    let mut console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    console.install_extensions();

    console.print(&Rule::new("rs-rich-cli"));
    console.print_str("[bold magenta]rs-rich-cli[/] — a Rust port of [italic]rich[/]");
    console.print_str("Everything below is byte-parity-tested against Python rich 15.0.0.");

    // Markup, color, theme, and the rich-ext highlighter.
    console.print(&Rule::new("markup · color · theme · extension"));
    console.print_str(
        "Styles:   [bold]bold[/] [dim]dim[/] [italic]italic[/] [underline]underline[/] [reverse]reverse[/]",
    );
    console.print_str(
        "Color:    [red]red[/] [green]green[/] [blue]blue[/] [#ff8800]#ff8800[/] [white on blue]on blue[/]",
    );
    console.print_str("Theme:    [error]error[/], [warning]warning[/], [info]info[/]");
    console.print_str("Extension: numbers like 3.14 and 2026 are auto-highlighted");
    // Built-in ReprHighlighter (highlight=true): auto-colors numbers, bools,
    // strings, paths, calls, etc.
    let hl = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .highlight(true)
        .build();
    hl.print_str("Highlight:  result = func(42, True, None, '/usr/bin')");
    console.print_str("Emoji:     :rocket: :fire: :sparkles: :thumbs_up: :snake: :coffee:");

    // Rule variants (title alignment).
    console.print(&Rule::new("rule"));
    console.print(&Rule::line());
    console.print(&Rule::new("centered"));
    console.print(&Rule::new("left").align(HorizontalAlign::Left));
    console.print(&Rule::new("right").align(HorizontalAlign::Right));

    // Panels with different boxes.
    console.print(&Rule::new("panel"));
    console.print(&Panel::new(text("rounded box (default)")).title("rounded"));
    console.print(
        &Panel::new(text("square box"))
            .box_set(SQUARE)
            .title("square"),
    );
    console.print(
        &Panel::new(text("double box"))
            .box_set(DOUBLE)
            .title("double"),
    );
    console.print(
        &Panel::new(text("title + subtitle"))
            .title("top")
            .subtitle("bottom"),
    );

    // Padding (shown inside a panel so the blank space is visible).
    console.print(&Rule::new("padding"));
    console.print(
        &Panel::new(Box::new(Padding::new(text("padded (1, 4)"), (1, 4, 1, 4)))).title("padding"),
    );

    // Horizontal alignment (fills the console width).
    console.print(&Rule::new("align"));
    console.print(&Align::left(text("← left")));
    console.print(&Align::center(text("center")));
    console.print(&Align::right(text("right →")));

    // Text justify (via print(justify=…)).
    console.print(&Rule::new("justify"));
    console.print_justified("left justified", Justify::Left);
    console.print_justified("centered", Justify::Center);
    console.print_justified("right justified", Justify::Right);

    // Constrain — same panel, capped to 24 cells.
    console.print(&Rule::new("constrain (width 24)"));
    console.print(&Constrain::new(
        Box::new(Panel::new(text("constrained")).title("≤24")),
        Some(24),
    ));

    // Table.
    console.print(&Rule::new("table"));
    let mut table = Table::new().title("ported renderables");
    table.add_column("Renderable");
    table.add_column("Module");
    table.add_column_justify("Parity", Justify::Center);
    for (renderable, module, parity) in [
        ("Text / markup", "text, markup", "✓"),
        ("Rule", "rule", "✓"),
        ("Panel", "panel", "✓"),
        ("Padding", "padding", "✓"),
        ("Align", "align", "✓"),
        ("Constrain", "constrain", "✓"),
        ("Columns", "columns", "✓"),
        ("Table", "table", "✓"),
        ("Tree", "tree", "✓"),
        ("ProgressBar", "progress_bar", "✓"),
    ] {
        table.add_row(&[renderable, module, parity]);
    }
    console.print(&table);

    // Columns — packs items into as many columns as fit the width.
    console.print(&Rule::new("columns"));
    let months = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    console.print(&Columns::new(
        months.iter().map(|m| m.to_string()).collect(),
    ));

    // Tree.
    console.print(&Rule::new("tree"));
    let mut tree = Tree::new("rs-rich-cli");
    let core = tree.add("crates/rich (core, mirrors rich 15.0.0)");
    core.add("color · style · text · console");
    core.add("box · rule · panel · padding · align");
    core.add("table · tree · wrap · filesize");
    tree.add("crates/rich-ext (our extensions)");
    tree.add("crates/rich-cli (this binary)");
    console.print(&tree);

    // Progress bars at a few completion levels (0 → 100%).
    console.print(&Rule::new("progress bar"));
    for pct in [0.0, 33.0, 66.0, 100.0] {
        console.print(&ProgressBar::new(100.0, pct).width(48));
    }

    // JSON — pretty-printed and highlighted.
    console.print(&Rule::new("json"));
    if let Ok(json) = Json::new(
        r#"{"port": "rich", "version": "15.0.0", "parity": true, "widgets": ["panel", "table", "tree"]}"#,
    ) {
        console.print(&json);
    }

    // Bar — a filled span within a range (eighth-block resolution).
    console.print(&Rule::new("bar (range)"));
    for (begin, end) in [(0.0, 100.0), (0.0, 62.0), (20.0, 80.0), (55.0, 100.0)] {
        console.print(&Bar::new(100.0, begin, end).width(48));
    }

    // Markdown — headings, inline styles, lists, block quotes, and rules.
    console.print(&Rule::new("markdown"));
    console.print(&Markdown::new(
        "# Heading\n\nA paragraph with **bold**, *italic*, and `code`.\n\n- bullet item\n- another\n\n1. first\n2. second\n\n> a block quote\n\n---",
    ));

    // Control codes — cursor/screen sequences (shown escaped, not executed).
    console.print(&Rule::new("control codes"));
    let show_escape = |label: &str, control: &Control| {
        // Escaped so the sequence is visible (and its `[` isn't read as markup).
        let escaped = control
            .as_str()
            .replace('\x1b', "\\x1b")
            .replace('\x07', "\\a");
        console.print(&Text::new(format!("  {label:>14}: {escaped}")));
    };
    show_escape("clear screen", &Control::clear());
    show_escape("home", &Control::home());
    show_escape("move (2,-1)", &Control::move_(2, -1));
    show_escape("move_to (3,4)", &Control::move_to(3, 4));
    show_escape("hide cursor", &Control::show_cursor(false));
    show_escape("bell", &Control::bell());

    // ANSI decoder — parse a raw SGR string back into styled Text, then re-render.
    console.print(&Rule::new("ansi decoder"));
    let raw =
        "\x1b[1;31mred bold\x1b[0m \x1b[38;5;214morange\x1b[0m \x1b[3;4mitalic underline\x1b[0m";
    console.print(&Text::new(format!(
        "  input:  {}",
        raw.replace('\x1b', "\\x1b")
    )));
    let mut decoder = rich::AnsiDecoder::new();
    for line in decoder.decode(raw) {
        console.print(&Text::new("  output: ").append_text(&line));
    }

    // Capture / export — record a rendering to a buffer, then strip its styles.
    console.print(&Rule::new("capture · export_text"));
    let plain = console.export_text(|c| {
        c.print(&Panel::new(text("captured panel")).box_set(SQUARE));
    });
    console.print(&Text::new("  export_text (styles stripped):"));
    for line in plain.lines() {
        console.print(&Text::new(format!("    {line}")));
    }

    // Layout — split a region into ratioed rows/columns; Panel leaves expand
    // to fill their region (rendered here at a fixed size).
    console.print(&Rule::new("layout"));
    let panel_leaf = |title: &str, body: &str| {
        Layout::with_renderable(Box::new(
            Panel::new(text(body))
                .box_set(SQUARE)
                .title(title.to_string()),
        ))
    };
    let mut header = Layout::new();
    header.split_row(vec![
        panel_leaf("left", "pane A"),
        panel_leaf("right", "pane B"),
    ]);
    let mut root = Layout::new();
    root.split_column(vec![
        header,
        Layout::with_renderable(text("footer (fixed size 3)")).size(3),
    ]);
    let grid = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(48)
        .height(7)
        .build();
    for line in grid.export_text(|c| c.print(&root)).lines() {
        console.print(&Text::new(line));
    }

    // Spinners — a few frames of each built-in (animation needs a Live loop).
    console.print(&Rule::new("spinner (frames)"));
    for name in ["dots", "line", "arrow", "simpleDots"] {
        let spinner = Spinner::new(name);
        let frames: String = (0..6)
            .map(|i| console.render_to_string(&spinner.render(i as f64 * 0.15)))
            .collect::<Vec<_>>()
            .join(" ");
        console.print_str(&format!("  {name:>11}: {frames}"));
    }

    // filesize.
    console.print(&Rule::new("filesize"));
    for bytes in [1u64, 999, 1_000, 1_500, 1_000_000, 1_500_000_000] {
        console.print_str(&format!("  {bytes:>13} → {}", filesize::decimal(bytes)));
    }
    console.print(&Rule::line());
}
