//! The scope-tui rebuild (examples/scope.rs): its signal processing, its
//! sources, and the app driven by keys on the test signal.

// The example reads raw PCM, behind the `pcm` feature.
#![cfg(feature = "pcm")]

mod common;

#[path = "../examples/scope.rs"]
#[allow(dead_code)]
mod scope;

use std::collections::VecDeque;
use std::io::Cursor;
use std::time::Duration;

use common::{row_of, run, screen};
use intuituive::audio::{self, Capture, Format};
use intuituive::interact::BackendKind;
use intuituive::App;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use rich::Console;
use rich_interact::headless::Script;
use scope::*;

fn args(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

const PERIOD: Duration = Duration::from_millis(40);

fn silence(channels: usize, buffer: usize) -> Matrix {
    vec![vec![0.0; buffer]; channels]
}

/// The app on `input`, its threads on the app's clock: what a test sees
/// is the same on every run.
fn app_on(options: Options, input: Input) -> App {
    scope_app(options, input).wait_for_tasks(true)
}

/// The app on the test signal, standing still, one buffer every 40 ms.
fn app() -> App {
    let options = Options::default();
    let demo = Demo::new(2, options.buffer as usize, options.rate, true);
    app_on(options, Input::Capture(Capture::simulated(demo, PERIOD)))
}

/// `buffers` as s16le PCM, interleaved, read back by `audio::pcm`.
fn pcm_of(buffers: &[Matrix]) -> audio::Pcm<Cursor<Vec<u8>>> {
    let (channels, buffer) = (buffers[0].len(), buffers[0][0].len());
    let bytes = (buffers.iter())
        .flat_map(|m| (0..buffer).flat_map(move |i| (0..channels).map(move |c| m[c][i])))
        .flat_map(|s| ((s * 32768.0).round() as i16).to_le_bytes())
        .collect();
    audio::pcm(Cursor::new(bytes), Format::S16Le, channels, buffer)
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
fn files_are_read_as_scope_tuis_s16le() {
    // scope-tui divides each 16-bit sample by 32768, as `audio::pcm` does.
    let bytes: Vec<u8> = [16384i16, -32768, 0, 32767]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let m = audio::deinterleave(&bytes, Format::S16Le, 2);
    assert_eq!(m, vec![vec![0.5, 0.0], vec![-1.0, 32767.0 / 32768.0]]);
    // A short last buffer is padded with silence, as scope-tui's, then it
    // ends.
    let bytes: Vec<u8> = [1000i16, 2000, 3000]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    let mut pcm = audio::pcm(Cursor::new(bytes), Format::S16Le, 1, 4);
    let m = pcm.next().expect("a frame");
    assert_eq!(m[0].len(), 4, "padded to the buffer: {m:?}");
    assert_eq!(m[0][3], 0.0);
    assert!(pcm.next().is_none());
}

#[test]
fn the_trigger_finds_a_debounced_crossing() {
    let data = [-0.5, 0.2, -0.1, -0.2, 0.3, 0.4, 0.5];
    let at = |depth, falling_edge, threshold| {
        Oscilloscope {
            triggering: true,
            falling_edge,
            threshold,
            depth,
            peaks: true,
        }
        .trigger_at(&data)
    };
    // Rising through 0 at 1 bounces back; at 3 it stays up for 3 samples.
    assert_eq!(at(1, false, 0.0), Some(0));
    assert_eq!(at(2, false, 0.0), Some(3));
    assert_eq!(at(3, false, 0.0), Some(3));
    assert_eq!(at(1, true, 0.0), Some(1), "falling at 1");
    assert_eq!(at(4, false, 0.0), None, "too near the end");
    // The threshold in 16-bit units: 0.25 of full scale.
    assert_eq!(at(1, false, 8192.0), Some(3));
    let idle = Oscilloscope::default();
    assert_eq!(idle.trigger_at(&data), Some(0), "not triggering");
}

#[test]
fn the_spectroscope_plots_every_bin_of_rich_exts_fft() {
    // scope-tui plots the whole transform, the mirrored half above the
    // Nyquist frequency too, where `dsp::spectrum` stops at it.
    let g = Graph {
        references: false,
        ..graph()
    };
    let tone: Vec<f64> = (0..100).map(|i| (i as f64 * 0.7).sin() * 0.5).collect();
    let s = Spectroscope {
        log_y: false,
        ..Spectroscope::default()
    };
    let plot = spectroscope(&g, &s, &VecDeque::from([vec![tone.clone()]]));
    let bins = rich_ext::dsp::fft(&tone);
    assert_eq!(plot.sets[0].points.len(), bins.len());
    for (point, bin) in plot.sets[0].points.iter().zip(&bins) {
        assert!((point.1 - rich_ext::dsp::magnitude(*bin)).abs() < 1e-9);
    }
}

#[test]
fn the_spectroscope_peaks_at_the_test_tones() {
    let g = Graph {
        references: false,
        ..graph()
    };
    let frame = Demo::new(2, 2048, 48000, true)
        .next()
        .expect("the demo never ends");
    let plot = spectroscope(&g, &Spectroscope::default(), &VecDeque::from([frame]));
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

#[test]
fn the_test_signal_turns_unless_held_still() {
    let mut moving = Demo::new(2, 512, 48000, false);
    let (a, b) = (moving.next().unwrap(), moving.next().unwrap());
    assert_ne!(a, b, "the right channel drifts");
    let mut still = Demo::new(2, 512, 48000, true);
    let first = still.next().unwrap();
    assert_eq!(still.next().unwrap(), first, "noise included");
    // The same first buffer either way, but for the right channel's drift.
    assert_eq!(first[0], a[0]);
    assert!(first.iter().flatten().all(|s| s.abs() <= 1.0));
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
    assert!(c.options.scatter && !c.options.braille);
    let file = |path: &str, limit_rate| Source::File {
        path: path.into(),
        limit_rate,
    };
    assert_eq!(c.source, file("-", true));
    let c = parse(&args("--palette-color Green,blue --still")).unwrap();
    assert_eq!(c.options.palette, ["green", "blue"]);
    assert_eq!(c.source, Source::Demo { still: true });
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
    // C4 is 261.6 Hz, then nudged off a multiple of channels × 2: A3 is
    // 218 samples, and 219 on one channel.
    assert_eq!(tune("C4", 48000, 2), Some(183));
    assert_eq!(tune("A3", 48000, 2), Some(218));
    assert_eq!(tune("A3", 48000, 1), Some(219));
    assert_eq!(tune("H2", 48000, 2), None);
    // -l before or after the path, options after the source.
    let c = parse(&args("file -l x.pcm --scatter")).unwrap();
    assert!(c.options.scatter && c.source == file("x.pcm", true));
    assert!(parse(&args("-l")).is_err(), "-l belongs to file");
    assert!(parse(&args("file a file b")).is_err(), "one source");
    // The sound card sources, with their own options.
    let audio = |device: Option<&str>, timeout, list| Source::Audio {
        device: device.map(String::from),
        timeout,
        list,
    };
    assert_eq!(parse(&args("audio")).unwrap().source, audio(None, 5, false));
    assert_eq!(
        parse(&args("audio --list -c 1")).unwrap().source,
        audio(None, 5, true)
    );
    assert_eq!(
        parse(&args("audio USB --timeout 2")).unwrap().source,
        audio(Some("USB"), 2, false)
    );
    assert_eq!(
        parse(&args("pulse --server-buffer 8")).unwrap().source,
        Source::Pulse {
            device: None,
            server_buffer: 8
        }
    );
    assert!(parse(&args("--list")).is_err(), "--list belongs to audio");
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
fn tune_reads_notes_as_scope_tui_does_on_rich_exts_pitches() {
    // A note without an octave is in octave 0, as scope-tui reads it:
    // A0 is 27.5 Hz, 1745 samples at 48 kHz.
    assert_eq!(tune("A", 48000, 2), Some(1745));
    assert_eq!(tune("Eb", 48000, 2), tune("Eb0", 48000, 2));
    assert_eq!(tune("C#3", 48000, 2), tune("Db3", 48000, 2));
    // scope-tui's own table (`dsp::NoteTable::ScopeTui`), rounded to the
    // hundredth of a hertz in octave 0 and doubled: E2 is 20.60 × 4 =
    // 82.4 Hz, 583 samples, where equal temperament's 82.41 Hz gives 582.
    assert_eq!(tune("E2", 48000, 2), Some(583));
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
    // Ctrl ×5 and alt ×⅕, as scope-tui steps them: 25 × 5 and 25 / 5.
    let rows = after("ctrl+right alt+left ctrl+up alt+down");
    assert!(rows[0].contains("2168/2048 spf"), "{:?}", rows[0]);
    assert!(rows[0].contains("-1.05x+"), "{:?}", rows[0]);
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
    let plot = spectroscope(&g, &Spectroscope::default(), &VecDeque::from([vec![tone]]));
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
    let plot = |points| Plot {
        sets: vec![DataSet {
            name: None,
            points,
            scatter: false,
            style: "red".into(),
        }],
        x: (0.0, 2.0),
        y: (-1.0, 1.0),
        x_title: "",
        y_title: "",
        x_log: false,
        x_grid: Vec::new(),
        y_grid: Vec::new(),
    };
    let console = Console::builder().width(20).color_system(None).build();
    let draw = |points| {
        let chart = chart(&plot(points), &graph()).width(20).height(6);
        console.render_export(&chart)
    };
    let dots = |text: String| braille(&[text]);
    // Inside, a line; with an end outside, only the points inside.
    assert!(dots(draw(vec![(0.0, 0.0), (2.0, 0.5)])) > 10);
    assert_eq!(dots(draw(vec![(0.0, 0.0), (1.0, 5.0), (2.0, 0.5)])), 2);
}

/// `plot` with its reference lines as datasets of their own, before the
/// traces, as scope-tui makes them (and as round 2 of the rebuild drew
/// them, checked against ratatui 0.29 by eye at three sizes).
fn as_reference_datasets(plot: &Plot, g: &Graph) -> Plot {
    let line = |points| DataSet {
        name: None,
        points,
        scatter: false,
        style: g.axis.clone(),
    };
    let x = plot
        .x_grid
        .iter()
        .map(|x| line(vec![(*x, plot.y.0), (*x, plot.y.1)]));
    let y = plot
        .y_grid
        .iter()
        .map(|y| line(vec![(plot.x.0, *y), (plot.x.1, *y)]));
    Plot {
        sets: y.chain(x).chain(plot.sets.iter().cloned()).collect(),
        x_grid: Vec::new(),
        y_grid: Vec::new(),
        ..plot.clone()
    }
}

#[test]
fn reference_lines_are_drawn_as_scope_tuis_datasets_are() {
    // The axis grid stands for scope-tui's reference datasets: the
    // oscilloscope's zero line and the vectorscope's cross, with a trace
    // over them, cell for cell as datasets of their own draw them.
    let g = graph();
    let (width, height) = (40usize, 12usize);
    let trace: Vec<f64> = (0..=2048).map(|i| (i as f64 / 300.0).sin() * 0.8).collect();
    let lissajous: Matrix = vec![
        (0..400).map(|i| (i as f64 / 40.0).sin() * 0.7).collect(),
        (0..400).map(|i| (i as f64 / 27.0).cos() * 0.7).collect(),
    ];
    let still = Oscilloscope {
        peaks: false,
        ..Oscilloscope::default()
    };
    let cases = [
        ("oscilloscope", oscilloscope(&g, &still, &vec![trace])),
        ("vectorscope", vectorscope(&g, &lissajous)),
    ];
    let console = Console::builder().width(width).color_system(None).build();
    let draw = |plot: &Plot| console.render_export(&chart(plot, &g).width(width).height(height));
    for (name, plot) in cases {
        let grid = draw(&plot);
        let datasets = draw(&as_reference_datasets(&plot, &g));
        assert_eq!(grid, datasets, "{name}:\n{grid}\n{datasets}");
    }
    // Where the cross meets with nothing over it, each grid line is a
    // layer of its own, the y axis's first: scope-tui's second dataset
    // drawn over its first (`⢸`), not the two merged (`⣸`).
    let cross = vectorscope(&g, &vec![vec![], vec![]]);
    let (grid, datasets) = (draw(&cross), draw(&as_reference_datasets(&cross, &g)));
    assert_eq!(grid, datasets, "the cross:\n{grid}\n{datasets}");
    assert!(grid.contains('⢸') && !grid.contains('⣸'), "{grid}");
}

#[test]
fn the_fps_counter_counts_frames_paused_or_not() {
    // On the app's clock, frame for frame: a buffer every 40 ms is 25 a
    // second, counted by `rate` from `on_write_ref`.
    let mut driver = app().driver(100, 24);
    driver.advance(Duration::from_millis(2100));
    let rows = driver.screen().plain();
    assert!(rows[0].contains(" 25fps"), "{:?}", rows[0]);
    // Paused, scope-tui still reads its source a frame at a time.
    driver.press("space");
    driver.advance(Duration::from_secs(2));
    let rows = driver.screen().plain();
    assert!(
        rows[0].contains(" 25fps") && rows[0].contains("||"),
        "{:?}",
        rows[0]
    );
}

#[test]
fn the_frame_rate_is_a_buffers_worth_of_time_as_scope_tuis() {
    // scope-tui draws each buffer as it comes: 48000 / 2048 is 23.4 a
    // second, 48000 / 1024 46.9. The test signal comes a buffer's worth
    // of time apart, and the app is woken for each, faster than 20 a
    // second too.
    for (buffer, fps) in [(2048, " 23fps"), (1024, " 47fps"), (4096, " 12fps")] {
        let command = parse(&args(&format!("-b {buffer} --still"))).unwrap();
        let input = input(&command.source, &command.options).unwrap();
        let mut driver = app_on(command.options, input).driver(100, 24);
        driver.advance(Duration::from_millis(2100));
        let rows = driver.screen().plain();
        assert!(rows[0].contains(fps), "-b {buffer}: {:?}", rows[0]);
    }
}

#[test]
fn equal_buffers_are_frames_too() {
    // Digital silence: every buffer the same. A watch would see one; the
    // scope counts each, as scope-tui does.
    let silent = std::iter::repeat(silence(2, 64));
    let input = Input::Capture(Capture::simulated(silent, Duration::from_millis(10)));
    let mut driver = app_on(Options::default(), input).driver(100, 24);
    driver.advance(Duration::from_millis(1050));
    let rows = driver.screen().plain();
    assert!(rows[0].contains(" 100fps"), "{:?}", rows[0]);
}

#[test]
fn a_capture_that_stalls_ends_the_app_as_scope_tuis_timeout() {
    // `audio --timeout S`: no buffer for S seconds ends the app with
    // scope-tui's message, a simulated capture standing in for the card.
    let capture = Capture::simulated(std::iter::repeat(silence(2, 64)), PERIOD)
        .stall_after(Duration::from_secs(2));
    let control = capture.clone();
    let mut driver = app_on(Options::default(), Input::Capture(capture)).driver(100, 24);
    driver.advance(Duration::from_secs(3));
    assert!(driver.take_error().is_none(), "running");
    control.stall();
    driver.advance(Duration::from_millis(1900));
    assert!(driver.take_error().is_none(), "not yet stalled");
    driver.advance(Duration::from_millis(200));
    let error = driver.take_error().expect("stalled");
    assert_eq!(error.to_string(), STALLED);
}

#[test]
fn a_capture_that_fails_ends_the_app_saying_why() {
    let capture = Capture::simulated(std::iter::repeat(silence(2, 64)), PERIOD);
    let control = capture.clone();
    let mut driver = app_on(Options::default(), Input::Capture(capture)).driver(100, 24);
    driver.advance(Duration::from_millis(500));
    control.fail("the device was unplugged");
    driver.advance(Duration::from_millis(100));
    let error = driver.take_error().expect("failed");
    assert_eq!(error.to_string(), "the device was unplugged");
}

#[test]
fn past_the_end_of_the_input_the_scope_draws_silence() {
    let bytes: Vec<u8> = (0..64i16).flat_map(|s| (s * 300).to_le_bytes()).collect();
    let pcm = audio::pcm(Cursor::new(bytes), Format::S16Le, 2, 16);
    let options = Options {
        references: false,
        ..Options::default()
    };
    // Two buffers of 16 at 400 Hz, one every 40 ms (`-l`), then silence
    // every 40 ms, all on the app's clock.
    let input = play(pcm, 400, true);
    let mut driver = app_on(options, input).driver(100, 24);
    driver.advance(Duration::from_millis(1010));
    let rows = driver.screen().plain();
    // 25 frames a second, the file's two and silence's; every channel flat
    // on the zero line.
    assert!(rows[0].contains(" 25fps"), "{:?}", rows[0]);
    let drawn: Vec<_> = rows[1..]
        .iter()
        .filter(|r| braille(&[r.to_string()]) > 0)
        .collect();
    assert_eq!(drawn.len(), 1, "{drawn:#?}");
}

#[test]
fn the_end_of_a_file_or_a_pipe_does_not_end_the_scope() {
    // scope-tui's file source pads the last buffer with silence and then
    // reads silence for ever: the end of a file is not an error, and
    // neither is a pipe's (read as fast as it comes, without `-l`).
    for limit_rate in [true, false] {
        let pcm = pcm_of(&[vec![vec![0.5; 16]; 2]]);
        let mut driver = app_on(Options::default(), play(pcm, 400, limit_rate)).driver(100, 24);
        driver.advance(Duration::from_millis(2100));
        assert!(driver.take_error().is_none(), "-l {limit_rate}");
        let rows = driver.screen().plain();
        assert!(rows[0].contains(" 25fps"), "-l {limit_rate}: {:?}", rows[0]);
    }
}

#[test]
fn a_file_plays_at_its_sample_rate_with_l() {
    // Four buffers of 16 at 400 Hz with `-l`, 40 ms each: after 100 ms the
    // third shows.
    let buffers: Vec<Matrix> = (1..=4).map(|n| vec![vec![n as f64 / 8.0; 16]]).collect();
    let options = Options {
        channels: 1,
        buffer: 16,
        rate: 400,
        references: false,
        ..Options::default()
    };
    let input = play(pcm_of(&buffers), options.rate, true);
    let mut driver = app_on(options, input).driver(60, 18);
    driver.advance(Duration::from_millis(100));
    let level = |rows: &[String]| {
        rows.iter()
            .position(|r| braille(std::slice::from_ref(r)) > 2)
    };
    let third = level(&driver.screen().plain());
    driver.advance(Duration::from_millis(40));
    let fourth = level(&driver.screen().plain());
    assert!(third.is_some() && fourth < third, "{third:?} {fourth:?}");
}

#[test]
fn each_scope_takes_its_own_keys_only_while_it_shows() {
    // t triggers in the oscilloscope; in the vectorscope it is not bound.
    let rows = after("tab t tab tab");
    assert!(
        rows[0].contains("oscillo") && rows[0].contains("live"),
        "{:?}",
        rows[0]
    );
    // p: peaks in the oscilloscope, the phase difference in the
    // spectroscope, each scope's own.
    let rows = after("tab tab p");
    assert!(row_of(&rows, "phase diff").is_some(), "{rows:#?}");
    let rows = after("p tab tab");
    assert!(row_of(&rows, "phase diff").is_none(), "{rows:#?}");
    // PgUp: the trigger's threshold in one, the averaging in the other.
    let rows = after_at("pageup tab tab pageup", 160);
    assert!(rows[0].contains("2x avg"), "{:?}", rows[0]);
    let rows = after("pageup tab tab pageup tab");
    assert!(rows[0].contains("live"), "not triggering: {:?}", rows[0]);
    let rows = after("t pageup tab tab pageup tab");
    assert!(rows[0].contains("^ 250 trigger"), "{:?}", rows[0]);
    // The debounce by its named keys: = and + up, - and _ down.
    let rows = after("t equals plus plus minus underscore");
    assert!(rows[0].contains("^ 0:11 trigger"), "{:?}", rows[0]);
}

/// The help as it opens after `keys`.
fn help_after(keys: &str) -> Vec<String> {
    let script = Script::new()
        .wait(Duration::from_millis(100))
        .keys(keys)
        .keys("?")
        .wait(Duration::from_millis(100));
    screen(&common::run_open(app(), script, 120, 50))
}

#[test]
fn question_mark_lists_the_keys_of_the_scope_showing() {
    let rows = help_after("");
    // The framework's help, from the bindings' descriptions: the keys
    // every scope takes, then the oscilloscope's under its name.
    assert!(row_of(&rows, "Help").is_some(), "{rows:#?}");
    let at = |rows: &[String], text| {
        row_of(rows, text).unwrap_or_else(|| panic!("{text:?} in {rows:#?}"))
    };
    // A section's heading starts at the box's edge.
    let heading = |rows: &[String], name: &str| row_of(rows, &format!("│{name} "));
    assert!(
        heading(&rows, "oscilloscope").expect("a heading") < at(&rows, "rising or falling edge")
    );
    assert!(at(&rows, "threshold up") > 0);
    assert!(row_of(&rows, "Hann window").is_none(), "{rows:#?}");
    assert!(heading(&rows, "spectroscope").is_none(), "{rows:#?}");
    // A key with its modifiers is one row, said as scope-tui's docs say
    // them; the named punctuation keys show as their characters.
    assert!(at(&rows, "scale up (shift ×10, ctrl ×5, alt ⅕)") > 0);
    let debounce = &rows[at(&rows, "debounce up by 10")];
    assert!(debounce.contains('+'), "{debounce:?}");
    // In the spectroscope, its keys instead; in the vectorscope, none of
    // either.
    let rows = help_after("tab tab");
    assert!(heading(&rows, "spectroscope").expect("a heading") < at(&rows, "Hann window"));
    assert!(heading(&rows, "oscilloscope").is_none(), "{rows:#?}");
    assert!(at(&rows, "phase difference") > 0);
    assert!(
        row_of(&rows, "rising or falling edge").is_none(),
        "{rows:#?}"
    );
    let rows = help_after("tab");
    assert!(row_of(&rows, "Hann window").is_none(), "{rows:#?}");
    assert!(row_of(&rows, "threshold up").is_none(), "{rows:#?}");
    assert!(at(&rows, "reset the view") > 0);
}

#[test]
fn the_header_cells_sit_where_ratatuis_table_puts_them() {
    // scope-tui's header is a one-row Table: its columns are a Layout of
    // percentages of the whole width, one column apart, from the start.
    let percents = [35, 25, 7, 13, 6, 6, 6];
    for width in [60u16, 80, 100, 110, 140, 200] {
        let rows = after_at("", width);
        let header = &rows[0];
        let layout = Layout::horizontal(percents.map(Constraint::Percentage))
            .flex(Flex::Start)
            .spacing(1)
            .split(Rect::new(0, 0, width, 1));
        let column = |text: &str| header.find(text);
        // What every cell shows of itself even when cut to three columns.
        let cells = ["osc", "liv", "-1", "20", "fps", "--", "|>"];
        for (index, (part, area)) in cells.iter().zip(layout.iter()).enumerate() {
            let at = if *part == "fps" {
                // The fps cell starts at its number.
                let end = column(part).unwrap();
                header[..end].rfind(' ').map_or(0, |space| space + 1)
            } else {
                column(part).unwrap_or_else(|| panic!("{part:?} at {width}: {header:?}"))
            };
            // Character offsets: the header is ASCII.
            assert_eq!(
                at,
                usize::from(area.x),
                "cell {index} at {width}: {header:?}"
            );
        }
    }
}

#[test]
fn the_header_is_cut_as_a_table_cell_is() {
    // At 60 columns the 25 % cell holds 14: the spectroscope's text is cut,
    // with no ellipsis, and the row stays one line.
    let rows = after_at("tab tab", 60);
    assert!(rows[0].contains("live  ---  23."), "{:?}", rows[0]);
    assert!(!rows[0].contains('…'), "{:?}", rows[0]);
    assert!(rows[1].starts_with("| level"), "{:?}", rows[1]);
}

#[cfg(feature = "audio")]
#[test]
fn an_audio_input_that_cannot_open_fails_before_the_app() {
    let command = parse(&args("audio no-such-sound-card-surely")).unwrap();
    let error = input(&command.source, &command.options)
        .err()
        .expect("no such device");
    assert!(
        error.to_string().contains("no audio input named"),
        "{error}"
    );
}

#[cfg(feature = "audio")]
#[test]
fn audio_list_prints_as_scope_tuis() {
    let device = audio::Device {
        id: "hw:CARD=PCH,DEV=0".into(),
        name: "HDA Intel PCH, ALC892 Analog".into(),
        default: false,
        host: audio::Host::Default,
    };
    let config = |channels, buffer| audio::DeviceConfig {
        channels,
        min_rate: 8000,
        max_rate: 192000,
        buffer,
        format: "i16".into(),
    };
    let listed = list_devices(&[(device, vec![config(2, Some((32, 65536))), config(1, None)])]);
    assert_eq!(
        listed,
        "> hw:CARD=PCH,DEV=0\n  + 2ch 8000-192000hz 32-65536buf (i16)\n  \
         + 1ch 8000-192000hz 0-0buf (i16)\n"
    );
}

#[cfg(not(feature = "audio"))]
#[test]
fn without_the_audio_feature_the_sound_sources_say_how_to_get_it() {
    let command = parse(&args("audio")).unwrap();
    let error = input(&command.source, &command.options)
        .err()
        .expect("no audio in this build");
    assert!(error.to_string().contains("--features audio"), "{error}");
    assert!(error.to_string().contains("file -"), "{error}");
}
