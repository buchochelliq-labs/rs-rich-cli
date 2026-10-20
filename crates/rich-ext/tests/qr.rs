//! QR codes (0.0.21): what is drawn reads back as the encoder's modules,
//! in half blocks and ASCII, with colour on and off, inverted, and with the
//! quiet zone narrowed to fit.
#![cfg(feature = "qr")]

use rich::{ColorSystem, Console};
use rich_ext::a11y::AccessibleText;
use rich_ext::chart::Charset;
use rich_ext::qr::{ErrorCorrection, QrCode, QrError};

fn plain(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

fn color(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .no_color(false)
        .build()
}

/// The modules a half-block drawing shows, as rows of `inked` cells.
fn half_blocks(out: &str) -> Vec<Vec<bool>> {
    let mut rows = Vec::new();
    for line in out.lines() {
        let (mut top, mut bottom) = (Vec::new(), Vec::new());
        for c in line.chars() {
            let (t, b) = match c {
                '█' => (true, true),
                '▀' => (true, false),
                '▄' => (false, true),
                ' ' => (false, false),
                other => panic!("unexpected {other:?}"),
            };
            top.push(t);
            bottom.push(b);
        }
        rows.push(top);
        rows.push(bottom);
    }
    rows
}

/// The code's modules with `quiet` light modules round them, dark `true`.
fn expected(code: &QrCode, quiet: usize) -> Vec<Vec<bool>> {
    let side = code.size() + 2 * quiet;
    (0..side)
        .map(|y| {
            (0..side)
                .map(|x| x >= quiet && y >= quiet && code.module(x - quiet, y - quiet))
                .collect()
        })
        .collect()
}

fn code() -> QrCode {
    QrCode::with_error_correction("rs-rich", ErrorCorrection::Low).unwrap()
}

#[test]
fn half_blocks_draw_the_light_modules_without_colour() {
    let code = code();
    assert_eq!(code.size(), 21);
    let out = plain(80).render_export(&code);
    let mut drawn = half_blocks(&out);
    // 29 rows of modules on 15 lines: the last line's bottom half is empty.
    assert_eq!(drawn.len(), 30);
    assert!(drawn.pop().unwrap().iter().all(|inked| !inked));
    let light: Vec<Vec<bool>> = expected(&code, 4)
        .into_iter()
        .map(|row| row.into_iter().map(|dark| !dark).collect())
        .collect();
    assert_eq!(drawn, light);
    // Inverted, the dark ones.
    let inverted = plain(80).render_export(&code.clone().invert(true));
    let mut drawn = half_blocks(&inverted);
    drawn.pop();
    assert_eq!(drawn, expected(&code, 4));
}

#[test]
fn colour_paints_dark_on_light() {
    let code = code().quiet_zone(2);
    let out = color(80).render_export(&code);
    // Every line is one run, black on bright white.
    for line in out.lines() {
        assert!(line.starts_with("\x1b[30;107m"), "{line:?}");
        assert!(line.ends_with("\x1b[0m"), "{line:?}");
    }
    let stripped: String = out.replace("\x1b[30;107m", "").replace("\x1b[0m", "");
    let mut drawn = half_blocks(&stripped);
    drawn.pop();
    assert_eq!(drawn, expected(&code, 2));
    let custom = color(80).render_export(&code.colours("blue", "yellow"));
    assert!(custom.starts_with("\x1b[34;43m"), "{custom:?}");
}

#[test]
fn ascii_draws_two_cells_a_module() {
    let code = code().quiet_zone(1);
    let out = plain(80).render_export(&code.clone().charset(Charset::Ascii));
    let drawn: Vec<Vec<bool>> = out
        .lines()
        .map(|line| {
            let cells: Vec<char> = line.chars().collect();
            assert_eq!(cells.len(), 46);
            cells.chunks(2).map(|pair| pair == ['#', '#']).collect()
        })
        .collect();
    let light: Vec<Vec<bool>> = expected(&code, 1)
        .into_iter()
        .map(|row| row.into_iter().map(|dark| !dark).collect())
        .collect();
    assert_eq!(drawn, light);
    // An ASCII console gets the same.
    let ascii = Console::builder()
        .width(80)
        .color_system(None)
        .ascii_only(true)
        .build();
    assert_eq!(ascii.render_export(&code), out);
}

#[test]
fn the_quiet_zone_narrows_to_fit_then_a_note() {
    let code = code();
    let width = |w: usize| {
        plain(w)
            .render_export(&code)
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap()
    };
    assert_eq!(width(80), 29);
    assert_eq!(width(28), 27);
    assert_eq!(width(23), 23);
    assert_eq!(plain(22).render_export(&code), "QR code needs 23 colu…\n");
    let measured = rich::measure::Measurement::get(&plain(80), &plain(80).options(), &code);
    assert_eq!((measured.minimum, measured.maximum), (23, 29));
}

#[test]
fn error_correction_and_versions() {
    let low =
        QrCode::with_error_correction("x".repeat(100).as_str(), ErrorCorrection::Low).unwrap();
    let high =
        QrCode::with_error_correction("x".repeat(100).as_str(), ErrorCorrection::High).unwrap();
    assert!(high.version() > low.version());
    assert_eq!(high.error_correction(), ErrorCorrection::High);
    assert_eq!(usize::from(low.version()) * 4 + 17, low.size());
    let bytes = QrCode::from_bytes(&[0, 159, 146, 150], ErrorCorrection::Medium).unwrap();
    assert!(bytes.accessible_text(80).ends_with("Encodes binary data"));
    let too_long = QrCode::with_error_correction(&"x".repeat(5000), ErrorCorrection::High);
    assert_eq!(too_long.unwrap_err(), QrError::TooLong { bytes: 5000 });
}

#[test]
fn accessible_text_says_what_it_encodes() {
    // Low was asked for; high fits the same 21 modules, so it is used.
    assert_eq!(
        code().accessible_text(80),
        "QR code, version 1, 21 by 21 modules, high error correction\nEncodes: rs-rich"
    );
}
