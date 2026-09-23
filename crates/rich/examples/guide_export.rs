//! Guide: Exporting — run: cargo run -p rs-rich --example guide_export [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/export.md are cut from this file. Without
//! `--svg`, the exported files are written to the system temp directory.

#[path = "guide_support/mod.rs"]
mod guide_support;

use std::path::Path;

use guide_support::Shots;

// --8<-- [start:imports]
use rich::r#box::ROUNDED;
use rich::{
    export, svg, ColorSystem, Console, Justify, Table, DEFAULT_TERMINAL_THEME, MONOKAI,
    NIGHT_OWLISH, SVG_EXPORT_THEME,
};
// --8<-- [end:imports]

fn main() {
    let shots = Shots::from_args("guide_export");
    let out = std::env::temp_dir().join("rich-guide-export");
    std::fs::create_dir_all(&out).expect("create the output directory");

    exports(&out).expect("write exports");
    themed(&out).expect("write exports");
    record_once(&out).expect("write exports");

    // The same render under three palettes.
    let console = Shots::pinned_console(48);
    for (name, theme) in [
        ("svg-theme", &SVG_EXPORT_THEME),
        ("monokai", &MONOKAI),
        ("night-owlish", &NIGHT_OWLISH),
    ] {
        let stem = shots.stem(name);
        shots.write(
            &stem,
            console.export_svg_themed(theme, &stem, &stem, report),
        );
    }
    if !shots.is_svg() {
        println!("exports written to {}", out.display());
        report(&Console::new());
    }
}

// --8<-- [start:report]
fn report(console: &Console) {
    console.print_str("[bold]Build report[/] :package:");
    let mut table = Table::new().box_set(ROUNDED);
    table.add_column("Crate");
    table.add_column_justify("Tests", Justify::Right);
    table.add_column("Result");
    table.add_row(&["rs-rich", "312", "pass"]);
    table.add_row(&["rs-rich-ext", "187", "pass"]);
    console.print(&table);
    console.print_str("[green]✓[/] all green in [bold]41[/] s");
}
// --8<-- [end:report]

// --8<-- [start:exports]
fn exports(out: &Path) -> std::io::Result<()> {
    // Pin the console so the export does not depend on the terminal.
    let console = Console::builder()
        .width(60)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();

    // Each export renders the closure, records it, and returns a document.
    // Nothing is written to the terminal.
    let text = console.export_text(report); // plain text
    let html = console.export_html(report); // inline style="…" spans
    let html_classes = console.export_html_classes(report); // <style> + classes
    let svg = console.export_svg("Build report", "build-report", report);

    std::fs::write(out.join("report.txt"), &text)?;
    std::fs::write(out.join("report.html"), &html)?;
    std::fs::write(out.join("report-classes.html"), &html_classes)?;
    std::fs::write(out.join("report.svg"), &svg)?;

    assert!(text.starts_with("Build report"));
    assert!(html.contains("<pre"));
    assert!(svg.starts_with("<svg"));
    Ok(())
}
// --8<-- [end:exports]

// --8<-- [start:themed]
fn themed(out: &Path) -> std::io::Result<()> {
    let console = Console::builder().width(60).build();

    // Choose the palette the abstract colours map to.
    let dark_html = console.export_html_themed(&MONOKAI, report);
    let light_svg = console.export_svg_themed(&DEFAULT_TERMINAL_THEME, "Report", "light", report);

    std::fs::write(out.join("report-monokai.html"), dark_html)?;
    std::fs::write(out.join("report-light.svg"), light_svg)?;
    Ok(())
}
// --8<-- [end:themed]

// --8<-- [start:record]
fn record_once(out: &Path) -> std::io::Result<()> {
    let console = Console::builder()
        .width(60)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();

    // Render once, keep the segments, produce every format from them.
    let segments = console.record_output(report);

    let ansi = console.segments_to_string(&segments); // what the terminal gets
    let html = export::export_html_classes(&segments, &DEFAULT_TERMINAL_THEME);
    let image = svg::export_svg(
        &segments,
        &SVG_EXPORT_THEME,
        "Report",
        "rpt",
        console.width(),
    );

    print!("{ansi}"); // show it, too
    std::fs::write(out.join("once.html"), html)?;
    std::fs::write(out.join("once.svg"), image)?;
    Ok(())
}
// --8<-- [end:record]
