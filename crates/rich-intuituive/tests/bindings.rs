//! Bindings that apply in a state (`bind_when`, `when`, `in_mode`,
//! `modes`), help sections for every kind of binding (`section`), bindings
//! that toggle or step a field, `bind_scaled`'s labels, and the named
//! punctuation keys.

use intuituive::prelude::*;
use intuituive::widgets::key_hints;
use intuituive::{HelpOptions, KeyFormat};

fn first(driver: &intuituive::Driver) -> String {
    driver.screen().plain()[0].trim_end().to_string()
}

fn help(driver: &mut intuituive::Driver) -> String {
    driver.press("?");
    let rows = driver.screen().plain().join("\n");
    driver.press("esc");
    rows
}

#[test]
fn two_bindings_share_a_key_under_different_conditions() {
    let app = App::new(|| {
        let (mode, said) = (signal(0), signal("-"));
        text!("{said}")
            .bind_when(
                move || mode.get() == 0,
                "p",
                "Peaks",
                move |_| said.set("peaks"),
            )
            .bind_when(
                move || mode.get() == 1,
                "p",
                "Phase",
                move |_| said.set("phase"),
            )
            .on_key("tab", move |_| mode.update(|m| *m = (*m + 1) % 3))
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(60, 12);
    driver.press("p");
    assert_eq!(first(&driver), "peaks");
    let rows = help(&mut driver);
    assert!(rows.contains("Peaks") && !rows.contains("Phase"), "{rows}");
    driver.press("tab p");
    assert_eq!(first(&driver), "phase");
    let rows = help(&mut driver);
    assert!(rows.contains("Phase") && !rows.contains("Peaks"), "{rows}");
    // In the third mode, neither applies: `p` does nothing.
    driver.press("tab p");
    assert_eq!(first(&driver), "phase");
}

#[test]
fn a_binding_that_does_not_apply_lets_the_key_go_up() {
    let app = App::new(|| {
        let (on, said) = (signal(false), signal("-"));
        column([text!("{said}").focusable().bind_when(
            move || on.get(),
            "x",
            "Inner",
            move |_| said.set("inner"),
        )])
        .on_key("x", move |_| said.set("outer"))
        .on_key("o", move |_| on.set(true))
    });
    let mut driver = app.driver(10, 1);
    driver.press("x");
    assert_eq!(first(&driver), "outer");
    driver.press("o x");
    assert_eq!(first(&driver), "inner");
}

#[test]
fn a_footer_lists_only_the_bindings_that_apply() {
    let app = App::new(|| {
        let on = signal(false);
        column([label("body").flex(1), key_hints().fixed(1)])
            .bind_when(move || on.get(), "s", "Save", |_| {})
            .bind("o", "On", move |_| on.set(true))
    });
    let mut driver = app.driver(40, 3);
    driver.settle();
    let footer = driver.screen().plain()[2].clone();
    assert!(
        footer.contains("On") && !footer.contains("Save"),
        "{footer}"
    );
    driver.press("o");
    let footer = driver.screen().plain()[2].clone();
    assert!(footer.contains("Save"), "{footer}");
}

#[test]
fn sequences_in_a_when_scope_apply_only_while_it_holds() {
    let app = App::new(|| {
        let (on, said) = (signal(false), signal("-"));
        text!("{said}")
            .when(
                move || on.get(),
                |node| node.bind_sequence("g g", "Top", move |_| said.set("top")),
            )
            .on_key("o", move |_| on.set(true))
    });
    let mut driver = app.driver(10, 1);
    driver.press("g g");
    assert_eq!(first(&driver), "-");
    driver.press("o g g");
    assert_eq!(first(&driver), "top");
}

#[test]
fn nested_scopes_need_every_condition() {
    let app = App::new(|| {
        let (a, b, n) = (signal(false), signal(false), signal(0));
        text!("{n}")
            .when(
                move || a.get(),
                |node| {
                    node.when(
                        move || b.get(),
                        |node| node.bind("+", "More", move |_| n.update(|n| *n += 1)),
                    )
                },
            )
            .on_key("a", move |_| a.set(true))
            .on_key("b", move |_| b.set(true))
    });
    let mut driver = app.driver(5, 1);
    driver.press("+ a +");
    assert_eq!(first(&driver), "0");
    driver.press("b +");
    assert_eq!(first(&driver), "1");
}

#[test]
fn modes_list_and_apply_only_the_shown_modes_bindings() {
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Osc,
        Spec,
    }
    let app = App::new(|| {
        let (mode, peaks, log) = (signal(Mode::Osc), signal(false), signal(false));
        let view = switch(
            move || mode.get() == Mode::Osc,
            |osc| {
                if osc {
                    label("osc")
                } else {
                    label("spec")
                }
            },
        );
        column([view.fixed(1), text!("{peaks} {log}").fixed(1)])
            .modes(
                mode,
                &[
                    (Mode::Osc, "oscilloscope", &|node: Node| {
                        node.bind_toggle("p", "Peaks", peaks, |p| p)
                    }),
                    (Mode::Spec, "spectroscope", &|node: Node| {
                        node.bind_toggle("l", "Logarithmic", log, |l| l)
                    }),
                ],
            )
            .bind_cycle("tab", "Next mode", mode, &[Mode::Osc, Mode::Spec])
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(60, 14);
    driver.press("l p");
    assert_eq!(driver.screen().plain()[1].trim_end(), "true false");
    let rows = help(&mut driver);
    assert!(
        rows.contains("oscilloscope") && rows.contains("Peaks"),
        "{rows}"
    );
    assert!(!rows.contains("spectroscope") && !rows.contains("Logarithmic"));
    driver.press("tab l p");
    assert_eq!(first(&driver), "spec");
    assert_eq!(driver.screen().plain()[1].trim_end(), "true true");
    let rows = help(&mut driver);
    assert!(
        rows.contains("spectroscope") && rows.contains("Logarithmic"),
        "{rows}"
    );
    assert!(!rows.contains("oscilloscope") && !rows.contains("Peaks"));
}

#[test]
fn every_kind_of_binding_takes_a_section() {
    #[derive(Default)]
    struct Scope {
        peaks: bool,
        samples: u32,
    }
    let app = App::new(|| {
        let (state, mode, scale) = (signal(Scope::default()), signal(0), signal(1.0));
        label("scope")
            .section("oscilloscope", |node| {
                node.bind_scaled(
                    "pageup",
                    &[("", 1.0), ("shift", 10.0)],
                    "Threshold up",
                    move |_, by| scale.update(|s| *s += by),
                )
                .bind_cycle("tab", "Next mode", mode, &[0, 1])
                .bind_toggle("p", "Show peaks", state, |s| &mut s.peaks)
                .bind_step("+", "More samples", state, |s| &mut s.samples, 1.0, 0..=8)
                // Its own section, kept.
                .bind_in("other", "o", "Elsewhere", |_| {})
            })
            .bind("q", "Quit", |cx| cx.quit())
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(70, 20);
    let rows = help(&mut driver);
    let at = |text: &str| rows.find(text).unwrap_or_else(|| panic!("{text}: {rows}"));
    let section = at("oscilloscope");
    for row in ["Threshold up", "Next mode", "Show peaks", "More samples"] {
        assert!(section < at(row), "{row}: {rows}");
    }
    assert!(at("other") < at("Elsewhere"));
    // The node's own bindings stay under the node.
    assert!(at("Quit") < section || at("Quit") > at("More samples"));
}

#[test]
fn toggles_and_clamped_steps() {
    #[derive(Default)]
    struct View {
        grid: bool,
        zoom: f64,
        page: usize,
    }
    let app = App::new(|| {
        let view = signal(View {
            grid: false,
            zoom: 1.0,
            page: 2,
        });
        text(move || view.with(|v| format!("{} {} {}", v.grid, v.zoom, v.page)))
            .bind_toggle("g", "Grid", view, |v| &mut v.grid)
            .bind_step("equals", "Zoom in", view, |v| &mut v.zoom, 0.5, 0.5..=2.0)
            .bind_step("minus", "Zoom out", view, |v| &mut v.zoom, -0.5, 0.5..=2.0)
            .bind_step(
                "bracketright",
                "Next page",
                view,
                |v| &mut v.page,
                1.0,
                0..=3,
            )
            .bind_step(
                "bracketleft",
                "Back a page",
                view,
                |v| &mut v.page,
                -1.0,
                0..=3,
            )
    });
    let mut driver = app.driver(20, 1);
    driver.press("g = = = ] ] ]");
    assert_eq!(first(&driver), "true 2 3");
    driver.press("g - - - - [ [ [ [ [");
    assert_eq!(first(&driver), "false 0.5 0");
}

#[test]
fn scaled_variants_say_themselves_with_labels() {
    let app = App::new(|| {
        let scale = signal(0.0);
        text!("{scale}")
            .bind_scaled(
                "up",
                &[("", 1.0, "1"), ("shift", 10.0, "×10"), ("alt", 0.2, "⅕")],
                "Up",
                move |_, by| scale.update(|s| *s += by),
            )
            .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(60, 12);
    driver.press("up shift+up");
    assert_eq!(first(&driver), "11");
    let rows = help(&mut driver);
    assert!(rows.contains("Up (shift ×10, alt ⅕)"), "{rows}");
}

#[test]
fn punctuation_keys_by_name_bind_and_show_as_their_glyph() {
    let app = App::new(|| {
        let said = signal(String::from("-"));
        let mut node = text!("{said}");
        for name in [
            "equals",
            "minus",
            "plus",
            "underscore",
            "slash",
            "backslash",
            "period",
            "semicolon",
            "colon",
            "quote",
            "backtick",
            "tilde",
            "bracketleft",
            "bracketright",
            "lt",
            "gt",
            "question",
        ] {
            node = node.bind(name, name, move |_| said.set(name.to_string()));
        }
        node.on_key("f1", |cx| cx.help())
    });
    let mut driver = app.driver(40, 30);
    // The quote by name: a list of keys reads a quote as one round a name.
    driver.press("= - + _ / \\ . ; : quote ` ~ [ ] < > ?");
    assert_eq!(first(&driver), "question");
    driver.press("dot");
    assert_eq!(first(&driver), "period");
    driver.press("f1");
    let rows = driver.screen().plain().join("\n");
    // The help shows the key, not its name.
    assert!(rows.contains("=  ") && rows.contains("]  "), "{rows}");
}

#[test]
fn the_help_wraps_long_rows_instead_of_cutting_them() {
    let app = App::new(|| {
        label("scope")
            .bind_scaled(
                "pageup",
                &[("", 1.0), ("shift", 10.0), ("ctrl", 5.0), ("alt", 0.2)],
                "Threshold or average up, by the step shown",
                |_, _| {},
            )
            .on_key("?", |cx| cx.help())
    });
    // The help is 70% of 60 columns: far less than the row.
    let mut driver = app.driver(60, 20);
    driver.press("?");
    let rows = driver.screen().plain().join("\n");
    assert!(rows.contains("alt ×0.2)"), "{rows}");
}

#[test]
fn help_options_title_vim_keys_and_the_filter_first_layout() {
    let app = App::new(|| {
        label("files")
            .bind_in("mgr", "ctrl+u", "Half a page up", |_| {})
            .bind_sequence_in("goto", "g g", "To the top", |_| {})
            .bind("shift+tab", "Back", |_| {})
            .on_key("?", |cx| {
                cx.help_with(
                    HelpOptions::new()
                        .title("mgr.help")
                        .key_format(KeyFormat::Vim)
                        .filter_first(true),
                )
            })
    });
    let mut driver = app.driver(60, 20);
    driver.press("?");
    let rows = driver.screen().plain();
    let all = rows.join("\n");
    assert!(all.contains("mgr.help"), "{all}");
    for keys in ["<C-u>", "gg", "<S-Tab>"] {
        assert!(all.contains(keys), "{keys}: {all}");
    }
    // A rule under the search line, and no section headings.
    let search = rows
        .iter()
        .position(|row| row.contains("Search keys"))
        .unwrap();
    assert!(rows[search + 1].contains("───"), "{all}");
    assert!(!all.contains("goto"), "{all}");
    // Typing searches the keys as said.
    driver.press("lt");
    let all = driver.screen().plain().join("\n");
    assert!(
        all.contains("<C-u>") && !all.contains("To the top"),
        "{all}"
    );
}
