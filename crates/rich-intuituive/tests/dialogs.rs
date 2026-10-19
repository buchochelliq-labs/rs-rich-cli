//! Round 3 of the demo gaps, pop-ups: a confirm sized and styled per call
//! with no empty body row, an input prompt, a pick menu, a rule joining a
//! padded panel, a pop-up below a row kept under it, and a notification
//! as wide as its line.

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::{
    Caret, ConfirmOptions, Corner, Level, Offset, Origin, PickOptions, PromptOptions,
};

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver.screen().plain()
}

fn row_of(rows: &[String], text: &str) -> usize {
    rows.iter()
        .position(|r| r.contains(text))
        .unwrap_or_else(|| panic!("no {text:?} in {rows:#?}"))
}

#[test]
fn a_confirm_with_no_body_lists_its_items_under_the_title() {
    let app = App::new(|| {
        label("").on_key("d", |cx| {
            let items = vec!["a.txt".to_string(), "b.txt".into()];
            cx.confirm("Trash 2 files?", "", items, |_| {})
        })
    });
    let mut driver = app.driver(60, 12);
    driver.press("d");
    let shown = rows(&driver);
    let top = row_of(&shown, "Trash 2 files?");
    // No empty row, no rule, between the title and the items.
    assert!(shown[top + 1].contains("│ a.txt"), "{shown:#?}");
    assert!(shown[top + 2].contains("│ b.txt"), "{shown:#?}");
    assert!(shown[top + 3].contains("├───"), "{shown:#?}");
    assert!(shown[top + 4].contains("[Y]es"), "{shown:#?}");
    // With neither, only the answers.
    let app = App::new(|| label("").on_key("d", |cx| cx.confirm("Quit?", "", vec![], |_| {})));
    let mut driver = app.driver(60, 12);
    driver.press("d");
    let shown = rows(&driver);
    let top = row_of(&shown, "Quit?");
    assert!(shown[top + 1].contains("[Y]es"), "{shown:#?}");
}

#[test]
fn a_confirm_takes_a_size_and_a_title_style() {
    let app = App::new(|| {
        label("").on_key("d", |cx| {
            let look = ConfirmOptions::new()
                .size(Size::Fixed(70), Size::Fixed(20))
                .title_style("bold magenta");
            let gone = signal(false);
            cx.confirm_with("Trash 1 file?", "", vec!["a.txt".into()], look, move |_| {
                gone.set(true)
            })
        })
    })
    .theme(Theme::dark().style(
        "popup.title",
        intuituive::rich::Style::parse("blue").unwrap(),
    ));
    let mut driver = app.driver(80, 24);
    driver.press("d");
    let shown = rows(&driver);
    let top = row_of(&shown, "Trash 1 file?");
    assert_eq!(top, 2);
    assert!(shown[top].starts_with("     ╭"), "{shown:#?}");
    // The answers at the bottom of the 20 rows.
    assert!(shown[top + 18].contains("[Y]es"), "{shown:#?}");
    assert!(shown[top + 17].contains("├───"), "{shown:#?}");
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.contains("magenta") && styled.contains("Trash 1 file?"),
        "{styled}"
    );
    let title = styled.lines().nth(top).unwrap();
    assert!(
        title.contains("magenta") && !title.contains("blue"),
        "{title}"
    );
}

/// An app whose `r` opens a prompt with `options`; what it shows.
fn prompting(options: fn() -> PromptOptions) -> App {
    App::new(move || {
        let name = signal(String::from("notes.txt"));
        let said = signal(String::from("-"));
        column([text!("{name}").fixed(1), text!("{said}").fixed(1)]).on_key("r", move |cx| {
            let options = options()
                .on_change(move |text, _| said.set(format!("typed {text}")))
                .on_cancel(move |_| said.set("cancelled".into()));
            cx.prompt_with("Rename:", name.get_untracked(), options, move |text, _| {
                name.set(text)
            })
        })
    })
}

#[test]
fn a_prompt_edits_submits_and_cancels() {
    let mut driver = prompting(PromptOptions::new).driver(60, 10);
    driver.press("r");
    let shown = rows(&driver);
    // Yazi's place: the top centre, two rows down, 50 wide.
    assert!(shown[2].starts_with("     ╭─ Rename: ─"), "{shown:#?}");
    assert!(shown[3].contains("│notes.txt"), "{shown:#?}");
    driver.press("x");
    assert_eq!(rows(&driver)[1].trim_end(), "typed notes.txtx");
    driver.press("enter");
    assert_eq!(rows(&driver)[0].trim_end(), "notes.txtx");
    // Esc cancels: the name stays, the cancel handler runs.
    driver.press("r y esc");
    assert_eq!(rows(&driver)[0].trim_end(), "notes.txtx");
    assert_eq!(rows(&driver)[1].trim_end(), "cancelled");
    assert!(!rows(&driver)[2].contains("Rename"));
}

#[test]
fn a_prompt_places_its_caret_and_box_as_asked() {
    let options = || {
        PromptOptions::new()
            .at(
                Origin::At(Rect::new(10, 4, 20, 1)),
                Offset::new(0, 0, 30, 3),
            )
            .caret(Caret::At(5))
            .title_style("default")
    };
    let mut driver = prompting(options).driver(60, 12);
    driver.press("r");
    let shown = rows(&driver);
    assert!(shown[5][10..].starts_with("╭─ Rename: ─"), "{shown:#?}");
    // The caret before the extension: a letter goes there.
    driver.press("X enter");
    assert_eq!(rows(&driver)[0].trim_end(), "notesX.txt");
}

#[test]
fn a_pick_menu_picks_with_enter_or_a_click_and_closes_with_esc() {
    let app = App::new(|| {
        let chosen = signal(String::from("-"));
        text!("{chosen}").on_key("o", move |cx| {
            let items = vec!["vim".to_string(), "less".into(), "xdg-open".into()];
            let names = items.clone();
            let options = PickOptions::new().title_style("bold");
            let at = Origin::At(Rect::new(4, 1, 10, 1));
            cx.pick_with("Open with:", at, items, options, move |i, _| {
                chosen.set(names[i].clone())
            })
        })
    });
    let mut driver = app.driver(30, 8);
    driver.press("o");
    let shown = rows(&driver);
    // Below the row, as wide as the title needs.
    assert!(shown[2][4..].starts_with("╭─ Open with: ─╮"), "{shown:#?}");
    assert!(shown[3][4..].starts_with("│vim"), "{shown:#?}");
    assert!(shown[6][4..].starts_with("╰"), "{shown:#?}");
    driver.press("esc");
    assert_eq!(rows(&driver)[0].trim_end(), "-");
    assert!(!rows(&driver)[2].contains("Open with"));
    driver.press("o down enter");
    assert_eq!(rows(&driver)[0].trim_end(), "less");
    driver.press("o");
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        6,
        5,
    )));
    driver.settle();
    driver.event(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 6, 5)));
    driver.settle();
    assert_eq!(rows(&driver)[0].trim_end(), "xdg-open");
}

#[test]
fn a_rule_in_a_padded_help_box_joins_its_border() {
    // Yazi's help: the column padded, not each part.
    let app = App::new(|| {
        column([label("filter"), intuituive::rule(), label("q quit")])
            .padding(0, 1)
            .panel("help")
    });
    let mut driver = app.driver(14, 5);
    driver.settle();
    assert_eq!(rows(&driver)[2], "├────────────┤");
}

#[test]
fn a_pop_up_below_a_row_too_wide_for_the_room_stays_under_it() {
    // Yazi's rename box at 80 columns: 80 wide, below a row at x = 10.
    let app = App::new(|| {
        label("files").on_key("r", |cx| {
            let row = Rect::new(10, 3, 32, 1);
            cx.popup_at(Origin::At(row), Offset::new(0, 1, 80, 3), || {
                label("name").panel("Rename:")
            })
        })
    });
    let mut driver = app.driver(80, 12);
    driver.press("r");
    let shown = rows(&driver);
    // Under the row, from its left edge to the screen's right.
    assert!(shown[5][10..].starts_with("╭─ Rename: "), "{shown:#?}");
    assert!(shown[5].ends_with('╮'), "{shown:#?}");
    // A box the room cannot hold even at the row's width moves left.
    let app = App::new(|| {
        label("files").on_key("r", |cx| {
            let row = Rect::new(70, 3, 20, 1);
            cx.popup_at(Origin::At(row), Offset::new(0, 0, 30, 3), || {
                label("name").panel("Rename:")
            })
        })
    });
    let mut driver = app.driver(80, 12);
    driver.press("r");
    let shown = rows(&driver);
    assert!(shown[4][50..].starts_with("╭─ Rename: "), "{shown:#?}");
}

#[test]
fn a_notification_grows_to_its_line_within_the_width_asked() {
    let error = "Failed to rename notes.txt: Permission denied (os error 13)";
    let app = |width: Option<Size>| {
        let app = App::new(move || {
            label("").on_key("e", move |cx| cx.notify(Level::Error, "Rename", error))
        })
        .toasts_at(Corner::TopRight);
        match width {
            Some(width) => app.notify_width(width),
            None => app,
        }
    };
    // 48 cells by default: the line wraps.
    let mut driver = app(None).driver(80, 8);
    driver.press("e");
    assert!(!rows(&driver)[1].contains(error));
    assert!(rows(&driver)[3].contains("╰"));
    // Wider allowed: one line, the box as wide as it.
    for width in [Size::Fixed(70), Size::Percent(90)] {
        let mut driver = app(Some(width)).driver(80, 8);
        driver.press("e");
        let shown = rows(&driver);
        assert!(shown[1].contains(error), "{shown:#?}");
        assert!(shown[2].contains("╰"), "{shown:#?}");
        assert_eq!(shown[1].trim().chars().count(), error.chars().count() + 4);
    }
}
