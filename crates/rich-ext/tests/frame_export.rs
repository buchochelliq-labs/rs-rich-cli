//! Frame export (#226): HTML and SVG from a frame match core's exporters when
//! there are no regions, regions never change the terminal bytes, and a
//! document with regions and links exports as the checked-in fixtures.
//! Regenerate the fixtures with
//! `UPDATE_SNAPSHOTS=1 cargo test -p rs-rich-ext --test frame_export`.

use rich::markdown::Markdown;
use rich::panel::Panel;
use rich::terminal_theme::{DEFAULT_TERMINAL_THEME, SVG_EXPORT_THEME};
use rich::{ColorSystem, Console, Renderable, Rule, Segment, Table, Text, Tree};
use rich_ext::frame::{render_frame, role_name, Frame, HtmlOptions, SvgOptions};

fn console(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .highlight(false)
        .build()
}

fn table() -> Table {
    let mut table = Table::new().title("Crates").caption("two rows");
    table.add_column("Name");
    table.add_column("Notes");
    table.add_row(&[
        "rs-rich",
        "[bold]core[/] [link=https://docs.rs/rs-rich]docs[/link]",
    ]);
    table.add_row(&["rs-rich-ext", "frames\nand regions"]);
    table
}

fn renderables() -> Vec<(&'static str, Box<dyn Renderable>)> {
    let mut tree = Tree::new("root");
    tree.add("src").add("main.rs");
    vec![
        ("table", Box::new(table())),
        (
            "panel",
            Box::new(Panel::new(Box::new(table())).title("Box")),
        ),
        ("tree", Box::new(tree)),
        ("rule", Box::new(Rule::new("[bold]Section[/]"))),
        (
            "markdown",
            Box::new(Markdown::new(
                "# Title\n\nSome *emphasis* and a [link](https://x.test).\n\n\
                 ## Install\n\n```sh\ncargo add rs-rich\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
            )),
        ),
        (
            "text",
            Box::new(Text::from_markup("[on red]hot[/] and [u]cold[/]").unwrap()),
        ),
    ]
}

/// The segments `Console::print` would write: the render and a line break.
fn printed(console: &Console, renderable: &dyn Renderable) -> Vec<Segment> {
    let mut segments = renderable.rich_render(console, &console.options());
    segments.push(Segment::line());
    segments
}

fn frame_with_regions(console: &Console, renderable: &dyn Renderable) -> Frame {
    render_frame(console, &console.options(), renderable)
}

#[test]
fn regions_leave_the_bytes_alone() {
    let console = console(48);
    for (name, renderable) in renderables() {
        let plain = renderable.rich_render(&console, &console.options());
        let frame = frame_with_regions(&console, renderable.as_ref());
        assert_eq!(
            frame.to_ansi(&console),
            console.segments_to_string(&plain),
            "{name}"
        );
        let without = HtmlOptions {
            regions: false,
            ..HtmlOptions::default()
        };
        assert_eq!(
            frame.to_html(&without),
            Frame::from_segments(&plain).to_html(&without),
            "{name}"
        );
    }
}

#[test]
fn without_regions_export_matches_core() {
    let console = console(48);
    for (name, renderable) in renderables() {
        let segments = printed(&console, renderable.as_ref());
        let frame = Frame::from_segments(&segments);
        assert_eq!(
            frame.to_html(&HtmlOptions::default()),
            rich::export::export_html_inline(&segments, &DEFAULT_TERMINAL_THEME),
            "{name}: inline HTML"
        );
        let classes = HtmlOptions {
            inline_styles: false,
            ..HtmlOptions::default()
        };
        let core = rich::export::export_html_classes(&segments, &DEFAULT_TERMINAL_THEME);
        // Core writes a link's URL as it is; a frame escapes it for the
        // attribute. These URLs need no escaping, so the two agree.
        assert_eq!(frame.to_html(&classes), core, "{name}: class HTML");
        let options = SvgOptions {
            title: name,
            unique_id: "t",
            width: Some(48),
            ..SvgOptions::default()
        };
        let svg = frame.to_svg(&options);
        let core = rich::svg::export_svg(&segments, &SVG_EXPORT_THEME, name, "t", 48);
        // Core drops links from SVG; a frame wraps linked text in `<a>`.
        let unlinked = strip_svg_links(&svg);
        assert_eq!(unlinked, core, "{name}: SVG");
    }
}

/// `svg` with each `<a href="…">…</a>` replaced by its content.
fn strip_svg_links(svg: &str) -> String {
    let mut out = String::new();
    let mut rest = svg;
    while let Some(start) = rest.find("<a href=\"") {
        out.push_str(&rest[..start]);
        let open_end = rest[start..].find('>').expect("a closed tag") + start + 1;
        let close = rest[open_end..].find("</a>").expect("a closing a") + open_end;
        out.push_str(&rest[open_end..close]);
        rest = &rest[close + 4..];
    }
    out.push_str(rest);
    out
}

/// A document with regions and links, exported and compared with fixtures.
#[test]
fn export_fixtures() {
    let console = console(48);
    let document = Panel::new(Box::new(Markdown::new(
        "# Frames\n\nA [link](https://github.com/buchochelliq-labs/rs-rich-cli).\n\n\
         | Crate | Version |\n|---|---|\n| rs-rich | 0.0.9 |\n\n```rust\nlet frame = render_frame(..);\n```\n",
    )))
    .title("Export");
    let frame = render_frame(&console, &console.options(), &document);
    let regions: Vec<String> = frame
        .regions()
        .iter()
        .map(|region| {
            let bounds = region.bounds().expect("spans");
            format!(
                "{}{} {:?} label={:?} link={:?} rows {}..{} columns {}..{}{}",
                "  ".repeat(region.depth),
                role_name(&region.role),
                region.role,
                region.label,
                region.link,
                bounds.row,
                bounds.row + bounds.height,
                bounds.column,
                bounds.column + bounds.width,
                if region.is_rectangle() {
                    " rectangle"
                } else {
                    ""
                }
            )
        })
        .collect();
    let html = frame.to_html(&HtmlOptions::default());
    let svg = frame.to_svg(&SvgOptions {
        title: "Export",
        unique_id: "fixture",
        ..SvgOptions::default()
    });
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frame_export");
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    for (file, got) in [
        ("regions.txt", regions.join("\n") + "\n"),
        ("document.html", html),
        ("document.svg", svg),
    ] {
        let path = dir.join(file);
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run with UPDATE_SNAPSHOTS=1", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            got, expected,
            "{file} changed; run with UPDATE_SNAPSHOTS=1 if intended"
        );
    }
}
