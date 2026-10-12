//! Each tab in a process of its own: an app that can crash, hang or eat
//! memory on purpose, to show the other tabs and the server carry on.
//!
//!     cargo run -p rs-rich-web --example isolated [-- ADDRESS [--memory BYTES] [--cpu SECONDS] [--token TOKEN]]
//!
//! Each tab shows the id of the process it runs in. In the page: `c`
//! panics, `h` loops forever, `m` allocates until the memory limit (256
//! MiB unless `--memory` says otherwise) stops it, and `+` shows the tab
//! is still answering. The server tells the page its session ended and
//! kills the process (a hung one when the tab closes, or when its CPU
//! time, 10 s unless `--cpu` says, runs out); the other tabs never notice.

use std::time::Duration;

use rich_web::intuituive::prelude::*;

fn app() -> App {
    App::new(|| {
        let pid = std::process::id();
        let state = signal("waiting");
        column([
            text!("[b]pid {pid}[/] · {state}").fixed(1),
            label("[muted]c crashes · h hangs · m eats memory · + answers").fixed(1),
        ])
        .on_key("+", move |_| state.set("ANSWERED"))
        .on_key("c", |_| panic!("crashed on purpose"))
        .on_key("h", |_| {
            #[allow(clippy::empty_loop)]
            loop {}
        })
        .on_key("m", |_| {
            let mut held: Vec<Vec<u8>> = Vec::new();
            loop {
                held.push(vec![1u8; 16 << 20]);
            }
        })
    })
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut addr = "127.0.0.1:8080".to_string();
    let mut memory = 256u64 << 20;
    let mut cpu = 10u64;
    let mut token = None;
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the option");
        match arg.as_str() {
            "--memory" => memory = value().parse().expect("bytes"),
            "--cpu" => cpu = value().parse().expect("seconds"),
            "--token" => token = Some(value()),
            _ => addr = arg,
        }
    }
    let mut server = rich_web::Server::bind(addr.as_str(), app)?
        .title("isolated sessions")
        .session_memory(memory)
        .session_cpu(Duration::from_secs(cpu));
    if let Some(token) = token {
        server = server.token(token);
    }
    server.run()
}
