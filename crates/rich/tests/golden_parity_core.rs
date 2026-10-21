//! Parity for the rendering core (0.0.22 workstream 3), against
//! `golden/parity_core.tsv` (captured by `scripts/golden_parity_core.py` from
//! real rich 15.0.0). Each fixture line is `name<TAB>json(expected)`; every
//! name has a Rust builder below. Where upstream prints a Python `repr`, the
//! builder formats the same values the same way.

use std::sync::Arc;

use rich::ansi::AnsiDecoder;
use rich::color::{blend_rgb, parse_rgb_hex, ColorType, Palette};
use rich::containers::{Lines, Renderables};
use rich::errors::RichError;
use rich::markdown::Markdown;
use rich::markup::{self, Tag};
use rich::measure::{measure_renderables, Measurement};
use rich::protocol::Highlighter;
use rich::style::{MetaValue, StyleStack, StyleType};
use rich::terminal_theme::{
    TerminalTheme, DEFAULT_TERMINAL_THEME, DIMMED_MONOKAI, MONOKAI, NIGHT_OWLISH, SVG_EXPORT_THEME,
};
use rich::{
    Color, ColorSystem, ColorTriplet, Columns, Console, ISO8601Highlighter, JSONHighlighter,
    Justify, NullHighlighter, Overflow, Panel, Renderable, ReprHighlighter, Rule, Segment, Style,
    Table, Text,
};

/// `_console`: truecolor, no highlighting.
fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .highlight(false)
        .no_color(false)
        .legacy_windows(false)
        .build()
}

fn print(console: &Console, renderable: &dyn Renderable) -> String {
    console.capture(|c| c.print(renderable))
}

fn lines(values: &[String]) -> String {
    values.iter().map(|value| format!("{value}\n")).collect()
}

fn pair(m: Measurement) -> String {
    format!("({}, {})", m.minimum, m.maximum)
}

fn py_bool(value: bool) -> &'static str {
    if value {
        "True"
    } else {
        "False"
    }
}

fn py_opt_bool(value: Option<bool>) -> &'static str {
    value.map_or("None", py_bool)
}

/// Python's `repr()` of a string, enough for these cases.
fn py_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", ch as u32))
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

fn triplet_repr(t: ColorTriplet) -> String {
    format!(
        "ColorTriplet(red={}, green={}, blue={})",
        t.red, t.green, t.blue
    )
}

fn color_type_name(kind: ColorType) -> &'static str {
    match kind {
        ColorType::Default => "DEFAULT",
        ColorType::Standard => "STANDARD",
        ColorType::EightBit => "EIGHT_BIT",
        ColorType::Truecolor => "TRUECOLOR",
        ColorType::Windows => "WINDOWS",
    }
}

fn float_repr(value: f64) -> String {
    let text = format!("{value:?}");
    text
}

// --- measure.py ---------------------------------------------------------------

fn measure_api() -> String {
    let m = Measurement::new(3, 7);
    lines(&[
        m.span().to_string(),
        pair(m.with_minimum(5)),
        pair(m.with_minimum(9)),
        pair(m.with_maximum(4)),
        pair(m.clamp(Some(4), Some(6))),
        pair(m.clamp(None, Some(2))),
        pair(m.clamp(Some(8), None)),
        pair(Measurement::new(9, 4).normalize()),
    ])
}

fn measure_default() -> String {
    let console = console(20);
    let options = console.options();
    let markdown = Markdown::new("hello world");
    let columns = Columns::new(vec!["aa".into(), "bbb".into()]);
    let first: String = "abc defgh".into();
    let second = Text::new("x".repeat(12));
    let third = Text::new("abcdef ghi");
    let fourth: String = "[b]x[/b] yy".into();
    lines(&[
        pair(Measurement::get(&console, &options, &markdown)),
        pair(Measurement::get(&console, &options, &columns)),
        pair(measure_renderables(&console, &options, &[])),
        pair(measure_renderables(&console, &options, &[&first, &second])),
        pair(measure_renderables(&console, &options, &[&third, &fourth])),
    ])
}

fn measure_default_in_table() -> String {
    let mut table = Table::new();
    table.add_column("h");
    table.add_column("k");
    table.add_row_cells(vec![
        rich::table::Cell::Renderable(Arc::new(Columns::new(vec!["aa".into(), "bbb".into()]))),
        "x".into(),
    ]);
    table.add_row_cells(vec![
        rich::table::Cell::Renderable(Arc::new(Markdown::new("hello"))),
        "y".into(),
    ]);
    print(&console(30), &table)
}

// --- terminal_theme.py --------------------------------------------------------

fn terminal_themes() -> String {
    let mut out = Vec::new();
    for (name, theme) in [
        ("default", &DEFAULT_TERMINAL_THEME),
        ("monokai", &MONOKAI),
        ("dimmed_monokai", &DIMMED_MONOKAI),
        ("night_owlish", &NIGHT_OWLISH),
        ("svg", &SVG_EXPORT_THEME),
    ] {
        let palette = theme.ansi_colors();
        let colors: Vec<String> = (0..16).map(|index| palette[index].hex()).collect();
        out.push(format!(
            "{name} {} {} {}",
            theme.background_color().hex(),
            theme.foreground_color().hex(),
            colors.join(" ")
        ));
    }
    let normal: Vec<(u8, u8, u8)> = (0..8).map(|n| (n, n, n)).collect();
    let custom = TerminalTheme::new((1, 2, 3), (4, 5, 6), &normal, None);
    let palette = custom.ansi_colors();
    let colors: Vec<String> = (0..16).map(|index| palette[index].hex()).collect();
    out.push(colors.join(" "));
    lines(&out)
}

// --- containers.py ------------------------------------------------------------

fn lines_justify() -> String {
    let console = console(20);
    let mut out = Vec::new();
    for (justify, justify_name) in [
        (Justify::Left, "left"),
        (Justify::Center, "center"),
        (Justify::Right, "right"),
        (Justify::Full, "full"),
    ] {
        for (overflow, overflow_name) in [
            (Overflow::Fold, "fold"),
            (Overflow::Crop, "crop"),
            (Overflow::Ellipsis, "ellipsis"),
        ] {
            let mut texts = Lines::new(vec![
                Text::styled("hello world  ", Style::parse("red").unwrap()),
                Text::assemble(
                    [
                        rich::text::TextPart::Styled("a".into(), "bold".into()),
                        " ".into(),
                        rich::text::TextPart::Styled("b".into(), "bold".into()),
                        " c".into(),
                    ],
                    "",
                ),
                Text::new("supercalifragilistic"),
                Text::new("last line here"),
            ]);
            texts.justify(&console, 12, justify, overflow);
            for line in &texts {
                let printed = print(&console, line);
                out.push(format!(
                    "{justify_name}/{overflow_name}|{}|",
                    printed.trim_end_matches('\n')
                ));
            }
        }
    }
    lines(&out)
}

fn lines_ops() -> String {
    let mut texts = Lines::new(vec![Text::new("a"), Text::new("b")]);
    texts.append(Text::new("c"));
    texts.extend([Text::new("d"), Text::new("e")]);
    let popped = texts.pop(None).unwrap();
    texts[0] = Text::new("z");
    let first = texts.pop(Some(0)).unwrap();
    let rest: Vec<&str> = texts.lines().iter().map(Text::plain).collect();
    lines(&[
        texts.len().to_string(),
        popped.plain().to_string(),
        first.plain().to_string(),
        rest.join(" "),
    ])
}

fn lines_render() -> String {
    let texts = Lines::new(vec![
        Text::styled("one", Style::parse("bold").unwrap()),
        Text::new("two"),
    ]);
    print(&console(10), &texts)
}

fn renderables_render() -> String {
    let console = console(10);
    let mut renderables = Renderables::new(vec![
        Arc::new(String::from("[b]a[/b]")),
        Arc::new(Text::new("bb")),
    ]);
    renderables.append(Arc::new(Rule::new("")));
    let mut out = print(&console, &renderables);
    let inner = Renderables::new(vec![
        Arc::new(String::from("x")),
        Arc::new(String::from("yyy")),
    ]);
    out += &print(&console, &Panel::new(Box::new(inner)).expand(false));
    out += &lines(&[pair(Measurement::get(
        &console,
        &console.options(),
        &Renderables::default(),
    ))]);
    out
}

// --- ansi.py --------------------------------------------------------------------

fn ansi_decode() -> String {
    let console = console(30);
    let mut out = String::new();
    for source in [
        "a\x1b7b\x1b8c",
        "x\x1b=y\x1b>z",
        "\x1b[99999999999999999999mbig\x1b[0m",
        "\x1b[38;5;99999mq\x1b[0m",
        "\x1b[1;;3mbi\x1b[0m",
        "\x1b(Bok\x1bMup",
        "\x1b[2Jcl\x1b[?25lear",
    ] {
        let mut decoder = AnsiDecoder::new();
        for text in decoder.decode_split_newlines(source) {
            out += &print(&console, &text);
        }
    }
    out
}

// --- highlighter.py -------------------------------------------------------------

fn highlighters() -> String {
    let console = console(60);
    let mut out = String::new();
    for source in [
        r#"{"key": "value", "n": -1.5e3, "t": true, "f": false, "z": null}"#,
        "{\"a\" :\n  \"b\", \"c\"\t: [1, 2]}",
        r#""lonely" "string":x"#,
    ] {
        out += &print(
            &console,
            &JSONHighlighter::new().highlighted(&Text::new(source)),
        );
    }
    out += &print(
        &console,
        &NullHighlighter::new().highlighted(&Text::new("plain 123 True")),
    );
    let text = Text::styled("2024-01-02", Style::parse("italic").unwrap());
    let highlighted = ISO8601Highlighter::new().highlighted(&text);
    out += &print(&console, &text);
    out += &print(&console, &highlighted);
    out += &print(
        &console,
        &ReprHighlighter::new().highlighted(&Text::new("x=1 [None]")),
    );
    out
}

// --- color.py, color_triplet.py, palette.py -----------------------------------------

fn color_api() -> String {
    let mut out = Vec::new();
    for name in [
        "red",
        "bright_red",
        "color(9)",
        "color(100)",
        "#ff0080",
        "rgb(1,2,3)",
        "default",
        "grey50",
    ] {
        let color = Color::parse(name).unwrap();
        let system = match color.system() {
            ColorSystem::Standard => 1,
            ColorSystem::EightBit => 2,
            ColorSystem::Truecolor => 3,
            ColorSystem::Windows => 4,
        };
        out.push(format!(
            "{name}: system={system} sysdef={} default={} true={} bg={} mono={}",
            py_bool(color.is_system_defined()),
            py_bool(color.is_default()),
            color.get_truecolor_for(None, true).hex(),
            color.get_truecolor_for(None, false).hex(),
            color.get_truecolor_for(Some(&MONOKAI), true).hex(),
        ));
    }
    let triplet = ColorTriplet::new(10, 20, 255);
    let (r, g, b) = triplet.normalized();
    out.push(format!(
        "{} {} ({}, {}, {})",
        triplet.hex(),
        triplet.rgb(),
        float_repr(r),
        float_repr(g),
        float_repr(b)
    ));
    for color in [Color::from_triplet(triplet), Color::from_rgb(1, 2, 255)] {
        out.push(format!(
            "Color({}, ColorType.{}, triplet={})",
            py_repr(&color.name),
            color_type_name(color.kind),
            triplet_repr(color.triplet.unwrap())
        ));
    }
    out.push(triplet_repr(parse_rgb_hex("a0b1c2")));
    out.push(triplet_repr(blend_rgb(
        ColorTriplet::new(0, 0, 0),
        ColorTriplet::new(255, 100, 51),
        0.3,
    )));
    out.push(triplet_repr(blend_rgb(
        ColorTriplet::new(0, 0, 0),
        ColorTriplet::new(255, 100, 51),
        0.5,
    )));
    let palette = DEFAULT_TERMINAL_THEME.ansi_colors();
    for (red, green, blue) in [(250, 0, 0), (128, 128, 128), (0, 0, 1)] {
        out.push(
            palette
                .match_color(ColorTriplet::new(red, green, blue))
                .to_string(),
        );
    }
    lines(&out)
}

fn color_parse_errors() -> String {
    let mut out = Vec::new();
    for name in [
        "color(256)",
        "color( 5)",
        "color(0005)",
        "rgb(1,2)",
        "rgb(1,2,300)",
        "rgb( 1 , 2 , 3 )",
        "#ABCDEF",
        "  Red  ",
        "#abc",
        "nope",
        "rgb(1,2,3,4)",
    ] {
        out.push(match Color::parse(name) {
            Ok(color) => format!(
                "{}: ok {} {} {} {}",
                py_repr(name),
                color.name,
                color_type_name(color.kind),
                color.number.map_or("None".to_string(), |n| n.to_string()),
                color.triplet.map_or("None".to_string(), triplet_repr),
            ),
            Err(error) => format!("{}: {}", py_repr(name), error.message()),
        });
    }
    lines(&out)
}

fn color_rich() -> String {
    let console = console(40);
    let mut out = String::new();
    for name in ["red", "color(100)", "#ff0080", "default"] {
        out += &print(&console, &Color::parse(name).unwrap());
    }
    out
}

fn palette_rich() -> String {
    let palette = Palette::new(vec![
        ColorTriplet::new(255, 0, 0),
        ColorTriplet::new(0, 128, 255),
        ColorTriplet::new(12, 34, 56),
    ]);
    print(&console(50), &palette)
}

// --- style.py -------------------------------------------------------------------

fn style_api() -> String {
    let style = Style::parse("bold not italic red on blue link https://x").unwrap();
    let parse = |definition: &str| Style::parse(definition).unwrap();
    let mut out = vec![
        format!(
            "{} {} {} {}",
            py_opt_bool(style.bold()),
            py_opt_bool(style.italic()),
            py_opt_bool(style.underline()),
            py_opt_bool(style.dim())
        ),
        style.definition(),
        style.without_color().definition(),
        style.background_style().definition(),
        format!(
            "{} {} {}",
            py_bool(style.transparent_background()),
            py_bool(parse("on default").transparent_background()),
            py_bool(Style::new().transparent_background())
        ),
        Style::null().definition(),
        py_bool(!Style::null().is_null()).to_string(),
        Style::combine_all(&[parse("bold"), parse("red"), parse("not bold")])
            .unwrap()
            .definition(),
        Style::combine_all(&[parse("italic"), parse("underline")])
            .unwrap()
            .definition(),
        Style::pick_first(&[None, Some("bold"), Some("red")])
            .unwrap()
            .to_string(),
        style.copy().definition(),
        parse("bold red link https://x")
            .clear_meta_and_links()
            .definition(),
        parse("blink blink2 conceal strike underline2 frame encircle overline reverse dim")
            .definition(),
    ];
    let meta: rich::style::Meta = [("a", MetaValue::Int(1))].into_iter().collect();
    out.push(py_bool(Style::from_meta(meta).without_color() == Style::new()).to_string());
    out.push(
        py_bool(
            parse("bold").combine(&Style::new().with_meta(rich::style::Meta::new()))
                == parse("bold"),
        )
        .to_string(),
    );
    let mut stack = StyleStack::new(parse("red"));
    stack.push(&parse("bold"));
    stack.push(&parse("blue"));
    out.push(stack.current().definition());
    out.push(stack.pop().unwrap().definition());
    lines(&out)
}

fn style_parse_errors() -> String {
    let mut out = Vec::new();
    for definition in [
        "on",
        "bold on nope",
        "not",
        "not boldx",
        "not BOLD",
        "link",
        "nope",
        "bold color(300)",
        "red on rgb(1,2)",
    ] {
        out.push(match Style::parse(definition) {
            Ok(style) => format!("{}: ok {}", py_repr(definition), style.definition()),
            Err(error) => format!("{}: {}", py_repr(definition), error.message()),
        });
    }
    lines(&out)
}

fn style_render() -> String {
    let system = Some(ColorSystem::Truecolor);
    lines(&[
        py_repr(&Style::parse("bold red").unwrap().render("hi", system)),
        py_repr(&Style::parse("bold #ff8800").unwrap().render("hi", None)),
        py_repr(&Style::parse("red").unwrap().render("", system)),
        py_repr(&Style::new().render("plain", system)),
    ])
}

// --- markup.py ------------------------------------------------------------------

fn markup_meta() -> String {
    let mut out = Vec::new();
    for source in [
        "[@click]a[/]",
        "[@click=foo]a[/]",
        "[@click=foo(1, 'x')]a[/]",
        "[@click='hi']a[/]",
        "[@click=5]a[/]",
        "[@click=[1, 2.5, None, True]]a[/]",
        "[@click=(1,)]a[/]",
        "[@click=foo(]a[/]",
        "[bold][@enter=go(1)]x[/][/bold]",
        "[@click=-3]a[/]",
    ] {
        out.push(match markup::render(source) {
            Ok(text) => {
                let metas: Vec<String> = text
                    .spans()
                    .iter()
                    .map(|span| match &span.style {
                        StyleType::Style(style) => style.meta().repr(),
                        StyleType::Name(name) => py_repr(name),
                    })
                    .collect();
                format!("{source} -> [{}] | {}", metas.join(", "), text.markup())
            }
            Err(RichError::Markup(message)) => format!("{source} -> error: {message}"),
            Err(other) => panic!("unexpected {other:?}"),
        });
    }
    lines(&out)
}

fn markup_errors() -> String {
    let mut out = Vec::new();
    for source in ["[/  bold ]", "[/bold=x]", "a[/]"] {
        out.push(match markup::render(source) {
            Ok(_) => "ok".to_string(),
            Err(error) => error.message().to_string(),
        });
    }
    lines(&out)
}

fn markup_tag_and_style() -> String {
    let bold = Tag::new("bold", None);
    let link = Tag::new("link", Some("https://x".to_string()));
    let mut out = lines(&[
        bold.to_string(),
        link.to_string(),
        bold.markup(),
        link.markup(),
    ]);
    let text = markup::render_with("a [i]b[/i] c", "red", true, None).unwrap();
    out += &print(&console(20), &text);
    out
}

// --- text.py --------------------------------------------------------------------

fn text_lines<'a>(console: &Console, lines: impl IntoIterator<Item = &'a Text>) -> String {
    lines
        .into_iter()
        .map(|line| format!("|{}|\n", print(console, line).trim_end_matches('\n')))
        .collect()
}

fn named(name: &str) -> StyleType {
    StyleType::Name(name.to_string())
}

fn text_api() -> String {
    let console = console(30);
    let mut text = Text::styled("Hello, World!", "bold");
    text.stylize("red", 0, 5);
    text.stylize("underline", 3, 9);
    let mut out = print(&console, &text.slice(7, 12));
    out += &print(&console, &text.slice(3, 9));
    let mut spanned = Text::new("ab");
    spanned.push_span(rich::text::Span::new(0, 1, "red"));
    out += &lines(&[
        py_bool(text.contains("World")).to_string(),
        py_bool(text.contains(Text::new("World").plain())).to_string(),
        py_bool(text.contains("xyz")).to_string(),
        py_bool(Text::new("ab") == Text::new("ab")).to_string(),
        py_bool(spanned == Text::new("ab")).to_string(),
        py_bool(Text::styled("ab", "red") == Text::new("ab")).to_string(),
    ]);
    out += &print(&console, &(&text + " more"));
    out += &print(&console, &(&text + &Text::styled("x", "blue")));
    for offset in [0isize, 3, 6, -1, -4] {
        out += &format!(
            "{offset}: {}\n",
            text.get_style_at_offset(&console, offset).definition()
        );
    }
    out
}

fn text_edit() -> String {
    use rich::align::HorizontalAlign;
    use rich::text::TextPart;
    let console = console(30);
    let assembled = Text::assemble(
        [
            TextPart::from("a"),
            TextPart::Styled("b".into(), named("red")),
            TextPart::Text(Text::styled("c", "blue")),
        ],
        "italic",
    );
    let mut out = print(&console, &assembled);
    let mut text = Text::styled("file.py", "green");
    text.stylize("bold", 4, 7);
    text.remove_suffix(".py");
    text.remove_suffix(".rs");
    out += &print(&console, &text);
    for length in [3, 10] {
        let mut text = Text::styled("abcdef", "on blue");
        text.stylize("red", 2, 6);
        text.set_length(length);
        out += &text_lines(&console, [&text]);
    }
    let mut text = Text::new("ab");
    text.stylize("red", 0, 2);
    text.stylize("blue", 0, 1);
    text.extend_style(3);
    text.extend_style(0);
    out += &text_lines(&console, [&text]);
    let mut text = Text::new("plain");
    text.extend_style(2);
    out += &text_lines(&console, [&text]);
    for align in [
        HorizontalAlign::Left,
        HorizontalAlign::Center,
        HorizontalAlign::Right,
    ] {
        for width in [3, 9] {
            let mut text = Text::styled("hello", "red");
            text.align(align, width, '.');
            out += &text_lines(&console, [&text]);
        }
    }
    let mut text = Text::new("x");
    text.append_tokens([
        ("a".to_string(), Some("red")),
        ("b".to_string(), None),
        ("c\x07".to_string(), Some("bold")),
        (String::new(), Some("blue")),
    ]);
    out += &print(&console, &text);
    let mut target = Text::new("abcdef");
    let mut source = Text::new("zz");
    source.stylize("red", 0, 2);
    source.stylize("bold", 1, 4);
    target.copy_styles(&source);
    out += &print(&console, &target);
    out
}

const WRAP_SOURCE: &str = "The quick brown fox jumps over\tthe lazy dog supercalifragilistic";

fn text_wrap() -> String {
    let console = console(40);
    let mut out = String::new();
    let mut text = Text::new(WRAP_SOURCE);
    text.stylize("red", 4, 9);
    text.stylize("bold", 10, 19);
    for (justify, justify_name) in [
        (Justify::Left, "left"),
        (Justify::Center, "center"),
        (Justify::Right, "right"),
        (Justify::Full, "full"),
        (Justify::Default, "default"),
    ] {
        for (overflow, overflow_name) in [
            (Overflow::Fold, "fold"),
            (Overflow::Crop, "crop"),
            (Overflow::Ellipsis, "ellipsis"),
        ] {
            out += &format!("{justify_name}/{overflow_name}\n");
            let wrapped = text.wrap(&console, 12, Some(justify), Some(overflow), 4, None);
            out += &text_lines(&console, &wrapped);
        }
    }
    out += "no_wrap\n";
    out += &text_lines(
        &console,
        &text.wrap(&console, 12, None, None, 8, Some(true)),
    );
    out += "ignore\n";
    out += &text_lines(
        &console,
        &text.wrap(&console, 12, None, Some(Overflow::Ignore), 8, None),
    );
    let own = Text::new("one two three four")
        .justify(Justify::Right)
        .overflow(Overflow::Ellipsis)
        .no_wrap(false);
    out += "own\n";
    out += &text_lines(&console, &own.wrap(&console, 7, None, None, 8, None));
    out
}

fn text_fit() -> String {
    let console = console(20);
    let mut text = Text::new("abc\ndefgh\n\nij\n");
    text.stylize("red", 1, 6);
    text_lines(&console, &text.fit(4))
}

fn text_from_markup() -> String {
    let console = console(30);
    let mut out = print(
        &console,
        &Text::from_markup(":smile: [b]x[/b] :thumbs_up:").unwrap(),
    );
    out += &print(&console, &Text::from_markup("no tags :smile:").unwrap());
    out += &print(
        &console,
        &Text::from_markup_with("[b]:smile:[/b]", "", false, None).unwrap(),
    );
    out += &print(
        &console,
        &Text::from_markup_with("a [i]b[/i]", "red", true, None).unwrap(),
    );
    out
}

fn span_repr(span: &rich::text::Span) -> String {
    let style = match &span.style {
        StyleType::Name(name) => py_repr(name),
        StyleType::Style(style) => style.definition(),
    };
    format!("Span({}, {}, {style})", span.start, span.end)
}

fn span_api() -> String {
    let span = rich::text::Span::new(2, 5, "red");
    let split = |offset: usize| {
        let (first, second) = span.split(offset);
        format!(
            "({}, {})",
            span_repr(&first),
            second.map_or("None".to_string(), |second| span_repr(&second))
        )
    };
    lines(&[
        split(3),
        split(1),
        split(5),
        span_repr(&span.move_by(2)),
        span_repr(&span.right_crop(4)),
        span_repr(&span.right_crop(9)),
        span_repr(&span.extend(2)),
        span_repr(&span.extend(0)),
    ])
}

// --- segment.py -----------------------------------------------------------------

fn seg(segments: &[Segment]) -> String {
    let items: Vec<String> = segments
        .iter()
        .map(|segment| {
            format!(
                "{}{}:{}",
                py_repr(&segment.text),
                if segment.control { "C" } else { "" },
                segment
                    .style
                    .as_ref()
                    .map_or("None".to_string(), Style::definition)
            )
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn segment_split_cells() -> String {
    let mut out = Vec::new();
    for text in ["abcdef", "日本語テキスト", "a日b本c", "😀x😀"] {
        let segment = Segment::new(text, Some(Style::parse("bold").unwrap()));
        for cut in 0..segment.cell_length() + 2 {
            let (before, after) = segment.split_cells(cut);
            out.push(format!(
                "{text} {cut}: {} {}",
                seg(&[before]),
                seg(&[after])
            ));
        }
    }
    lines(&out)
}

fn segment_divide() -> String {
    let red = Style::parse("red").unwrap();
    let segments = vec![
        Segment::new("abc", None),
        Segment::new("日本", Some(red)),
        Segment::new("xyz", None),
    ];
    let mut out = Vec::new();
    let cases: [&[usize]; 8] = [
        &[],
        &[0],
        &[0, 0, 2],
        &[1, 4, 6],
        &[3, 5, 9],
        &[2, 20],
        &[4],
        &[8, 8],
    ];
    for cuts in cases {
        let pieces: Vec<String> = Segment::divide(&segments, cuts)
            .iter()
            .map(|piece| seg(piece))
            .collect();
        let cuts: Vec<String> = cuts.iter().map(usize::to_string).collect();
        out.push(format!("[{}]: {}", cuts.join(", "), pieces.join(" ")));
    }
    lines(&out)
}

fn segment_lines() -> String {
    let red = Style::parse("red").unwrap();
    let segments = vec![
        Segment::new("ab\ncdef", None),
        Segment::new("gh\n", Some(red.clone())),
        Segment::new("\n", None),
        Segment::new("tail", None),
    ];
    let mut out = Vec::new();
    for (line, terminated) in Segment::split_lines_terminator(&segments) {
        out.push(format!("{} {}", seg(&line), py_bool(terminated)));
    }
    let split: Vec<String> = Segment::split_lines(&segments)
        .iter()
        .map(|line| py_repr(&seg(line)))
        .collect();
    out.push(format!("[{}]", split.join(", ")));
    for (length, pad, include) in [(3, true, true), (5, false, true), (5, true, false)] {
        let cropped: Vec<String> =
            Segment::split_and_crop_lines(&segments, length, Some(&red), pad, include)
                .iter()
                .map(|line| seg(line))
                .collect();
        out.push(cropped.join(" "));
    }
    out.push(seg(&Segment::adjust_line_length_with(
        &[Segment::new("ab", None)],
        4,
        None,
        false,
    )));
    lines(&out)
}

fn segment_shape() -> String {
    let blue = Style::parse("on blue").unwrap();
    let shape_lines = vec![
        vec![Segment::new("ab", None)],
        vec![Segment::new("abcd", None), Segment::control("\x1b[1m")],
        vec![Segment::new("a", None)],
    ];
    let join = |lines: Vec<Vec<Segment>>| -> String {
        lines
            .iter()
            .map(|line| seg(line))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let shape = |(width, height): (usize, usize)| format!("({width}, {height})");
    let mut out = vec![
        Segment::get_line_length(&shape_lines[1]).to_string(),
        shape(Segment::get_shape(&shape_lines)),
        shape(Segment::get_shape(&[])),
    ];
    for (height, new_lines) in [
        (None, false),
        (Some(5), false),
        (Some(2), false),
        (Some(4), true),
    ] {
        out.push(join(Segment::set_shape_with(
            &shape_lines,
            3,
            height,
            Some(&blue),
            new_lines,
        )));
    }
    type Align = fn(&[Vec<Segment>], usize, usize, Option<&Style>, bool) -> Vec<Vec<Segment>>;
    let aligns: [Align; 3] = [
        Segment::align_top,
        Segment::align_middle,
        Segment::align_bottom,
    ];
    for align in aligns {
        for (height, new_lines) in [(6, false), (4, true), (3, false), (2, false)] {
            out.push(join(align(&shape_lines, 4, height, Some(&blue), new_lines)));
        }
    }
    lines(&out)
}

fn segment_misc() -> String {
    let parse = |definition: &str| Style::parse(definition).unwrap();
    let segments = vec![
        Segment::new("a", Some(parse("red link https://x"))),
        Segment::control("\x07"),
        Segment::new("b", None),
        Segment::new("c", Some(Style::new())),
        Segment::new("d", Some(parse("bold"))),
    ];
    let italic = parse("italic");
    lines(&[
        seg(&Segment::strip_links(&segments)),
        seg(&Segment::filter_control(&segments, false)),
        seg(&Segment::filter_control(&segments, true)),
        seg(&Segment::strip_styles(&segments)),
        seg(&Segment::apply_style_with(&segments, Some(&italic), None)),
        seg(&Segment::apply_style_with(
            &segments,
            None,
            Some(&parse("blue")),
        )),
        seg(&Segment::apply_style_with(
            &segments,
            Some(&italic),
            Some(&parse("underline")),
        )),
        seg(&Segment::apply_style_with(&segments, None, None)),
        py_bool(Segment::new("abc", None).is_control()).to_string(),
        py_bool(Segment::control("abc").is_control()).to_string(),
    ])
}

fn segments_render() -> String {
    use rich::segment::{SegmentLines, Segments};
    let console = console(12);
    let red = Style::parse("red").unwrap();
    let pair = || {
        vec![
            Segment::new("one", Some(red.clone())),
            Segment::new("two", None),
        ]
    };
    let mut out = print(&console, &Panel::new(Box::new(Segments::new(pair(), true))));
    out += &print(
        &console,
        &Panel::new(Box::new(Segments::new(pair(), false))),
    );
    let seg_lines = vec![
        vec![
            Segment::new("a", Some(red.clone())),
            Segment::new("b", None),
        ],
        vec![Segment::new("cd", None)],
    ];
    out += &print(
        &console,
        &Panel::new(Box::new(SegmentLines::new(seg_lines.clone(), true))),
    );
    out += &print(
        &console,
        &Panel::new(Box::new(SegmentLines::new(seg_lines, false))),
    );
    out
}

// --- console.py -----------------------------------------------------------------

fn console_print() -> String {
    use rich::console::{PrintObject as O, PrintOptions};
    let console = console(20);
    let print_objects = |objects: &[O<'_>], options: PrintOptions<'_>| -> String {
        console.capture(|c| c.print_objects(objects, &options).unwrap())
    };
    let panel = Panel::new(Box::new(String::from("p"))).expand(false);
    let mut out = String::new();
    out += &print_objects(
        &[O::Str("a"), O::Str("b")],
        PrintOptions {
            sep: "-",
            end: "!\n",
            ..Default::default()
        },
    );
    out += &print_objects(
        &[
            O::Str("[b]x[/b]"),
            O::Text(&Text::styled("y", "red")),
            O::Renderable(&panel),
            O::Str("z"),
            O::Str("1"),
        ],
        PrintOptions::default(),
    );
    out += &format!(
        "<{}>\n",
        print_objects(
            &[],
            PrintOptions {
                end: "",
                ..Default::default()
            }
        )
    );
    out += &format!("<{}>\n", print_objects(&[], PrintOptions::default()));
    out += &print_objects(
        &[O::Str("the quick brown fox jumps")],
        PrintOptions {
            width: Some(10),
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("hello")],
        PrintOptions {
            style: Some("red on blue".into()),
            ..Default::default()
        },
    );
    let xs = "x".repeat(30);
    out += &print_objects(&[O::Str(&xs)], PrintOptions::default());
    let ys = "y".repeat(30);
    out += &print_objects(
        &[O::Str(&ys)],
        PrintOptions {
            crop: false,
            no_wrap: Some(true),
            overflow: Some(Overflow::Ignore),
            ..Default::default()
        },
    );
    let soft = "soft ".repeat(6);
    out += &print_objects(
        &[O::Str(&soft)],
        PrintOptions {
            soft_wrap: Some(true),
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("one\ntwo")],
        PrintOptions {
            new_line_start: true,
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("one")],
        PrintOptions {
            new_line_start: true,
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("mid"), O::Text(&Text::new("text"))],
        PrintOptions {
            justify: Some(Justify::Center),
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Renderable(&panel)],
        PrintOptions {
            justify: Some(Justify::Right),
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("[b]no[/b] :smile:")],
        PrintOptions {
            markup: Some(false),
            emoji: Some(false),
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("1 2 None 'str'")],
        PrintOptions {
            highlight: Some(true),
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("a")],
        PrintOptions {
            end: "",
            ..Default::default()
        },
    );
    out += &print_objects(
        &[O::Str("b"), O::Str("c")],
        PrintOptions {
            sep: "",
            end: "--\n",
            ..Default::default()
        },
    );
    out
}

fn console_methods() -> String {
    use rich::align::HorizontalAlign;
    let console = console(20);
    let mut out = console.capture(|c| {
        c.line(2);
        c.rule("T", "=", "red", HorizontalAlign::Left);
        c.rule("", "─", "rule.line", HorizontalAlign::Center);
        c.out(&["a", "b [b]c[/b]"], ",", "\n", Some("bold".into()), None)
            .unwrap();
        c.out(&["1 2"], " ", "|\n", None, Some(true)).unwrap();
        c.print_json(
            r#"{"a": [1, true, null], "b": "x"}"#,
            &rich::json::JsonOptions::default(),
        )
        .unwrap();
    });
    out += &lines(&[
        pair(console.measure(&Text::new("hello world"), None)),
        pair(console.measure(&Panel::new(Box::new(String::from("ab"))), None)),
    ]);
    out
}

fn build(name: &str) -> String {
    match name {
        "measure_api" => measure_api(),
        "measure_default" => measure_default(),
        "measure_default_in_table" => measure_default_in_table(),
        "terminal_themes" => terminal_themes(),
        "lines_justify" => lines_justify(),
        "lines_ops" => lines_ops(),
        "lines_render" => lines_render(),
        "renderables_render" => renderables_render(),
        "ansi_decode" => ansi_decode(),
        "highlighters" => highlighters(),
        "color_api" => color_api(),
        "color_parse_errors" => color_parse_errors(),
        "color_rich" => color_rich(),
        "palette_rich" => palette_rich(),
        "style_api" => style_api(),
        "style_parse_errors" => style_parse_errors(),
        "style_render" => style_render(),
        "markup_meta" => markup_meta(),
        "markup_errors" => markup_errors(),
        "markup_tag_and_style" => markup_tag_and_style(),
        "text_api" => text_api(),
        "text_edit" => text_edit(),
        "text_wrap" => text_wrap(),
        "text_fit" => text_fit(),
        "text_from_markup" => text_from_markup(),
        "span_api" => span_api(),
        "segment_split_cells" => segment_split_cells(),
        "segment_divide" => segment_divide(),
        "segment_lines" => segment_lines(),
        "segment_shape" => segment_shape(),
        "segment_misc" => segment_misc(),
        "segments_render" => segments_render(),
        "console_print" => console_print(),
        "console_methods" => console_methods(),
        other => panic!("no builder for parity_core case {other:?}"),
    }
}

#[test]
fn parity_core() {
    let data = include_str!("golden/parity_core.tsv");
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
    assert_eq!(checked, 34, "expected every parity core case to run");
}
