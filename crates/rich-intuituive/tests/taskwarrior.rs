//! The taskwarrior-tui rebuild (examples/tasks.rs): its report, filter,
//! command bar, marks and bulk actions, undo, menus and calendar, driven
//! by taskwarrior-tui's keys over a store in a temporary directory.

#[path = "../examples/tasks.rs"]
#[allow(dead_code)]
mod tasks;

use std::path::PathBuf;

use intuituive::widgets::Date;
use intuituive::Driver;
use tasks::*;

/// Monday 2026-10-19, noon UTC.
const NOW: i64 = 20_745 * 86_400 + 12 * 3_600;

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("intuituive-tasks-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn store(name: &str) -> (JsonlStore, PathBuf) {
    let dir = dir(name);
    let mut store = JsonlStore::open(&dir, 7).unwrap();
    sample(&mut store, &dir, NOW, 0).unwrap();
    (store, dir)
}

fn driver_at(name: &str, width: u16, height: u16) -> (Driver, PathBuf) {
    let (store, dir) = store(name);
    let options = Options {
        now: Some(NOW),
        offset: 0,
    };
    (
        tasks_app(Box::new(store), options).driver(width, height),
        dir,
    )
}

fn driver(name: &str) -> (Driver, PathBuf) {
    driver_at(name, 100, 30)
}

fn rows(d: &Driver) -> Vec<String> {
    d.screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn has(d: &Driver, needle: &str) -> bool {
    rows(d).iter().any(|r| r.contains(needle))
}

fn row_of(d: &Driver, needle: &str) -> Option<usize> {
    rows(d).iter().position(|r| r.contains(needle))
}

/// The descriptions in the report, top to bottom.
fn report(d: &Driver) -> Vec<String> {
    let rows = rows(d);
    let header = rows
        .iter()
        .position(|r| r.contains("Description"))
        .expect("a header");
    let at = rows[header].find("Description").unwrap();
    (rows[header + 1..].iter())
        .take_while(|r| !r.is_empty() && !r.starts_with('─'))
        .map(|r| {
            let text: String = r.chars().skip(at).collect();
            let text = text.split("  ").next().unwrap_or("").trim().to_string();
            text
        })
        .collect()
}

/// The report row showing `description`.
fn line(d: &Driver, description: &str) -> String {
    let at = row_of(d, description).unwrap_or_else(|| panic!("{description}: {:#?}", rows(d)));
    rows(d)[at].clone()
}

/// The row of the styled screen showing `text`.
fn styled_line(d: &Driver, text: &str) -> String {
    let styled = intuituive::testing::styled(d);
    styled
        .lines()
        .find(|l| l.contains(text))
        .unwrap_or("")
        .to_string()
}

fn stored(dir: &std::path::Path) -> Vec<Task> {
    let mut store = JsonlStore::open(dir, 1).unwrap();
    store.load().unwrap()
}

fn find<'a>(tasks: &'a [Task], description: &str) -> &'a Task {
    tasks.iter().find(|t| t.description == description).unwrap()
}

const DAY: i64 = 86_400;

#[test]
fn the_next_report_is_taskwarriors() {
    let (mut d, _) = driver("next");
    d.settle();
    let r = rows(&d);
    assert!(
        r[0].starts_with(" Tasks   Projects   Timesheet   Calendar"),
        "{r:#?}"
    );
    assert!(r[0].ends_with("next [none]"), "{r:#?}");
    assert_eq!(
        r[1],
        "  ID Active Age P Project     Tag    Due Description                                           Urg"
    );
    // Sorted by urgency, the most urgent first, the selection on it.
    assert_eq!(
        r[2],
        "• 2  2h     10d   work        next   4d  Write the quarterly report                            27.60"
    );
    assert_eq!(
        report(&d),
        [
            "Write the quarterly report",
            "Pay the rent",
            "Review pull requests [1]",
            "Call the plumber",
            "Renew the passport",
            "Book flights to Lisbon",
            "Fix the garden fence",
        ]
    );
    assert!(line(&d, "Review pull requests").contains("-1d"));
    // The completed task is not in it; the command bar shows the filter.
    assert!(!has(&d, "Buy groceries"));
    assert_eq!(r[28], "Filter Tasks");
    assert_eq!(r[29], "status:pending");
}

#[test]
fn urgency_is_taskwarriors() {
    let mut task = Task::new("u".into(), "t", NOW - 10 * DAY);
    task.project = Some("work".into());
    task.tags = vec!["next".into()];
    task.start = Some(NOW - 3_600);
    task.due = Some(NOW + 4 * DAY + 12 * 3_600);
    // project 1 + active 4 + next 15 + due 12 * (9.5 * 0.8 / 21 + 0.2)
    // + one tag 0.8 + age 2 * 10 / 365.
    let expected = 1.0 + 4.0 + 15.0 + 12.0 * (9.5 * 0.8 / 21.0 + 0.2) + 0.8 + 20.0 / 365.0;
    assert!((urgency(&task, NOW) - expected).abs() < 1e-9);
    task.due = Some(NOW - 8 * DAY);
    task.priority = Some('H');
    task.annotations = vec![
        Annotation {
            entry: NOW,
            description: "a".into()
        };
        2
    ];
    let expected = 1.0 + 4.0 + 15.0 + 12.0 + 6.0 + 0.8 + 0.9 + 20.0 / 365.0;
    assert!((urgency(&task, NOW) - expected).abs() < 1e-9);
}

#[test]
fn rows_are_coloured_by_taskwarriors_default_rules() {
    let (mut d, _) = driver("colours");
    d.press("j");
    // Active: rgb555 on rgb410.
    assert!(styled_line(&d, "Write the quarterly").starts_with("[color(231) on color(166)]"));
    // Due within the week: color1, on the alternate rows' gray2; the
    // selection is bold.
    assert!(styled_line(&d, "Pay the rent").starts_with("[bold color(1) on color(234)]•"));
    // Overdue: color9.
    assert!(styled_line(&d, "Review pull requests").starts_with("[color(9)]"));
    // Tagged: color2; low priority: color245.
    assert!(styled_line(&d, "Call the plumber").starts_with("[color(2) on color(234)]"));
    assert!(styled_line(&d, "Fix the garden fence").starts_with("[color(245)]"));
    // The header is underlined, the navigation bar reversed.
    assert!(styled_line(&d, "Description").starts_with("[underline]  ID"));
    assert!(styled_line(&d, "Timesheet").starts_with("[bold reverse] Tasks [/][reverse]"));
}

#[test]
fn the_details_are_task_info() {
    let (mut d, _) = driver("info");
    d.press("j j");
    let r = rows(&d);
    let at = row_of(&d, "Name          Value").expect("the info header");
    assert!(r[at - 1].starts_with("────"), "{r:#?}");
    assert_eq!(
        r[at + 1].trim_end_matches(['┃', '│', ' ']),
        "ID            3"
    );
    assert!(r[at + 2].starts_with("Description   Review pull requests"));
    assert!(r[at + 3].starts_with("                2026-10-18 12:00:00 Two are from Ada"));
    assert!(has(&d, "Entered       2026-10-14 12:00:00 (5d)"));
    assert!(has(
        &d,
        "Virtual tags  ANNOTATED DUE OVERDUE PENDING PROJECT READY TAGGED UNBLOCKED"
    ));
    // Ctrl+E scrolls them to the urgency's terms.
    d.press("ctrl+e ctrl+e ctrl+e ctrl+e ctrl+e ctrl+e ctrl+e ctrl+e");
    assert!(
        has(&d, "    due            0.79 *    12 =   9.486"),
        "{:#?}",
        rows(&d)
    );
    assert!(has(&d, "    annotations     0.8 *     1 =     0.8"));
    assert!(has(&d, "                                      12.113"));
    // Ctrl+Y scrolls back.
    d.press("ctrl+y ctrl+y ctrl+y ctrl+y ctrl+y ctrl+y ctrl+y ctrl+y");
    assert!(has(&d, "Name          Value"));
}

#[test]
fn z_hides_the_details_and_backslash_puts_them_beside() {
    let (mut d, _) = driver("layout");
    d.press("z");
    assert!(!has(&d, "Name          Value"));
    assert_eq!(report(&d).len(), 7);
    d.press("z backslash");
    let r = rows(&d);
    assert!(r[1].starts_with("  ID Active Age"), "{r:#?}");
    assert!(r[2].starts_with("• 2") && r[2].contains("Name"), "{r:#?}");
    // j still moves the report after the layout changed.
    d.press("j");
    let r = rows(&d);
    assert!(
        r[3].starts_with("• 1") && r[3].ends_with("ID            1"),
        "{r:#?}"
    );
}

#[test]
fn slash_filters_as_it_is_typed_and_esc_puts_it_back() {
    let (mut d, _) = driver("filter");
    d.press("slash");
    assert_eq!(rows(&d)[28], "Filter Tasks");
    d.type_text("project:home");
    assert_eq!(rows(&d)[29], "status:pending project:home");
    assert_eq!(report(&d), ["Pay the rent", "Fix the garden fence"]);
    d.press("esc");
    assert_eq!(report(&d).len(), 7);
    assert_eq!(rows(&d)[29], "status:pending");
    // Enter keeps it; words, tags and virtual tags filter too.
    d.press("slash");
    d.type_text("-bills +DUE");
    d.press("enter");
    assert_eq!(
        report(&d),
        ["Write the quarterly report", "Review pull requests [1]"]
    );
    d.press("slash ctrl+u");
    d.type_text("status:pending the");
    d.press("enter");
    assert_eq!(
        report(&d),
        [
            "Write the quarterly report",
            "Pay the rent",
            "Call the plumber",
            "Renew the passport",
            "Fix the garden fence"
        ]
    );
    // The keys work on the report again.
    d.press("G");
    assert!(line(&d, "Fix the garden fence").starts_with("•"));
}

#[test]
fn a_filter_it_cannot_read_turns_the_bar_red() {
    let (mut d, _) = driver("bad-filter");
    d.press("slash");
    d.type_text(" or +bills");
    // taskwarrior-tui's `command.error` style: red, not reversed.
    assert!(styled_line(&d, "Filter Tasks").starts_with("[bold red]Filter Tasks[/][red]"));
    assert!(!has(&d, "Pay the rent"));
}

#[test]
fn filters_read_taskwarriors_syntax() {
    let (store, _) = store("filters");
    let mut store = store;
    let tasks = store.load().unwrap();
    let ids = |f: &str| -> Vec<u64> {
        let filter = Filter::parse(f, NOW, 0).unwrap();
        tasks
            .iter()
            .filter(|t| filter.matches(t, NOW, 0))
            .map(|t| t.id)
            .collect()
    };
    assert_eq!(ids("project:home"), [1, 5]);
    assert_eq!(ids("project: status:pending"), [6]);
    assert_eq!(ids("+OVERDUE"), [3]);
    assert_eq!(ids("+ACTIVE"), [2]);
    assert_eq!(ids("status:pending -TAGGED priority:"), [4, 7]);
    assert_eq!(ids("2,4-5"), [2, 4, 5]);
    assert_eq!(ids("Ada"), [3]);
    assert_eq!(ids("status:completed"), [0]);
    assert_eq!(ids("due.before:eow status:pending"), [1, 2, 3]);
    assert_eq!(ids("due.before:today"), [3]);
    assert!(Filter::parse("+a or +b", NOW, 0).is_err());
    assert!(Filter::parse("priority:X", NOW, 0).is_err());
}

#[test]
fn dates_read_as_taskwarrior_reads_them() {
    let at = |w: &str| parse_date(w, NOW, 0).map(|t| (Date::from_days(t / DAY), t % DAY));
    assert_eq!(at("today"), Some((Date::new(2026, 10, 19), 0)));
    assert_eq!(at("tomorrow"), Some((Date::new(2026, 10, 20), 0)));
    assert_eq!(at("eod"), Some((Date::new(2026, 10, 19), DAY - 1)));
    // Weeks start on Sunday: the week ends on Saturday the 24th.
    assert_eq!(at("eow"), Some((Date::new(2026, 10, 24), DAY - 1)));
    assert_eq!(at("sow"), Some((Date::new(2026, 10, 25), 0)));
    assert_eq!(at("eom"), Some((Date::new(2026, 10, 31), DAY - 1)));
    assert_eq!(at("eoy"), Some((Date::new(2026, 12, 31), DAY - 1)));
    // A weekday is the next one, never today.
    assert_eq!(at("monday"), Some((Date::new(2026, 10, 26), 0)));
    assert_eq!(at("fri"), Some((Date::new(2026, 10, 23), 0)));
    assert_eq!(at("2026-12-25"), Some((Date::new(2026, 12, 25), 0)));
    assert_eq!(
        at("2026-12-25T09:30"),
        Some((Date::new(2026, 12, 25), 9 * 3_600 + 1_800))
    );
    assert_eq!(at("3d"), Some((Date::new(2026, 10, 22), 12 * 3_600)));
    assert_eq!(at("2w"), Some((Date::new(2026, 11, 2), 12 * 3_600)));
    assert_eq!(at("someday"), None);
    // In a zone two hours east, today starts at 22:00 UTC the day before.
    assert_eq!(parse_date("today", NOW, 7_200), Some(20_745 * DAY - 7_200));
}

#[test]
fn a_adds_a_task_with_its_attributes() {
    let (mut d, dir) = driver("add");
    d.press("a");
    assert_eq!(rows(&d)[28], "Add Task");
    d.type_text("Water the plants project:home.garden +garden due:tomorrow priority:M");
    d.press("enter");
    assert_eq!(rows(&d)[28], "Filter Tasks");
    let row = line(&d, "Water the plants");
    assert!(
        row.contains("8 ")
            && row.contains("home.garden")
            && row.contains("garden ")
            && row.contains("12h"),
        "{row}"
    );
    let tasks = stored(&dir);
    let task = find(&tasks, "Water the plants");
    assert_eq!(task.due, Some(20_746 * DAY));
    assert_eq!(task.priority, Some('M'));
    assert_eq!(task.tags, ["garden"]);
    assert_eq!(task.entry, NOW);
    assert_eq!(task.id, 8);
    // l logs a task done.
    d.press("l");
    d.type_text("\"Sweep the porch\" project:home");
    d.press("enter");
    let tasks = stored(&dir);
    let task = find(&tasks, "Sweep the porch");
    assert_eq!((task.status, task.end), (Status::Completed, Some(NOW)));
    assert!(!has(&d, "Sweep the porch"));
}

#[test]
fn an_empty_add_is_an_error_and_a_key_closes_it() {
    let (mut d, _) = driver("error");
    d.press("a");
    d.type_text("project:home");
    d.press("enter");
    assert!(has(&d, "Additional text must be provided."));
    assert_eq!(rows(&d)[28], "Error");
    assert_eq!(rows(&d)[29], "Press any key to continue.");
    let styled = intuituive::testing::styled(&d);
    assert!(styled
        .lines()
        .nth(28)
        .unwrap()
        .starts_with("[bold red]Error[/]"));
    d.press("space");
    assert!(!has(&d, "Additional text"));
    assert_eq!(rows(&d)[28], "Filter Tasks");
    // The report has the keys again.
    d.press("j");
    assert!(line(&d, "Pay the rent").starts_with("• 1"));
}

#[test]
fn m_modifies_from_the_tasks_own_attributes() {
    let (mut d, dir) = driver("modify");
    d.press("j m");
    assert_eq!(rows(&d)[28], "Modify Task 1");
    assert_eq!(
        rows(&d)[29],
        "Pay the rent +bills project:home.bills priority:H due:2026-10-20"
    );
    d.type_text("+urgent priority:L");
    d.press("enter");
    let tasks = stored(&dir);
    let task = find(&tasks, "Pay the rent");
    assert_eq!(task.tags, ["bills", "urgent"]);
    assert_eq!(task.priority, Some('L'));
    assert_eq!(task.modified, NOW);
    // New words replace the description.
    d.press("m ctrl+u");
    d.type_text("Pay the October rent");
    d.press("enter");
    assert!(has(&d, "Pay the October rent"));
    // The selection stayed on the task as it moved.
    assert!(line(&d, "Pay the October rent").starts_with("•"));
}

#[test]
fn a_capital_annotates() {
    let (mut d, dir) = driver("annotate");
    d.press("j j j A");
    assert_eq!(rows(&d)[28], "Annotate Task 6");
    d.type_text("\"Ask about the boiler\"");
    d.press("enter");
    assert!(has(&d, "Call the plumber [1]"));
    assert!(has(&d, "  2026-10-19 12:00:00 Ask about the boiler"));
    let tasks = stored(&dir);
    assert_eq!(
        find(&tasks, "Call the plumber").annotations[0].description,
        "Ask about the boiler"
    );
}

#[test]
fn d_completes_and_u_undoes_after_asking() {
    let (mut d, dir) = driver("done");
    d.press("d");
    assert!(!has(&d, "Write the quarterly report"));
    assert!(line(&d, "Pay the rent").starts_with("• 1"));
    // The only active task done, the Active column goes; the keys still
    // reach the report.
    assert!(rows(&d)[1].starts_with("  ID Age P"));
    d.press("j k j");
    // Ids are renumbered, as Taskwarrior's working set is.
    assert!(line(&d, "Review pull requests").starts_with("• 2"));
    d.press("k");
    let tasks = stored(&dir);
    assert_eq!(
        find(&tasks, "Write the quarterly report").status,
        Status::Completed
    );
    d.press("u");
    assert_eq!(rows(&d)[28], "Run `task undo`?");
    assert_eq!(rows(&d)[29], "Press <u> to confirm or <q> to abort.");
    d.press("q");
    assert_eq!(rows(&d)[28], "Filter Tasks");
    assert!(!has(&d, "Write the quarterly report"));
    d.press("u u");
    assert!(has(&d, "Write the quarterly report"));
    assert_eq!(
        find(&stored(&dir), "Write the quarterly report").status,
        Status::Pending
    );
    // Nothing left to undo is an error.
    d.press("u u");
    assert!(has(&d, "No undo information available"));
}

#[test]
fn x_deletes_after_asking() {
    let (mut d, dir) = driver("delete");
    d.press("j x");
    assert_eq!(rows(&d)[28], "Delete Task 1?");
    assert_eq!(rows(&d)[29], "Press <x> to confirm or <q> to abort.");
    d.press("esc");
    assert!(has(&d, "Pay the rent"));
    d.press("x x");
    assert!(!has(&d, "Pay the rent"));
    assert_eq!(find(&stored(&dir), "Pay the rent").status, Status::Deleted);
    assert_eq!(rows(&d)[28], "Filter Tasks");
}

#[test]
fn s_starts_and_stops_and_the_selection_follows_the_task() {
    let (mut d, dir) = driver("start");
    d.press("j j j j s");
    // Active, it is more urgent and moves up; the selection goes with it.
    let row = line(&d, "Renew the passport");
    assert!(row.starts_with("• 7  0s"), "{row}");
    assert_eq!(report(&d)[3], "Renew the passport");
    assert!(styled_line(&d, "Renew the passport").starts_with("[bold color(231) on color(166)]"));
    assert_eq!(find(&stored(&dir), "Renew the passport").start, Some(NOW));
    d.press("s");
    assert_eq!(find(&stored(&dir), "Renew the passport").start, None);
    assert!(line(&d, "Renew the passport").starts_with("• 7"));
}

#[test]
fn t_and_the_priority_keys_change_the_selected_task() {
    let (mut d, dir) = driver("quick");
    d.press("G t H");
    let tasks = stored(&dir);
    let task = find(&tasks, "Fix the garden fence");
    assert_eq!(task.tags, ["next"]);
    assert_eq!(task.priority, Some('H'));
    // +next makes it the most urgent but one.
    assert_eq!(report(&d)[1], "Fix the garden fence");
    d.press("N y");
    let tasks = stored(&dir);
    assert_eq!(
        tasks
            .iter()
            .filter(|t| t.description == "Fix the garden fence")
            .count(),
        2
    );
    assert!(tasks
        .iter()
        .all(|t| t.description != "Fix the garden fence" || t.priority.is_none()));
}

#[test]
fn v_marks_tasks_and_actions_take_the_marked() {
    let (mut d, dir) = driver("marks");
    d.press("v j v");
    assert!(line(&d, "Write the quarterly").starts_with("  ✔ 2"));
    assert!(line(&d, "Pay the rent").starts_with("• ✔ 1"));
    assert!(line(&d, "Review pull").starts_with("    3"));
    d.press("m");
    assert_eq!(rows(&d)[28], "Modify Tasks 2,1");
    assert_eq!(rows(&d)[29], "");
    d.type_text("+urgent");
    d.press("enter");
    let tasks = stored(&dir);
    assert!(find(&tasks, "Pay the rent")
        .tags
        .contains(&"urgent".to_string()));
    assert!(find(&tasks, "Write the quarterly report")
        .tags
        .contains(&"urgent".to_string()));
    assert!(!find(&tasks, "Review pull requests")
        .tags
        .contains(&"urgent".to_string()));
    // The marks are cleared after an action; V marks every task.
    assert!(line(&d, "Pay the rent").starts_with("• 1"));
    d.press("V");
    assert!(line(&d, "Fix the garden fence").starts_with("  ✔ 5"));
    d.press("x");
    assert_eq!(rows(&d)[28], "Delete Tasks 2,1,3,6,7,4,5?");
    d.press("x");
    assert!(!has(&d, "Description"));
    // One undo brings them all back.
    d.press("u u");
    assert_eq!(report(&d).len(), 7);
    d.press("V V");
    assert!(line(&d, "Fix the garden fence").starts_with("  5"));
}

#[test]
fn tab_completes_projects_tags_and_dates() {
    let (mut d, _) = driver("complete");
    d.press("a");
    d.type_text("Plant bulbs project:ho");
    d.press("tab");
    assert_eq!(rows(&d)[29], "Plant bulbs project:home.bills");
    d.press("tab");
    assert_eq!(rows(&d)[29], "Plant bulbs project:home.garden");
    d.type_text(" +ph");
    d.press("tab");
    assert_eq!(rows(&d)[29], "Plant bulbs project:home.garden +phone");
    d.type_text(" due:to");
    d.press("tab");
    assert_eq!(
        rows(&d)[29],
        "Plant bulbs project:home.garden +phone due:today"
    );
    d.press("tab");
    assert_eq!(
        rows(&d)[29],
        "Plant bulbs project:home.garden +phone due:tomorrow"
    );
    d.type_text(" pri");
    d.press("tab");
    assert_eq!(
        rows(&d)[29],
        "Plant bulbs project:home.garden +phone due:tomorrow priority:"
    );
}

#[test]
fn colon_jumps_to_an_id() {
    let (mut d, _) = driver("jump");
    d.press("colon");
    assert_eq!(rows(&d)[28], "Jump to Task");
    d.type_text("4");
    d.press("enter");
    assert!(line(&d, "Book flights").starts_with("• 4"));
    d.press("colon");
    d.type_text("42");
    d.press("enter");
    assert!(has(&d, "Cannot locate task id 42"));
}

#[test]
fn the_context_menu_sets_a_context() {
    let (mut d, dir) = driver("context");
    d.press("c");
    assert!(has(&d, "Context  (type to filter)"));
    let r = rows(&d);
    let at = row_of(&d, "Definition").unwrap();
    assert!(r[at].contains("Name Definition"));
    assert!(r[at + 1].contains("• none") && r[at + 1].contains("yes"));
    assert!(r[at + 2].contains("work project:work"));
    // Typing filters the contexts; Enter picks.
    d.type_text("ho");
    assert!(
        rows(&d)[at + 1].contains("• home project:home"),
        "{:#?}",
        rows(&d)
    );
    d.press("enter");
    assert!(rows(&d)[0].ends_with("next [home]"));
    assert_eq!(report(&d), ["Pay the rent", "Fix the garden fence"]);
    assert_eq!(
        std::fs::read_to_string(dir.join("taskrc"))
            .unwrap()
            .lines()
            .last(),
        Some("context=home")
    );
    // Back to none, with the arrows.
    d.press("c down down up up enter");
    assert!(rows(&d)[0].ends_with("next [none]"));
    assert_eq!(report(&d).len(), 7);
}

#[test]
fn the_report_menu_switches_reports() {
    let (mut d, _) = driver("reports");
    d.press("R");
    assert!(has(&d, "Report  (type to filter)"));
    assert!(has(&d, "• next      Most urgent tasks"));
    d.type_text("comp");
    d.press("enter");
    assert!(rows(&d)[0].ends_with("completed [none]"));
    assert_eq!(rows(&d)[29], "status:completed");
    assert!(
        rows(&d)[1].starts_with("  ID UUID     Created    Completed  Age Description"),
        "{:#?}",
        rows(&d)
    );
    assert_eq!(report(&d), ["Buy groceries"]);
    d.press("R down down down enter");
    assert!(rows(&d)[0].ends_with("overdue [none]"));
    assert_eq!(report(&d), ["Review pull requests [1]"]);
}

#[test]
fn the_calendar_marks_the_due_dates() {
    let (mut d, _) = driver_at("calendar", 100, 40);
    d.press("bracketleft");
    let r = rows(&d);
    assert!(r[2].trim() == "2026", "{r:#?}");
    assert!(has(&d, "October 2026"));
    assert!(has(&d, "Su Mo Tu We Th Fr Sa"));
    let october = styled_line(&d, "[bold]19[/]");
    // Overdue in color9, due in color1, the active task's in its colours,
    // and today in bold.
    assert!(
        october.contains("[color(9)]18[/] [bold]19[/] [color(1)]20[/]"),
        "{october}"
    );
    assert!(
        october.contains("[color(231) on color(166)]24[/]"),
        "{october}"
    );
    // j and k move a year, J and K ten.
    d.press("j");
    assert_eq!(rows(&d)[2].trim(), "2027");
    d.press("K");
    assert_eq!(rows(&d)[2].trim(), "2017");
    // ] goes round to the report.
    d.press("bracketright");
    assert!(has(&d, "Write the quarterly report"));
    d.press("j");
    assert!(line(&d, "Pay the rent").starts_with("•"));
}

#[test]
fn the_projects_and_timesheet_views() {
    let (mut d, _) = driver("views");
    d.press("bracketright");
    let r = rows(&d);
    assert_eq!(
        r[2..9],
        [
            "Project  Tasks",
            "(none)       1",
            "home         2",
            "  bills      1",
            "  garden     1",
            "travel       2",
            "work         2"
        ]
    );
    assert_eq!(r[10], "5 projects (7 tasks)");
    d.press("bracketright");
    assert!(has(
        &d,
        "Wk  Date       Day ID Action    Project Due        Task"
    ));
    assert!(has(
        &d,
        "W42 2026-10-18 Sun  - Completed                    Buy groceries"
    ));
    assert!(has(
        &d,
        "    2026-10-19 Mon  2 Started   work    2026-10-24 Write the quarterly report"
    ));
    assert!(has(&d, "1 completed, 1 started."));
}

#[test]
fn help_lists_the_keys() {
    let (mut d, _) = driver("help");
    d.press("question");
    assert!(has(&d, "Help"));
    assert!(has(&d, "Task report"));
    assert!(has(&d, "/       Filter task report"));
    assert!(has(&d, "a       Add new task"));
}

#[test]
fn the_store_keeps_tasks_as_json_lines_and_undoes() {
    let (mut store, dir) = store("store");
    let tasks = store.load().unwrap();
    assert_eq!(tasks.len(), 8);
    let text = std::fs::read_to_string(dir.join("tasks.jsonl")).unwrap();
    let first = text.lines().next().unwrap();
    assert!(
        first.contains("\"description\":\"Pay the rent\""),
        "{first}"
    );
    assert!(first.contains("\"due\":\"20261020T000000Z\""), "{first}");
    assert!(first.contains("\"project\":\"home.bills\""), "{first}");
    // A round trip keeps everything.
    for task in &tasks {
        let mut back = Task::from_json(&task.to_json()).unwrap();
        back.id = task.id;
        assert_eq!(&back, task);
    }
    let mut changed = tasks[0].clone();
    changed.description = "Pay the \"big\" rent".into();
    let new = Task::new(store.new_uuid(), "New", NOW);
    store.commit(vec![changed, new]).unwrap();
    let after = store.load().unwrap();
    assert_eq!(after.len(), 9);
    assert_eq!(after[0].description, "Pay the \"big\" rent");
    assert_eq!(store.undo().unwrap(), 2);
    let undone = store.load().unwrap();
    assert_eq!(undone, tasks);
    assert!(store.undo().is_err());
}

#[test]
fn an_empty_report_still_takes_the_keys() {
    let (mut d, dir) = driver("empty");
    d.press("slash");
    d.type_text("+nothing");
    d.press("enter");
    assert!(!has(&d, "Description"));
    assert!(has(&d, "Task not found"));
    d.press("a");
    d.type_text("Something +nothing");
    d.press("enter");
    assert_eq!(report(&d), ["Something"]);
    assert_eq!(stored(&dir).len(), 9);
}
