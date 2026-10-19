//! A range on a list (round 6): Shift with the moves extends it from an
//! anchor; a list without one leaves Shift to its ancestors.

use intuituive::prelude::*;

fn items() -> Vec<String> {
    (0..8).map(|i| format!("line {i}")).collect()
}

#[test]
fn shift_extends_a_range_and_a_plain_move_ends_it() {
    let app = App::new(|| {
        let (cursor, anchor) = (signal(2usize), signal(None));
        column([
            list(items, cursor).range(anchor).panel("diff").fixed(10),
            text(move || format!("{:?} {}", anchor.get(), cursor.get())),
        ])
    });
    let mut driver = app.driver(16, 11);
    driver.press("shift+up shift+up");
    assert_eq!(driver.screen().plain()[10].trim_end(), "Some(2) 0");
    driver.press("shift+end");
    assert_eq!(driver.screen().plain()[10].trim_end(), "Some(2) 7");
    let styled = intuituive::testing::styled(&driver);
    let lit = styled.lines().filter(|l| l.contains("[on grey30]")).count();
    // Rows 2 to 6 lit, row 7 the cursor's.
    assert_eq!(lit, 5, "{styled}");
    driver.press("home");
    assert_eq!(driver.screen().plain()[10].trim_end(), "None 0");
}

#[test]
fn without_a_range_shift_goes_on_to_the_ancestors() {
    let app = App::new(|| {
        let (cursor, heard) = (signal(0usize), signal(0));
        column([list(items, cursor).fixed(8), text!("{heard} {cursor}")])
            .on_key("shift+down", move |_| heard.update(|h| *h += 1))
    });
    let mut driver = app.driver(16, 9);
    driver.press("shift+down");
    assert_eq!(driver.screen().plain()[8].trim_end(), "1 0");
}
