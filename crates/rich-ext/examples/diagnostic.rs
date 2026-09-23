use rich::Console;
use rich_ext::{
    diagnostic::{Diagnostic, SourceSnippet},
    event::EventView,
};
fn main() {
    let d = Diagnostic::new("Invalid endpoint")
        .cause("port must be numeric")
        .snippet(
            SourceSnippet::new("config.toml".into(), "port = invalid".into(), 7..14, 1).unwrap(),
        )
        .help("Use a port from 1 to 65535")
        .view(EventView::Expanded);
    Console::new().print(&d);
}
