//! Regressions for untrusted input reaching the inspectors, diff views and
//! test reports: decoded escapes, lying numbers, huge settings and inputs
//! that used to be quadratic.
use std::time::{Duration, Instant};

use rich::{ColorSystem, Console, Renderable, Segment};
use rich_ext::capabilities::{Capabilities, CapabilityReport, ColorDepth, MapEnvironment};
use rich_ext::diff::git::{parse_unified, LinkProvider, PatchHunk, PatchView, TemplateLinks};
use rich_ext::env_inspect::{is_secret_name, EnvView};
use rich_ext::hex::HexView;
use rich_ext::source_view::SourceView;
use rich_ext::unicode_inspect::{Kind, UnicodeView};

/// Every segment a renderable produces, in colour (so styles are escapes the
/// console writes, and only the segment text is data).
fn segments(r: &dyn Renderable, width: usize) -> Vec<Segment> {
    let c = Console::builder()
        .width(width)
        .height(500)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    r.rich_render(&c, &c.options())
}

fn plain(r: &dyn Renderable, width: usize) -> String {
    let c = Console::builder()
        .width(width)
        .height(500)
        .no_color(true)
        .build();
    c.segments_to_string(&r.rich_render(&c, &c.options()))
}

fn is_bidi(c: char) -> bool {
    matches!(c as u32, 0x061c | 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069)
}

/// No segment text holds a terminal control (other than a line break) or a
/// bidi control, and no link target holds a control.
fn assert_inert(segments: &[Segment]) {
    for s in segments {
        if s.control {
            continue;
        }
        for c in s.text.chars() {
            assert!(
                c == '\n' || !(c.is_control() || is_bidi(c)),
                "control {c:?} in segment {:?}",
                s.text
            );
        }
        if let Some(link) = s.style.as_ref().and_then(|st| st.link()) {
            assert!(
                !link.chars().any(|c| c.is_control()),
                "control in link {link:?}"
            );
        }
    }
}

// ---- X1: decoded git paths -------------------------------------------------

const EVIL_PATCH: &str = "diff --git \"a/\\033[2J\\033[Hx\" \"b/\\033[2J\\033[Hx\"
index 1111111..2222222 100644
--- \"a/\\033[2J\\033[Hx\"
+++ \"b/\\033[2J\\033[Hx\"
@@ -1 +1 @@ fn \\033]8;;x
-a
+b
diff --git a/old b/new
similarity index 90%
rename from \"\\033]0;title\\007old\"
rename to \"new\\302\\233\"
";

#[test]
fn patch_view_neutralises_decoded_path_controls() {
    let patch = parse_unified(EVIL_PATCH).unwrap();
    // The data keeps what git encoded.
    assert_eq!(patch.files[0].new_path.as_deref(), Some("\x1b[2J\x1b[Hx"));
    assert_eq!(
        patch.files[1].old_path.as_deref(),
        Some("\x1b]0;title\x07old")
    );
    let view = PatchView::new(patch)
        .links(TemplateLinks::new("https://x/{path}#L{line}").file_template("https://x/{path}"));
    assert_inert(&segments(&view, 100));
    let out = plain(&view, 100);
    assert!(!out.contains('\x1b'), "{out}");
    assert!(out.contains("␛[2J␛[Hx"), "{out}");
}

#[test]
fn template_links_percent_encode_controls_and_delimiters() {
    let links = TemplateLinks::new("https://x/{path}#L{line}");
    let url = links
        .line_url("a\x1b]8;;https://evil\x07b c\u{9c}d\u{7f}\"<>`{}|\\^", 3)
        .unwrap();
    assert!(!url.chars().any(|c| c.is_control()), "{url}");
    assert_eq!(
        url,
        "https://x/a%1B%5D8;;https://evil%07b%20c%C2%9Cd%7F%22%3C%3E%60%7B%7D%7C%5C%5E#L3"
    );
    // Ordinary paths stay readable.
    assert_eq!(
        links.line_url("src/a-b_c.~d/ü.rs", 1).unwrap(),
        "https://x/src/a-b_c.~d/%C3%BC.rs#L1"
    );
}

// ---- X5: lying hunk headers --------------------------------------------------

#[test]
fn hunk_headers_with_impossible_numbers_are_errors() {
    for header in [
        "@@ -0,1 +0,1 @@",
        "@@ -1,1 +0,1 @@",
        "@@ -18446744073709551615 +1 @@",
        "@@ -5,18446744073709551615 +1 @@",
        "@@ -1 +18446744073709551615,2 @@",
    ] {
        let input = format!("--- a/x\n+++ b/x\n{header}\n x\n");
        let err = parse_unified(&input).expect_err(header);
        assert_eq!(err.line, 3, "{header}: {err}");
    }
    // Empty ranges at line 0 are what git writes for new and deleted files.
    let ok = parse_unified("--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+x\n").unwrap();
    assert_eq!(ok.files[0].hunks[0].header(), "@@ -0,0 +1 @@");
    // A hand-built hunk never panics while formatting.
    let hunk = PatchHunk {
        old_start: 0,
        old_len: 1,
        new_start: usize::MAX,
        new_len: usize::MAX,
        section: String::new(),
        lines: Vec::new(),
    };
    let _ = hunk.header();
}

// ---- X2 / L9: env secrets ------------------------------------------------------

#[test]
fn env_secret_names_match_whole_segments() {
    for name in [
        "STRIPE_KEY",
        "DB_PASS",
        "MYSQL_PWD",
        "PRIVATEKEY",
        "SSH_PRIVATEKEY",
        "SIGNING_KEY",
        "ENCRYPTION_KEY",
        "SESSION_KEY",
        "TLS_KEY",
        "CLIENT_KEY",
        "MASTER_KEY",
        "PASSPHRASE",
        "GPG_PASSPHRASE",
        "API-KEY",
        "JWT",
        "SESSION_COOKIE",
        "SENTRY_DSN",
        "FLASK_SESSION",
        "app-key",
    ] {
        assert!(is_secret_name(name), "{name} should be masked");
    }
    for name in [
        "AUTHOR",
        "KEYBOARD_LAYOUT",
        "MONKEY",
        "PATH",
        "PWD",
        "OLDPWD",
        "XDG_SESSION_TYPE",
        "PASSENGER_APP_ENV",
        "COMPASS",
    ] {
        assert!(!is_secret_name(name), "{name} should be shown");
    }
}

#[test]
fn env_view_masks_secret_values_whatever_the_name() {
    let env = EnvView::new(
        [
            ("DATABASE_URL", "postgres://admin:hunter2@db/x"),
            ("HTTPS_PROXY", "http://u:pw@p:1"),
            ("SOME_SETTING", "sk_live_0123456789abcdefghij"),
            (
                "UPSTREAM",
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJl",
            ),
            ("EDITOR", "vim"),
            ("PATH", "/bin:/usr/bin"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    );
    let out = plain(&env, 120);
    for secret in ["hunter2", ":pw@", "sk_live_0123", "eyJhbGci"] {
        assert!(!out.contains(secret), "{secret} shown:\n{out}");
    }
    assert!(out.contains("postgres://admin:"), "{out}");
    assert!(out.contains("@db/x"), "{out}");
    assert!(out.contains("vim"));
    assert!(out.contains("/usr/bin"));
    // Turning redaction off shows everything.
    assert!(plain(&env.redact(false), 120).contains("hunter2"));
}

// ---- X7: bidi controls -------------------------------------------------------

#[test]
fn bidi_controls_are_their_own_inert_clusters() {
    let view = UnicodeView::from_str("a\u{202e}bc\u{2066}\u{200f}\u{61c}");
    let kinds: Vec<Kind> = view.clusters().iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            Kind::Ascii,
            Kind::Bidi,
            Kind::Ascii,
            Kind::Ascii,
            Kind::Bidi,
            Kind::Bidi,
            Kind::Bidi
        ]
    );
    assert_eq!(view.clusters()[1].display(false), "\\u{202e}");
    assert_eq!(Kind::Bidi.label(), "bidi");
    assert_inert(&segments(&view, 100));
    assert_inert(&segments(&view, 40));

    let env = EnvView::new([("X\u{202e}".to_string(), "a\u{2067}b\x1b".to_string())]);
    assert_inert(&segments(&env, 60));
    assert!(plain(&env, 60).contains("a\\u{2067}b"));
}

// ---- X4: huge bytes per line -------------------------------------------------

#[test]
fn huge_bytes_per_line_is_clamped() {
    let view = HexView::new(vec![1u8]).bytes_per_line(Some(usize::MAX));
    assert!(view.line_width(usize::MAX) > 0);
    let started = Instant::now();
    let out = plain(&view, 80);
    assert!(out.starts_with("00000000  01"));
    let view = HexView::new(vec![1u8, 2, 3]).bytes_per_line(Some(50_000_000));
    let _ = plain(&view, 80);
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(
        HexView::new(vec![0u8; 5000])
            .bytes_per_line(Some(usize::MAX))
            .resolved_bytes_per_line(80),
        rich_ext::hex::MAX_BYTES_PER_LINE
    );
}

// ---- X8: start line at the top of usize --------------------------------------

#[test]
fn source_view_start_line_saturates() {
    let view = SourceView::new("a\nb\nc\n", "text").start_line(usize::MAX);
    let out = plain(&view, 60);
    assert!(out.contains(&usize::MAX.to_string()), "{out}");
    let _ = view.search("b").matches();
}

// ---- L5: many spans on one long line ------------------------------------------

#[test]
fn source_view_wraps_many_spans_in_linear_time() {
    let mut json = String::from("{");
    for i in 0..10_000 {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!("\"k{i}\":{i}"));
    }
    json.push('}');
    let started = Instant::now();
    let out = plain(&SourceView::new(json.clone(), "json"), 100);
    let elapsed = started.elapsed();
    assert!(out.lines().count() > 1000);
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");

    let nested = "[".repeat(100_000) + &"]".repeat(100_000);
    let started = Instant::now();
    let _ = plain(&SourceView::new(nested, "json").search("[]"), 100);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
}

// ---- X9: capability report ----------------------------------------------------

#[test]
fn empty_force_color_is_ignored() {
    let r = Capabilities::detect(&MapEnvironment::new().var("FORCE_COLOR", ""));
    assert_eq!(r.color.value, ColorDepth::None);
    assert_eq!(r.color.reason, "stdout is not a terminal");
}

#[test]
fn capability_report_escapes_environment_values() {
    let r = Capabilities::detect(
        &MapEnvironment::tty()
            .var("LANG", "en\x1b[2J.UTF-8")
            .var("COLORTERM", "truecolor\x1b]0;t\x07")
            .var("TERM_PROGRAM", "x\u{202e}\x1b[H"),
    );
    assert_inert(&segments(&CapabilityReport::new(&r), 120));
    assert!(!r.color.reason.contains('\x1b'), "{}", r.color.reason);
    assert!(!r.terminal.as_deref().unwrap_or("").contains('\x1b'));
}

// ---- X1, X3, X6: test reports ------------------------------------------------

#[cfg(feature = "test-report")]
mod reports {
    use super::*;
    use rich_ext::diff::test_report::{junit, libtest, TestReport};

    #[test]
    fn junit_decoded_controls_render_inert() {
        let xml = r#"<testsuite name="s&#x1b;[2J"><testcase name="a&#x1b;[31mb" classname="c&#x202e;"><failure message="m&#x1b;]0;x&#x7;">trace&#x1b;[H<![CDATA[
expected:<a> but was:<b>]]></failure><system-out>out&#x1b;[2J</system-out></testcase></testsuite>"#;
        let run = junit::parse(xml).unwrap();
        let case = &run.suites[0].cases[0];
        assert_eq!(case.name, "a\x1b[31mb");
        let report = TestReport::new(run).show_passed(true);
        assert_inert(&segments(&report, 80));
        assert_inert(&segments(&report, 30));
    }

    #[test]
    fn libtest_decoded_controls_render_inert() {
        let stream = concat!(
            "{ \"type\": \"suite\", \"event\": \"started\", \"test_count\": 1 }\n",
            "{ \"type\": \"test\", \"event\": \"failed\", \"name\": \"t\\u001b[2J\\u202e\", ",
            "\"stdout\": \"thread 'x' panicked at src/lib.rs:1:1:\\nboom\\u001b[31m\\n  left: 1\\u001b\\n right: 2\\n\" }\n",
            "{ \"type\": \"suite\", \"event\": \"failed\", \"passed\": 0, \"failed\": 1 }\n",
        );
        let run = libtest::parse(stream).unwrap();
        assert_eq!(run.suites[0].cases[0].name, "t\x1b[2J\u{202e}");
        assert_inert(&segments(&TestReport::new(run), 80));
    }

    #[test]
    fn truncated_junit_is_an_error() {
        for xml in [
            r#"<testsuite name="s"><testcase name="ok1"/><testcase name="boom"><failure message="bad">trace"#,
            r#"<testsuite name="s"><testcase name="ok1"/><testcase name="boom">"#,
            r#"<testsuites><testsuite name="s"><testcase name="ok1"/></testsuite>"#,
        ] {
            let err = junit::parse(xml).expect_err(xml);
            assert!(err.message.contains("truncated"), "{err}");
        }
        // A complete file still parses.
        let ok = junit::parse(r#"<testsuite name="s"><testcase name="ok1"/></testsuite>"#);
        assert!(ok.unwrap().is_success());
    }

    #[test]
    fn libtest_with_many_tests_parses_in_linear_time() {
        let n = 50_000;
        let mut stream = String::from("{ \"type\": \"suite\", \"event\": \"started\" }\n");
        for i in 0..n {
            stream.push_str(&format!(
                "{{ \"type\": \"test\", \"event\": \"started\", \"name\": \"m::t{i}\" }}\n"
            ));
        }
        for i in (0..n).rev() {
            stream.push_str(&format!(
                "{{ \"type\": \"test\", \"event\": \"ok\", \"name\": \"m::t{i}\" }}\n"
            ));
        }
        stream.push_str("{ \"type\": \"suite\", \"event\": \"ok\" }\n");
        let started = Instant::now();
        let run = libtest::parse(&stream).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(run.suites.len(), 1);
        let cases = &run.suites[0].cases;
        assert_eq!(cases.len(), n);
        // Order of first appearance is kept.
        assert_eq!(cases[0].name, "m::t0");
        assert_eq!(cases[n - 1].name, format!("m::t{}", n - 1));
        assert!(elapsed < Duration::from_secs(20), "took {elapsed:?}");
    }
}
