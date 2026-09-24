//! Badges, status chips and size bars (0.0.11 workstream 10).

use std::sync::Arc;

use rich::cells::cell_len;
use rich::{Cell, ColorSystem, Console, Renderable, Table};
use rich_ext::a11y::{AccessibleText, Status, SymbolSet};
use rich_ext::badge::{Badge, Badges};
use rich_ext::size_bar::{SizeBar, Units};

fn plain(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

fn ascii(width: usize) -> Console {
    Console::builder()
        .width(width)
        .color_system(None)
        .ascii_only(true)
        .build()
}

fn color(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .theme(rich_ext::extended_theme())
        .build()
}

fn no_color(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .no_color(true)
        .theme(rich_ext::extended_theme())
        .build()
}

fn render(console: &Console, r: &dyn Renderable) -> String {
    console.segments_to_string(&r.rich_render(console, &console.options()))
}

// ---------------------------------------------------------------- badges

#[test]
fn plain_badges_carry_meaning_without_colour() {
    let c = plain(80);
    let cases = [
        (Badge::label("beta"), "[beta]"),
        (Badge::status(Status::Ok, "build"), "[OK build]"),
        (Badge::status(Status::Error, ""), "[ERROR]"),
        (Badge::status(Status::Warning, "lint"), "[WARN lint]"),
        (Badge::status(Status::Skipped, "docs"), "[SKIP docs]"),
        (
            Badge::link("docs", "https://example.com/docs"),
            "[docs <https://example.com/docs>]",
        ),
        (Badge::meta("version", "1.2.0"), "[version: 1.2.0]"),
    ];
    for (badge, expected) in cases {
        assert_eq!(render(&c, &badge), expected);
        assert_eq!(badge.plain(), expected);
        assert_eq!(badge.accessible_text(80), expected);
    }
}

#[test]
fn symbol_sets_choose_the_status_marker() {
    let c = plain(80);
    let ok = |set| Badge::status(Status::Ok, "build").symbols(set);
    assert_eq!(render(&c, &ok(SymbolSet::Words)), "[ok: build]");
    assert_eq!(render(&c, &ok(SymbolSet::Unicode)), "[✔ build]");
    assert_eq!(render(&c, &ok(SymbolSet::Ascii)), "[OK build]");
    let bare = Badge::status(Status::Ok, "").symbols(SymbolSet::Words);
    assert_eq!(render(&c, &bare), "[ok]");
    // An ASCII-only console never gets the Unicode glyphs.
    assert_eq!(render(&ascii(80), &ok(SymbolSet::Unicode)), "[OK build]");
    // The plain form is ASCII even for a Unicode badge.
    assert_eq!(ok(SymbolSet::Unicode).plain(), "[OK build]");
}

#[test]
fn colour_badges_are_padded_chips_on_theme_styles() {
    let c = color(80);
    assert_eq!(
        render(&c, &Badge::status(Status::Ok, "build")),
        "\x1b[1;30;42m ✔ build \x1b[0m"
    );
    assert_eq!(
        render(&c, &Badge::status(Status::Error, "")),
        "\x1b[1;97;41m ✖ error \x1b[0m"
    );
    assert_eq!(
        render(&c, &Badge::label("beta")),
        "\x1b[30;47m beta \x1b[0m"
    );
    assert_eq!(
        render(&c, &Badge::meta("version", "1.2.0")),
        "\x1b[97;100m version \x1b[0m\x1b[30;106m 1.2.0 \x1b[0m"
    );
    // A style override: a theme key or a definition.
    assert_eq!(
        render(&c, &Badge::label("hot").style("badge.error")),
        "\x1b[1;97;41m hot \x1b[0m"
    );
    assert_eq!(
        render(&c, &Badge::label("x").style("black on magenta")),
        "\x1b[30;45m x \x1b[0m"
    );
}

#[test]
fn links_are_osc8_where_styles_render_and_spelled_out_elsewhere() {
    let link = Badge::link("docs", "https://example.com");
    assert_eq!(
        render(&color(80), &link),
        "\x1b]8;;https://example.com\x1b\\\x1b[4;30;104m docs \x1b[0m\x1b]8;;\x1b\\"
    );
    // NO_COLOR keeps the hyperlink and the underline; brackets replace colour.
    assert_eq!(
        render(&no_color(80), &link),
        "\x1b]8;;https://example.com\x1b\\\x1b[4m[docs]\x1b[0m\x1b]8;;\x1b\\"
    );
    assert_eq!(render(&plain(80), &link), "[docs <https://example.com>]");
    assert_eq!(render(&plain(80), &link.clone().show_url(false)), "[docs]");
    assert_eq!(
        render(&no_color(80), &link.show_url(true)),
        "\x1b]8;;https://example.com\x1b\\\x1b[4m[docs <https://example.com>]\x1b[0m\x1b]8;;\x1b\\"
    );
}

#[test]
fn no_color_keeps_brackets_and_attributes() {
    assert_eq!(
        render(&no_color(80), &Badge::status(Status::Ok, "build")),
        "\x1b[1m[OK build]\x1b[0m"
    );
}

#[test]
fn a_row_wraps_between_badges_never_inside_one() {
    let row = Badges::new([
        Badge::status(Status::Ok, "build"),
        Badge::status(Status::Error, "tests"),
        Badge::label("beta"),
        Badge::meta("version", "1.2.0"),
    ]);
    assert_eq!(
        render(&plain(80), &row),
        "[OK build] [ERROR tests] [beta] [version: 1.2.0]"
    );
    assert_eq!(
        render(&plain(30), &row),
        "[OK build] [ERROR tests]\n[beta] [version: 1.2.0]"
    );
    assert_eq!(
        row.clone().separator(" · ").plain(),
        "[OK build] · [ERROR tests] · [beta] · [version: 1.2.0]"
    );
    assert_eq!(
        row.accessible_text(80),
        "[OK build] [ERROR tests] [beta] [version: 1.2.0]"
    );
    // A badge wider than the line is cut with an ellipsis.
    assert_eq!(
        render(&plain(8), &Badge::label("very long label")),
        "[very l…"
    );
}

#[test]
fn badges_measure_to_their_width_and_fit_table_cells() {
    let c = plain(80);
    let row = Badges::new([Badge::label("a"), Badge::label("bbbb")]);
    let m = row.measure(&c, &c.options());
    assert_eq!((m.minimum, m.maximum), (6, 10));
    let m = Badge::status(Status::Ok, "").measure(&c, &c.options());
    assert_eq!((m.minimum, m.maximum), (4, 4));

    let mut table = Table::new();
    table.add_column("Crate");
    table.add_column("Status");
    table.add_row_cells(vec![
        Cell::Markup("core".into()),
        Cell::Renderable(Arc::new(Badge::status(Status::Ok, "tests"))),
    ]);
    table.add_row_cells(vec![
        Cell::Markup("ext".into()),
        Cell::Renderable(Arc::new(Badge::status(Status::Error, "lint"))),
    ]);
    assert_eq!(
        render(&c, &table),
        [
            "┏━━━━━━━┳━━━━━━━━━━━━━━┓",
            "┃ Crate ┃ Status       ┃",
            "┡━━━━━━━╇━━━━━━━━━━━━━━┩",
            "│ core  │ [OK tests]   │",
            "│ ext   │ [ERROR lint] │",
            "└───────┴──────────────┘",
        ]
        .join("\n")
    );
}

// ---------------------------------------------------------------- size bars

#[test]
fn size_bar_plain_lines() {
    let c = plain(80);
    let bar = SizeBar::new(1_500_000, 4_000_000).bar_width(10);
    assert_eq!(render(&c, &bar), "████░░░░░░  1.5 MB / 4.0 MB  38%");
    let labelled = SizeBar::new(750, 1000).label("lib").bar_width(8);
    assert_eq!(
        render(&c, &labelled),
        "lib  ██████░░  750 bytes / 1.0 kB  75%"
    );
    let binary = SizeBar::new(1_572_864, 4_194_304)
        .units(Units::Binary)
        .bar_width(8);
    assert_eq!(render(&c, &binary), "███░░░░░  1.5 MiB / 4.0 MiB  38%");
    let quiet = SizeBar::new(1, 2)
        .bar_width(4)
        .show_sizes(false)
        .show_percent(false);
    assert_eq!(render(&c, &quiet), "██░░");
}

#[test]
fn size_bar_never_rounds_away_a_small_or_a_missing_part() {
    let c = plain(80);
    let tiny = SizeBar::new(1, 1_000_000).bar_width(10).show_sizes(false);
    assert_eq!(render(&c, &tiny), "█░░░░░░░░░  0%");
    let almost = SizeBar::new(999_999, 1_000_000)
        .bar_width(10)
        .show_sizes(false);
    assert_eq!(render(&c, &almost), "█████████░  100%");
    let empty = SizeBar::new(0, 0).bar_width(4).show_sizes(false);
    assert_eq!(render(&c, &empty), "░░░░  0%");
}

#[test]
fn over_limit_is_shown_by_glyph_and_words() {
    let c = plain(80);
    let over = SizeBar::limit(5_000_000, 4_000_000)
        .label("pkg")
        .bar_width(10);
    assert!(over.is_over());
    assert_eq!(
        render(&c, &over),
        "pkg  ████████▓▓  5.0 MB / 4.0 MB  125%  over by 1.0 MB"
    );
    assert_eq!(
        render(&ascii(80), &over),
        "pkg  ########!!  5.0 MB / 4.0 MB  125%  over by 1.0 MB"
    );
    let from_nothing = SizeBar::new(10, 0).bar_width(4);
    assert_eq!(
        render(&c, &from_nothing),
        "▓▓▓▓  10 bytes / 0 bytes  -  over by 10 bytes"
    );
}

#[test]
fn size_bar_ascii_fallback() {
    let bar = SizeBar::new(1_500_000, 4_000_000).bar_width(10);
    let out = render(&ascii(80), &bar);
    assert_eq!(out, "####......  1.5 MB / 4.0 MB  38%");
    assert!(out.is_ascii());
}

#[test]
fn size_bar_styles_mark_normal_high_and_over() {
    let c = color(80);
    let normal = SizeBar::new(1, 2).bar_width(2).show_sizes(false);
    assert_eq!(render(&c, &normal), "\x1b[32m█\x1b[0m\x1b[90m░\x1b[0m  50%");
    let high = SizeBar::limit(95, 100).bar_width(2).show_sizes(false);
    assert!(high.is_high());
    assert_eq!(
        render(&c, &high),
        "\x1b[33m█\x1b[0m\x1b[90m░\x1b[0m  \x1b[33m95%\x1b[0m"
    );
    let over = SizeBar::limit(3, 2).bar_width(3).show_sizes(false);
    assert_eq!(
        render(&c, &over),
        "\x1b[1;31m██\x1b[0m\x1b[1;31m▓\x1b[0m  \x1b[1;31m150%\x1b[0m  \x1b[1;31mover by 1 byte\x1b[0m"
    );
}

#[test]
fn size_bar_shrinks_to_fit_and_measures() {
    let bar = SizeBar::new(1_500_000, 4_000_000).label("docs");
    let c = plain(80);
    let m = bar.measure(&c, &c.options());
    // "docs  " + bar + "  1.5 MB / 4.0 MB" + "  38%"
    assert_eq!((m.minimum, m.maximum), (6 + 4 + 17 + 5, 6 + 20 + 17 + 5));
    let narrow = plain(40);
    let out = render(&narrow, &bar);
    assert_eq!(out, "docs  █████░░░░░░░  1.5 MB / 4.0 MB  38%");
    assert_eq!(cell_len(&out), 40);
    // Narrower than the minimum: the line is cut, never wrapped.
    let out = render(&plain(20), &bar);
    assert_eq!(cell_len(&out), 20);
    assert!(!out.contains('\n'));
}

#[test]
fn size_bars_line_up_in_a_table() {
    let total = 10_000_000;
    let mut table = Table::new();
    table.add_column("File");
    table.add_column("Size");
    for (name, size) in [("app.wasm", 6_200_000), ("vendor.js", 2_500_000)] {
        table.add_row_cells(vec![
            Cell::Markup(name.into()),
            Cell::Renderable(Arc::new(
                SizeBar::new(size, total).bar_width(10).show_percent(false),
            )),
        ]);
    }
    assert_eq!(
        render(&plain(80), &table),
        [
            "┏━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓",
            "┃ File      ┃ Size                         ┃",
            "┡━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩",
            "│ app.wasm  │ ██████░░░░  6.2 MB / 10.0 MB │",
            "│ vendor.js │ ███░░░░░░░  2.5 MB / 10.0 MB │",
            "└───────────┴──────────────────────────────┘",
        ]
        .join("\n")
    );
}
