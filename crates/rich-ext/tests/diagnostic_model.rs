//! The structured diagnostic model: levels, codes, locations, labelled spans,
//! suggestions and error adapters (#210, #386, #387).
use rich::{Console, Renderable};
use rich_ext::diagnostic::{
    Diagnostic, DiagnosticInfo, Level, Location, SourceSnippet, Suggestion,
};
use rich_ext::event::EventView;
use rich_ext::hyperlink::Hyperlinker;

fn plain(d: &dyn Renderable, width: usize) -> String {
    let c = Console::builder().width(width).height(100).build();
    c.render_to_string(d)
}

const SOURCE: &str = "fn main() {\n    let port: u16 = \"eighty\";\n}\n";

fn mismatch() -> Diagnostic {
    let ty = SOURCE.find("u16").unwrap();
    let value = SOURCE.find("\"eighty\"").unwrap();
    Diagnostic::error("mismatched types")
        .code("E0308")
        .location(Location::new("src/main.rs", Some(2), Some(21)))
        .view(EventView::Expanded)
        .snippet(
            SourceSnippet::new("src/main.rs".into(), SOURCE.into(), value..value + 8, 0)
                .unwrap()
                .primary_label("expected `u16`, found `&str`")
                .secondary(ty..ty + 3, "expected due to this")
                .unwrap(),
        )
        .suggestion(Suggestion::replace("use a number", SOURCE, value..value + 8, "80").unwrap())
}

#[test]
fn a_compiler_style_diagnostic_renders_every_part() {
    let text = plain(&mismatch(), 80);
    let expected = [
        "error[E0308]: mismatched types",
        "  --> src/main.rs:2:21",
        "2 |     let port: u16 = \"eighty\";",
        "  |               ---   ^^^^^^^^ expected `u16`, found `&str`",
        "  |               expected due to this",
        "help: use a number",
        "2 |     let port: u16 = 80;",
        "  |                     ++",
    ]
    .join("\n");
    assert_eq!(text, expected);
}

#[test]
fn levels_style_the_header_and_primary_markers() {
    let c = Console::builder().force_terminal(true).width(80).build();
    let out = c.render_to_string(&mismatch());
    assert!(out.starts_with("\x1b[1;31merror"), "{out:?}");
    assert!(out.contains("\x1b[1;31m^^^^^^^^"), "{out:?}");
    assert!(out.contains("\x1b[1;34m---"), "{out:?}");
    let warn = c.render_to_string(&Diagnostic::warning("unused variable"));
    assert!(warn.starts_with("\x1b[1;33mwarning"), "{warn:?}");
}

#[test]
fn locations_and_codes_link_when_asked() {
    let c = Console::builder().force_terminal(true).width(80).build();
    let linked = mismatch()
        .code_url("https://doc.rust-lang.org/error_codes/E0308.html")
        .hyperlinker(Hyperlinker::new().base_dir("/work"));
    let out = c.render_to_string(&linked);
    assert!(out.contains("E0308.html"), "{out:?}");
    assert!(out.contains("file:///work/src/main.rs#2"), "{out:?}");
    // Without a hyperlinker the location is plain text.
    assert!(!c.render_to_string(&mismatch()).contains("file://"));
}

#[test]
fn a_label_on_a_multi_line_span_sits_on_its_last_line() {
    let source = "call(\n  a,\n  b)\n";
    let d = Diagnostic::error("bad call")
        .view(EventView::Expanded)
        .snippet(
            SourceSnippet::new("x".into(), source.into(), 0..source.len() - 1, 0)
                .unwrap()
                .primary_label("this call"),
        );
    let text = plain(&d, 80);
    assert!(text.contains("3 |   b)\n  | ^^^^ this call"), "{text}");
    assert!(!text.contains("^^^^^ this call\n2"), "{text}");
    assert_eq!(text.matches("this call").count(), 1);
}

#[test]
fn a_location_and_its_snippet_show_the_file_once() {
    let snippet = || SourceSnippet::new("a.toml".into(), "k = v\n".into(), 4..5, 0).unwrap();
    let located = Diagnostic::error("bad")
        .location(Location::new("a.toml", Some(1), Some(5)))
        .view(EventView::Expanded)
        .snippet(snippet());
    assert_eq!(
        plain(&located, 80),
        "error: bad\n  --> a.toml:1:5\n1 | k = v\n  |     ^"
    );
    // A snippet from another file keeps its own header.
    let elsewhere = Diagnostic::error("bad")
        .location(Location::new("b.toml", Some(1), Some(1)))
        .view(EventView::Expanded)
        .snippet(snippet());
    assert_eq!(
        plain(&elsewhere, 80),
        "error: bad\n  --> b.toml:1:1\n--> a.toml\n1 | k = v\n  |     ^"
    );
    // Without a location the snippet's header is the only one, as before.
    let bare = Diagnostic::error("bad")
        .view(EventView::Expanded)
        .snippet(snippet());
    assert_eq!(
        plain(&bare, 80),
        "error: bad\n--> a.toml\n1 | k = v\n  |     ^"
    );
}

#[test]
fn snippet_locations_count_characters() {
    let source = "é = 1\nlet x = é;\n";
    let at = source.rfind('é').unwrap();
    let snippet = SourceSnippet::new("a.rs".into(), source.into(), at..at + 2, 0).unwrap();
    assert_eq!(snippet.location(), Location::new("a.rs", Some(2), Some(9)));
    let d = Diagnostic::new("x").snippet(snippet);
    assert_eq!(d.get_location().unwrap().line, Some(2));
}

#[derive(Debug, thiserror::Error)]
enum ConfigError {
    #[error("port {0:?} is not a number")]
    BadPort(String),
    #[error("cannot read the config")]
    Read(#[from] std::io::Error),
}

impl DiagnosticInfo for ConfigError {
    fn code(&self) -> Option<String> {
        Some(match self {
            ConfigError::BadPort(_) => "C001".into(),
            ConfigError::Read(_) => "C002".into(),
        })
    }
    fn help(&self) -> Option<String> {
        matches!(self, ConfigError::BadPort(_)).then(|| "use a number such as 8080".into())
    }
    fn location(&self) -> Option<Location> {
        Some(Location::new("app.toml", Some(2), None))
    }
}

#[test]
fn thiserror_types_map_through_diagnostic_info() {
    let d = ConfigError::BadPort("eighty".into())
        .to_diagnostic()
        .view(EventView::Expanded);
    assert_eq!(d.get_level(), Some(Level::Error));
    assert_eq!(
        plain(&d, 80),
        "error[C001]: port \"eighty\" is not a number\n  --> app.toml:2\nhelp: use a number such as 8080"
    );
    let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
    let text = plain(&ConfigError::from(io).to_diagnostic(), 80);
    assert!(
        text.contains("error[C002]: cannot read the config"),
        "{text}"
    );
    assert!(text.contains("caused by: no such file"), "{text}");
}

#[cfg(feature = "anyhow")]
#[test]
fn anyhow_context_frames_become_causes() {
    use anyhow::Context;
    let error = std::fs::read_to_string("/definitely/missing")
        .context("reading the config")
        .context("starting the server")
        .unwrap_err();
    let d = Diagnostic::from_anyhow(&error, 8);
    let text = plain(&d, 80);
    assert!(text.starts_with("error: starting the server\n"), "{text}");
    assert!(text.contains("caused by: reading the config"), "{text}");
    assert!(text.contains("caused by: No such file"), "{text}");
}

#[test]
fn a_stack_trace_renders_in_the_expanded_view() {
    let trace = rich_ext::stacktrace::parse(
        "Traceback (most recent call last):\n  File \"/app/x.py\", line 3, in main\n    f()\nKeyError: 'k'\n",
    )
    .unwrap();
    let d = Diagnostic::error("worker crashed").trace(trace.clone());
    assert!(!plain(&d, 80).contains("KeyError"), "compact view hides it");
    let text = plain(&d.view(EventView::Expanded), 80);
    assert!(text.contains("at /app/x.py:3"), "{text}");
    assert!(text.contains("KeyError: 'k'"), "{text}");
}
