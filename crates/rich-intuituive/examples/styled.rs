//! A status page styled by a stylesheet: the code builds the nodes and
//! names them, and `styled.tcss` decides the colours, borders, sizes and
//! what docks where.
//!
//!     cargo run -p rs-rich-intuituive --example styled
//!     cargo run -p rs-rich-intuituive --example styled -- crates/rich-intuituive/examples/styled.tcss
//!
//! Given a file, the app reads it again whenever it changes, so edit the
//! sheet while the app runs; a sheet that does not parse is reported in a
//! toast and the last good one stays. Without one, it uses the same sheet,
//! built in.
//!
//! Tab moves between the services and the deploys · Space takes the
//! focused service down or up again (its `down` class follows) · the
//! pointer over a service is `:hover` · q quits.

use intuituive::prelude::*;
use intuituive::widgets::{table, Column};

/// The sheet used when no file is given: the one beside this example.
const SHEET: &str = include_str!("styled.tcss");

/// A service on the status page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Service {
    pub name: &'static str,
    pub version: &'static str,
    pub latency_ms: u32,
}

/// The services, left to right.
pub const SERVICES: [Service; 4] = [
    Service {
        name: "checkout",
        version: "v2.14.0",
        latency_ms: 84,
    },
    Service {
        name: "payments",
        version: "v1.9.3",
        latency_ms: 121,
    },
    Service {
        name: "search",
        version: "v3.2.1",
        latency_ms: 46,
    },
    Service {
        name: "images",
        version: "v0.8.7",
        latency_ms: 12,
    },
];

/// Recent deploys: when, what, which version, who.
pub const DEPLOYS: [[&str; 4]; 6] = [
    ["09:42", "checkout", "v2.14.0", "Sam"],
    ["09:15", "search", "v3.2.1", "Priya"],
    ["Yesterday", "payments", "v1.9.3", "Lena"],
    ["Yesterday", "images", "v0.8.7", "Priya"],
    ["Mon", "checkout", "v2.13.2", "Sam"],
    ["Mon", "search", "v3.2.0", "Lena"],
];

// --8<-- [start:card]
/// One service's card: a panel with the `card` class, and `down` while
/// the service is down.
fn card(service: Service, down: Signal<bool>) -> Node {
    text(move || {
        if down.get() {
            format!("● down\n{}", service.version)
        } else {
            format!("● up · {} ms\n{}", service.latency_ms, service.version)
        }
    })
    .panel(service.name)
    .class("card")
    .class_when("down", move || down.get())
    .focusable()
    .on_key("space", move |cx| {
        down.update(|d| *d = !*d);
        let now = if down.get_untracked() { "down" } else { "up" };
        cx.toast(format!("{} is {now}", service.name));
    })
}
// --8<-- [end:card]

/// The app, styled by the sheet at `path`, or by the built-in one.
pub fn styled_app(path: Option<String>) -> App {
    let app = App::new(|| {
        let down: Vec<Signal<bool>> = SERVICES
            .iter()
            .map(|s| signal(s.name == "payments"))
            .collect();
        let cards: Vec<Node> = SERVICES
            .iter()
            .zip(down.iter())
            .map(|(service, down)| card(*service, *down).flex(1))
            .collect();
        let deploys = table(
            vec![
                Column::new("When", Size::Auto),
                Column::new("Service", Size::Auto),
                Column::new("Version", Size::Auto),
                Column::new("By", Size::Auto),
            ],
            || {
                DEPLOYS
                    .iter()
                    .map(|row| row.iter().map(|cell| cell.to_string()).collect())
                    .collect()
            },
            signal(0usize),
        )
        .name("deploys");

        column([
            label("shop · production").name("title"),
            row(cards).name("services"),
            deploys,
            label("tab moves · space takes a service down or up · q quits").name("status"),
        ])
        .on_key("q", |cx| cx.quit())
    });
    // --8<-- [start:sheet]
    match path {
        Some(path) => app.stylesheet_file(path),
        None => app.stylesheet(SHEET),
    }
    // --8<-- [end:sheet]
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    styled_app(std::env::args().nth(1)).run()
}
