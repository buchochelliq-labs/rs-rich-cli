//! Parity for the live display (0.0.22 workstream 5): Live's render hook,
//! alternate screen and stdout/stderr redirect, and the live `Progress` and
//! `Status` displays, against `golden/parity_live.tsv` captured from real
//! Python `rich` with `auto_refresh=False`. The port's refresh threads are
//! given an interval long enough never to fire.
//!
//! This test is alone in its binary because some cases set `TERM`, which
//! upstream reads to detect a dumb terminal.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rich::color::ColorSystem;
use rich::console::Console;
use rich::text::Text;
use rich::{
    Live, LiveOptions, Progress, ProgressColumn, ProxyStream, Status, TaskId, VerticalOverflow,
};

fn markup(source: &str) -> Box<Text> {
    Box::new(Text::from_markup(source).expect("markup"))
}

fn stream(value: &serde_json::Value) -> ProxyStream {
    match value.as_str().expect("stream") {
        "stdout" => ProxyStream::Stdout,
        "stderr" => ProxyStream::Stderr,
        other => panic!("unknown stream {other:?}"),
    }
}

fn options(case: &serde_json::Value) -> LiveOptions {
    let options = &case["options"];
    let flag = |key: &str, default: bool| options[key].as_bool().unwrap_or(default);
    LiveOptions {
        transient: flag("transient", false),
        screen: flag("screen", false),
        redirect_stdout: flag("redirect_stdout", true),
        redirect_stderr: flag("redirect_stderr", true),
        vertical_overflow: match options["vertical_overflow"].as_str() {
            Some("crop") => VerticalOverflow::Crop,
            Some("visible") => VerticalOverflow::Visible,
            _ => VerticalOverflow::Ellipsis,
        },
        refresh_on_start: true,
    }
}

fn column(spec: &serde_json::Value) -> ProgressColumn {
    match spec[0].as_str().expect("column") {
        "description" => ProgressColumn::Description,
        "mofn" => ProgressColumn::MofN,
        "bar" => ProgressColumn::Bar,
        other => panic!("column {other:?} is not used by parity_live"),
    }
}

/// The streams written through proxies: what reached the files.
#[derive(Default)]
struct Files {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Files {
    fn file(&mut self, stream: ProxyStream) -> &mut Vec<u8> {
        match stream {
            ProxyStream::Stdout => &mut self.stdout,
            ProxyStream::Stderr => &mut self.stderr,
        }
    }
}

fn run_live(case: &serde_json::Value, console: Console, files: &mut Files) -> Vec<u8> {
    let options = options(case);
    let mut live = Live::new(
        markup(case["markup"].as_str().unwrap()),
        console,
        Vec::<u8>::new(),
    )
    .options(&options);
    for step in case["steps"].as_array().expect("steps") {
        match step[0].as_str().expect("op") {
            "start" => live.start_with(step[1].as_bool().unwrap()),
            "stop" => live.stop(),
            "update" => live.update_with(
                markup(step[1].as_str().unwrap()),
                step[2].as_bool().unwrap(),
            ),
            "refresh" => live.refresh(),
            "print" => live.print_str(step[1].as_str().unwrap()),
            "write" => {
                let stream = stream(&step[1]);
                let mut proxy = live.file_proxy(stream, files.file(stream));
                proxy
                    .write_all(step[2].as_str().unwrap().as_bytes())
                    .unwrap();
            }
            "flush" => {
                let stream = stream(&step[1]);
                live.file_proxy(stream, files.file(stream)).flush().unwrap();
            }
            other => panic!("unknown live step {other:?}"),
        }
    }
    live.into_writer()
}

fn run_progress(
    case: &serde_json::Value,
    console: Console,
    files: &mut Files,
    now: &Arc<AtomicU64>,
) -> Vec<u8> {
    let options = &case["options"];
    let flag = |key: &str, default: bool| options[key].as_bool().unwrap_or(default);
    let clock = now.clone();
    let live = Progress::new()
        .columns(
            case["columns"]
                .as_array()
                .unwrap()
                .iter()
                .map(column)
                .collect(),
        )
        .transient(flag("transient", false))
        .disable(flag("disable", false))
        .redirect_stdout(flag("redirect_stdout", true))
        .redirect_stderr(flag("redirect_stderr", true))
        .clock(move || f64::from_bits(clock.load(Ordering::SeqCst)))
        .start(console, Vec::<u8>::new(), 1e-9);
    for step in case["steps"].as_array().expect("steps") {
        match step[0].as_str().expect("op") {
            "add" => {
                live.add_task(
                    step[1].as_str().unwrap(),
                    step[2].as_f64(),
                    step[3].as_f64().unwrap(),
                );
            }
            "advance" => live.advance(
                TaskId(step[1].as_u64().unwrap() as usize),
                step[2].as_f64().unwrap(),
            ),
            "refresh" => live.refresh(),
            "print" => live.print_str(step[1].as_str().unwrap()),
            "write" => {
                let stream = stream(&step[1]);
                live.file_proxy(stream, files.file(stream))
                    .write_all(step[2].as_str().unwrap().as_bytes())
                    .unwrap();
            }
            "time" => now.store(step[1].as_f64().unwrap().to_bits(), Ordering::SeqCst),
            other => panic!("unknown progress step {other:?}"),
        }
    }
    live.stop().1
}

fn run_status(
    case: &serde_json::Value,
    console: Console,
    files: &mut Files,
    now: &Arc<AtomicU64>,
) -> Vec<u8> {
    let live = Status::new(case["message"].as_str().unwrap())
        .spinner(case["spinner"].as_str().unwrap())
        .refresh_per_second(1e-9)
        .start(console, Vec::<u8>::new());
    for step in case["steps"].as_array().expect("steps") {
        match step[0].as_str().expect("op") {
            "time" => now.store(step[1].as_f64().unwrap().to_bits(), Ordering::SeqCst),
            "refresh" => live.refresh(),
            "print" => live.print_str(step[1].as_str().unwrap()),
            "status" => live.update(
                step[1]["status"].as_str(),
                step[1]["spinner"].as_str(),
                None,
                None,
            ),
            "write" => {
                let stream = stream(&step[1]);
                live.file_proxy(stream, files.file(stream))
                    .write_all(step[2].as_str().unwrap().as_bytes())
                    .unwrap();
            }
            other => panic!("unknown status step {other:?}"),
        }
    }
    live.stop()
}

#[test]
fn parity_live() {
    let data = include_str!("golden/parity_live.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let case: serde_json::Value =
            serde_json::from_str(parts.next().expect("case")).expect("case json");
        let expected: Vec<String> =
            serde_json::from_str(parts.next().expect("expected")).expect("expected json");

        let term = std::env::var_os("TERM");
        match case["term"].as_str() {
            Some(value) => std::env::set_var("TERM", value),
            None => std::env::set_var("TERM", "xterm-256color"),
        }
        let now = Arc::new(AtomicU64::new(0f64.to_bits()));
        let clock = now.clone();
        let terminal = case["terminal"].as_bool().unwrap_or(true);
        let console = Console::builder()
            .force_terminal(terminal)
            .color_system(terminal.then_some(ColorSystem::Truecolor))
            .width(case["width"].as_u64().expect("width") as usize)
            .height(case["height"].as_u64().unwrap_or(5) as usize)
            .highlight(false)
            .no_color(false)
            .get_time(move || f64::from_bits(clock.load(Ordering::SeqCst)))
            .build();
        let mut files = Files::default();
        let output = match case["kind"].as_str().expect("kind") {
            "live" => run_live(&case, console, &mut files),
            "progress" => run_progress(&case, console, &mut files, &now),
            "status" => run_status(&case, console, &mut files, &now),
            other => panic!("unknown kind {other:?}"),
        };
        match term {
            Some(value) => std::env::set_var("TERM", value),
            None => std::env::remove_var("TERM"),
        }
        let got = vec![
            String::from_utf8(output).expect("utf-8"),
            String::from_utf8(files.stdout).expect("utf-8"),
            String::from_utf8(files.stderr).expect("utf-8"),
        ];
        assert_eq!(
            got,
            expected,
            "parity_live case {name:?} (line {}) diverged",
            index + 1
        );
        checked += 1;
    }
    assert_eq!(checked, 32, "expected every parity_live case to run");
}
