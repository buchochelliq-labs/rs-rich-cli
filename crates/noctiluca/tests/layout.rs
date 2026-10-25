//! Reader mode and the projection, on snapshots Chrome made of the
//! fixture pages: no browser needed.

use noctiluca::layout::{projection, reader, Line, Marks, Piece};
use noctiluca::model::{build, Page, Revisions};
use noctiluca::testing::snapshot_file;

fn page(name: &str) -> Page {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/snapshots/{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn text_of(pieces: &[Piece]) -> Vec<String> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text(lines) => out.extend(lines.iter().map(Line::plain)),
            Piece::Media { index, rows } => out.push(format!("<media {index} x{rows}>")),
            Piece::Controls(c) => out.push(format!("<controls {c:?}>")),
        }
    }
    out
}

#[test]
fn show() {
    for name in ["reader", "docs", "article", "form"] {
        let page = page(name);
        eprintln!("==== {name} reader");
        for l in text_of(&reader(&page, 60, &Marks::default())) {
            eprintln!("|{l}");
        }
        eprintln!("==== {name} projection");
        for l in projection(&page, 100, (8.0, 18.0), &Marks::default()) {
            eprintln!("|{}", l.plain());
        }
    }
}
