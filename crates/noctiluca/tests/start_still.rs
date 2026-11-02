//! The start page with no motion (reduced motion, no animation, or
//! `NO_COLOR`): the banner and the fish are still from the start. A file
//! of its own: the motion override is process-wide.

use std::path::Path;
use std::time::Duration;

use noctiluca::app::{Options, app};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::Config;
use noctiluca::testing::FakeEngine;

#[test]
fn with_no_motion_the_start_page_is_still_from_the_start() {
    rich_intuituive::a11y::set_motion(Some(false));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let options = Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: None,
        view: None,
        images: Some(noctiluca::config::Images::Ansi),
        tunnel: None,
    };
    let mut driver = app(Box::new(FakeEngine::from_dir(&dir).unwrap()), options).driver(120, 40);
    driver.settle();
    let still = driver.screen().plain();
    let text = still.join("\n");
    // The fish is already where it settles, and the title is lit.
    assert!(text.contains('▀'), "{text}");
    assert!(text.contains("a web browser for the terminal"), "{text}");
    // With no bookmarks, how to add one.
    assert!(
        text.contains("No bookmarks yet: on a page, m keeps it."),
        "{text}"
    );
    let frames = driver.stats().frames;
    driver.advance(Duration::from_secs(1));
    assert_eq!(driver.stats().frames, frames);
    assert_eq!(driver.screen().plain(), still);
}
