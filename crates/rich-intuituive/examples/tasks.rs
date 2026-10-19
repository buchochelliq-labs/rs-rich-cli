//! A Taskwarrior front end in the style of
//! [taskwarrior-tui](https://github.com/kdheepak/taskwarrior-tui), rebuilt
//! on intuiTUIve: its view tabs (Tasks, Projects, Timesheet, Calendar) with
//! the report and context in the navigation bar, the task report (the
//! columns of Taskwarrior's `next` report, coloured by Taskwarrior's default
//! colour rules and sorted by urgency) over the selected task's details, the
//! two-line command bar where filters, new tasks, modifications and
//! annotations are typed, the confirm prompts, marks and bulk actions,
//! undo, the context and report menus, the calendar of due dates and the
//! help.
//!
//! Ported from taskwarrior-tui (<https://github.com/kdheepak/taskwarrior-tui>),
//! by Dheepak Krishnamurthy and its contributors, MIT licence. Its layout,
//! keys, strings and colours are theirs, and the report columns, urgency
//! coefficients and colour rules are Taskwarrior's defaults; this rebuild
//! reuses none of their code. taskwarrior-tui runs the `task` command for
//! everything; this one talks to a [`Store`]. The store here is
//! [`JsonlStore`], a small Taskwarrior-like store in a directory (tasks as
//! the JSON objects `task export` writes, one a line, an undo log beside
//! them), with urgency computed as Taskwarrior computes it for the
//! attributes it supports. A store over the real `task` command is the
//! report's "Taskwarrior backend plugin".
//!
//!     cargo run -p rs-rich-intuituive --example tasks --features rich-ext/data [-- DIR]
//!
//! DIR defaults to `$TMPDIR/intuituive-tasks`, filled with a few tasks the
//! first time.
//!
//! taskwarrior-tui's default keys. Views: `]` and `[` the next and previous
//! view, `?` the help, `q` quits. Report: j/k (↓/↑) move, J/K a page, g/G
//! the top and the bottom; `/` filters (Taskwarrior's syntax: `project:x`,
//! `+tag`, `-tag`, `status:`, `priority:`, `due.before:`, ids, words,
//! `or`, `not` and parentheses); `a` adds, `l` logs a done task, `m`
//! modifies, `A` annotates, `d` marks done,
//! `x` deletes (asked first), `s` starts or stops, `t` toggles `+next`,
//! H/M/L/N set the priority, `y` duplicates, `u` undoes (asked first), `v`
//! marks a task and `V` every task (actions then apply to the marked
//! ones), `:` jumps to an id, `z` hides the details, `\` puts them beside
//! the report, Ctrl+E/Ctrl+Y scroll them, `c` opens the context menu and
//! `R` the report menu, `r` reloads. In the command bar, Enter runs, Esc
//! cancels, Tab completes attributes (`project:` values, tags, dates) in a
//! pop-up, the top one's rest shown as ghost text, and ↑/↓ go through
//! what was typed before for the same action.
//! Calendar: j/k a year, J/K ten.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intuituive::command_line::Mode;
use intuituive::complete::{word_before, Completion};
use intuituive::prelude::*;
use intuituive::query::{self, Grammar, Kind, Query, Record, TextMatch, Value};
use intuituive::reactive::NodeId;
use intuituive::rich::markup::escape;
use intuituive::widgets::{
    table_with, tabs, year_view_with, CalendarOptions, Column, Date, HighlightSpacing, MonthTitle,
    TableLayout, TableOptions, Weekday, YearOptions,
};
use intuituive::{
    command_line_with, dates, keyed, multi_select, CommandLine, CommandLineOptions, HelpOptions,
    MultiSelect,
};
use rich_ext::data::{parse_json, Node as Json, Value as JsonValue};
use rich_interact::dates::DateOptions;
use rich_interact::datetime::{DateTime, Zone};

// ---------------------------------------------------------------------
// Times: seconds since the epoch, shown on a zone's clocks.
// ---------------------------------------------------------------------

/// Seconds since 1970-01-01 00:00 UTC.
pub type Time = i64;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;
const MONTH: i64 = 30 * DAY;
const YEAR: i64 = 365 * DAY;

pub fn unix_now() -> Time {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// The date on `zone`'s clocks at `ts`, and the seconds into it.
pub fn local(ts: Time, zone: &Zone) -> (Date, i64) {
    let at = DateTime::from_unix(ts, zone);
    (at.date, i64::from(at.time.seconds()))
}

/// Midnight starting `date` on `zone`'s clocks, as a time.
pub fn midnight(date: Date, zone: &Zone) -> Time {
    DateTime::from(date).to_unix(zone)
}

pub fn fmt_datetime(ts: Time, zone: &Zone) -> String {
    DateTime::from_unix(ts, zone).format(true, false)
}

pub fn fmt_date(ts: Time, zone: &Zone) -> String {
    local(ts, zone).0.to_string()
}

/// Taskwarrior's stored form: `20261019T120000Z`.
pub fn stamp(ts: Time) -> String {
    let (d, s) = local(ts, &Zone::Utc);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        d.year,
        d.month,
        d.day,
        s / HOUR,
        s % HOUR / MINUTE,
        s % MINUTE
    )
}

pub fn parse_stamp(s: &str) -> Option<Time> {
    let b = s.as_bytes();
    if b.len() != 16 || b[8] != b'T' || b[15] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let date = Date::new(n(0..4)? as i32, n(4..6)? as u32, n(6..8)? as u32);
    date.is_valid().then(|| {
        date.days() * DAY
            + n(9..11).unwrap_or(0) * HOUR
            + n(11..13).unwrap_or(0) * MINUTE
            + n(13..15).unwrap_or(0)
    })
}

/// taskwarrior-tui's vague duration from `from` to `to`: `3d`, `-2w`.
pub fn vague(from: Time, to: Time) -> String {
    let mut s = to - from;
    let minus = if s < 0 { "-" } else { "" };
    s = s.abs();
    let (n, unit) = if s >= YEAR {
        (s / YEAR, "y")
    } else if s >= MONTH * 3 {
        (s / MONTH, "mo")
    } else if s >= WEEK * 2 {
        (s / WEEK, "w")
    } else if s >= DAY {
        (s / DAY, "d")
    } else if s >= HOUR {
        (s / HOUR, "h")
    } else if s >= MINUTE {
        (s / MINUTE, "min")
    } else {
        (s, "s")
    };
    format!("{minus}{n}{unit}")
}

/// A date as Taskwarrior reads one (`today`, `eow`, `friday`, `3d`, ISO),
/// by rs-rich-interact's `dates`, weeks from Sunday (Taskwarrior's
/// default), on `zone`'s clocks.
pub fn parse_date(word: &str, now: Time, zone: &Zone) -> Option<Time> {
    let sunday = DateOptions::new().week_start(6);
    let at = dates::parse_with(word, DateTime::from_unix(now, zone), sunday).ok()?;
    Some(at.to_unix(zone))
}

// ---------------------------------------------------------------------
// Tasks: Taskwarrior's data model, for the attributes this clone keeps.
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Completed,
    Deleted,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Completed => "completed",
            Status::Deleted => "deleted",
        }
    }

    fn of(name: &str) -> Option<Status> {
        Some(match name {
            "pending" | "waiting" | "recurring" => Status::Pending,
            "completed" => Status::Completed,
            "deleted" => Status::Deleted,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub entry: Time,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub uuid: String,
    /// The working-set id: pending tasks in the store's order from 1, and
    /// 0 for the others. Given by the store when it loads.
    pub id: u64,
    pub description: String,
    pub status: Status,
    pub entry: Time,
    pub modified: Time,
    pub start: Option<Time>,
    pub end: Option<Time>,
    pub due: Option<Time>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub priority: Option<char>,
    pub annotations: Vec<Annotation>,
}

impl Task {
    pub fn new(uuid: String, description: &str, now: Time) -> Task {
        Task {
            uuid,
            id: 0,
            description: description.to_string(),
            status: Status::Pending,
            entry: now,
            modified: now,
            start: None,
            end: None,
            due: None,
            project: None,
            tags: Vec::new(),
            priority: None,
            annotations: Vec::new(),
        }
    }

    pub fn is_pending(&self) -> bool {
        self.status == Status::Pending
    }

    /// The task as `task export` writes it (without `id` and `urgency`,
    /// which are computed).
    pub fn to_json(&self) -> String {
        let s = |v: &str| Json::new(JsonValue::String(v.to_string()));
        let mut map = vec![
            ("uuid".to_string(), s(&self.uuid)),
            ("description".to_string(), s(&self.description)),
            ("status".to_string(), s(self.status.name())),
            ("entry".to_string(), s(&stamp(self.entry))),
            ("modified".to_string(), s(&stamp(self.modified))),
        ];
        for (name, at) in [("start", self.start), ("end", self.end), ("due", self.due)] {
            if let Some(at) = at {
                map.push((name.to_string(), s(&stamp(at))));
            }
        }
        if let Some(p) = &self.project {
            map.push(("project".to_string(), s(p)));
        }
        if let Some(p) = self.priority {
            map.push(("priority".to_string(), s(&p.to_string())));
        }
        if !self.tags.is_empty() {
            let tags = self.tags.iter().map(|t| s(t)).collect();
            map.push(("tags".to_string(), Json::new(JsonValue::Seq(tags))));
        }
        if !self.annotations.is_empty() {
            let notes = (self.annotations.iter())
                .map(|a| {
                    Json::new(JsonValue::Map(vec![
                        ("entry".to_string(), s(&stamp(a.entry))),
                        ("description".to_string(), s(&a.description)),
                    ]))
                })
                .collect();
            map.push(("annotations".to_string(), Json::new(JsonValue::Seq(notes))));
        }
        Json::new(JsonValue::Map(map)).to_json().to_string()
    }

    pub fn from_json(line: &str) -> Result<Task, String> {
        let json = parse_json(line).map_err(|e| e.to_string())?;
        let text = |key: &str| json.get(key).and_then(Json::as_str).map(str::to_string);
        let time = |key: &str| text(key).and_then(|v| parse_stamp(&v));
        let uuid = text("uuid").ok_or("a task without a uuid")?;
        let strings = |node: Option<&Json>| match node.map(|n| &n.value) {
            Some(JsonValue::Seq(items)) => items
                .iter()
                .filter_map(|i| i.as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        };
        let annotations = match json.get("annotations").map(|n| &n.value) {
            Some(JsonValue::Seq(items)) => items
                .iter()
                .map(|a| Annotation {
                    entry: (a.get("entry").and_then(Json::as_str))
                        .and_then(parse_stamp)
                        .unwrap_or(0),
                    description: (a.get("description").and_then(Json::as_str))
                        .unwrap_or("")
                        .to_string(),
                })
                .collect(),
            _ => Vec::new(),
        };
        let entry = time("entry").unwrap_or(0);
        Ok(Task {
            uuid,
            id: 0,
            description: text("description").unwrap_or_default(),
            status: text("status")
                .and_then(|s| Status::of(&s))
                .unwrap_or(Status::Pending),
            entry,
            modified: time("modified").unwrap_or(entry),
            start: time("start"),
            end: time("end"),
            due: time("due"),
            project: text("project"),
            tags: strings(json.get("tags")),
            priority: text("priority").and_then(|p| p.chars().next()),
            annotations,
        })
    }
}

// ---------------------------------------------------------------------
// Urgency, as Taskwarrior computes it for the attributes kept here.
// ---------------------------------------------------------------------

/// Taskwarrior's default urgency coefficients.
pub const COEFFICIENTS: [(&str, f64); 9] = [
    ("next", 15.0),
    ("due", 12.0),
    ("priority", 1.0),
    ("active", 4.0),
    ("age", 2.0),
    ("annotations", 1.0),
    ("tags", 1.0),
    ("project", 1.0),
    ("scheduled", 5.0),
];

fn count_factor(n: usize) -> f64 {
    match n {
        0 => 0.0,
        1 => 0.8,
        2 => 0.9,
        _ => 1.0,
    }
}

/// The terms of a task's urgency: `(name, factor, coefficient)` for each
/// that is not zero, in the order `task info` lists them.
pub fn urgency_terms(task: &Task, now: Time) -> Vec<(&'static str, f64, f64)> {
    let mut terms = Vec::new();
    if task.project.is_some() {
        terms.push(("project", 1.0, 1.0));
    }
    if task.start.is_some() {
        terms.push(("active", 1.0, 4.0));
    }
    if task.tags.iter().any(|t| t == "next") {
        terms.push(("next", 1.0, 15.0));
    }
    if let Some(due) = task.due {
        let overdue = (now - due) as f64 / DAY as f64;
        let factor = if overdue >= 7.0 {
            1.0
        } else if overdue >= -14.0 {
            (overdue + 14.0) * 0.8 / 21.0 + 0.2
        } else {
            0.2
        };
        terms.push(("due", factor, 12.0));
    }
    if let Some(p) = task.priority {
        let c = match p {
            'H' => 6.0,
            'M' => 3.9,
            _ => 1.8,
        };
        terms.push(("priority", 1.0, c));
    }
    if !task.tags.is_empty() {
        terms.push(("tags", count_factor(task.tags.len()), 1.0));
    }
    if !task.annotations.is_empty() {
        terms.push(("annotations", count_factor(task.annotations.len()), 1.0));
    }
    let age = (now - task.entry) as f64 / DAY as f64 / 365.0;
    if age > 0.0 {
        terms.push(("age", age.min(1.0), 2.0));
    }
    terms
}

pub fn urgency(task: &Task, now: Time) -> f64 {
    urgency_terms(task, now).iter().map(|(_, f, c)| f * c).sum()
}

/// Taskwarrior's virtual tags, for the ones this clone can tell.
pub fn virtual_tags(task: &Task, now: Time, zone: &Zone) -> Vec<&'static str> {
    let mut tags = Vec::new();
    let pending = task.is_pending();
    if task.start.is_some() && pending {
        tags.push("ACTIVE");
    }
    if !task.annotations.is_empty() {
        tags.push("ANNOTATED");
    }
    if task.status == Status::Completed {
        tags.push("COMPLETED");
    }
    if task.status == Status::Deleted {
        tags.push("DELETED");
    }
    if let (Some(due), true) = (task.due, pending) {
        if due <= now + 7 * DAY {
            tags.push("DUE");
        }
        if local(due, zone).0 == local(now, zone).0 {
            tags.push("DUETODAY");
        }
        if due < now {
            tags.push("OVERDUE");
        }
    }
    if pending {
        tags.push("PENDING");
    }
    if task.priority.is_some() {
        tags.push("PRIORITY");
    }
    if task.project.is_some() {
        tags.push("PROJECT");
    }
    if pending {
        tags.push("READY");
    }
    if !task.tags.is_empty() {
        tags.push("TAGGED");
    }
    if let (Some(due), true) = (task.due, pending) {
        if local(due, zone).0 == local(now, zone).0 {
            tags.push("TODAY");
        }
    }
    tags.push("UNBLOCKED");
    tags
}

// ---------------------------------------------------------------------
// The store: where the tasks live.
// ---------------------------------------------------------------------

/// Where the tasks are kept: Taskwarrior's data, read and changed. A
/// store over the `task` command would be another one.
pub trait Store {
    /// Every task, pending, completed and deleted, pending ones numbered.
    fn load(&mut self) -> Result<Vec<Task>, String>;
    /// Add or replace (by uuid) these tasks, as one change `undo` reverts.
    fn commit(&mut self, changes: Vec<Task>) -> Result<(), String>;
    /// Revert the last change; how many tasks it touched.
    fn undo(&mut self) -> Result<usize, String>;
    /// The contexts defined: `(name, filter)`.
    fn contexts(&self) -> Vec<(String, String)>;
    /// The context set, if any.
    fn context(&self) -> Option<String>;
    fn set_context(&mut self, name: Option<String>) -> Result<(), String>;
    /// A new uuid for a new task.
    fn new_uuid(&mut self) -> String;
}

/// A Taskwarrior-like store in a directory: `tasks.jsonl` (each task as
/// `task export` writes it, one a line), `undo.jsonl` (each change's
/// tasks as they were before it) and `taskrc` (`context.NAME=FILTER`
/// lines and `context=NAME`).
pub struct JsonlStore {
    dir: PathBuf,
    tasks: Vec<Task>,
    seed: u64,
}

impl JsonlStore {
    /// The store in `dir`, made if missing. `seed` starts the uuids.
    pub fn open(dir: &Path, seed: u64) -> Result<JsonlStore, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(JsonlStore {
            dir: dir.to_path_buf(),
            tasks: Vec::new(),
            seed: seed | 1,
        })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read_lines(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.path(name))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect()
    }

    fn write_lines(&self, name: &str, lines: &[String]) -> Result<(), String> {
        let mut text = lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        std::fs::write(self.path(name), text).map_err(|e| format!("{name}: {e}"))
    }

    fn save(&self) -> Result<(), String> {
        let lines: Vec<String> = self.tasks.iter().map(Task::to_json).collect();
        self.write_lines("tasks.jsonl", &lines)
    }

    fn number(&mut self) {
        let mut id = 0;
        for task in &mut self.tasks {
            task.id = if task.is_pending() {
                id += 1;
                id
            } else {
                0
            };
        }
    }

    fn rc(&self) -> Vec<(String, String)> {
        self.read_lines("taskrc")
            .iter()
            .filter_map(|l| {
                let (k, v) = l.split_once('=')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect()
    }
}

impl Store for JsonlStore {
    fn load(&mut self) -> Result<Vec<Task>, String> {
        let lines = self.read_lines("tasks.jsonl");
        self.tasks = (lines.iter())
            .enumerate()
            .map(|(n, l)| Task::from_json(l).map_err(|e| format!("tasks.jsonl:{}: {e}", n + 1)))
            .collect::<Result<_, _>>()?;
        self.number();
        Ok(self.tasks.clone())
    }

    fn commit(&mut self, changes: Vec<Task>) -> Result<(), String> {
        let mut before = Vec::new();
        for task in changes {
            match self.tasks.iter_mut().find(|t| t.uuid == task.uuid) {
                Some(old) => {
                    before.push(old.to_json());
                    *old = task;
                }
                None => {
                    before.push(format!("{{\"uuid\":\"{}\",\"new\":true}}", task.uuid));
                    self.tasks.push(task);
                }
            }
        }
        let mut undo = self.read_lines("undo.jsonl");
        undo.push(format!("[{}]", before.join(",")));
        self.write_lines("undo.jsonl", &undo)?;
        self.number();
        self.save()
    }

    fn undo(&mut self) -> Result<usize, String> {
        let mut undo = self.read_lines("undo.jsonl");
        let last = undo.pop().ok_or("No undo information available")?;
        let json = parse_json(&last).map_err(|e| e.to_string())?;
        let JsonValue::Seq(items) = &json.value else {
            return Err("undo.jsonl: not a list".into());
        };
        for item in items {
            let uuid = item.get("uuid").and_then(Json::as_str).unwrap_or("");
            if item.get("new").is_some() {
                self.tasks.retain(|t| t.uuid != uuid);
            } else {
                let old = Task::from_json(&item.to_json().to_string())?;
                if let Some(t) = self.tasks.iter_mut().find(|t| t.uuid == uuid) {
                    *t = old;
                }
            }
        }
        self.write_lines("undo.jsonl", &undo)?;
        self.number();
        self.save()?;
        Ok(items.len())
    }

    fn contexts(&self) -> Vec<(String, String)> {
        (self.rc().into_iter())
            .filter_map(|(k, v)| {
                let name = k.strip_prefix("context.")?;
                Some((name.trim_end_matches(".read").to_string(), v))
            })
            .collect()
    }

    fn context(&self) -> Option<String> {
        (self.rc().into_iter())
            .find(|(k, _)| k == "context")
            .map(|(_, v)| v)
            .filter(|v| !v.is_empty())
    }

    fn set_context(&mut self, name: Option<String>) -> Result<(), String> {
        let mut lines: Vec<String> = (self.read_lines("taskrc").into_iter())
            .filter(|l| l.split_once('=').map(|(k, _)| k.trim()) != Some("context"))
            .collect();
        if let Some(name) = name {
            lines.push(format!("context={name}"));
        }
        self.write_lines("taskrc", &lines)
    }

    fn new_uuid(&mut self) -> String {
        // xorshift64*, two draws: a version 4 uuid's shape.
        let mut next = || {
            self.seed ^= self.seed >> 12;
            self.seed ^= self.seed << 25;
            self.seed ^= self.seed >> 27;
            self.seed.wrapping_mul(0x2545_F491_4F6C_DD1D)
        };
        let (a, b) = (next(), next());
        format!(
            "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
            a >> 32,
            (a >> 16) & 0xffff,
            a & 0xfff,
            ((b >> 48) & 0x3fff) | 0x8000,
            b & 0xffff_ffff_ffff
        )
    }
}

// ---------------------------------------------------------------------
// Filters and modifications: Taskwarrior's command line.
// ---------------------------------------------------------------------

/// Split a command line into words, double quotes keeping spaces.
pub fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out
}

fn priority_of(value: &str) -> Result<Option<char>, String> {
    match value {
        "" => Ok(None),
        "H" | "M" | "L" => Ok(value.chars().next()),
        _ => Err(format!(
            "The 'priority' attribute does not allow a value of '{value}'."
        )),
    }
}

fn ids_of(word: &str) -> Option<Vec<u64>> {
    let mut ids = Vec::new();
    for part in word.split(',') {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b): (u64, u64) = (a.parse().ok()?, b.parse().ok()?);
                ids.extend(a..=b);
            }
            None => ids.push(part.parse().ok()?),
        }
    }
    Some(ids)
}

/// A task as the query language sees it: its fields, its description
/// and annotations for bare words, and its tags with Taskwarrior's
/// virtual ones (`+OVERDUE`).
pub struct TaskRecord<'a> {
    pub task: &'a Task,
    pub now: Time,
    pub zone: &'a Zone,
}

impl Record for TaskRecord<'_> {
    fn field(&self, name: &str) -> Option<Value> {
        let task = self.task;
        let date = |t: Option<Time>| t.map(|t| Value::Date(DateTime::from_unix(t, self.zone)));
        match name {
            "id" => (task.id != 0).then_some(Value::Number(task.id as f64)),
            "uuid" => Some(Value::from(task.uuid.as_str())),
            "description" => Some(Value::Text(self.text())),
            "status" => Some(Value::from(task.status.name())),
            "project" => task.project.clone().map(Value::Text),
            "priority" => task.priority.map(|p| Value::Text(p.to_string())),
            "due" => date(task.due),
            "start" => date(task.start),
            "end" => date(task.end),
            "entry" => date(Some(task.entry)),
            "modified" => date(Some(task.modified)),
            "urgency" => Some(Value::Number(urgency(task, self.now))),
            _ => None,
        }
    }

    fn text(&self) -> String {
        let notes = self.task.annotations.iter().map(|a| a.description.as_str());
        std::iter::once(self.task.description.as_str())
            .chain(notes)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn tags(&self) -> Vec<String> {
        self.task.tags.clone()
    }

    fn has_tag(&self, tag: &str, _case_sensitive: bool) -> bool {
        self.task.tags.iter().any(|t| t == tag)
            || virtual_tags(self.task, self.now, self.zone).contains(&tag)
    }
}

/// Taskwarrior's filter syntax as a grammar for intuiTUIve's query
/// language: its attributes and their short names, `project:` matching
/// subprojects, dates as Taskwarrior names them relative to `now`, ids
/// (`2,4-5`) as words of their own, an unknown `name:value` a word.
pub fn grammar(now: Time, zone: &Zone) -> Grammar {
    let zone = zone.clone();
    Grammar::new()
        .field("id", Kind::Number)
        .field("uuid", Kind::Text)
        .matching(TextMatch::Prefix)
        .field("description", Kind::Text)
        .matching(TextMatch::Contains)
        .field("status", Kind::Text)
        .read(|v| Status::of(v).map(|s| Value::from(s.name())))
        .field("project", Kind::Text)
        .matching(TextMatch::Prefix)
        .alias("pro", "project")
        .alias("proj", "project")
        .field("priority", Kind::Text)
        .read(|v| priority_of(v).ok().flatten().map(|p| Value::Text(p.to_string())))
        .alias("pri", "priority")
        .field("urgency", Kind::Number)
        .alias("urg", "urgency")
        .field("due", Kind::Date)
        .field("start", Kind::Date)
        .field("end", Kind::Date)
        .field("entry", Kind::Date)
        .field("modified", Kind::Date)
        .dates(move |word| {
            let at = parse_date(word, now, &zone)?;
            Some(DateTime::from_unix(at, &zone))
        })
        .words(|word| {
            let ids = ids_of(word)?;
            Some(Rc::new(move |record: &dyn Record| {
                matches!(record.field("id"), Some(Value::Number(id)) if ids.contains(&(id as u64)))
            }))
        })
        .case_sensitive(true)
        .lenient()
}

/// A filter as Taskwarrior reads one: `query::parse` by [`grammar`].
pub fn parse_filter(text: &str, now: Time, zone: &Zone) -> Result<Query, String> {
    query::parse(text, &grammar(now, zone)).map_err(|e| e.message)
}

/// Whether `task` passes `filter`.
pub fn passes(filter: &Query, task: &Task, now: Time, zone: &Zone) -> bool {
    filter.matches(&TaskRecord { task, now, zone })
}

/// Apply `task add`/`modify` words to `task`: attributes set, tags added
/// or removed, the other words its description (replaced when any).
pub fn modify(task: &mut Task, line: &str, now: Time, zone: &Zone) -> Result<(), String> {
    let mut description = Vec::new();
    for word in words(line) {
        if let Some(tag) = word.strip_prefix('+').filter(|t| !t.is_empty()) {
            if !task.tags.iter().any(|t| t == tag) {
                task.tags.push(tag.to_string());
            }
        } else if let Some(tag) = word
            .strip_prefix('-')
            .filter(|t| !t.is_empty() && !t.contains(' '))
        {
            task.tags.retain(|t| t != tag);
        } else if let Some((name, value)) = word
            .split_once(':')
            .filter(|(n, _)| matches!(*n, "project" | "pro" | "proj" | "priority" | "pri" | "due"))
        {
            match name {
                "priority" | "pri" => task.priority = priority_of(value)?,
                "due" if value.is_empty() => task.due = None,
                "due" => {
                    task.due = Some(
                        parse_date(value, now, zone)
                            .ok_or_else(|| format!("'{value}' is not a valid date."))?,
                    )
                }
                _ if value.is_empty() => task.project = None,
                _ => task.project = Some(value.to_string()),
            }
        } else {
            description.push(word);
        }
    }
    if !description.is_empty() {
        task.description = description.join(" ");
    }
    task.modified = now;
    Ok(())
}

// ---------------------------------------------------------------------
// Reports: columns, sorting and Taskwarrior's default colours.
// ---------------------------------------------------------------------

/// A report column, as Taskwarrior names them (`id`, `entry.age`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    Id,
    Status,
    Uuid,
    /// `start.age`: how long it has been active.
    Active,
    Started,
    Age,
    Done,
    Priority,
    Project,
    Tags,
    Due,
    /// `due.relative`: `3d`, `-2d`.
    DueRel,
    Entry,
    End,
    Description,
    Urgency,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Urgency,
    Start,
    Due,
    Project,
    Entry,
    End,
}

/// A report: its columns with their labels, and its sort (`true` for
/// descending).
pub struct Report {
    pub name: &'static str,
    pub description: &'static str,
    pub filter: &'static str,
    pub columns: &'static [(Col, &'static str)],
    pub sort: &'static [(Sort, bool)],
}

/// Taskwarrior's default reports, for the columns kept here.
pub const REPORTS: [Report; 6] = [
    Report {
        name: "next",
        description: "Most urgent tasks",
        filter: "status:pending",
        columns: &[
            (Col::Id, "ID"),
            (Col::Active, "Active"),
            (Col::Age, "Age"),
            (Col::Priority, "P"),
            (Col::Project, "Project"),
            (Col::Tags, "Tag"),
            (Col::DueRel, "Due"),
            (Col::Description, "Description"),
            (Col::Urgency, "Urg"),
        ],
        sort: &[(Sort::Urgency, true)],
    },
    Report {
        name: "list",
        description: "Most details of tasks",
        filter: "status:pending",
        columns: &[
            (Col::Id, "ID"),
            (Col::Active, "Active"),
            (Col::Age, "Age"),
            (Col::Priority, "P"),
            (Col::Project, "Project"),
            (Col::Tags, "Tags"),
            (Col::Due, "Due"),
            (Col::Description, "Description"),
            (Col::Urgency, "Urg"),
        ],
        sort: &[
            (Sort::Start, true),
            (Sort::Due, false),
            (Sort::Project, false),
            (Sort::Urgency, true),
        ],
    },
    Report {
        name: "active",
        description: "Active tasks",
        filter: "status:pending +ACTIVE",
        columns: &[
            (Col::Id, "ID"),
            (Col::Started, "Started"),
            (Col::Active, "Active"),
            (Col::Age, "Age"),
            (Col::Priority, "P"),
            (Col::Project, "Project"),
            (Col::Due, "Due"),
            (Col::Description, "Description"),
        ],
        sort: &[(Sort::Project, false), (Sort::Start, false)],
    },
    Report {
        name: "overdue",
        description: "Overdue tasks",
        filter: "status:pending +OVERDUE",
        columns: &[
            (Col::Id, "ID"),
            (Col::Active, "Active"),
            (Col::Age, "Age"),
            (Col::Priority, "P"),
            (Col::Project, "Project"),
            (Col::Tags, "Tag"),
            (Col::Due, "Due"),
            (Col::Description, "Description"),
            (Col::Urgency, "Urg"),
        ],
        sort: &[(Sort::Urgency, true), (Sort::Due, false)],
    },
    Report {
        name: "completed",
        description: "Completed tasks",
        filter: "status:completed",
        columns: &[
            (Col::Id, "ID"),
            (Col::Uuid, "UUID"),
            (Col::Entry, "Created"),
            (Col::End, "Completed"),
            (Col::Age, "Age"),
            (Col::Priority, "P"),
            (Col::Project, "Project"),
            (Col::Tags, "Tags"),
            (Col::Due, "Due"),
            (Col::Description, "Description"),
        ],
        sort: &[(Sort::End, false)],
    },
    Report {
        name: "all",
        description: "All tasks",
        filter: "",
        columns: &[
            (Col::Id, "ID"),
            (Col::Status, "St"),
            (Col::Uuid, "UUID"),
            (Col::Age, "Age"),
            (Col::Done, "Done"),
            (Col::Priority, "P"),
            (Col::Project, "Project"),
            (Col::Tags, "Tags"),
            (Col::Due, "Due"),
            (Col::Description, "Description"),
        ],
        sort: &[(Sort::Entry, true)],
    },
];

/// A task's cell in column `col`, as plain text.
pub fn cell(task: &Task, col: Col, now: Time, zone: &Zone) -> String {
    let age = |t: Option<Time>| t.map(|t| vague(t, now)).unwrap_or_default();
    match col {
        Col::Id if task.id == 0 => "-".to_string(),
        Col::Id => task.id.to_string(),
        Col::Status => task.status.name()[..1].to_ascii_uppercase(),
        Col::Uuid => task.uuid.chars().take(8).collect(),
        Col::Active => age(task.start.filter(|_| task.is_pending())),
        Col::Started => task.start.map(|t| fmt_date(t, zone)).unwrap_or_default(),
        Col::Age => vague(task.entry, now),
        Col::Done => age(task.end),
        Col::Priority => task.priority.map(String::from).unwrap_or_default(),
        Col::Project => task.project.clone().unwrap_or_default(),
        Col::Tags => task.tags.join(" "),
        Col::Due => task.due.map(|t| fmt_date(t, zone)).unwrap_or_default(),
        Col::DueRel => task.due.map(|t| vague(now, t)).unwrap_or_default(),
        Col::Entry => fmt_date(task.entry, zone),
        Col::End => task.end.map(|t| fmt_date(t, zone)).unwrap_or_default(),
        Col::Description if task.annotations.is_empty() => task.description.clone(),
        Col::Description => format!("{} [{}]", task.description, task.annotations.len()),
        Col::Urgency => format!("{:.2}", urgency(task, now)),
    }
}

fn sort_tasks(tasks: &mut [Task], sort: &[(Sort, bool)], now: Time) {
    let key = |t: &Task, s: Sort| -> (bool, f64, String) {
        match s {
            Sort::Urgency => (false, urgency(t, now), String::new()),
            Sort::Start => (
                t.start.is_none(),
                t.start.unwrap_or(0) as f64,
                String::new(),
            ),
            Sort::Due => (t.due.is_none(), t.due.unwrap_or(0) as f64, String::new()),
            Sort::Project => (
                t.project.is_none(),
                0.0,
                t.project.clone().unwrap_or_default(),
            ),
            Sort::Entry => (false, t.entry as f64, String::new()),
            Sort::End => (t.end.is_none(), t.end.unwrap_or(0) as f64, String::new()),
        }
    };
    tasks.sort_by(|a, b| {
        for &(s, descending) in sort {
            let (ka, kb) = (key(a, s), key(b, s));
            // A task without the value goes last either way.
            let order = ka.0.cmp(&kb.0).then_with(|| {
                let o = ka.1.total_cmp(&kb.1).then_with(|| ka.2.cmp(&kb.2));
                if descending {
                    o.reverse()
                } else {
                    o
                }
            });
            if order.is_ne() {
                return order;
            }
        }
        a.id.cmp(&b.id)
    });
}

/// The style Taskwarrior's default colour rules give a task (its
/// `rule.precedence.color`, the earlier rules winning): active
/// `rgb555 on rgb410`, overdue `color9`, due today and due `color1`,
/// tagged `color2`, and the priorities `color255`, `color250`, `color245`.
pub fn task_colours(
    task: &Task,
    now: Time,
    zone: &Zone,
) -> (Option<&'static str>, Option<&'static str>) {
    let virt = virtual_tags(task, now, zone);
    let has = |t: &str| virt.contains(&t);
    let mut fg = None;
    let mut bg = None;
    if let Some(p) = task.priority {
        fg = Some(match p {
            'H' => "color(255)",
            'M' => "color(250)",
            _ => "color(245)",
        });
    }
    if has("TAGGED") {
        fg = Some("color(2)");
    }
    if has("DUE") || has("DUETODAY") {
        fg = Some("color(1)");
    }
    if has("OVERDUE") {
        fg = Some("color(9)");
    }
    if has("ACTIVE") {
        fg = Some("color(231)");
        bg = Some("color(166)");
    }
    (fg, bg)
}

/// A row's style: the task's colours over Taskwarrior's alternate rows
/// (`on gray2`).
pub fn row_style(task: &Task, row: usize, now: Time, zone: &Zone) -> Option<String> {
    let (fg, bg) = task_colours(task, now, zone);
    let bg = bg.or((row % 2 == 1).then_some("color(234)"));
    match (fg, bg) {
        (None, None) => None,
        (Some(f), None) => Some(f.to_string()),
        (None, Some(b)) => Some(format!("on {b}")),
        (Some(f), Some(b)) => Some(format!("{f} on {b}")),
    }
}

/// `task info`: the task's attributes, then its urgency's terms.
pub fn info(task: &Task, now: Time, zone: &Zone) -> String {
    let when = |t: Time| format!("{} ({})", fmt_datetime(t, zone), vague(t, now));
    let mut rows: Vec<(&str, String)> = Vec::new();
    if task.id != 0 {
        rows.push(("ID", task.id.to_string()));
    }
    let mut description = escape(&task.description);
    for a in &task.annotations {
        description.push_str(&format!(
            "\n  {} {}",
            fmt_datetime(a.entry, zone),
            escape(&a.description)
        ));
    }
    rows.push(("Description", description));
    let status = task.status.name();
    rows.push((
        "Status",
        format!("{}{}", status[..1].to_uppercase(), &status[1..]),
    ));
    if let Some(p) = &task.project {
        rows.push(("Project", escape(p)));
    }
    if let Some(p) = task.priority {
        rows.push(("Priority", p.to_string()));
    }
    rows.push(("Entered", when(task.entry)));
    if let Some(t) = task.start {
        rows.push(("Start", fmt_datetime(t, zone)));
    }
    if let Some(t) = task.end {
        rows.push(("End", fmt_datetime(t, zone)));
    }
    if let Some(t) = task.due {
        rows.push(("Due", fmt_datetime(t, zone)));
    }
    rows.push(("Last modified", when(task.modified)));
    if !task.tags.is_empty() {
        rows.push(("Tags", escape(&task.tags.join(" "))));
    }
    rows.push(("Virtual tags", virtual_tags(task, now, zone).join(" ")));
    rows.push(("UUID", task.uuid.clone()));
    rows.push(("Urgency", format!("{:5.3}", urgency(task, now))));
    let width = rows.iter().map(|(n, _)| n.len()).max().unwrap_or(4);
    let mut out = format!(
        "[underline]Name[/]{} [underline]Value[/]\n",
        " ".repeat(width - 4)
    );
    for (name, value) in rows {
        let mut lines = value.lines();
        out.push_str(&format!("{name:width$} {}\n", lines.next().unwrap_or("")));
        for line in lines {
            out.push_str(&format!("{:width$} {line}\n", ""));
        }
    }
    out.push('\n');
    let terms = urgency_terms(task, now);
    for (name, factor, coefficient) in &terms {
        out.push_str(&format!(
            "    {name:<12}{:>7} * {:>5} = {:>7}\n",
            trim_float(*factor),
            trim_float(*coefficient),
            trim_float(factor * coefficient)
        ));
    }
    out.push_str(&format!(
        "{:>44}\n{:>44}",
        "------",
        trim_float(urgency(task, now))
    ));
    out
}

fn trim_float(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `task projects`: the projects of pending tasks as a tree, with the
/// tasks in each (its subprojects' counted in).
pub fn projects_text(tasks: &[Task]) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut none = 0;
    let mut total = 0;
    for task in tasks.iter().filter(|t| t.is_pending()) {
        total += 1;
        match &task.project {
            None => none += 1,
            Some(p) => {
                let parts: Vec<&str> = p.split('.').collect();
                for n in 1..=parts.len() {
                    *counts.entry(parts[..n].join(".")).or_default() += 1;
                }
            }
        }
    }
    if total == 0 {
        return "No projects.".to_string();
    }
    let mut rows: Vec<(String, usize)> = Vec::new();
    if none > 0 {
        rows.push(("(none)".to_string(), none));
    }
    for (name, n) in &counts {
        let depth = name.matches('.').count();
        let leaf = name.rsplit('.').next().unwrap_or(name);
        rows.push((format!("{}{leaf}", "  ".repeat(depth)), *n));
    }
    let width = rows
        .iter()
        .map(|(n, _)| n.chars().count())
        .max()
        .unwrap_or(0)
        .max(7);
    let mut out = format!(
        "\n[underline]Project[/]{} [underline]Tasks[/]\n",
        " ".repeat(width - 7)
    );
    for (name, n) in &rows {
        out.push_str(&format!("{:width$} {n:>5}\n", escape(name)));
    }
    let projects = counts.len();
    out.push_str(&format!(
        "\n{projects} project{} ({total} task{})",
        if projects == 1 { "" } else { "s" },
        if total == 1 { "" } else { "s" }
    ));
    out
}

/// `task timesheet`: the tasks completed and started each week, the last
/// four weeks, the latest last.
pub fn timesheet_text(tasks: &[Task], now: Time, zone: &Zone) -> String {
    let (today, _) = local(now, zone);
    let sunday = today.add_days(-((today.weekday() as i64 + 1) % 7));
    let mut out = String::from(
        "\n[underline]Wk [/] [underline]Date      [/] [underline]Day[/] [underline]ID[/] \
         [underline]Action   [/] [underline]Project[/] [underline]Due       [/] [underline]Task[/]\n",
    );
    let (mut completed, mut started) = (0, 0);
    for back in (0..4).rev() {
        let first = sunday.add_days(-7 * back);
        let (from, to) = (midnight(first, zone), midnight(first.add_days(7), zone));
        let mut events: Vec<(Time, &Task, &str)> = Vec::new();
        for t in tasks {
            if let (Some(end), Status::Completed) = (t.end, t.status) {
                if (from..to).contains(&end) {
                    events.push((end, t, "Completed"));
                }
            }
            if let Some(start) = t.start.filter(|s| (from..to).contains(s)) {
                if t.is_pending() {
                    events.push((start, t, "Started"));
                }
            }
        }
        events.sort_by_key(|e| e.0);
        let week = (first.add_days(1).days() - Date::new(first.year, 1, 1).days()) / 7 + 1;
        let mut wk = format!("W{week:<2}");
        for (at, t, action) in events {
            if action == "Completed" {
                completed += 1;
            } else {
                started += 1;
            }
            let (date, _) = local(at, zone);
            let day = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][date.weekday() as usize];
            let id = if t.id == 0 {
                "-".to_string()
            } else {
                t.id.to_string()
            };
            out.push_str(&format!(
                "{wk:3} {date} {day} {id:>2} {action:9} {:7} {:10} {}\n",
                escape(t.project.as_deref().unwrap_or("")),
                t.due.map(|d| fmt_date(d, zone)).unwrap_or_default(),
                escape(&t.description)
            ));
            wk = String::new();
        }
    }
    out.push_str(&format!("\n{completed} completed, {started} started."));
    out
}

// ---------------------------------------------------------------------
// The app's state.
// ---------------------------------------------------------------------

/// How the app is set up.
#[derive(Clone, Debug)]
pub struct Options {
    /// A fixed "now" (tests); `None` follows the clock.
    pub now: Option<Time>,
    /// The zone whose clocks times are shown and read on.
    pub zone: Zone,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            now: None,
            zone: Zone::Local,
        }
    }
}

/// The report's tasks, filtered and sorted, or why the filter failed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub tasks: Vec<Task>,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
struct Tw {
    store: Signal<RefCell<Box<dyn Store>>>,
    tasks: Signal<Vec<Task>>,
    now: Signal<Time>,
    zone: Signal<Zone>,
    filter: Signal<String>,
    report: Signal<usize>,
    context: Signal<Option<String>>,
    contexts: Signal<Vec<(String, String)>>,
    view: Memo<View>,
    selected: Signal<usize>,
    marked: MultiSelect<String>,
    /// The command bar: taskwarrior-tui's prompts are its modes.
    cmd: CommandLine,
    /// Typing in the command bar (rather than asking yes or no).
    typing: Signal<bool>,
    /// The title over a yes-or-no question (`Delete Task 1?`).
    asking: Signal<String>,
    error: Signal<Option<String>>,
    tab: Signal<usize>,
    details: Signal<bool>,
    beside: Signal<bool>,
    detail_top: Signal<u16>,
    year: Signal<i32>,
    /// The report table's node.
    table: Signal<Option<NodeId>>,
}

pub const VIEWS: [&str; 4] = ["Tasks", "Projects", "Timesheet", "Calendar"];

impl Tw {
    fn new(store: Box<dyn Store>, options: Options) -> Tw {
        let now = signal(options.now.unwrap_or_else(unix_now));
        let zone = signal(options.zone.clone());
        let tasks = signal(Vec::<Task>::new());
        let filter = signal(format!("{} ", REPORTS[0].filter));
        let report = signal(0usize);
        let context = signal(store.context());
        let contexts = signal(store.contexts());
        let view = memo(move || {
            let now = now.get();
            let zone = zone.get();
            let report = &REPORTS[report.get()];
            let context = context.get();
            let context_filter = context
                .and_then(|c| {
                    contexts.with(|cs| cs.iter().find(|(n, _)| *n == c).map(|(_, f)| f.clone()))
                })
                .unwrap_or_default();
            let parsed = parse_filter(&filter.get(), now, &zone)
                .and_then(|f| parse_filter(&context_filter, now, &zone).map(|c| (f, c)));
            match parsed {
                Err(error) => View {
                    tasks: Vec::new(),
                    error: Some(error),
                },
                Ok((f, c)) => {
                    let mut shown: Vec<Task> = tasks.with(|all| {
                        (all.iter())
                            .filter(|t| passes(&f, t, now, &zone) && passes(&c, t, now, &zone))
                            .cloned()
                            .collect()
                    });
                    sort_tasks(&mut shown, report.sort, now);
                    View {
                        tasks: shown,
                        error: None,
                    }
                }
            }
        });
        let marked = multi_select::<String>()
            .over(move || view.with(|v| v.tasks.iter().map(|t| t.uuid.clone()).collect()));
        Tw {
            store: signal(RefCell::new(store)),
            tasks,
            now,
            zone,
            filter,
            report,
            context,
            contexts,
            view,
            selected: signal(0usize),
            marked,
            cmd: CommandLine::new(),
            typing: signal(false),
            asking: signal(String::new()),
            error: signal(None),
            tab: signal(0usize),
            details: signal(true),
            beside: signal(false),
            detail_top: signal(0u16),
            year: signal(local(now.get_untracked(), &options.zone).0.year),
            table: signal(None),
        }
    }

    fn now(&self) -> Time {
        self.now.get_untracked()
    }

    fn zone(&self) -> Zone {
        self.zone.get_untracked()
    }

    /// Read the store again. The table keeps its cursor on the selected
    /// task (`cursor_by`), as taskwarrior-tui's does when a change of
    /// urgency re-sorts the report.
    fn reload(&self) -> Result<(), String> {
        let tasks = self.store.with_untracked(|s| s.borrow_mut().load())?;
        self.tasks.set(tasks);
        let len = self.view.with_untracked(|v| v.tasks.len());
        if self.selected.get_untracked() >= len {
            self.selected.set(len.saturating_sub(1));
        }
        Ok(())
    }

    fn current(&self) -> Option<Task> {
        let at = self.selected.get_untracked();
        self.view.with_untracked(|v| v.tasks.get(at).cloned())
    }

    /// What an action acts on: the marked tasks, or else the selected one.
    fn targets(&self) -> Vec<Task> {
        let marked = self.marked.selected();
        if marked.is_empty() {
            return self.current().into_iter().collect();
        }
        self.tasks.with_untracked(|all| {
            (marked.iter())
                .filter_map(|u| all.iter().find(|t| &t.uuid == u).cloned())
                .collect()
        })
    }

    fn task_label(&self, verb: &str) -> String {
        let targets = self.targets();
        let s = if targets.len() > 1 { "s" } else { "" };
        let ids: Vec<String> = targets.iter().map(|t| t.id.to_string()).collect();
        format!("{verb} Task{s} {}", ids.join(","))
    }

    /// Change tasks in the store, as one change `u` can undo.
    fn commit(&self, cx: &mut Ctx, changes: Vec<Task>) -> bool {
        if changes.is_empty() {
            return false;
        }
        let done = self
            .store
            .with_untracked(|s| s.borrow_mut().commit(changes))
            .and_then(|_| self.reload());
        match done {
            Ok(()) => true,
            Err(e) => {
                self.fail(cx, e);
                false
            }
        }
    }

    /// Change each target with `f`, as one change.
    fn change_each(&self, cx: &mut Ctx, f: impl Fn(&mut Task, Time)) {
        let now = self.now();
        let changes: Vec<Task> = (self.targets().into_iter())
            .map(|mut t| {
                f(&mut t, now);
                t.modified = now;
                t
            })
            .collect();
        if self.commit(cx, changes) {
            self.marked.clear();
        }
    }

    /// taskwarrior-tui's error: the command bar's title `Error` in red, the
    /// message in a box until a key is pressed.
    fn fail(&self, cx: &mut Ctx, message: String) {
        let tw = *self;
        self.typing.set(false);
        self.cmd.close(cx);
        self.error.set(Some(message.clone()));
        cx.modal(Size::Percent(90), Size::Percent(60), move || {
            label(escape(&message))
                .panel("[b]Error[/]")
                .on_any_key(move |cx, _| {
                    cx.pop();
                    tw.error.set(None);
                    // The modal gives the focus back to the command line
                    // it opened over (closed by then), not the report.
                    if let (0, Some(table)) = (tw.tab.get_untracked(), tw.table.get_untracked()) {
                        cx.focus(table);
                    }
                    true
                })
        });
    }

    /// Open the command bar to type, titled `title`, starting with
    /// `initial`, completing attributes and keeping a history per action
    /// (taskwarrior-tui's); `submit` runs on Enter.
    fn edit(
        &self,
        cx: &mut Ctx,
        title: &str,
        initial: String,
        submit: impl Fn(Tw, &str, &mut Ctx) + 'static,
    ) {
        let tw = *self;
        let history = title.split(' ').next().unwrap_or(title).to_lowercase();
        let mode = Mode::new("")
            .title(format!("[b]{}[/]", escape(title)))
            .initial(initial)
            .complete(move |text, caret| tw.complete(text, caret))
            .history(history.as_str())
            .on_submit(move |text, cx| {
                tw.typing.set(false);
                submit(tw, text, cx);
                Ok(())
            })
            .on_cancel(move |_| tw.typing.set(false));
        self.typing.set(true);
        self.cmd.open(cx, mode);
    }

    /// `/`: the filter in the command bar, the report following each key,
    /// Esc putting it back.
    fn edit_filter(&self, cx: &mut Ctx) {
        let tw = *self;
        let before = self.filter.get_untracked();
        let mode = Mode::new("")
            .title("[b]Filter Tasks[/]")
            .initial(before.clone())
            .complete(move |text, caret| tw.complete(text, caret))
            .history("filter")
            .on_change(move |text, _| {
                tw.filter.set(text.to_string());
                // taskwarrior-tui's `command.error`: the title in red.
                if parse_filter(text, tw.now(), &tw.zone()).is_err() {
                    tw.cmd.error("Filter Tasks");
                }
            })
            .on_submit(move |_, _| {
                tw.typing.set(false);
                Ok(())
            })
            .on_cancel(move |_| {
                tw.typing.set(false);
                tw.filter.set(before.clone());
            });
        self.typing.set(true);
        self.cmd.open(cx, mode);
    }

    /// A yes-or-no question in the command bar, `title` over it: `key`
    /// confirms, `q` or Esc aborts.
    fn ask(&self, cx: &mut Ctx, title: String, key: &str, yes: impl FnOnce(&mut Ctx) + 'static) {
        self.asking.set(format!("[b]{}[/]", escape(&title)));
        let question = format!("Press <{key}> to confirm or <q> to abort.");
        self.cmd.confirm_keys(cx, question, key, yes);
    }

    /// Enter on a new task (`a`), or a logged one (`l`).
    fn add(&self, cx: &mut Ctx, text: &str, log: bool) {
        let (now, zone) = (self.now(), self.zone());
        let uuid = self.store.with_untracked(|s| s.borrow_mut().new_uuid());
        let mut task = Task::new(uuid, "", now);
        match modify(&mut task, text, now, &zone) {
            Err(e) => self.fail(cx, e),
            Ok(()) if task.description.trim().is_empty() => {
                self.fail(cx, "Additional text must be provided.".to_string())
            }
            Ok(()) => {
                if log {
                    task.status = Status::Completed;
                    task.end = Some(now);
                }
                self.commit(cx, vec![task]);
            }
        }
    }

    /// Enter on a modification (`m`).
    fn modify(&self, cx: &mut Ctx, text: &str) {
        let (now, zone) = (self.now(), self.zone());
        let mut changes = Vec::new();
        for mut task in self.targets() {
            if let Err(e) = modify(&mut task, text, now, &zone) {
                return self.fail(cx, e);
            }
            changes.push(task);
        }
        if self.commit(cx, changes) {
            self.marked.clear();
        }
    }

    /// What the word before the caret may complete to: `project:` values,
    /// tags, priorities, statuses, dates, or the attribute names.
    fn complete(&self, text: &str, caret: usize) -> Vec<Completion> {
        let range = word_before(text, caret);
        let word = &text[range.clone()];
        if word.is_empty() {
            return Vec::new();
        }
        let (projects, tags) = self.tasks.with_untracked(|all| {
            let p: BTreeSet<String> = all.iter().filter_map(|t| t.project.clone()).collect();
            let t: BTreeSet<String> = all.iter().flat_map(|t| t.tags.clone()).collect();
            (p, t)
        });
        let mut out: Vec<String> = match word.split_once(':') {
            Some((name, _)) => {
                let values: Vec<String> = match name {
                    "project" | "pro" | "proj" => projects.into_iter().collect(),
                    "priority" | "pri" => vec!["H".into(), "M".into(), "L".into()],
                    "status" => vec!["pending".into(), "completed".into(), "deleted".into()],
                    "due" | "due.before" | "due.after" => {
                        dates::NAMES.iter().map(|d| d.to_string()).collect()
                    }
                    _ => Vec::new(),
                };
                values.into_iter().map(|v| format!("{name}:{v}")).collect()
            }
            None if word.starts_with('+') || word.starts_with('-') => {
                let sign = &word[..1];
                tags.iter().map(|t| format!("{sign}{t}")).collect()
            }
            None => [
                "project:",
                "priority:",
                "due:",
                "status:",
                "due.before:",
                "due.after:",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        };
        out.retain(|c| c.starts_with(word) && c != word);
        out.into_iter()
            .map(|c| Completion::new(range.clone(), c))
            .collect()
    }

    /// Every column of the report, for each task: the ID cell with
    /// taskwarrior-tui's `✔` while marking.
    fn row_cells(&self) -> Vec<Vec<String>> {
        let (now, zone) = (self.now.get(), self.zone());
        let columns = REPORTS[self.report.get()].columns;
        let marking = !self.marked.is_empty();
        self.view.with(|v| {
            (v.tasks.iter())
                .map(|t| {
                    (columns.iter())
                        .map(|&(c, _)| {
                            let text = escape(&cell(t, c, now, &zone));
                            match (c, marking) {
                                (Col::Id, true)
                                    if self.marked.mark(&t.uuid) != intuituive::Mark::None =>
                                {
                                    format!("✔ {text}")
                                }
                                (Col::Id, true) => format!("  {text}"),
                                _ => text,
                            }
                        })
                        .collect()
                })
                .collect()
        })
    }

    /// The report's columns that have something in them: taskwarrior-tui
    /// drops the empty ones.
    fn visible(&self) -> Vec<usize> {
        let (now, zone) = (self.now.get(), self.zone());
        let columns = REPORTS[self.report.get()].columns;
        self.view.with(|v| {
            (0..columns.len())
                .filter(|&i| {
                    (v.tasks.iter()).any(|t| !cell(t, columns[i].0, now, &zone).is_empty())
                })
                .collect()
        })
    }

    /// The style of the tasks due on `date`, for the calendar: the first
    /// one's, in the report's order.
    fn due_style(&self, date: Date) -> Option<String> {
        let (now, zone) = (self.now.get(), self.zone());
        self.view.with(|v| {
            let task = v
                .tasks
                .iter()
                .find(|t| t.due.is_some_and(|d| local(d, &zone).0 == date))?;
            let (fg, bg) = task_colours(task, now, &zone);
            let style = match (fg, bg) {
                (Some(f), Some(b)) => format!("{f} on {b}"),
                (Some(f), None) => f.to_string(),
                (None, Some(b)) => format!("on {b}"),
                (None, None) => "underline".to_string(),
            };
            Some(style)
        })
    }
}

// ---------------------------------------------------------------------
// The views.
// ---------------------------------------------------------------------

/// The top row: the view tabs, and the report and context at the right.
fn navbar(tw: Tw) -> Node {
    row([
        tabs(|| VIEWS.iter().map(|v| v.to_string()).collect(), tw.tab)
            .divider(" ")
            .no_focus()
            .flex(1),
        text(move || {
            let context = tw.context.get().unwrap_or_else(|| "none".to_string());
            escape(&format!("{} [{}]", REPORTS[tw.report.get()].name, context))
        })
        .no_wrap()
        .auto(),
    ])
    .style("reverse")
    .theme_styles([
        ("tabs.selected", "bold reverse"),
        ("tabs.selected.blur", "bold reverse"),
        ("tabs.unselected", "reverse"),
        ("tabs.divider", "reverse"),
    ])
}

/// The task report: one table whose columns follow the report and drop
/// the empty ones, its cursor kept on the selected task as the report
/// re-sorts, `⦿`/`⦾` on the selected row while marking.
fn report_table(tw: Tw, layout: &TableLayout) -> Node {
    let columns = move || {
        (REPORTS[tw.report.get()].columns.iter())
            .map(|&(col, title)| {
                let size = match col {
                    Col::Description => Size::Flex(1),
                    _ => Size::Auto,
                };
                Column::new(title, size)
            })
            .collect()
    };
    let uuid = move |i: usize| tw.view.with(|v| v.tasks.get(i).map(|t| t.uuid.clone()));
    let options = TableOptions::default()
        .columns_with(columns)
        .columns_shown(move || tw.visible())
        .symbol("• ")
        .symbol_for(2, move |i, selected| {
            let marked = uuid(i).is_some_and(|u| tw.marked.mark(&u) != intuituive::Mark::None);
            match (selected, tw.marked.is_empty(), marked) {
                (false, _, _) | (true, true, _) => None,
                (true, false, true) => Some("⦿ ".into()),
                (true, false, false) => Some("⦾ ".into()),
            }
        })
        .highlight_spacing(HighlightSpacing::Always)
        .row_style(move |i| {
            let (now, zone) = (tw.now.get(), tw.zone());
            tw.view
                .with(|v| v.tasks.get(i).and_then(|t| row_style(t, i, now, &zone)))
        })
        .layout(layout);
    table_with(Vec::new(), move || tw.row_cells(), tw.selected, options)
        .cursor_by(move |i| uuid(i).unwrap_or_default())
}

/// The selected task's details: `task info`, under a rule.
fn details(tw: Tw) -> Node {
    scroll_with(
        text(move || {
            let (now, zone) = (tw.now.get(), tw.zone());
            let at = tw.selected.get();
            tw.view.with(|v| match v.tasks.get(at) {
                Some(t) => info(t, now, &zone),
                None => "Task not found".to_string(),
            })
        }),
        tw.detail_top,
    )
    .no_focus()
    .borders(Sides::TOP)
}

/// The two-row command bar: a command line, and over it while it is
/// closed (or asking) what taskwarrior-tui shows there: `Filter Tasks`
/// and the filter, red when the filter cannot be read, or `Error`.
fn command_bar(tw: Tw) -> Node {
    let failed = move || {
        tw.error.with(Option::is_some) || (!tw.cmd.is_open() && tw.view.with(|v| v.error.is_some()))
    };
    let title = move || {
        if tw.cmd.is_open() {
            tw.asking.get()
        } else if tw.error.with(Option::is_some) {
            "[b]Error[/]".to_string()
        } else if failed() {
            "[b]Filter Tasks[/]".to_string()
        } else {
            "Filter Tasks".to_string()
        }
    };
    // taskwarrior-tui's `command` style (reversed), or `command.error`
    // (red).
    let heading = keyed(failed, move |failed| {
        text(title)
            .no_wrap()
            .style(if failed { "red" } else { "reverse" })
    });
    let line = text(move || match tw.error.with(Option::is_some) {
        true => "Press any key to continue.".to_string(),
        false => escape(&tw.filter.get()),
    })
    .no_wrap()
    .hidden_when(move || tw.cmd.is_open());
    let closed = column([heading.fixed(1), line.fixed(1)]).hidden_when(move || tw.typing.get());
    layers([
        command_line_with(tw.cmd, CommandLineOptions::new().two_rows()),
        closed,
    ])
}

/// The Tasks view: the report over (or beside) the details, the command
/// bar, and the report's keys.
fn tasks_view(tw: Tw) -> Node {
    let layout = TableLayout::new();
    let table = report_table(tw, &layout);
    tw.table.set(Some(table.id()));
    let under = move || tw.details.get() && !tw.beside.get();
    let beside = move || tw.details.get() && tw.beside.get();
    let main = column([
        row([table.flex(1), details(tw).flex(1).shown_when(beside)]).flex(1),
        details(tw).flex(1).shown_when(under),
    ]);
    // A page of the report: the rows in view.
    let page = move || layout.rows_in_view().max(1);
    let page = Rc::new(page);
    let page_up = page.clone();
    let last = move || tw.view.with_untracked(|v| v.tasks.len()).saturating_sub(1);
    watch(move || tw.selected.get(), move |_, _| tw.detail_top.set(0));

    let set_priority =
        move |p: Option<char>| move |cx: &mut Ctx| tw.change_each(cx, |t, _| t.priority = p);
    column([main.flex(1), command_bar(tw).fixed(2)]).section("Task report", move |node| {
        node.bind("slash", "Filter task report", move |cx| tw.edit_filter(cx))
            .bind("a", "Add new task", move |cx| {
                tw.edit(cx, "Add Task", String::new(), |tw, text, cx| {
                    tw.add(cx, text, false)
                })
            })
            .bind("l", "Log new task", move |cx| {
                tw.edit(cx, "Log Task", String::new(), |tw, text, cx| {
                    tw.add(cx, text, true)
                })
            })
            .bind("m", "Modify selected task", move |cx| {
                // taskwarrior-tui fills in the task's description and
                // attributes; for several tasks, nothing.
                let targets = tw.targets();
                let text = match targets.as_slice() {
                    [t] => {
                        let mut s = format!("{} ", t.description);
                        for tag in &t.tags {
                            s.push_str(&format!("+{tag} "));
                        }
                        if let Some(p) = &t.project {
                            s.push_str(&format!("project:{p} "));
                        }
                        if let Some(p) = t.priority {
                            s.push_str(&format!("priority:{p} "));
                        }
                        if let Some(d) = t.due {
                            s.push_str(&format!("due:{} ", fmt_date(d, &tw.zone())));
                        }
                        s
                    }
                    _ => String::new(),
                };
                if !targets.is_empty() {
                    let title = tw.task_label("Modify");
                    tw.edit(cx, &title, text, |tw, text, cx| tw.modify(cx, text));
                }
            })
            .bind("A", "Annotate current task", move |cx| {
                if !tw.targets().is_empty() {
                    let title = tw.task_label("Annotate");
                    tw.edit(cx, &title, String::new(), |tw, text, cx| {
                        let note = words(text).join(" ");
                        if !note.is_empty() {
                            tw.change_each(cx, |t, now| {
                                t.annotations.push(Annotation {
                                    entry: now,
                                    description: note.clone(),
                                })
                            });
                        }
                    })
                }
            })
            .bind("d", "Mark task as done", move |cx| {
                tw.change_each(cx, |t, now| {
                    t.status = Status::Completed;
                    t.end = Some(now);
                    t.start = None;
                })
            })
            .bind("x", "Delete task", move |cx| {
                if !tw.targets().is_empty() {
                    let title = format!("{}?", tw.task_label("Delete"));
                    tw.ask(cx, title, "x", move |cx| {
                        tw.change_each(cx, |t, now| {
                            t.status = Status::Deleted;
                            t.end = Some(now);
                            t.start = None;
                        })
                    });
                }
            })
            .bind("s", "Toggle start and stop", move |cx| {
                tw.change_each(cx, |t, now| {
                    t.start = if t.start.is_some() { None } else { Some(now) }
                })
            })
            .bind("t", "Toggle +next", move |cx| {
                tw.change_each(cx, |t, _| {
                    if t.tags.iter().any(|g| g == "next") {
                        t.tags.retain(|g| g != "next");
                    } else {
                        t.tags.push("next".into());
                    }
                })
            })
            .bind("H", "Priority high", set_priority(Some('H')))
            .bind("M", "Priority medium", set_priority(Some('M')))
            .bind("L", "Priority low", set_priority(Some('L')))
            .bind("N", "No priority", set_priority(None))
            .bind("y", "Duplicate tasks", move |cx| {
                let now = tw.now();
                let copies: Vec<Task> = (tw.targets().into_iter())
                    .map(|t| {
                        let uuid = tw.store.with_untracked(|s| s.borrow_mut().new_uuid());
                        Task {
                            uuid,
                            entry: now,
                            modified: now,
                            status: Status::Pending,
                            start: None,
                            end: None,
                            ..t
                        }
                    })
                    .collect();
                tw.commit(cx, copies);
            })
            .bind("u", "Undo", move |cx| {
                tw.ask(cx, "Run `task undo`?".into(), "u", move |cx| {
                    let undone = tw.store.with_untracked(|s| s.borrow_mut().undo());
                    if let Err(e) = undone.and_then(|_| tw.reload()) {
                        tw.fail(cx, e);
                    }
                })
            })
            .bind("v", "Toggle mark on selected", move |_| {
                if let Some(t) = tw.current() {
                    tw.marked.toggle(t.uuid);
                }
            })
            .bind("V", "Toggle marks on all tasks", move |_| {
                let all = tw.view.with_untracked(|v| v.tasks.len());
                if tw.marked.selected().len() == all {
                    tw.marked.clear();
                } else {
                    tw.marked.select_all();
                }
            })
            .bind("colon", "Jump to task id", move |cx| {
                tw.edit(cx, "Jump to Task", String::new(), |tw, text, cx| {
                    let at = text.trim().parse::<u64>().ok().and_then(|id| {
                        tw.view
                            .with_untracked(|v| v.tasks.iter().position(|t| t.id == id))
                    });
                    match at {
                        Some(at) => tw.selected.set(at),
                        None => tw.fail(cx, format!("Cannot locate task id {}", text.trim())),
                    }
                })
            })
            .bind("z", "Toggle task info view", move |_| {
                tw.details.update(|d| *d = !*d)
            })
            .bind("backslash", "Transpose split", move |_| {
                tw.beside.update(|b| *b = !*b)
            })
            .bind("ctrl+e", "Scroll task details down", move |_| {
                tw.detail_top.update(|t| *t = t.saturating_add(1))
            })
            .bind("ctrl+y", "Scroll task details up", move |_| {
                tw.detail_top.update(|t| *t = t.saturating_sub(1))
            })
            .bind("g", "Go to top", move |_| tw.selected.set(0))
            .bind("G", "Go to bottom", move |_| tw.selected.set(last()))
            .bind("J", "Move page down", move |_| {
                tw.selected.update(|s| *s = (*s + page()).min(last()))
            })
            .bind("K", "Move page up", move |_| {
                tw.selected.update(|s| *s = s.saturating_sub(page_up()))
            })
            .bind("c", "Context switcher menu", move |cx| context_menu(tw, cx))
            .bind("R", "Report switcher menu", move |cx| report_menu(tw, cx))
            .bind("r", "Refresh", move |cx| {
                if let Err(e) = tw.reload() {
                    tw.fail(cx, e);
                }
            })
    })
}

/// The Projects view: `task projects`.
fn projects_view(tw: Tw) -> Node {
    scroll(text(move || tw.tasks.with(|t| projects_text(t))))
}

/// The Timesheet view: `task timesheet`.
fn timesheet_view(tw: Tw) -> Node {
    scroll(text(move || {
        let now = tw.now.get();
        tw.tasks.with(|t| timesheet_text(t, now, &tw.zone()))
    }))
}

/// The Calendar view: a year of months, as many across as fit, the due
/// dates of the report's tasks in their colours, today's month and day
/// bold.
fn calendar_view(tw: Tw) -> Node {
    let (today, _) = local(tw.now(), &tw.zone());
    let options = move || {
        let heading = match tw.year.get() == today.year {
            true => "bold underline",
            false => "underline",
        };
        let months = CalendarOptions::new()
            .title(MonthTitle::Name)
            .first_weekday(Weekday::Sunday)
            .today(today)
            .weekday_style("underline")
            .month_style_with(move |first: Date| {
                let this = first.year == today.year && first.month == today.month;
                Some(if this { "bold" } else { "default" })
            });
        YearOptions::new()
            .calendar(months)
            .marks(move |date| tw.due_style(date))
            .heading_style(heading)
    };
    column([
        label("").fixed(1),
        year_view_with(move || tw.year.get(), options).flex(1),
    ])
    .focusable()
    .section("Calendar", |node| {
        node.bind("j down", "Forward a year", move |_| {
            tw.year.update(|y| *y += 1)
        })
        .bind("k up", "Back a year", move |_| {
            tw.year.update(|y| *y = (*y - 1).max(0))
        })
        .bind("J pagedown", "Forward a decade", move |_| {
            tw.year.update(|y| *y += 10)
        })
        .bind("K pageup", "Back a decade", move |_| {
            tw.year.update(|y| *y = (*y - 10).max(0))
        })
    })
}

/// A switcher menu: a search box over a table, Enter picks the row.
/// taskwarrior-tui's context and report menus.
fn menu(
    cx: &mut Ctx,
    title: &'static str,
    headers: &'static [&'static str],
    rows: impl Fn() -> Vec<(String, Vec<String>, bool)> + 'static,
    pick: impl Fn(&str, &mut Ctx) + 'static,
) {
    cx.modal(Size::Percent(80), Size::Percent(50), move || {
        let rows = Rc::new(rows);
        let query = signal(String::new());
        let sel = signal(0usize);
        let shown = move || {
            let q = query.get();
            let all = rows();
            all.into_iter()
                .filter(|r| r.0.contains(&q))
                .collect::<Vec<_>>()
        };
        let shown = Rc::new(shown);
        watch(move || query.get(), move |_, _| sel.set(0));
        let columns = (headers.iter().enumerate())
            .map(|(i, h)| Column::new(h, if i == 1 { Size::Flex(1) } else { Size::Auto }))
            .collect();
        let (cells, styles, picked, count) = (shown.clone(), shown.clone(), shown.clone(), shown);
        let table = table_with(
            columns,
            move || {
                cells()
                    .into_iter()
                    .map(|r| r.1.iter().map(|c| escape(c)).collect())
                    .collect()
            },
            sel,
            TableOptions::default()
                .symbol("• ")
                .highlight_spacing(HighlightSpacing::Always)
                .row_style(move |i| styles().get(i).filter(|r| r.2).map(|_| "bold".to_string())),
        )
        .no_focus();
        let search = input(query)
            .autofocus()
            .on_submit(move |_, cx| {
                if let Some(row) = picked().get(sel.get_untracked()) {
                    let key = row.0.clone();
                    cx.pop();
                    pick(&key, cx);
                }
            })
            .on_key("esc", |cx| cx.pop());
        column([
            search.panel(&format!("[b]{title}[/]")).fixed(3),
            table.panel(""),
        ])
        .bind("down", "Next", move |_| {
            let n = count().len();
            sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1)))
        })
        .bind("up", "Previous", move |_| {
            sel.update(|s| *s = s.saturating_sub(1))
        })
        .theme_styles([("table.selected", "bold"), ("table.header", "underline")])
    });
}

fn context_menu(tw: Tw, cx: &mut Ctx) {
    let rows = move || {
        let active = tw.context.get_untracked();
        let mut rows = vec![(
            "none".to_string(),
            vec!["none".to_string(), String::new(), yes_no(active.is_none())],
            active.is_none(),
        )];
        for (name, filter) in tw.contexts.get_untracked() {
            let on = active.as_deref() == Some(name.as_str());
            rows.push((name.clone(), vec![name, filter, yes_no(on)], on));
        }
        rows
    };
    menu(
        cx,
        "Context  (type to filter)",
        &["Name", "Definition", "Active"],
        rows,
        move |name, cx| {
            let context = (name != "none").then(|| name.to_string());
            match tw
                .store
                .with_untracked(|s| s.borrow_mut().set_context(context.clone()))
            {
                Ok(()) => {
                    tw.context.set(context);
                    tw.selected.set(0);
                }
                Err(e) => tw.fail(cx, e),
            }
        },
    );
}

fn yes_no(b: bool) -> String {
    if b { "yes" } else { "no" }.to_string()
}

fn report_menu(tw: Tw, cx: &mut Ctx) {
    let rows = move || {
        let at = tw.report.get_untracked();
        (REPORTS.iter().enumerate())
            .map(|(i, r)| {
                let row = vec![r.name.to_string(), r.description.to_string()];
                (r.name.to_string(), row, i == at)
            })
            .collect()
    };
    menu(
        cx,
        "Report  (type to filter)",
        &["Name", "Description"],
        rows,
        move |name, _| {
            if let Some(i) = REPORTS.iter().position(|r| r.name == name) {
                tw.report.set(i);
                tw.filter
                    .set(format!("{} ", REPORTS[i].filter).trim_start().to_string());
                tw.selected.set(0);
            }
        },
    );
}

// ---------------------------------------------------------------------
// The app.
// ---------------------------------------------------------------------

/// The app over `store`.
pub fn tasks_app(store: Box<dyn Store>, options: Options) -> App {
    let follow_clock = options.now.is_none();
    App::new(move || {
        let tw = Tw::new(store, options);
        let loaded = tw.reload().err();
        let pages = RefCell::new(vec![
            Some(tasks_view(tw)),
            Some(projects_view(tw)),
            Some(timesheet_view(tw)),
            Some(calendar_view(tw)),
        ]);
        let ids: Vec<Option<NodeId>> = pages
            .borrow()
            .iter()
            .map(|p| p.as_ref().map(Node::id))
            .collect();
        let body = switch(
            move || tw.tab.get(),
            move |tab| pages.borrow_mut()[tab].take().unwrap_or_else(|| label("")),
        );
        // The focus follows the view: the report, or the view's own keys.
        watch(
            move || tw.tab.get(),
            move |tab, cx| {
                let id = match tab {
                    0 => tw.table.get_untracked(),
                    _ => ids[tab],
                };
                if let Some(id) = id {
                    cx.focus(id);
                }
            },
        );
        // A store that failed to load says so once the app is up.
        let loaded = RefCell::new(loaded);
        watch(
            || (),
            move |_, cx| {
                if let Some(e) = loaded.borrow_mut().take() {
                    tw.fail(cx, e);
                }
            },
        );
        if follow_clock {
            every(Duration::from_secs(30), move |cx| {
                tw.now.set(unix_now());
                if let Err(e) = tw.reload() {
                    tw.fail(cx, e);
                }
            });
        }
        let views = VIEWS.len();
        column([navbar(tw).fixed(1), body.flex(1)])
            .theme_styles([
                ("table.header", "underline"),
                ("selected", "bold"),
                ("table.selected", "bold"),
            ])
            .bind("bracketright", "Next view", move |_| {
                tw.tab.update(|t| *t = (*t + 1) % views)
            })
            .bind("bracketleft", "Previous view", move |_| {
                tw.tab.update(|t| *t = (*t + views - 1) % views)
            })
            .bind("question", "Help", |cx| {
                cx.help_with(HelpOptions::new().title("Help"))
            })
            .bind("q", "Quit", |cx| cx.quit())
    })
    .palette_key("ctrl+p")
}

/// A few tasks to start with, and two contexts.
pub fn sample(store: &mut dyn Store, dir: &Path, now: Time, zone: &Zone) -> Result<(), String> {
    let (today, _) = local(now, zone);
    let day = |n: i64| midnight(today.add_days(n), zone);
    let mut add = |description: &str, entry: Time, f: &dyn Fn(&mut Task)| {
        let mut task = Task::new(store.new_uuid(), description, entry);
        f(&mut task);
        task
    };
    let tasks = vec![
        add("Pay the rent", now - 3 * DAY, &|t| {
            t.project = Some("home.bills".into());
            t.priority = Some('H');
            t.tags = vec!["bills".into()];
            t.due = Some(day(1));
        }),
        add("Write the quarterly report", now - 10 * DAY, &|t| {
            t.project = Some("work".into());
            t.tags = vec!["next".into()];
            t.start = Some(now - 2 * HOUR);
            t.due = Some(day(5));
        }),
        add("Review pull requests", now - 5 * DAY, &|t| {
            t.project = Some("work".into());
            t.tags = vec!["review".into()];
            t.due = Some(day(-1));
            t.annotations = vec![Annotation {
                entry: now - DAY,
                description: "Two are from Ada".into(),
            }];
        }),
        add("Book flights to Lisbon", now - DAY, &|t| {
            t.project = Some("travel".into());
            t.due = Some(day(20));
        }),
        add("Fix the garden fence", now - 40 * DAY, &|t| {
            t.project = Some("home.garden".into());
            t.priority = Some('L');
        }),
        add("Call the plumber", now - 2 * HOUR, &|t| {
            t.priority = Some('M');
            t.tags = vec!["phone".into()];
        }),
        add("Renew the passport", now - 100 * DAY, &|t| {
            t.project = Some("travel".into());
            t.due = Some(day(60));
        }),
        add("Buy groceries", now - 2 * DAY, &|t| {
            t.status = Status::Completed;
            t.end = Some(now - DAY);
        }),
    ];
    store.commit(tasks)?;
    std::fs::write(dir.join("undo.jsonl"), "").map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("taskrc"),
        "context.work=project:work\ncontext.home=project:home\n",
    )
    .map_err(|e| e.to_string())
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let dir = std::env::args().nth(1).map_or_else(
        || std::env::temp_dir().join("intuituive-tasks"),
        PathBuf::from,
    );
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    let fail = |e: String| std::io::Error::other(e);
    let mut store = JsonlStore::open(&dir, seed).map_err(fail)?;
    if !dir.join("tasks.jsonl").exists() {
        sample(&mut store, &dir, unix_now(), &Zone::Local).map_err(fail)?;
    }
    let options = Options::default();
    tasks_app(Box::new(store), options).run()
}
