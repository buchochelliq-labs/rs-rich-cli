//! The scope-tui rebuild (examples/scope.rs): its signal processing, and
//! the app driven by keys on the test signal.

mod common;

#[path = "../examples/scope.rs"]
#[allow(dead_code)]
mod scope;

use std::collections::VecDeque;
use std::io::Cursor;
use std::time::Duration;

use common::{row_of, run, screen};
use intuituive::App;
use rich_interact::headless::Script;
use scope::*;

fn args(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

/// The app on the test signal, standing still, one buffer every 40 ms.
fn app() -> App {
    let options = Options::default();
    let demo = Demo::new(2, options.buffer as usize, true);
    scope_app(options, Box::new(demo), Duration::from_millis(40))
}

/// Run `keys` after the first buffers have arrived, then quit.
fn after(keys: &str) -> Vec<String> {
    after_at(keys, 140)
}

fn after_at(keys: &str, width: u16) -> Vec<String> {
    let mut script = Script::new().wait(Duration::from_millis(200));
    if !keys.is_empty() {
        script = script.keys(keys).wait(Duration::from_millis(100));
    }
    screen(&run(app(), script.keys("q"), width, 24))
}

fn braille(rows: &[String]) -> usize {
    rows.iter()
        .flat_map(|r| r.chars())
        .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
        .count()
}

#[test]
fn samples_are_split_into_channels_and_scaled() {
    let bytes: Vec<u8> = [16384i16, -32768, 0, 32767]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let m = deinterleave(&bytes, 2);
    assert_eq!(m, vec![vec![0.5, 0.0], vec![-1.0, 32767.0 / 32768.0]]);
}

#[test]
fn a_short_last_buffer_is_padded_and_then_the_input_ends() {
    let bytes: Vec<u8> = [1000i16, 2000, 3000]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let mut pcm = Pcm::new(Cursor::new(bytes), 1, 4);
    match pcm.pull() {
        Pull::Frame(m) => assert_eq!(m[0].len(), 4, "padded to the buffer: {m:?}"),
        _ => panic!("expected a frame"),
    }
    assert!(matches!(pcm.pull(), Pull::End));
}

#[test]
fn the_trigger_finds_a_debounced_crossing() {
    let data = [-0.5, 0.2, -0.1, -0.2, 0.3, 0.4, 0.5];
    // Rising through 0 at 1 bounces back; at 3 it stays up for 3 samples.
    assert!(triggered(&data, 0, 0.0, 1, false));
    assert!(!triggered(&data, 0, 0.0, 2, false));
    assert!(triggered(&data, 3, 0.0, 3, false));
    assert!(triggered(&data, 1, 0.0, 1, true), "falling at 1");
    assert!(!triggered(&data, 5, 0.0, 3, false), "too near the end");
}

#[test]
fn the_fft_finds_a_tone_in_its_bin() {
    let n = 256;
    let tone: Vec<f64> = (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * 10.0 * i as f64 / n as f64).sin())
        .collect();
    let bins = fft(&tone);
    let magnitude = |(re, im): (f64, f64)| (re * re + im * im).sqrt();
    let peak = (0..n / 2)
        .max_by(|a, b| magnitude(bins[*a]).total_cmp(&magnitude(bins[*b])))
        .unwrap();
    assert_eq!(peak, 10);
    assert!((magnitude(bins[10]) - n as f64 / 2.0).abs() < 1e-6);
    // Not a power of two: padded.
    assert_eq!(fft(&[1.0; 100]).len(), 128);
}

#[test]
fn the_spectroscope_peaks_at_the_test_tones() {
    let g = Graph {
        references: false,
        ..graph()
    };
    let Pull::Frame(frame) = Demo::new(2, 2048, true).pull() else {
        panic!("the demo always has a frame");
    };
    let history: Vec<VecDeque<Vec<f64>>> = frame.into_iter().map(|c| VecDeque::from([c])).collect();
    let plot = spectroscope(&g, &Spectroscope::default(), &history);
    // R is drawn first (channels in reverse, as scope-tui).
    let names: Vec<_> = plot.sets.iter().map(|s| s.name.clone().unwrap()).collect();
    assert_eq!(names, ["R", "L"]);
    let peak = |set: &DataSet| {
        let (x, _) = set
            .points
            .iter()
            .copied()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        x.exp().round()
    };
    // 3 and 2 cycles a buffer of 2048 at 48 kHz.
    assert_eq!(peak(&plot.sets[0]), (3.0 * 48000.0 / 2048.0f64).round());
    assert_eq!(peak(&plot.sets[1]), (2.0 * 48000.0 / 2048.0f64).round());
}

fn graph() -> Graph {
    let o = Options::default();
    Graph {
        samples: o.buffer,
        width: o.buffer,
        rate: o.rate,
        scale: o.scale,
        scatter: false,
        references: true,
        show_ui: true,
        pause: false,
        braille: true,
        palette: o.palette,
        labels: o.labels,
        axis: o.axis,
    }
}

#[test]
fn the_command_line_reads_as_scope_tuis() {
    let c = parse(&args(
        "-c 1 -b 1024 -r 44100 -s 0.5 --scatter --no-braille file - -l",
    ))
    .unwrap();
    assert_eq!(
        (
            c.options.channels,
            c.options.buffer,
            c.options.rate,
            c.options.scale
        ),
        (1, 1024, 44100, 0.5)
    );
    assert!(c.options.scatter && !c.options.braille && c.limit_rate);
    assert_eq!(c.file.as_deref(), Some("-"));
    let c = parse(&args("--palette-color Green,blue --still")).unwrap();
    assert_eq!(c.options.palette, ["green", "blue"]);
    assert!(c.still && c.file.is_none());
    // A4 is 440 Hz: 48000 / 440 samples a period.
    assert_eq!(parse(&args("-t A4")).unwrap().options.buffer, 109);
    assert_eq!(tune("A5", 48000), Some(55));
    assert!(parse(&args("--bogus")).is_err());
}

#[test]
fn it_opens_on_the_oscilloscope_with_its_header_and_legend() {
    let rows = after("");
    let header = &rows[0];
    for part in [
        "oscillo::scope-tui",
        "live",
        "-1.00x+",
        "2048/2048 spf",
        "---",
        "|>",
    ] {
        assert!(header.contains(part), "{part:?} in {header:?}");
    }
    assert!(rows[1].starts_with("| amplitude"), "{:?}", rows[1]);
    assert!(rows.last().unwrap().ends_with("time -"));
    assert!(rows.last().unwrap().starts_with("└──"));
    // The legend lists R then L, boxed at the top right.
    assert!(rows[2].trim_end().ends_with('┐'));
    assert!(rows[3].trim_end().ends_with("│R│"), "{:?}", rows[3]);
    assert!(rows[4].trim_end().ends_with("│L│"), "{:?}", rows[4]);
    assert!(braille(&rows) > 100, "the waveforms are drawn");
}

#[test]
fn tab_cycles_the_three_scopes() {
    let rows = after("tab");
    assert!(rows[0].contains("vector::scope-tui"));
    assert!(rows[1].starts_with("| right"));
    assert!(rows.last().unwrap().ends_with("left -"));
    assert!(braille(&rows) > 100);
    let rows = after("tab tab");
    assert!(rows[0].contains("spectro::scope-tui"));
    assert!(
        rows[0].contains("live  ---  23.438Hz bins"),
        "{:?}",
        rows[0]
    );
    assert!(rows[1].starts_with("| level"));
    let rows = after("tab tab tab");
    assert!(rows[0].contains("oscillo::scope-tui"));
}

#[test]
fn settings_show_in_the_header() {
    let rows = after("space s up shift+left t pageup");
    let header = &rows[0];
    assert!(header.contains("||"), "paused: {header}");
    assert!(header.contains("***"), "scatter: {header}");
    assert!(header.contains("-1.01x+"), "scale: {header}");
    assert!(header.contains("1798/2048 spf"), "samples: {header}");
    assert!(header.contains("^ 0.01 trigger"), "trigger: {header}");
    // Esc puts the view back and stops triggering.
    let rows = after("up shift+left t esc");
    assert!(rows[0].contains("-1.00x+") && rows[0].contains("2048/2048 spf"));
    assert!(rows[0].contains("live"));
}

#[test]
fn the_spectroscope_averages_and_windows() {
    // A wide screen: the header's second cell is a quarter of it.
    let rows = after_at("tab tab pageup pageup w l", 160);
    assert!(
        rows[0].contains("3x avg (0.1s)  -|-  7.812Hz bins"),
        "{:?}",
        rows[0]
    );
    assert!(rows[1].starts_with("| amplitude"), "linear: {:?}", rows[1]);
}

#[test]
fn h_hides_the_interface() {
    let rows = after("h");
    assert!(row_of(&rows, "scope-tui").is_none());
    assert!(row_of(&rows, "amplitude").is_none());
    assert!(!rows.iter().any(|r| r.contains("│R│")), "no legend");
    assert!(rows[0].starts_with('│'), "the plot takes the whole screen");
}

#[test]
fn r_removes_the_reference_line() {
    // The zero line runs the full width, so the middle row is dense with it.
    let middle = |rows: &[String]| {
        rows.iter()
            .map(|r| r.chars().filter(|c| *c != ' ').count())
            .max()
            .unwrap()
    };
    let with = after("p");
    let without = after("p r");
    assert!(middle(&without) < middle(&with), "{without:#?}");
}

#[test]
fn the_fps_counter_counts_frames_shown() {
    let script = Script::new().wait(Duration::from_millis(2100)).keys("q");
    let rows = screen(&run(app(), script, 100, 24));
    // A buffer every 40 ms: 25 a second.
    assert!(rows[0].contains("25fps"), "{:?}", rows[0]);
    let script = Script::new()
        .wait(Duration::from_millis(500))
        .keys("space")
        .wait(Duration::from_millis(2100))
        .keys("q");
    let rows = screen(&run(app(), script, 100, 24));
    assert!(
        rows[0].contains("0fps") && rows[0].contains("||"),
        "{:?}",
        rows[0]
    );
}

#[test]
fn the_end_of_the_input_shows_in_the_header() {
    let bytes = vec![0u8; 64];
    let pcm = Pcm::new(Cursor::new(bytes), 2, 16);
    let app = scope_app(Options::default(), Box::new(pcm), Duration::from_millis(40));
    let script = Script::new().wait(Duration::from_millis(200)).keys("q");
    let rows = screen(&run(app, script, 100, 24));
    assert!(rows[0].contains("end of input"), "{:?}", rows[0]);
}

#[test]
fn question_mark_lists_the_keys() {
    let script = Script::new()
        .wait(Duration::from_millis(100))
        .keys("?")
        .wait(Duration::from_millis(100));
    let rows = screen(&common::run_open(app(), script, 100, 30));
    assert!(row_of(&rows, "Keys").is_some());
    assert!(row_of(&rows, "phase difference").is_some());
}

#[test]
fn a_threaded_source_hands_over_the_newest_buffer_then_the_end() {
    let bytes: Vec<u8> = (0..8i16).flat_map(|s| (s * 1000).to_le_bytes()).collect();
    // Two buffers of two samples on two channels, read on a thread.
    let mut source = Threaded::new(Pcm::new(Cursor::new(bytes), 2, 2), None);
    let mut seen = Vec::new();
    for _ in 0..500 {
        match source.pull() {
            Pull::Frame(m) => seen.push(m),
            Pull::End => break,
            Pull::Wait => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    assert!(matches!(source.pull(), Pull::End), "still the end");
    // The thread may outrun the reader, which then sees only the newest
    // buffer, but the end never hides the last one.
    let last = seen.last().expect("the last buffer");
    assert_eq!(last[0], vec![4000.0 / 32768.0, 6000.0 / 32768.0]);
}
