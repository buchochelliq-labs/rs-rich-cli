//! The diagnostics dashboard groups by file and counts by level and code (#329).
use rich::Console;
use rich_ext::dashboard::DiagnosticsDashboard;
use rich_ext::diagnostic::{Diagnostic, Level, Location};
use rich_ext::hyperlink::Hyperlinker;

fn sample() -> DiagnosticsDashboard {
    let at = |path: &str, line, column| Location::new(path, Some(line), Some(column));
    let mut dashboard = DiagnosticsDashboard::new();
    dashboard
        .push(
            Diagnostic::error("mismatched types")
                .code("E0308")
                .location(at("src/main.rs", 12, 5)),
        )
        .push(
            Diagnostic::warning("unused variable `x`")
                .code("W1")
                .location(at("src/main.rs", 3, 9)),
        )
        .push(
            Diagnostic::warning("unused import")
                .code("W1")
                .location(at("src/lib.rs", 1, 5)),
        )
        .push(Diagnostic::new("note without a place").level(Level::Note));
    dashboard
}

fn plain(dashboard: &DiagnosticsDashboard) -> String {
    Console::builder()
        .width(60)
        .build()
        .render_to_string(dashboard)
}

#[test]
fn summary_codes_and_files_in_order() {
    let text = plain(&sample());
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    assert_eq!(lines[0], "1 error, 2 warnings, 1 note in 2 files");
    assert!(text.contains("Top codes"), "{text}");
    // W1 (twice) outranks E0308.
    let w1 = text.find("│ W1").unwrap();
    let e0308 = text.find("│ E0308").unwrap();
    assert!(w1 < e0308, "{text}");
    // Files sorted by path; entries by line; unlocated last.
    let lib = text.find("src/lib.rs  1 diagnostic").unwrap();
    let main = text.find("src/main.rs  2 diagnostics").unwrap();
    let none = text.find("(no location)").unwrap();
    assert!(lib < main && main < none, "{text}");
    let unused = text.find(" 3:9 warning[W1]  unused variable").unwrap();
    let mismatched = text.find("12:5 error[E0308] mismatched types").unwrap();
    assert!(unused < mismatched, "{text}");
}

#[test]
fn min_level_filters_and_empty_dashboards_say_so() {
    let errors = sample().min_level(Level::Error);
    assert_eq!(
        errors.counts().into_iter().collect::<Vec<_>>(),
        vec![(Level::Error, 1)]
    );
    assert!(plain(&errors).starts_with("1 error in 1 file"));
    assert!(plain(&DiagnosticsDashboard::new()).starts_with("No diagnostics"));
}

#[test]
fn paths_link_through_the_hyperlinker() {
    let console = Console::builder().force_terminal(true).width(60).build();
    let out = console.render_to_string(&sample().hyperlinker(Hyperlinker::new().base_dir("/w")));
    assert!(out.contains("file:///w/src/main.rs#12"), "{out:?}");
    assert!(!console.render_to_string(&sample()).contains("file://"));
}
