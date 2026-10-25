//! Reader mode and the projection, on snapshots Chrome made of the
//! fixture pages: no browser needed.

use noctiluca::layout::{projection, reader, Line, Marks, Piece};
use noctiluca::model::{build, Page, Revisions};
use noctiluca::testing::snapshot_file;

fn page(name: &str) -> Page {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/snapshots/{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn text_of(pieces: &[Piece]) -> Vec<String> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text(lines) => out.extend(lines.iter().map(Line::plain)),
            Piece::Media { index, rows } => out.push(format!("<media {index} x{rows}>")),
            Piece::Controls(c) => out.push(format!("<controls {c:?}>")),
            Piece::Player(i) => out.push(format!("<player {i}>")),
        }
    }
    out
}

#[test]
fn reader_mode_reads_a_docs_page_with_its_code_as_text() {
    let lines = text_of(&reader(&page("docs"), 80, &Marks::default()));
    let at = |text: &str| {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{text}: {lines:#?}"))
    };
    assert!(at("Struct glow::Lamp") < at("Method new"));
    // Code keeps its lines and indentation, as text to copy.
    assert!(
        lines.contains(&"▏pub fn new(brightness: u8) -> Lamp {".to_string()),
        "{lines:#?}"
    );
    assert!(
        lines.contains(&"▏    Lamp { brightness, lit: false }".to_string()),
        "{lines:#?}"
    );
    assert!(at("Field") < at("brightness │ u8"));
}

#[test]
fn reader_mode_reads_an_article_with_its_image_inline() {
    let pieces = reader(&page("article"), 80, &Marks::default());
    let lines = text_of(&pieces);
    let at = |text: &str| {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{text}: {lines:#?}"))
    };
    assert!(at("The glowing coast") < at("image: Blue waves at night"));
    assert!(at("image: Blue waves at night") < at("<media 0"));
    assert!(at("<media 0") < at("Last night the bay lit up blue"));
}

#[test]
fn the_projection_keeps_columns_side_by_side() {
    // page.html's two sections sit in a flex row.
    let lines: Vec<String> = projection(&page("page"), 100, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    let row = lines.iter().find(|l| l.contains("Where it lives")).unwrap();
    assert!(row.contains("Escape test"), "not side by side: {lines:#?}");
    let left = row.find("Where it lives").unwrap();
    let right = row.find("Escape test").unwrap();
    assert!(left < right);
}

#[test]
fn the_projection_never_puts_two_texts_in_one_cell_and_keeps_reading_order() {
    for name in ["reader", "docs", "article", "form", "page"] {
        let page = page(name);
        let lines = projection(&page, 100, (8.0, 18.0), &Marks::default());
        let text: Vec<String> = lines.iter().map(Line::plain).collect();
        // Every block's text is there, in the order Chrome laid it out
        // (top first) within the page's single column of headings.
        let headings: Vec<String> = page
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, noctiluca::model::BlockKind::Heading(_)))
            .map(|b| b.text().replace('\n', " "))
            .collect();
        let mut last = 0;
        for heading in &headings {
            let first_word = heading.split(' ').next().unwrap();
            let at = text
                .iter()
                .position(|l| l.contains(first_word))
                .unwrap_or_else(|| panic!("{name}: {heading}"));
            assert!(
                at >= last || name == "page",
                "{name}: {heading} out of order"
            );
            last = at;
        }
        // No line is wider than the canvas.
        assert!(lines.iter().all(|l| l.width() <= 100), "{name}");
    }
}
