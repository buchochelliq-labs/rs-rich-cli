//! A port of ratatui's `demo2` example
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps/demo2>),
//! rebuilt on intuiTUIve. It reuses none of ratatui's code: it rebuilds
//! the behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The
//! recipe, the emails, the traceroute's hosts, the weather and the mark
//! are our own.
//!
//!     cargo run -p rs-rich-intuituive --example demo2
//!     cargo run -p rs-rich-intuituive --example demo2 -- crates/rich-intuituive/examples/demo2.tcss
//!
//! Its colours are in `demo2.tcss`, a stylesheet. Given the sheet's path,
//! the app reads it again whenever it changes, so you can recolour the
//! demo while it runs; without one, it uses the same sheet, built in.
//! `--today 2026-10-18` sets the day the calendar marks, for recordings
//! that come out the same on any day.
//!
//! h/l, ←/→ or Tab change tabs · j/k or ↓/↑ move inside a tab (the mark's
//! colour, the ingredient, the email, the hop, the download) · d or Delete
//! destroys the screen, and again brings it back · ? lists the keys ·
//! Ctrl+P finds a command · q or Esc quits. A click picks a tab, an
//! ingredient, an email or a hop, and the wheel moves through them.
//!
//! What intuiTUIve adds to the original:
//!
//! - **A stylesheet** holds the colours of the frame, the panels and the
//!   key chips, reloaded live, rather than a struct of styles in the code.
//!   The highlights inside the widgets (a selected row, the selected tab)
//!   are theme styles given to each widget with `.theme_styles(..)`.
//! - **The mouse** picks tabs, ingredients, emails and hops.
//! - **It fits any size.** Under 80 columns the swatch's margins go, under
//!   60 the map and the mark hide. ratatui's is fixed at 81×18, the size
//!   of its recording.
//! - **Destroy is an effect** over the whole app (`effect(..)`), driven
//!   by an animation (`cx.animate`), on the virtual clock in the tests.
//! - **Only what moves redraws:** j/k redraw the list or table and what
//!   shows the selection, not the swatch, the title bar or the key chips.

use std::path::PathBuf;
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::rich::{Color, Segment, Style};
use intuituive::screen::Rect;
use intuituive::widget::{Canvas as WidgetCanvas, DrawCx, MeasureCx};
use intuituive::widgets::{
    month, table_with, tabs, CalendarOptions, Column, Date, Scrollbar, Side, TableOptions,
};
use intuituive::Easing;
use rich_ext::chart::{
    Bar, BarChart, Canvas, LineGauge, LineSet, MapResolution, Marker, Orientation, Sparkline,
};
use rich_ext::palette::{blend, okhsv};

/// The sheet used when no file is given: the one beside this example.
pub const SHEET: &str = include_str!("demo2.tcss");

/// The app's name, in the title bar.
pub const NAME: &str = "intuiTUIve";

/// The tabs, in order. About's title is blank, as ratatui's is.
pub const TABS: [&str; 5] = ["About", "Recipe", "Email", "Traceroute", "Weather"];

/// The colours the widgets' highlights and the drawings use. The frame's
/// colours are the stylesheet's.
pub mod colour {
    pub const DARK_BLUE: &str = "#101830";
    pub const LIGHT_BLUE: &str = "#4060c0";
    pub const LIGHT_YELLOW: &str = "#c0c060";
    pub const LIGHT_GREEN: &str = "#40c060";
    pub const LIGHT_RED: &str = "#c06060";
    pub const LIGHT_GREY: &str = "#bcbcbc";
    pub const WHITE: &str = "#eeeeee";
    pub const SEASON: &str = "#203060";
}

/// The selected tab: white, reversed over the dark blue.
const TAB_SELECTED: &str = "bold reverse #eeeeee on #101830";

/// The width from which the swatch keeps its margins round a tab.
pub const WIDE: u16 = 80;
/// The width from which the map and the mark show.
pub const ROOMY: u16 = 60;

/// How the demo runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The stylesheet to read, and read again when it changes; `None` for
    /// the built-in [`SHEET`].
    pub sheet: Option<PathBuf>,
    /// The day the calendar marks as today.
    pub today: Date,
    /// The seed of the destroy effect's drips.
    pub seed: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            sheet: None,
            today: Date::today_utc(),
            seed: 0x0d15_ea5e,
        }
    }
}

impl Options {
    /// `[--today YYYY-MM-DD] [SHEET]`, as the command line gives them.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let day = match arg.strip_prefix("--today=") {
                Some(day) => Some(day.to_string()),
                None if arg == "--today" => Some(args.next().unwrap_or_default()),
                None => None,
            };
            match day {
                Some(day) => {
                    options.today = day
                        .parse()
                        .map_err(|_| format!("--today takes a date, 2026-10-18, not {day:?}"))?;
                }
                None if arg.starts_with('-') => return Err(format!("unknown argument {arg}")),
                None => options.sheet = Some(PathBuf::from(arg)),
            }
        }
        Ok(options)
    }
}

/// A seeded xorshift: the same numbers every run (the crate has no `rand`).
#[derive(Clone, Copy, Debug)]
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> XorShift {
        XorShift(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
}

// ---------------------------------------------------------------------------
// Our own sample data.

/// The recipe's steps.
pub const STEPS: [(&str, &str); 2] = [
    (
        "Step 1: ",
        "Warm the oil in a deep pan over a medium heat. Add the onion, carrot and \
         celery with a pinch of salt, and cook them gently for ten minutes, until \
         they are soft but not brown.",
    ),
    (
        "Step 2: ",
        "Stir in the garlic, cumin and paprika for a minute. Add the lentils, the \
         tomatoes and the stock, and simmer for twenty-five minutes, until the \
         lentils fall apart. Season with the lemon juice and plenty of pepper.",
    ),
];

/// The ingredients: a quantity, and a name that may take several lines.
pub const INGREDIENTS: [(&str, &str); 12] = [
    ("2 tbsp", "olive oil"),
    ("1", "onion, finely\nchopped"),
    ("1", "carrot, diced"),
    ("1", "celery stick,\ndiced"),
    ("2", "garlic cloves,\ncrushed"),
    ("1 tsp", "ground cumin"),
    ("1 tsp", "smoked paprika"),
    ("200 g", "red lentils,\nrinsed"),
    ("400 g", "tinned chopped\ntomatoes"),
    ("1 l", "vegetable stock"),
    ("½", "lemon, juiced"),
    ("", "black pepper,\nto taste"),
];

/// An email in the inbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Email {
    pub from: &'static str,
    pub subject: &'static str,
    pub body: &'static str,
}

pub const EMAILS: [Email; 5] = [
    Email {
        from: "Priya <priya@example.com>",
        subject: "Lunch on Friday?",
        body: "Hi all,\nThe noodle place by the station has reopened. Friday at noon?\n\nPriya",
    },
    Email {
        from: "Sam <sam@example.com>",
        subject: "Re: Lunch on Friday?",
        body: "Count me in, but I can only stay for an hour.\n\nSam",
    },
    Email {
        from: "Lena <lena@example.com>",
        subject: "Re: Lunch on Friday?",
        body: "Friday works for me. Shall I book a table for six?\n\nLena",
    },
    Email {
        from: "Ops bot <ops@example.com>",
        subject: "Disk at 81% on build-3",
        body: "build-3: /var is 81% full.\nThis message was sent automatically.\n\nOps bot",
    },
    Email {
        from: "Tom <tom@example.com>",
        subject: "Re: Lunch on Friday? (booked)",
        body: "Booked for six at a quarter past twelve. See you there!\n\nTom",
    },
];

/// A hop of the traceroute, and where it is (longitude, latitude).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hop {
    pub host: &'static str,
    pub address: &'static str,
    pub at: (f64, f64),
}

const LISBON: (f64, f64) = (-9.14, 38.72);
const PORTO: (f64, f64) = (-8.61, 41.15);
const MADRID: (f64, f64) = (-3.70, 40.42);
const PARIS: (f64, f64) = (2.35, 48.86);
const LONDON: (f64, f64) = (-0.13, 51.51);
const DUBLIN: (f64, f64) = (-6.26, 53.35);
const AMSTERDAM: (f64, f64) = (4.90, 52.37);
const FRANKFURT: (f64, f64) = (8.68, 50.11);
const COPENHAGEN: (f64, f64) = (12.57, 55.68);
const OSLO: (f64, f64) = (10.75, 59.91);
const STOCKHOLM: (f64, f64) = (18.07, 59.33);
const WARSAW: (f64, f64) = (21.01, 52.23);
const VIENNA: (f64, f64) = (16.37, 48.21);
const MILAN: (f64, f64) = (9.19, 45.46);
const ROME: (f64, f64) = (12.50, 41.90);
const ATHENS: (f64, f64) = (23.73, 37.98);

/// Where the traceroute goes. The names and addresses are made up (the
/// addresses are from the ranges kept for documentation).
pub const HOPS: [Hop; 16] = [
    Hop {
        host: "home.lan",
        address: "192.168.1.1",
        at: LISBON,
    },
    Hop {
        host: "gw.opo.example.net",
        address: "198.51.100.1",
        at: PORTO,
    },
    Hop {
        host: "core1.mad.example.net",
        address: "198.51.100.17",
        at: MADRID,
    },
    Hop {
        host: "core2.par.example.net",
        address: "198.51.100.33",
        at: PARIS,
    },
    Hop {
        host: "ix.lon.example.net",
        address: "203.0.113.5",
        at: LONDON,
    },
    Hop {
        host: "edge.dub.example.net",
        address: "203.0.113.21",
        at: DUBLIN,
    },
    Hop {
        host: "core1.ams.example.net",
        address: "203.0.113.37",
        at: AMSTERDAM,
    },
    Hop {
        host: "core3.fra.example.net",
        address: "192.0.2.10",
        at: FRANKFURT,
    },
    Hop {
        host: "ix.cph.example.net",
        address: "192.0.2.26",
        at: COPENHAGEN,
    },
    Hop {
        host: "edge.osl.example.net",
        address: "192.0.2.42",
        at: OSLO,
    },
    Hop {
        host: "core1.sto.example.net",
        address: "192.0.2.58",
        at: STOCKHOLM,
    },
    Hop {
        host: "core2.waw.example.net",
        address: "198.51.100.65",
        at: WARSAW,
    },
    Hop {
        host: "edge.vie.example.net",
        address: "198.51.100.81",
        at: VIENNA,
    },
    Hop {
        host: "core1.mil.example.net",
        address: "198.51.100.97",
        at: MILAN,
    },
    Hop {
        host: "ix.rom.example.net",
        address: "203.0.113.69",
        at: ROME,
    },
    Hop {
        host: "cdn.ath.example.net",
        address: "203.0.113.85",
        at: ATHENS,
    },
];

/// The region the traceroute's map shows: longitudes, then latitudes.
pub const REGION: ([f64; 2], [f64; 2]) = ([-14.0, 30.0], [34.0, 63.0]);

/// The ping times, a value a column, turned by the selected hop.
pub fn pings() -> Vec<f64> {
    (0..80)
        .map(|i| {
            let t = i as f64;
            let wave = 4.5 + 2.5 * (t * 0.31).sin() + 1.5 * (t * 0.83).cos();
            wave.round().clamp(1.0, 8.0)
        })
        .collect()
}

/// The week's highs, in °F.
pub const WEEK: [(&str, u32); 7] = [
    ("Mon", 64),
    ("Tue", 68),
    ("Wed", 73),
    ("Thu", 77),
    ("Fri", 71),
    ("Sat", 66),
    ("Sun", 62),
];

/// The seasons' lows and highs, in °F, by the months they start in.
pub const SEASONS: [(&str, u32, u32, u32); 4] = [
    ("Winter", 12, 38, 49),
    ("Spring", 3, 45, 63),
    ("Summer", 6, 58, 79),
    ("Autumn", 9, 47, 66),
];

/// The season `date` is in.
fn season_of(date: Date) -> usize {
    match date.month {
        12 | 1 | 2 => 0,
        3..=5 => 1,
        6..=8 => 2,
        _ => 3,
    }
}

// ---------------------------------------------------------------------------
// The swatch, the mark and the wordmark.

/// A cell of the Okhsv swatch at `x`, `y` of `width` x `height`: its top
/// half's colour and its bottom half's. The hue goes round across, and the
/// value falls down.
pub fn swatch_cell(x: u16, y: u16, width: u16, height: u16) -> (Color, Color) {
    let (width, height) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let hue = f64::from(x) * 360.0 / width;
    let value = height - f64::from(y);
    (
        okhsv(hue, 1.0, value / height),
        okhsv(hue, 1.0, (value - 0.5) / height),
    )
}

/// intuiTUIve's mark, a pixel a character, two pixels to a cell: a
/// terminal window with a prompt. `F` is the frame, `R`, `Y` and `G` the
/// window's buttons, `B` the screen, `P` the prompt, and `.` lets the
/// swatch show.
pub const MARK: [&str; 14] = [
    ".FFFFFFFFFFFFFFFFFFFFFFFF.",
    "FFFFFFFFFFFFFFFFFFFFFFFFFF",
    "FFRFFYFFGFFFFFFFFFFFFFFFFF",
    "FFFFFFFFFFFFFFFFFFFFFFFFFF",
    "FBBBBBBBBBBBBBBBBBBBBBBBBF",
    "FBBPPBBBBBBBBBBBBBBBBBBBBF",
    "FBBBPPBBBBBBBBBBBBBBBBBBBF",
    "FBBBBPPBBBBBBBBBBBBBBBBBBF",
    "FBBBPPBBBBBBBBBBBBBBBBBBBF",
    "FBBPPBBBBPPPPPPBBBBBBBBBBF",
    "FBBBBBBBBBBBBBBBBBBBBBBBBF",
    "FBBBBBBBBBBBBBBBBBBBBBBBBF",
    "FFFFFFFFFFFFFFFFFFFFFFFFFF",
    ".FFFFFFFFFFFFFFFFFFFFFFFF.",
];

/// The prompt's colours, which j and k step through.
pub const PROMPT: [&str; 4] = [
    colour::LIGHT_GREEN,
    colour::LIGHT_YELLOW,
    colour::LIGHT_RED,
    colour::LIGHT_BLUE,
];

/// The colour of a mark pixel, or `None` where the swatch shows.
fn mark_pixel(c: char, prompt: usize) -> Option<Color> {
    let hex = match c {
        'F' => "#c8c8c8",
        'B' => "#080808",
        'R' => colour::LIGHT_RED,
        'Y' => colour::LIGHT_YELLOW,
        'G' => colour::LIGHT_GREEN,
        'P' => PROMPT[prompt % PROMPT.len()],
        _ => return None,
    };
    Color::parse(hex).ok()
}

/// The wordmark's rows as cells, each `true` where a block goes: our own
/// letters for "intuiTUIve", five rows tall.
pub fn wordmark() -> Vec<Vec<bool>> {
    // Each letter's rows; a blank column between letters.
    const LETTERS: [[&str; 5]; 10] = [
        ["#", " ", "#", "#", "#"],                     // i
        ["    ", "### ", "#  #", "#  #", "#  #"],      // n
        [" #  ", "####", " #  ", " #  ", "  ##"],      // t
        ["    ", "#  #", "#  #", "#  #", " ###"],      // u
        ["#", " ", "#", "#", "#"],                     // i
        ["#####", "  #  ", "  #  ", "  #  ", "  #  "], // T
        ["#   #", "#   #", "#   #", "#   #", " ### "], // U
        ["###", " # ", " # ", " # ", "###"],           // I
        ["     ", "#   #", "#   #", " # # ", "  #  "], // v
        ["    ", " ## ", "####", "#   ", " ###"],      // e
    ];
    (0..5)
        .map(|row| {
            let mut cells = Vec::new();
            for (n, letter) in LETTERS.iter().enumerate() {
                if n > 0 {
                    cells.push(false);
                }
                cells.extend(letter[row].chars().map(|c| c == '#'));
            }
            cells
        })
        .collect()
}

/// Where a tab's content sits on its swatch, given the swatch's rectangle.
type Place = fn(Rect) -> Rect;

/// A tab's content in the swatch's margins, which go under [`WIDE`].
fn in_margins(rect: Rect) -> Rect {
    if rect.width >= WIDE {
        rect.inset(1, 2, 1, 2)
    } else {
        rect
    }
}

/// The About tab's description: right of the mark, well inside.
fn beside_mark(rect: Rect) -> Rect {
    if rect.width >= ROOMY {
        let right = Rect::new(rect.x + 34, rect.y, rect.width - 34, rect.height);
        if rect.width >= WIDE {
            right.inset(4, 2, 4, 2)
        } else {
            right.inset(1, 1, 1, 1)
        }
    } else {
        rect.inset(1, 1, 1, 1)
    }
}

/// A tab: an Okhsv swatch, drawn round its content as chrome, and on the
/// About tab the mark, whose prompt changes colour with j/k. Where the
/// content goes it leaves blank cells, as ratatui's demo clears the area
/// first, so the content's padding and gaps take its background rather
/// than show the swatch.
struct Swatch {
    content: Node,
    place: Place,
    mark: Option<Signal<usize>>,
    /// Where the content went, in the swatch's own cells.
    hole: Rect,
}

impl Widget for Swatch {
    fn name(&self) -> &'static str {
        "swatch"
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.content)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let placed = (self.place)(rect);
        self.hole = Rect::new(
            placed.x - rect.x,
            placed.y - rect.y,
            placed.width,
            placed.height,
        );
        vec![placed]
    }

    fn retained(&self) -> bool {
        true
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut WidgetCanvas) {
        let (width, height) = (canvas.width(), canvas.height());
        // Text mode shows no colours: a blank swatch.
        if intuituive::a11y::text_mode() {
            canvas.clear(0, 0, width, height);
            return;
        }
        let mut cells: Vec<Vec<(Color, Color)>> = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| swatch_cell(x, y, width, height))
                    .collect()
            })
            .collect();
        let prompt = self.mark.map(|m| m.get());
        if let Some(prompt) = prompt.filter(|_| width >= ROOMY && height >= 7) {
            let (x0, y0) = (4usize, (height as usize - MARK.len() / 2) / 2);
            for (py, line) in MARK.iter().enumerate() {
                for (px, c) in line.chars().enumerate() {
                    let Some(colour) = mark_pixel(c, prompt) else {
                        continue;
                    };
                    let cell = &mut cells[y0 + py / 2][x0 + px];
                    if py % 2 == 0 {
                        cell.0 = colour;
                    } else {
                        cell.1 = colour;
                    }
                }
            }
        }
        let hole = self.hole;
        let lines: Vec<Vec<Segment>> = cells
            .into_iter()
            .enumerate()
            .map(|(y, row)| {
                row.into_iter()
                    .enumerate()
                    .map(|(x, (top, bottom))| {
                        if hole.contains(x as u16, y as u16) {
                            Segment::new(" ", None)
                        } else {
                            Segment::new("▀", Some(Style::from_color(Some(top), Some(bottom))))
                        }
                    })
                    .collect()
            })
            .collect();
        canvas.lines(&lines);
    }
}

fn swatch(content: Node, place: Place) -> Node {
    widget(Swatch {
        content,
        place,
        mark: None,
        hole: Rect::default(),
    })
}

// ---------------------------------------------------------------------------
// The tabs.

/// The selections j and k move, one a tab.
#[derive(Clone, Copy)]
pub struct Rows {
    pub prompt: Signal<usize>,
    pub ingredient: Signal<usize>,
    pub email: Signal<usize>,
    pub hop: Signal<usize>,
    pub download: Signal<usize>,
}

fn about(rows: Rows) -> Node {
    let description = label(
        "[bold]- reactive terminal interfaces -[/]\n\n\
         intuiTUIve is a Rust framework for terminal apps. State lives in signals, \
         and only what changed is drawn again.",
    )
    .name("description")
    .panel(&format!("[bold]{NAME}[/]"))
    .borders(Sides::TOP)
    .title_align(Align::Center)
    .padding(1, 2)
    .class("content");
    widget(Swatch {
        content: description,
        place: beside_mark,
        mark: Some(rows.prompt),
        hole: Rect::default(),
    })
}

fn recipe(rows: Rows) -> Node {
    let steps = label(
        STEPS
            .iter()
            .map(|(step, text)| format!("[bold {}]{step}[/]{text}", colour::WHITE))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .name("steps");
    let ingredients = table_with(
        vec![
            Column::new("Qty", Size::Fixed(7)),
            Column::new("Ingredient", Size::Flex(1)),
        ],
        || {
            INGREDIENTS
                .iter()
                .map(|(qty, name)| vec![qty.to_string(), name.to_string()])
                .collect()
        },
        rows.ingredient,
        TableOptions::default().multiline(),
    )
    .scrollbar(Scrollbar::new().thumb("▐").track(None))
    .theme_styles([
        ("selected", colour::LIGHT_YELLOW),
        ("table.selected", colour::LIGHT_YELLOW),
        ("table.header", "bold underline"),
    ])
    .no_focus()
    .name("ingredients");
    let content = row([steps.ratio(3, 5), ingredients.flex(1)])
        .gap(1)
        .padding(1, 2)
        .panel(&format!(
            "[bold {}]Tomato and lentil soup[/]",
            colour::WHITE
        ))
        .border(Border::None)
        .title_align(Align::Center)
        .class("content");
    swatch(content, in_margins)
}

fn email(rows: Rows) -> Node {
    let selected = rows.email;
    let width = EMAILS
        .iter()
        .map(|e| e.from.chars().count())
        .max()
        .unwrap_or(0);
    let folders = label(format!(
        "[bold {}] Inbox [/][#808080] Sent  Drafts [/]",
        colour::WHITE
    ))
    .name("folders");
    let inbox = list(
        move || {
            EMAILS
                .iter()
                .map(|e| escape(&format!("{:width$} {}", e.from, e.subject)))
                .collect()
        },
        selected,
    )
    .symbol(">>")
    .scrollbar(Scrollbar::new().thumb("▐").track(None))
    .theme_styles([("selected", colour::LIGHT_YELLOW)])
    .no_focus()
    .name("inbox")
    .label("Inbox");
    let shown = move || EMAILS[selected.get() % EMAILS.len()];
    let headers = text(move || {
        let e = shown();
        format!(
            "[bold]From:[/] {}\n[bold]Subject:[/] {}",
            escape(e.from),
            escape(e.subject)
        )
    })
    .name("headers");
    let body = text(move || escape(shown().body)).name("body");
    let view = column([headers.fixed(2), fill("-", "dim").fixed(1), body.flex(1)])
        .padding(0, 2)
        .panel("")
        .borders(Sides::TOP)
        .border(Border::Thick)
        .name("email-view");
    let content = column([folders.fixed(1), inbox.fixed(4), view.flex(1)]).class("content");
    swatch(content, in_margins)
}

fn traceroute(rows: Rows) -> Node {
    let selected = rows.hop;
    let hops = table_with(
        vec![
            Column::new("Host", Size::Flex(1)),
            Column::new("Address", Size::Fixed(15)),
        ],
        || {
            HOPS.iter()
                .map(|h| vec![h.host.to_string(), h.address.to_string()])
                .collect()
        },
        selected,
        TableOptions::default(),
    )
    .scrollbar(Scrollbar::new().side(Side::Left).thumb("▌").track(None))
    .theme_styles([
        ("selected", colour::LIGHT_YELLOW),
        ("table.selected", colour::LIGHT_YELLOW),
        ("table.header", "bold underline"),
    ])
    .no_focus()
    .name("hops")
    .padding(1, 1)
    .panel(&format!(
        "[bold {}]Traceroute {}[/]",
        colour::WHITE,
        HOPS[HOPS.len() - 1].host
    ))
    .border(Border::None)
    .title_align(Align::Center);
    let ping = renderable(move || {
        let mut values = pings();
        let turn = selected.get() % values.len();
        values.rotate_left(turn);
        Sparkline::new(values).min(0.0).style(colour::WHITE)
    })
    .name("ping")
    .label("Ping times, a sparkline")
    .panel("Ping")
    .title_align(Align::Center)
    .border(Border::Thick)
    .fixed(5);
    let map = renderable(move || {
        let at = selected.get();
        let path = HOPS.get(at).zip(HOPS.get(at + 1));
        let ([west, east], [south, north]) = REGION;
        Canvas::new()
            .x_bounds(west, east)
            .y_bounds(south, north)
            .marker(Marker::HalfBlock)
            .background(colour::DARK_BLUE)
            .paint(|p| {
                p.map(MapResolution::High, colour::LIGHT_GREY);
            })
            .paint(|p| {
                if let Some((from, to)) = path {
                    p.line(from.at.0, from.at.1, to.at.0, to.at.1, colour::LIGHT_BLUE);
                    p.point(from.at.0, from.at.1, colour::LIGHT_GREEN);
                    p.point(to.at.0, to.at.1, colour::LIGHT_RED);
                }
            })
    })
    .name("map")
    .label("The route on a map")
    .padding(1, 0)
    .hide_below(ROOMY);
    let content = row([column([hops.flex(1), ping]).flex(1), map.percent(50)]).class("content");
    swatch(content, in_margins)
}

fn weather(rows: Rows, today: Date) -> Node {
    let calendar = month(
        move || today,
        CalendarOptions::new()
            .today(today)
            .event(today, "bold red")
            .month_style("bold")
            .weekday_style("italic"),
    )
    .name("calendar")
    .padding(1, 0);
    let week = renderable(|| {
        WEEK.iter().fold(
            BarChart::new()
                .orientation(Orientation::Vertical)
                .thickness(3)
                .bar_gap(1)
                .values_inside(true),
            |chart, (day, high)| {
                let hot = *high > 70;
                chart.push(
                    Bar::new(*day, f64::from(*high))
                        .text(format!("{high}°"))
                        .style(if hot { "red" } else { "yellow" })
                        .value_style(if hot {
                            "white on red bold"
                        } else {
                            "bright_black on yellow bold"
                        }),
                )
            },
        )
    })
    .name("week")
    .label("This week's highs, a bar chart");
    let now = season_of(today);
    let seasons = renderable(move || {
        SEASONS.iter().enumerate().fold(
            BarChart::new()
                .bar_gap(1)
                .style(colour::SEASON)
                .value_style(format!("white on {}", colour::SEASON))
                .values_inside(true),
            |chart, (n, (name, _, low, high))| {
                let bar = Bar::new("", f64::from(*high)).text(format!("{name} {low}-{high}"));
                chart.push(if n == now {
                    bar.value_style("bold")
                } else {
                    bar
                })
            },
        )
    })
    .name("seasons")
    .label("The seasons' temperatures, a bar chart")
    .padding(1, 0);
    let download = rows.download;
    let gauge = renderable(move || {
        let percent = (download.get() * 3).min(100);
        // The colour goes from yellow to red as it fills.
        let hue = 90.0 - percent as f64 * 0.6;
        let hex = |value: f64| {
            okhsv(hue, 1.0, value)
                .get_truecolor()
                .map(|c| c.hex())
                .unwrap_or_default()
        };
        let label = if percent < 100 {
            format!("Downloading: {percent}%")
        } else {
            "Download complete!".to_string()
        };
        LineGauge::new(percent as f64 / 100.0)
            .label(label)
            .label_style(colour::LIGHT_BLUE)
            .filled_style(hex(1.0))
            .unfilled_style(hex(0.5))
            .line(LineSet::Thick)
    })
    .name("download");
    let charts = row([calendar.fixed(22), week.fixed(28), seasons.flex(1)]).gap(1);
    let content = column([charts.flex(1), gauge.fixed(1)])
        .gap(1)
        .padding(1, 2)
        .class("content");
    swatch(content, in_margins)
}

// ---------------------------------------------------------------------------
// Destroy.

/// How long destroy runs, in seconds; then it holds.
pub const DESTROY_SECS: f64 = 10.0;
/// When the wordmark starts to fade in, and how long it takes.
const WORDMARK_FROM: f64 = 1.5;
const WORDMARK_FADE: f64 = 4.0;

/// Drip the cells down the screen, `t` seconds in: more cells move each
/// frame as it goes on, each a row down, and now and then one is thrown
/// back to the top or blanked. The moves are seeded, and made over what
/// the app drew afresh, so the same `t` comes out the same.
fn drip(canvas: &mut WidgetCanvas, t: f64, seed: u64) {
    let (width, height) = (canvas.width() as usize, canvas.height() as usize);
    if width == 0 || height < 4 {
        return;
    }
    // What is on the screen, and where each cell's content is now (`None`
    // for a blank).
    let mut before = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let cell = canvas.cell(x as u16, y as u16);
            before.push(cell.map(|c| {
                let movable = c.width() == 1 && !c.is_continuation();
                (c.symbol().to_string(), c.style().cloned(), movable)
            }));
        }
    }
    let mut grid: Vec<Option<usize>> = (0..width * height).map(Some).collect();
    let movable = |i: usize| before[i].as_ref().is_some_and(|c| c.2);
    let moves = ((t * t * t * t * 40.0) as usize).min(60_000);
    let mut rng = XorShift::new(seed);
    for _ in 0..moves {
        // A cell between the title bar and the key chips.
        let x = rng.below(width as u64) as usize;
        let y = 1 + rng.below((height - 3) as u64) as usize;
        let from = y * width + x;
        // One move in a hundred throws it back to the top, near where it
        // was, and mostly blanks it there; the rest move it a row down.
        let thrown = rng.below(100) == 0;
        let to = if thrown {
            let x = (x + rng.below(11) as usize)
                .saturating_sub(5)
                .min(width - 1);
            width + x
        } else {
            (y + 1).min(height - 2) * width + x
        };
        if !movable(from) || !movable(to) {
            continue;
        }
        grid[to] = if thrown && rng.below(10) != 0 {
            None
        } else {
            grid[from]
        };
    }
    for (i, now) in grid.iter().enumerate() {
        if *now == Some(i) {
            continue;
        }
        let (x, y) = ((i % width) as u16, (i / width) as u16);
        match now.and_then(|j| before[j].as_ref()) {
            Some((symbol, style, _)) => canvas.set_cell(x, y, symbol, style.as_ref()),
            None => canvas.set_cell(x, y, " ", None),
        }
    }
}

/// Fade the wordmark in over the middle of the screen, `t` seconds in:
/// its blocks go from the colour behind them to white.
fn fade_in(canvas: &mut WidgetCanvas, t: f64) {
    let share = ((t - WORDMARK_FROM) / WORDMARK_FADE).clamp(0.0, 1.0);
    if share <= 0.0 {
        return;
    }
    let mark = wordmark();
    let (w, h) = (mark[0].len() as u16, mark.len() as u16);
    if canvas.width() < w || canvas.height() < h {
        return;
    }
    let (x0, y0) = ((canvas.width() - w) / 2, (canvas.height() - h) / 2);
    let white = Color::from_rgb(255, 255, 255);
    let black = Color::from_rgb(0, 0, 0);
    for (dy, row) in mark.iter().enumerate() {
        for (dx, on) in row.iter().enumerate() {
            if !on {
                continue;
            }
            let (x, y) = (x0 + dx as u16, y0 + dy as u16);
            let behind = canvas
                .cell(x, y)
                .and_then(|c| c.style().and_then(|s| s.bgcolor().cloned()))
                .filter(|c| c.get_truecolor().is_some())
                .unwrap_or(black.clone());
            let colour = blend(&behind, &white, share);
            canvas.set_cell(x, y, "█", Some(&Style::from_color(Some(colour), None)));
        }
    }
}

// ---------------------------------------------------------------------------
// The app.

/// The demo, as `options` says.
pub fn demo2_app(options: Options) -> App {
    let Options { sheet, today, seed } = options;
    let app = App::new(move || {
        let tab = signal(0usize);
        let rows = Rows {
            prompt: signal(0),
            ingredient: signal(0),
            email: signal(0),
            hop: signal(0),
            download: signal(0),
        };
        let destroyed = signal(0.0f64);

        let title = label(format!("[bold]{NAME}[/]")).name("app-title");
        let strip = tabs(
            || {
                TABS.iter()
                    .map(|t| {
                        if *t == "About" {
                            String::new()
                        } else {
                            format!(" {t} ")
                        }
                    })
                    .collect()
            },
            tab,
        )
        .divider("")
        .tab_padding("", "")
        .theme_styles([
            ("tabs.selected", TAB_SELECTED),
            ("tabs.selected.blur", TAB_SELECTED),
        ])
        .no_focus()
        .name("tabs");
        let title_bar = row([title.flex(1), strip.fixed(43)]).name("title-bar");
        let body = switch(
            move || tab.get(),
            move |page| match page {
                0 => about(rows),
                1 => recipe(rows),
                2 => email(rows),
                3 => traceroute(rows),
                _ => weather(rows, today),
            },
        )
        .name("body");
        let keys = [
            ("H/←", "Left"),
            ("L/→", "Right"),
            ("K/↑", "Up"),
            ("J/↓", "Down"),
            ("D/Del", "Destroy"),
            ("Q/Esc", "Quit"),
        ];
        let chips: Vec<Node> = keys
            .iter()
            .flat_map(|(key, what)| {
                [
                    label(format!(" {key} ")).class("key").auto(),
                    label(format!(" {what} ")).class("desc").auto(),
                ]
            })
            .collect();
        let key_bar = row(chips).justify(Justify::Center).name("keys");
        let frame = column([title_bar.fixed(1), body.flex(1), key_bar.fixed(1)]).name("root");
        let app = effect(frame, move |canvas, _cx| {
            let t = destroyed.get();
            if t > 0.0 {
                drip(canvas, t, seed);
                fade_in(canvas, t);
            }
        });

        // j and k: each tab's own selection. Recipe, Email and Traceroute
        // go round; About and Weather stop at the ends.
        let step = move |by: isize| {
            let around = |s: Signal<usize>, len: usize| {
                s.update(|at| *at = (*at as isize + by).rem_euclid(len as isize) as usize)
            };
            let stop = |s: Signal<usize>| s.update(|at| *at = at.saturating_add_signed(by));
            match tab.get_untracked() {
                0 => stop(rows.prompt),
                1 => around(rows.ingredient, INGREDIENTS.len()),
                2 => around(rows.email, EMAILS.len()),
                3 => around(rows.hop, HOPS.len()),
                _ => stop(rows.download),
            }
        };
        app.bind("left h", "the tab before", move |_| {
            tab.update(|t| *t = t.saturating_sub(1))
        })
        .bind("right l tab", "the next tab", move |_| {
            tab.update(|t| *t = (*t + 1).min(TABS.len() - 1))
        })
        .bind("up k", "up, in this tab", move |_| step(-1))
        .bind("down j", "down, in this tab", move |_| step(1))
        .bind("d delete", "destroy, or bring back", move |cx| {
            if destroyed.get_untracked() > 0.0 {
                cx.animate(destroyed, 0.0, Duration::ZERO, Easing::Linear);
            } else {
                cx.animate(
                    destroyed,
                    DESTROY_SECS,
                    Duration::from_secs_f64(DESTROY_SECS),
                    Easing::Linear,
                );
            }
        })
        .bind("q esc", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p");
    match sheet {
        Some(path) => app.stylesheet_file(path),
        None => app.stylesheet(SHEET),
    }
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    demo2_app(options).run()
}
