//! The `macros` feature: checked markup, derive and convenience macros
//! (#36, #280–#285, #385).
#![cfg(feature = "macros")]
use rich::{Console, Renderable, Style};
use rich_ext::derive::{self, Field, RichRecord};
use rich_ext::{
    markup, rich_dbg, rich_panel, rich_table, rich_tree, richf, style, theme_key, Rich,
};

fn plain(renderable: &dyn Renderable) -> String {
    Console::builder()
        .width(50)
        .build()
        .render_to_string(renderable)
}

fn ansi(renderable: &dyn Renderable) -> String {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .width(50)
        .build()
        .render_to_string(renderable)
}

#[test]
fn richf_formats_escapes_and_styles() {
    let name = "[red]mallory[/red]";
    let count = 7;
    let text = richf!("[bold]{name}[/] has {count:>3} items, {} left", 2);
    // User data is escaped: its brackets print instead of styling.
    assert_eq!(text.plain(), "[red]mallory[/red] has   7 items, 2 left");
    let out = ansi(&text);
    assert!(
        out.starts_with("\x1b[1m[red]mallory[/red]\x1b[0m"),
        "{out:?}"
    );
}

#[test]
fn richf_supports_named_args_indexes_and_width_refs() {
    let width = 6;
    let text = richf!("[green]{0}|{v:>width$}|{0}[/]", "x", v = 1.5);
    assert_eq!(text.plain(), "x|   1.5|x");
    let text = richf!(keys["app.title"], "[app.title]{}[/] {{literal}}", "T");
    assert_eq!(text.plain(), "T {literal}");
}

#[test]
fn a_placeholder_inside_a_tag_is_markup() {
    let color = "magenta";
    let text = richf!("[{color}]hue[/]");
    assert_eq!(text.plain(), "hue");
    assert!(ansi(&text).contains("\x1b[35mhue"), "{:?}", ansi(&text));
}

#[test]
fn style_theme_key_and_markup_are_checked_literals() {
    assert_eq!(style!("bold red"), Style::parse("bold red").unwrap());
    assert_eq!(theme_key!("repr.number"), "repr.number");
    assert_eq!(markup!("[green]ok[/]"), "[green]ok[/]");
    assert_eq!(markup!(keys["x.y"], "[x.y]a[/]"), "[x.y]a[/]");
}

#[derive(Rich)]
#[rich(title = "Server")]
struct Server {
    #[rich(label = "Host", style = "bold cyan")]
    host: String,
    #[rich(order = -1)]
    port: u16,
    #[rich(skip)]
    #[allow(dead_code)]
    secret: String,
    #[rich(format = "{:.1}%", justify = "right")]
    load: f64,
}

fn server(host: &str, load: f64) -> Server {
    Server {
        host: host.into(),
        port: 8080,
        secret: "hunter2".into(),
        load,
    }
}

#[test]
fn derive_renders_labelled_fields_in_order() {
    let text = plain(&server("example.com", 12.25));
    assert_eq!(
        text,
        "Server\nport: 8080       \nHost: example.com\nload: 12.2%      "
    );
    assert!(!text.contains("hunter2"));
    let (title, fields) = server("a", 1.0).rich_record();
    assert_eq!(title.as_deref(), Some("Server"));
    let labels: Vec<_> = fields.iter().map(|f| f.label.as_str()).collect();
    assert_eq!(labels, ["port", "Host", "load"]);
    assert!(ansi(&server("a", 1.0)).contains("\x1b[1;36ma"));
}

#[derive(Rich)]
enum Event {
    Started { pid: u32 },
    Stopped(#[rich(label = "code")] i32),
    Idle,
}

#[test]
fn derive_uses_variant_names_as_titles() {
    assert_eq!(plain(&Event::Started { pid: 42 }), "Started\npid: 42");
    assert_eq!(plain(&Event::Stopped(1)), "Stopped\ncode: 1");
    assert_eq!(plain(&Event::Idle), "Idle");
}

#[test]
fn derive_table_lays_records_out_as_rows() {
    let rows = [server("a.io", 1.0), server("b.io", 99.5)];
    let text = plain(&derive::table(&rows));
    assert!(text.contains("┃ port ┃ Host ┃  load ┃"), "{text}");
    assert!(text.contains("│ 8080 │ b.io │ 99.5% │"), "{text}");
    let events = [Event::Started { pid: 1 }, Event::Stopped(2)];
    let text = plain(&derive::table(&events));
    assert!(text.contains("┃ pid ┃ code ┃"), "{text}");
}

#[derive(Rich)]
#[rich(panel)]
struct Boxed {
    #[rich(display)]
    name: String,
}

#[derive(Rich)]
#[rich(table, title = "Row")]
struct Row {
    a: u8,
}

#[test]
fn panel_and_table_presentations() {
    let text = plain(&Boxed { name: "x".into() });
    assert!(
        text.starts_with("╭") && text.contains("Boxed") && text.contains("name: x"),
        "{text}"
    );
    let text = plain(&Row { a: 3 });
    assert!(
        text.contains("Row") && text.contains("┃ a ┃") && text.contains("│ 3 │"),
        "{text}"
    );
}

struct Manual;
impl RichRecord for Manual {
    fn rich_record(&self) -> (Option<String>, Vec<Field>) {
        (None, vec![Field::new("k", "v")])
    }
}

#[test]
fn hand_written_records_render_too() {
    assert_eq!(plain(&derive::table(&[Manual])).lines().count(), 5);
}

#[test]
fn convenience_macros_build_core_types() {
    let table = rich_table!(["Name", "Age"], ["Alice", 30], ["Bob", 4]);
    let text = plain(&table);
    assert!(
        text.contains("┃ Name  ┃ Age ┃") && text.contains("│ Bob   │ 4   │"),
        "{text}"
    );
    let panel = rich_panel!("[bold]ready[/]", title = "status");
    let text = plain(&panel);
    assert!(text.contains("status") && text.contains("ready"), "{text}");
    let panel = rich_panel!(rich::Text::new("[raw]"), subtitle = "s");
    assert!(plain(&panel).contains("[raw]"));
    let tree = rich_tree!("src" => ["main.rs", "lib" => ["mod.rs"], "build.rs"]);
    let text = plain(&tree);
    assert_eq!(text.lines().count(), 5, "{text}");
    assert!(
        text.contains("mod.rs") && text.ends_with("build.rs"),
        "{text}"
    );
}

#[test]
fn rich_dbg_returns_its_value() {
    assert_eq!(rich_dbg!(6 * 7), 42);
    let (a, b) = rich_dbg!(1, "two");
    assert_eq!((a, b), (1, "two"));
    let line = rich_ext::macros::dbg_line("f.rs", 1, 2, "x", &vec![1, 2]);
    assert!(
        line.plain().starts_with("[f.rs:1:2] x = [\n    1,"),
        "{:?}",
        line.plain()
    );
}

#[test]
fn compile_errors_name_the_problem() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}

/// The print macros wrap `richf!` in `macro_rules!`; inline captures must
/// still resolve at the caller, as they do for `format!` (and `richf!`).
#[test]
fn print_macros_capture_locals_like_format() {
    use rich_ext::{rich_eprintln, rich_println, rich_trace};
    let x = 7;
    let name = "[x]";
    let width = 5;
    rich_println!("[bold]{x}[/]");
    rich_println!("{x:>5}|{name}|{x:>width$}");
    rich_println!("{} and {0}", x);
    rich_println!("{label}={x}", label = "k");
    rich_eprintln!("[bold]{x}[/]");
    rich_eprintln!("{x:>5}|{name}|{x:>width$}");
    rich_eprintln!("{} and {0}", x);
    rich_eprintln!("{label}={x}", label = "k");
    rich_trace!("[bold]{x}[/]");
    rich_trace!("{x:>5}|{name}|{x:>width$}");
    rich_trace!("{} and {0}", x);
    rich_trace!("{label}={x}", label = "k");
}

/// A user's own `macro_rules!` wrapper around `richf!` sees the caller's
/// locals, which is what the print macros rely on.
#[test]
fn richf_through_a_macro_rules_wrapper_captures_locals() {
    macro_rules! wrap {
        ($($arg:tt)*) => { richf!($($arg)*) };
    }
    let x = 7;
    let width = 4;
    assert_eq!(wrap!("[bold]{x}[/]").plain(), "7");
    assert_eq!(wrap!("{x:>5}").plain(), "    7");
    assert_eq!(wrap!("{x:>width$}|{}|{0}", "p").plain(), "   7|p|p");
    assert_eq!(wrap!("{n}{x}", n = 1).plain(), "17");
    assert_eq!(wrap!("{} {x:>w$}", "[a]", w = width).plain(), "[a]    7");
}

/// A value ending in an odd run of three or more backslashes used to escape
/// the template's own next tag, leaving `[/]` closing nothing: a panic.
#[test]
fn richf_values_cannot_escape_the_templates_tags() {
    let v = "a\\\\\\";
    let text = richf!("{}[bold]x[/]", v);
    assert_eq!(text.plain(), "a\\\\\\x");
    assert!(ansi(&text).contains("\x1b[1mx"), "{:?}", ansi(&text));
    // A backslash in the template before a value cannot turn the value's
    // bracket into a tag either, nor can a template `[` open one with it.
    assert_eq!(richf!("\\{}", "[b]x").plain(), "\\[b]x");
    assert_eq!(richf!("[{}", "b]x").plain(), "[b]x");
    // A value's own `\[` is literal text, as written.
    assert_eq!(richf!("{}", "\\[1] \\[b]").plain(), "\\[1] \\[b]");
}

/// Values drawn from the markup-significant alphabet, in every kind of slot,
/// always parse and print exactly as given.
#[test]
fn richf_values_are_always_literal_property() {
    const ALPHABET: &[char] = &['\\', '[', ']', '/', 'a', 'b', '#', '@', ' '];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..4000 {
        let mut value = || -> String {
            let len = next() % 9;
            (0..len)
                .map(|_| ALPHABET[(next() % ALPHABET.len() as u64) as usize])
                .collect()
        };
        let (a, b) = (value(), value());
        let cases = [
            (richf!("{}[bold]x[/]{}", a, b), format!("{a}x{b}")),
            (richf!("[bold]{}{}[/]", a, b), format!("{a}{b}")),
            (richf!("\\\\{}\\[b]{}", a, b), format!("\\\\{a}[b]{b}")),
            (richf!("[i]{}[/i]\\{}[b]y[/b]", a, b), format!("{a}\\{b}y")),
            (richf!("\\[b {}]{}", a, b), format!("[b {a}]{b}")),
        ];
        for (text, want) in cases {
            assert_eq!(text.plain(), want, "a={a:?} b={b:?}");
        }
    }
}
