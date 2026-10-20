//! Guide: Markdown for GitHub — run: cargo run -p rs-rich-ext --example guide_markdown_export
//!
//! The snippets on `docs/guide/ext/markdown-export.md` come from this file.
//! It prints the Markdown it builds; `cargo test` runs it and checks it.

use rich::{Justify, Panel, Rule, Table, Text, Tree};
use rich_ext::markdown_export::{
    fit_github_comment, GfmDocument, GfmExporter, GfmOptions, PanelStyle,
};

fn report() -> String {
    // --8<-- [start:document]
    let mut table = Table::new().title("Checks");
    table.add_column("check");
    table.add_column_justify("time", Justify::Right);
    table.add_column_justify("result", Justify::Center);
    table.add_row(&["lint", "1.2 s", "[bold]pass[/]"]);
    table.add_row(&["unit | integration", "48 s", "[s]flaky[/]"]);

    let mut tree = Tree::new("[bold]changed[/]");
    tree.add("crates/rich-ext")
        .add("[code]markdown_export.rs[/]");

    let mut doc = GfmDocument::new();
    doc.push_heading(2, "CI report")
        .push(&table)
        .push(&Rule::line())
        .push(&tree)
        .push_code("SELECT count(*) FROM runs;", "sql");
    let comment = fit_github_comment(&doc.finish());
    // --8<-- [end:document]
    comment
}

fn panels() -> (String, String) {
    // --8<-- [start:panels]
    let panel = || Panel::new(Box::new(Text::new("3 warnings\n1 note"))).title("Lint");
    let quote = GfmExporter::new().export(&panel());
    let details =
        GfmExporter::with_options(GfmOptions::new().panel(PanelStyle::Details { open: false }))
            .export(&panel());
    // --8<-- [end:panels]
    (quote, details)
}

fn main() {
    let comment = report();
    assert_eq!(
        comment,
        "## CI report\n\n\
         **Checks**\n\n\
         | check | time | result |\n\
         | --- | ---: | :---: |\n\
         | lint | 1.2 s | **pass** |\n\
         | unit \\| integration | 48 s | ~~flaky~~ |\n\n\
         ---\n\n\
         - **changed**\n  - crates/rich-ext\n    - `markdown_export.rs`\n\n\
         ```sql\nSELECT count(*) FROM runs;\n```\n"
    );
    let (quote, details) = panels();
    assert_eq!(quote, "> **Lint**\n>\n> 3 warnings\\\n> 1 note");
    assert_eq!(
        details,
        "<details><summary>Lint</summary>\n\n3 warnings\\\n1 note\n\n</details>"
    );
    println!("{comment}\n{quote}\n\n{details}");
}

#[test]
fn guide_output() {
    main();
}
