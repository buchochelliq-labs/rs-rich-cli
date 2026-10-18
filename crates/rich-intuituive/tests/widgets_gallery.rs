//! The widget gallery (examples/widgets.rs).

#[path = "../examples/widgets.rs"]
#[allow(dead_code)]
mod widgets;

use intuituive::Driver;
use widgets::{widgets_app, Options};

fn driver(tab: usize) -> Driver {
    let mut driver = widgets_app(Options {
        tab,
        ..Options::default()
    })
    .driver(80, 24);
    driver.settle();
    driver
}

#[test]
fn dump2() {
    for (tab, keys) in [(3, "down right"), (3, "v down"), (7, "p"), (2, "down tab down down"), (8, "down"), (4, "n n"), (5, "j j l l")] {
        let mut d = driver(tab);
        d.press(keys);
        println!("=== {tab} {keys}");
        for r in d.screen().plain() {
            println!("{}", r.trim_end());
        }
    }
}

#[test]
fn dump() {
    for tab in 0..10 {
        let d = driver(tab);
        println!("=== {tab}");
        for r in d.screen().plain() {
            println!("{}", r.trim_end());
        }
    }
}
