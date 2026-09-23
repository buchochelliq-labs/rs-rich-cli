//! Structured data: explore, tabulate, search and redact a document, and
//! show a parse error as a diagnostic.
//!
//! Run with `cargo run -p rs-rich-ext --example structured_data --features data`.
use rich::Console;
use rich_ext::data::{
    parse, parse_dotenv, print_table, ConfigFileView, Explorer, Format, Redaction, SearchQuery,
    SearchResults,
};

#[derive(serde::Serialize)]
struct Server {
    name: &'static str,
    port: u16,
    region: Option<&'static str>,
}

fn main() {
    let console = Console::new();
    let doc = parse(
        Format::Json,
        r#"{"name": "demo", "servers": [{"host": "a.example", "port": 80},
            {"host": "b.example", "port": 443}], "db": {"user": "app", "password": "hunter2"}}"#,
    )
    .expect("valid JSON");

    console.print(&Explorer::new(doc.redacted(&Redaction::secrets())).root_label("config.json"));
    console.print(&SearchResults::new(&doc, &SearchQuery::key("port")).context(1));

    print_table(&[
        Server {
            name: "web",
            port: 80,
            region: Some("eu"),
        },
        Server {
            name: "db",
            port: 5432,
            region: None,
        },
    ])
    .expect("serializable");

    let env = parse_dotenv("# where to listen\nHOST=0.0.0.0\nAPI_TOKEN=abc123\n").expect("valid");
    console.print(&ConfigFileView::new(&env).redactor(Redaction::secrets()));

    let broken = "{\n  \"name\" \"demo\"\n}";
    if let Err(error) = parse(Format::Json, broken) {
        console.print(&error.to_diagnostic(broken, "broken.json"));
    }
}
