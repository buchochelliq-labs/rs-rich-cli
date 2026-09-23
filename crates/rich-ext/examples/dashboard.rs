use rich_ext::dashboard::DiagnosticsDashboard;
use rich_ext::diagnostic::{Diagnostic, Level, Location};
fn main() {
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
    rich::Console::builder().width(60).build().print(&dashboard);
}
