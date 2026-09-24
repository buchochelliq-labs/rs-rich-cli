//! Tests for the hex, Unicode and environment inspectors.

use std::path::Path;

use rich::cells::cell_len;
use rich::{ColorSystem, Console, Renderable};
use rich_ext::env_inspect::{
    is_path_like, is_secret_name, name_matches, EnvView, PathKind, PathStatus, PathView,
};
use rich_ext::hex::{find_all, parse_needle, ByteClass, HexView};
use rich_ext::unicode_inspect::{classify, control_picture, Kind, UnicodeView};

fn render(r: &dyn Renderable, width: usize, ascii: bool) -> String {
    let c = Console::builder()
        .width(width)
        .color_system(None)
        .ascii_only(ascii)
        .build();
    c.segments_to_string(&r.rich_render(&c, &c.options()))
}

fn lines(r: &dyn Renderable, width: usize) -> Vec<String> {
    render(r, width, false).lines().map(str::to_owned).collect()
}

fn ascii_lines(r: &dyn Renderable, width: usize) -> Vec<String> {
    render(r, width, true).lines().map(str::to_owned).collect()
}

fn colored(r: &dyn Renderable, width: usize) -> String {
    let c = Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    c.segments_to_string(&r.rich_render(&c, &c.options()))
}

/// Every line fits `width`, in Unicode and ASCII mode alike.
fn assert_fits(r: &dyn Renderable, width: usize) {
    for ascii in [false, true] {
        let out = render(r, width, ascii);
        for line in out.lines() {
            assert!(
                cell_len(line) <= width,
                "line wider than {width} (ascii={ascii}): {line:?}\n{out}"
            );
            if ascii {
                assert!(line.is_ascii(), "non-ASCII in ASCII mode: {line:?}");
            }
        }
    }
}

// ---------------------------------------------------------------- hex

#[test]
fn hex_lines_match_hexdump_c() {
    let view = HexView::new(b"Hello, World!\n\x00\x01\xff".to_vec());
    assert_eq!(
        lines(&view, 80),
        [
            "00000000  48 65 6c 6c 6f 2c 20 57  6f 72 6c 64 21 0a 00 01  │Hello, World!...│",
            "00000010  ff                                                │.│",
            "00000011",
        ]
    );
    assert_eq!(
        ascii_lines(&view, 80),
        [
            "00000000  48 65 6c 6c 6f 2c 20 57  6f 72 6c 64 21 0a 00 01  |Hello, World!...|",
            "00000010  ff                                                |.|",
            "00000011",
        ]
    );
    assert_eq!(
        lines(&view.clone().ascii_panel(false), 80),
        [
            "00000000  48 65 6c 6c 6f 2c 20 57  6f 72 6c 64 21 0a 00 01",
            "00000010  ff",
            "00000011",
        ]
    );
    assert_eq!(lines(&HexView::new(Vec::new()), 80), ["00000000"]);
}

#[test]
fn hex_offsets_past_32_bits_widen_the_column() {
    let bytes = vec![0x41; 16];
    assert_eq!(HexView::new(bytes.clone()).offset_digits(), 8);
    assert_eq!(
        HexView::new(bytes.clone())
            .offset(0xFFFF_FFF0 - 16)
            .offset_digits(),
        8
    );
    let view = HexView::new(bytes).offset(0xFFFF_FFF8);
    assert_eq!(view.offset_digits(), 9);
    assert_eq!(
        lines(&view, 80),
        [
            "0fffffff8  41 41 41 41 41 41 41 41  41 41 41 41 41 41 41 41  │AAAAAAAAAAAAAAAA│",
            "100000008",
        ]
    );
    let far = HexView::new(vec![0]).offset(0x12_3456_789A_BCDE);
    assert_eq!(far.offset_digits(), 14);
    // Too wide for 16 bytes a line at 80 columns: 8, panel still aligned.
    assert_eq!(far.resolved_bytes_per_line(80), 8);
    assert_eq!(
        lines(&far, 80)[0],
        format!("123456789abcde  00{}│.│", " ".repeat(23))
    );
}

#[test]
fn hex_fits_the_width() {
    let data: Vec<u8> = (0..=255).collect();
    let view = HexView::new(data.clone());
    assert_eq!(view.line_width(16), 78);
    assert_eq!(view.resolved_bytes_per_line(200), 16);
    assert_eq!(view.resolved_bytes_per_line(80), 16);
    assert_eq!(view.resolved_bytes_per_line(77), 8);
    assert_eq!(view.resolved_bytes_per_line(45), 8);
    // Nothing between 8 and 32 fits: fewer bytes, never overflow.
    assert_eq!(view.resolved_bytes_per_line(40), 6);
    assert_eq!(
        HexView::new(data.clone())
            .ascii_panel(false)
            .resolved_bytes_per_line(40),
        8
    );
    assert_eq!(
        HexView::new(data.clone())
            .group(4)
            .resolved_bytes_per_line(70),
        12
    );
    // A group above 16 goes up to the smallest multiple that fits.
    assert_eq!(
        HexView::new(data.clone())
            .group(32)
            .resolved_bytes_per_line(200),
        32
    );
    // An explicit count is used as given.
    assert_eq!(
        HexView::new(data.clone())
            .bytes_per_line(Some(32))
            .resolved_bytes_per_line(40),
        32
    );
    assert_eq!(
        lines(&view, 40)[..2],
        [
            "00000000  00 01 02 03 04 05  │......│",
            "00000006  06 07 08 09 0a 0b  │......│",
        ]
    );
    for width in [20, 40, 60, 79, 80, 120] {
        assert_fits(&view, width);
        assert_fits(&view.clone().offset(u64::MAX - 300), width);
        assert_fits(&view.clone().bytes_per_line(Some(32)), width);
    }
    let rich::measure::Measurement { minimum, maximum } = view.measure(
        &Console::builder().width(120).build(),
        &Console::builder().width(120).build().options(),
    );
    assert_eq!((minimum, maximum), (view.line_width(1), 78));
}

#[test]
fn hex_collapses_identical_lines() {
    let mut data = vec![0u8; 64];
    data.push(1);
    let view = HexView::new(data);
    assert_eq!(
        lines(&view, 80),
        [
            "00000000  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  │................│",
            "*",
            "00000040  01                                                │.│",
            "00000041",
        ]
    );
    let all = lines(&view.clone().collapse(false), 80);
    assert_eq!(all.len(), 6);
    assert!(!all.contains(&"*".to_string()));
    // A partial last line equal in prefix is never collapsed.
    let partial = lines(&HexView::new(vec![7u8; 24]), 80);
    assert_eq!(partial.len(), 3);
    assert!(partial[1].starts_with("00000010  07 07"));
}

#[test]
fn hex_highlights_needles() {
    let view = HexView::new(b"xxPNGxx".to_vec()).highlight(b"PNG");
    let out = colored(&view, 80);
    for byte in ["50", "4e", "47"] {
        assert!(
            out.contains(&format!("\x1b[7m{byte}\x1b[0m")),
            "{byte} not reversed: {out:?}"
        );
    }
    assert!(!out.contains("\x1b[7m78"), "{out:?}");
    assert!(
        out.contains("\x1b[7mP\x1b[0m"),
        "panel not highlighted: {out:?}"
    );
    // Byte classes carry their styles.
    let classes = colored(&HexView::new(vec![0x00, 0x20, 0x01, 0x80, 0x41]), 80);
    assert!(classes.contains("\x1b[2m00\x1b[0m"), "{classes:?}");
    assert!(classes.contains("\x1b[32m20\x1b[0m"), "{classes:?}");
    assert!(classes.contains("\x1b[33m01\x1b[0m"), "{classes:?}");
    assert!(classes.contains("\x1b[35m80\x1b[0m"), "{classes:?}");
    // A line holding a match is shown even inside a run of repeats.
    let mut data = vec![0u8; 48];
    data[20] = 0;
    let repeated = HexView::new(data).highlight(&[0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(lines(&repeated, 80).len(), 4);
    assert_eq!(ByteClass::of(b'\n'), ByteClass::Whitespace);
    assert_eq!(ByteClass::of(0x7f), ByteClass::Control);
}

#[test]
fn find_all_reports_overlapping_matches() {
    assert_eq!(find_all(b"aaaa", b"aa"), [0, 1, 2]);
    assert_eq!(find_all(b"abcabc", b"bc"), [1, 4]);
    assert!(find_all(b"abc", b"").is_empty());
    assert!(find_all(b"ab", b"abc").is_empty());
}

#[test]
fn parse_needle_accepts_every_form() {
    let dead = vec![0xde, 0xad, 0xbe, 0xef];
    assert_eq!(parse_needle("de ad be ef").unwrap(), dead);
    assert_eq!(parse_needle("deadbeef").unwrap(), dead);
    assert_eq!(parse_needle("DEADBEEF").unwrap(), dead);
    assert_eq!(parse_needle("0xDEAD 0xbeef").unwrap(), dead);
    assert_eq!(parse_needle("de:ad,be:ef").unwrap(), dead);
    assert_eq!(parse_needle("0xDEAD").unwrap(), [0xde, 0xad]);
    assert_eq!(parse_needle("  0Xde  ").unwrap(), [0xde]);
    assert_eq!(parse_needle("\"PNG\"").unwrap(), b"PNG");
    assert_eq!(parse_needle("'PNG'").unwrap(), b"PNG");
    assert_eq!(parse_needle("\"a b\"").unwrap(), b"a b");
    assert_eq!(parse_needle(r#""\x89PNG\r\n""#).unwrap(), b"\x89PNG\r\n");
    assert_eq!(parse_needle(r#""\"\\""#).unwrap(), b"\"\\");
    assert_eq!(parse_needle("\"é\"").unwrap(), "é".as_bytes());
    for bad in [
        "", "  ", "\"\"", "0x", "abc", "0xABC", "zz", "PNG", r#""\q""#, r#""\x4""#,
    ] {
        assert!(parse_needle(bad).is_err(), "{bad:?} parsed");
    }
    assert!(parse_needle("PNG").unwrap_err().contains("not a hex digit"));
}

// ---------------------------------------------------------------- unicode

#[test]
fn combining_mark_is_one_grapheme_of_width_one() {
    let view = UnicodeView::from_str("e\u{301}");
    let c = &view.clusters()[0];
    assert_eq!(view.clusters().len(), 1);
    assert_eq!(c.width, 1);
    assert_eq!(c.kind, Kind::Combining);
    assert_eq!(c.code_points(), "U+0065 U+0301");
    assert_eq!(c.hex(), "65 cc 81");
    assert_eq!(c.escape(), "e\\u{301}");
    assert_eq!(c.display(false), "e\u{301}");
    assert_eq!(c.display(true), ".");
    let s = view.summary();
    assert_eq!(
        (s.bytes, s.code_points, s.graphemes, s.cells, s.invalid),
        (3, 2, 1, 1, 0)
    );
}

#[test]
fn family_emoji_is_one_grapheme_of_width_two() {
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    let view = UnicodeView::from_str(family);
    assert_eq!(view.clusters().len(), 1);
    let c = &view.clusters()[0];
    assert_eq!(c.width, 2);
    assert_eq!(c.kind, Kind::Emoji);
    assert_eq!(c.code_points(), "U+1F468 U+200D U+1F469 U+200D U+1F467");
    assert_eq!(c.bytes.len(), 18);
    assert_eq!(
        view.summary().to_string(),
        "18 bytes, 5 code points, 1 grapheme, 2 cells, 0 invalid sequences"
    );
}

#[test]
fn invalid_bytes_get_their_own_row() {
    let view = UnicodeView::from_bytes(&[0x66, 0xff, 0x6f]);
    let rows: Vec<(usize, Kind, String)> = view
        .clusters()
        .iter()
        .map(|c| (c.offset, c.kind, c.hex()))
        .collect();
    assert_eq!(
        rows,
        [
            (0, Kind::Ascii, "66".into()),
            (1, Kind::Invalid, "ff".into()),
            (2, Kind::Ascii, "6f".into()),
        ]
    );
    assert_eq!(view.clusters()[1].escape(), "\\xff");
    assert_eq!(view.summary().invalid, 1);
    assert_eq!(view.summary().graphemes, 2);
    assert_eq!(
        lines(&view, 80),
        [
            "┏━━━━━━━━┳━━━━━━┳━━━━━━━━━━━━━┳━━━━━━━┳━━━━━━━┳━━━━━━━━┳━━━━━━━━━┓",
            "┃ Offset ┃ Char ┃ Code points ┃ UTF-8 ┃ Width ┃ Escape ┃ Kind    ┃",
            "┡━━━━━━━━╇━━━━━━╇━━━━━━━━━━━━━╇━━━━━━━╇━━━━━━━╇━━━━━━━━╇━━━━━━━━━┩",
            "│      0 │ f    │ U+0066      │ 66    │     1 │ f      │ ascii   │",
            "│      1 │ \u{fffd}    │ -           │ ff    │     - │ \\xff   │ invalid │",
            "│      2 │ o    │ U+006F      │ 6f    │     1 │ o      │ ascii   │",
            "└────────┴──────┴─────────────┴───────┴───────┴────────┴─────────┘",
            "3 bytes, 2 code points, 2 graphemes, 2 cells, 1 invalid sequence",
        ]
    );
    // The error row is styled as an error.
    assert!(colored(&view, 80).contains("\x1b[1;31minvalid"));
}

#[test]
fn invalid_sequences_split_as_from_utf8_lossy_does() {
    for input in [
        &b"\xe2\x82"[..],
        b"\xe2\x41",
        b"a\xf0\x9f\x98",
        b"\xc0\xaf",
        b"\xed\xa0\x80x",
        b"ok\xff\xfe\xfd",
    ] {
        let view = UnicodeView::from_bytes(input);
        let lossy = String::from_utf8_lossy(input);
        assert_eq!(
            view.summary().invalid,
            lossy.matches('\u{fffd}').count(),
            "{input:?}"
        );
        let covered: usize = view.clusters().iter().map(|c| c.bytes.len()).sum();
        assert_eq!(covered, input.len());
    }
    let truncated = UnicodeView::from_bytes(b"\xe2\x82");
    assert_eq!(truncated.clusters().len(), 1);
    assert_eq!(truncated.clusters()[0].hex(), "e2 82");
}

#[test]
fn control_characters_are_rows_of_their_own() {
    let view = UnicodeView::from_str("a\r\nb\u{7f}\u{1b}");
    let shown: Vec<(String, String, Kind)> = view
        .clusters()
        .iter()
        .map(|c| (c.display(false), c.display(true), c.kind))
        .collect();
    assert_eq!(
        shown,
        [
            ("a".into(), "a".into(), Kind::Ascii),
            ("␍".into(), "^M".into(), Kind::Control),
            ("␊".into(), "^J".into(), Kind::Control),
            ("b".into(), "b".into(), Kind::Ascii),
            ("␡".into(), "^?".into(), Kind::Control),
            ("␛".into(), "^[".into(), Kind::Control),
        ]
    );
    assert_eq!(view.clusters()[2].escape(), "\\u{a}");
    assert_eq!(control_picture('\u{9b}', false).as_deref(), Some("\\u{9b}"));
    assert_eq!(control_picture('x', false), None);
    // No raw control reaches the terminal.
    let out = render(&view, 80, false);
    assert!(!out.contains('\r') && !out.contains('\u{1b}') && !out.contains('\u{7f}'));
}

#[test]
fn classifier_labels() {
    let kind = |s: &str| classify(s, cell_len(s));
    assert_eq!(kind("a"), Kind::Ascii);
    assert_eq!(kind(" "), Kind::Whitespace);
    assert_eq!(kind("\u{a0}"), Kind::Whitespace);
    assert_eq!(kind("\t"), Kind::Control);
    assert_eq!(kind("\u{200b}"), Kind::ZeroWidth);
    assert_eq!(kind("\u{feff}"), Kind::ZeroWidth);
    assert_eq!(kind("\u{fe0e}"), Kind::VariationSelector);
    assert_eq!(kind("\u{2764}\u{fe0f}"), Kind::Emoji);
    assert_eq!(kind("\u{1f600}"), Kind::Emoji);
    assert_eq!(kind("\u{3042}"), Kind::Wide);
    assert_eq!(kind("é"), Kind::Other);
    assert_eq!(kind("\u{301}"), Kind::Combining);
    assert_eq!(Kind::VariationSelector.to_string(), "variation selector");
    // A lone combining mark sits on a dotted circle.
    let lone = UnicodeView::from_str("\u{301}");
    assert_eq!(lone.clusters()[0].display(false), "◌\u{301}");
}

#[test]
fn unicode_view_renders_compactly_when_narrow() {
    let view = UnicodeView::from_str("e\u{301}\t");
    assert_eq!(
        lines(&view, 40),
        [
            "┏━━━━━━━━┳━━━━━━┳━━━━━━━━━━━━━━━━━━━┓",
            "┃ Offset ┃ Char ┃ Details           ┃",
            "┡━━━━━━━━╇━━━━━━╇━━━━━━━━━━━━━━━━━━━┩",
            "│      0 │ e\u{301}    │ combining, 1 cell │",
            "│        │      │ U+0065 U+0301     │",
            "│        │      │ 65 cc 81          │",
            "│        │      │ e\\u{301}          │",
            "│      3 │ ␉    │ control, 0 cells  │",
            "│        │      │ U+0009            │",
            "│        │      │ 09                │",
            "│        │      │ \\u{9}             │",
            "└────────┴──────┴───────────────────┘",
            "4 bytes, 3 code points, 2 graphemes, 1 ",
            "cell, 0 invalid sequences",
        ]
    );
    assert_eq!(
        ascii_lines(&view, 40)[3..5],
        [
            "|      0 | .    | combining, 1 cell |",
            "|        |      | U+0065 U+0301     |",
        ]
    );
    let family = "a\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{3042}\u{200b} x\u{1b}";
    for width in [30, 40, 60, 79, 80, 120] {
        assert_fits(&UnicodeView::from_str(family), width);
        assert_fits(&UnicodeView::from_bytes(b"\xff\xfe abc \xe2\x82"), width);
    }
}

#[test]
fn unicode_limit_says_how_many_more() {
    let view = UnicodeView::from_str("abcdef").limit(2);
    let out = lines(&view, 80);
    assert_eq!(out.len(), 3 + 2 + 1 + 2);
    assert_eq!(out[6], "… 4 more");
    assert_eq!(
        out[7],
        "6 bytes, 6 code points, 6 graphemes, 6 cells, 0 invalid sequences"
    );
    assert_eq!(ascii_lines(&view, 80)[6], "... 4 more");
    assert_eq!(view.clusters().len(), 6);
}

// ---------------------------------------------------------------- env

fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn secret_names_are_detected() {
    for secret in [
        "GITHUB_TOKEN",
        "db_password",
        "MYSQL_PASSWD",
        "AWS_SECRET_ACCESS_KEY",
        "OPENAI_API_KEY",
        "StripeApiKey",
        "SSH_PRIVATE_KEY",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "AUTH",
        "GH_AUTH",
        "AUTH_HEADER",
        "npm-auth-x",
        "HTTP_AUTHORIZATION",
    ] {
        assert!(is_secret_name(secret), "{secret} not secret");
    }
    for plain in [
        "AUTHOR",
        "GIT_AUTHOR_NAME",
        "OAUTHLIB_DEBUG",
        "PATH",
        "HOME",
        "TOKENIZERS",
    ] {
        let expected = plain == "TOKENIZERS"; // a substring rule: over-masking is the safe side
        assert_eq!(is_secret_name(plain), expected, "{plain}");
    }
}

#[test]
fn env_view_redacts_secret_values() {
    let env = EnvView::new(vars(&[
        ("GITHUB_TOKEN", "ghp_0123456789"),
        ("AUTHOR", "Ann"),
        ("EDITOR", "vim"),
    ]));
    assert_eq!(
        lines(&env, 40),
        [
            "┏━━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━┓",
            "┃ Name         ┃ Value             ┃",
            "┡━━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━┩",
            "│ AUTHOR       │ Ann               │",
            "│ EDITOR       │ vim               │",
            "│ GITHUB_TOKEN │ •••••• (14 chars) │",
            "└──────────────┴───────────────────┘",
        ]
    );
    assert_eq!(
        ascii_lines(&env, 40)[5],
        "| GITHUB_TOKEN | ****** (14 chars) |"
    );
    let shown = render(&env.clone().redact(false), 40, false);
    assert!(shown.contains("ghp_0123456789"));
    assert!(!render(&env, 40, false).contains("ghp_"));
}

#[test]
fn env_view_filters_names() {
    let env = EnvView::new(vars(&[
        ("PATH", "/bin"),
        ("MANPATH", "/man"),
        ("HOME", "/home/a"),
        ("GH_TOKEN", "x"),
        ("NPM_TOKEN", "y"),
    ]));
    let names = |view: &EnvView| -> Vec<String> {
        view.vars().iter().map(|(k, _)| k.to_string()).collect()
    };
    assert_eq!(
        names(&env),
        ["GH_TOKEN", "HOME", "MANPATH", "NPM_TOKEN", "PATH"]
    );
    assert_eq!(names(&env.clone().filter("path")), ["MANPATH", "PATH"]);
    assert_eq!(
        names(&env.clone().filter("*_token")),
        ["GH_TOKEN", "NPM_TOKEN"]
    );
    assert_eq!(names(&env.clone().filter("g?_*")), ["GH_TOKEN"]);
    assert_eq!(names(&env.clone().filter("PATH*")), ["PATH"]);
    let none = env.clone().filter("nothing");
    assert!(none.is_empty());
    assert_eq!(lines(&none, 40), ["no matching environment variables"]);
    assert!(name_matches("*", "anything"));
    assert!(!name_matches("a*b", "acbx"));
    assert!(name_matches("a*b*c", "aXbYbZc"));
}

#[test]
fn env_view_splits_path_like_values() {
    assert!(is_path_like("PATH", "/bin", ':'));
    assert!(is_path_like("LD_LIBRARY_PATH", "", ':'));
    assert!(is_path_like("DIRS", "/a:/b", ':'));
    assert!(!is_path_like("URL", "http://host:80/x", ':'));
    assert!(!is_path_like("DISPLAY", ":0", ':'));
    assert!(!is_path_like("LS_COLORS", "di=01:ln=01", ':'));
    let env = EnvView::new(vars(&[
        (
            "PATH",
            "/usr/local/bin:/usr/bin:/a/really/long/directory/name/bin",
        ),
        ("TERM", "xterm\u{1b}[2J"),
    ]))
    .separator(':');
    assert_eq!(
        lines(&env, 40),
        [
            "┏━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓",
            "┃ Name ┃ Value                         ┃",
            "┡━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩",
            "│ PATH │ /usr/local/bin                │",
            "│      │ /usr/bin                      │",
            "│      │ /a/really/long/directory/name │",
            "│      │ /bin                          │",
            "│ TERM │ xterm␛[2J                     │",
            "└──────┴───────────────────────────────┘",
        ]
    );
    assert_eq!(
        ascii_lines(&env, 40)[7],
        "| TERM | xterm^[[2J                    |"
    );
    for width in [20, 40, 80] {
        assert_fits(&env, width);
    }
}

fn fake(path: &Path) -> PathKind {
    match path.to_str().unwrap() {
        "/bin" | "/usr/bin" | "C:\\Tools" => PathKind::Directory,
        "/etc/passwd" => PathKind::NotADirectory,
        _ => PathKind::Missing,
    }
}

#[test]
fn path_view_reports_every_status() {
    let view = PathView::new(
        "PATH",
        "/bin::/nope:/usr/bin:/bin/:/etc/passwd:/usr/bin",
        ':',
    )
    .case_insensitive(false)
    .probe(fake);
    let statuses: Vec<PathStatus> = view.entries().iter().map(|e| e.status).collect();
    assert_eq!(
        statuses,
        [
            PathStatus::Ok,
            PathStatus::Empty,
            PathStatus::Missing,
            PathStatus::Ok,
            PathStatus::Duplicate(1),
            PathStatus::NotADirectory,
            PathStatus::Duplicate(4),
        ]
    );
    assert_eq!(
        lines(&view, 40),
        [
            "┏━━━┳━━━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━┓",
            "┃ # ┃ PATH        ┃ Status          ┃",
            "┡━━━╇━━━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━┩",
            "│ 1 │ /bin        │ ok              │",
            "│ 2 │             │ empty           │",
            "│ 3 │ /nope       │ missing         │",
            "│ 4 │ /usr/bin    │ ok              │",
            "│ 5 │ /bin/       │ duplicate of #1 │",
            "│ 6 │ /etc/passwd │ not a directory │",
            "│ 7 │ /usr/bin    │ duplicate of #4 │",
            "└───┴─────────────┴─────────────────┘",
        ]
    );
    assert_eq!(
        ascii_lines(&view, 40)[5],
        "| 3 | /nope       | missing         |"
    );
    let out = colored(&view, 40);
    assert!(out.contains("\x1b[32mok"), "{out:?}");
    assert!(out.contains("\x1b[33mempty"), "{out:?}");
    assert!(out.contains("\x1b[1;31mmissing"), "{out:?}");
    for width in [20, 30, 40] {
        assert_fits(&view, width);
    }
}

#[test]
fn path_duplicates_ignore_case_only_when_asked() {
    let value = "C:\\Tools;c:\\tools\\;D:\\x";
    let sensitive = PathView::new("Path", value, ';')
        .case_insensitive(false)
        .probe(fake);
    let insensitive = PathView::new("Path", value, ';')
        .case_insensitive(true)
        .probe(fake);
    assert_eq!(sensitive.entries()[1].status, PathStatus::Missing);
    assert_eq!(insensitive.entries()[1].status, PathStatus::Duplicate(1));
    assert_eq!(insensitive.entries()[2].status, PathStatus::Missing);
    assert!(PathView::new("P", "", ':').entries().is_empty());
    assert_eq!(PathStatus::Duplicate(3).label(), "duplicate of #3");
    assert!(!PathStatus::Ok.is_problem());
}

#[test]
fn process_helpers_read_the_environment() {
    let env = EnvView::from_process();
    assert!(!env.is_empty());
    if let Some(path) = PathView::from_process("PATH") {
        assert_eq!(path.name(), "PATH");
        let _ = path.entries();
    }
    assert!(PathView::from_process("RS_RICH_EXT_SURELY_UNSET_VARIABLE").is_none());
}

#[test]
fn unicode_view_uses_the_full_table_when_it_fits() {
    let family = "a\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{3042}\u{200b} x\u{1b}";
    let view = UnicodeView::from_str(family);
    assert_eq!(
        lines(&view, 80)[1],
        "┃ Offset ┃ Char ┃ Code points ┃ UTF-8       ┃ Width ┃ Escape      ┃ Kind       ┃"
    );
    assert!(lines(&view, 79)[1].starts_with("┃ Offset ┃ Char ┃ Details "));
}
