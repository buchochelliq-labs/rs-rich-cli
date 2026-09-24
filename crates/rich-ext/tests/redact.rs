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

// Regression tests for the redaction review findings.

#[test]
fn osc_8_link_targets_are_redacted() {
    let r = Redactor::secrets();
    let url = format!("https://u:hunter2pw@h/?token={GHP}");
    let masked = "https://u:********@h/?token=********";
    for (open, close) in [("\x1b\\", "\x1b\\"), ("\x07", "\x07")] {
        let input = format!("\x1b]8;;{url}{open}link\x1b]8;;{close} done");
        assert_eq!(
            r.redact_ansi(&input),
            format!("\x1b]8;;{masked}{open}link\x1b]8;;{close} done")
        );
    }
    // Split across chunks, inside the escape.
    let chunks = [
        "\x1b]8;;https://u:hun".to_string(),
        format!("ter2pw@h/?token={GHP}\x1b\\link\x1b]8;;\x1b\\\n"),
    ];
    let out = r.redact_chunks(&chunks);
    assert_eq!(
        out.concat(),
        format!("\x1b]8;;{masked}\x1b\\link\x1b]8;;\x1b\\\n")
    );
    // Other string sequences (a window title) are searched too.
    assert_eq!(
        r.redact_ansi("\x1b]2;deploy password=hunter2\x07ok"),
        "\x1b]2;deploy password=********\x07ok"
    );
    // A mask never breaks the escape: controls in it become `*`.
    let odd = Redactor::secrets().mask("<\x07\x1b\u{9c}>");
    assert_eq!(
        odd.redact_ansi("\x1b]8;;https://u:pw@h/\x1b\\x\x1b]8;;\x1b\\"),
        "\x1b]8;;https://u:<***>@h/\x1b\\x\x1b]8;;\x1b\\"
    );
}

#[test]
fn segment_link_targets_are_redacted() {
    let console = color(40);
    let r = Redactor::secrets();
    let print = |c: &Console| {
        c.print(&Text::from_markup("[link=https://u:hunter2pw@h/]click[/link]").unwrap());
    };
    let out = r.capture(&console, print);
    assert!(out.contains("https://u:********@h/"), "{out:?}");
    assert!(!out.contains("hunter2pw"), "{out:?}");
    let html = r.export_html(&console, print);
    assert!(!html.contains("hunter2pw"), "{html}");
    let segments = vec![Segment::new(
        "click",
        Some(Style::default().with_link(format!("https://x.test/?token={GHP}"))),
    )];
    let out = r.redact_segments(&segments);
    assert_eq!(out[0].text, "click");
    assert_eq!(
        out[0].style.as_ref().unwrap().link(),
        Some("https://x.test/?token=********")
    );
}

#[test]
fn escapes_with_intermediate_bytes_are_skipped_whole() {
    let r = Redactor::secrets();
    // `tput sgr0` emits `ESC ( B` before the SGR reset.
    assert_eq!(
        r.redact_ansi(&format!("\x1b(B\x1b[m{GHP}")),
        "\x1b(B\x1b[m********"
    );
    assert_eq!(
        r.redact_ansi("API_KEY=\x1b(B\x1b[mabc123"),
        "API_KEY=\x1b(B\x1b[m********"
    );
    // Several intermediates (`ESC $ ( B`) and `ESC # 8`.
    assert_eq!(
        r.redact_ansi("\x1b$(B\x1b#8token=abc"),
        "\x1b$(B\x1b#8token=********"
    );
}

#[test]
fn escape_parsing_follows_ecma_48() {
    let r = Redactor::secrets();
    let cases = [
        // CSI with private parameters and an intermediate byte.
        (
            "\x1b[?25l\x1b[1 qtoken=abc",
            "\x1b[?25l\x1b[1 qtoken=********",
        ),
        // A CSI cut short by a byte it cannot hold ends there; the text
        // after it is visible.
        ("\x1b[\u{2502}token=abc", "\x1b[\u{2502}token=********"),
        ("\x1b[31\x1b[1mtoken=abc", "\x1b[31\x1b[1mtoken=********"),
        // ESC ESC: the first is a lone ESC, the second starts the CSI.
        ("\x1b\x1b[31mtoken=abc", "\x1b\x1b[31mtoken=********"),
        // ESC before a non-ASCII character is a lone ESC.
        ("\x1b\u{2502}token=abc", "\x1b\u{2502}token=********"),
        // Two-byte escapes.
        ("\x1b7token=abc\x1b8", "\x1b7token=********\x1b8"),
        // DCS, SOS, PM and APC strings end at ST; their bodies are searched.
        ("\x1bPq#0\x1b\\token=abc", "\x1bPq#0\x1b\\token=********"),
        (
            "\x1b_Gpassword=pw\x1b\\ok",
            "\x1b_Gpassword=********\x1b\\ok",
        ),
        (
            "\x1bXsos\x1b\\\x1b^pm\x1b\\token=a",
            "\x1bXsos\x1b\\\x1b^pm\x1b\\token=********",
        ),
        // An unterminated OSC runs to the end of the line and is searched.
        ("\x1b]2;token=abc", "\x1b]2;token=********"),
        // A lone ESC at the end of the input.
        ("token=abc\x1b", "token=********\x1b"),
        ("token=abc\x1b[", "token=********\x1b["),
    ];
    for (input, expected) in cases {
        assert_eq!(r.redact_ansi(input), expected, "{input:?}");
    }
}

#[test]
fn user_patterns_fail_closed_on_regex_errors() {
    let r = Redactor::new().pattern(r"(?:a|aa)+(?=b)|hunter2").unwrap();
    let line = format!("{} hunter2", "a".repeat(40));
    let out = r.redact_str(&format!("{line}\nok"));
    assert!(!out.contains("hunter2"), "{out}");
    assert_eq!(out, "********\nok");
    // From where the search failed, not from the start of the line.
    let r = Redactor::new()
        .pattern(r"^x |(?:a|aa)+(?=b)|hunter2")
        .unwrap();
    let found = r.find(&format!("x {line}"));
    let spans: Vec<_> = found.iter().map(|m| (m.start, m.end)).collect();
    assert_eq!(spans, [(0, 2), (2, line.len() + 2)]);
}

#[test]
fn quoted_values_are_masked_to_the_closing_quote() {
    let r = Redactor::secrets();
    let cases = [
        (
            r#"password="correct horse battery" user=ann"#,
            r#"password="********" user=ann"#,
        ),
        (
            r#"{"password": "hunter2, with comma", "user": "ann"}"#,
            r#"{"password": "********", "user": "ann"}"#,
        ),
        (
            r#"{"token": "a\"b c\\", "x": 1}"#,
            r#"{"token": "********", "x": 1}"#,
        ),
        ("password='a b; c' ok", "password='********' ok"),
        // An unclosed quote: the value ends where an unquoted one would.
        (r#"password="abc def"#, r#"password="******** def"#),
        (
            "https://user:p@ss@host/path",
            "https://user:********@host/path",
        ),
        ("redis://:pw@cache:6379", "redis://:********@cache:6379"),
        ("https://host:8080/a@b", "https://host:8080/a@b"),
        (
            "https://u:pw@host/x see me@example.com",
            "https://u:********@host/x see me@example.com",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(r.redact_str(input), expected, "{input}");
    }
    let fitted = Redactor::secrets().preserve_width(true);
    assert_eq!(fitted.redact_str(r#"password="a b""#), r#"password="***""#);
}

#[test]
fn command_lines_mask_secret_flag_values() {
    let r = Redactor::secrets();
    let argv = [
        "deploy",
        "--token",
        "abc123",
        "--password",
        "hunter2",
        "--api-key=xyz",
        "--DB-PASSWORD=pw",
        "-p",
        "8080",
        "-u",
        "ann:pw",
        "--author",
        "ann",
        "https://u:pw@host/x",
        "user:pw@host",
        "--token",
    ];
    assert_eq!(
        r.redact_args(&argv),
        [
            "deploy",
            "--token",
            "********",
            "--password",
            "********",
            "--api-key=********",
            "--DB-PASSWORD=********",
            // One-letter flags are ambiguous (`-p` is a port as often as a
            // password), so they are left alone, as is `user:pw@host`
            // without a scheme.
            "-p",
            "8080",
            "-u",
            "ann:pw",
            "--author",
            "ann",
            "https://u:********@host/x",
            "user:pw@host",
            "--token",
        ]
    );
    // A secret flag as the value of another is masked, and so is its value.
    assert_eq!(
        r.redact_args(&["--token", "--password", "x"]),
        ["--token", "********", "********"]
    );
    // Masks keep the width when asked; flags need the KeyValue detector.
    let fitted = Redactor::secrets().preserve_width(true);
    assert_eq!(fitted.redact_args(&["--token", "abc"]), ["--token", "***"]);
    let only_jwt = Redactor::new().detector(Detector::Jwt);
    assert_eq!(
        only_jwt.redact_args(&["--token", "abc"]),
        ["--token", "abc"]
    );
}

#[test]
fn byte_chunks_keep_split_characters_and_invalid_bytes() {
    let r = Redactor::secrets();
    let text = format!("x{}\n", "é".repeat(10_000));
    let chunks: Vec<&[u8]> = text.as_bytes().chunks(8192).collect();
    // Nothing matched: every chunk comes back byte for byte.
    assert_eq!(r.redact_byte_chunks(&chunks), chunks);
    // A secret after split characters and invalid bytes: only it changes.
    let mut bytes = "é".repeat(5).into_bytes();
    bytes.extend(b"\xff\xfe token=abc \xc3");
    let chunks: Vec<&[u8]> = bytes.chunks(3).collect();
    let out = r.redact_byte_chunks(&chunks);
    assert_eq!(out.len(), chunks.len());
    let mut expected = "é".repeat(5).into_bytes();
    expected.extend(b"\xff\xfe token=******** \xc3");
    assert_eq!(out.concat(), expected);
    // A secret split between chunks is masked in the chunk where it starts.
    let out = r.redact_byte_chunks(&[&b"pass"[..], b"word=hun", b"ter2\n"]);
    assert_eq!(out, [&b"pass"[..], b"word=********", b"\n"]);
}

#[test]
fn pathological_lines_redact_in_linear_time() {
    use std::time::{Duration, Instant};
    let r = Redactor::secrets();
    let fitted = r.clone().preserve_width(true);
    let lines = [
        "a:".repeat(100_000),
        "a-".repeat(100_000),
        "a=".repeat(100_000),
        "a.".repeat(100_000),
        "abcd: ".repeat(33_000),
        "a://".repeat(50_000),
        "a://b:".repeat(33_000),
        "a:b@".repeat(50_000),
        "x=\"".repeat(66_000),
        "bearer ".repeat(28_000),
        "token=".repeat(33_000),
        format!("password={}", "x".repeat(200_000)),
        format!("password=\"{}", "x ".repeat(100_000)),
    ];
    // Each line is 200 KB. Quadratic detectors took minutes on these; a
    // debug build now takes about a second. The limit is generous for CI.
    for line in &lines {
        let redactions: [&dyn Fn(); 2] = [&|| drop(r.redact_str(line)), &|| {
            drop(fitted.redact_ansi(line))
        }];
        for redact in redactions {
            let started = Instant::now();
            redact();
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_secs(5),
                "{elapsed:?} for {:?}…",
                &line[..20]
            );
        }
    }
}
