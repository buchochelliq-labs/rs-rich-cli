//! The terminal adaptations, each on a fixture page Chrome laid out
//! (`tests/fixtures/adapt.html` at 72 columns, a narrow terminal), with
//! the rule on and off: no browser needed. Every rule is generic: the
//! fixture is ordinary HTML, and nothing here names a site.

use noctiluca::adapt::{decorative, Adapt, Rule, MAX_PICTURE_ROWS};
use noctiluca::args::{adapt_of, parse_args, Run};
use noctiluca::layout::{project, Line, Marks, Projection};
use noctiluca::model::{build, Page, Revisions};
use noctiluca::testing::snapshot_file;

fn page(name: &str) -> Page {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/snapshots/{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn projected(page: &Page, columns: u16, adapt: Adapt) -> Projection {
    let marks = Marks {
        adapt,
        ..Marks::default()
    };
    project(page, columns, (8.0, 18.0), &marks)
}

fn lines(page: &Page, columns: u16, adapt: Adapt) -> Vec<String> {
    projected(page, columns, adapt)
        .lines
        .iter()
        .map(Line::plain)
        .collect()
}

/// The row a line containing `text` is on.
fn row_of(lines: &[String], text: &str) -> usize {
    lines
        .iter()
        .position(|l| l.contains(text))
        .unwrap_or_else(|| panic!("{text:?} not in {lines:#?}"))
}

fn only(rule: Rule) -> Adapt {
    Adapt::none().with(rule, true)
}

#[test]
fn decorative_pictures_drop_and_real_ones_stay() {
    let page = page("adapt");
    let size = |m: &noctiluca::model::Media| (m.rect.width as u32, m.rect.height as u32);
    let dropped: Vec<_> = page
        .media
        .iter()
        .filter(|m| decorative(m))
        .map(size)
        .collect();
    // A 1 x 1 pixel, and two icons marked decorative (alt="" and
    // aria-hidden); the 96 x 54 picture with alt="" is real and stays.
    assert_eq!(dropped, [(16, 16), (16, 16), (1, 1)]);
    let kept = |adapt| {
        projected(&page, 72, adapt)
            .regions
            .iter()
            .map(|r| size(&page.media[r.media]))
            .collect::<Vec<_>>()
    };
    let on = kept(only(Rule::DecorativeImages));
    assert!(on.contains(&(96, 54)), "{on:?}");
    assert!(!on.contains(&(16, 16)) && !on.contains(&(1, 1)), "{on:?}");
    assert!(kept(Adapt::none()).contains(&(16, 16)));
}

#[test]
fn navigation_collapses_to_a_row_on_a_narrow_terminal() {
    let page = page("adapt");
    let on = lines(&page, 72, only(Rule::CollapseNav));
    assert_eq!(on[0], "≡ Sections: News Sea Sky About");
    // The main column moves into the sidebar's columns.
    assert!(
        on.iter().any(|l| l.starts_with("Before the pictures.")),
        "{on:#?}"
    );
    let off = lines(&page, 72, Adapt::none());
    assert!(off[0].starts_with("News "), "{off:#?}");
    assert!(off.iter().all(|l| !l.contains('≡')));
    // A wide terminal keeps its sidebar.
    assert!(lines(&page, 120, only(Rule::CollapseNav))
        .iter()
        .all(|l| !l.contains('≡')));
}

#[test]
fn icon_buttons_read_as_their_name() {
    let page = page("adapt");
    let on = lines(&page, 72, only(Rule::IconButtons));
    let row = &on[row_of(&on, "[ Share ]")];
    assert!(row.contains("[ Settings ] [ Share ]"), "{row:?}");
    let off = lines(&page, 72, Adapt::none());
    let row = &off[row_of(&off, "[ Share ]")];
    assert!(
        !row.contains("Settings") && row.contains("░░░░ [ Share ]"),
        "{row:?}"
    );
}

#[test]
fn tall_pictures_are_capped_and_what_is_under_moves_up() {
    let page = page("adapt");
    let tall = page
        .media
        .iter()
        .position(|m| m.alt == "A tall lighthouse")
        .unwrap();
    let rows = |adapt| {
        let p = projected(&page, 72, adapt);
        let region = p.regions.iter().find(|r| r.media == tall).unwrap().rows;
        let lines: Vec<String> = p.lines.iter().map(Line::plain).collect();
        (region, row_of(&lines, "Under the lighthouse."))
    };
    let (capped, under_capped) = rows(only(Rule::CapPictures));
    let (full, under_full) = rows(Adapt::none());
    assert_eq!(capped, MAX_PICTURE_ROWS);
    assert_eq!(full, 30);
    assert!(
        under_full - under_capped >= full - capped - 2,
        "{under_full} {under_capped}"
    );
}

#[test]
fn grids_snap_their_columns_to_the_cells() {
    let page = page("adapt");
    // At 72 columns the grid's five 80 px tracks are 10 cells each: too
    // narrow, so it snaps to three of 17 cells, in two rows.
    let on = lines(&page, 72, only(Rule::SnapGrids));
    let first = &on[row_of(&on, "Card one")];
    assert!(first.contains("Card two") && first.contains("Card three"));
    assert!(!first.contains("Card four"), "{first:?}");
    let second = &on[row_of(&on, "Card four")];
    assert!(second.contains("Card five"));
    assert_eq!(
        first.find("Card two").unwrap() - first.find("Card one").unwrap(),
        18
    );
    let off = lines(&page, 72, Adapt::none());
    assert!(off[row_of(&off, "Card one")].contains("Card five"));
}

#[test]
fn a_picture_of_text_reads_as_its_text_when_asked() {
    let page = page("adapt");
    let banner = page
        .media
        .iter()
        .position(|m| m.alt == "Glow report")
        .unwrap();
    // Off by default: it changes what the page means.
    assert!(!Adapt::default().on(Rule::TextOverPictures));
    let regions = |adapt| projected(&page, 72, adapt).regions;
    assert!(regions(Adapt::default()).iter().any(|r| r.media == banner));
    let on = only(Rule::TextOverPictures);
    assert!(regions(on).iter().all(|r| r.media != banner));
    let text = lines(&page, 72, on);
    assert!(text.iter().any(|l| l.trim_end().ends_with("Glow report")));
}

#[test]
fn no_adapt_gives_the_faithful_projection() {
    let args =
        |list: &[&str]| match parse_args(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap()
        {
            Run::Browse(args) => args,
            other => panic!("{other:?}"),
        };
    let page = page("adapt");
    let none = adapt_of(&args(&["--no-adapt", "adapt.html"]), Adapt::default()).unwrap();
    assert!(none.is_none());
    assert_eq!(
        projected(&page, 72, none),
        projected(&page, 72, Adapt::none())
    );
    assert_ne!(
        projected(&page, 72, none).lines,
        projected(&page, 72, Adapt::default()).lines
    );
    // One rule off, the others on.
    let some = adapt_of(
        &args(&["--adapt=-decorative-images,-collapse-nav", "adapt.html"]),
        Adapt::default(),
    )
    .unwrap();
    assert!(!some.on(Rule::DecorativeImages) && !some.on(Rule::CollapseNav));
    assert!(some.on(Rule::SnapGrids) && some.on(Rule::LiveTail));
    let collapsed = lines(&page, 72, some);
    assert!(collapsed.iter().all(|l| !l.contains('≡')));
}
