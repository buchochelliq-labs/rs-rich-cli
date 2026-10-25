//! The gaps three or more of round 7's clones shared: panel titles flush
//! with the corner and plain-text titles (git, music, http), a node of
//! its own for `App::min_size` and shorter forms of its message (blocks,
//! disk), focus-scope cycling past hidden panes (http, top), and a tree
//! table (top, tasks).

use intuituive::prelude::*;
use intuituive::Driver;

fn rows(driver: &Driver) -> Vec<String> {
    driver.screen().plain()
}

// Panel titles.

#[test]
fn flush_titles_sit_at_the_corners() {
    let app = App::new(|| {
        label("x")
            .panel("Files")
            .border(Border::Plain)
            .title_flush()
            .title_right("2/9")
            .add_title(Edge::Bottom, Align::Start, "{main}")
            .add_title(Edge::Bottom, Align::End, "ok")
    });
    let mut driver = app.driver(16, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0], "┌Files──────2/9┐");
    assert_eq!(rows(&driver)[2], "└{main}──────ok┘");
}

#[test]
fn a_flush_title_keeps_the_spaces_it_is_given() {
    let app = App::new(|| label("x").panel(" Library ").title_flush());
    let mut driver = app.driver(14, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0], "╭ Library ───╮");
}

#[test]
fn a_flush_title_too_long_is_cut_between_the_corners() {
    let app = App::new(|| label("x").panel("A long title").title_flush());
    let mut driver = app.driver(8, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0], "╭A lon…╮");
}

#[test]
fn a_centred_flush_title_is_centred_on_the_whole_edge() {
    let app = App::new(|| {
        label("x")
            .panel("ab")
            .title_align(Align::Center)
            .title_flush()
    });
    let mut driver = app.driver(8, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0], "╭──ab──╮");
}

#[test]
fn padded_titles_are_unchanged() {
    let app = App::new(|| label("x").panel("Files").title_right("2/9"));
    let mut driver = app.driver(18, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0], "╭─ Files ── 2/9 ─╮");
}

#[test]
fn plain_text_titles_show_their_brackets() {
    let app = App::new(|| {
        let which = signal("Recipe [r]");
        label("x")
            .panel_text_with(move || which.get().to_string())
            .add_title_text(Edge::Top, Align::End, "[ctrl h]")
            .on_key("n", move |_| which.set("History [h]"))
    });
    let mut driver = app.driver(30, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0], "╭─ Recipe [r] ──── [ctrl h] ─╮");
    driver.press("n");
    assert_eq!(rows(&driver)[0], "╭─ History [h] ─── [ctrl h] ─╮");
    let app = App::new(|| label("x").panel_text("[b]bold?[/b]"));
    let mut driver = app.driver(20, 3);
    driver.settle();
    assert!(rows(&driver)[0].contains("[b]bold?[/b]"));
}

#[test]
fn a_flush_title_in_collapsed_borders() {
    let app = App::new(|| {
        row([
            label("a").panel("One").title_flush().flex(1),
            label("b").panel("Two").flex(1),
        ])
        .collapse_borders()
    });
    let mut driver = app.driver(16, 3);
    driver.settle();
    let top = &rows(&driver)[0];
    assert!(top.starts_with("╭One"), "{top}");
    assert!(top.contains("─ Two "), "{top}");
}
