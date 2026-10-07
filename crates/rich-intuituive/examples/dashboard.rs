//! An ops dashboard: a service list with a selection, a detail pane, a
//! streaming log and a status line.
//!
//!     cargo run -p rs-rich-intuituive --example dashboard
//!
//! ↑/↓ (or j/k) select, l adds a log line, q quits. A timer ticks the
//! status line and streams the log once a second.

use std::time::Duration;

use intuituive::prelude::*;

pub const SERVICES: usize = 20;

pub fn service(i: usize) -> (String, &'static str, u32) {
    let status = ["up", "up", "degraded", "up", "down"][i % 5];
    (format!("svc-{i:02}"), status, 10 + (i as u32 * 37) % 90)
}

pub fn log_line(n: usize) -> String {
    let (name, _, _) = service(n % SERVICES);
    format!(
        "12:{:02}:{:02} {name} heartbeat #{n}",
        (n / 60) % 60,
        n % 60
    )
}

// app: intuituive
pub fn dashboard(ticking: bool) -> App {
    App::new(move || {
        let selected = signal(0usize);
        let tick = signal(0u64);
        let log = Log::new(500);
        (0..50).for_each(|n| log.push(log_line(n)));
        let add_line = move || {
            tick.update(|t| *t += 1);
            log.push(log_line(49 + tick.get_untracked() as usize));
        };
        if ticking {
            every(Duration::from_secs(1), move |_| add_line());
        }

        let services = each(
            || (0..SERVICES).collect(),
            move |i| {
                let mine = memo(move || selected.get() == i);
                text(move || {
                    let (name, status, ms) = service(i);
                    let colour = match status {
                        "up" => "green",
                        "degraded" => "yellow",
                        _ => "red",
                    };
                    let row = format!("[{colour}]●[/] {name:<8} {ms:>4}ms");
                    if mine.get() {
                        format!("[reverse]{row}[/]")
                    } else {
                        row
                    }
                })
            },
        );
        let detail = text(move || {
            let (name, status, ms) = service(selected.get());
            format!("[bold]{name}[/]\nstatus   {status}\nlatency  {ms}ms")
        });

        column([
            label(format!("[bold] ops[/] · {SERVICES} services")).fixed(1),
            row([
                services.panel("Services").flex(40),
                column([detail.panel("Detail").fixed(5), log.view().panel("Log")]).flex(60),
            ]),
            text!("[dim] tick {tick} · ↑↓ select · l log · q quit[/]").fixed(1),
        ])
        .on_key("down j", move |_| {
            selected.update(|s| *s = (*s + 1) % SERVICES)
        })
        .on_key("up k", move |_| {
            selected.update(|s| *s = (*s + SERVICES - 1) % SERVICES)
        })
        .on_key("t", move |_| tick.update(|t| *t += 1))
        .on_key("l", move |_| add_line())
        .on_key("q", |cx| cx.quit())
    })
}
// app: end

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    dashboard(true).run()
}
