//! Render a named demo and write it as a self-contained SVG.
//!
//! This is how every image in the documentation is produced — the docs render
//! through the real library, so a screenshot cannot drift from the behaviour it
//! claims to show. Regenerate them all with `scripts/capture_screenshots.sh`.
//!
//! ```text
//! cargo run -p rs-rich --example screenshot -- table docs/assets/table.svg
//! cargo run -p rs-rich --example screenshot -- --list
//! ```
//!
//! A demo may also emit a numbered *frame* (`--frame N`), which is how the
//! animated images are built: frames are rendered individually and stitched into
//! one animated SVG by `scripts/build_animation.py`.

use rich::markdown::Markdown;
use rich::r#box::{HEAVY_HEAD, ROUNDED};
use rich::{
    Align, ColorSystem, Columns, Console, HorizontalAlign, Json, Justify, Padding, Panel, Pretty,
    Progress, ProgressBar, ProgressColumn, Renderable, Rule, Spinner, Style, Syntax, Table, Text,
    Tree,
};

/// Every demo, with the width it looks best at.
const DEMOS: &[(&str, usize)] = &[
    ("markup", 64),
    ("colour", 64),
    ("table", 64),
    ("table-styled", 68),
    ("panel", 60),
    ("tree", 52),
    ("markdown", 68),
    ("syntax", 68),
    ("json", 52),
    ("columns", 64),
    ("rule", 60),
    ("align", 60),
    ("progress", 60),
    ("spinner", 40),
    ("overflow", 44),
    ("pretty", 52),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--list") {
        for (name, _) in DEMOS {
            println!("{name}");
        }
        return;
    }

    // `screenshot <demo> <out.svg> [--frame N]`
    let (name, out) = match (args.first(), args.get(1)) {
        (Some(n), Some(o)) => (n.clone(), o.clone()),
        _ => {
            eprintln!("usage: screenshot <demo> <out.svg> [--frame N]");
            std::process::exit(2);
        }
    };
    let frame: usize = args
        .iter()
        .position(|a| a == "--frame")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let width = DEMOS
        .iter()
        .find(|(d, _)| *d == name)
        .map(|(_, w)| *w)
        .unwrap_or(64);

    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .no_color(false)
        .build();

    // A fixed id keeps the SVG byte-stable across runs, so regenerating the
    // docs produces no diff unless the rendering actually changed.
    let svg = console.export_svg(&name, "rich-docs", |c| render(c, &name, frame));

    if let Some(parent) = std::path::Path::new(&out).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&out, svg).expect("write svg");
    eprintln!("wrote {out}");
}

fn render(c: &Console, demo: &str, frame: usize) {
    match demo {
        "markup" => {
            c.print_str(
                "[bold]bold[/]  [italic]italic[/]  [underline]underline[/]  [strike]strike[/]",
            );
            c.print_str("[red]red[/]  [green]green[/]  [blue]blue[/]  [#ff8800]#ff8800[/]");
            c.print_str("[white on blue] on blue [/]  [black on #ffcc00] on hex [/]");
            c.print_str("[bold magenta]nested [italic]inside[/] outer[/]");
            c.print_str("Escaped: \\[not a tag]  ·  emoji: :rocket: :sparkles:");
        }
        "colour" => {
            c.print_str("Automatic highlighting, no markup needed:");
            c.print_str("");
            c.print_str("  path   = /usr/local/bin");
            c.print_str("  number = 42, 3.14, 0xff");
            c.print_str("  bool   = True / False / None");
            c.print_str("  url    = https://example.com");
            c.print_str("  uuid   = 123e4567-e89b-12d3-a456-426614174000");
        }
        "table" => {
            let mut t = Table::new().box_set(HEAVY_HEAD).title("Releases");
            t.add_column("Crate");
            t.add_column("Version");
            t.add_column_justify("Downloads", Justify::Right);
            t.add_row(&["rs-rich", "0.0.1", "1,204"]);
            t.add_row(&["rs-rich-cli", "0.0.1", "731"]);
            t.add_row(&["rs-rich-art", "0.0.1", "88"]);
            c.print(&t);
        }
        "table-styled" => {
            let mut t = Table::new()
                .box_set(ROUNDED)
                .border_style(Style::parse("blue").unwrap())
                .title("Test results")
                .caption("10 suites · 0 failures");
            t.add_column("Suite");
            t.add_column_justify("Tests", Justify::Right);
            t.add_column("Status");
            t.add_row(&["golden parity", "10", "pass"]);
            t.add_row(&["rich (lib)", "187", "pass"]);
            t.add_row(&["rich-cli", "21", "pass"]);
            c.print(&t);
        }
        "panel" => {
            let inner = Text::new(
                "Panels wrap any renderable.\nThey take a title, a caption and a border style.",
            );
            let p = Panel::new(Box::new(inner))
                .box_set(ROUNDED)
                .title("Panel")
                .subtitle("rich::Panel")
                .border_style(Style::parse("green").unwrap());
            c.print(&p);
        }
        "tree" => {
            let mut tree = Tree::new("rs-rich-cli");
            let crates = tree.add("crates");
            crates.add("rich");
            crates.add("rich-ext");
            crates.add("rich-cli");
            crates.add("rich-art");
            let docs = tree.add("docs");
            docs.add("BRANCHING.md");
            docs.add("DIVERGENCES.md");
            c.print(&tree);
        }
        "markdown" => {
            let md = "# Markdown\n\nRenders **bold**, *italic* and `code` inline.\n\n- bullet one\n- bullet two\n\n> A block quote.\n";
            c.print(&Markdown::new(md));
        }
        "syntax" => {
            let code = "fn main() {\n    let console = Console::new();\n    console.print_str(\"[bold]Hello[/]\");\n}\n";
            c.print(&Syntax::new(code, "rs"));
        }
        "json" => {
            let raw = r#"{"name":"rs-rich","version":"0.0.1","keywords":["cli","terminal"],"yanked":false}"#;
            match Json::new(raw) {
                Ok(j) => c.print(&j),
                Err(_) => c.print_str("invalid json"),
            }
        }
        "columns" => {
            let items: Vec<String> = [
                "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
                "juliet", "kilo", "lima",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            c.print(&Columns::new(items));
        }
        "rule" => {
            c.print(&Rule::new("Section"));
            c.print_str("");
            c.print(&Rule::new("Left").align(HorizontalAlign::Left));
            c.print_str("");
            c.print(&Rule::line());
        }
        "align" => {
            let boxed = |s: &str| Box::new(Text::new(s)) as Box<dyn Renderable>;
            c.print(&Align::left(boxed("left")));
            c.print(&Align::center(boxed("center")));
            c.print(&Align::right(boxed("right")));
            c.print(&Padding::new(boxed("padded"), (1, 4, 1, 4)));
        }
        "progress" => {
            // `frame` drives the animation: 0..=10 maps to 0..100%.
            let pct = (frame.min(10) as f64) * 10.0;
            let mut p = Progress::new().columns(vec![
                ProgressColumn::Description,
                ProgressColumn::Bar,
                ProgressColumn::Percentage,
            ]);
            p.add_task("Downloading", 100.0, pct);
            p.add_task("Extracting", 100.0, (pct - 30.0).max(0.0));
            p.add_task("Verifying", 100.0, (pct - 60.0).max(0.0));
            c.print(&p);
        }
        "spinner" => {
            let s = Spinner::new("dots");
            let t = s.render(frame as f64 * 0.08);
            c.print(
                &Text::new("  ")
                    .append_text(&t)
                    .append_text(&Text::new("  working…")),
            );
        }
        "overflow" => {
            use rich::Overflow;
            let long = "supercalifragilisticexpialidocious";
            c.print_str("[dim]fold[/]");
            c.print(&Text::new(long).overflow(Overflow::Fold));
            c.print_str("[dim]crop[/]");
            c.print(&Text::new(long).overflow(Overflow::Crop));
            c.print_str("[dim]ellipsis[/]");
            c.print(&Text::new(long).overflow(Overflow::Ellipsis));
        }
        "pretty" => {
            #[derive(Debug)]
            #[allow(dead_code)]
            struct Config<'a> {
                width: usize,
                colour: &'a str,
                tags: Vec<&'a str>,
                truecolor: bool,
            }
            c.print(&Pretty::new(&Config {
                width: 80,
                colour: "truecolor",
                tags: vec!["cli", "terminal"],
                truecolor: true,
            }));
        }
        "progress-bar" => {
            c.print(&ProgressBar::new(100.0, frame as f64 * 10.0).width(40));
        }
        other => {
            c.print_str(&format!("[red]unknown demo:[/] {other}"));
        }
    }
}
