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
fn a_cookie_banner_fixed_to_the_bottom_is_docked_and_the_shops_header_pinned() {
    let (lines, pinned, docked) = strips("product-grid-scrolled", 100);
    let all = |l: &[String]| l.join("\n");
    assert!(all(&docked).contains("We use cookies"), "{docked:#?}");
    assert!(all(&docked).contains("[Accept]"), "{docked:#?}");
    assert!(!all(&lines).contains("We use cookies"), "{lines:#?}");
    assert!(all(&pinned).contains("Search the shop"), "{pinned:#?}");
    assert!(!all(&lines).contains("Search the shop"), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("1-3 of over 2,000")));
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
