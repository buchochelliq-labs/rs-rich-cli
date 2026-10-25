//! A local page read through a real browser. Runs only when
//! `RICH_EMBED_CHROME` names a Chrome or Chromium binary, as
//! rs-rich-embed's own browser tests do; without it the test says it
//! skipped and passes. The browser keeps its sandbox on, so it must be able
//! to start one (as root it cannot).

use std::path::Path;
use std::time::Duration;

use noctiluca::{file_url, render, Block, BlockKind, Reader};

#[test]
fn a_local_page_reads_as_headings_and_paragraphs() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/page.html");
    let url = file_url(&page).unwrap();
    let mut reader = Reader::new(Some(binary.into()), 100, 40).unwrap();
    reader.load(&url, Duration::from_secs(60)).unwrap();
    let blocks = reader.blocks().unwrap();
    let block = |kind, text: &str| Block {
        kind,
        text: text.to_string(),
    };
    assert_eq!(
        blocks,
        [
            block(BlockKind::Heading(1), "Sea sparkle"),
            block(
                BlockKind::Paragraph,
                "Noctiluca makes the waves glow at night."
            ),
            block(BlockKind::Heading(2), "Where it lives"),
            block(BlockKind::Paragraph, "Coastal water, all over the world."),
            block(BlockKind::Heading(3), "Escape test"),
            // The ESC is gone; what followed it is plain text.
            block(BlockKind::Paragraph, "Before[31mafter"),
            // The page's own script ran.
            block(BlockKind::Paragraph, "Written by script."),
        ]
    );
    assert!(!render(&blocks).plain().contains('\x1b'));
}
