//! Text alignment, wrapping and overflow (0.0.19 gap A3).

use intuituive::prelude::*;
use intuituive::{assert_screen, Driver};

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

const POEM: &str = "The quick brown\nfox jumps over\nthe lazy dog";

#[test]
fn text_centered() {
    let app = App::new(|| label(POEM).text_align(TextAlign::Center).panel("Centred"));
    assert_screen!(shown(app, 22, 5), "text_centered");
}

#[test]
fn text_right() {
    let app = App::new(|| label(POEM).text_align(TextAlign::Right).panel("Right"));
    let driver = shown(app, 22, 5);
    assert_screen!(driver, "text_right");
    assert_eq!(driver.screen().plain()[3], "│        the lazy dog│");
}

#[test]
fn text_no_wrap_ellipsis() {
    let app = App::new(|| {
        column([
            label("cut at the edge, not wrapped").no_wrap(),
            label("cut with an ellipsis instead")
                .no_wrap()
                .overflow(Overflow::Ellipsis),
            label("wrapped as before, a word at a time"),
        ])
    });
    let driver = shown(app, 16, 4);
    assert_screen!(driver, "text_no_wrap_ellipsis");
    let rows = driver.screen().plain();
    assert_eq!(rows[0], "cut at the edge,");
    assert_eq!(rows[1], "cut with an ell…");
}

#[test]
fn text_justify() {
    let app = App::new(|| {
        label("Words spread out evenly to fill each line but the last one.")
            .text_align(TextAlign::Justify)
    });
    let driver = shown(app, 20, 4);
    assert_screen!(driver, "text_justify");
    // Full lines reach both edges; the last is left as it is.
    let rows = driver.screen().plain();
    assert!(
        rows[0].starts_with("Words") && rows[0].ends_with("out"),
        "{rows:?}"
    );
}

#[test]
fn a_stylesheets_text_align_matches_the_code() {
    let code = App::new(|| label(POEM).text_align(TextAlign::Center));
    let sheet = App::new(|| label(POEM)).stylesheet("label { text-align: center; }");
    assert_eq!(
        shown(code, 20, 3).screen().plain(),
        shown(sheet, 20, 3).screen().plain()
    );
    let code = App::new(|| label("a long line").no_wrap().overflow(Overflow::Ellipsis));
    let sheet = App::new(|| label("a long line"))
        .stylesheet("label { text-wrap: nowrap; text-overflow: ellipsis; }");
    assert_eq!(
        shown(code, 6, 2).screen().plain(),
        shown(sheet, 6, 2).screen().plain()
    );
    // Code wins over the sheet.
    let app = App::new(|| label("x").text_align(TextAlign::Left))
        .stylesheet("label { text-align: right; }");
    assert_eq!(shown(app, 3, 1).screen().plain()[0], "x  ");
}

#[test]
fn an_auto_node_still_measures_its_natural_width() {
    let app = App::new(|| {
        row([
            label("ab").text_align(TextAlign::Center).auto(),
            label("|").auto(),
            label("cd").text_align(TextAlign::Right),
        ])
    });
    assert_eq!(shown(app, 8, 1).screen().plain()[0], "ab|   cd");
    // `text!` takes the options too.
    let app = App::new(|| {
        let n = signal(7);
        text!("n = {n}").text_align(TextAlign::Right)
    });
    assert_eq!(shown(app, 8, 1).screen().plain()[0], "   n = 7");
}
