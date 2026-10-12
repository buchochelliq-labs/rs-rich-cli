//! Guide: Transforms — run: cargo run -p rs-rich-ext --example guide_transforms --features jsonpath [-- --svg docs/media/guide]
//!
//! The snippets and pictures on `docs/guide/ext/transforms.md` come from this
//! file: text and data pipelines, table and patch stages, and a transform
//! contributed by a plugin.

use std::path::PathBuf;
use std::sync::Arc;

use rich::{ColorSystem, Console, Style, Text};
use rich_ext::data::transform::{Document, Filter, Highlight, Redact, Select};
use rich_ext::data::{parse, Format, Redaction};
use rich_ext::plugin::{Plugin, PluginError, PluginMetadata, PluginRegistrar, TextTransform};
use rich_ext::table::transform::Sort;
use rich_ext::table::{Column, SortKey, TableData, Value};
use rich_ext::transform::{HighlightMatches, KeepLines, Pipeline};
use rich_ext::ExtensionRegistry;

// --8<-- [start:plugin]
/// Masks every digit.
struct MaskDigits;

impl TextTransform for MaskDigits {
    fn transform(&self, text: Text) -> Result<Text, PluginError> {
        let masked: String = text
            .plain()
            .chars()
            .map(|c| if c.is_ascii_digit() { '#' } else { c })
            .collect();
        // Same byte length, so the styles still line up.
        let mut out = Text::new(masked);
        for span in text.spans() {
            out.stylize(span.style.clone(), span.start, span.end);
        }
        Ok(out)
    }
}

struct Masking;

impl Plugin for Masking {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("masking", "Masking transforms", "1.0.0")
    }
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.transform("mask-digits", Arc::new(MaskDigits));
        Ok(())
    }
}
// --8<-- [end:plugin]

// --8<-- [start:text]
fn text(console: &Console) {
    let pipeline = Pipeline::new()
        .then("filter", KeepLines::new("WARN|ERROR").unwrap())
        .then(
            "highlight",
            HighlightMatches::new("ERROR", Style::parse("bold red").unwrap()).unwrap(),
        );
    let log = Text::new("INFO start\nWARN slow disk\nERROR failed\nINFO done");
    console.print(&pipeline.apply(log).unwrap());
}
// --8<-- [end:text]

// --8<-- [start:data]
fn data(console: &Console) {
    let node = parse(
        Format::Json,
        r#"{"servers": [{"host": "a", "port": 1, "token": "t1"},
                        {"host": "b", "port": 2, "token": "t2"}]}"#,
    )
    .unwrap();
    let pipeline = Pipeline::new()
        .then("redact", Redact(Redaction::secrets()))
        .then("select", Select::new("$.servers").unwrap())
        .then("filter", Filter::new("$[*]['host', 'token']").unwrap())
        .then(
            "highlight",
            Highlight::new("$[1]", Style::parse("reverse").unwrap()).unwrap(),
        );
    let document = pipeline
        .apply(Document::new(node).label("hosts.json"))
        .unwrap();
    console.print(&document.explorer());
}
// --8<-- [end:data]

// --8<-- [start:table]
fn table(console: &Console) {
    let mut data = TableData::new([Column::new("host"), Column::new("port")]);
    data.extend([
        [Value::from("b"), Value::from(2)],
        [Value::from("a"), Value::from(1)],
    ]);
    let sorted = Pipeline::new()
        .then("sort", Sort(vec![SortKey::asc(0)]))
        .apply(data)
        .unwrap();
    console.print(&sorted.to_table(console));
}
// --8<-- [end:table]

// --8<-- [start:registry]
fn registry(console: &Console) {
    let mut registry = ExtensionRegistry::new();
    registry.add_plugin(&Masking).unwrap();
    let pipeline = registry.text_pipeline(["mask-digits"]).unwrap();
    console.print(&pipeline.apply(Text::new("card 4242 4242")).unwrap());
}
// --8<-- [end:registry]

fn main() {
    let shots = Shots::from_args("guide_transforms");
    shots.shot("text", 40, text);
    shots.shot("data", 40, data);
    shots.shot("table", 40, table);
    shots.shot("registry", 40, registry);
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal.
struct Shots {
    topic: &'static str,
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args(topic: &'static str) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { topic, dir }
    }

    fn shot(&self, name: &str, width: usize, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            return f(&Console::new());
        };
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        let id = format!("{}-{name}", self.topic);
        let svg = console.export_svg(&id, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        let path = dir.join(format!("{id}.svg"));
        std::fs::write(&path, svg).expect("write the SVG");
        eprintln!("wrote {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_pipelines_produce_what_the_page_shows() {
        let text = Pipeline::new()
            .then("filter", KeepLines::new("WARN|ERROR").unwrap())
            .apply(Text::new(
                "INFO start\nWARN slow disk\nERROR failed\nINFO done",
            ))
            .unwrap();
        assert_eq!(text.plain(), "WARN slow disk\nERROR failed");

        let mut registry = ExtensionRegistry::new();
        registry.add_plugin(&Masking).unwrap();
        let masked = registry
            .text_pipeline(["mask-digits"])
            .unwrap()
            .apply(Text::new("card 4242"))
            .unwrap();
        assert_eq!(masked.plain(), "card ####");
    }
}
