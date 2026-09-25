//! The "Transforms" guide: text and data pipelines, table and patch stages,
//! and a transform contributed by a plugin.
//!
//! `cargo run -p rs-rich-ext --example guide_transforms --features jsonpath`

use std::sync::Arc;

use rich::{Console, Style, Text};
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

fn main() {
    let console = Console::new();

    // --8<-- [start:text]
    let pipeline = Pipeline::new()
        .then("filter", KeepLines::new("WARN|ERROR").unwrap())
        .then(
            "highlight",
            HighlightMatches::new("ERROR", Style::parse("bold red").unwrap()).unwrap(),
        );
    let log = Text::new("INFO start\nWARN slow disk\nERROR failed\nINFO done");
    console.print(&pipeline.apply(log).unwrap());
    // --8<-- [end:text]

    // --8<-- [start:data]
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
    // --8<-- [end:data]

    // --8<-- [start:table]
    let mut data = TableData::new([Column::new("host"), Column::new("port")]);
    data.extend([
        [Value::from("b"), Value::from(2)],
        [Value::from("a"), Value::from(1)],
    ]);
    let sorted = Pipeline::new()
        .then("sort", Sort(vec![SortKey::asc(0)]))
        .apply(data)
        .unwrap();
    console.print(&sorted.to_table(&console));
    // --8<-- [end:table]

    // --8<-- [start:registry]
    let mut registry = ExtensionRegistry::new();
    registry.add_plugin(&Masking).unwrap();
    let pipeline = registry.text_pipeline(["mask-digits"]).unwrap();
    console.print(&pipeline.apply(Text::new("card 4242 4242")).unwrap());
    // --8<-- [end:registry]
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
