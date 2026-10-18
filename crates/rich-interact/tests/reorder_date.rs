//! Reorder (#720) and the date and time pickers (#719), headless.

use rich_interact::datetime::{Date, DateTime, Time};
use rich_interact::headless::{self, Script};
use rich_interact::policy::{Fallback, Reason, ScriptedLineIo};
use rich_interact::{
    degrade, Button, DatePicker, DateTimePicker, Event, Item, Key, MouseKind, NotInteractive,
    Outcome, Reorder, TimePicker,
};

fn before_answer(record: &headless::Record) -> &str {
    &record.frames[record.frames.len() - 2]
}

fn letters() -> Reorder<&'static str> {
    Reorder::new("Order", ["a", "b", "c", "d"])
}

// ---- Reorder: keys ----

#[test]
fn alt_arrows_move_the_focused_item() {
    let script = Script::new().keys("down alt+down alt+down enter");
    let (outcome, record) = headless::run(letters(), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "c", "d", "b"]));
    assert_eq!(record.last_frame(), "? Order › a, c, d, b");
}

#[test]
fn ctrl_arrows_and_shifted_letters_move_too() {
    // Ctrl+↑/↓ and K/J are the same moves, for terminals without Alt on
    // the arrows.
    let script = Script::new().keys("end ctrl+up K enter");
    let (outcome, _) = headless::run(letters(), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "d", "b", "c"]));
    let script = Script::new().keys("J J enter");
    let (outcome, _) = headless::run(letters(), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["b", "c", "a", "d"]));
}

#[test]
fn kitty_keys_move_as_legacy_ones_do() {
    // A terminal with the kitty keyboard protocol reports exact keys.
    let alt_down = Key::parse("alt+down").unwrap().exact();
    let script = Script::new()
        .event(Event::Key(alt_down))
        .event(Event::Key(Key::parse("enter").unwrap().exact()));
    let (outcome, _) = headless::run(letters(), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["b", "a", "c", "d"]));
}

#[test]
fn moves_stop_at_the_ends() {
    // Up from the first and down from the last change nothing.
    let script = Script::new().keys("alt+up end alt+down enter");
    let (outcome, _) = headless::run(letters(), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "b", "c", "d"]));
    let script = Script::new().keys("alt+end end alt+home enter");
    let (outcome, _) = headless::run(letters(), script, 40, 10);
    // a goes to the bottom, then the last (a) back to the top.
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "b", "c", "d"]));
    let script = Script::new().keys("down alt+end enter");
    let (outcome, _) = headless::run(letters(), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "c", "d", "b"]));
}

#[test]
fn space_grabs_and_the_arrows_carry() {
    let script = Script::new().keys("space down down");
    let script = script.keys("space up enter");
    let (outcome, record) = headless::run(letters(), script, 50, 10);
    // a was carried to third place, dropped, and the focus moved up to c.
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["b", "c", "a", "d"]));
    let carrying = record
        .frames
        .iter()
        .find(|frame| frame.contains("moving a"))
        .expect("a frame while carrying");
    // The carried item has a mark of its own, readable without colour.
    assert!(carrying.contains("↕ a"), "{carrying}");
}

#[test]
fn escape_puts_a_grabbed_item_back_then_cancels() {
    let script = Script::new().keys("space down down escape enter");
    let (outcome, _) = headless::run(letters(), script, 50, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "b", "c", "d"]));
    let script = Script::new().keys("alt+down escape");
    let (outcome, record) = headless::run(letters(), script, 50, 10);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert_eq!(record.last_frame(), "? Order › cancelled");
}

#[test]
fn ctrl_r_restores_the_first_order() {
    let script = Script::new().keys("alt+down alt+down ctrl+r enter");
    let (outcome, _) = headless::run(letters(), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "b", "c", "d"]));
}

#[test]
fn one_item_and_no_items() {
    let script = Script::new().keys("alt+down alt+up space down enter");
    let (outcome, _) = headless::run(Reorder::new("One", ["only"]), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["only"]));
    let script = Script::new().keys("alt+down space enter");
    let (outcome, record) = headless::run(
        Reorder::<String>::new("None", Vec::<String>::new()),
        script,
        40,
        6,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(Vec::new()));
    assert!(before_answer(&record).contains("nothing to order"));
}

#[test]
fn a_long_list_scrolls_with_the_carried_item() {
    let items: Vec<String> = (1..=30).map(|n| format!("item {n}")).collect();
    let reorder = Reorder::new("Long", items).height(5);
    // Carry the first item down 12 places: the list scrolls to follow.
    let script = Script::new().keys(&"alt+down ".repeat(12)).keys("enter");
    let (outcome, record) = headless::run(reorder, script, 40, 12);
    let order = outcome.unwrap().value().unwrap();
    assert_eq!(order[12], "item 1");
    assert_eq!(order[0], "item 2");
    let frame = before_answer(&record);
    assert!(
        frame.contains("↕ item 1") || frame.contains("❯ item 1"),
        "{frame}"
    );
    assert!(!frame.contains("item 2\n"), "{frame}");
    assert!(frame.contains("13/30"), "{frame}");
}

#[test]
fn order_reports_the_first_indices() {
    let mut reorder = Reorder::new("Order", [Item::new(10, "ten"), Item::new(20, "twenty")]);
    let script = Script::new().keys("alt+down enter");
    let (outcome, _) = headless::run(&mut reorder, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec![20, 10]));
    assert_eq!(reorder.order(), [1, 0]);
}

#[test]
fn rebinding_the_move_keys() {
    let reorder = letters()
        .rebind("move-down", [Key::char('+')])
        .rebind("move-up", Vec::new());
    let script = Script::new().keys("+ alt+down enter");
    let (outcome, _) = headless::run(reorder, script, 40, 10);
    // `+` moved a down once; alt+down does nothing now... except focus?
    // No: unbound keys are ignored.
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["b", "a", "c", "d"]));
}

// ---- Reorder: the mouse ----

#[test]
fn dragging_a_row_carries_it() {
    // Rows: 0 the question, 1 to 4 the items.
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 3, 1)
        .mouse(MouseKind::Drag(Button::Left), 3, 2)
        .mouse(MouseKind::Drag(Button::Left), 3, 3)
        .mouse(MouseKind::Up(Button::Left), 3, 3)
        .keys("enter");
    let (outcome, _) = headless::run(letters().with_mouse(true), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["b", "c", "a", "d"]));
}

#[test]
fn dragging_up_from_the_last_row_and_past_the_top() {
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 3, 4)
        .mouse(MouseKind::Drag(Button::Left), 3, 2)
        .mouse(MouseKind::Drag(Button::Left), 3, 0)
        .mouse(MouseKind::Up(Button::Left), 3, 0)
        .keys("enter");
    let (outcome, _) = headless::run(letters().with_mouse(true), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["d", "a", "b", "c"]));
}

#[test]
fn a_click_only_focuses_and_a_drag_without_a_press_does_nothing() {
    let script = Script::new()
        .mouse(MouseKind::Drag(Button::Left), 3, 3)
        .click(3, 3)
        .keys("alt+up enter");
    let (outcome, _) = headless::run(letters().with_mouse(true), script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "c", "b", "d"]));
}

// ---- Reorder: no terminal ----

#[test]
fn reorder_reads_numbers_without_a_terminal() {
    let mut io = ScriptedLineIo::new(["3 1"]);
    let outcome = degrade(
        &mut letters(),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["c", "a", "b", "d"]));
    let mut io = ScriptedLineIo::new([""]);
    let outcome = degrade(
        &mut letters(),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "b", "c", "d"]));
    let mut io = ScriptedLineIo::new(["5"]);
    let outcome = degrade(
        &mut letters(),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(outcome.is_err());
    // Asked for defaults, nothing moves.
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let outcome = degrade(
        &mut letters(),
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(vec!["a", "b", "c", "d"]));
}

// ---- DatePicker ----

const DAY: Date = Date::new(2026, 10, 17);

fn date_picker() -> DatePicker {
    DatePicker::new("Due").value(DAY).today(DAY)
}

#[test]
fn the_grid_shows_the_month_with_the_day_in_brackets() {
    let (_, record) = headless::run(date_picker(), Script::new().keys("escape"), 40, 14);
    let frame = before_answer(&record);
    let lines: Vec<&str> = frame.lines().collect();
    assert_eq!(lines[0], "? Due › 2026-10-17");
    assert_eq!(lines[1].trim(), "October 2026");
    assert_eq!(lines[2], "  Mo Tu We Th Fr Sa Su");
    assert_eq!(lines[3], "            1  2  3  4");
    assert_eq!(lines[5], "  12 13 14 15 16[17]18");
    assert_eq!(lines[6], "  19 20 21 22 23 24 25");
    // A Sunday's closing bracket is after the grid's last column.
    let (_, record) = headless::run(
        DatePicker::new("Due").value(Date::new(2026, 10, 18)),
        Script::new().keys("escape"),
        40,
        14,
    );
    let frame = before_answer(&record);
    assert!(frame.contains("  12 13 14 15 16 17[18]"), "{frame}");
    // The first column's opening bracket is in the gutter.
    let (_, record) = headless::run(
        DatePicker::new("Due").value(Date::new(2026, 10, 12)),
        Script::new().keys("escape"),
        40,
        14,
    );
    assert!(before_answer(&record).contains(" [12]13 14"));
}

#[test]
fn keys_move_by_day_week_month_and_year() {
    let cases = [
        ("left", Date::new(2026, 10, 16)),
        ("right", Date::new(2026, 10, 18)),
        ("up", Date::new(2026, 10, 10)),
        ("down", Date::new(2026, 10, 24)),
        ("pageup", Date::new(2026, 9, 17)),
        ("]", Date::new(2026, 11, 17)),
        ("{", Date::new(2025, 10, 17)),
        ("shift+pagedown", Date::new(2027, 10, 17)),
        ("home", Date::new(2026, 10, 1)),
        ("end", Date::new(2026, 10, 31)),
        ("right right t", DAY),
    ];
    for (keys, expected) in cases {
        let script = Script::new().keys(keys).keys("enter");
        let (outcome, _) = headless::run(date_picker(), script, 40, 14);
        assert_eq!(outcome.unwrap(), Outcome::Done(expected), "{keys}");
    }
}

#[test]
fn days_outside_the_range_cannot_be_reached() {
    let picker = || {
        date_picker()
            .min(Date::new(2026, 10, 10))
            .max(Date::new(2026, 10, 20))
    };
    for (keys, expected) in [
        ("pageup", Date::new(2026, 10, 10)),
        ("down down", Date::new(2026, 10, 20)),
        ("end", Date::new(2026, 10, 20)),
        ("home", Date::new(2026, 10, 10)),
    ] {
        let script = Script::new().keys(keys).keys("enter");
        let (outcome, _) = headless::run(picker(), script, 40, 16);
        assert_eq!(outcome.unwrap(), Outcome::Done(expected), "{keys}");
    }
    let (_, record) = headless::run(picker(), Script::new().keys("escape"), 40, 16);
    assert!(
        before_answer(&record).contains("from 2026-10-10 to 2026-10-20"),
        "{}",
        before_answer(&record)
    );
    // A value that starts outside moves into the range.
    let script = Script::new().keys("enter");
    let start = DatePicker::new("Due")
        .value(Date::new(2027, 1, 1))
        .max(Date::new(2026, 10, 20));
    let (outcome, _) = headless::run(start, script, 40, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(Date::new(2026, 10, 20)));
}

#[test]
fn a_validator_refuses_with_a_message() {
    let picker = date_picker().validate(|date| {
        if date.weekday() >= 5 {
            Err("not on a weekend".into())
        } else {
            Ok(())
        }
    });
    // The 17th is a Saturday: Enter is refused, then Friday is picked.
    let script = Script::new().keys("enter left enter");
    let (outcome, record) = headless::run(picker, script, 40, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(Date::new(2026, 10, 16)));
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("✗ not on a weekend")));
}

#[test]
fn clicks_select_a_day_and_a_second_click_picks_it() {
    // Row 7 is the week of the 26th; column 5 is Tuesday's cell.
    let picker = date_picker().with_mouse(true);
    let script = Script::new().click(5, 7).click(5, 7);
    let (outcome, _) = headless::run(picker, script, 40, 14);
    assert_eq!(outcome.unwrap(), Outcome::Done(Date::new(2026, 10, 27)));
    // A click on a day outside the range does nothing; the wheel turns
    // the month.
    let picker = date_picker().with_mouse(true).max(Date::new(2026, 10, 20));
    let script = Script::new().click(5, 7).scroll(false, 5, 5).keys("enter");
    let (outcome, _) = headless::run(picker, script, 40, 14);
    assert_eq!(outcome.unwrap(), Outcome::Done(Date::new(2026, 9, 17)));
}

#[test]
fn date_picker_degrades_to_a_line() {
    let mut io = ScriptedLineIo::new(["2026-10-20"]);
    let outcome = degrade(
        &mut date_picker(),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(Date::new(2026, 10, 20)));
    // Outside the range: an error naming the limit.
    let mut io = ScriptedLineIo::new(["2026-11-01"]);
    let mut picker = date_picker().max(Date::new(2026, 10, 31));
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    let error = outcome.unwrap_err().to_string();
    assert!(error.contains("the latest is 2026-10-31"), "{error}");
    // Not a date.
    let mut io = ScriptedLineIo::new(["2026-02-30"]);
    let outcome = degrade(
        &mut date_picker(),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(outcome.is_err());
    // An empty line answers with the default.
    let mut io = ScriptedLineIo::new([""]);
    let mut picker = DatePicker::new("Due").default(DAY);
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(DAY));
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let outcome = degrade(
        &mut picker,
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(DAY));
}

// ---- TimePicker ----

#[test]
fn time_fields_step_wrap_and_take_digits() {
    let picker = || TimePicker::new("Start").value(Time::new(23, 58, 0));
    let script = Script::new().keys("up right up up enter");
    let (outcome, record) = headless::run(picker(), script, 40, 6);
    // 23 → 0; 58 → 59 → 0 (no carry into the hour).
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(0, 0, 0)));
    assert_eq!(record.last_frame(), "? Start › 00:00");
    // `7` fills the minute at once (no minute starts with 7x); `2` waits
    // for a second digit.
    let script = Script::new().text("2").text("17").keys("enter");
    let (outcome, _) = headless::run(picker(), script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(21, 7, 0)));
    let (_, record) = headless::run(picker(), Script::new().keys("escape"), 40, 6);
    assert_eq!(before_answer(&record).lines().nth(1), Some("  [23]:58"));
}

#[test]
fn seconds_and_a_twelve_hour_clock() {
    let picker = TimePicker::new("Alarm")
        .value(Time::new(14, 5, 0))
        .seconds(true)
        .twelve_hour(true);
    let (_, record) = headless::run(picker, Script::new().keys("escape"), 40, 6);
    assert_eq!(
        before_answer(&record).lines().next(),
        Some("? Alarm › 2:05:00 PM")
    );
    assert_eq!(
        before_answer(&record).lines().nth(1),
        Some("  [02]:05:00 PM")
    );
    let picker = TimePicker::new("Alarm")
        .value(Time::new(14, 5, 0))
        .seconds(true)
        .twelve_hour(true);
    // 09 on the hour, then minute 30, second 15, then `a` for AM.
    let script = Script::new()
        .text("0930")
        .text("15")
        .text("a")
        .keys("enter");
    let (outcome, _) = headless::run(picker, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(9, 30, 15)));
}

#[test]
fn a_time_outside_the_range_is_refused_on_enter() {
    let picker = TimePicker::new("Start")
        .value(Time::new(8, 0, 0))
        .min(Time::new(9, 0, 0))
        .max(Time::new(17, 30, 0));
    let script = Script::new().keys("enter up enter");
    let (outcome, record) = headless::run(picker, script, 40, 6);
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(9, 0, 0)));
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("✗ the earliest is 09:00")));
}

#[test]
fn time_picker_degrades_to_a_line() {
    let mut io = ScriptedLineIo::new(["2:30 pm"]);
    let mut picker = TimePicker::new("Start");
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(Time::new(14, 30, 0)));
    let mut io = ScriptedLineIo::new(["18:00"]);
    let mut picker = TimePicker::new("Start").max(Time::new(17, 30, 0));
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    let error = outcome.unwrap_err().to_string();
    assert!(error.contains("the latest is 17:30"), "{error}");
    // No answer and no default.
    let mut io = ScriptedLineIo::new([""]);
    let outcome = degrade(
        &mut TimePicker::new("Start"),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(outcome.is_err());
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let outcome = degrade(
        &mut TimePicker::new("Start"),
        Fallback::Default,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(matches!(
        outcome,
        Err(rich_interact::Error::NotInteractive(
            NotInteractive::NoDefault(_)
        ))
    ));
}

// ---- DateTimePicker ----

fn at(day: u32, hour: u32, minute: u32) -> DateTime {
    DateTime::new(Date::new(2026, 10, day), Time::new(hour, minute, 0))
}

#[test]
fn tab_moves_between_the_grid_and_the_time() {
    let picker = DateTimePicker::new("Meeting")
        .value(at(17, 9, 0))
        .today(DAY);
    let script = Script::new()
        .keys("left tab up tab")
        .text("45")
        .keys("tab right enter");
    let (outcome, record) = headless::run(picker, script, 40, 16);
    // Back on the grid after the minute, Right is a day later.
    assert_eq!(outcome.unwrap(), Outcome::Done(at(17, 10, 45)));
    assert_eq!(record.last_frame(), "? Meeting › 2026-10-17 10:45");
}

#[test]
fn the_range_counts_the_time_on_its_first_and_last_days() {
    let picker = || {
        DateTimePicker::new("Meeting")
            .value(at(18, 8, 0))
            .min(at(17, 9, 0))
            .max(at(20, 17, 0))
    };
    // 08:00 on the 18th is fine; the 17th at 08:00 is too early.
    let script = Script::new().keys("enter");
    let (outcome, _) = headless::run(picker(), script, 40, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(at(18, 8, 0)));
    let script = Script::new().keys("left enter tab up enter");
    let (outcome, record) = headless::run(picker(), script, 40, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(at(17, 9, 0)));
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("✗ the earliest is 2026-10-17 09:00")));
    // Before the 17th cannot be reached.
    let script = Script::new().keys("pageup tab up enter");
    let (outcome, _) = headless::run(picker(), script, 40, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(at(17, 9, 0)));
}

#[test]
fn a_click_focuses_a_time_field() {
    let picker = DateTimePicker::new("Meeting")
        .value(at(17, 9, 0))
        .with_mouse(true);
    // The time row is under the six weeks: row 9; "  Time " is 7 cells,
    // then the hour (2) and ':', so the minute starts at column 10.
    let script = Script::new().click(11, 9).keys("up enter");
    let (outcome, _) = headless::run(picker, script, 40, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(at(17, 9, 1)));
}

#[test]
fn date_time_picker_degrades_to_a_line() {
    let mut io = ScriptedLineIo::new(["2026-10-17 14:30"]);
    let mut picker = DateTimePicker::new("Meeting");
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(at(17, 14, 30)));
    let mut io = ScriptedLineIo::new(["2026-10-17T08:00"]);
    let mut picker = DateTimePicker::new("Meeting").min(at(17, 9, 0));
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    let error = outcome.unwrap_err().to_string();
    assert!(
        error.contains("the earliest is 2026-10-17 09:00"),
        "{error}"
    );
}

#[test]
fn readable_without_colour() {
    // NO_COLOR keeps the brackets and the marks: the frames are plain text.
    let picker = DateTimePicker::new("Meeting").value(at(17, 9, 0));
    let (_, record) = headless::run(picker, Script::new().keys("tab escape"), 40, 16);
    let frame = before_answer(&record);
    assert!(frame.contains("16[17]18"), "{frame}");
    assert!(frame.contains("Time [09]:00"), "{frame}");
}
