use rich::Console;
use rich_ext::{
    diagnostic::{Diagnostic, Location, SourceSnippet, Suggestion},
    event::EventView,
    hyperlink::Hyperlinker,
};
fn main() {
    let source = "[server]\nport = invalid\nhost = \"localhost\"\n";
    let value = source.find("invalid").unwrap();
    let key = source.find("port").unwrap();
    let d = Diagnostic::error("invalid endpoint")
        .code("CFG001")
        .location(Location::new("config.toml", Some(2), Some(8)))
        .cause("port must be numeric")
        .snippet(
            SourceSnippet::new("config.toml".into(), source.into(), value..value + 7, 1)
                .unwrap()
                .primary_label("not a number")
                .secondary(key..key + 4, "for this key")
                .unwrap(),
        )
        .help("use a port from 1 to 65535")
        .suggestion(Suggestion::replace("for example", source, value..value + 7, "8080").unwrap())
        .hyperlinker(Hyperlinker::new())
        .view(EventView::Expanded);
    Console::new().print(&d);
}
