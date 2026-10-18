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
use intuituive::interact::BackendKind;
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
}

#[test]
fn the_fft_of_any_length_is_the_dft() {
    // Not a power of two: Bluestein's, of the samples' own length, as
    // scope-tui's FFT is.
    for n in [3usize, 100, 109, 1000] {
        let x: Vec<f64> = (0..n).map(|i| ((i * 7919) % 13) as f64 - 6.0).collect();
        let bins = fft(&x);
        assert_eq!(bins.len(), n);
        for k in [0, 1, n / 3, n - 1] {
            let (mut re, mut im) = (0.0, 0.0);
            for (j, s) in x.iter().enumerate() {
                let a = -2.0 * std::f64::consts::PI * (j * k) as f64 / n as f64;
                re += s * a.cos();
                im += s * a.sin();
            }
            assert!(
                (bins[k].0 - re).abs() < 1e-6 && (bins[k].1 - im).abs() < 1e-6,
                "{n} {k}"
            );
        }
    }
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
    // Every bin is a point, the mirrored half too; the peak is in the first.
    let peak = |set: &DataSet| {
        let (x, _) = set.points[..1024]
            .iter()
            .copied()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        x.round()
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
    // Colours as ratatui reads them, as the console's names for the same.
    let colours = "--palette-color LightRed,dark_gray,Gray,#FF8800,208 --axis-color White";
    let c = parse(&args(colours)).unwrap();
    assert_eq!(
        c.options.palette,
        [
            "bright_red",
            "bright_black",
            "white",
            "#ff8800",
            "color(208)"
        ]
    );
    assert_eq!(c.options.axis, "bright_white");
    assert!(parse(&args("--labels-color mauve")).is_err());
    // A4 is 440 Hz: 48000 / 440 samples a period.
    assert_eq!(parse(&args("-t A4")).unwrap().options.buffer, 109);
    assert_eq!(tune("A5", 48000, 2), Some(55));
    // From scope-tui's table (C0 is 16.35 Hz, C4 261.6), then nudged off a
    // multiple of channels × 2: A3 is 218 samples, and 219 on one channel.
    assert_eq!(tune("C4", 48000, 2), Some(183));
    assert_eq!(tune("A3", 48000, 2), Some(218));
    assert_eq!(tune("A3", 48000, 1), Some(219));
    assert_eq!(tune("H2", 48000, 2), None);
    // -l before or after the path, options after the source.
    let c = parse(&args("file -l x.pcm --scatter")).unwrap();
    assert!(c.limit_rate && c.options.scatter && c.file.as_deref() == Some("x.pcm"));
    assert!(parse(&args("-l")).is_err(), "-l belongs to file");
    let error = parse(&args("audio")).unwrap_err();
    assert!(error.contains("file -"), "{error}");
    assert!(parse(&args("--help"))
        .unwrap()
        .print
        .unwrap()
        .contains("--palette-color"));
    assert!(parse(&args("-V")).unwrap().print.unwrap().contains("0.3.5"));
    assert!(parse(&args("--bogus")).is_err());
    // The backend, by name; crossterm is in every build.
    assert_eq!(parse(&args("")).unwrap().backend, None);
    assert_eq!(
        parse(&args("--backend Crossterm")).unwrap().backend,
        Some(BackendKind::Crossterm)
    );
    let error = parse(&args("--backend curses")).unwrap_err();
    assert!(
        error.contains("no backend \"curses\" in this build, which has crossterm"),
        "{error}"
    );
    assert!(parse(&args("--backend")).is_err());
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
    // scope-tui's axes have no labels, so ratatui draws no axis lines: the
    // titles stand alone, over the plot's first and last rows.
    assert!(rows[1].starts_with("| amplitude"), "{:?}", rows[1]);
    assert_eq!(rows.len(), 24);
    assert!(rows[23].ends_with("time -"), "{rows:#?}");
    assert!(!rows
        .iter()
        .any(|r| r.starts_with('│') || r.starts_with('└')));
    // The legend lists R then L, boxed at the top right.
    assert!(rows[1].trim_end().ends_with('┐'));
    assert!(rows[2].trim_end().ends_with("│R│"), "{:?}", rows[2]);
    assert!(rows[3].trim_end().ends_with("│L│"), "{:?}", rows[3]);
    assert!(braille(&rows) > 100, "the waveforms are drawn");
}

#[test]
fn tab_cycles_the_three_scopes() {
    let rows = after("tab");
    assert!(rows[0].contains("vector::scope-tui"));
    assert!(rows[1].starts_with("| right"));
    assert!(rows[23].ends_with("left -"));
    // The vectorscope numbers its channels, as scope-tui's does.
    assert!(rows[2].trim_end().ends_with("│1│"), "{:?}", rows[2]);
    assert!(rows[3].trim_end().ends_with("│0│"), "{:?}", rows[3]);
    assert!(braille(&rows) > 100);
    let rows = after("tab tab");
    assert!(rows[0].contains("spectro::scope-tui"));
    assert!(
        rows[0].contains("live  ---  23.438Hz bins"),
        "{:?}",
        rows[0]
    );
    assert!(rows[1].starts_with("| level"));
    assert!(rows[2].trim_end().ends_with("│R│"), "{:?}", rows[2]);
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
    // The threshold in 16-bit units, as scope-tui counts it.
    assert!(header.contains("^ 250 trigger"), "trigger: {header}");
    let rows = after("t e shift+pageup = = +");
    assert!(rows[0].contains("v 2500:13 trigger"), "{:?}", rows[0]);
    assert!(rows[2].trim_end().ends_with("│T│"), "{:?}", rows[2]);
    // Esc puts the view back and stops triggering.
    let rows = after("up shift+left t esc");
    assert!(rows[0].contains("-1.00x+") && rows[0].contains("2048/2048 spf"));
    assert!(rows[0].contains("live"));
    // Only the scope showing gets it: in the vectorscope, the trigger stays.
    let rows = after("t tab esc tab tab");
    assert!(rows[0].contains("^ 0 trigger"), "{:?}", rows[0]);
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
    // The averaging steps by one whatever the modifier, as scope-tui's.
    let rows = after_at(
        "tab tab shift+pageup ctrl+pageup alt+pageup ctrl+pagedown",
        160,
    );
    assert!(rows[0].contains("3x avg"), "{:?}", rows[0]);
}

#[test]
fn the_header_reports_the_bin_width_of_the_buffer() {
    // -t A4: 109 samples, transformed as they are: 440.367 Hz bins at 48 kHz.
    let g = Graph {
        width: 109,
        samples: 109,
        ..graph()
    };
    let header = spectroscope_header(&g, &Spectroscope::default());
    assert_eq!(header, "live  ---  440.367Hz bins");
    let g = Graph {
        references: false,
        ..g
    };
    let tone: Vec<f64> = (0..109).map(|i| (i as f64 * 0.3).sin()).collect();
    let plot = spectroscope(&g, &Spectroscope::default(), &[VecDeque::from([tone])]);
    assert_eq!(plot.sets[0].points.len(), 109);
    assert!((plot.sets[0].points[1].0 - 48000.0 / 109.0).abs() < 1e-6);
}

#[test]
fn h_hides_the_interface_but_not_the_legend() {
    let rows = after("h");
    assert!(row_of(&rows, "scope-tui").is_none());
    assert!(row_of(&rows, "amplitude").is_none());
    assert!(row_of(&rows, "time -").is_none());
    // ratatui draws a legend whether or not the axes have titles.
    assert!(rows[0].trim_end().ends_with('┐'), "{:?}", rows[0]);
    assert!(rows[1].trim_end().ends_with("│R│"), "{:?}", rows[1]);
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
    // Not the header, which runs the full width too.
    let plot = |rows: Vec<String>| rows[1..].to_vec();
    let with = plot(after("p"));
    let without = plot(after("p r"));
    assert!(middle(&without) < middle(&with), "{without:#?}");
}

#[test]
fn lines_leaving_the_bounds_are_left_out_as_ratatuis_are() {
    let plot = Plot {
        sets: vec![DataSet {
            name: None,
            points: vec![(0.0, 0.0), (1.0, 5.0), (2.0, 0.5)],
            scatter: false,
            style: "red".into(),
        }],
        x: (0.0, 2.0),
        y: (-1.0, 1.0),
        x_title: "",
        y_title: "",
        x_log: false,
    };
    let chart = chart(&plot, &graph());
    let points = chart.datasets()[0].points();
    assert_eq!(points[0], (0.0, 0.0));
    assert!(points[1].0.is_nan() && points[1].1.is_nan());
    assert_eq!(points[2], (2.0, 0.5));
}

#[test]
fn the_fps_counter_counts_frames_paused_or_not() {
    let script = Script::new().wait(Duration::from_millis(2100)).keys("q");
    let rows = screen(&run(app(), script, 100, 24));
    // A buffer every 40 ms: 25 a second.
    assert!(rows[0].contains("25fps"), "{:?}", rows[0]);
    // Paused, scope-tui still reads its source a frame at a time.
    let script = Script::new()
        .wait(Duration::from_millis(500))
        .keys("space")
        .wait(Duration::from_millis(2100))
        .keys("q");
    let rows = screen(&run(app(), script, 100, 24));
    assert!(
        rows[0].contains("25fps") && rows[0].contains("||"),
        "{:?}",
        rows[0]
    );
}

#[test]
fn past_the_end_of_the_input_the_scope_draws_silence() {
    let bytes: Vec<u8> = (0..64i16).flat_map(|s| (s * 300).to_le_bytes()).collect();
    let pcm = Pcm::new(Cursor::new(bytes), 2, 16);
    let options = Options {
        references: false,
        ..Options::default()
    };
    let app = scope_app(options, Box::new(pcm), Duration::from_millis(40));
    let script = Script::new().wait(Duration::from_millis(1100)).keys("q");
    let rows = screen(&run(app, script, 100, 24));
    // Still counting frames, and every channel flat on the zero line.
    assert!(rows[0].contains("25fps"), "{:?}", rows[0]);
    let drawn: Vec<_> = rows[1..]
        .iter()
        .filter(|r| braille(&[r.to_string()]) > 0)
        .collect();
    assert_eq!(drawn.len(), 1, "{drawn:#?}");
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
