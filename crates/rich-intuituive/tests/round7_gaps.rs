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

// `App::min_size` with a node of its own, and shorter forms.

#[test]
fn min_size_with_shows_its_node_centred_and_keeps_the_app() {
    let app = App::new(|| {
        let n = signal(0);
        text!("count {n}").on_key("+", move |_| n.update(|n| *n += 1))
    })
    .min_size_with(30, 8, || {
        column([label("Too small!"), label("Make it bigger")])
            .panel("")
            .border(Border::Plain)
    });
    let mut driver = app.driver(30, 8);
    driver.press("+");
    assert_eq!(rows(&driver)[0].trim_end(), "count 1");
    driver.resize(20, 6);
    driver.settle();
    let shown = rows(&driver);
    assert_eq!(shown[1], "  ┌──────────────┐  ");
    assert_eq!(shown[2], "  │Too small!    │  ");
    // Keys go nowhere meanwhile, and the app comes back as it was.
    driver.press("+");
    driver.resize(30, 8);
    driver.settle();
    assert_eq!(rows(&driver)[0].trim_end(), "count 1");
    // `q` quits from the small screen.
    driver.resize(20, 6);
    driver.press("q");
    assert!(driver.is_done());
}

#[test]
fn min_size_forms_shows_the_longest_that_fits() {
    let mut driver = App::new(|| label("map"))
        .min_size_forms(
            60,
            10,
            ["Terminal window is too small", "Window too small", "!!!"],
        )
        .driver(40, 3);
    driver.settle();
    assert_eq!(rows(&driver)[1].trim(), "Terminal window is too small");
    driver.resize(20, 3);
    driver.settle();
    assert_eq!(rows(&driver)[1].trim(), "Window too small");
    driver.resize(8, 3);
    driver.settle();
    assert_eq!(rows(&driver)[1].trim(), "!!!");
}

// Focus-scope cycling past hidden panes.

/// Three panes in a row; `f` shows the focused one alone. The middle one
/// can be revealed only while `open`.
fn panes() -> App {
    App::new(|| {
        let (full, at, open) = (signal(false), signal(0usize), signal(true));
        let pane = move |n: usize, name: &'static str| {
            let when = move || n != 1 || open.get();
            label(name)
                .focusable()
                .name(name)
                .on_focus(move |_| at.set(n))
                .focus_scope_reveal(when, move |_| at.set(n))
                .shown_when(move || (!full.get() || at.get() == n) && (n != 1 || open.get()))
                .flex(1)
        };
        row([pane(0, "a"), pane(1, "b"), pane(2, "c")])
            .on_key("tab", |cx| cx.focus_next_scope())
            .on_key("shift+tab", |cx| cx.focus_previous_scope())
            .on_key("f", move |_| full.update(|f| *f = !*f))
            .on_key("o", move |_| open.update(|o| *o = !*o))
    })
}

#[test]
fn scope_cycling_shows_a_hidden_pane_that_reveals_itself() {
    let mut driver = panes().driver(9, 1);
    driver.settle();
    driver.assert_focused("a");
    driver.press("f");
    assert_eq!(rows(&driver)[0].trim_end(), "a");
    driver.press("tab");
    driver.assert_focused("b");
    assert_eq!(rows(&driver)[0].trim_end(), "b");
    driver.press("tab tab");
    driver.assert_focused("a");
    driver.press("shift+tab");
    driver.assert_focused("c");
    assert_eq!(rows(&driver)[0].trim_end(), "c");
}

#[test]
fn a_hidden_pane_that_cannot_be_revealed_is_passed_by() {
    let mut driver = panes().driver(9, 1);
    driver.press("o f tab");
    driver.assert_focused("c");
    driver.press("tab");
    driver.assert_focused("a");
    // Shown all together, the closed one is not there to visit either.
    driver.press("f tab");
    driver.assert_focused("c");
}
