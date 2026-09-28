//! The semantic-region seam (#226): not upstream. Without a sink nothing
//! changes; with one, the output bytes are still the same, and the reporting
//! renderables enter their regions in nesting order and tag what they drew.

use std::sync::{Arc, Mutex};

use rich::markdown::Markdown;
use rich::protocol::{region_of, ConsoleRegions, RegionId, RegionInfo, RegionRole, RegionSink};
use rich::{ColorSystem, Console, Panel, Renderable, Rule, Segment, Table};

#[derive(Default)]
struct Log {
    /// (info, parent) per region, in the order entered; a region's id is
    /// its index.
    regions: Mutex<Vec<(RegionInfo, Option<u64>)>>,
    open: Mutex<Vec<u64>>,
}

impl RegionSink for Log {
    fn enter(&self, region: RegionInfo) -> RegionId {
        let mut regions = self.regions.lock().unwrap();
        let mut open = self.open.lock().unwrap();
        let id = regions.len() as u64;
        regions.push((region, open.last().copied()));
        open.push(id);
        RegionId(id)
    }

    fn exit(&self, id: RegionId) {
        let mut open = self.open.lock().unwrap();
        assert_eq!(open.pop(), Some(id.0), "regions exit in nesting order");
    }
}

fn console() -> Console {
    Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build()
}

fn render(console: &Console, renderable: &dyn Renderable) -> Vec<Segment> {
    renderable.rich_render(console, &console.options())
}

fn with_sink(renderable: &dyn Renderable) -> (Vec<Segment>, Arc<Log>, String, String) {
    let plain = console();
    let expected = plain.segments_to_string(&render(&plain, renderable));
    let log = Arc::new(Log::default());
    let mut observed = console();
    observed.set_region_sink(Some(log.clone()));
    let segments = render(&observed, renderable);
    let got = observed.segments_to_string(&segments);
    (segments, log, expected, got)
}

fn table() -> Table {
    let mut table = Table::new().title("Crates");
    table.add_column("Name");
    table.add_column("Version");
    table.add_row(&["rich", "0.0.9"]);
    table.add_row(&["[link=https://x.test]ext[/link]", "0.0.11"]);
    table
}

#[test]
fn without_a_sink_nothing_is_tagged() {
    let console = console();
    assert!(console.region_sink().is_none());
    let panel = Panel::new(Box::new(table())).title("Box");
    for segment in render(&console, &panel) {
        assert!(segment.style.as_ref().and_then(region_of).is_none());
    }
}

#[test]
fn a_sink_leaves_the_bytes_alone_and_sees_nesting() {
    let panel = Panel::new(Box::new(table())).title("Box");
    let (segments, log, expected, got) = with_sink(&panel);
    assert_eq!(got, expected);
    let regions = log.regions.lock().unwrap();
    assert_eq!(regions[0].0.role, RegionRole::Panel);
    assert_eq!(regions[0].0.label.as_deref(), Some("Box"));
    let table = regions
        .iter()
        .position(|(info, _)| info.role == RegionRole::Table)
        .expect("a table region");
    assert_eq!(regions[table].1, Some(0));
    assert_eq!(regions[table].0.label.as_deref(), Some("Crates"));
    let cell = regions
        .iter()
        .position(|(info, _)| info.role == RegionRole::TableCell { row: 1, column: 0 })
        .expect("a body cell");
    assert_eq!(regions[cell].1, Some(table as u64));
    assert!(regions
        .iter()
        .any(|(info, _)| info.role == RegionRole::TableHeader { column: 1 }));
    // Every drawn segment carries the innermost region that drew it.
    let tagged: Vec<u64> = segments
        .iter()
        .filter(|s| !s.control && !s.text.is_empty())
        .map(|s| s.style.as_ref().and_then(region_of).expect("tagged").0)
        .collect();
    assert!(tagged.contains(&0) && tagged.contains(&(cell as u64)));
    let link = segments
        .iter()
        .find(|s| s.text == "ext")
        .and_then(|s| s.style.clone())
        .expect("the linked cell");
    assert_eq!(link.link(), Some("https://x.test"));
    assert_eq!(region_of(&link), Some(RegionId(cell as u64)));
    assert!(log.open.lock().unwrap().is_empty());
}

#[test]
fn rules_headings_and_code_report_regions() {
    let rule = Rule::new("[bold]Part[/] 2");
    let (_, log, expected, got) = with_sink(&rule);
    assert_eq!(got, expected);
    let regions = log.regions.lock().unwrap();
    assert_eq!(regions[0].0.role, RegionRole::Rule);
    assert_eq!(regions[0].0.label.as_deref(), Some("Part 2"));
    drop(regions);

    let markdown = Markdown::new(
        "# Title\n\nText with [a link](https://y.test).\n\n```rust\nfn main() {}\n```\n",
    );
    let (segments, log, expected, got) = with_sink(&markdown);
    assert_eq!(got, expected);
    let regions = log.regions.lock().unwrap();
    let roles: Vec<&RegionRole> = regions.iter().map(|(info, _)| &info.role).collect();
    assert_eq!(
        roles,
        [&RegionRole::Heading { level: 1 }, &RegionRole::Code]
    );
    assert_eq!(regions[0].0.label.as_deref(), Some("Title"));
    assert_eq!(regions[1].0.label.as_deref(), Some("rust"));
    let heading = segments.iter().find(|s| s.text.contains("Title")).unwrap();
    assert_eq!(
        heading.style.as_ref().and_then(region_of),
        Some(RegionId(0))
    );
    let code = segments.iter().find(|s| s.text.contains("main")).unwrap();
    assert_eq!(code.style.as_ref().and_then(region_of), Some(RegionId(1)));
}
