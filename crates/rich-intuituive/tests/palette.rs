//! Semantic themes (#801): Textual's palettes and the shades generated
//! from them, checked against Textual's own values, for contrast, and on
//! the widget gallery.

#[path = "../examples/widgets.rs"]
#[allow(dead_code)]
mod widgets;

use intuituive::prelude::*;
use intuituive::{Palette, assert_screen_styled};
use rich_ext::a11y::contrast::{CheckOptions, Severity, check_theme};

const GOLDEN: &str = include_str!("fixtures/textual_themes.txt");

/// `#RRGGBB[AA]` as channels.
fn channels(hex: &str) -> Vec<i32> {
    let hex = hex.trim_start_matches('#');
    (0..hex.len() / 2)
        .map(|i| i32::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

#[test]
fn shades_match_textuals_within_rounding() {
    let mut checked = 0;
    let mut wrong = Vec::new();
    for line in GOLDEN.lines().filter(|l| !l.starts_with('#')) {
        let mut parts = line.split_whitespace();
        let (theme, name, want) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            parts.next().unwrap(),
        );
        let colors = Palette::named(theme).unwrap().colors();
        let got = &colors
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{theme} has no {name}"))
            .1;
        let (a, b) = (channels(got), channels(want));
        let close = a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 1);
        if !close {
            wrong.push(format!("{theme} {name}: {got}, Textual {want}"));
        }
        checked += 1;
    }
    assert!(checked > 2000, "{checked}");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn textual_dark_is_exact() {
    let colors = Palette::named("textual-dark").unwrap().colors();
    for line in GOLDEN.lines().filter(|l| l.starts_with("textual-dark ")) {
        let mut parts = line.split_whitespace().skip(1);
        let (name, want) = (parts.next().unwrap(), parts.next().unwrap());
        let got = &colors.iter().find(|(n, _)| n == name).unwrap().1;
        assert_eq!(got, want, "{name}");
    }
}

/// The styles text is drawn in.
const TEXT: &[&str] = &[
    "text",
    "text-primary",
    "text-secondary",
    "text-warning",
    "text-error",
    "text-success",
    "text-accent",
    "good",
    "warn",
    "bad",
];

#[test]
fn every_builtin_theme_passes_the_contrast_check() {
    for name in Theme::builtin_names() {
        let theme = Theme::named(name).unwrap_or_else(|| panic!("{name}"));
        let Some(palette) = Palette::named(name) else {
            continue; // the presets: terminal colours
        };
        let colors = palette.colors();
        let get = |n: &str| colors.iter().find(|(c, _)| c == n).unwrap().1.clone();
        let (background, foreground) = (get("background"), get("foreground"));
        if !background.starts_with('#') {
            continue; // an ANSI palette: the terminal's own colours
        }
        let rgb = |hex: &str| {
            let c = channels(hex);
            (c[0] as u8, c[1] as u8, c[2] as u8)
        };
        let terminal =
            rich::TerminalTheme::new(rgb(&background), rgb(&foreground), &[(0, 0, 0); 8], None);
        let styles = theme
            .styles
            .iter()
            .filter(|(n, _)| TEXT.contains(&n.as_str()))
            .cloned();
        let checked = rich::Theme::from_styles(styles, false).unwrap();
        let options = CheckOptions {
            backgrounds: vec![terminal],
            groups: Vec::new(),
            ..CheckOptions::default()
        };
        let errors: Vec<_> = check_theme(&checked, &options)
            .into_iter()
            .filter(|f| f.severity == Severity::Error)
            .collect();
        // No text under 3:1 (WCAG's floor for large text). Textual's light
        // palettes keep a few warning and success texts between 3:1 and
        // AA's 4.5:1, which the check reports as warnings.
        assert!(errors.is_empty(), "{name}: {errors:#?}");
    }
}

#[test]
fn the_presets_render_as_before() {
    let app = App::new(|| label("[accent]a[/] [bad]b[/] [good]c[/]")).theme(Theme::dark());
    let mut driver = app.driver(8, 1);
    driver.settle();
    assert_eq!(
        intuituive::testing::styled(&driver).trim_end(),
        "[bright_cyan]a[/] [bold red]b[/] [green]c[/]"
    );
    assert_eq!(Theme::named("dark").unwrap().styles, Theme::dark().styles);
}

#[test]
fn shades_work_in_markup_and_stylesheets() {
    let app = App::new(|| {
        column([
            label("[primary-lighten-2]a[/] [text-muted]b[/]"),
            label("c").class("box"),
        ])
    })
    .theme(Theme::named("nord").unwrap())
    .stylesheet(".box { color: $error; background: $surface; }");
    let mut driver = app.driver(6, 2);
    driver.settle();
    let styled = intuituive::testing::styled(&driver);
    let colors = Palette::named("nord").unwrap().colors();
    let get = |n: &str| {
        colors
            .iter()
            .find(|(c, _)| c == n)
            .unwrap()
            .1
            .to_lowercase()
    };
    assert!(
        styled.contains(&format!("[{}]a[/]", get("primary-lighten-2"))),
        "{styled}"
    );
    assert!(
        styled.contains(&format!("[{} on {}]c", get("error"), get("surface"))),
        "{styled}"
    );
}

#[test]
fn a_theme_switches_by_name_at_run_time() {
    let app = App::new(|| {
        label("[accent]x[/]")
            .on_key("n", |cx| {
                cx.set_theme_named("nord");
            })
            .on_key("z", |cx| assert!(!cx.set_theme_named("zzz")))
    });
    let mut driver = app.driver(2, 1);
    driver.settle();
    assert!(intuituive::testing::styled(&driver).contains("[bright_cyan]x"));
    driver.press("n z");
    let accent = Palette::named("nord").unwrap().colors();
    let accent = accent
        .iter()
        .find(|(n, _)| n == "accent")
        .unwrap()
        .1
        .to_lowercase();
    assert!(intuituive::testing::styled(&driver).contains(&format!("[{accent}]x")));
}

#[test]
fn the_widget_gallery_in_three_themes() {
    for name in ["textual-dark", "nord", "solarized-light"] {
        let mut driver = widgets::widgets_app(widgets::Options::default())
            .theme(Theme::named(name).unwrap())
            .driver(80, 24);
        driver.settle();
        assert_screen_styled!(driver, &format!("palette_widgets_{name}"));
    }
}
