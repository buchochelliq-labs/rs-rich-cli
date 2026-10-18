//! A port of ratatui's widget examples
//! (<https://github.com/ratatui/ratatui/tree/main/ratatui-widgets/examples>)
//! and of its `table`, `scrollbar`, `popup`, `custom-widget` and
//! `hyperlink` example apps
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps>), as one
//! gallery rebuilt on intuiTUIve. It reuses none of ratatui's code: it
//! rebuilds the behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The
//! text, the dishes, the people and the events are our own. The charts,
//! gauges, sparklines and the canvas are in the chart gallery,
//! `examples/charts.rs`.
//!
//!     cargo run -p rs-rich-intuituive --example widgets
//!     cargo run -p rs-rich-intuituive --example widgets -- --linear
//!
//! One tab for each family: Block, Text, List, Table, Tabs, Scrollbar,
//! Calendar, Popup, Borders and Custom. ←/→ or 1 to 9 and 0 change tabs,
//! and a click picks one; ? lists the keys, Ctrl+P finds a command, q
//! quits. Inside a tab:
//!
//! - **Text:** m shows or masks the secret.
//! - **List:** ↑/↓ move the selection, Tab goes to the other list.
//! - **Table:** ↑/↓ the row, ←/→ the cell (the gallery's tabs then by
//!   number), v the other table, c its next colours.
//! - **Tabs:** n and p, or a click, the inner tab.
//! - **Scrollbar:** j/k scroll down and up, h/l across; the wheel too.
//! - **Calendar:** n and p the month.
//! - **Popup:** p opens and closes the pop-up.
//! - **Borders:** ↑/↓, Tab or a click pick a pane.
//! - **Custom:** Tab, ↑/↓ or the pointer pick a button; Enter, Space or a
//!   click presses it.
//!
//! What intuiTUIve adds to the originals:
//!
//! - **The mouse** picks the tabs, the list items, the rows, the panes and
//!   the buttons, and the wheel scrolls.
//! - **The keys have descriptions**, so `?` lists them and Ctrl+P finds
//!   them.
//! - **The nodes have labels**, so linear mode (`--linear`) reads well.
//! - **Only what changes redraws:** a key in a tab draws that tab's widget,
//!   not the tab strip or the rest (`tests/widgets_gallery.rs` checks it).

use rich_ext::palette::tailwind::{Palette, BLUE, EMERALD, INDIGO, RED, SLATE};

use intuituive::border::Shadow as ShadowKind;
use intuituive::interact::{KeyCode, MouseKind};
use intuituive::prelude::*;
use intuituive::widget::{Canvas, DrawCx, EventCx, MouseExt, Used, WidgetEvent};
use intuituive::widgets::{
    month, table_with, tabs, CalendarOptions, Column, Date, ListDirection, Scrollbar,
    ScrollbarSymbols, Show, Side, TableOptions, Weekday,
};
use intuituive::{scroll_both_with, ModalOptions, Sides};

/// The tabs, in order: 1 to 9, then 0 for the tenth.
pub const TABS: [&str; 10] = [
    "Block",
    "Text",
    "List",
    "Table",
    "Tabs",
    "Scrollbar",
    "Calendar",
    "Popup",
    "Borders",
    "Custom",
];

/// How the gallery runs: its command line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// The tab it opens on.
    pub tab: usize,
    /// Run in linear mode: the screen as lines of text, for a screen
    /// reader.
    pub linear: bool,
}

impl Options {
    /// `--tab N` (1 to 10) and `--linear`, as the command line gives them.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) => (name.to_string(), Some(value.to_string())),
                None => (arg.clone(), None),
            };
            match name.as_str() {
                "--tab" => {
                    let value = inline
                        .or_else(|| args.next())
                        .ok_or("--tab takes a number from 1 to 10")?;
                    let n: usize = value
                        .parse()
                        .ok()
                        .filter(|n| (1..=TABS.len()).contains(n))
                        .ok_or("--tab takes a number from 1 to 10")?;
                    options.tab = n - 1;
                }
                "--linear" => options.linear = true,
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(options)
    }
}

/// Block: borders, styles, titles, sides and a shadow.
fn blocks() -> Node {
    let body = |text: &str| label(text.to_string());
    grid(
        [Size::Flex(1), Size::Flex(1), Size::Flex(1)],
        [
            body("A plain border, all four sides.")
                .panel("Bordered")
                .border(Border::Plain),
            body("Bold italic blue on black, the border and the inside.")
                .panel("Styled")
                .style("bold italic blue on black"),
            body("A rounded border in red, its title in bold.")
                .panel("[bold]Rounded red[/]")
                .border_style("red"),
            body("A thick border.").panel("Thick").border(Border::Thick),
            body("A double border, its title in the middle.")
                .panel("Double")
                .border(Border::Double)
                .title_align(Align::Center),
            body("Titles left, right and on the bottom edge.")
                .panel("Left")
                .title_right("[yellow]Right[/]")
                .title_bottom("[dim]Bottom[/]")
                .add_title(Edge::Bottom, Align::End, "[dim]1/9[/]"),
            body("Only the top and the bottom.")
                .panel("Sides")
                .borders(Sides::TOP | Sides::BOTTOM),
            body("Dashes, light and heavy.")
                .panel("Dashed")
                .border(Border::LightDoubleDashed),
            body("A shadow down the right and along the bottom.")
                .panel("Shadow")
                .shadow(ShadowKind::MediumShade),
        ],
    )
    .rows([Size::Flex(1)])
    .name("blocks")
    .label("Blocks: borders, styles, titles and a shadow")
}

/// The wrapped paragraph: a recipe of our own, with bold and italic runs.
pub const RECIPE: &str = "[bold]Toasted barley salad.[/] Toast a cup of pearl barley \
in a dry pan until it smells of [italic]popcorn[/], then simmer it for half an hour. \
Drain it, dress it while warm with lemon, olive oil and a little honey, and fold in \
chopped parsley, cucumber and [bold]feta[/]. It keeps for three days, and tastes \
better on the second.";

/// The secret, masked until `m` shows it.
pub const SECRET: &str = "correct-horse-battery";

/// Where the link goes.
pub const LINK: &str = "https://github.com/buchochelliq-labs/rs-rich-cli";

/// Text: alignment, wrapping, overflow, a secret and a link.
fn text_page() -> Node {
    let shown = signal(false);
    let secret = text(move || {
        let value = if shown.get() {
            SECRET.to_string()
        } else {
            "•".repeat(SECRET.chars().count())
        };
        format!("Password: [bold]{value}[/]\n\n[dim]m shows or masks it[/]")
    })
    .name("secret")
    .label("A secret, masked")
    .panel("Masked");
    let link = label(format!(
        "The project's home:\n[link={LINK}][blue underline]rs-rich on GitHub[/][/link]\n\n\
         [dim]A terminal with OSC 8 links opens it.[/]"
    ))
    .name("link")
    .panel("Hyperlink");
    grid(
        [Size::Flex(1), Size::Flex(1), Size::Flex(1)],
        [
            label("Each line\nsits in the middle\nof the panel.")
                .text_align(TextAlign::Center)
                .panel("Centred"),
            label("Each line\nsits at the right\nof the panel.")
                .text_align(TextAlign::Right)
                .panel("Right"),
            label("Each line but the last is spread to the whole width, the words spaced out.")
                .text_align(TextAlign::Justify)
                .panel("Justified"),
            label(RECIPE).name("recipe").panel("Wrapped").span(2, 1),
            label("A line too long for its panel is cut, and ends in an ellipsis.")
                .no_wrap()
                .overflow(Overflow::Ellipsis)
                .panel("No wrap"),
            secret,
            link.span(2, 1),
        ],
    )
    .rows([Size::Fixed(5), Size::Flex(1), Size::Fixed(6)])
    .bind("m", "show or mask the secret", move |_| {
        shown.update(|s| *s = !*s)
    })
    .focusable()
    .name("text-page")
    .label("Text: alignment, wrapping, overflow")
}

/// The first list's items.
pub const GROCERIES: [&str; 12] = [
    "Apples",
    "Barley",
    "Carrots",
    "Dates",
    "Eggs",
    "Fennel",
    "Garlic",
    "Honey",
    "Ink for the printer",
    "Jam",
    "Kale",
    "Lemons",
];

/// The chat's messages, the newest first: it runs bottom to top.
pub const CHAT: [(&str, &str); 8] = [
    ("Ana", "On my way, ten minutes."),
    ("Ben", "Table for four at eight, by the window."),
    ("Ana", "Did you book somewhere?"),
    (
        "Ben",
        "The new place on the corner has a long menu and short queues on a Tuesday.",
    ),
    ("Ana", "Dinner tonight?"),
    ("Ben", "Fine, and you?"),
    ("Ana", "How was the week?"),
    ("Ben", "Hello!"),
];

/// List: a symbol and a reversed highlight; a chat bottom to top.
fn lists() -> Node {
    let picked = signal(0usize);
    let groceries = list(|| GROCERIES.iter().map(|g| g.to_string()).collect(), picked)
        .symbol("> ")
        .theme_styles([("selected", "reverse"), ("list.selected", "reverse")])
        .name("groceries")
        .label("Groceries")
        .panel("List")
        .title_bottom("[dim]↑/↓ move · Tab the chat[/]");
    let message = signal(0usize);
    let chat = list(
        || {
            CHAT.iter()
                .map(|(who, text)| format!("[bold]{who}:[/] {text}"))
                .collect()
        },
        message,
    )
    .symbol("[red]>>[/] ")
    .repeat_symbol()
    .multiline()
    .scroll_padding(1)
    .direction(ListDirection::BottomToTop)
    .theme_styles([("selected", "italic"), ("list.selected", "italic")])
    .name("chat")
    .label("Chat, newest at the bottom")
    .panel("Chat, bottom to top")
    .title_bottom("[dim]multi-line, scroll padding[/]");
    row([groceries.percent(40), chat.flex(1)])
}

/// A dish on the Table tab's first table.
pub const DISHES: [[&str; 4]; 8] = [
    ["Lentil soup", "Starter", "6.50", "20 min"],
    ["Beetroot salad", "Starter", "7.00", "15 min"],
    ["Mushroom risotto", "Main", "14.00", "35 min"],
    ["Fish pie", "Main", "16.50", "45 min"],
    ["Chickpea curry", "Main", "13.00", "30 min"],
    ["Apple crumble", "Dessert", "6.00", "40 min"],
    ["Lemon tart", "Dessert", "6.50", "25 min"],
    ["Cheese board", "Dessert", "9.00", "5 min"],
];

/// A person in the table app: a name, an address over three lines, an
/// email. All made up.
pub const PEOPLE: [(&str, &str, &str); 12] = [
    (
        "Ada Marsh",
        "4 Willow Row\nEastbrook\nEB2 7QL",
        "ada.marsh@example.com",
    ),
    (
        "Ben Okafor",
        "17 Quarry Hill\nLow Fenton\nLF9 1AA",
        "ben.okafor@example.com",
    ),
    (
        "Chloe Ruiz",
        "2 Harbour Steps\nPortwick\nPW1 3DS",
        "chloe.ruiz@example.com",
    ),
    (
        "Dev Patel",
        "88 Mill Lane\nCrossford\nCF4 8NT",
        "dev.patel@example.com",
    ),
    (
        "Elif Kaya",
        "31 Orchard Close\nBramley\nBR6 2PE",
        "elif.kaya@example.com",
    ),
    (
        "Finn Larsen",
        "9 Signal Road\nNorth Arden\nNA3 5HX",
        "finn.larsen@example.com",
    ),
    (
        "Grace Liu",
        "56 Tannery Yard\nOldbury Vale\nOV7 4KR",
        "grace.liu@example.com",
    ),
    (
        "Hugo Brandt",
        "12 Ferry Walk\nSaltmere\nSM5 9JB",
        "hugo.brandt@example.com",
    ),
    (
        "Isla Moreau",
        "3 Beacon Court\nHighcliff\nHC8 6WD",
        "isla.moreau@example.com",
    ),
    (
        "Jonas Weber",
        "70 Granary Street\nKingsholt\nKH2 1UF",
        "jonas.weber@example.com",
    ),
    (
        "Kemi Adeyemi",
        "25 Lantern Way\nMeadowford\nMF3 2ZT",
        "kemi.adeyemi@example.com",
    ),
    (
        "Leo Novak",
        "41 Copper Lane\nRiverside\nRS9 7GA",
        "leo.novak@example.com",
    ),
];

/// The table app's colours, cycled with `c`: tailwind's.
pub const PALETTES: [(&str, Palette); 4] = [
    ("blue", BLUE),
    ("emerald", EMERALD),
    ("indigo", INDIGO),
    ("red", RED),
];

/// The first table: a header with a gap, a footer, column spacing, the
/// row, column and cell highlights and a symbol.
fn dishes_table(row: Signal<usize>, column: Signal<usize>) -> Node {
    let columns = vec![
        Column::new("Dish", Size::Flex(2)),
        Column::new("Course", Size::Flex(1)),
        Column::new("Price", Size::Fixed(6)).align(Align::End),
        Column::new("Time", Size::Fixed(7)).align(Align::End),
    ];
    let rows = || {
        DISHES
            .iter()
            .map(|d| d.iter().map(|c| c.to_string()).collect())
            .collect()
    };
    let total: f64 = DISHES.iter().filter_map(|d| d[2].parse::<f64>().ok()).sum();
    let options = TableOptions::default()
        .cells(column)
        .column_highlight()
        .symbol("🍴 ")
        .header_gap(1)
        .column_spacing(2)
        .footer(move || {
            vec![
                format!("{} dishes", DISHES.len()),
                String::new(),
                format!("{total:.2}"),
                String::new(),
            ]
        });
    table_with(columns, rows, row, options)
        .theme_styles([
            ("table.header", "bold"),
            ("table.footer", "italic"),
            ("selected", "on grey30"),
            ("table.selected", "on grey30"),
            ("selected.column", "on grey15"),
            ("selected.cell", "bold black on yellow"),
        ])
        .name("dishes")
        .label("Dishes, a table")
        .panel("Table")
        .title_bottom("[dim]↑/↓ row · ←/→ cell · v the table app[/]")
}

/// The table app: stripes, rows of four lines, the column highlight, the
/// palette `palette` of [`PALETTES`], and a scrollbar.
fn people_table(palette: usize, row: Signal<usize>, column: Signal<usize>) -> Node {
    let (name, colours) = PALETTES[palette % PALETTES.len()];
    let columns = vec![
        Column::new("Name", Size::Flex(1)),
        Column::new("Address", Size::Flex(1)),
        Column::new("Email", Size::Flex(2)),
    ];
    let rows = || {
        PEOPLE
            .iter()
            .map(|(name, address, email)| {
                vec![name.to_string(), address.to_string(), email.to_string()]
            })
            .collect()
    };
    let options = TableOptions::default()
        .cells(column)
        .column_highlight()
        .multiline()
        .row_height(3)
        .row_gap(1)
        .symbol(" █ ")
        .row_style(|i| {
            Some(if i % 2 == 0 {
                format!("{} on {}", SLATE.c200, SLATE.c950)
            } else {
                format!("{} on {}", SLATE.c200, SLATE.c900)
            })
        });
    let header = format!("bold {} on {}", SLATE.c200, colours.c900);
    let selected = format!("reverse {}", colours.c400);
    table_with(columns, rows, row, options)
        .scrollbar(
            Scrollbar::new()
                .symbols(ScrollbarSymbols::DOUBLE_VERTICAL)
                .ends(None, None),
        )
        .theme_styles([
            ("table.header", header.clone()),
            ("selected", selected.clone()),
            ("table.selected", selected),
            ("selected.column", colours.c400.to_string()),
            ("selected.cell", format!("reverse {}", colours.c600)),
        ])
        .name("people")
        .label("People, the table app")
        .panel(&format!("Table app · {name}"))
        .border_style(colours.c400)
        .title_bottom("[dim]↑/↓ row · ←/→ cell · c colours · v the first table[/]")
}

/// Table: the widget example, or the table app.
fn tables() -> Node {
    let app = signal(false);
    let palette = signal(0usize);
    let (dish, dish_column) = (signal(0usize), signal(0usize));
    let (person, person_column) = (signal(0usize), signal(0usize));
    switch(
        move || (app.get(), palette.get()),
        move |(app, palette)| {
            if app {
                people_table(palette, person, person_column)
            } else {
                dishes_table(dish, dish_column)
            }
        },
    )
    .bind("v", "the other table", move |_| app.update(|a| *a = !*a))
    .bind("c", "the table app's next colours", move |_| {
        palette.update(|p| *p = (*p + 1) % PALETTES.len())
    })
}

/// The inner tabs' titles.
pub const INNER: [&str; 4] = ["Overview", "Logs", "Metrics", "Settings"];

/// Tabs: three looks of one strip, and its content.
fn tab_strips() -> Node {
    let inner = signal(0usize);
    let titles = || INNER.iter().map(|t| t.to_string()).collect();
    let strip = |node: Node, title: &str| {
        node.theme_styles([
            ("tabs.selected", "bold yellow"),
            ("tabs.selected.blur", "bold yellow"),
        ])
        .no_focus()
        .panel(title)
        .fixed(3)
    };
    let content = text(move || {
        format!(
            "\n\n[bold]{}[/]\n\nThe content of the selected tab, centred.",
            INNER[inner.get()]
        )
    })
    .text_align(TextAlign::Center)
    .name("tab-content")
    .panel("Content");
    column([
        strip(
            tabs(titles, inner),
            "Default: a line between, a space each side",
        ),
        strip(
            tabs(titles, inner).divider(" • "),
            "A dot between, a space each side",
        ),
        strip(
            tabs(titles, inner).divider("").tab_padding("[", "]"),
            "Nothing between, brackets each side",
        ),
        strip(
            tabs(titles, inner).titles_align(Align::Center),
            "In the middle",
        ),
        content.flex(1),
    ])
    .bind("n", "the next inner tab", move |_| {
        inner.update(|t| *t = (*t + 1) % INNER.len())
    })
    .bind("p", "the inner tab before", move |_| {
        inner.update(|t| *t = (*t + INNER.len() - 1) % INNER.len())
    })
    .focusable()
    .name("tabs-page")
}

/// The lines the scrolling views show.
fn scroll_text() -> String {
    (1..=30)
        .map(|n| format!("Line {n:>2}: the quick brown fox jumps over the lazy dog, {n} times over, and then rests in the shade of the old oak tree."))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Scrollbar: ratatui's `scrollbar` app's four looks.
fn scrollbars() -> Node {
    let down = signal(0u16);
    let across = signal(0u16);
    let lines = scroll_text();
    let vertical = |bar: Scrollbar| {
        scroll_with(label(lines.clone()).no_wrap(), down)
            .scrollbar(bar)
            .no_focus()
    };
    let horizontal = |bar: Scrollbar| {
        scroll_both_with(
            label(lines.clone()).no_wrap(),
            across,
            signal(0),
            true,
            false,
        )
        .scrollbar(bar)
        .no_focus()
    };
    column([
        vertical(
            Scrollbar::new()
                .symbols(ScrollbarSymbols::VERTICAL)
                .ends(Some("↑"), Some("↓"))
                .show(Show::Always),
        )
        .name("right")
        .panel("Down the right, with arrows")
        .flex(1),
        vertical(
            Scrollbar::new()
                .side(Side::Left)
                .track(None)
                .ends(None, None)
                .show(Show::Always),
        )
        .name("left")
        .panel("Down the left, no track")
        .flex(1),
        horizontal(
            Scrollbar::new()
                .side(Side::Bottom)
                .symbols(ScrollbarSymbols::HORIZONTAL)
                .ends(Some("◄"), None)
                .thumb("▬")
                .show(Show::Always),
        )
        .name("bottom")
        .panel("Along the bottom, a begin arrow and a thumb of its own")
        .flex(1),
        horizontal(
            Scrollbar::new()
                .side(Side::Bottom)
                .track(Some("-"))
                .thumb("#")
                .show(Show::Always),
        )
        .name("track")
        .panel("Along the bottom, a track of its own")
        .flex(1),
    ])
    .bind("j", "scroll down", move |_| {
        down.update(|d| *d = (*d + 1).min(29))
    })
    .bind("k", "scroll up", move |_| {
        down.update(|d| *d = d.saturating_sub(1))
    })
    .bind("l", "scroll right", move |_| {
        across.update(|a| *a = (*a + 4).min(100))
    })
    .bind("h", "scroll left", move |_| {
        across.update(|a| *a = a.saturating_sub(4))
    })
    .focusable()
    .name("scrollbars")
}

/// The month the Calendar tab opens on.
pub const FIRST_MONTH: Date = Date::new(2026, 10, 1);

/// The events on the calendar, and their styles: our own.
pub const EVENTS: [(Date, &str, &str); 6] = [
    (Date::new(2026, 10, 5), "Harvest fair", "bold green"),
    (Date::new(2026, 10, 16), "Team offsite", "bold blue"),
    (Date::new(2026, 10, 31), "Lantern night", "bold yellow"),
    (Date::new(2026, 11, 11), "Remembrance", "red"),
    (Date::new(2026, 11, 26), "Thanksgiving", "bold magenta"),
    (
        Date::new(2026, 12, 25),
        "Midwinter feast",
        "bold red underline",
    ),
];

/// Calendar: a month with events, the days around it and a weekday header;
/// the next month with the weeks starting on Sunday.
fn calendars() -> Node {
    let shown = signal(FIRST_MONTH);
    let events = || {
        EVENTS
            .iter()
            .fold(CalendarOptions::new(), |options, (date, _, style)| {
                options.event(*date, *style)
            })
    };
    let first = month(move || shown.get(), events().surrounding("dim"))
        .name("month")
        .label("The month, with its events")
        .padding(0, 1)
        .panel("Surrounding days")
        .fixed(24);
    let second = month(
        move || shown.get().add_months(1),
        events()
            .first_weekday(Weekday::Sunday)
            .weekday_style("bold cyan")
            .month_style("bold underline"),
    )
    .name("next-month")
    .label("The next month")
    .padding(0, 1)
    .panel("Sunday first")
    .fixed(24);
    let legend = text(move || {
        let at = shown.get();
        let next = at.add_months(1);
        let lines: Vec<String> = EVENTS
            .iter()
            .filter(|(d, _, _)| {
                (d.year, d.month) == (at.year, at.month)
                    || (d.year, d.month) == (next.year, next.month)
            })
            .map(|(d, name, style)| {
                format!("[{style}]{:>2} {}[/] {name}", d.day, &d.month_name()[..3])
            })
            .collect();
        if lines.is_empty() {
            "[dim]No events in these months.[/]".to_string()
        } else {
            lines.join("\n")
        }
    })
    .name("events")
    .panel("Events")
    .title_bottom("[dim]n/p month[/]");
    row([
        column([first.fixed(10), label("")]).fixed(24),
        column([second.fixed(10), label("")]).fixed(24),
        legend.flex(1),
    ])
    .gap(1)
    .bind("n", "the next month", move |_| {
        shown.update(|d| *d = d.add_months(1))
    })
    .bind("p", "the month before", move |_| {
        shown.update(|d| *d = d.add_months(-1))
    })
    .focusable()
    .name("calendars")
}

/// The pop-up `p` opens: 60% wide and 20% tall, centred.
fn popup() -> Node {
    label("A pop-up over the blue panel, with a shadow.\n[dim]p or Esc closes it.[/]")
        .name("popup")
        .panel("Popup")
        .title_align(Align::Center)
        .style("white on #3b2f63")
        .bind("p esc", "close the pop-up", |cx| cx.pop())
}

/// Popup: a pop-up over a panel, and four shadows.
fn popups() -> Node {
    let behind = label(
        "This panel sits under the pop-up. Press [bold]p[/] to open it: a box of \
         60% by 20% of the screen, centred, with a shadow over what is below.",
    )
    .name("behind")
    .panel("Behind")
    .border_style("bright_white")
    .style("white on blue");
    let shadow = |kind: ShadowKind, name: &str| {
        label(name.to_string())
            .text_align(TextAlign::Center)
            .panel("")
            .shadow(kind)
            .flex(1)
    };
    column([
        behind.flex(1),
        row([
            shadow(ShadowKind::LightShade, "Light"),
            shadow(ShadowKind::MediumShade, "Medium"),
            shadow(ShadowKind::DarkShade, "Dark"),
            shadow(ShadowKind::Dim, "Dim"),
        ])
        .gap(2)
        .name("shadows")
        .fixed(5),
    ])
    .bind("p", "open the pop-up", |cx| {
        cx.modal_with(
            Size::Percent(60),
            Size::Percent(20),
            ModalOptions::new().shadow("on grey11"),
            popup,
        )
    })
    .focusable()
    .name("popups")
}

/// Borders: four panes sharing their borders, the one with the focus thick.
fn borders() -> Node {
    let pane = |name: &str, text: &str| {
        label(text.to_string())
            .focusable()
            .name(&name.to_lowercase().replace(' ', "-"))
            .panel(name)
    };
    grid(
        [Size::Flex(1), Size::Flex(1)],
        [
            pane("Top left", "Four panes share their borders."),
            pane("Top right", "The lines join where they meet."),
            pane("Bottom left", "The pane with the focus is drawn thick."),
            pane("Bottom right", "↑/↓, Tab or a click pick a pane."),
        ],
    )
    .rows([Size::Flex(1)])
    .collapse_borders()
    .focus_border(Border::Thick)
    .bind("down", "the next pane", |cx| cx.focus_next())
    .bind("up", "the pane before", |cx| cx.focus_previous())
    .name("borders")
}

/// A button's colours: its face, its lit top edge, its shadowed bottom
/// edge, and its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub face: &'static str,
    pub light: &'static str,
    pub shade: &'static str,
    pub text: &'static str,
}

/// The three buttons, and their looks when idle and when picked.
pub const BUTTONS: [(&str, Look, Look); 3] = [
    (
        "Red",
        Look {
            face: "#802020",
            light: "#c04040",
            shade: "#401010",
            text: "#f0d0d0",
        },
        Look {
            face: "#b03030",
            light: "#f06060",
            shade: "#601818",
            text: "#ffffff",
        },
    ),
    (
        "Green",
        Look {
            face: "#206020",
            light: "#40a040",
            shade: "#103010",
            text: "#d0f0d0",
        },
        Look {
            face: "#309030",
            light: "#60d060",
            shade: "#184818",
            text: "#ffffff",
        },
    ),
    (
        "Blue",
        Look {
            face: "#203080",
            light: "#4060c0",
            shade: "#101840",
            text: "#d0d8f0",
        },
        Look {
            face: "#3048b0",
            light: "#6080f0",
            shade: "#182460",
            text: "#ffffff",
        },
    ),
];

/// A button drawn by hand: a face, a lit top edge and a shadowed bottom
/// edge, which swap while it is pressed. It is picked while it has the
/// focus or the pointer is over it.
struct Button {
    index: usize,
    /// The button held down with the mouse, if any.
    held: Signal<Option<usize>>,
    /// How many times each button was pressed, and the last pressed.
    presses: Signal<([u32; 3], Option<usize>)>,
}

impl Button {
    fn press(&self) {
        let i = self.index;
        self.presses.update(|(counts, last)| {
            counts[i] += 1;
            *last = Some(i);
        });
    }
}

impl Widget for Button {
    fn name(&self) -> &'static str {
        "button"
    }

    fn role(&self) -> intuituive::a11y::Role {
        intuituive::a11y::Role::Button
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (label, idle, picked) = BUTTONS[self.index];
        let pressed = self.held.get() == Some(self.index);
        let look = if pressed || cx.focused() || cx.hovered() {
            picked
        } else {
            idle
        };
        let (top, bottom) = if pressed {
            (look.shade, look.light)
        } else {
            (look.light, look.shade)
        };
        let style = |fg: &str| rich::Style::parse(&format!("{fg} on {}", look.face)).ok();
        let (width, height) = (canvas.width(), canvas.height());
        canvas.fill(0, 0, width, height, style(look.text).as_ref());
        if height >= 3 {
            canvas.print(0, 0, &"▔".repeat(width as usize), style(top).as_ref());
            canvas.print(
                0,
                height - 1,
                &"▁".repeat(width as usize),
                style(bottom).as_ref(),
            );
        }
        let text = if pressed {
            format!("[{label}]")
        } else {
            label.to_string()
        };
        let x = width.saturating_sub(text.chars().count() as u16) / 2;
        canvas.print(x, height / 2, &text, style(look.text).as_ref());
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse) if mouse.is_press() => {
                self.held.set(Some(self.index));
                cx.capture_mouse();
                Used::Yes
            }
            WidgetEvent::Mouse(mouse) if matches!(mouse.kind, MouseKind::Up(_)) => {
                cx.release_mouse();
                if self.held.get_untracked() == Some(self.index) {
                    self.held.set(None);
                    let (width, height) = cx.size();
                    // Let go over the button: a click.
                    if mouse.column < width && mouse.row < height {
                        self.press();
                    }
                }
                Used::Yes
            }
            WidgetEvent::Key(key) if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) => {
                self.press();
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// Custom: three buttons drawn by hand.
fn buttons() -> Node {
    let held = signal(None::<usize>);
    let presses = signal(([0u32; 3], None::<usize>));
    let button = |index: usize| {
        widget(Button {
            index,
            held,
            presses,
        })
        .name(&BUTTONS[index].0.to_lowercase())
        .label(BUTTONS[index].0)
        .fixed(15)
    };
    let status = text(move || {
        let (counts, last) = presses.get();
        match last {
            None => "Press a button: Enter or Space, or a click.".to_string(),
            Some(i) => {
                let n = counts[i];
                let times = if n == 1 { "time" } else { "times" };
                format!("[bold]{}[/] pressed, {n} {times}.", BUTTONS[i].0)
            }
        }
    })
    .name("status")
    .live();
    column([
        label(
            "Three buttons, each a widget of its own: it draws its face and edges, \
               takes the pointer and the keys, and looks picked while it has the focus \
               or the pointer, and pressed while a button is held on it.",
        )
        .auto(),
        label("").fixed(1),
        row([button(0), button(1), button(2)])
            .gap(3)
            .justify(Justify::Center)
            .fixed(3),
        label("").fixed(1),
        status.text_align(TextAlign::Center).fixed(1),
    ])
    .padding(1, 2)
    .bind("down", "the next button", |cx| cx.focus_next())
    .bind("up", "the button before", |cx| cx.focus_previous())
    .name("custom")
}

/// The page for tab `i`.
fn page(i: usize) -> Node {
    match i {
        0 => blocks(),
        1 => text_page(),
        2 => lists(),
        3 => tables(),
        4 => tab_strips(),
        5 => scrollbars(),
        6 => calendars(),
        7 => popups(),
        8 => borders(),
        _ => buttons(),
    }
}

/// The gallery, as `options` says.
pub fn widgets_app(options: Options) -> App {
    App::new(move || {
        let tab = signal(options.tab.min(TABS.len() - 1));
        let strip = tabs(|| TABS.iter().map(|t| t.to_string()).collect(), tab)
            .divider("")
            .theme_styles([
                ("tabs.selected", "bold black on cyan"),
                ("tabs.selected.blur", "bold black on cyan"),
            ])
            .no_focus()
            .name("tabs")
            .label("Widget families")
            .fixed(1);
        let body = switch(move || tab.get(), page).name("page");
        let footer = label("[dim]←/→ or 1-9, 0 tabs · ? keys · Ctrl+P commands · q quit[/]")
            .no_wrap()
            .name("footer")
            .fixed(1);
        let mut root = column([strip, body.flex(1), footer])
            .bind("left", "the tab before", move |_| {
                tab.update(|t| *t = (*t + TABS.len() - 1) % TABS.len())
            })
            .bind("right", "the next tab", move |_| {
                tab.update(|t| *t = (*t + 1) % TABS.len())
            })
            .bind("q", "quit", |cx| cx.quit());
        for (i, name) in TABS.iter().enumerate() {
            let key = ((i + 1) % 10).to_string();
            root = root.bind(&key, &format!("the {name} tab"), move |_| tab.set(i));
        }
        root
    })
    .help_key("?")
    .palette_key("ctrl+p")
    .linear(options.linear)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    widgets_app(options).run()
}
