//! A timedemo: replay input against an app with no waits and report what
//! each frame cost, as Doom's `-timedemo` does.
//!
//!     cargo run --release -p rs-rich-intuituive --example timedemo
//!     cargo run --release -p rs-rich-intuituive --example timedemo -- my.tape
//!
//! It replays the ops dashboard (the `dashboard` example) with a scripted
//! run of selections, log lines and a minute of its timer, or the input of
//! a rich-record tape given as the argument, at the tape's size. Set
//! `RICH_PAINT_STRATEGY` to compare paint strategies.

#[path = "dashboard.rs"]
#[allow(dead_code)]
mod dashboard;

use std::time::Duration;

use intuituive::interact::headless::Script;
use intuituive::timedemo::script_from_tape;

fn main() {
    let (script, (width, height)) = match std::env::args().nth(1) {
        Some(path) => {
            let tape = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!("{path}: {e}");
                std::process::exit(2)
            });
            script_from_tape(&tape).unwrap_or_else(|e| {
                eprintln!("{path}: {e}");
                std::process::exit(2)
            })
        }
        None => {
            let mut script = Script::new();
            for _ in 0..50 {
                script = script.keys("down down l");
            }
            (
                script.wait(Duration::from_secs(60)).keys("up up up"),
                (100, 30),
            )
        }
    };
    let demo = dashboard::dashboard(true).timedemo(script, width, height);
    println!("{demo}");
}
