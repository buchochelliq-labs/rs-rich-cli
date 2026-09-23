//! Guide: Banners — run: cargo run -p rs-rich-art --example guide_banners [-- --svg docs/media/guide]
//!
//! Every snippet on the `docs/guide/art/banners.md` page comes from this file,
//! and every screenshot on it is this file's own SVG export. FIGlet banners
//! need no Cargo features.

use std::path::PathBuf;

use rich::r#box::ROUNDED;
use rich::{ColorSystem, Console, Panel, Rule, Style};
use rich_art::{Figlet, FigletFont, Justify};

fn quickstart(console: &Console) {
    // --8<-- [start:quickstart]
    console.print(&Figlet::new("Hello"));
    // --8<-- [end:quickstart]
}

fn styled(console: &Console) {
    // --8<-- [start:styled]
    let style = Style::parse("bold magenta").expect("a valid style");
    console.print(&Figlet::new("rich-art").style(style));
    // --8<-- [end:styled]
}

fn justify(console: &Console) {
    // --8<-- [start:justify]
    for (label, justify) in [
        ("Left", Justify::Left),
        ("Center", Justify::Center),
        ("Right", Justify::Right),
    ] {
        console.print(&Figlet::new(label).justify(justify));
    }
    // --8<-- [end:justify]
}

fn wrap(console: &Console) {
    // --8<-- [start:wrap]
    // A banner lays out to the console width (or `.width(n)`), and wraps
    // onto further banner rows when a line would not fit, as figlet does.
    console.print(&Figlet::new("wraps at forty").width(40));
    // --8<-- [end:wrap]
}

fn panel(console: &Console) {
    // --8<-- [start:panel]
    // A banner is an ordinary renderable, so it composes with everything else.
    let banner = Figlet::new("Ship it").style(Style::parse("cyan").expect("a valid style"));
    let panel = Panel::new(Box::new(banner))
        .box_set(ROUNDED)
        .title("release")
        .subtitle("rs-rich-art")
        .border_style(Style::parse("green").expect("a valid style"));
    console.print(&panel);
    // --8<-- [end:panel]
}

fn plain_text(console: &Console) {
    // --8<-- [start:to-text]
    // `to_text` returns exactly what figlet(1) prints — no console, no styling.
    let text: String = Figlet::new("Hi").to_text(80);
    assert!(text.starts_with(" _   _ _ \n"));
    // --8<-- [end:to-text]

    // --8<-- [start:font]
    // Fonts are FIGfont (`.flf`) files. The bundled `standard` font is the
    // default; parse any other with `FigletFont::parse`, e.g.
    // `FigletFont::parse(&std::fs::read_to_string("slant.flf")?)?`.
    let font = FigletFont::parse(rich_art::figlet::STANDARD_FONT).expect("a valid FIGfont");
    console.print_str(&format!(
        "standard font: {} rows per character",
        font.height()
    ));
    console.print(&Figlet::new("Hi").font(font));
    // --8<-- [end:font]
}

type Shot = (&'static str, &'static str, usize, fn(&Console));

const SHOTS: &[Shot] = &[
    ("quickstart", "Figlet::new", 40, quickstart),
    ("styled", "Styled banner", 50, styled),
    ("justify", "Justification", 60, justify),
    ("wrap", "Wrapping", 40, wrap),
    ("panel", "In a panel", 50, panel),
    ("font", "Fonts", 50, plain_text),
];

fn main() {
    let svg_dir = svg_dir();
    let console = Console::builder().build();
    for (name, title, width, draw) in SHOTS {
        console.print(&Rule::new(*title));
        draw(&console);
        if let Some(dir) = &svg_dir {
            export(dir, name, title, *width, *draw);
        }
    }
}

/// `--svg DIR` from the command line, if given.
fn svg_dir() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let index = args.iter().position(|a| a == "--svg")?;
    Some(PathBuf::from(
        args.get(index + 1).expect("--svg needs a DIR"),
    ))
}

/// Render one shot on a fixed-width truecolor console and write it as
/// `DIR/guide_banners-<name>.svg`.
fn export(dir: &std::path::Path, name: &str, title: &str, width: usize, draw: fn(&Console)) {
    let stem = format!("guide_banners-{name}");
    let console = Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .build();
    let svg = console.export_svg(title, &stem, draw);
    std::fs::create_dir_all(dir).expect("create the SVG directory");
    let path = dir.join(format!("{stem}.svg"));
    std::fs::write(&path, svg).expect("write the SVG");
    eprintln!("wrote {}", path.display());
}
