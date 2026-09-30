//! Width and layout (#578, #586): an asset takes exactly its columns in
//! measure, wrapping, cropping, tables and panels, whatever draws it; and
//! the renderer seam only ever swaps cells for cells of the same width.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::panel::Panel;
use rich::table::{Cell, Table};
use rich::{Console, Overflow, Renderable, Segment, Text};
use rich_micro::{
    CellSize, FallbackPreference, FallbackRenderer, Layer, MicroAsset, MicroAssetRef, MicroExt,
    MicroMeta, MicroRegistry, MicroRenderer, MicroView, Placement, PAD_CELL,
};

fn registry() -> MicroRegistry {
    let mut registry = MicroRegistry::new();
    let assets = [
        MicroAsset::new("ship", "rocket ship")
            .unwrap()
            .with_emoji("🚀")
            .unwrap()
            .with_text("^^")
            .unwrap(),
        MicroAsset::new("dot", "dot")
            .unwrap()
            .with_size(CellSize::new(1, 1).unwrap())
            .unwrap()
            .with_text("*")
            .unwrap(),
        MicroAsset::new("wide", "wide badge")
            .unwrap()
            .with_size(CellSize::new(3, 1).unwrap())
            .unwrap()
            .with_text("ab")
            .unwrap(),
    ];
    for asset in assets {
        registry.add(Layer::Inline, asset).unwrap();
    }
    registry
}

fn console(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(false)
        .build()
}

/// Rendered lines, as plain strings.
fn lines(console: &Console, renderable: &dyn Renderable) -> Vec<String> {
    console
        .render_lines(renderable, &console.options(), false)
        .into_iter()
        .map(|line| {
            line.iter()
                .filter(|s| !s.control)
                .map(|s| s.text.as_str())
                .collect()
        })
        .collect()
}

fn with(names: &[&str], registry: &MicroRegistry) -> Text {
    let mut text = Text::new("");
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            text.append(" ", None);
        }
        if let Some(asset) = name.strip_prefix(':') {
            text.append_micro(registry, asset).unwrap();
        } else {
            text.append(name, None);
        }
    }
    text
}

#[test]
fn measure_is_the_asset_width() {
    let registry = registry();
    let console = console(80);
    for (name, cols) in [("ship", 2), ("dot", 1), ("wide", 3)] {
        let micro = MicroAssetRef::from_registry(&registry, name).unwrap();
        let m = Measurement::get(&console, &console.options(), &micro);
        assert_eq!((m.minimum, m.maximum), (cols, cols), "{name}");
        let text = micro.text();
        assert_eq!(text.cell_len(), cols, "{name}");
        // Every preference fills the same cells.
        for preference in [
            FallbackPreference::Emoji,
            FallbackPreference::Text,
            FallbackPreference::Alt,
        ] {
            let text = micro.clone().preference(preference).text();
            assert_eq!(text.cell_len(), cols, "{name} {preference:?}");
        }
    }
    // Inside text, an asset is one unbreakable word: the minimum width is at
    // least its columns.
    let text = with(&["a", ":wide", "b"], &registry);
    let m = Measurement::get(&console, &console.options(), &text);
    assert_eq!((m.minimum, m.maximum), (3, 7));
}

#[test]
fn wrapping_never_splits_an_asset() {
    let registry = registry();
    let text = with(
        &["hello", ":wide", "world", ":ship", ":ship", "end"],
        &registry,
    );
    for width in 3..20 {
        let console = console(width);
        let out = lines(&console, &text);
        for line in &out {
            assert!(cell_len(line) <= width, "{width}: {line:?}");
        }
        let joined = out.join("\n");
        assert!(
            joined.contains(&format!("ab{PAD_CELL}")),
            "{width}: {joined}"
        );
        assert_eq!(joined.matches('🚀').count(), 2, "{width}: {joined}");
    }
}

#[test]
fn tables_and_panels_size_the_asset_exactly() {
    let registry = registry();
    let console = console(40);

    let mut table = Table::new();
    table.add_column("A");
    table.add_column("B");
    table.add_row_text(vec![with(&[":ship"], &registry), Text::new("x")]);
    table.add_row_cells(vec![
        Cell::Renderable(Arc::new(
            MicroAssetRef::from_registry(&registry, "wide").unwrap(),
        )),
        Cell::Text(Text::new("y")),
    ]);
    let out = lines(&console, &table);
    // Column A is as wide as its widest cell: the 3-column asset.
    let widths: Vec<usize> = out.iter().map(|l| cell_len(l)).collect();
    assert!(widths.iter().all(|w| *w == widths[0]), "{out:#?}");
    assert_eq!(widths[0], 1 + (1 + 3 + 1) + 1 + (1 + 1 + 1) + 1, "{out:#?}");
    assert!(out.iter().any(|l| l.contains("│ 🚀  │")), "{out:#?}");

    let panel = Panel::fit(Box::new(with(&["go", ":ship"], &registry)));
    let out = lines(&console, &panel);
    assert_eq!(out[1], "│ go 🚀 │", "{out:#?}");
    assert!(out.iter().all(|l| cell_len(l) == 9), "{out:#?}");
}

/// Draws each placement as a fake escape plus blank cells, and counts.
struct Probe {
    calls: AtomicUsize,
}

impl MicroRenderer for Probe {
    fn name(&self) -> &str {
        "probe"
    }
    fn render(&self, placement: &Placement<'_>, _: &Console) -> Option<Vec<Segment>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Some(vec![
            Segment::control(format!("<img {}>", placement.asset.name())),
            Segment::new(
                PAD_CELL.to_string().repeat(placement.meta.cols),
                placement.style.clone(),
            ),
        ])
    }
}

/// Always the wrong width.
struct Wrong;

impl MicroRenderer for Wrong {
    fn name(&self) -> &str {
        "wrong"
    }
    fn render(&self, _: &Placement<'_>, _: &Console) -> Option<Vec<Segment>> {
        Some(vec![Segment::new("toolong", None)])
    }
}

#[test]
fn renderers_swap_cells_for_cells() {
    let registry = Arc::new(registry());
    let console = console(40);
    let text = with(&["go", ":ship", ":dot"], &registry);
    let probe = Arc::new(Probe {
        calls: AtomicUsize::new(0),
    });
    let view = MicroView::new(text.clone(), Arc::clone(&registry))
        .renderer(Arc::new(Wrong))
        .renderer(probe.clone());
    let segments = view.rich_render(&console, &console.options());
    assert_eq!(probe.calls.load(Ordering::Relaxed), 2);
    let controls: Vec<&str> = segments
        .iter()
        .filter(|s| s.control)
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(controls, ["<img ship>", "<img dot>"]);
    // Same width as the fallback it replaced, line for line.
    let plain = text.rich_render(&console, &console.options());
    let width = |segments: &[Segment]| segments.iter().map(Segment::cell_length).sum::<usize>();
    assert_eq!(width(&segments), width(&plain));
    // Visible cells still carry the tag.
    assert!(segments
        .iter()
        .any(|s| !s.control && s.style.as_ref().and_then(MicroMeta::from_style).is_some()));

    // Without renderers, or with the plain fallback renderer, nothing changes
    // but the chosen fallback.
    let bare = MicroView::new(text.clone(), Arc::clone(&registry));
    assert_eq!(bare.rich_render(&console, &console.options()), plain);
    let text_only = MicroView::new(text, Arc::clone(&registry))
        .renderer(Arc::new(FallbackRenderer::new(FallbackPreference::Text)));
    let out: String = text_only
        .rich_render(&console, &console.options())
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(out.trim_end(), "go ^^ *");
}

#[test]
fn a_cut_placement_keeps_its_fallback() {
    let registry = Arc::new(registry());
    let console = console(4);
    // Cropped mid-asset: `ab⠀` cut to `ab` at width 4 after "xy ".
    let mut text = with(&["xy", ":wide"], &registry);
    text.set_no_wrap(Some(true));
    text.set_overflow(Some(Overflow::Crop));
    let probe = Arc::new(Probe {
        calls: AtomicUsize::new(0),
    });
    let view = MicroView::new(text, Arc::clone(&registry)).renderer(probe.clone());
    let segments = view.rich_render(&console, &console.options());
    assert_eq!(probe.calls.load(Ordering::Relaxed), 0);
    let out: String = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(out.trim_end(), "xy a");
}

#[test]
fn exports_hold_the_fallback_only() {
    let registry = registry();
    let console = Console::builder().width(40).force_terminal(true).build();
    let text = with(&["deploy", ":ship"], &registry);
    assert_eq!(console.export_text(|c| c.print(&text)), "deploy 🚀\n");
    // Unstyled text on a terminal: no escape sequences at all.
    assert_eq!(console.capture(|c| c.print(&text)), "deploy 🚀\n");
    assert!(console
        .export_html(|c| c.print(&text))
        .contains("deploy 🚀"));
}
