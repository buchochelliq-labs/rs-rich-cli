//! Source semantics and terminal-safe presentation regression tests.
use noctiluca::adapt::Adapt;
use noctiluca::engine::Rect;
use noctiluca::layout::{Marks, Piece, project, reader};
use noctiluca::model::{Page, Revisions, build};
use noctiluca::testing::PageBuilder;

fn heading_page() -> Page {
    let mut p = PageBuilder::new("https://fixture.example/headings", "Headings", 1280., 900.);
    for (i, name) in ["H1", "H2", "H3"].iter().enumerate() {
        let r = Rect::new(0., i as f64 * 54., 320., 36.);
        let h = p.element(
            p.body(),
            name,
            &[],
            &[("color", "rgb(15, 17, 17)")],
            Some(r),
        );
        p.text(h, ["Title", "Brand", "Price"][i], r);
    }
    build(&p.snapshot(), &mut Revisions::new())
}
#[test]
fn neutral_headings_inherit_surface_foreground_in_both_views() {
    let p = heading_page();
    for width in [40, 80, 120, 160] {
        let m = Marks::default();
        let mut lines = project(&p, width, (8., 18.), &m).lines;
        for piece in reader(&p, width, &m) {
            if let Piece::Text(rows) = piece {
                lines.extend(rows);
            }
        }
        for name in ["Title", "Brand", "Price"] {
            let styles: Vec<_> = lines
                .iter()
                .flat_map(|l| &l.spans)
                .filter(|s| s.text.contains(name))
                .collect();
            assert_eq!(styles.len(), 2);
            for s in styles {
                let style = rich::style::Style::parse(&s.style).unwrap();
                assert!(style.color().is_none(), "{name}: {}", s.style);
                assert_eq!(style.bold(), Some(true));
            }
        }
    }
}
fn figure(alt: &str, caption: &str) -> Page {
    let mut p = PageBuilder::new("https://fixture.example/figure", "Figure", 640., 600.);
    let f = p.element(
        p.body(),
        "FIGURE",
        &[],
        &[],
        Some(Rect::new(0., 0., 400., 250.)),
    );
    p.image(f, "photo.png", alt, Rect::new(0., 0., 400., 180.));
    let c = p.element(
        f,
        "FIGCAPTION",
        &[],
        &[],
        Some(Rect::new(0., 180., 400., 54.)),
    );
    p.text(c, caption, Rect::new(0., 180., 400., 54.));
    build(&p.snapshot(), &mut Revisions::new())
}
#[test]
fn figure_caption_must_retain_distinct_alt_meaning() {
    for (alt, caption, equivalent) in [
        ("Blue boat side view", "Blue boat", false),
        ("海の光 blue boat", "海の光 blue boat", true),
        ("Blue  boat", "Blue boat", true),
        ("Boat", "Boat at night", false),
    ] {
        let p = figure(alt, caption);
        assert_eq!(p.media[0].captioned, equivalent, "{alt} / {caption}");
        let lines = project(&p, 80, (8., 18.), &Marks::default()).lines;
        let text = lines
            .iter()
            .map(|l| l.plain())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text.contains("Image:"), !equivalent, "{text}");
        assert!(!p.media[0].alt.is_empty());
        let off = Marks {
            adapt: Adapt::none(),
            ..Marks::default()
        };
        assert!(!project(&p, 80, (8., 18.), &off).lines.is_empty());
    }
}
