//! Screenshots and SVG snapshot tests (#809).

use intuituive::interact::{Event, Key, KeyCode};
use intuituive::prelude::*;
use intuituive::screen::{Rect, Screen};
use intuituive::testing::{check_svg_snapshot, styled};
use rich::{Segment, Style};
use rich_ext::frame::SvgOptions;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "intuituive-screenshots-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn one_cell(style: &str) -> Screen {
    let mut screen = Screen::new(20, 3);
    let style = Style::parse(style).unwrap();
    screen.write_lines(
        Rect::new(3, 1, 1, 1),
        &[vec![Segment::new("X", Some(style))]],
    );
    screen
}

/// Every tag closed, in order, and nothing outside the root: enough of
/// XML's well-formedness to catch a broken export.
fn well_formed(xml: &str) -> Result<(), String> {
    let mut open: Vec<String> = Vec::new();
    let mut rest = xml.trim();
    let mut roots = 0;
    while let Some(at) = rest.find('<') {
        if open.is_empty() && !rest[..at].trim().is_empty() {
            return Err(format!("text outside the root: {:?}", &rest[..at]));
        }
        let end = rest[at..].find('>').ok_or("an unclosed tag")? + at;
        let tag = &rest[at + 1..end];
        if tag.starts_with('?') || tag.starts_with('!') {
        } else if let Some(name) = tag.strip_prefix('/') {
            match open.pop() {
                Some(top) if top == name.trim() => {}
                other => return Err(format!("</{name}> closes {other:?}")),
            }
        } else if !tag.ends_with('/') {
            let name = tag.split_whitespace().next().unwrap_or_default();
            if open.is_empty() {
                roots += 1;
            }
            open.push(name.to_string());
        } else if open.is_empty() {
            roots += 1;
        }
        rest = &rest[end + 1..];
    }
    if !open.is_empty() {
        return Err(format!("left open: {open:?}"));
    }
    if roots != 1 {
        return Err(format!("{roots} roots"));
    }
    Ok(())
}

#[test]
fn to_svg_holds_a_cells_colour_and_weight() {
    let svg = one_cell("bold red").to_svg(&SvgOptions::default().title("demo"));
    well_formed(&svg).unwrap();
    assert!(svg.contains("demo"));
    assert!(svg.contains("font-weight: bold"), "{svg}");
    // The theme's red, as the export writes it.
    let red = rich::terminal_theme::SVG_EXPORT_THEME.ansi[1].hex();
    assert!(svg.contains(&red), "{red} in {svg}");
}

#[test]
fn a_changed_colour_fails_and_writes_the_report() {
    let dir = scratch("changed");
    let before = one_cell("bold red");
    std::fs::write(dir.join("cell.styled.txt"), styled(&before)).unwrap();
    std::fs::write(
        dir.join("cell.svg"),
        before.to_svg(&SvgOptions::default().title("cell")),
    )
    .unwrap();
    check_svg_snapshot(&dir, "cell", &before).unwrap();
    let error = check_svg_snapshot(&dir, "cell", &one_cell("bold blue")).unwrap_err();
    assert!(error.contains("report.html"), "{error}");
    let report = intuituive::testing::report_dir().join("report.html");
    let html = std::fs::read_to_string(report).unwrap();
    let section = html
        .split("<section>")
        .find(|part| part.contains("<h2>cell</h2>"))
        .expect("the test's section");
    assert_eq!(section.matches("<svg").count(), 2, "both pictures inline");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_svg_is_written_when_the_text_matches() {
    let dir = scratch("missing");
    let screen = one_cell("italic green");
    std::fs::write(dir.join("kept.styled.txt"), styled(&screen)).unwrap();
    check_svg_snapshot(&dir, "kept", &screen).unwrap();
    well_formed(&std::fs::read_to_string(dir.join("kept.svg")).unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cx_screenshot_writes_an_svg_the_size_of_the_screen() {
    let dir = scratch("cx");
    let path = dir.join("shot.svg");
    let target = path.clone();
    let app = App::new(move || {
        let target = target.clone();
        label("[bold]Hello[/] screenshot").on_key("s", move |cx| cx.screenshot(target.clone()))
    });
    let mut driver = app.driver(24, 4);
    driver.press("s");
    let svg = std::fs::read_to_string(&path).unwrap();
    well_formed(&svg).unwrap();
    assert!(svg.contains("Hello"));
    // As wide and tall as a 24x4 export of the same screen.
    let expected = driver
        .screen()
        .to_svg(&SvgOptions::default().title("intuiTUIve"));
    let size = |svg: &str| {
        let root = &svg[..svg.find('>').unwrap()];
        root.split_whitespace()
            .filter(|part| part.starts_with("viewBox") || part.starts_with("width"))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    assert_eq!(size(&svg), size(&expected));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_palette_saves_a_screenshot_into_the_screenshot_dir() {
    let dir = scratch("palette");
    let app = App::new(|| label("app"))
        .palette_key("ctrl+p")
        .screenshot_dir(&dir);
    let mut driver = app.driver(40, 12);
    driver.press("ctrl+p");
    driver.type_text("Screenshot");
    driver.send(Event::Key(Key::new(KeyCode::Enter)));
    let shot = dir.join("screenshot-1.svg");
    assert!(
        shot.is_file(),
        "{:?}",
        std::fs::read_dir(&dir).unwrap().collect::<Vec<_>>()
    );
    well_formed(&std::fs::read_to_string(&shot).unwrap()).unwrap();
    let shown = driver.screen().plain().join("\n");
    assert!(shown.contains("Saved"), "{shown}");
    let _ = std::fs::remove_dir_all(&dir);
}
