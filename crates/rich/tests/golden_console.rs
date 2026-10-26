//! Parity for the rest of `rich/console.py` (0.0.23), against
//! `golden/console.tsv` (captured by `scripts/golden_console.py` from real
//! rich 15.0.0). Each fixture line is `name<TAB>json(expected)`; every name
//! has a Rust builder below. Environment detection reads an injected
//! environment (`ConsoleBuilder::environ`, upstream's `_environ`), so no
//! case depends on the machine running it.

use rich::align::HorizontalAlign;
use rich::console::{Environ, PrintObject as O, PrintOptions};
use rich::terminal_theme::{MONOKAI, NIGHT_OWLISH};
use rich::{
    ColorSystem, Console, Control, ExportHtmlOptions, ExportSvgOptions, Justify, Panel, Renderable,
    Rule, Text, Theme,
};

/// `_console`: truecolor, no highlighting.
fn builder(width: usize) -> rich::console::ConsoleBuilder {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .safe_box(false)
        .legacy_windows(false)
        .no_color(false)
}

fn console(width: usize) -> Console {
    builder(width).build()
}

/// `console.print(*objects, **options)` under a capture.
fn print(console: &Console, objects: &[O<'_>], options: PrintOptions<'_>) -> String {
    console.capture(|c| c.print_objects(objects, &options).unwrap())
}

fn print_str(console: &Console, content: &str) -> String {
    print(console, &[O::Str(content)], PrintOptions::default())
}

fn print_renderable(console: &Console, renderable: &dyn Renderable) -> String {
    print(
        console,
        &[O::Renderable(renderable)],
        PrintOptions::default(),
    )
}

fn lines(values: &[String]) -> String {
    values.iter().map(|value| format!("{value}\n")).collect()
}

fn py_bool(value: bool) -> String {
    if value { "True" } else { "False" }.to_string()
}

/// `str(console.color_system)`.
fn system_name(system: Option<ColorSystem>) -> String {
    match system {
        None => "None",
        Some(ColorSystem::Standard) => "standard",
        Some(ColorSystem::EightBit) => "256",
        Some(ColorSystem::Truecolor) => "truecolor",
        Some(ColorSystem::Windows) => "windows",
    }
    .to_string()
}

fn size(console: &Console) -> String {
    let size = console.size();
    format!("{}x{}", size.width, size.height)
}

fn environ(pairs: &[(&str, &str)]) -> Environ {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Python's `repr()` of a string, enough for these cases.
fn py_repr(value: &str) -> String {
    let mut out = String::from("'");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\'' => out.push_str("\\'"),
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", ch as u32))
            }
            ch => out.push(ch),
        }
    }
    out.push('\'');
    out
}

// --- group 1 ------------------------------------------------------------

fn console_quiet() -> String {
    let console = builder(20).quiet(true).build();
    let mut out = vec![
        py_bool(console.quiet()),
        print_str(&console, "[bold]hidden[/]"),
    ];
    console.set_quiet(false);
    out.push(print_str(&console, "[bold]shown[/]"));
    lines(&out)
}

fn console_size_setters() -> String {
    let mut console = builder(30).height(10).build();
    let mut out = vec![size(&console)];
    console.set_width(12);
    out.push(size(&console));
    console.set_height(4);
    out.push(size(&console));
    console.set_size(16, 3);
    out.push(size(&console));
    out.push(print_renderable(&console, &Rule::new("x")));
    lines(&out)
}

fn console_size_environ() -> String {
    let cases: [(Option<usize>, Option<usize>, Environ, bool); 8] = [
        (Some(30), Some(10), environ(&[]), true),
        (
            None,
            None,
            environ(&[("COLUMNS", "50"), ("LINES", "7")]),
            false,
        ),
        (
            None,
            None,
            environ(&[("COLUMNS", "50"), ("LINES", "7")]),
            true,
        ),
        (None, Some(9), environ(&[("COLUMNS", "50")]), true),
        (
            Some(33),
            None,
            environ(&[("COLUMNS", "5 0"), ("LINES", "7")]),
            false,
        ),
        (
            None,
            None,
            environ(&[("TERM", "dumb"), ("COLUMNS", "50")]),
            false,
        ),
        (
            None,
            None,
            environ(&[("TERM", "dumb"), ("COLUMNS", "50"), ("LINES", "9")]),
            false,
        ),
        (Some(40), None, environ(&[("TERM", "unknown")]), false),
    ];
    let out: Vec<String> = cases
        .into_iter()
        .map(|(width, height, environ, legacy)| {
            let mut builder = Console::builder()
                .force_terminal(true)
                .legacy_windows(legacy)
                .environ(environ);
            if let Some(width) = width {
                builder = builder.width(width);
            }
            if let Some(height) = height {
                builder = builder.height(height);
            }
            size(&builder.build())
        })
        .collect();
    lines(&out)
}

fn console_dumb_terminal() -> String {
    let cases: [(bool, Environ); 7] = [
        (true, environ(&[("TERM", "dumb")])),
        (true, environ(&[("TERM", "Dumb")])),
        (true, environ(&[("TERM", "unknown")])),
        (true, environ(&[("TERM", " dumb")])),
        (true, environ(&[("TERM", "xterm")])),
        (true, environ(&[])),
        (false, environ(&[("TERM", "dumb")])),
    ];
    let out: Vec<String> = cases
        .into_iter()
        .map(|(terminal, environ)| {
            let console = Console::builder()
                .force_terminal(terminal)
                .environ(environ)
                .build();
            format!(
                "{} {}",
                py_bool(console.is_dumb_terminal()),
                system_name(console.color_system())
            )
        })
        .collect();
    lines(&out)
}

fn console_color_detection() -> String {
    let environs = [
        environ(&[("TERM", "xterm-kitty")]),
        environ(&[("TERM", "kitty")]),
        environ(&[("TERM", "xterm-256")]),
        environ(&[("TERM", "xterm-256color")]),
        environ(&[("TERM", " XTERM-256COLOR ")]),
        environ(&[("TERM", "screen-16color")]),
        environ(&[("TERM", "xterm")]),
        environ(&[("COLORTERM", "truecolor")]),
        environ(&[("COLORTERM", " TrueColor "), ("TERM", "xterm-256color")]),
        environ(&[("COLORTERM", "24bit")]),
        environ(&[("COLORTERM", "truecolor-ish")]),
        environ(&[("COLORTERM", "yes"), ("TERM", "xterm-256color")]),
        environ(&[("TERM", "dumb"), ("COLORTERM", "truecolor")]),
        environ(&[]),
    ];
    let mut out: Vec<String> = environs
        .into_iter()
        .map(|environ| {
            let console = Console::builder()
                .force_terminal(true)
                .legacy_windows(false)
                .environ(environ)
                .build();
            system_name(console.color_system())
        })
        .collect();
    let given = |system, environ| {
        Console::builder()
            .force_terminal(true)
            .color_system(system)
            .environ(environ)
            .build()
            .color_system()
    };
    out.push(system_name(given(
        Some(ColorSystem::EightBit),
        environ(&[("TERM", "dumb")]),
    )));
    out.push(system_name(given(
        None,
        environ(&[("COLORTERM", "truecolor")]),
    )));
    out.push(system_name(
        Console::builder()
            .force_terminal(false)
            .environ(environ(&[("COLORTERM", "truecolor")]))
            .build()
            .color_system(),
    ));
    lines(&out)
}

fn console_terminal_detection() -> String {
    let environs = [
        environ(&[("TTY_COMPATIBLE", "1")]),
        environ(&[("TTY_COMPATIBLE", "0"), ("FORCE_COLOR", "1")]),
        environ(&[("TTY_COMPATIBLE", "1"), ("FORCE_COLOR", "")]),
        environ(&[("FORCE_COLOR", "")]),
        environ(&[("FORCE_COLOR", "0")]),
        environ(&[("FORCE_COLOR", "1"), ("TERM", "xterm-256color")]),
    ];
    let describe = |console: Console| {
        format!(
            "{} {}",
            py_bool(console.is_terminal()),
            system_name(console.color_system())
        )
    };
    let mut out: Vec<String> = environs
        .into_iter()
        .map(|environ| {
            describe(
                Console::builder()
                    .legacy_windows(false)
                    .environ(environ)
                    .build(),
            )
        })
        .collect();
    out.push(describe(
        Console::builder()
            .force_terminal(false)
            .legacy_windows(false)
            .environ(environ(&[("TTY_COMPATIBLE", "1")]))
            .build(),
    ));
    lines(&out)
}

fn console_no_color_environ() -> String {
    let mut out: Vec<String> = [
        environ(&[("NO_COLOR", "1")]),
        environ(&[("NO_COLOR", "")]),
        environ(&[]),
    ]
    .into_iter()
    .map(|environ| {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .environ(environ)
            .build();
        format!(
            "{} {}",
            py_bool(console.no_color()),
            py_repr(&print_str(&console, "[red]r[/] [bold]b[/]"))
        )
    })
    .collect();
    out.push(py_bool(
        Console::builder()
            .no_color(false)
            .environ(environ(&[("NO_COLOR", "1")]))
            .build()
            .no_color(),
    ));
    lines(&out)
}

const LONG: &str = "aaaa bbbb cccc dddd eeee ffff gggg";

fn console_soft_wrap() -> String {
    let console = builder(12).soft_wrap(true).build();
    let panel = Panel::new(Box::new(String::from("hi there"))).width(20);
    [
        format!("{}\n", py_bool(console.soft_wrap())),
        print_str(&console, LONG),
        print(
            &console,
            &[O::Str(LONG)],
            PrintOptions {
                soft_wrap: Some(false),
                ..Default::default()
            },
        ),
        print_renderable(&console, &panel),
        print(
            &console_without_soft_wrap(),
            &[O::Str(LONG)],
            PrintOptions {
                soft_wrap: Some(true),
                ..Default::default()
            },
        ),
    ]
    .concat()
}

fn console_without_soft_wrap() -> Console {
    console(12)
}

fn console_style() -> String {
    let console = builder(12).style("bold").build();
    let panel = Panel::new(Box::new(String::from("x"))).expand(false);
    let tx = Text::styled("tx", "red");
    let mut out = vec![
        print_str(&console, "hi"),
        print_renderable(&console, &panel),
        print(
            &console,
            &[O::Str("a"), O::Str("b")],
            PrintOptions {
                sep: "-",
                end: "!\n",
                ..Default::default()
            },
        ),
        print_str(&console, LONG),
        print(
            &console,
            &[O::Str("c")],
            PrintOptions {
                style: Some("italic".into()),
                ..Default::default()
            },
        ),
        print(
            &console,
            &[O::Str("mid")],
            PrintOptions {
                justify: Some(Justify::Center),
                ..Default::default()
            },
        ),
        print(&console, &[O::Text(&tx)], PrintOptions::default()),
    ];
    let mut named = builder(12).style("warning").build();
    named.push_theme(
        Theme::from_styles([("warning", "red on white")], true).unwrap(),
        true,
    );
    out.push(print_str(&named, "w"));
    out.concat()
}

fn console_style_soft_wrap() -> String {
    let console = builder(12).style("bold").soft_wrap(true).build();
    let panel = Panel::new(Box::new(String::from("x"))).expand(false);
    [
        print_str(&console, "hi"),
        print_str(&console, LONG),
        print_renderable(&console, &panel),
    ]
    .concat()
}

// --- group 2 ------------------------------------------------------------

fn console_begin_end_capture() -> String {
    let console = console(20);
    console.begin_capture();
    console.print_str("[bold]a[/]");
    console.rule("r", "─", "rule.line", HorizontalAlign::Center);
    let first = console.end_capture();
    console.begin_capture();
    console.print_str("a");
    console.begin_capture();
    console.print_str("b");
    let inner = console.end_capture();
    let outer = console.end_capture();
    console.begin_capture();
    console.show_cursor(false);
    console
        .print_objects(
            &[O::Str("z")],
            &PrintOptions {
                end: "",
                ..Default::default()
            },
        )
        .unwrap();
    let control = console.end_capture();
    let quiet = builder(20).quiet(true).build();
    quiet.begin_capture();
    quiet.print_str("q");
    let hushed = quiet.end_capture();
    lines(&[
        py_repr(&first),
        py_repr(&inner),
        py_repr(&outer),
        py_repr(&control),
        py_repr(&hushed),
    ])
}

// --- group 3 ------------------------------------------------------------

/// A recording console. Unlike the Python side, which writes to a throwaway
/// file, it writes to the test's stdout; the record is the same.
fn recording(width: usize) -> Console {
    builder(width).record(true).build()
}

fn record_session(console: &Console, control: bool) {
    let panel = Panel::new(Box::new(String::from("boxed"))).expand(false);
    let print = |objects: &[O<'_>], options: PrintOptions<'_>| {
        console.print_objects(objects, &options).unwrap();
    };
    print(
        &[
            O::Str("[bold red]hi[/] there"),
            O::Str("[link=https://x.io/?a=1&b=2]<&>[/]"),
        ],
        PrintOptions::default(),
    );
    print(&[O::Renderable(&panel)], PrintOptions::default());
    if control {
        console.show_cursor(false);
    }
    print(
        &[O::Str("open")],
        PrintOptions {
            end: "",
            ..Default::default()
        },
    );
    print(&[], PrintOptions::default());
    let _ = console.capture(|c| c.print_str("captured"));
    console.set_quiet(true);
    console.print_str("hushed");
    console.set_quiet(false);
    print(
        &[O::Str("[on blue]last[/]")],
        PrintOptions {
            style: Some("italic".into()),
            ..Default::default()
        },
    );
}

fn console_record_text() -> String {
    let console = recording(20);
    record_session(&console, true);
    let mut out = vec![
        py_repr(&console.export_text_recorded(false, false).unwrap()),
        py_repr(&console.export_text_recorded(false, true).unwrap()),
        py_repr(&console.export_text_recorded(true, false).unwrap()),
        py_repr(&console.export_text_recorded(true, false).unwrap()),
    ];
    let plain = builder(20).force_terminal(false).record(true).build();
    plain.control(&Control::bell());
    plain.print_str("x");
    out.push(py_repr(&plain.export_text_recorded(false, false).unwrap()));
    out.push(py_repr(&plain.export_text_recorded(true, true).unwrap()));
    out.push(
        Console::builder()
            .build()
            .export_text_recorded(true, false)
            .unwrap_err()
            .message()
            .to_string(),
    );
    lines(&out)
}

fn console_record_html() -> String {
    let console = recording(20);
    record_session(&console, false);
    let keep = ExportHtmlOptions {
        clear: false,
        ..Default::default()
    };
    [
        console.export_html_recorded(&keep).unwrap(),
        console
            .export_html_recorded(&ExportHtmlOptions {
                theme: Some(&MONOKAI),
                inline_styles: true,
                ..keep
            })
            .unwrap(),
        console
            .export_html_recorded(&ExportHtmlOptions {
                code_format: Some("{foreground}|{background}|{stylesheet}|{code}"),
                ..Default::default()
            })
            .unwrap(),
        console
            .export_html_recorded(&ExportHtmlOptions::default())
            .unwrap(),
    ]
    .join("\n")
}

const SVG_FORMAT: &str = "{unique_id}|{char_width}|{char_height}|{line_height}|{terminal_width}|{terminal_height}|{width}|{height}|{terminal_x}|{terminal_y}|{{lit}}\n{styles}\n{matrix}\n{backgrounds}\n{lines}\n{chrome}";

fn console_record_svg() -> String {
    let console = recording(20);
    record_session(&console, true);
    [
        console
            .export_svg_recorded(&ExportSvgOptions {
                unique_id: "u",
                clear: false,
                ..Default::default()
            })
            .unwrap(),
        console
            .export_svg_recorded(&ExportSvgOptions {
                title: "T",
                theme: Some(&NIGHT_OWLISH),
                unique_id: "v",
                clear: false,
                ..Default::default()
            })
            .unwrap(),
        console
            .export_svg_recorded(&ExportSvgOptions {
                code_format: Some(SVG_FORMAT),
                font_aspect_ratio: 0.5,
                unique_id: "w",
                ..Default::default()
            })
            .unwrap(),
        console
            .export_svg_recorded(&ExportSvgOptions {
                unique_id: "x",
                ..Default::default()
            })
            .unwrap(),
    ]
    .join("\n")
}

fn console_record_save() -> String {
    let console = recording(20);
    record_session(&console, false);
    let root = std::env::temp_dir().join(format!("rs-rich-console-save-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    console
        .save_text_recorded(root.join("a.txt"), false, false)
        .unwrap();
    console
        .save_text_recorded(root.join("b.txt"), false, true)
        .unwrap();
    console
        .save_html_recorded(
            root.join("c.html"),
            &ExportHtmlOptions {
                inline_styles: true,
                clear: false,
                ..Default::default()
            },
        )
        .unwrap();
    console
        .save_svg_recorded(
            root.join("d.svg"),
            &ExportSvgOptions {
                title: "S",
                unique_id: "s",
                ..Default::default()
            },
        )
        .unwrap();
    console
        .save_text_recorded(root.join("e.txt"), true, false)
        .unwrap();
    let out = ["a.txt", "b.txt", "c.html", "d.svg", "e.txt"]
        .iter()
        .map(|name| std::fs::read_to_string(root.join(name)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::remove_dir_all(&root).unwrap();
    out
}

fn build(name: &str) -> String {
    match name {
        "console_quiet" => console_quiet(),
        "console_size_setters" => console_size_setters(),
        "console_size_environ" => console_size_environ(),
        "console_dumb_terminal" => console_dumb_terminal(),
        "console_color_detection" => console_color_detection(),
        "console_terminal_detection" => console_terminal_detection(),
        "console_no_color_environ" => console_no_color_environ(),
        "console_soft_wrap" => console_soft_wrap(),
        "console_style" => console_style(),
        "console_style_soft_wrap" => console_style_soft_wrap(),
        "console_begin_end_capture" => console_begin_end_capture(),
        "console_record_text" => console_record_text(),
        "console_record_html" => console_record_html(),
        "console_record_svg" => console_record_svg(),
        "console_record_save" => console_record_save(),
        other => panic!("no builder for console case {other:?}"),
    }
}

const CASES: usize = 15;

#[test]
fn console_parity() {
    let data = include_str!("golden/console.tsv");
    let mut failures = Vec::new();
    let mut checked = 0;
    for raw in data.lines() {
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let (name, expected) = raw.split_once('\t').expect("name<TAB>json");
        let expected: String = serde_json::from_str(expected).expect("json string");
        let got = build(name);
        if got != expected {
            failures.push(format!(
                "{name}:\n  expected {expected:?}\n  got      {got:?}"
            ));
        }
        checked += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(checked, CASES, "expected every console case to run");
}

/// The single-renderable print paths (`print`, `print_str`, `render_export`)
/// honour `soft_wrap` and `style` as `print_objects` does.
#[test]
fn print_paths_agree_with_print_objects() {
    let panel = Panel::new(Box::new(String::from("x"))).expand(false);
    for console in [
        builder(12).style("bold").build(),
        builder(12).style("bold").soft_wrap(true).build(),
        builder(12).soft_wrap(true).build(),
        builder(12).style("on red").build(),
    ] {
        for content in ["hi", LONG] {
            let expected = print_str(&console, content);
            assert_eq!(console.capture(|c| c.print_str(content)), expected);
            assert_eq!(
                console.render_export(&console.build_text(content)),
                expected
            );
        }
        let expected = print_renderable(&console, &panel);
        assert_eq!(console.capture(|c| c.print(&panel)), expected);
        assert_eq!(console.render_export(&panel), expected);
    }
}

/// A replacement `on_broken_pipe` runs instead of upstream's exit.
#[test]
fn on_broken_pipe_runs_the_replacement() {
    let console = Console::builder()
        .on_broken_pipe(|console| console.set_quiet(true))
        .build();
    assert!(!console.quiet());
    console.on_broken_pipe();
    assert!(console.quiet());
}
