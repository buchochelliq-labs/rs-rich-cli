//! Guide: Building your own components — run: cargo run -p rs-rich-interact --example guide_components [-- --svg docs/media/guide]
//!
//! The snippets and pictures on `docs/guide/interact/custom-components.md`
//! come from this file. Each picture is one frame: the component is started,
//! given the keys of a headless `Script`, and its view printed as the event
//! loop would paint it. `custom_component.rs` is the full, runnable app.

use std::path::PathBuf;

use rich::{ColorSystem, Console, ConsoleOptions, Renderable, Segment, Table, Text};
use rich_interact::compose::{Column, ComponentExt, Layer, Layers, Split, Tabs, FOCUS_NEXT};
use rich_interact::headless::{self, Script, Step};
use rich_interact::keymap::{keys, Keymap};
use rich_interact::kit::{self, FilterState, ListState, Theme};
use rich_interact::{
    Component, Confirm, Context, Event, Flow, Input, KeyCode, Outcome, Select, View,
};

// --8<-- [start:checklist]
pub struct Checklist {
    items: Vec<String>,
    filter: FilterState,
    list: ListState,
    keymap: Keymap,
}

impl Checklist {
    pub fn new(items: Vec<String>) -> Checklist {
        let mut list = ListState::new();
        list.clear_selection(items.len());
        list.set_len(items.len());
        Checklist {
            filter: FilterState::new(items.iter().cloned()),
            items,
            list,
            keymap: Keymap::new("checklist")
                .bind("up", keys("up"), "move up")
                .bind("down", keys("down"), "move down")
                .bind("tick", keys("space"), "tick or untick")
                .bind("done", keys("enter"), "finish"),
        }
    }
}

impl Component for Checklist {
    type Output = Vec<String>;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Vec<String>> {
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        match self.keymap.action(key) {
            Some("up") => self.list.step(-1, 8),
            Some("down") => self.list.step(1, 8),
            Some("tick") => {
                if let Some(index) = self.filter.index(self.list.cursor()) {
                    self.list.toggle(index);
                }
            }
            Some("done") => {
                let ticked = self.list.selected();
                return Flow::Done(ticked.into_iter().map(|i| self.items[i].clone()).collect());
            }
            _ => match key.code {
                KeyCode::Char(c) if !key.modifiers.ctrl => {
                    self.filter.query_mut().push(c);
                    self.filter.refilter();
                    self.list.set_len(self.filter.len());
                    self.list.reset(0, 8);
                }
                // Not ours: the container may want it.
                _ => return Flow::Ignored,
            },
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let theme = Theme::default();
        let mut question = kit::question(&theme, "Todo");
        question.push(kit::plain(self.filter.query().to_string()));
        let mut lines = vec![question];
        for position in self.list.visible(8) {
            let (index, matched) = &self.filter.matches()[position];
            let focused = position == self.list.cursor();
            let mark = if self.list.is_selected(*index) {
                "◉ "
            } else {
                "○ "
            };
            let mut line = vec![
                kit::plain(if focused { "› " } else { "  " }),
                kit::plain(mark),
            ];
            let base = focused.then_some(&theme.focused);
            line.extend(kit::highlight(
                &self.items[*index],
                matched,
                base,
                &theme.matched,
            ));
            lines.push(kit::fit(line, context.width));
        }
        View::new(lines)
    }

    fn keymap(&self) -> Keymap {
        self.keymap.clone()
    }
}
// --8<-- [end:checklist]

fn todo() -> Checklist {
    Checklist::new(
        [
            "bump the versions",
            "update the changelog",
            "run the tests",
            "tag the release",
            "publish the crates",
        ]
        .map(String::from)
        .to_vec(),
    )
}

// --8<-- [start:split]
fn split() -> impl Component<Output = String> {
    let checklist = todo().map(|ticked| Flow::Done(ticked.join(", ")));
    let region = Select::new("Region", ["eu-west", "us-east", "ap-south"])
        .map(|region| Flow::Done(region.to_string()));
    Split::horizontal(checklist, region).ratio(60)
}
// --8<-- [end:split]

// --8<-- [start:layers]
#[derive(Debug)]
enum App {
    Picked(String),
    Quit,
}

fn layers<'a>() -> Layers<'a, App> {
    let files = Select::new("File", ["a.rs", "b.rs"]).map(|f| Flow::Done(App::Picked(f.into())));
    let more = Select::new("More", ["c.rs"]).map(|f| Flow::Done(App::Picked(f.into())));
    Layers::new(Tabs::new().tab("Files", files).tab("More", more)).open_on(
        "quit",
        keys("ctrl+q"),
        "quit",
        || {
            let confirm = Confirm::new("Quit?").map(|answer| {
                if answer == "yes" {
                    Flow::Done(App::Quit)
                } else {
                    Flow::Continue
                }
            });
            Layer::modal(confirm).title("Quit").size(30, 6)
        },
    )
}
// --8<-- [end:layers]

// --8<-- [start:column]
fn form<'a>() -> Column<'a, ()> {
    Column::new()
        .child(Input::new("Name").map(|_| Flow::Continue))
        .child(Input::new("Email").map(|_| Flow::Continue))
        .rebind(FOCUS_NEXT, keys("f6 tab"))
        .on("save", keys("ctrl+s"), "save", || Flow::Done(()))
}
// --8<-- [end:column]

// --8<-- [start:keymap]
fn keys_table(console: &Console) {
    let app = layers();
    let mut table = Table::new().title("Keys");
    table.add_column("keys");
    table.add_column("does");
    table.add_column("context");
    for binding in app.keymap().bindings() {
        // `Text` cells: a key label such as `[` is not markup.
        table.add_row_text(vec![
            Text::new(binding.keys_label()),
            Text::new(binding.description.clone()),
            Text::new(binding.context.clone()),
        ]);
    }
    console.print(&table);
}
// --8<-- [end:keymap]

fn main() {
    // The composition runs headless as it does on a terminal.
    let (outcome, _) = headless::run(layers(), Script::new().keys("down enter"), 60, 10);
    match outcome.expect("the script finishes the app") {
        Outcome::Done(App::Picked(file)) => assert_eq!(file, "b.rs"),
        other => panic!("unexpected {other:?}"),
    }

    let shots = Shots::from_args("guide_components");
    shots.shot("checklist", 48, |c| {
        paint(c, todo(), Script::new().text("t").keys("down space"), 10)
    });
    shots.shot("split", 72, |c| {
        paint(c, split(), Script::new().keys("space down space"), 8)
    });
    shots.shot("layers", 60, |c| {
        paint(c, layers(), Script::new().keys("ctrl+q"), 10)
    });
    shots.shot("column", 48, |c| {
        paint(c, form(), Script::new().text("Ada Lovelace").keys("tab"), 6)
    });
    shots.shot("keymap", 72, keys_table);
}

/// Start `component`, give it the events of `script`, and print the view it
/// then paints at the console's width and `height` rows.
fn paint<C: Component>(console: &Console, mut component: C, script: Script, height: usize) {
    let context = Context {
        console,
        width: console.width(),
        height,
    };
    component.start(&context);
    for step in script.0 {
        if let Step::Event(event) = step {
            component.handle(&event, &context);
        }
    }
    console.print(&Lines(component.render(&context).lines));
}

/// A view's lines as a renderable.
struct Lines(Vec<Vec<Segment>>);

impl Renderable for Lines {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        for line in &self.0 {
            segments.extend(line.iter().cloned());
            segments.push(Segment::line());
        }
        segments
    }
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal at the shot's width.
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
            return f(&Console::builder().width(width).build());
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
