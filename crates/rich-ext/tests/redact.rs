//! Secret redaction for strings, ANSI streams, segments and exports (0.0.11
//! workstream 10).

use rich::cells::cell_len;
use rich::{ColorSystem, Console, Panel, Segment, Style, Text};
use rich_ext::redact::{is_secret_key, Detector, Redacted, Redactor, SECRET_KEYS};

const GHP: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";
const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJlLWJ5dGVz";

fn color(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build()
}

#[test]
fn key_list_is_shared_with_data_redaction() {
    assert!(SECRET_KEYS.contains(&"password"));
    #[cfg(feature = "data")]
    assert_eq!(rich_ext::data::SECRET_KEYS, SECRET_KEYS);
    assert!(is_secret_key("AWS_SECRET_ACCESS_KEY"));
    assert!(!is_secret_key("authority"));
}

#[test]
fn built_in_detectors() {
    let r = Redactor::secrets();
    let cases = [
        ("password=hunter2", "password=********"),
        (
            "DB_PASSWORD: 'hunter2' user: ann",
            "DB_PASSWORD: '********' user: ann",
        ),
        (
            r#"{"api_key": "abc123", "name": "x"}"#,
            r#"{"api_key": "********", "name": "x"}"#,
        ),
        ("--token=abc123 --verbose", "--token=******** --verbose"),
        (
            "https://x.test/?token=abc&page=2",
            "https://x.test/?token=********&page=2",
        ),
        ("author=ann auth=xyz", "author=ann auth=********"),
        (
            "Authorization: Bearer abc.def-123456",
            "Authorization: Bearer ********",
        ),
        (
            "curl -H 'bearer 0123456789abcdef'",
            "curl -H 'bearer ********'",
        ),
        (&format!("push with {GHP} now"), "push with ******** now"),
        ("key sk-proj-abcdefghijklmnopqrstuv1234", "key ********"),
        ("xoxb-1234567890-abcdef", "********"),
        ("glpat-abcdefghijklmnopqrst12", "********"),
        ("sk_live_abcdefghijklmnop1234", "********"),
        ("id AKIAIOSFODNN7EXAMPLE ok", "id ******** ok"),
        (&format!("jwt {JWT}"), "jwt ********"),
        (
            "postgres://app:s3cret@db:5432/app",
            "postgres://app:********@db:5432/app",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(r.redact_str(input), expected, "{input}");
    }
}

#[test]
fn ordinary_text_is_left_alone() {
    let r = Redactor::secrets();
    for input in [
        "the bearer of bad news",
        "basic authentication is enabled",
        "task-abcdefghijklmnopqrstuvwxyz",
        "AKIA is a prefix",
        "user=ann author=bob",
        "https://example.com/path",
        "eyJ alone",
    ] {
        assert_eq!(r.redact_str(input), input);
    }
}

#[test]
fn find_reports_merged_spans_with_their_kind() {
    let r = Redactor::secrets();
    let line = format!("GITHUB_TOKEN={GHP}");
    let found = r.find(&line);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].start, found[0].end), (13, line.len()));
    assert_eq!(found[0].kind, "key-value");
    let only_aws = Redactor::new().detector(Detector::AwsAccessKey);
    assert!(only_aws.find(&line).is_empty());
    assert_eq!(Detector::ALL.len(), 6);
    assert_eq!(Detector::Jwt.name(), "jwt");
}

#[test]
fn custom_patterns_masks_and_width() {
    let r = Redactor::new()
        .pattern(r"\d{3}-\d{2}-\d{4}")
        .unwrap()
        .named_pattern("card", r"card (?P<secret>\d{4})")
        .unwrap()
        .mask("[{kind}]");
    assert_eq!(
        r.redact_str("ssn 123-45-6789, card 4242"),
        "ssn [pattern], card [card]"
    );
    let err = Redactor::new().pattern("(").unwrap_err();
    assert_eq!(err.pattern, "(");
    assert!(
        err.to_string().starts_with("invalid pattern \"(\""),
        "{err}"
    );

    let fitted = Redactor::secrets().preserve_width(true);
    assert_eq!(fitted.redact_str("token=abc"), "token=***");
    let labelled = Redactor::secrets().mask("[hidden]").preserve_width(true);
    assert_eq!(labelled.redact_str("token=abcdefghij"), "token=[hidden]  ");
    assert_eq!(labelled.redact_str("token=abc"), "token=[hi");
    // Rules match within a line.
    assert_eq!(
        Redactor::secrets().redact_str("token=a\npassword=b\nok"),
        "token=********\npassword=********\nok"
    );
    assert!(Redactor::new().is_empty());
    assert_eq!(Redactor::new().redact_str("token=a"), "token=a");
}

#[test]
fn ansi_text_keeps_its_escapes() {
    let r = Redactor::secrets();
    // The secret is split by a colour change; both halves go, the codes stay.
    let input = "\x1b[1mtoken\x1b[0m=abc\x1b[31m123\x1b[0m done";
    assert_eq!(
        r.redact_ansi(input),
        "\x1b[1mtoken\x1b[0m=********\x1b[31m\x1b[0m done"
    );
    let link = "\x1b]8;;https://x.test\x1b\\password=pw\x1b]8;;\x1b\\";
    assert_eq!(
        r.redact_ansi(link),
        "\x1b]8;;https://x.test\x1b\\password=********\x1b]8;;\x1b\\"
    );
    let fitted = r.clone().preserve_width(true);
    assert_eq!(
        fitted.redact_ansi("\x1b[32mtoken=ab\x1b[0mcd"),
        "\x1b[32mtoken=**\x1b[0m**"
    );
}

#[test]
fn chunks_keep_their_boundaries() {
    let r = Redactor::secrets();
    let chunks = ["line one\npass", "word=hun", "ter2\nbye\n"];
    let out = r.redact_chunks(&chunks);
    assert_eq!(out, ["line one\npass", "word=********", "\nbye\n"]);
    assert_eq!(out.concat(), "line one\npassword=********\nbye\n");
    let clean = ["a", "b"];
    assert_eq!(r.redact_chunks(&clean), ["a", "b"]);
}

#[test]
fn segments_keep_styles_and_line_width() {
    let r = Redactor::secrets();
    let bold = Style::parse("bold").unwrap();
    let red = Style::parse("red").unwrap();
    let segments = vec![
        Segment::new("│ token=ab", Some(bold.clone())),
        Segment::new("cd", Some(red.clone())),
        Segment::new(" │", None),
        Segment::line(),
        Segment::new("│ plain      │", None),
        Segment::line(),
    ];
    let out = r.redact_segments(&segments);
    assert_eq!(
        out,
        vec![
            Segment::new("│ token=**", Some(bold)),
            Segment::new("**", Some(red)),
            Segment::new(" │", None),
            Segment::line(),
            Segment::new("│ plain      │", None),
            Segment::line(),
        ]
    );
    // Nothing to redact: the input comes back unchanged.
    let clean = vec![Segment::new("hello", None), Segment::line()];
    assert_eq!(r.redact_segments(&clean), clean);
    // Control segments survive.
    let with_control = vec![
        Segment::control("\x1b[?25l"),
        Segment::new("secret=x\nnext", None),
    ];
    assert_eq!(
        r.redact_segments(&with_control),
        vec![
            Segment::control("\x1b[?25l"),
            Segment::new("secret=*", None),
            Segment::new("\n", None),
            Segment::new("next", None),
        ]
    );
}

#[test]
fn a_redacted_panel_keeps_its_borders_aligned() {
    let console = Console::builder().width(40).color_system(None).build();
    let redactor = Redactor::secrets().mask("[secret]");
    let out = redactor.export_text(&console, |c| {
        c.print(&Panel::new(Box::new(Text::new(format!(
            "GITHUB_TOKEN={}",
            &GHP[..20]
        )))));
    });
    assert_eq!(
        out,
        [
            "╭──────────────────────────────────────╮",
            "│ GITHUB_TOKEN=[secret]                │",
            "╰──────────────────────────────────────╯",
            "",
        ]
        .join("\n")
    );
    for line in out.lines() {
        assert_eq!(cell_len(line), 40);
    }
}

#[test]
fn export_helpers_redact_between_recording_and_export() {
    let console = color(40);
    let r = Redactor::secrets();
    let print = |c: &Console| {
        c.print(&Text::from_markup("[bold]password[/]=[red]hunter2[/] ok").unwrap());
    };
    assert_eq!(
        r.capture(&console, print),
        "\x1b[1mpassword\x1b[0m=\x1b[31m*******\x1b[0m ok\n"
    );
    assert_eq!(r.export_text(&console, print), "password=******* ok\n");
    let html = r.export_html(&console, print);
    assert!(
        html.contains("*******") && !html.contains("hunter2"),
        "{html}"
    );
    let classes = r.export_html_classes(&console, print);
    assert!(classes.contains("*******") && !classes.contains("hunter2"));
    let svg = r.export_svg(&console, "Redacted", "redacted", print);
    assert!(svg.contains("*******") && !svg.contains("hunter2"));
    // The same bytes as recording, redacting and exporting by hand.
    let by_hand = rich::svg::export_svg(
        &r.redact_segments(&console.record_output(print)),
        &rich::terminal_theme::SVG_EXPORT_THEME,
        "Redacted",
        "redacted",
        40,
    );
    assert_eq!(svg, by_hand);
}

#[test]
fn redacted_renderable() {
    let console = color(80);
    let safe = Redacted::new(Text::new(format!("key {JWT}")), Redactor::secrets());
    assert_eq!(
        console.render_to_string(&safe),
        format!("key {}", "*".repeat(JWT.len()))
    );
}
