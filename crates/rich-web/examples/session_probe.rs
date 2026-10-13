//! A probe app for the process-per-session isolation tests
//! (`tests/isolation_hardening.rs`): not a docs example.
//!
//!     session_probe ADDRESS [--token T] [--startup-sleep MS] [--stdin-stop]
//!     session_probe --nested ADDRESS
//!
//! Keys in the page: `f` starts `sleep 1000` (in the session's process
//! group), `n` runs this program as an unrelated nested server (`--nested
//! 127.0.0.1:0`) and shows how it ended.

use std::io::BufRead;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rich_web::intuituive::prelude::*;

fn app() -> App {
    App::new(|| {
        let pid = std::process::id();
        let state = signal(String::from("waiting"));
        column([
            text!("pid {pid} · {state}").fixed(1),
            label("f forks · n nested").fixed(1),
        ])
        .on_key("f", move |_| {
            // Left running on purpose: the tests check what ends it.
            #[allow(clippy::zombie_processes)]
            let child = Command::new("sleep").arg("1000").spawn().unwrap();
            state.set(format!("FORKED {} END", child.id()));
        })
        .on_key("n", move |_| {
            // A program the app runs, which happens to serve as well.
            let out = Command::new(std::env::current_exe().unwrap())
                .args(["--nested", "127.0.0.1:0"])
                .stdin(Stdio::null())
                .output()
                .unwrap();
            let code = out.status.code().unwrap_or(-1);
            let bound = String::from_utf8_lossy(&out.stdout).contains("BOUND");
            state.set(format!("NESTED code={code} bound={bound} END"));
        })
    })
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut addr = "127.0.0.1:8080".to_string();
    let mut token = None;
    let mut startup = None;
    let mut stdin_stop = false;
    let mut nested = false;
    while let Some(arg) = args.next() {
        let mut value = || args.next().expect("a value after the option");
        match arg.as_str() {
            "--token" => token = Some(value()),
            "--startup-sleep" => startup = Some(value().parse::<u64>().expect("ms")),
            "--stdin-stop" => stdin_stop = true,
            "--nested" => nested = true,
            _ => addr = arg,
        }
    }
    if nested {
        // An unrelated program that serves an app: it should get the
        // address it asked for.
        let server = rich_web::Server::bind(addr.as_str(), app)?;
        println!("BOUND {}", server.local_addr());
        return Ok(());
    }
    if let (Some(ms), true) = (startup, std::env::var_os(rich_web::SESSION_ENV).is_some()) {
        std::thread::sleep(Duration::from_millis(ms));
    }
    let mut server = rich_web::Server::bind(addr.as_str(), app)?.title("session probe");
    if let Some(token) = token {
        server = server.token(token);
    }
    if stdin_stop {
        let handle = server.spawn()?;
        println!("Serving session probe at {}", handle.url());
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        let started = Instant::now();
        handle.stop();
        println!("STOPPED {}", started.elapsed().as_millis());
        return Ok(());
    }
    server.run()
}
