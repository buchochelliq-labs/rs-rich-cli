//! `rich` — a Rust port of the `rich-cli` terminal toolbox.
//!
//! This binary mirrors the upstream `rich-cli` command-line tool and is built on
//! the [`rich`] library crate. It implements a slice of upstream's rendering
//! flags — `--print`, `--markdown`, `--json`, `--syntax`, `--csv`, `--rule` — plus width/justify
//! options, a plain-file printer (with extension auto-detection), and a
//! capability demo. Syntax, CSV, and export flags are tracked as roadmap issues.

use std::io::Read;
use std::process::ExitCode;

use rich::markdown::Markdown;
use rich::r#box::{DOUBLE, SQUARE};
use rich::text::Text;
use rich::{
    filesize, Align, Bar, ColorSystem, Columns, Console, Constrain, Control, Highlighter,
    HorizontalAlign, ISO8601Highlighter, Json, Justify, Layout, Live, LiveRender, LogLevel,
    LogRender, Padding, Panel, Pretty, Progress, ProgressBar, ProgressColumn, Renderable, Rule,
    Spinner, Status, Style, Styled, Syntax, Table, Traceback, Tree, DEFAULT_TERMINAL_THEME,
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
    /// `-x/--syntax`: syntax-highlight the resource (language from extension).
    Syntax,
    /// `--csv`: render a CSV/TSV resource as a table.
    Csv,
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
    /// Emit a self-contained HTML document instead of writing to the terminal.
    export_html: bool,
    /// Emit a self-contained SVG document instead of writing to the terminal.
    export_svg: bool,
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
        return Err(
            "only one render mode (--print/--markdown/--json/--syntax/--csv/--rule) may be given"
                .into(),
        );
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
    let mut export_html = false;
    let mut export_svg = false;

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
            "-x" | "--syntax" => set_mode(&mut mode, Mode::Syntax)?,
            "--csv" => set_mode(&mut mode, Mode::Csv)?,
            "--rule" => set_mode(&mut mode, Mode::Rule)?,
            "--left" => justify = Some(Justify::Left),
            "--right" => justify = Some(Justify::Right),
            "--center" => justify = Some(Justify::Center),
            "--no-color" => no_color = true,
            "--export-html" => export_html = true,
            "--export-svg" => export_svg = true,
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

    if export_html && export_svg {
        return Err("only one of --export-html/--export-svg may be given".into());
    }

    Ok(Some(Cli {
        mode,
        resource,
        width,
        justify,
        no_color,
        export_html,
        export_svg,
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
        Some(ext) if ext == "csv" || ext == "tsv" => Mode::Csv,
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

    // SVG export needs a title (the resource's basename, else "rich") and a
    // stable id. Upstream's auto id hashes Python reprs, so we use a fixed one
    // (see the rich crate's svg module / DIVERGENCES #15).
    let svg_title = cli
        .resource
        .as_deref()
        .filter(|r| *r != "-")
        .map(|r| r.rsplit(['/', '\\']).next().unwrap_or(r).to_string())
        .unwrap_or_else(|| "rich".to_string());
    let export = if cli.export_svg {
        Export::Svg {
            title: &svg_title,
            unique_id: "rich-cli",
        }
    } else if cli.export_html {
        Export::Html
    } else {
        Export::Terminal
    };

    let mode = match cli.mode {
        Mode::Auto => detect_mode(cli.resource.as_deref()),
        other => other,
    };

    // A rule takes its optional title from the resource string directly.
    if mode == Mode::Rule {
        let rule = match cli.resource.as_deref() {
            Some(title) if title != "-" => Rule::new(title),
            _ => Rule::line(),
        };
        emit(&console, &export, |c| c.print(&rule));
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

    // Pre-parse JSON so a parse error surfaces before any (HTML) rendering.
    let json = if mode == Mode::Json {
        match Json::new(content.trim()) {
            Ok(json) => Some(json),
            Err(err) => {
                eprintln!("rich: invalid JSON: {err}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    // The highlighting language comes from the resource's file extension.
    let language = cli
        .resource
        .as_deref()
        .and_then(|r| r.rsplit_once('.').map(|(_, ext)| ext.to_string()))
        .unwrap_or_default();

    // Pre-build the CSV/TSV table (delimiter from the extension: tab for `.tsv`).
    let csv_table = if mode == Mode::Csv {
        let delimiter = if language == "tsv" { '\t' } else { ',' };
        Some(render_csv(&parse_csv(&content, delimiter)))
    } else {
        None
    };

    emit(&console, &export, |c| match mode {
        Mode::Markdown => c.print(&Markdown::new(&content)),
        Mode::Json => c.print(json.as_ref().expect("json parsed above")),
        Mode::Csv => c.print(csv_table.as_ref().expect("csv built above")),
        Mode::Syntax => c.print(&Syntax::new(content.as_str(), language.as_str())),
        Mode::Print => match cli.justify {
            Some(justify) => c.print_justified(&content, justify),
            None => c.print_str(&content),
        },
        // Auto with no detected type: print the resource as plain text.
        _ => match cli.justify {
            Some(justify) => c.print_justified(&content, justify),
            None => c.print(&Text::new(content.as_str())),
        },
    });
    ExitCode::SUCCESS
}

/// Parse CSV/TSV `content` into rows of fields. Handles double-quoted fields
/// (with `""` escaping) that may contain the delimiter or newlines; `\r` outside
/// quotes is dropped (so `\r\n` line endings work). A trailing newline does not
/// produce an empty final row.
fn parse_csv(content: &str, delimiter: char) -> Vec<Vec<String>> {
    // Strip a leading UTF-8 BOM so it doesn't cling to the first header cell.
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    // Whether the current record has any content yet (so a lone `""` or a
    // trailing delimiter still yields a field, but a bare newline does not).
    let mut pending = false;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
            pending = true;
        } else if c == delimiter {
            row.push(std::mem::take(&mut field));
            pending = true;
        } else if c == '\n' {
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
            pending = false;
        } else if c != '\r' {
            field.push(c);
            pending = true;
        }
    }
    if pending || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Whether `value` is a plain number (`-?[0-9]+(\.[0-9]+)?`). Mirrors rich-cli's
/// `is_number` for the numeric-column heuristic.
fn is_number(value: &str) -> bool {
    let value = value.trim();
    let body = value.strip_prefix('-').unwrap_or(value);
    let mut parts = body.split('.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next();
    if parts.next().is_some() {
        return false; // more than one '.'
    }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(int_part) && frac_part.map_or(true, digits)
}

/// Build a table from parsed CSV `rows`, mirroring rich-cli's `render_csv`:
/// `HEAVY_HEAD` box (the `Table` default), a blue border, the first row as the
/// header, and any all-numeric column right-justified with bold-green body +
/// header cells.
///
/// First slice: the first row is always treated as the header. `csv.Sniffer`'s
/// dialect/has-header heuristics and the title/caption are follow-ups.
fn render_csv(rows: &[Vec<String>]) -> Table {
    let mut table = Table::new().border_style(Style::parse("blue").expect("valid style"));
    let Some((header, data)) = rows.split_first() else {
        return table;
    };
    for (index, name) in header.iter().enumerate() {
        // A column is numeric when no data cell is a non-empty non-number
        // (empty cells are allowed); an empty data set counts as numeric, as
        // upstream's `for … else` does.
        let numeric = data.iter().all(|row| {
            let value = row.get(index).map(String::as_str).unwrap_or("");
            value.is_empty() || is_number(value)
        });
        if numeric {
            table.add_column_justify(name, Justify::Right);
            table.column_style(Style::parse("bold green").expect("valid style"));
            table.column_header_fill(Style::parse("bold green").expect("valid style"));
        } else {
            table.add_column(name);
        }
    }
    for row in data {
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        table.add_row(&cells);
    }
    table
}

/// Where a render action's output goes: straight to the terminal, or captured
/// into a self-contained HTML or SVG document.
enum Export<'a> {
    Terminal,
    Html,
    Svg { title: &'a str, unique_id: &'a str },
}

/// Apply a render action per the chosen [`Export`] target.
fn emit(console: &Console, export: &Export, render: impl FnOnce(&Console)) {
    match export {
        Export::Terminal => render(console),
        // CSS-class stylesheet form (upstream rich-cli's default HTML export).
        Export::Html => print!("{}", console.export_html_classes(render)),
        Export::Svg { title, unique_id } => {
            print!("{}", console.export_svg(title, unique_id, render))
        }
    }
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
    -x, --syntax     Syntax-highlight RESOURCE (language from its extension)\n\
        --csv        Render RESOURCE as a CSV/TSV table\n\
        --rule       Draw a horizontal rule (RESOURCE is its title)\n\
\n\
OPTIONS:\n\
    -w, --width N     Set the output width\n\
        --left        Left-justify output\n\
        --center      Center output\n\
        --right       Right-justify output\n\
        --export-html Emit a self-contained HTML document instead of ANSI\n\
        --export-svg  Emit a self-contained SVG document instead of ANSI\n\
        --no-color    Disable colored output\n\
    -h, --help        Show this help\n\
    -V, --version     Show the version (mirrors upstream rich-cli)\n\
\n\
With no RESOURCE and no mode flag, a capability demo is shown.\n\
\n\
NOT YET PORTED (tracked as roadmap issues): csv/tsv, ipynb, URL fetch.\n"
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
    // ISO8601Highlighter — colors date/time/timezone fields.
    let mut stamp = Text::new("2023-06-15T13:45:30+02:00");
    ISO8601Highlighter::new().highlight(&mut stamp);
    console.print(&Text::new("ISO 8601:   ").append_text(&stamp));
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
    // Legacy-Windows box substitution: a ROUNDED panel falls back to SQUARE.
    let legacy = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(console.width())
        .legacy_windows(true)
        .build();
    legacy.print(&Panel::new(text("rounded → square on legacy Windows")).title("legacy"));

    // Padding (shown inside a panel so the blank space is visible).
    console.print(&Rule::new("padding"));
    console.print(
        &Panel::new(Box::new(Padding::new(text("padded (1, 4)"), (1, 4, 1, 4)))).title("padding"),
    );

    // Styled — lay one style under an entire renderable.
    console.print(&Rule::new("styled"));
    console.print(&Styled::new(
        Box::new(Panel::new(text("green under the whole panel")).title("styled")),
        Style::parse("green").unwrap(),
    ));

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
    // A styled column and a fixed-width column (content wraps with ellipsis).
    table
        .add_column("Renderable")
        .column_style(Style::parse("cyan").unwrap());
    table.add_column("Module").column_width(12);
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

    // Progress display — description + flexing bar + percentage (static frame).
    console.print(&Rule::new("progress"));
    let mut progress = Progress::new();
    progress.add_task("Downloading", 100.0, 50.0);
    progress.add_task("Processing", 100.0, 100.0);
    progress.add_task("Waiting", 100.0, 0.0);
    console.print(&progress);

    // Progress with custom columns — description + bar + M-of-N counter.
    let mut mofn = Progress::new().columns(vec![
        ProgressColumn::Description,
        ProgressColumn::Bar,
        ProgressColumn::MofN,
    ]);
    mofn.add_task("Files", 8.0, 3.0);
    mofn.add_task("Chunks", 128.0, 128.0);
    console.print(&mofn);

    // Progress with a download column — completed/total in shared byte units.
    let mut download = Progress::new().columns(vec![
        ProgressColumn::Description,
        ProgressColumn::Bar,
        ProgressColumn::Download,
    ]);
    download.add_task("archive.tar", 10_000_000.0, 4_200_000.0);
    console.print(&download);

    // JSON — pretty-printed and highlighted.
    console.print(&Rule::new("json"));
    if let Ok(json) = Json::new(
        r#"{"port": "rich", "version": "15.0.0", "parity": true, "widgets": ["panel", "table", "tree"]}"#,
    ) {
        console.print(&json);
    }

    // Pretty — colorize a Rust value's Debug output (Rust-native rich.pretty).
    console.print(&Rule::new("pretty"));
    let value = vec![("panel", 1u32), ("table", 2), ("tree", 3)];
    console.print(&Pretty::new(&value));

    // Log records — a severity-colored line per record (Rust-native rich.logging).
    console.print(&Rule::new("log"));
    for (level, message) in [
        (LogLevel::Info, "server started on port 8080"),
        (LogLevel::Debug, "cache warm: 128 entries"),
        (LogLevel::Warn, "disk usage at 85%"),
        (LogLevel::Error, "connection reset by peer"),
    ] {
        console.print(
            &LogRender::new(level, message)
                .time("12:00:00")
                .path("main.rs:42"),
        );
    }

    // Traceback — render an error + its source chain (Rust-native rich.traceback).
    console.print(&Rule::new("traceback"));
    let cause = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file: config.toml");
    let err = std::io::Error::other(cause);
    console.print(&Traceback::new(&err));

    // Bar — a filled span within a range (eighth-block resolution).
    console.print(&Rule::new("bar (range)"));
    for (begin, end) in [(0.0, 100.0), (0.0, 62.0), (20.0, 80.0), (55.0, 100.0)] {
        console.print(&Bar::new(100.0, begin, end).width(48));
    }

    // Markdown — headings, inline styles, lists, block quotes, tables, and rules.
    console.print(&Rule::new("markdown"));
    console.print(&Markdown::new(
        "# Heading\n\nA paragraph with **bold**, *italic*, `code`, and a [link](https://example.com).\n\n- bullet item\n- another\n\n1. first\n2. second\n\n> a block quote\n\n| Name | Age |\n| :--- | ---: |\n| Alice | 30 |\n| Bob | 7 |\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n---",
    ));

    // Syntax highlighting (via syntect — functional, not byte-parity with rich).
    console.print(&Rule::new("syntax"));
    console.print(&Syntax::new(
        "fn main() {\n    let msg = \"hello, rich\";\n    println!(\"{msg}\");\n}",
        "rust",
    ));

    // Control codes — cursor/screen sequences (shown escaped, not executed).
    console.print(&Rule::new("control codes"));
    let show_escape = |label: &str, control: &Control| {
        // Escaped so the sequence is visible (and its `[` isn't read as markup).
        let escaped = control
            .as_str()
            .replace('\x1b', "\\x1b")
            .replace('\x07', "\\a")
            .replace('\r', "\\r");
        console.print(&Text::new(format!("  {label:>14}: {escaped}")));
    };
    show_escape("clear screen", &Control::clear());
    show_escape("home", &Control::home());
    show_escape("move (2,-1)", &Control::move_(2, -1));
    show_escape("move_to (3,4)", &Control::move_to(3, 4));
    show_escape("hide cursor", &Control::show_cursor(false));
    show_escape("bell", &Control::bell());
    // LiveRender — the in-place redraw sequence for a 3-line render.
    let live_render = LiveRender::new(text("a\nb\nc"));
    let _ = console.render_to_string(&live_render); // render once to record the shape
    show_escape("live refresh", &live_render.position_cursor());
    // Live — the full control stream for a two-frame in-place update (captured
    // to a buffer so the demo stays static instead of animating).
    let live_console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(20)
        .build();
    let mut live = Live::new(text("frame one"), live_console, Vec::<u8>::new());
    live.start();
    live.update(text("frame two"));
    live.stop();
    let stream = String::from_utf8_lossy(live.writer())
        .replace('\x1b', "\\x1b")
        .replace('\r', "\\r")
        .replace('\n', "\\n");
    console.print(&Text::new(format!("  {:>14}: {stream}", "live stream")));

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
    // export_html — the inline CSS a style compiles to (via `--export-html`).
    let html_rule = Style::parse("bold red")
        .unwrap()
        .get_html_style(&DEFAULT_TERMINAL_THEME);
    console.print(&Text::new(format!(
        "  export_html: [bold red] → <span style=\"{html_rule}\">"
    )));

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
    // Status — a spinner + message (green frame; animation needs a Live loop).
    let status = Status::new("Loading…").renderable().render(0.0);
    console.print(&Text::new("       status: ").append_text(&status));

    // CSV rendered as a table (blue border, numeric columns bold-green + right).
    console.print(&Rule::new("csv"));
    let csv = "Product,Qty,Price\nWidget,3,9.99\nGadget,12,19.50\nGizmo,1,4.25";
    console.print(&render_csv(&parse_csv(csv, ',')));

    // filesize.
    console.print(&Rule::new("filesize"));
    for bytes in [1u64, 999, 1_000, 1_500, 1_000_000, 1_500_000_000] {
        console.print_str(&format!("  {bytes:>13} → {}", filesize::decimal(bytes)));
    }
    console.print(&Rule::line());
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::ColorSystem;

    #[test]
    fn parse_csv_handles_quotes_and_delimiters() {
        // Quoted field containing the delimiter, and `""` escaping.
        let rows = parse_csv("a,\"b,c\",d\n\"he said \"\"hi\"\"\",2\n", ',');
        assert_eq!(
            rows,
            vec![
                vec!["a".to_string(), "b,c".to_string(), "d".to_string()],
                vec!["he said \"hi\"".to_string(), "2".to_string()],
            ]
        );
        // No trailing empty row after a final newline; `\r\n` endings work.
        assert_eq!(parse_csv("x\r\ny\r\n", ','), vec![vec!["x"], vec!["y"]]);
        // Tab delimiter.
        assert_eq!(parse_csv("a\tb", '\t'), vec![vec!["a", "b"]]);
        // A leading UTF-8 BOM is stripped, not glued to the first cell.
        assert_eq!(parse_csv("\u{feff}a,b", ','), vec![vec!["a", "b"]]);
    }

    #[test]
    fn parses_export_flags() {
        let s = |v: &str| v.to_string();
        let cli = parse(&[s("--export-svg"), s("x")]).unwrap().unwrap();
        assert!(cli.export_svg && !cli.export_html);
        let cli = parse(&[s("--export-html"), s("x")]).unwrap().unwrap();
        assert!(cli.export_html && !cli.export_svg);
        // The two export formats are mutually exclusive.
        assert!(parse(&[s("--export-html"), s("--export-svg"), s("x")]).is_err());
    }

    #[test]
    fn is_number_matches_pattern() {
        for ok in ["0", "42", "-7", "3.14", "-0.5", " 12 "] {
            assert!(is_number(ok), "{ok:?} should be numeric");
        }
        for no in ["", "1.2.3", "1e5", "abc", "5%", "-", "."] {
            assert!(!is_number(no), "{no:?} should not be numeric");
        }
    }

    #[test]
    fn render_csv_matches_upstream() {
        // Byte-parity with the Table real rich-cli's render_csv builds for this
        // CSV (captured from rich 15.0.0): HEAVY_HEAD box, blue border, the
        // numeric Age column right-justified with bold-green body + header cells.
        let table = render_csv(&parse_csv("Name,Age,City\nAlice,30,NYC\nBob,25,LA\n", ','));
        let out = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .no_color(false)
            .build()
            .render_to_string(&table);
        let expected = concat!(
            "\x1b[34m┏━━━━━━━┳━━━━━┳━━━━━━┓\x1b[0m\n",
            "\x1b[34m┃\x1b[0m\x1b[1m \x1b[0m\x1b[1mName \x1b[0m\x1b[1m \x1b[0m\x1b[34m┃\x1b[0m",
            "\x1b[1;32m \x1b[0m\x1b[1;32mAge\x1b[0m\x1b[1;32m \x1b[0m\x1b[34m┃\x1b[0m",
            "\x1b[1m \x1b[0m\x1b[1mCity\x1b[0m\x1b[1m \x1b[0m\x1b[34m┃\x1b[0m\n",
            "\x1b[34m┡━━━━━━━╇━━━━━╇━━━━━━┩\x1b[0m\n",
            "\x1b[34m│\x1b[0m Alice \x1b[34m│\x1b[0m\x1b[1;32m \x1b[0m\x1b[1;32m 30\x1b[0m",
            "\x1b[1;32m \x1b[0m\x1b[34m│\x1b[0m NYC  \x1b[34m│\x1b[0m\n",
            "\x1b[34m│\x1b[0m Bob   \x1b[34m│\x1b[0m\x1b[1;32m \x1b[0m\x1b[1;32m 25\x1b[0m",
            "\x1b[1;32m \x1b[0m\x1b[34m│\x1b[0m LA   \x1b[34m│\x1b[0m\n",
            "\x1b[34m└───────┴─────┴──────┘\x1b[0m",
        );
        assert_eq!(out, expected);
    }
}
