//! Guide: Text and style — run: cargo run -p rs-rich --example guide_text [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/text-and-style.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use guide_support::Shots;

use rich::highlighter::RegexHighlighter;
use rich::markup::escape;
use rich::{Color, ColorSystem, Console, Justify, Overflow, Panel, Style, Text, Theme};

fn main() {
    let shots = Shots::from_args("guide_text");
    shots.shot("markup", 64, markup);
    shots.shot("colors", 64, colors);
    shots.shot("text", 64, text_api);
    shots.shot("justify", 40, justify);
    shots.shot("overflow", 40, overflow);
    shots.shot("highlight", 64, highlight);

    // Automatic repr highlighting off, so only the ticket highlighter runs.
    let mut pinned = Shots::pinned_builder(64).highlight(false).build();
    install_tickets(&mut pinned);
    let mut terminal = Console::builder().highlight(false).build();
    install_tickets(&mut terminal);
    shots.shot_on("custom-highlighter", pinned, terminal, tickets);

    shots.shot_on(
        "theme",
        Shots::pinned_builder(64).theme(my_theme()).build(),
        themed_console(),
        use_theme_names,
    );

    style_values();
    theme_files();
    if !shots.is_svg() {
        theme_stack(&mut Console::new());
    }
}

// --8<-- [start:markup]
fn markup(console: &Console) {
    console.print_str("[bold]bold[/] [italic]italic[/] [underline]underline[/] [strike]strike[/]");
    console.print_str("[bold red]nested [italic]and inner[/italic] outer[/bold red] plain");
    console.print_str("[b]short[/b] [i]forms[/] [reverse] reverse [/] [dim]dim[/]");
    console.print_str("[link=https://github.com/Textualize/rich]a hyperlink[/link]");
    console.print_str("A literal \\[bold] tag, and :rocket: :sparkles: emoji");

    // Text you did not write: escape it first.
    let user_input = "[red]not markup[/red]";
    console.print_str(&format!("user said: {}", escape(user_input)));
}
// --8<-- [end:markup]

// --8<-- [start:colors]
fn colors(console: &Console) {
    console.print_str("[red]red[/] [bright_green]bright_green[/] [grey62]grey62[/]");
    console.print_str(
        "[#ff8800]#ff8800[/] [rgb(90,160,255)]rgb(90,160,255)[/] [color(201)]color(201)[/]",
    );
    console.print_str("[white on dark_blue] white on dark_blue [/] [black on #ffcc00] on hex [/]");
    console.print_str("[bold not italic]bold not italic[/] [i]italic [not i]not[/] again[/]");
}
// --8<-- [end:colors]

// --8<-- [start:style]
fn style_values() {
    // Parse a definition — the same grammar as a markup tag.
    let warning = Style::parse("bold yellow on #202020").unwrap();

    // Or build from colours.
    let accent = Style::from_color(Some(Color::from_rgb(255, 136, 0)), None);
    let link = Style::new().with_link("https://docs.rs/rs-rich");

    // Combine: the right-hand style wins where both set something.
    let both = warning.combine(&accent);
    assert_eq!(both.definition(), "bold #ff8800 on #202020");
    assert_eq!(link.link(), Some("https://docs.rs/rs-rich"));

    // Colours downgrade to what the terminal supports.
    let orange = Color::parse("#ff8800").unwrap();
    let ansi256 = orange.downgrade(ColorSystem::EightBit);
    assert_eq!(ansi256.ansi_codes(true), vec!["38", "5", "208"]);

    // Bad definitions are errors, not silent no-ops.
    assert!(Style::parse("bold chartreuse-ish").is_err());
}
// --8<-- [end:style]

// --8<-- [start:text]
fn text_api(console: &Console) {
    // Build up a Text piece by piece.
    let mut text = Text::new("Status: ");
    text.append("ok", Some(Style::parse("bold green").unwrap().into()));
    text.append(" (3 warnings)", Some("yellow".into())); // a style name
    console.print(&text);

    // Style a byte range after the fact.
    let mut text = Text::new("Hello, World!");
    text.stylize("bold magenta", 0, 5);
    console.print(&text);

    // Style every match.
    let mut text = Text::new("error: disk full; error: retry failed");
    text.highlight_words(&["error"], "bold red", true).unwrap();
    text.highlight_regex(r"\b\w+ \w+$", Some("underline".into()), "")
        .unwrap();
    console.print(&text);

    // From markup, then keep editing.
    let text = Text::from_markup("[b]parsed[/b] markup")
        .unwrap()
        .append_text(&Text::styled(" + appended", "italic cyan"));
    console.print(&text);
}
// --8<-- [end:text]

// --8<-- [start:justify]
fn justify(console: &Console) {
    let words = "Justification decides where the spare cells on each line go.";
    for justify in [
        Justify::Left,
        Justify::Center,
        Justify::Right,
        Justify::Full,
    ] {
        // A Text's own justify applies when it is inside another renderable.
        let text = Text::new(words).justify(justify);
        console.print(&Panel::new(Box::new(text)).title(format!("{justify:?}")));
    }
}
// --8<-- [end:justify]

// --8<-- [start:overflow]
fn overflow(console: &Console) {
    let word = "supercalifragilisticexpialidocious-and-then-some";
    for overflow in [Overflow::Fold, Overflow::Crop, Overflow::Ellipsis] {
        let text = Text::new(word)
            .overflow(overflow)
            .no_wrap(overflow != Overflow::Fold);
        console.print(&Panel::new(Box::new(text)).title(format!("{overflow:?}")));
    }
}
// --8<-- [end:overflow]

// --8<-- [start:highlight]
fn highlight(console: &Console) {
    // No markup: the ReprHighlighter finds these on its own.
    console.print_str("int 42, float 3.14, hex 0xff, str 'quoted', bool True / None");
    console.print_str("path /usr/local/bin/rich, url https://example.com/a?b=1");
    console.print_str("uuid 123e4567-e89b-12d3-a456-426614174000, ip 192.168.0.1");
    console.print_str("call Point(x=1, y=2) → {'k': [1, 2]}");
}
// --8<-- [end:highlight]

// --8<-- [start:custom_highlighter]
/// Colour ticket ids like `ENG-1234` wherever they appear.
fn install_tickets(console: &mut Console) {
    // Each named group becomes a span styled `ticket.<group>`…
    let tickets = RegexHighlighter::new("ticket.", &[r"\b(?P<id>[A-Z]{2,5}-\d+)\b"]);
    console.add_highlighter(Box::new(tickets));

    // …which a theme maps to a real style.
    let theme = Theme::from_styles([("ticket.id", "bold black on bright_yellow")], true).unwrap();
    console.push_theme(theme, true);
}

fn tickets(console: &Console) {
    console.print_str("Fixed in ENG-1234; see also OPS-77 and 1234.");
}
// --8<-- [end:custom_highlighter]

// --8<-- [start:theme]
fn my_theme() -> Theme {
    let mut theme = Theme::default_theme();
    // Override a built-in name…
    theme.insert("repr.number", Style::parse("bold magenta").unwrap());
    // …and add your own.
    theme.insert("danger", Style::parse("bold white on red").unwrap());
    theme.insert("muted", Style::parse("dim italic").unwrap());
    theme
}

fn themed_console() -> Console {
    Console::builder().theme(my_theme()).build()
}

fn use_theme_names(console: &Console) {
    console.print_str("[danger] DANGER [/] [muted]quietly noted[/] answer = 42");
    // Style names work anywhere a style is accepted.
    console.print(&Text::styled("a muted Text", "muted"));
}
// --8<-- [end:theme]

// --8<-- [start:theme_stack]
fn theme_stack(console: &mut Console) {
    let alert = Theme::from_styles([("danger", "bold yellow on red")], false).unwrap();
    {
        // Pushed until the guard drops; it inherits the current styles.
        let themed = console.use_theme(alert);
        themed.print_str("[danger] ALERT [/] answer = 42");
    }
    // Back to the previous theme, where `danger` is not defined.
    console.print_str("[danger]no longer styled[/]");

    // The manual form.
    console.push_theme(Theme::from_styles([("x", "red")], true).unwrap(), true);
    console.pop_theme().unwrap();
    assert!(console.pop_theme().is_err()); // the base theme cannot be popped
}
// --8<-- [end:theme_stack]

// --8<-- [start:theme_file]
fn theme_files() {
    // Python rich's theme file format ([styles] section, one name per line).
    let ini = "[styles]\nwarning = bold yellow\nrepr.number = cyan\n";
    let theme = Theme::from_file(ini, true).unwrap(); // true: keep the defaults
    assert!(theme.get("warning").is_some());
    assert!(theme.get("rule.line").is_some()); // inherited

    // Theme::read(path, inherit) loads the same format from disk, and
    // theme.config() writes one back out.
    let round_trip = Theme::from_file(&theme.config(), false).unwrap();
    assert_eq!(round_trip.get("warning"), theme.get("warning"));
}
// --8<-- [end:theme_file]
