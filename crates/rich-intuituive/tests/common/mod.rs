//! Running apps under rich-interact's headless driver.

#![allow(dead_code)]

use intuituive::App;
use rich_interact::headless::{Headless, Record, Script};

pub fn run(app: App, script: Script, width: u16, height: u16) -> Record {
    let mut backend = Headless::new(script, width, height);
    let record = backend.record();
    app.run_on(&mut backend)
        .expect("the app runs to the end of its script");
    let record = record.borrow().clone();
    record
}

/// Run until the script runs out, with the app still open.
pub fn run_open(app: App, script: Script, width: u16, height: u16) -> Record {
    let mut backend = Headless::new(script, width, height);
    let record = backend.record();
    let ended = app.run_on(&mut backend).expect_err("the app is still open");
    assert_eq!(ended.kind(), std::io::ErrorKind::UnexpectedEof);
    let record = record.borrow().clone();
    record
}

/// The last frame's rows, without trailing spaces.
pub fn screen(record: &Record) -> Vec<String> {
    record
        .last_frame()
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect()
}

/// The row of the first line containing `needle`.
pub fn row_of(screen: &[String], needle: &str) -> Option<usize> {
    screen.iter().position(|line| line.contains(needle))
}
