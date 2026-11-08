//! Reader mode and the projection, on snapshots Chrome made of the
//! fixture pages: no browser needed.

use noctiluca::layout::{Line, Marks, Piece, projection, reader};
use noctiluca::model::{Page, Revisions, build};
use noctiluca::testing::snapshot_file;

fn page(name: &str) -> Page {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/snapshots/{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn text_of(pieces: &[Piece]) -> Vec<String> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text(lines) => out.extend(lines.iter().map(Line::plain)),
            Piece::Media { index, rows } => out.push(format!("<media {index} x{rows}>")),
            Piece::Controls(c) => out.push(format!("<controls {c:?}>")),
            Piece::Player(i) => out.push(format!("<player {i}>")),
        }
    }
    out
}

#[test]
fn reader_mode_reads_a_docs_page_with_its_code_as_text() {
    let lines = text_of(&reader(&page("docs"), 80, &Marks::default()));
    let at = |text: &str| {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{text}: {lines:#?}"))
    };
    assert!(at("Struct glow::Lamp") < at("Method new"));
    // Code keeps its lines and indentation, as text to copy.
    assert!(
        lines.contains(&"▏pub fn new(brightness: u8) -> Lamp {".to_string()),
        "{lines:#?}"
    );
    assert!(
        lines.contains(&"▏    Lamp { brightness, lit: false }".to_string()),
        "{lines:#?}"
    );
    assert!(at("Field") < at("brightness │ u8"));
}

#[test]
fn reader_mode_reads_an_article_with_its_image_inline() {
    let pieces = reader(&page("article"), 80, &Marks::default());
    let lines = text_of(&pieces);
    let at = |text: &str| {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{text}: {lines:#?}"))
    };
    assert!(at("The glowing coast") < at("image: Blue waves at night"));
    assert!(at("image: Blue waves at night") < at("<media 0"));
    assert!(at("<media 0") < at("Last night the bay lit up blue"));
}

#[test]
fn the_projection_keeps_columns_side_by_side() {
    // page.html's two sections sit in a flex row.
    let lines: Vec<String> = projection(&page("page"), 100, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    let row = lines.iter().find(|l| l.contains("Where it lives")).unwrap();
    assert!(row.contains("Escape test"), "not side by side: {lines:#?}");
    let left = row.find("Where it lives").unwrap();
    let right = row.find("Escape test").unwrap();
    assert!(left < right);
}

#[test]
fn the_projection_never_puts_two_texts_in_one_cell_and_keeps_reading_order() {
    for name in ["reader", "docs", "article", "form", "page"] {
        let page = page(name);
        let lines = projection(&page, 100, (8.0, 18.0), &Marks::default());
        let text: Vec<String> = lines.iter().map(Line::plain).collect();
        // Every block's text is there, in the order Chrome laid it out
        // (top first) within the page's single column of headings.
        let headings: Vec<String> = page
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, noctiluca::model::BlockKind::Heading(_)))
            .map(|b| b.text().replace('\n', " "))
            .collect();
        let mut last = 0;
        for heading in &headings {
            let first_word = heading.split(' ').next().unwrap();
            let at = text
                .iter()
                .position(|l| l.contains(first_word))
                .unwrap_or_else(|| panic!("{name}: {heading}"));
            assert!(
                at >= last || name == "page",
                "{name}: {heading} out of order"
            );
            last = at;
        }
        // No line is wider than the canvas.
        assert!(lines.iter().all(|l| l.width() <= 100), "{name}");
    }
}

/// A page of one paragraph saying `text`, at `y` CSS pixels down, 80 pixels
/// wide (ten cells of 8).
fn one_paragraph(text: &str, y: f64) -> Page {
    use noctiluca::engine::Rect;
    use noctiluca::model::{Block, BlockKind, Run};
    let mut page = page("reader");
    let mut block = page
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Paragraph)
        .cloned()
        .unwrap();
    block.runs = vec![Run::plain(text)];
    block.rect = Rect::new(0.0, y, 80.0, 18.0);
    block.frame = block.rect;
    page.blocks = vec![Block { ..block }];
    page.media.clear();
    page
}

#[test]
fn a_combining_mark_at_the_edge_joins_its_letter() {
    // Ten letters fill the ten columns; the accent is the tenth's.
    let page = one_paragraph("abcdefghij\u{0301}", 0.0);
    let lines = projection(&page, 10, (8.0, 18.0), &Marks::default());
    assert_eq!(lines[0].plain(), "abcdefghij\u{0301}");
    // Away from the edge it stays on its letter too.
    let page = one_paragraph("ae\u{0301}x", 0.0);
    let lines = projection(&page, 10, (8.0, 18.0), &Marks::default());
    assert_eq!(lines[0].plain(), "ae\u{0301}x");
}

#[test]
fn a_block_far_down_the_page_does_not_grow_the_projection() {
    // Thirty million pixels down: about 1.7 million rows at 18 pixels.
    let page = one_paragraph("far away", 30_000_000.0);
    let lines = projection(&page, 10, (8.0, 18.0), &Marks::default());
    assert!(lines.len() <= 300, "{} rows", lines.len());
    assert_eq!(lines.last().unwrap().plain(), "far away");
}

#[test]
fn a_tables_rank_column_keeps_each_rank_beside_its_title() {
    // rank-table.html, reduced from Hacker News: from rank 10 the rank is
    // wider than its narrow right-aligned cell. It must not wrap its period
    // onto a row of its own, and each rank stays on its title's row, even
    // where rounding puts a story's subtext and the next story on one row.
    let lines: Vec<String> = projection(&page("rank-table"), 120, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    let titles = [
        (1, "Lanterns on the harbour wall"),
        (9, "How the quay was rebuilt"),
        (10, "Counting stars over the lighthouse"),
        (11, "Ten years of shore lights"),
        (12, "A history of the old pier"),
    ];
    for (rank, title) in titles {
        let row = lines
            .iter()
            .find(|l| l.contains(title))
            .unwrap_or_else(|| panic!("{title}: {lines:#?}"));
        assert!(
            row.trim_start().starts_with(&format!("{rank}. ")),
            "rank {rank} not beside its title: {lines:#?}"
        );
    }
    // No row holds a lone period.
    assert!(!lines.iter().any(|l| l.trim() == "."), "{lines:#?}");
}

#[test]
fn a_snapped_grid_item_keeps_its_links_readable() {
    // narrow-grid.html, reduced from Wikipedia's header: snapping the grid
    // to equal columns narrows its last item. Its links keep their words,
    // never one letter to a row.
    let lines: Vec<String> = projection(&page("narrow-grid"), 120, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    let text = lines.join("\n");
    for word in ["Donate", "Create", "account", "Log"] {
        assert!(
            lines
                .iter()
                .any(|l| l.split_whitespace().any(|w| w == word)),
            "{word} broken: {text}"
        );
    }
    let short = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && t.chars().count() <= 2
        })
        .count();
    assert_eq!(short, 0, "one-letter rows: {text}");
}

#[test]
fn a_captioned_picture_still_loading_shows_its_alt_text_once() {
    // A picture with no caption of its own gets "Image: alt" under it; its
    // box, until the picture comes, no longer says the alt text too (The
    // Guardian's pictures showed it twice).
    let lines: Vec<String> = projection(&page("news"), 100, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    let alt = "Boats at anchor";
    let count = lines.iter().filter(|l| l.contains(alt)).count();
    assert!(
        lines.iter().any(|l| l.contains(&format!("Image: {alt}"))),
        "no caption: {lines:#?}"
    );
    assert_eq!(count, 1, "alt text shown {count} times: {lines:#?}");
}

#[test]
fn a_tables_rows_keep_their_order_among_blocks_on_the_same_terminal_row() {
    // Rows a few pixels apart round to one terminal row; their cells
    // alternate between the right and the left, with paragraphs between
    // them. The reading order must stay one total order (the sort used to
    // compare a table's rows by index but everything else by position,
    // which is not transitive and can make the sort panic), and every row
    // must come after the one before it.
    use noctiluca::engine::Rect;
    use noctiluca::model::{BlockKind, Cell, Run, Table};
    let mut page = page("reader");
    let template = page
        .blocks
        .iter()
        .find(|b| b.kind == BlockKind::Paragraph)
        .cloned()
        .unwrap();
    let rows: Vec<Vec<Cell>> = (0..40)
        .map(|i| {
            let x = if i % 2 == 0 { 400.0 } else { 0.0 };
            vec![Cell {
                runs: vec![Run::plain(format!("row{i:02}"))],
                header: false,
                rect: Rect::new(x, 4.0 * i as f64, 60.0, 4.0),
            }]
        })
        .collect();
    let mut table = template.clone();
    table.kind = BlockKind::Table(Table {
        caption: String::new(),
        rows,
    });
    table.rect = Rect::new(0.0, 0.0, 480.0, 160.0);
    table.frame = table.rect;
    let mut blocks = vec![table];
    for i in 0..20 {
        let mut paragraph = template.clone();
        paragraph.runs = vec![Run::plain(format!("para{i:02}"))];
        paragraph.rect = Rect::new(200.0, 8.0 * i as f64 + 2.0, 60.0, 4.0);
        paragraph.frame = paragraph.rect;
        blocks.push(paragraph);
    }
    page.media.clear();
    let render = |blocks: Vec<_>| {
        let mut page = page.clone();
        page.blocks = blocks;
        projection(&page, 120, (8.0, 18.0), &Marks::default())
            .iter()
            .map(Line::plain)
            .collect::<Vec<String>>()
    };
    let lines = render(blocks.clone());
    let text = lines.join("\n");
    // One total order reads the same wherever the table sits among the
    // paragraphs (paragraphs at one position keep their document order).
    for at in [10, 20] {
        let mut moved = blocks.clone();
        let table = moved.remove(0);
        moved.insert(at, table);
        assert_eq!(render(moved).join("\n"), text, "order depends on the input");
    }
    // Rows sharing a terminal row may share a line, in any column; a row is
    // never on a line above the row before it.
    let mut last = 0;
    for i in 0..40 {
        let name = format!("row{i:02}");
        let at = lines
            .iter()
            .position(|line| line.contains(&name))
            .unwrap_or_else(|| panic!("{name} missing: {text}"));
        assert!(at >= last, "{name} above the row before it: {text}");
        last = at;
    }
}

#[test]
fn a_cell_belongs_to_the_control_its_span_is() {
    use noctiluca::layout::{Line, Span, control_at};
    let link = Span {
        control: Some(3),
        ..Span::new("open sea", "underline")
    };
    let line = Line {
        spans: vec![Span::new("the ", ""), link, Span::new(" — 海", "")],
    };
    assert_eq!(control_at(&line, 3), None);
    assert_eq!(
        (control_at(&line, 4), control_at(&line, 11)),
        (Some(3), Some(3))
    );
    assert_eq!(control_at(&line, 12), None);
    // Wide characters count their cells; past the end is nothing.
    assert_eq!(control_at(&line, 16), None);
    assert_eq!(control_at(&line, 40), None);
}

/// The page in `tests/fixtures/caption-grid.html` as Chrome lays it out
/// at 800 CSS pixels (100 columns): a grid of a wide story (528 pixels)
/// and a narrow one (264), the wide one's picture 297 pixels tall with
/// long alt text and no caption of the page's.
fn caption_grid() -> Page {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/caption-grid.html",
        "Caption grid fixture",
        800.0,
        400.0,
    );
    let body = p.body();
    let row = p.element(
        body,
        "DIV",
        &[("class", "row")],
        &[("display", "grid")],
        Some(Rect::new(0.0, 0.0, 800.0, 333.0)),
    );
    let lead = p.element(
        row,
        "DIV",
        &[("class", "lead")],
        &[],
        Some(Rect::new(0.0, 0.0, 528.0, 333.0)),
    );
    p.image(
        lead,
        "file:///noctiluca/fixtures/media/harbour.jpg",
        "Boats rest in the harbour at dusk while the tide turns and the first lights of the town come on along the sea wall",
        Rect::new(0.0, 0.0, 528.0, 297.0),
    );
    let h2 = p.element(
        lead,
        "H2",
        &[],
        &[("font-weight", "700")],
        Some(Rect::new(0.0, 297.0, 528.0, 36.0)),
    );
    p.text(
        h2,
        "The harbour at night",
        Rect::new(0.0, 306.0, 190.0, 18.0),
    );
    let side = p.element(
        row,
        "DIV",
        &[("class", "side")],
        &[],
        Some(Rect::new(536.0, 0.0, 264.0, 80.0)),
    );
    let h3 = p.element(
        side,
        "H3",
        &[],
        &[("font-weight", "700")],
        Some(Rect::new(536.0, 0.0, 264.0, 18.0)),
    );
    p.text(h3, "Tides this week", Rect::new(536.0, 0.0, 130.0, 18.0));
    let para = p.element(
        side,
        "P",
        &[],
        &[],
        Some(Rect::new(536.0, 34.0, 264.0, 36.0)),
    );
    p.text(
        para,
        "High water comes an hour later each day.",
        Rect::new(536.0, 34.0, 250.0, 36.0),
    );
    let under = p.element(
        body,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 349.0, 800.0, 18.0)),
    );
    p.text(under, "Under the row.", Rect::new(0.0, 349.0, 110.0, 18.0));
    build(&p.snapshot(), &mut Revisions::new())
}

#[test]
fn a_pictures_caption_in_a_narrowed_grid_item_wraps_and_shows_in_full() {
    let page = caption_grid();
    let lines: Vec<String> = projection(&page, 100, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    // The caption's words, in order, over as many rows as it takes: none
    // cut at the narrowed item's edge.
    let caption: String = lines
        .iter()
        .filter(|l| !l.contains('░'))
        .map(|l| l.split("  ").next().unwrap_or_default().trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        caption.contains(
            "Image: Boats rest in the harbour at dusk while the tide turns and the first \
             lights of the town come on along the sea wall"
        ),
        "{lines:#?}"
    );
    // The story beside it keeps its column, and what is under the row
    // comes after the caption.
    let at = |text: &str| {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{text}: {lines:#?}"))
    };
    assert!(at("Tides this week") < at("sea wall"));
    assert!(at("sea wall") < at("Under the row."));
}

/// The page in `tests/fixtures/gaps.html` as Chrome lays it out at 800
/// CSS pixels: a header, an empty 216-pixel ad slot, the first story, a
/// sidebar of six links beside one short paragraph, an empty 180-pixel
/// spacer, and a last paragraph.
fn gaps() -> Page {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/gaps.html",
        "Gaps fixture",
        800.0,
        700.0,
    );
    let body = p.body();
    let header = p.element(
        body,
        "HEADER",
        &[],
        &[("display", "flex")],
        Some(Rect::new(0.0, 0.0, 800.0, 36.0)),
    );
    let menu = p.element(
        header,
        "A",
        &[("href", "#menu")],
        &[("display", "inline")],
        Some(Rect::new(8.0, 9.0, 80.0, 18.0)),
    );
    p.text(menu, "Open menu", Rect::new(8.0, 9.0, 80.0, 18.0));
    let live = p.element(
        header,
        "A",
        &[("href", "#live")],
        &[("display", "inline")],
        Some(Rect::new(704.0, 9.0, 88.0, 18.0)),
    );
    p.text(live, "Watch Live", Rect::new(704.0, 9.0, 88.0, 18.0));
    p.element(
        body,
        "DIV",
        &[("class", "slot")],
        &[],
        Some(Rect::new(0.0, 36.0, 800.0, 216.0)),
    );
    let h2 = p.element(
        body,
        "H2",
        &[],
        &[("font-weight", "700")],
        Some(Rect::new(0.0, 270.0, 800.0, 18.0)),
    );
    p.text(h2, "The first story", Rect::new(0.0, 270.0, 140.0, 18.0));
    let para = p.element(
        body,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 306.0, 800.0, 18.0)),
    );
    p.text(
        para,
        "Its first paragraph.",
        Rect::new(0.0, 306.0, 150.0, 18.0),
    );
    let columns = p.element(
        body,
        "DIV",
        &[("class", "columns")],
        &[("display", "flex")],
        Some(Rect::new(0.0, 342.0, 800.0, 108.0)),
    );
    let aside = p.element(
        columns,
        "ASIDE",
        &[],
        &[],
        Some(Rect::new(0.0, 342.0, 200.0, 108.0)),
    );
    for (k, name) in ["One", "Two", "Three", "Four", "Five", "Six"]
        .iter()
        .enumerate()
    {
        let y = 342.0 + 18.0 * k as f64;
        let href = format!("#{k}");
        let item = p.element(
            aside,
            "LI",
            &[],
            &[("display", "list-item")],
            Some(Rect::new(0.0, y, 200.0, 18.0)),
        );
        let a = p.element(
            item,
            "A",
            &[("href", href.as_str())],
            &[("display", "inline")],
            Some(Rect::new(0.0, y, 40.0, 18.0)),
        );
        p.text(a, name, Rect::new(0.0, y, 40.0, 18.0));
    }
    let main = p.element(
        columns,
        "MAIN",
        &[],
        &[],
        Some(Rect::new(200.0, 342.0, 600.0, 34.0)),
    );
    let beside = p.element(
        main,
        "P",
        &[],
        &[],
        Some(Rect::new(200.0, 342.0, 600.0, 18.0)),
    );
    p.text(
        beside,
        "Beside the sidebar.",
        Rect::new(200.0, 342.0, 150.0, 18.0),
    );
    p.element(
        body,
        "DIV",
        &[("class", "spacer")],
        &[],
        Some(Rect::new(0.0, 450.0, 800.0, 180.0)),
    );
    let last = p.element(
        body,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 646.0, 800.0, 18.0)),
    );
    p.text(
        last,
        "After the spacer.",
        Rect::new(0.0, 646.0, 140.0, 18.0),
    );
    build(&p.snapshot(), &mut Revisions::new())
}

#[test]
fn rows_blank_in_every_column_collapse_and_rows_beside_a_sidebar_stay() {
    use noctiluca::adapt::{Adapt, Rule};
    let page = gaps();
    let lines_with = |marks: &Marks| -> Vec<String> {
        projection(&page, 100, (8.0, 18.0), marks)
            .iter()
            .map(Line::plain)
            .collect()
    };
    let lines = lines_with(&Marks::default());
    let at = |lines: &[String], text: &str| {
        lines
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("{text}: {lines:#?}"))
    };
    // The empty ad slot and the spacer take one blank row each.
    assert_eq!(
        at(&lines, "The first story"),
        at(&lines, "Open menu") + 2,
        "{lines:#?}"
    );
    assert_eq!(
        at(&lines, "After the spacer"),
        at(&lines, "Six") + 2,
        "{lines:#?}"
    );
    // The paragraph beside the sidebar stays on the sidebar's first row,
    // and the sidebar keeps its six rows, though only one has main text.
    assert_eq!(at(&lines, "Beside the sidebar"), at(&lines, "One"));
    assert_eq!(at(&lines, "Six"), at(&lines, "One") + 5);
    // Off, the rows stay where Chrome put them.
    let faithful = lines_with(&Marks {
        adapt: Adapt::default().with(Rule::CollapseGaps, false),
        ..Marks::default()
    });
    assert!(
        at(&faithful, "The first story") >= at(&faithful, "Open menu") + 12,
        "{faithful:#?}"
    );
}

/// The projection of a saved snapshot at `columns`: the page's lines, its
/// pinned strip's and its docked strip's.
fn strips(name: &str, columns: u16) -> (Vec<String>, Vec<String>, Vec<String>) {
    use noctiluca::layout::project;
    let projection = project(&page(name), columns, (8.0, 18.0), &Marks::default());
    let plain = |lines: &[Line]| lines.iter().map(Line::plain).collect::<Vec<_>>();
    (
        plain(&projection.lines),
        projection
            .pinned
            .as_ref()
            .map_or(Vec::new(), |p| plain(&p.lines)),
        projection
            .docked
            .as_ref()
            .map_or(Vec::new(), |p| plain(&p.lines)),
    )
}

#[test]
fn a_sticky_header_read_while_scrolled_is_pinned_not_mid_page() {
    // search-results.html read with Chrome scrolled to its bottom (as a
    // short page was by the view's "near the end"): the sticky header is
    // reported where it is stuck, 327 pixels down, under the first result.
    let (lines, pinned, _) = strips("search-results-scrolled", 100);
    assert!(
        pinned.iter().any(|l| l.contains("Search ‹tide tables ▾›")),
        "{pinned:#?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("tide tables ▾")),
        "{lines:#?}"
    );
    // The page starts with what is under the header: the result tabs.
    let first = lines.iter().position(|l| !l.trim().is_empty()).unwrap();
    assert!(lines[first].contains("All Images News"), "{lines:#?}");
}

#[test]
fn a_source_dialog_at_the_bottom_is_projected_separately_and_header_stays_pinned() {
    let page = page("product-grid-scrolled");
    let projection = noctiluca::layout::project(&page, 100, (8.0, 18.0), &Marks::default());
    let lines = projection
        .lines
        .iter()
        .map(Line::plain)
        .collect::<Vec<_>>()
        .join("\n");
    let pinned = projection
        .pinned
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .map(Line::plain)
        .collect::<Vec<_>>()
        .join("\n");
    let popup = page
        .overlays
        .iter()
        .enumerate()
        .filter_map(|(index, _)| {
            noctiluca::layout::project_overlay(&page, index, 100, (8.0, 18.0), &Marks::default())
        })
        .map(|p| {
            p.lines
                .iter()
                .map(Line::plain)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .find(|text| text.contains("We use cookies"))
        .expect("source dialog was not projected separately");
    assert!(popup.contains("[Accept]"), "{popup}");
    assert!(!lines.contains("We use cookies"), "{lines}");
    assert!(pinned.contains("Search the shop"), "{pinned}");
    assert!(lines.contains("1-3 of over 2,000"), "{lines}");
}

#[test]
fn badges_over_a_picture_stay_in_its_cells() {
    use noctiluca::layout::project;
    let page = page("overlays");
    let projection = project(&page, 100, (8.0, 18.0), &Marks::default());
    let lines: Vec<String> = projection.lines.iter().map(Line::plain).collect();
    let region = |alt: &str| {
        let m = page
            .media
            .iter()
            .position(|m| m.alt.starts_with(alt))
            .unwrap();
        *projection.regions.iter().find(|r| r.media == m).unwrap()
    };
    let inside = |text: &str, r: noctiluca::layout::Region| {
        (r.row..r.row + r.rows).any(|row| {
            // In cells, not bytes (the shading is three bytes a cell).
            lines[row].find(text).is_some_and(|at| {
                let at = lines[row][..at].chars().count();
                at >= r.column && at + text.len() <= r.column + r.cols
            })
        })
    };
    let past = region("Harbour at dusk");
    let live = region("Radar sweep");
    assert!(inside("1:02:33", past), "{lines:#?}");
    assert!(inside("LIVE", live), "{lines:#?}");
    assert!(inside("412 viewers", live), "{lines:#?}");
    // The duration at the picture's bottom right, LIVE at its top left.
    assert!(lines[past.row + past.rows - 1].contains("1:02:33"));
    assert!(lines[live.row].contains("LIVE"));
    // They take no rows of their own under it: its caption follows.
    assert!(lines[past.row + past.rows].contains("Image: Harbour at dusk"));
    assert_eq!(projection.overlays.len(), 3, "{:?}", projection.overlays);
}

#[test]
fn art_over_a_player_takes_none_of_its_rows() {
    use noctiluca::layout::project;
    let page = page("overlays");
    let projection = project(&page, 100, (8.0, 18.0), &Marks::default());
    let art = page
        .media
        .iter()
        .position(|m| m.alt == "Channel offline art")
        .unwrap();
    let video = page
        .media
        .iter()
        .position(|m| m.kind == noctiluca::model::MediaKind::Video)
        .unwrap();
    assert!(projection.regions.iter().all(|r| r.media != art));
    let player = projection
        .regions
        .iter()
        .find(|r| r.media == video)
        .unwrap();
    // The thumbnails start under the player, not under the art too.
    let next = projection
        .regions
        .iter()
        .find(|r| r.row > player.row)
        .unwrap();
    assert!(
        next.row <= player.row + player.rows + 1,
        "{:?}",
        projection.regions
    );
}

#[test]
fn a_panel_under_an_opaque_layer_painted_above_it_is_not_drawn() {
    let (lines, _, docked) = strips("overlays", 100);
    let all = lines.join("\n");
    assert!(!all.contains("An error occurred"), "{all}");
    assert!(all.contains("This video is for viewers"), "{all}");
    assert!(all.contains("[Start watching]"), "{all}");
    assert!(docked.join(" ").contains("Join the Streams"), "{docked:#?}");
}

#[test]
fn icon_buttons_at_the_right_edge_shorten_rather_than_being_cut() {
    for columns in [100, 64, 50] {
        let (_, pinned, _) = strips("overlays", columns);
        let bar = &pinned[0];
        assert!(rich::cells::cell_len(bar) <= columns as usize, "{bar}");
        // Every face is whole or shortened with `…`, its bracket kept.
        for name in ["Prime", "Log", "Sign", "User"] {
            let at = bar
                .find(&format!("[{name}"))
                .unwrap_or_else(|| panic!("{bar}"));
            let face = &bar[at..at + bar[at..].find(']').unwrap_or_else(|| panic!("{bar}")) + 1];
            assert!(!face[1..].contains('['), "{name} in {bar}");
        }
    }
    let (_, pinned, _) = strips("overlays", 50);
    assert!(pinned[0].contains('…'), "{pinned:#?}");
}

#[test]
fn two_runs_that_touch_in_cells_keep_a_space_between_them() {
    use noctiluca::engine::Rect;
    use noctiluca::testing::PageBuilder;
    // "16" views and "13y ago", 2 pixels apart in Chrome: snapped, the
    // first fills its cells up to where the second starts.
    let mut p = PageBuilder::new("https://fixture.example/meta.html", "Meta", 800.0, 100.0);
    let body = p.body();
    let views = p.element(
        body,
        "DIV",
        &[],
        &[],
        Some(Rect::new(100.0, 40.0, 16.0, 18.0)),
    );
    p.text(views, "16", Rect::new(100.0, 40.0, 16.0, 18.0));
    let age = p.element(
        body,
        "DIV",
        &[],
        &[],
        Some(Rect::new(118.0, 40.0, 50.0, 18.0)),
    );
    p.text(age, "13y ago", Rect::new(118.0, 40.0, 50.0, 18.0));
    let page = build(&p.snapshot(), &mut Revisions::new());
    let lines: Vec<String> = projection(&page, 100, (8.0, 18.0), &Marks::default())
        .iter()
        .map(Line::plain)
        .collect();
    assert!(lines.iter().any(|l| l.contains("16 13y ago")), "{lines:#?}");
}

#[test]
fn opaque_tall_fixed_dialog_does_not_mix_covered_underlay() {
    use noctiluca::engine::Rect;
    use noctiluca::layout::project;
    use noctiluca::testing::PageBuilder;
    let mut p = PageBuilder::new("https://example.test/layers", "Layers", 960.0, 720.0);
    p.view(0.0, 720.0);
    let body = p.body();
    let under = p.element(
        body,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(384.0, 272.0, 176.0, 40.0)),
    );
    p.paint(under, 10);
    p.text(
        under,
        "Underlying action",
        Rect::new(390.0, 278.0, 150.0, 18.0),
    );
    let panel = p.element(
        body,
        "DIV",
        &[],
        &[
            ("position", "fixed"),
            ("background-color", "rgb(55, 57, 82)"),
        ],
        Some(Rect::new(196.0, 72.0, 567.0, 532.0)),
    );
    p.paint(panel, 100);
    let dialog = p.element(
        panel,
        "DIV",
        &[("role", "dialog")],
        &[],
        Some(Rect::new(196.0, 72.0, 567.0, 532.0)),
    );
    let text = p.element(
        dialog,
        "P",
        &[],
        &[],
        Some(Rect::new(210.0, 272.0, 220.0, 18.0)),
    );
    p.paint(text, 101);
    p.text(text, "Dialog content", Rect::new(210.0, 272.0, 200.0, 18.0));
    let page = build(&p.snapshot(), &mut Revisions::new());
    let projection = project(&page, 120, (8.0, 18.0), &Marks::default());
    let text = projection
        .lines
        .iter()
        .map(Line::plain)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(page.overlays.len(), 1);
    assert!(!page.overlays[0].modal);
    assert!(
        text.contains("Underlying action"),
        "source underlay must remain in the page projection: {text}"
    );
    assert!(
        !text.contains("Dialog content"),
        "dialog remained in page flow: {text}"
    );
    let popup =
        noctiluca::layout::project_overlay(&page, 0, 70, (8.0, 18.0), &Marks::default()).unwrap();
    let popup_text = popup
        .lines
        .iter()
        .map(Line::plain)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(popup_text.contains("Dialog content"), "{popup_text}");
    assert!(!popup_text.contains("Underlying action"), "{popup_text}");
}

fn source_dialog_layer(modal: bool, higher: bool) {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        engine::{PageAction, Rect},
        testing::{FakeEngine, PageBuilder},
    };
    use std::time::{Duration, Instant};
    let mut p = PageBuilder::new("https://example.test/popup.html", "Popup", 960.0, 720.0);
    p.view(0.0, 720.0);
    let body = p.body();
    let outside = p.element(
        body,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(0.0, 0.0, 160.0, 30.0)),
    );
    p.text(outside, "Outside", Rect::new(4.0, 4.0, 100.0, 18.0));
    let behind = p.element(
        body,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(384.0, 272.0, 176.0, 40.0)),
    );
    p.paint(behind, 10);
    p.text(
        behind,
        "Underlying action",
        Rect::new(390.0, 278.0, 150.0, 18.0),
    );
    let panel = p.element(
        body,
        "DIV",
        &[],
        &[
            ("position", "fixed"),
            ("background-color", "rgb(55, 57, 82)"),
        ],
        Some(Rect::new(196.0, 72.0, 567.0, 532.0)),
    );
    p.paint(panel, 100);
    let attrs = if modal {
        vec![("role", "dialog"), ("aria-modal", "true")]
    } else {
        vec![("role", "dialog")]
    };
    let dialog = p.element(
        panel,
        "DIV",
        &attrs,
        &[],
        Some(Rect::new(196.0, 72.0, 567.0, 532.0)),
    );
    let text = p.element(
        dialog,
        "P",
        &[],
        &[],
        Some(Rect::new(210.0, 100.0, 220.0, 18.0)),
    );
    p.text(text, "Dialog content", Rect::new(210.0, 100.0, 200.0, 18.0));
    let action = p.element(
        dialog,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(210.0, 140.0, 160.0, 30.0)),
    );
    p.text(action, "Choose", Rect::new(215.0, 145.0, 80.0, 18.0));
    let other = p.element(
        dialog,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(410.0, 140.0, 160.0, 30.0)),
    );
    p.text(other, "Second", Rect::new(415.0, 145.0, 80.0, 18.0));
    if !higher && !modal {
        for i in 0..45 {
            let y = 180.0 + f64::from(i) * 18.0;
            let row = p.element(
                dialog,
                "P",
                &[],
                &[],
                Some(Rect::new(210.0, y, 200.0, 18.0)),
            );
            p.text(
                row,
                &format!("Popup row {i}"),
                Rect::new(210.0, y, 180.0, 18.0),
            );
        }
        let deep = p.element(
            dialog,
            "BUTTON",
            &[],
            &[],
            Some(Rect::new(210.0, 1000.0, 150.0, 30.0)),
        );
        p.text(deep, "Deep action", Rect::new(210.0, 1000.0, 130.0, 18.0));
    }
    if higher {
        let over = p.element(
            body,
            "BUTTON",
            &[],
            &[],
            Some(Rect::new(700.0, 180.0, 55.0, 30.0)),
        );
        p.paint(over, 200);
        p.text(over, "Above", Rect::new(700.0, 180.0, 50.0, 18.0));
        let incoming = p.element(
            body,
            "BUTTON",
            &[],
            &[],
            Some(Rect::new(700.0, 900.0, 70.0, 30.0)),
        );
        p.paint(incoming, 201);
        p.text(incoming, "Incoming", Rect::new(700.0, 900.0, 65.0, 18.0));
        for i in 0..35 {
            let y = 740.0 + f64::from(i) * 30.0;
            let row = p.element(body, "P", &[], &[], Some(Rect::new(0.0, y, 140.0, 18.0)));
            p.text(
                row,
                &format!("Later row {i}"),
                Rect::new(0.0, y, 120.0, 18.0),
            );
        }
    }
    let model = build(&p.snapshot(), &mut Revisions::new());
    let higher_node = higher.then(|| model.find("button", "Above")[0].node);
    let incoming_node = higher.then(|| model.find("button", "Incoming")[0].node);
    let higher_hint = higher_node.map(|node| {
        let controls = model
            .controls
            .iter()
            .filter(|control| !control.disabled && !control.hover_only)
            .collect::<Vec<_>>();
        let index = controls
            .iter()
            .position(|control| control.node == node)
            .unwrap();
        noctiluca::layout::hint_labels(controls.len())[index].clone()
    });
    let lower_node = model.find("button", "Underlying action")[0].node;
    assert!(
        !model.overlays[0].foreground.contains(&lower_node),
        "lower-painted control was promoted"
    );
    if let Some(node) = higher_node {
        assert!(
            model.overlays[0].foreground.contains(&node),
            "higher-painted control was not associated"
        );
    }
    let expected = model.find("button", "Second")[0].node;
    let choose = model.find("button", "Choose")[0].node;
    let deep = (!higher && !modal).then(|| model.find("button", "Deep action")[0].node);
    let hint_for = |node| {
        let controls = model
            .controls
            .iter()
            .filter(|c| !c.disabled && !c.hover_only)
            .collect::<Vec<_>>();
        let index = controls.iter().position(|c| c.node == node).unwrap();
        noctiluca::layout::hint_labels(controls.len())[index].clone()
    };
    let choose_hint = hint_for(choose);
    let deep_hint = deep.map(hint_for);
    let outside_node = model.find("button", "Outside")[0].node;
    let visible = p.snapshot();
    p.set_style(panel, "opacity", "0");
    let hidden = p.snapshot();
    assert!(
        build(&hidden, &mut Revisions::new()).overlays.is_empty(),
        "hidden source still declared a dialog"
    );
    let engine = FakeEngine::from_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
    )
    .unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("popup".into(), visible.clone());
    let mut driver = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("popup.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(120, 40);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        driver.settle();
        if driver.find_text("Choose").is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "popup not shown: {}",
            driver.screen().plain().join("\n")
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(driver.find_text("Dialog content").is_some());
    assert!(driver.find_text("Underlying action").is_none());
    if let Some(deep_node) = deep {
        driver.press("f");
        driver.settle();
        assert!(
            driver.find_text(&format!("[{choose_hint}]")).is_some(),
            "popup hint was not visible"
        );
        driver.press("esc");
        use rich_intuituive::interact::{Event, Mouse, MouseKind};
        for _ in 0..35 {
            driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 30, 10)));
        }
        driver.settle();
        assert!(
            driver.find_text("Deep action").is_some(),
            "popup did not scroll to its deep control: {:?}",
            driver.screen().plain()
        );
        driver.press("f");
        driver.settle();
        driver
            .find_text(&format!("[{}]", deep_hint.as_ref().unwrap()))
            .expect("scrolled popup hint was not visible");
        driver.type_text(deep_hint.as_ref().unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            driver.settle();
            if state
                .lock()
                .unwrap()
                .actions
                .iter()
                .any(|a| matches!(a, PageAction::Click(node) if *node == deep_node))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "scrolled popup hint did not activate its source control"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        for _ in 0..35 {
            driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollUp, 30, 10)));
        }
        driver.settle();
    }
    if modal {
        let focused = driver.focused_name();
        let before = state.lock().unwrap().actions.len();
        driver.press("f1");
        driver.settle();
        assert!(
            driver.find_text("Link hints").is_some(),
            "Actions did not open above source modal"
        );
        driver.press("esc");
        driver.settle();
        assert!(driver.find_text("Choose").is_some());
        assert_eq!(
            driver.focused_name(),
            focused,
            "source modal focus was not restored"
        );
        assert!(!state.lock().unwrap().actions[before..].iter().any(|a| matches!(a, PageAction::Key(key) if *key == rich_intuituive::interact::Key::parse("esc").unwrap())), "Actions Escape reached source");
        driver.press("a");
        driver.settle();
        assert!(
            driver.find_text("Choose").is_some(),
            "faithful projection lost source dialog"
        );
        driver.press("a");
        driver.press("r");
        driver.settle();
        let reader = driver.screen().plain().join("\n");
        assert!(
            reader.matches("Dialog content").count() <= 1,
            "Reader duplicated source popup: {reader}"
        );
        driver.press("r");
        driver.settle();
        assert!(
            driver.find_text("Choose").is_some(),
            "return to projection lost source dialog"
        );
    }
    if let Some(target) = higher_node {
        let (x, y) = driver
            .find_text("Above")
            .expect("higher-painted source control hidden by popup");
        let hint = higher_hint.as_ref().unwrap();
        driver.press("f");
        driver.settle();
        let (hint_x, hint_y) = driver
            .find_text(&format!("[{hint}]"))
            .expect("foreground hint is not visible");
        assert_eq!(hint_y, y);
        assert!(
            hint_x.abs_diff(x) <= 1,
            "hint appeared away from its source control"
        );
        driver.type_text(hint);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            driver.settle();
            if state
                .lock()
                .unwrap()
                .actions
                .iter()
                .any(|a| matches!(a, PageAction::Click(node) if *node == target))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "foreground hint did not activate its source control"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let copies = driver
            .accessibility()
            .iter()
            .filter(|entry| entry.name.contains("Above"))
            .count();
        assert!(
            copies <= 1,
            "foreground duplicated accessibility entries: {copies}"
        );
        let previous = state
            .lock()
            .unwrap()
            .actions
            .iter()
            .filter(|a| matches!(a, PageAction::Click(node) if *node == target))
            .count();
        driver.click(x, y);
        let deadline = Instant::now() + Duration::from_secs(5);
        while state
            .lock()
            .unwrap()
            .actions
            .iter()
            .filter(|a| matches!(a, PageAction::Click(node) if *node == target))
            .count()
            <= previous
        {
            driver.settle();
            assert!(
                Instant::now() < deadline,
                "higher-painted source control did not receive click"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        driver.press("j j");
        driver.settle();
        let (moved_x, moved_y) = driver
            .find_text("Above")
            .expect("scrolled foreground disappeared");
        assert!(
            moved_y < y,
            "foreground did not follow underlay scroll: {y} -> {moved_y}"
        );
        assert_eq!(moved_x, x);
        let previous = state
            .lock()
            .unwrap()
            .actions
            .iter()
            .filter(|a| matches!(a, PageAction::Click(node) if *node == target))
            .count();
        driver.click(moved_x, moved_y);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            driver.settle();
            if state
                .lock()
                .unwrap()
                .actions
                .iter()
                .filter(|a| matches!(a, PageAction::Click(node) if *node == target))
                .count()
                > previous
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "scrolled foreground hit did not reach source"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        driver.press("home");
        driver.settle();
        assert!(driver.find_text("Incoming").is_none());
        for _ in 0..35 {
            driver.press("j");
        }
        driver.settle();
        let (incoming_x, incoming_y) = driver.find_text("Incoming").expect("higher-painted offscreen control was hidden after scrolling into popup without a source update");
        let incoming = incoming_node.unwrap();
        driver.click(incoming_x, incoming_y);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            driver.settle();
            if state
                .lock()
                .unwrap()
                .actions
                .iter()
                .any(|a| matches!(a, PageAction::Click(node) if *node == incoming))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "incoming higher-painted control was not clickable"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        driver.press("home");
        driver.settle();
    }
    let (outside_x, outside_y) = driver.find_text("Outside").unwrap();
    driver.click(outside_x, outside_y);
    let until = Instant::now() + Duration::from_millis(250);
    while Instant::now() < until {
        driver.settle();
        std::thread::sleep(Duration::from_millis(5));
    }
    let outside_clicked = state
        .lock()
        .unwrap()
        .actions
        .iter()
        .any(|a| matches!(a, PageAction::Click(node) if *node == outside_node));
    assert_eq!(
        outside_clicked, !modal,
        "modal policy incorrectly routed outside click"
    );
    let target = format!("popup-control-{expected}");
    for _ in 0..12 {
        if driver.focused_name().as_deref() == Some(target.as_str()) {
            break;
        }
        driver.press("tab");
    }
    assert_eq!(
        driver.focused_name().as_deref(),
        Some(target.as_str()),
        "second control was not keyboard reachable"
    );
    driver.press("enter");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        driver.settle();
        if state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Click(node) if *node == expected))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "keyboard action did not reach second source control"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (x, y) = driver.find_text("Choose").unwrap();
    driver.click(x, y);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        driver.settle();
        if state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Click(node) if *node == choose))
        {
            break;
        }
        assert!(Instant::now() < deadline, "popup action was not forwarded");
        std::thread::sleep(Duration::from_millis(10));
    }
    driver.press("esc");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        driver.settle();
        if state.lock().unwrap().actions.iter().any(|a| matches!(a, PageAction::Key(key) if *key == rich_intuituive::interact::Key::parse("esc").unwrap())) { break; }
        assert!(
            Instant::now() < deadline,
            "Escape was not forwarded to the source"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        driver.find_text("Choose").is_some(),
        "Escape hid a source-owned dialog without source change"
    );
    for (snapshot, should_show) in [(hidden, false), (visible.clone(), true)] {
        {
            let mut state = state.lock().unwrap();
            state.pages.insert("popup".into(), snapshot);
            state.events.push(noctiluca::engine::EngineEvent::Changed);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            driver.settle();
            if driver.find_text("Choose").is_some() == should_show {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "popup visibility did not update to {should_show}: {}",
                driver.screen().plain().join("\n")
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if !modal {
        let engine = FakeEngine::from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        )
        .unwrap();
        let state = engine.state.clone();
        state.lock().unwrap().pages.insert("popup".into(), visible);
        let policy = rich_intuituive::ImagePresentation::new(
            rich_art::picture::Pixels::Kitty,
            rich_intuituive::KittyFeatures {
                unicode_placeholders: true,
                native_animation: false,
            },
            rich_intuituive::PixelFormats::cells().with(rich_art::picture::Pixels::Kitty),
        );
        let mut pixels = noctiluca::app::app_with_presentation(
            Box::new(engine),
            Options {
                config: Config::default(),
                bookmarks: Bookmarks::default(),
                url: Some("popup.html".into()),
                view: Some(View::Pixels),
                images: Some(Images::Kitty),
                tunnel: None,
                control: None,
            },
            policy,
        )
        .driver(120, 40);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !state.lock().unwrap().screencast {
            pixels.settle();
            assert!(Instant::now() < deadline, "pixel view did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        pixels.settle();
        assert!(
            pixels.find_text("Dialog content").is_none(),
            "pixel view duplicated the projected dialog"
        );
        assert!(
            !pixels
                .accessibility()
                .iter()
                .any(|node| node.name.contains("Dialog content")),
            "pixel view exposed a duplicate projected dialog"
        );
    }
}

#[test]
fn source_nonmodal_dialog_is_shown_as_an_interactive_layer() {
    source_dialog_layer(false, false);
}

#[test]
fn oversized_nonmodal_popup_has_a_keyboard_scroll_route() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        engine::{PageAction, Rect},
        testing::{FakeEngine, PageBuilder},
    };
    use std::time::{Duration, Instant};

    let mut p = PageBuilder::new(
        "https://example.test/keyboard.html",
        "Keyboard",
        960.0,
        4000.0,
    );
    p.view(0.0, 720.0);
    let body = p.body();
    let outside = p.element(
        body,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(0.0, 0.0, 140.0, 18.0)),
    );
    p.text(outside, "Outside", Rect::new(0.0, 0.0, 120.0, 18.0));
    for i in 0..80 {
        let y = 36.0 + f64::from(i) * 36.0;
        let row = p.element(body, "P", &[], &[], Some(Rect::new(0.0, y, 160.0, 18.0)));
        p.text(
            row,
            &format!("Underlay {i:02}"),
            Rect::new(0.0, y, 150.0, 18.0),
        );
    }
    let panel = p.element(
        body,
        "DIV",
        &[("role", "dialog"), ("aria-label", "Keyboard popup")],
        &[
            ("position", "fixed"),
            ("background-color", "rgb(55, 57, 82)"),
        ],
        Some(Rect::new(24.0, 72.0, 296.0, 288.0)),
    );
    p.paint(panel, 100);
    for i in 0..45 {
        let y = 90.0 + f64::from(i) * 18.0;
        let row = p.element(panel, "P", &[], &[], Some(Rect::new(32.0, y, 240.0, 18.0)));
        p.text(
            row,
            &format!("Popup paragraph {i:02}"),
            Rect::new(32.0, y, 230.0, 18.0),
        );
    }
    let action = p.element(
        panel,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(32.0, 1000.0, 150.0, 30.0)),
    );
    p.text(action, "Deep action", Rect::new(32.0, 1000.0, 130.0, 18.0));
    let model = build(&p.snapshot(), &mut Revisions::new());
    assert_eq!(model.overlays.len(), 1);
    assert!(!model.overlays[0].modal);
    let deep_node = model.find("button", "Deep action")[0].node;
    let outside_node = model.find("button", "Outside")[0].node;
    let controls = model
        .controls
        .iter()
        .filter(|c| !c.disabled && !c.hover_only)
        .collect::<Vec<_>>();
    let hints = noctiluca::layout::hint_labels(controls.len());
    let hint_for = |node| hints[controls.iter().position(|c| c.node == node).unwrap()].clone();
    let deep_hint = hint_for(deep_node);
    let outside_hint = hint_for(outside_node);
    let engine = FakeEngine::from_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
    )
    .unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("keyboard".into(), p.snapshot());
    let mut driver = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("keyboard.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(40, 24);
    let deadline = Instant::now() + Duration::from_secs(5);
    while driver.find_text("Popup paragraph 00").is_none() {
        driver.settle();
        assert!(
            Instant::now() < deadline,
            "popup not shown: {:?}",
            driver.screen().plain()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(driver.find_text("Deep action").is_none());
    driver.assert_focused("page");
    let page_focus = driver.focused();
    let status = driver.screen().plain()[22].clone();
    // The first ordinary Tab enters the popup's scroll pane, before any
    // of its controls have been materialized. Tab counts alone cannot
    // reveal those controls: PageDown must go to this focused pane.
    driver.press("tab");
    let popup_focus = driver.focused();
    assert!(popup_focus.is_some());
    assert_ne!(popup_focus, page_focus);
    for _ in 0..3 {
        driver.press("pagedown");
        assert_eq!(driver.focused(), popup_focus);
        assert_eq!(
            driver.screen().plain()[22],
            status,
            "popup scroll moved the underlay"
        );
        assert!(driver.find_text("Underlay 00").is_some());
    }
    assert!(driver.find_text("Popup paragraph 00").is_none());
    assert!(
        driver.find_text("Deep action").is_some(),
        "{:?}",
        driver.screen().plain()
    );
    driver.press("f");
    driver
        .find_text(&format!("[{deep_hint}]"))
        .expect("deep control hint must be visible before use");
    driver.type_text(&deep_hint);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        driver.settle();
        if state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Click(node) if *node == deep_node))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "visible deep hint did not activate its control"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        state
            .lock()
            .unwrap()
            .actions
            .iter()
            .filter(|a| matches!(a, PageAction::Click(node) if *node == deep_node))
            .count(),
        1
    );
    assert!(
        driver.find_text("Deep action").is_some(),
        "source popup was dismissed without a source update"
    );
    // Nonmodal focus can leave the popup. Its scroll stays put while the
    // page pane acts on a visible outside control and then scrolls itself.
    driver.press("shift+tab");
    driver.assert_focused("page");
    driver.press("f");
    driver
        .find_text(&format!("[{outside_hint}]"))
        .expect("outside control hint must be visible before use");
    driver.type_text(&outside_hint);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        driver.settle();
        if state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Click(node) if *node == outside_node))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "nonmodal outside control was not usable"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        state
            .lock()
            .unwrap()
            .actions
            .iter()
            .filter(|a| matches!(a, PageAction::Click(node) if *node == outside_node))
            .count(),
        1
    );
    let popup_rows = |d: &rich_intuituive::Driver| {
        d.screen().plain()[6..22]
            .iter()
            .map(|row| row.chars().skip(3).collect::<String>())
            .collect::<Vec<_>>()
    };
    let popup_before = popup_rows(&driver);
    let status_before = driver.screen().plain()[22].clone();
    driver.press("pagedown");
    driver.assert_focused("page");
    assert_ne!(
        driver.screen().plain()[22],
        status_before,
        "page pane did not scroll"
    );
    assert_eq!(
        popup_rows(&driver),
        popup_before,
        "underlay scrolling moved the popup"
    );
    assert!(driver.find_text("Deep action").is_some());
    let scrolled_status = driver.screen().plain()[22].clone();
    driver.press("tab pageup");
    assert_eq!(driver.focused(), popup_focus);
    assert_eq!(driver.screen().plain()[22], scrolled_status);
    assert_ne!(popup_rows(&driver), popup_before);
    for target in [deep_node, outside_node] {
        assert_eq!(
            state
                .lock()
                .unwrap()
                .actions
                .iter()
                .filter(|a| matches!(a, PageAction::Click(node) if *node == target))
                .count(),
            1,
            "source control was activated more than once"
        );
    }
}

#[test]
fn source_modal_dialog_blocks_outside_and_keeps_its_actions() {
    source_dialog_layer(true, false);
}

#[test]
fn opening_and_closing_a_fixed_dialog_keeps_the_underlay_scroll_anchor() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        engine::{EngineEvent, Rect},
        testing::{FakeEngine, PageBuilder},
    };
    use std::time::{Duration, Instant};
    let mut p = PageBuilder::new("https://example.test/anchor.html", "Anchor", 960.0, 4000.0);
    p.view(0.0, 720.0);
    let body = p.body();
    for i in 0..80 {
        let y = i as f64 * 40.0;
        let block = p.element(body, "P", &[], &[], Some(Rect::new(0.0, y, 160.0, 18.0)));
        p.text(
            block,
            &format!("Anchor item {i:02}"),
            Rect::new(0.0, y, 150.0, 18.0),
        );
    }
    let panel = p.element(
        body,
        "DIV",
        &[("role", "dialog")],
        &[
            ("position", "fixed"),
            ("background-color", "rgb(55, 57, 82)"),
        ],
        Some(Rect::new(400.0, 72.0, 400.0, 532.0)),
    );
    p.paint(panel, 100);
    p.text(panel, "Popup visible", Rect::new(410.0, 100.0, 150.0, 18.0));
    let visible = p.snapshot();
    p.set_style(panel, "opacity", "0");
    let hidden = p.snapshot();
    let engine = FakeEngine::from_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
    )
    .unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("anchor".into(), hidden.clone());
    let mut driver = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("anchor.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(120, 40);
    let deadline = Instant::now() + Duration::from_secs(5);
    while driver.find_text("Anchor item 00").is_none() {
        driver.settle();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    for _ in 0..20 {
        driver.press("j");
    }
    driver.settle();
    let before = driver.screen().plain();
    let marker = before
        .iter()
        .find_map(|line| {
            line.split_whitespace()
                .collect::<Vec<_>>()
                .windows(3)
                .find(|w| w[0] == "Anchor" && w[1] == "item")
                .map(|w| w.join(" "))
        })
        .expect("scrolled anchor");
    assert_ne!(marker, "Anchor item 00");
    for (snapshot, popup) in [(visible, true), (hidden, false)] {
        {
            let mut state = state.lock().unwrap();
            state.pages.insert("anchor".into(), snapshot);
            state.events.push(EngineEvent::Changed);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            driver.settle();
            if driver.find_text("Popup visible").is_some() == popup {
                break;
            }
            assert!(Instant::now() < deadline, "popup state did not change");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            driver.find_text(&marker).is_some(),
            "lost {marker} on popup={popup}: {:?}",
            driver.screen().plain()
        );
    }
}

#[test]
fn higher_painted_source_control_remains_above_a_nonmodal_popup() {
    source_dialog_layer(false, true);
}

#[test]
fn source_dialog_fits_narrow_off_right_and_short_viewports_with_reachable_controls() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        engine::{PageAction, Rect},
        testing::{FakeEngine, PageBuilder},
    };
    use std::time::{Duration, Instant};
    for (columns, rows, source_x, source_y) in [
        (40, 24, 196.0, 72.0),
        (20, 16, 196.0, 72.0),
        (40, 24, 900.0, 500.0),
    ] {
        let mut p = PageBuilder::new("https://example.test/fit.html", "Fit", 960.0, 720.0);
        p.view(0.0, 720.0);
        let panel = p.element(
            p.body(),
            "DIV",
            &[("role", "dialog")],
            &[
                ("position", "fixed"),
                ("background-color", "rgb(55, 57, 82)"),
            ],
            Some(Rect::new(source_x, source_y, 567.0, 532.0)),
        );
        p.paint(panel, 100);
        let action = p.element(
            panel,
            "BUTTON",
            &[],
            &[],
            Some(Rect::new(source_x + 16.0, source_y + 18.0, 112.0, 30.0)),
        );
        p.text(
            action,
            "Accept",
            Rect::new(source_x + 16.0, source_y + 18.0, 90.0, 18.0),
        );
        let model = build(&p.snapshot(), &mut Revisions::new());
        let target = model.find("button", "Accept")[0].node;
        let engine = FakeEngine::from_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
        )
        .unwrap();
        let state = engine.state.clone();
        state
            .lock()
            .unwrap()
            .pages
            .insert("fit".into(), p.snapshot());
        let mut driver = app(
            Box::new(engine),
            Options {
                config: Config::default(),
                bookmarks: Bookmarks::default(),
                url: Some("fit.html".into()),
                view: Some(View::Projection),
                images: Some(Images::None),
                tunnel: None,
                control: None,
            },
        )
        .driver(columns, rows);
        let deadline = Instant::now() + Duration::from_secs(2);
        let (x, y) = loop {
            driver.settle();
            if let Some(at) = driver.find_text("Accept") {
                break at;
            }
            assert!(
                Instant::now() < deadline,
                "popup control not identifiable in {columns}x{rows} at {source_x},{source_y}: {:?}",
                driver.screen().plain()
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        driver.click(x, y);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            driver.settle();
            if state
                .lock()
                .unwrap()
                .actions
                .iter()
                .any(|action| matches!(action, PageAction::Click(node) if *node == target))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "fitted control was not reachable"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
