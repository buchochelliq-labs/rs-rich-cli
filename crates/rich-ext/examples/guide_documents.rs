//! Guide: Documents — run: cargo run -p rs-rich-ext --example guide_documents [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/documents.md` comes from this file.
use std::path::PathBuf;
use std::sync::Arc;

use rich::{ColorSystem, Console, Table, Text};
use rich_ext::document::{
    model::{Block, Document, Inline},
    DocumentFormat, DocumentRegistry, DocumentView, RenderOptions, Severity, CERTAIN,
};

const ORG: &str = "\
#+TITLE: Release plan
* DONE [#A] Parse the formats                      :core:
  CLOSED: [2026-09-30 Wed]
* TODO Write the docs [1/2]
- [X] A guide page with /screenshots/
- [ ] A tape for =rich record=
#+BEGIN_SRC rust
let view = DocumentView::org(source);
#+END_SRC
";

const ASCIIDOC: &str = "\
= Field Guide
:product: rich

NOTE: {product} renders *AsciiDoc* too.

[source,bash]
----
rich guide.adoc # <1>
----
<1> Found by its extension.

|===
| Format | Extensions

| Org | .org
| AsciiDoc | .adoc, .asciidoc
|===
";

const MAN: &str = r#".TH GREET 1 "October 2026" "greet 1.0" "User Commands"
.SH NAME
greet \- print a friendly greeting
.SH OPTIONS
.TP
.BR \-n ", " \-\-count =\fIN\fR
Repeat each greeting \fIN\fR times.
.TP
\fB\-s\fR, \fB\-\-shout\fR
Write the greeting in capitals.
"#;

// --8<-- [start:org]
fn show_org(console: &Console) {
    console.print(&DocumentView::org(ORG));
}
// --8<-- [end:org]

// --8<-- [start:asciidoc]
fn show_asciidoc(console: &Console) {
    // Without hyperlinks a link's target is written out, as `rich` does
    // unless `-y` asks for terminal hyperlinks.
    console.print(&DocumentView::asciidoc(ASCIIDOC).hyperlinks(false));
}
// --8<-- [end:asciidoc]

// --8<-- [start:man]
fn show_man(console: &Console) {
    console.print(&DocumentView::man(MAN));
}
// --8<-- [end:man]

// --8<-- [start:detect]
fn show_detect(console: &Console) {
    let registry = DocumentRegistry::with_builtins();
    let mut table = Table::new();
    for header in ["Path", "Content", "Format", "Confidence", "By"] {
        table.add_column(header);
    }
    for (path, content) in [
        (Some("notes.org"), ""),
        (Some("ls.1"), MAN),
        (Some("syslog.1"), "Jan 1 boot ok\n"),
        (Some("gen.c"), MAN),
        (Some("GREET"), MAN),
        (None, ORG),
    ] {
        let detected = registry.detect(path, content, CERTAIN);
        let (format, confidence, by) = match &detected {
            Some(found) => (
                found.format.clone(),
                format!("{:.2}", found.confidence),
                format!("{:?}", found.by),
            ),
            None => ("—".into(), "".into(), "".into()),
        };
        let shown = content.lines().next().unwrap_or("");
        table.add_row_text(vec![
            Text::new(path.unwrap_or("<stdin>")),
            Text::new(shown.chars().take(18).collect::<String>()),
            Text::new(format),
            Text::new(confidence),
            Text::new(by),
        ]);
    }
    console.print(&table);
}
// --8<-- [end:detect]

// --8<-- [start:registry]
fn show_registry(console: &Console) {
    let registry = DocumentRegistry::with_builtins();
    // Options apply to every format: links, code theme, the language of
    // unlabelled code, and fence renderers (Mermaid, DOT) for code blocks.
    let options = RenderOptions::new()
        .hyperlinks(false)
        .default_language("bash");
    let source = ".TH DEMO 1\n.SH EXAMPLE\n.EX\nrich ls.1 | less -R\n.EE\n.so other.1\n";
    let rendered = registry
        .render(source, "man", &options)
        .expect("a built-in");
    console.print(rendered.renderable.as_ref());
    for diagnostic in &rendered.diagnostics {
        if diagnostic.severity == Severity::Warning {
            console.print(&Text::styled(format!("warning: {diagnostic}"), "yellow"));
        }
    }
}
// --8<-- [end:registry]

// --8<-- [start:model]
fn show_model(console: &Console) {
    // Any producer can build the shared model and draw it the same way.
    let document = Document {
        blocks: vec![
            Block::Paragraph(vec![
                Inline::text("Built by hand: "),
                Inline::Strong(vec![Inline::text("strong")]),
                Inline::text(", "),
                Inline::Code("code".into()),
                Inline::text(" and "),
                Inline::Link {
                    target: "https://example.com".into(),
                    content: vec![Inline::text("a link")],
                },
                Inline::text("."),
            ]),
            Block::Rule,
        ],
    };
    console.print(&DocumentView::new(document).hyperlinks(false));
}
// --8<-- [end:model]

// --8<-- [start:custom]
/// A format of your own: `.todo` files, one task per line.
struct TodoFormat;

impl DocumentFormat for TodoFormat {
    fn id(&self) -> &str {
        "todo"
    }

    fn name(&self) -> &str {
        "Task list"
    }

    fn extensions(&self) -> Vec<String> {
        vec!["todo".into()]
    }

    fn parse(&self, source: &str) -> Option<rich_ext::document::Parsed> {
        use rich_ext::document::model::{Checkbox, List, ListItem, ListKind};
        let items = source
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let (done, text) = match line.strip_prefix("x ") {
                    Some(text) => (true, text),
                    None => (false, line),
                };
                ListItem {
                    checkbox: Some(if done {
                        Checkbox::Checked
                    } else {
                        Checkbox::Unchecked
                    }),
                    number: None,
                    blocks: vec![Block::Paragraph(vec![Inline::text(text)])],
                }
            })
            .collect();
        let list = Block::List(List {
            kind: ListKind::Bullet,
            items,
        });
        Some(rich_ext::document::Parsed {
            document: Document { blocks: vec![list] },
            diagnostics: Vec::new(),
        })
    }
}

fn show_custom(console: &Console) {
    let mut registry = DocumentRegistry::with_builtins();
    registry.register(Arc::new(TodoFormat)).expect("a new name");
    let found = registry
        .detect(Some("week.todo"), "", CERTAIN)
        .expect("by extension");
    let rendered = registry
        .render(
            "x Ship 0.0.19\nWrite the release notes\n",
            &found.format,
            &RenderOptions::new(),
        )
        .expect("registered");
    console.print(rendered.renderable.as_ref());
}
// --8<-- [end:custom]

fn main() {
    let shots = Shots::from_args();
    shots.shot("org", 64, "notes.org", show_org);
    shots.shot("asciidoc", 64, "guide.adoc", show_asciidoc);
    shots.shot("man", 64, "greet.1", show_man);
    shots.shot("detect", 72, "DocumentRegistry::detect", show_detect);
    shots.shot("registry", 64, "DocumentRegistry::render", show_registry);
    shots.shot("model", 64, "DocumentView::new", show_model);
    shots.shot("custom", 64, "A registered format", show_custom);
}

/// `--svg DIR` writes each shot as `DIR/guide_documents-<shot>.svg`;
/// without it, shots print to the terminal.
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
            let console = Console::builder().theme(rich_ext::extended_theme()).build();
            return f(&console);
        };
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .theme(rich_ext::extended_theme())
            .build();
        let id = format!("guide_documents-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}
