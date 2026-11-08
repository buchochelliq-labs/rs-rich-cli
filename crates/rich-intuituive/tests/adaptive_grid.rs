//! Container driven grid policy and the real retained Driver path.
use intuituive::button;
use intuituive::layout::adaptive_columns;
use intuituive::prelude::*;
use intuituive::widgets::hsplit;

#[test]
fn adaptive_policy_bounds_thresholds_and_overflow() {
    for (w, min, gap, max, expected) in [
        (0, 20, 1, 4, 1),
        (40, 20, 1, 4, 1),
        (41, 20, 1, 4, 2),
        (80, 20, 1, 4, 3),
        (120, 20, 1, 4, 4),
        (160, 20, 1, 4, 4),
        (100, 0, 0, 0, 0),
        (100, 0, 0, 1000, 100),
        (u16::MAX, u16::MAX, u16::MAX, usize::MAX, 1),
    ] {
        assert_eq!(adaptive_columns(w, min, gap, max), expected);
    }
    let mut last = 0;
    for w in 0..=u16::MAX {
        let n = adaptive_columns(w, 17, 3, usize::MAX);
        assert!(n >= last);
        assert!(n == 1 || n * 17 + (n - 1) * 3 <= usize::from(w));
        last = n;
    }
}

fn cards() -> Node {
    grid(
        [],
        (0..4).map(|i| {
            label(format!(
                "Card {i} 海の光 long Unicode title wraps over several lines"
            ))
            .name(&format!("card{i}"))
        }),
    )
    .adaptive_columns(20, 4)
    .rows([Size::Auto])
    .gap(1)
    .name("cards")
}

#[test]
fn retained_grid_measures_wrapped_rows_at_each_parent_width() {
    let mut d =
        App::new(|| column([cards().auto(), label("after").auto().name("after")])).driver(80, 24);
    for (w, h, n) in [
        (80, 24, 3),
        (120, 40, 4),
        (160, 50, 4),
        (40, 24, 1),
        (80, 24, 3),
    ] {
        d.resize(w, h);
        d.settle();
        let a = d.rect_of("card0").unwrap();
        let b = d.rect_of("card1").unwrap();
        assert_eq!(a.y == b.y, n > 1, "{w}: {a:?} {b:?}");
        let last = d.rect_of("card3").unwrap();
        assert!(last.bottom() <= d.rect_of("after").unwrap().y);
        for i in 0..4 {
            let r = d.rect_of(&format!("card{i}")).unwrap();
            assert!(r.right() <= w);
            assert!(r.height > 0);
        }
    }
}

#[test]
fn forty_column_split_inside_wide_terminal_uses_container() {
    let mut d =
        App::new(|| hsplit(cards(), label("other pane"), signal(40.0 / 159.0))).driver(160, 50);
    d.settle();
    assert_eq!(d.rect_of("cards").unwrap().width, 40);
    assert!(d.rect_of("card1").unwrap().y > d.rect_of("card0").unwrap().y);
}

#[test]
fn resizing_retains_focus_draft_and_pointer_action_in_both_themes() {
    use intuituive::Theme;
    use std::cell::Cell;
    use std::rc::Rc;
    for theme in [Theme::light(), Theme::dark()] {
        let calls = Rc::new(Cell::new(0));
        let hit = calls.clone();
        let mut d = App::new(move || {
            grid(
                [],
                [
                    input(signal(String::new())).name("draft"),
                    button("Apply 海の光", move |_| hit.set(hit.get() + 1)).name("apply"),
                    button("Disabled", |_| panic!("disabled action reached"))
                        .disabled_when(|| true)
                        .name("disabled"),
                ],
            )
            .adaptive_columns(25, 3)
            .rows([Size::Auto])
            .gap(1)
        })
        .theme(theme)
        .driver(120, 40);
        d.settle();
        d.click_on("draft");
        d.type_text("persistent draft");
        let id = d.focused();
        for (w, h) in [(40, 24), (80, 24), (160, 50), (120, 40)] {
            d.resize(w, h);
            d.settle();
            assert_eq!(d.focused(), id);
            d.assert_focused("draft");
            assert!(d.screen().plain().join("\n").contains("persistent draft"));
            let r = d.rect_of("apply").unwrap();
            d.click(r.x + 1, r.y);
            assert!(calls.get() > 0);
            d.press("enter");
            d.click_on("disabled");
            d.click_on("draft");
            assert_eq!(d.focused(), id);
        }
        assert_eq!(calls.get(), 8);
    }
}

#[test]
fn adaptive_tracks_override_css_and_respect_padding_spans_and_hidden_children() {
    let mut d = App::new(|| {
        grid(
            [Size::Fixed(1); 4],
            [
                label("wide").span(2, 1).name("wide"),
                label("hidden").shown_when(|| false),
                label("last").name("last"),
            ],
        )
        .adaptive_columns(20, 4)
        .rows([Size::Auto])
        .gap(1)
        .padding(1, 2)
        .name("grid")
    })
    .stylesheet("#grid { grid-columns: 1 1 1 1; }")
    .driver(45, 10);
    d.settle();
    let wide = d.rect_of("wide").unwrap();
    let last = d.rect_of("last").unwrap();
    assert_eq!(wide.width, 41);
    assert_eq!(wide.x, 2);
    assert!(last.y > wide.y);
    assert!(!d.screen().plain().join("\n").contains("hidden"));
}
