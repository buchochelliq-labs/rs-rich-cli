//! Container driven grid policy and the real retained Driver path.
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
