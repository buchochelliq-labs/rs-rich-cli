//! A process table: sorted by a click on a header or by `s`, columns
//! resized by dragging, and a cell cursor (`table_with` and
//! `TableOptions`).
//!
//!     cargo run -p rs-rich-intuituive --example tables
//!
//! ↑/↓ move the row and ←/→ the cell · s sorts by the cell's column, and
//! again reverses · a click on a header sorts by it · drag the gap after a
//! header to resize its column · q quits.
//!
//! The table is given the sort as a signal and sorts nothing itself: the
//! rows closure reads the signal and sorts the processes, so the status
//! line can name the selected process and the cell under the cursor.

use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::rich::Style;
use intuituive::widgets::{table_with, Column, Order, TableOptions};
use intuituive::Theme;

/// One process.
#[derive(Clone, Debug, PartialEq)]
pub struct Process {
    pub pid: u32,
    pub command: &'static str,
    pub user: &'static str,
    pub cpu: f64,
    pub memory: u32,
    pub threads: u32,
}

const fn process(
    pid: u32,
    command: &'static str,
    user: &'static str,
    cpu: f64,
    memory: u32,
    threads: u32,
) -> Process {
    Process {
        pid,
        command,
        user,
        cpu,
        memory,
        threads,
    }
}

/// The processes the table shows: the same every run.
pub const PROCESSES: [Process; 14] = [
    process(1, "systemd", "root", 0.0, 12, 1),
    process(412, "sshd", "root", 0.1, 8, 1),
    process(988, "postgres", "postgres", 6.4, 412, 9),
    process(1024, "nginx", "www-data", 1.2, 36, 4),
    process(1311, "redis-server", "redis", 2.8, 96, 6),
    process(2240, "node api/server.js", "app", 23.5, 618, 11),
    process(2251, "node api/worker.js", "app", 11.9, 344, 7),
    process(3075, "cargo build", "dev", 87.0, 1540, 16),
    process(3102, "rust-analyzer", "dev", 14.2, 2210, 22),
    process(3388, "vim src/main.rs", "dev", 0.3, 24, 1),
    process(4121, "prometheus", "monitor", 3.6, 288, 12),
    process(4130, "grafana-server", "monitor", 1.7, 172, 14),
    process(5502, "bash", "dev", 0.0, 6, 1),
    process(5577, "htop", "dev", 0.9, 5, 1),
];

/// The columns, in order.
pub const COLUMNS: [&str; 6] = ["PID", "Command", "User", "CPU %", "Memory", "Threads"];

/// The processes sorted as `sort` says, or in PID order.
pub fn sorted(sort: Option<(usize, Order)>) -> Vec<Process> {
    let mut rows = PROCESSES.to_vec();
    if let Some((column, order)) = sort {
        rows.sort_by(|a, b| {
            let ordering = match column {
                0 => a.pid.cmp(&b.pid),
                1 => a.command.cmp(b.command),
                2 => a.user.cmp(b.user).then(a.pid.cmp(&b.pid)),
                3 => a.cpu.total_cmp(&b.cpu),
                4 => a.memory.cmp(&b.memory),
                _ => a.threads.cmp(&b.threads).then(a.pid.cmp(&b.pid)),
            };
            match order {
                Order::Ascending => ordering,
                Order::Descending => ordering.reverse(),
            }
        });
    }
    rows
}

/// A process's cells, numbers right-aligned.
fn cells(p: &Process) -> Vec<String> {
    let cpu = if p.cpu >= 50.0 {
        format!("[red]{:>5.1}[/]", p.cpu)
    } else {
        format!("{:>5.1}", p.cpu)
    };
    vec![
        format!("{:>5}", p.pid),
        escape(p.command),
        p.user.to_string(),
        cpu,
        format!("{:>5} MiB", p.memory),
        format!("{:>7}", p.threads),
    ]
}

/// A cell's value as the status line says it.
fn value(p: &Process, column: usize) -> String {
    match column {
        0 => p.pid.to_string(),
        1 => p.command.to_string(),
        2 => p.user.to_string(),
        3 => format!("{:.1}%", p.cpu),
        4 => format!("{} MiB", p.memory),
        _ => p.threads.to_string(),
    }
}

/// The app.
pub fn tables_app() -> App {
    App::new(|| {
        // --8<-- [start:table]
        let sort = signal(Some((3, Order::Descending)));
        let selected = signal(0usize);
        let cell = signal(1usize);

        let columns = COLUMNS.map(|title| Column::new(title, Size::Auto)).to_vec();
        let processes = table_with(
            columns,
            move || sorted(sort.get()).iter().map(cells).collect(),
            selected,
            TableOptions::default().sort(sort).cells(cell).resizable(),
        )
        .panel("Processes");
        // --8<-- [end:table]

        let status = text(move || {
            let rows = sorted(sort.get());
            let column = cell.get().min(COLUMNS.len() - 1);
            let by = match sort.get() {
                Some((c, Order::Ascending)) => format!("{} ▲", COLUMNS[c]),
                Some((c, Order::Descending)) => format!("{} ▼", COLUMNS[c]),
                None => "PID".to_string(),
            };
            let here = rows
                .get(selected.get())
                .map(|p| {
                    format!(
                        "{} {}: [b]{}[/]",
                        p.pid,
                        COLUMNS[column],
                        escape(&value(p, column))
                    )
                })
                .unwrap_or_default();
            format!("{here}  [muted]sorted by {by}[/]  [dim]s sorts · q quits[/]")
        });

        column([processes.flex(1), status.fixed(1)]).on_key("q", |cx| cx.quit())
    })
    // The selected row in grey and the cell cursor in yellow over it.
    .theme(
        Theme::dark()
            .style("selected", Style::parse("on grey27").expect("a style"))
            .style(
                "selected.cell",
                Style::parse("bold black on yellow").expect("a style"),
            ),
    )
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    tables_app().run()
}
