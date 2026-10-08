//! A project planner: projects and their tasks in a tree, a task's due date
//! on a calendar, and a long activity log, under a menu bar.
//!
//!     cargo run -p rs-rich-intuituive --example planner
//!
//! Tab moves between the tree, the calendar and the log · Space marks the
//! selected task done · n adds a task to its project · the calendar's
//! arrows move the due date · F10 opens the menus · ? shows the keys ·
//! Ctrl+P finds a command · q quits. The divider between the tree and the
//! rest, and the one above the log, drag.
//!
//! Built on the framework's components: a `menu_bar`, a `tree` of the
//! projects, `hsplit` and `vsplit` panes, a `calendar` for the due date,
//! and a `virtual_list` of a hundred thousand log rows that draws only the
//! rows in view. The help and the command palette are made from the
//! bindings' descriptions, and changes raise toasts.

use std::collections::HashSet;

use intuituive::menu::{menu_bar, Menu, MenuItem};
use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::widgets::{calendar_with, hsplit, tree_with, virtual_list, vsplit, Date, TreeItem};

/// A task, with its due date.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub title: String,
    pub due: Date,
    pub done: bool,
}

/// A project and its tasks.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub name: String,
    pub tasks: Vec<Task>,
}

fn task(title: &str, month: u32, day: u32, done: bool) -> Task {
    Task {
        title: title.to_string(),
        due: Date {
            year: 2026,
            month,
            day,
        },
        done,
    }
}

/// The projects the planner starts with.
pub fn sample_projects() -> Vec<Project> {
    vec![
        Project {
            name: "intuiTUIve".into(),
            tasks: vec![
                task("Widget trait v2", 10, 6, true),
                task("Owning the loop", 10, 7, true),
                task("Examples", 10, 8, false),
                task("Release 0.0.2", 10, 9, false),
            ],
        },
        Project {
            name: "rs-rich".into(),
            tasks: vec![
                task("Sync upstream 15.1", 10, 20, false),
                task("Golden fixtures", 10, 21, false),
            ],
        },
        Project {
            name: "Home".into(),
            tasks: vec![task("Water the plants", 10, 8, false)],
        },
    ]
}

/// How many rows the log had before the planner opened.
pub const HISTORY: usize = 100_000;

/// Row `i` of the log from before the planner opened: the same every run.
fn history_row(i: usize) -> String {
    let what = ["synced", "built", "tested", "deployed", "reviewed"][i % 5];
    let day = 1 + (i / 3_000) % 28;
    let time = (i * 37) % 1440;
    format!(
        "[muted]2026-09-{day:02} {:02}:{:02}[/] {what} [dim]#{i}[/]",
        time / 60,
        time % 60
    )
}

/// The project and task a tree path points at.
fn at(path: &[usize]) -> (usize, Option<usize>) {
    (path.first().copied().unwrap_or(0), path.get(1).copied())
}

/// The planner.
pub fn planner_app() -> App {
    App::new(|| {
        let projects = signal(sample_projects());
        let selected = signal(vec![0, 2]);
        let expanded = signal(HashSet::from([vec![0], vec![1], vec![2]]));
        let due = signal(Date {
            year: 2026,
            month: 10,
            day: 8,
        });
        let events = signal(Vec::<String>::new());
        let log_selected = signal(HISTORY - 1);

        // Write `line` to the log and show its newest row.
        let log = move |line: String| {
            events.update(|e| e.push(line));
            log_selected.set(HISTORY + events.with_untracked(Vec::len) - 1);
        };

        // The calendar follows the selected task; moving the date on the
        // calendar moves the task's due date.
        watch(
            move || {
                let (p, t) = at(&selected.get());
                projects.with(|ps| t.and_then(|t| ps.get(p)?.tasks.get(t).map(|t| t.due)))
            },
            move |date, _| {
                if let Some(date) = date {
                    due.set(date);
                }
            },
        );
        watch(
            move || due.get(),
            move |date, _| {
                let (p, t) = at(&selected.get_untracked());
                let Some(t) = t else { return };
                let moved = projects.with_untracked(|ps| {
                    ps.get(p)
                        .and_then(|p| p.tasks.get(t))
                        .filter(|task| task.due != date)
                        .map(|task| task.title.clone())
                });
                if let Some(title) = moved {
                    projects.update(|ps| ps[p].tasks[t].due = date);
                    log(format!(
                        "due date of [b]{}[/] moved to {} {}",
                        escape(&title),
                        date.day,
                        date.month_name()
                    ));
                }
            },
        );

        // Mark the selected task done or not.
        let toggle = move |cx: &mut Ctx| {
            let (p, t) = at(&selected.get_untracked());
            let Some(t) = t else { return };
            projects.update(|ps| ps[p].tasks[t].done = !ps[p].tasks[t].done);
            let (title, done) =
                projects.with_untracked(|ps| (ps[p].tasks[t].title.clone(), ps[p].tasks[t].done));
            let what = if done { "done" } else { "open again" };
            log(format!("[b]{}[/] {what}", escape(&title)));
            cx.toast(format!("[green]✓[/] {} {what}", escape(&title)));
        };
        // A task added to the selected project, a week after the date shown.
        let add = move |cx: &mut Ctx| {
            let (p, _) = at(&selected.get_untracked());
            let date = due.get_untracked().add_days(7);
            let count = projects.with_untracked(|ps| ps[p].tasks.len());
            projects.update(|ps| {
                ps[p].tasks.push(Task {
                    title: format!("New task {}", count + 1),
                    due: date,
                    done: false,
                })
            });
            expanded.update(|e| {
                e.insert(vec![p]);
            });
            selected.set(vec![p, count]);
            let name = projects.with_untracked(|ps| ps[p].name.clone());
            log(format!("task added to [b]{}[/]", escape(&name)));
            cx.toast(format!("Added to {}", escape(&name)));
        };

        let bar = menu_bar(vec![
            Menu::new(
                "File",
                vec![
                    MenuItem::new("New task", add).hint("n"),
                    MenuItem::separator(),
                    MenuItem::new("Quit", |cx| cx.quit()).hint("q"),
                ],
            ),
            Menu::new(
                "Task",
                vec![MenuItem::new("Done / open again", toggle).hint("space")],
            ),
            Menu::new(
                "Help",
                vec![
                    MenuItem::new("Keys", |cx| cx.help()).hint("?"),
                    MenuItem::new("Commands", |cx| cx.palette()).hint("ctrl+p"),
                ],
            ),
        ]);
        let bar_id = bar.id();

        let items = move || {
            projects.with(|ps| {
                ps.iter()
                    .map(|p| {
                        let open = p.tasks.iter().filter(|t| !t.done).count();
                        TreeItem::new(format!("[b]{}[/] [muted]{open}[/]", escape(&p.name)))
                            .children(p.tasks.iter().map(|t| {
                                let mark = if t.done {
                                    "[green]✓[/]"
                                } else {
                                    "[dim]·[/]"
                                };
                                TreeItem::new(format!("{mark} {}", escape(&t.title)))
                            }))
                    })
                    .collect()
            })
        };
        // The tree has the focus when the planner opens, not the menu bar
        // above it.
        let projects_tree = tree_with(items, selected, expanded)
            .autofocus()
            .panel("Projects");

        let details = text(move || {
            let (p, t) = at(&selected.get());
            projects.with(|ps| {
                let Some(project) = ps.get(p) else {
                    return String::new();
                };
                match t.and_then(|t| project.tasks.get(t)) {
                    Some(task) => format!(
                        "[b]{}[/]\n[muted]{}[/]\n\n{}",
                        escape(&task.title),
                        escape(&project.name),
                        if task.done {
                            "[green]done[/]"
                        } else {
                            "[yellow]open[/]"
                        }
                    ),
                    None => format!(
                        "[b]{}[/]\n[muted]{} tasks, {} open[/]",
                        escape(&project.name),
                        project.tasks.len(),
                        project.tasks.iter().filter(|t| !t.done).count()
                    ),
                }
            })
        });
        let due_panel = row([
            calendar_with(due, None).fixed(22),
            details.padding(0, 1).flex(1),
        ])
        .panel("Due");

        let activity = virtual_list(
            move || HISTORY + events.with(Vec::len),
            move |i| {
                if i < HISTORY {
                    history_row(i)
                } else {
                    events.with(|e| e[i - HISTORY].clone())
                }
            },
            log_selected,
        )
        .panel("Activity");

        let right = vsplit(due_panel, activity, signal(0.55));
        let body = hsplit(projects_tree, right, signal(0.34));
        let status = text(move || {
            let open: usize = projects.with(|ps| {
                ps.iter()
                    .map(|p| p.tasks.iter().filter(|t| !t.done).count())
                    .sum()
            });
            format!(
                "[muted]{open} open · {} log rows[/]  [dim]? keys · ctrl+p commands · f10 menu[/]",
                HISTORY + events.with(Vec::len)
            )
        });

        column([bar.fixed(1), body.flex(1), status.fixed(1)])
            .bind("space", "mark the task done or open", toggle)
            .bind("n", "add a task to the project", add)
            .bind("f10", "open the menus", move |cx| cx.focus(bar_id))
            .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    planner_app().run()
}
