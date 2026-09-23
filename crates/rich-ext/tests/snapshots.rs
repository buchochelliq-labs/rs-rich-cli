#![cfg(feature = "testing")]
use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Text, Theme};
use rich_ext::{
    target::{RenderTarget, TargetKind},
    testing::RenderSnapshot,
};
fn target() -> RenderTarget {
    RenderTarget::new(
        TargetKind::Capture,
        TargetCapabilities {
            width: 20,
            height: 8,
            color_system: Some(ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    )
}
#[test]
fn style_only_differences_are_visible_and_serialization_is_stable() {
    let t = target();
    let a = RenderSnapshot::capture(&t, &Text::styled("hello", "red"));
    let b = RenderSnapshot::capture(&t, &Text::styled("hello", "blue"));
    assert_eq!(a.plain, "hello");
    assert_eq!(a.plain, b.plain);
    assert!(a.diff(&b).unwrap().contains("foreground"));
    assert_eq!(
        a.to_json().unwrap(),
        RenderSnapshot::capture(&t, &Text::styled("hello", "red"))
            .to_json()
            .unwrap()
    );
    assert_eq!((a.width, a.height, a.schema_version), (20, 8, 1));
    assert!(a.ansi.contains("\x1b[31m"));
}
#[test]
fn visible_diff_identifies_changed_line_and_metadata_does_not_hide_links() {
    let t = target();
    let a = RenderSnapshot::capture(&t, &Text::new("one\ntwo"));
    let b = RenderSnapshot::capture(&t, &Text::new("one\nthree"));
    let diff = a.diff(&b).unwrap();
    assert!(diff.contains("-two"));
    assert!(diff.contains("+three"));
    assert_eq!(a.diff(&a), None);
}

/// Checked-in `RenderSnapshot` fixtures for diagnostics (#146): a multiline
/// message with notes, a real error chain and a source snippet. Regenerate
/// with `UPDATE_SNAPSHOTS=1 cargo test -p rs-rich-ext --features testing`.
#[test]
fn diagnostic_snapshots_match_fixtures() {
    use rich_ext::diagnostic::{Diagnostic, SourceSnippet};
    use rich_ext::event::EventView;

    #[derive(Debug)]
    struct Inner;
    impl std::fmt::Display for Inner {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "permission denied")
        }
    }
    impl std::error::Error for Inner {}
    #[derive(Debug)]
    struct Outer(Inner);
    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "cannot open config")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    let source = "[server]\nport = \"eighty\"\nhost = \"localhost\"\n";
    let start = source.find("\"eighty\"").unwrap();
    let cases: Vec<(&str, Diagnostic)> = vec![
        (
            "diagnostic_multiline",
            Diagnostic::new("migration failed\nrolled back 3 steps")
                .view(EventView::Expanded)
                .note("the database is unchanged")
                .help("rerun with --verbose"),
        ),
        (
            "diagnostic_chain",
            Diagnostic::from_error(&Outer(Inner), 8).view(EventView::Expanded),
        ),
        (
            "diagnostic_source",
            Diagnostic::new("expected an integer")
                .view(EventView::Expanded)
                .snippet(
                    SourceSnippet::new("app.toml".into(), source.into(), start..start + 8, 1)
                        .unwrap(),
                )
                .help("use a port number such as 8080"),
        ),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    for (name, diagnostic) in cases {
        let got = RenderSnapshot::capture(&target(), &diagnostic)
            .to_json()
            .unwrap();
        let path = dir.join(format!("{name}.json"));
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run with UPDATE_SNAPSHOTS=1", path.display()));
        assert_eq!(
            got, expected,
            "{name} snapshot changed; run with UPDATE_SNAPSHOTS=1 if intended"
        );
    }
}
