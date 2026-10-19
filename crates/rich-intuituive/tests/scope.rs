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

/// The app on the test signal, standing still, one buffer every 40 ms.
fn app() -> App {
    let options = Options::default();
    let demo = Demo::new(2, options.buffer as usize, true);
    scope_app(options, Input::Clock(Box::new(demo), PERIOD))
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
    let m = pcm.next().expect("a frame");
    assert_eq!(m[0].len(), 4, "padded to the buffer: {m:?}");
    assert_eq!(m[0][3], 0.0);
    assert!(pcm.next().is_none());
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
    let plot = spectroscope(&g, &s, &[VecDeque::from([tone.clone()])]);
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
    let frame = Demo::new(2, 2048, true)
        .next()
        .expect("the demo never ends");
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
    // From scope-tui's table (C0 is 16.35 Hz, C4 261.6), then nudged off a
    // multiple of channels × 2: A3 is 218 samples, and 219 on one channel.
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
    // Read as fast as it comes, then silence every 40 ms.
    let app = scope_app(options, play(pcm, silence(2, 16), None, PERIOD));
    let script = Script::new().wait(Duration::from_millis(1100)).keys("q");
    let rows = screen(&run(app, script, 100, 24));
    // Every channel flat on the zero line. (The reader thread keeps the
    // wall clock and the headless script a clock of its own, so how many
    // frames it has counted by then is not fixed.)
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
    let rows = screen(&common::run_open(app(), script, 120, 44));
    // The framework's help, from the bindings' descriptions.
    assert!(row_of(&rows, "Help").is_some(), "{rows:#?}");
    assert!(row_of(&rows, "phase difference").is_some(), "{rows:#?}");
    assert!(row_of(&rows, "scale up").is_some(), "{rows:#?}");
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
fn an_audio_input_that_cannot_open_ends_the_app_with_why() {
    let input = Input::Audio(Some("no such sound card, surely".into()));
    let mut backend = rich_interact::headless::Headless::new(
        Script::new().wait(Duration::from_millis(500)),
        80,
        24,
    );
    let error = scope_app(Options::default(), input)
        .run_on(&mut backend)
        .expect_err("no such device");
    assert!(
        error.to_string().contains("no audio input named"),
        "{error}"
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
