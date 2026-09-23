//! Guide: Structured data — run: cargo run -p rs-rich-ext --example guide_data --features data,yaml,toml,xml,jsonpath [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/structured-data.md` comes from this file.
use std::path::PathBuf;

use rich::{ColorSystem, Console, Justify};
use rich_ext::data::{
    diff, flatten, parse, parse_ini, print_table_to, print_tree_to, unflatten, ConfigFileView,
    DataError, DiffView, Explorer, FlatView, Format, JsonPathSelector, Node, Path, Redaction,
    SearchQuery, SearchResults, SelectError, Selector, SelectorBackend, Selectors, TableOptions,
    TableView, Value, View,
};
use rich_ext::ConsoleExt;

// --8<-- [start:explorer]
const DEPLOY_YAML: &str = "\
defaults: &defaults
  replicas: 2
  image: registry.example/app:1.4
services:
  web:
    port: 8080
    settings: *defaults
  worker: *defaults
";

fn show_explorer(console: &Console) {
    let doc = parse(Format::Yaml, DEPLOY_YAML).expect("valid YAML");
    console.print(&Explorer::new(&doc).root_label("deploy.yaml"));
}
// --8<-- [end:explorer]

// --8<-- [start:parse]
fn parse_documents() -> Result<(Node, Node), DataError> {
    // Name the format explicitly...
    let deploy = parse(Format::Yaml, DEPLOY_YAML)?;
    // ...or let `Format::detect` guess it (a file name hint wins outright).
    let text = r#"{"name": "api", "ports": [80, 443], "tls": true}"#;
    let format = Format::detect(text, None).expect("recognisable");
    assert_eq!(format, Format::Json);
    let api = parse(format, text)?;

    // Every format produces the same `Node` tree.
    let port = api.at(&"ports[1]".parse::<Path>().unwrap()).unwrap();
    assert_eq!(port.value, Value::Int(443));
    assert_eq!(
        deploy.get("defaults").unwrap().meta.anchor.as_deref(),
        Some("defaults")
    );
    Ok((deploy, api))
}
// --8<-- [end:parse]

// --8<-- [start:explorer-limits]
fn show_limits(console: &Console, deploy: &Node) {
    let explorer = Explorer::new(deploy)
        .root_label("deploy.yaml")
        .fold("defaults".parse().unwrap()) // fold one container by path
        .max_depth(2) // fold everything two levels down
        .show_paths(true);
    console.print(&explorer);
}
// --8<-- [end:explorer-limits]

// --8<-- [start:explorer-table]
fn show_table_view(console: &Console) {
    let hosts = parse(
        Format::Json,
        r#"[{"host": "a.example", "port": 80, "up": true},
            {"host": "b.example", "port": 443, "up": false},
            {"host": "c.example", "up": null}]"#,
    )
    .unwrap();
    console.print(&Explorer::new(&hosts).view(View::Table));
}
// --8<-- [end:explorer-table]

// --8<-- [start:formats]
fn show_formats(console: &Console) {
    let toml = parse(
        Format::Toml,
        "[package]\nname = \"demo\"\nreleased = 2026-09-23\n",
    )
    .unwrap();
    let xml = parse(
        Format::Xml,
        r#"<server id="web"><port>8080</port><port>8443</port></server>"#,
    )
    .unwrap();
    console.print(&Explorer::new(&toml).root_label("Cargo.toml"));
    console.print(&Explorer::new(&xml).root_label("server.xml"));
}
// --8<-- [end:formats]

// --8<-- [start:serde]
#[derive(serde::Serialize)]
struct Release {
    name: &'static str,
    version: &'static str,
    downloads: u64,
    yanked: bool,
}

fn show_serde(console: &Console) -> Result<(), DataError> {
    let releases = [
        Release {
            name: "rs-rich",
            version: "0.0.7",
            downloads: 1200,
            yanked: false,
        },
        Release {
            name: "rs-rich-ext",
            version: "0.0.9",
            downloads: 310,
            yanked: false,
        },
        Release {
            name: "rs-rich-cli",
            version: "0.0.10",
            downloads: 5400,
            yanked: true,
        },
    ];
    // `print_table(&releases)` prints to stdout; the `_to` forms take a console.
    print_table_to(console, &releases)?;
    print_tree_to(console, &releases[0])?;
    Ok(())
}
// --8<-- [end:serde]

// --8<-- [start:table-options]
fn show_table_options(console: &Console) -> Result<(), DataError> {
    let rows = rich_ext::data::from_serialize(&serde_json::json!([
        {"name": "web", "cpu": 0.42, "region": "eu-west-1", "notes": "canary"},
        {"name": "db", "cpu": 0.91, "region": "eu-west-1"},
        {"name": "cache", "cpu": 0.08, "region": "us-east-2"},
    ]))?;
    let options = TableOptions::new()
        .title("Services")
        .columns(["name", "cpu"]) // pick and order columns
        .header("cpu", "CPU")
        .justify("name", Justify::Center)
        .max_rows(2); // then "… 1 more"
    console.print(&TableView::new(&rows).options(options));
    Ok(())
}
// --8<-- [end:table-options]

// --8<-- [start:errors]
fn show_error(console: &Console) {
    let source = "{\n  \"name\": \"api\",\n  \"port\" 8080\n}\n";
    let error = parse(Format::Json, source).unwrap_err();
    // `Display` gives one line: invalid JSON at line 3, column 10: …
    eprintln!("{error}");
    // `to_diagnostic` points at the offending character.
    console.print(&error.to_diagnostic(source, "api.json"));
}
// --8<-- [end:errors]

// --8<-- [start:flatten]
fn show_flatten(console: &Console, api: &Node) {
    console.print(&FlatView::new(api).show_types(true));

    // Leaves go back together into the same shape.
    let leaves = flatten(api);
    let rebuilt = unflatten(leaves).expect("consistent paths");
    assert_eq!(rebuilt.to_json(), api.to_json());
}
// --8<-- [end:flatten]

// --8<-- [start:search]
fn show_search(console: &Console, deploy: &Node) {
    // `**` is any number of segments; `*` one segment; `[*]` any index.
    let query = SearchQuery::path("services.**").and_value("8080");
    console.print(&SearchResults::new(deploy, &query).context(1));
    console.print(&SearchResults::new(
        deploy,
        &SearchQuery::key("IMAGE").case_insensitive(true),
    ));
}
// --8<-- [end:search]

// --8<-- [start:select]
fn show_select(api: &Node) -> Result<(), SelectError> {
    // Through the registry, as a CLI flag like `--select jsonpath:…` would...
    let selector = Selectors::default().compile("jsonpath", "$.ports[?(@ > 100)]")?;
    for (path, node) in selector.select(api)? {
        println!("{path} = {:?}", node.value);
    }
    // ...or directly.
    let tls = JsonPathSelector::parse("$.tls")?.select(api)?;
    assert_eq!(tls[0].1.value, Value::Bool(true));
    Ok(())
}
// --8<-- [end:select]

// --8<-- [start:backend]
/// A tiny selection language: a dotted path such as `ports.0`.
struct Dotted;

struct DottedSelector(Path);

impl Selector for DottedSelector {
    fn select<'a>(&self, root: &'a Node) -> Result<Vec<(Path, &'a Node)>, SelectError> {
        Ok(root
            .at(&self.0)
            .map(|n| (self.0.clone(), n))
            .into_iter()
            .collect())
    }
}

impl SelectorBackend for Dotted {
    fn name(&self) -> &str {
        "dotted"
    }
    fn compile(&self, expr: &str) -> Result<Box<dyn Selector>, SelectError> {
        let mut path = Path::root();
        for part in expr.split('.') {
            path = match part.parse::<usize>() {
                Ok(index) => path.child_index(index),
                Err(_) => path.child_key(part),
            };
        }
        Ok(Box::new(DottedSelector(path)))
    }
}

fn custom_backend(api: &Node) -> Result<(), SelectError> {
    let mut selectors = Selectors::default();
    selectors.register(Box::new(Dotted));
    let hits = selectors.compile("dotted", "ports.0")?.select(api)?;
    assert_eq!(hits[0].0.to_string(), "ports[0]");
    Ok(())
}
// --8<-- [end:backend]

// --8<-- [start:diff]
fn show_diff(console: &Console, api: &Node) {
    let next = parse(
        Format::Json,
        r#"{"name": "api", "ports": [80, 8443], "tls": true, "hsts": true}"#,
    )
    .unwrap();
    for change in diff(api, &next) {
        eprintln!("{:?} at {}", change.kind, change.path);
    }
    console.print(&DiffView::new(api, &next));
}
// --8<-- [end:diff]

// --8<-- [start:redact]
fn show_redaction(console: &Console) {
    let ini = parse_ini(
        "; production settings\n\
         [database]\n\
         host = db.internal\n\
         ; rotate monthly\n\
         password = hunter2\n\
         [api]\n\
         token = sk-live-123\n\
         timeout = 30\n",
    )
    .unwrap();
    // Masks keys containing password, secret, token, key, … (see SECRET_KEYS).
    let redaction = Redaction::secrets().pattern("host").mask("[hidden]");
    console.print(
        &ConfigFileView::new(&ini)
            .title("app.ini")
            .redactor(redaction),
    );

    // Or redact the tree itself, for any view.
    let safe = ini.redacted(&Redaction::secrets());
    console.print(&Explorer::new(&safe).max_depth(2));
}
// --8<-- [end:redact]

fn main() {
    let shots = Shots::from_args();
    let (deploy, api) = parse_documents().expect("valid documents");
    shots.shot("explorer", 60, "Explorer", show_explorer);
    shots.shot("explorer-limits", 60, "Explorer limits", |c| {
        show_limits(c, &deploy)
    });
    shots.shot("explorer-table", 60, "Table view", show_table_view);
    shots.shot("formats", 60, "TOML and XML", show_formats);
    shots.shot("serde", 60, "print_table", |c| show_serde(c).unwrap());
    shots.shot("table-options", 60, "TableOptions", |c| {
        show_table_options(c).unwrap()
    });
    shots.shot("errors", 60, "Parse error", show_error);
    shots.shot("flatten", 60, "FlatView", |c| show_flatten(c, &api));
    shots.shot("search", 60, "Search", |c| show_search(c, &deploy));
    show_select(&api).expect("valid selector");
    custom_backend(&api).expect("valid selector");
    shots.shot("diff", 60, "DiffView", |c| show_diff(c, &api));
    shots.shot("redact", 64, "Redaction", show_redaction);
}

/// `--svg DIR` writes each shot as `DIR/guide_data-<shot>.svg`; without it,
/// shots print to the terminal.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { dir }
    }

    fn shot(&self, name: &str, width: usize, title: &str, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            let mut console = Console::new();
            console.install_extensions();
            return f(&console);
        };
        let mut console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        console.install_extensions();
        let id = format!("guide_data-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}
