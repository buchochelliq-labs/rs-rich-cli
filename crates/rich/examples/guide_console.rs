//! Guide: Console and printing — run: cargo run -p rs-rich --example guide_console [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/console.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use guide_support::Shots;

// --8<-- [start:imports]
use rich::measure::Measurement;
use rich::{
    ColorSystem, Console, ConsoleOptions, Justify, Overflow, Panel, Renderable, Rule, Segment,
    Style, Text,
};
// --8<-- [end:imports]

fn main() {
    let shots = Shots::from_args("guide_console");
    builder();
    shots.shot("printing", 60, printing);
    shots.shot("print-with", 60, print_with);
    shots.shot("measure", 60, measure);
    shots.shot("custom", 60, custom);
    capturing();
    if !shots.is_svg() {
        segments_demo(&Console::new());
    }
}

// --8<-- [start:builder]
fn builder() {
    let console = Console::builder()
        .width(72) // ignore the detected width
        .height(20) // ignore the detected height
        .force_terminal(true) // style even when stdout is not a TTY
        .color_system(Some(ColorSystem::EightBit)) // downgrade colours to 256
        .no_color(false) // true: plain output, no colour or other styling
        .highlight(true) // automatic repr highlighting (the default)
        .emoji(true) // expand :shortcodes: (the default)
        .build();

    assert_eq!(console.width(), 72);
    assert_eq!(console.color_system(), Some(ColorSystem::EightBit));
    assert!(console.is_terminal());
}
// --8<-- [end:builder]

// --8<-- [start:printing]
fn printing(console: &Console) {
    // Markup: parsed, emoji-expanded and highlighted.
    console.print_str("[bold magenta]Hello[/], [italic]world[/] :wave:");
    // Automatic highlighting of numbers, strings, paths, URLs…
    console.print_str("Loaded 3 files from /etc/app in 250 ms");

    // Any Renderable goes through `print`.
    console.print(&Text::styled("A styled Text value", "bold green"));
    console.print(&Rule::new("[b]a rule[/]"));

    // A blank line is an empty print.
    console.print_str("");
    console.print_justified("[reverse] centred [/]", Justify::Center);
    console.print_justified("right →", Justify::Right);
}
// --8<-- [end:printing]

// --8<-- [start:print_with]
fn print_with(console: &Console) {
    let long = "Pneumonoultramicroscopicsilicovolcanoconiosis-is-a-very-long-word";

    let mut options = console.options();
    options.max_width = 30; // render into 30 cells
    options.overflow = Some(Overflow::Ellipsis);
    options.no_wrap = Some(true);
    console.print_with(&Text::new(long), &options);

    options.overflow = Some(Overflow::Fold);
    options.no_wrap = None;
    console.print_with(&Text::new(long), &options);
}
// --8<-- [end:print_with]

// --8<-- [start:measure]
fn measure(console: &Console) {
    let options = console.options();
    let text = Text::new("the quick brown fox");
    let m = Measurement::get(console, &options, &text);
    // minimum = the longest word, maximum = the whole line.
    console.print_str(&format!("Text:  min={} max={}", m.minimum, m.maximum));

    // Containers such as Panel fill the width by default.
    let panel = Panel::new(Box::new(Text::new("hi")));
    let m = Measurement::get(console, &options, &panel);
    console.print_str(&format!("Panel: min={} max={}", m.minimum, m.maximum));

    // Clamp a measurement into bounds.
    let m = Measurement::new(5, 19).clamp(Some(8), Some(12));
    console.print_str(&format!("clamped: min={} max={}", m.minimum, m.maximum));
}
// --8<-- [end:measure]

// --8<-- [start:custom]
/// A key/value line that pushes its value to the right edge with dots:
/// `name ........ value`. It adapts to whatever width it is given.
struct Leader {
    key: String,
    value: String,
}

impl Renderable for Leader {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let used = self.key.chars().count() + self.value.chars().count() + 2;
        let dots = options.max_width.saturating_sub(used).max(1);
        vec![
            Segment::new(self.key.clone(), Some(Style::parse("bold").unwrap())),
            Segment::new(" ", None),
            Segment::new(".".repeat(dots), Some(Style::parse("dim").unwrap())),
            Segment::new(" ", None),
            Segment::new(self.value.clone(), Some(Style::parse("cyan").unwrap())),
        ]
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        // At least "key . value"; happy to take the whole width.
        let minimum = self.key.chars().count() + self.value.chars().count() + 3;
        Measurement::new(minimum.min(options.max_width), options.max_width)
    }
}

fn custom(console: &Console) {
    let row = |key: &str, value: &str| Leader {
        key: key.into(),
        value: value.into(),
    };
    console.print(&row("version", "0.0.7"));
    console.print(&row("licence", "MIT"));
    // Custom renderables nest inside the built-in containers.
    console.print(&Panel::new(Box::new(row("inside", "a panel"))).title("Leader"));
}
// --8<-- [end:custom]

// --8<-- [start:capture]
fn capturing() {
    let console = Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .build();

    // Everything printed inside the closure is returned instead of written.
    let ansi = console.capture(|c| c.print_str("[bold]hi[/]"));
    assert_eq!(ansi, "\x1b[1mhi\x1b[0m\n");

    // The same, with styles stripped.
    let plain = console.export_text(|c| c.print_str("[bold]hi[/]"));
    assert_eq!(plain, "hi\n");

    // Render one value without printing it.
    let rule = Rule::new("x");
    let with_newline = console.render_export(&rule); // exactly what print writes
    let without = console.render_to_string(&rule); // no trailing newline
    assert_eq!(with_newline, format!("{without}\n"));

    // The raw segments, for producing several outputs from one render.
    let segments: Vec<Segment> = console.record_output(|c| c.print_str("[red]a[/] b"));
    assert_eq!(segments[0].text, "a");
    assert_eq!(
        console.segments_to_string(&segments),
        "\x1b[31ma\x1b[0m b\n"
    );
}
// --8<-- [end:capture]

// --8<-- [start:segments]
fn segments_demo(console: &Console) {
    let text = Text::from_markup("[bold]Hello[/] world").unwrap();
    for segment in console.record_output(|c| c.print(&text)) {
        println!(
            "{:?} {:?}",
            segment.text,
            segment.style.map(|s| s.definition())
        );
    }
    // "Hello" Some("bold")
    // " world" Some("none")
    // "\n" None
}
// --8<-- [end:segments]
