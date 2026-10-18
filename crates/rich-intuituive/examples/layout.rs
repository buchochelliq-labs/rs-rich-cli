//! A port of ratatui's `constraint-explorer`, `constraints` and `flex`
//! examples (<https://github.com/ratatui/ratatui/tree/main/examples/apps>),
//! as one layout explorer rebuilt on intuiTUIve. It reuses none of
//! ratatui's code: it rebuilds the behaviour and the look with
//! intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example layout
//!
//! Tabs: Explorer, Sizes, Flex and Beyond. Tab and Shift+Tab change tabs,
//! and a click picks one; ? lists the keys, Ctrl+P finds a command, q
//! quits. Inside a tab:
//!
//! - **Explorer:** a row of blocks drawn once for each `Justify` mode, with
//!   the spacers between them (from `split_with_spacers`), each block with
//!   its size and the width it got. ←/→ picks a block; 1 to 6 set its kind
//!   (fixed, percent, ratio, flex, min, max); ↑/↓ its value; a adds a
//!   block and x removes one; + and - change the gap.
//! - **Sizes:** each kind of size in a few rows of real nodes, as
//!   ratatui's `constraints` example has; ←/→ picks the kind.
//! - **Flex:** each `Justify` mode, with and without a gap, as ratatui's
//!   `flex` example has, in a scroll: ↑/↓ and the wheel scroll it.
//! - **Beyond:** what ratatui has no counterpart for: `align_items` across
//!   the row, and a row that wraps. The `align` example goes further.
//!
//! What intuiTUIve adds to the originals: the keys have descriptions, so
//! `?` lists them and Ctrl+P finds them; the mouse picks the tabs and
//! scrolls the Flex tab; and a key draws only the explorer, not the rest
//! (`tests/layout_explorer.rs` checks it).

use intuituive::layout::{split_with_spacers, Track};
use intuituive::node::Axis;
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::widget::{Canvas, DrawCx};
use intuituive::widgets::tabs;
use intuituive::{AlignContent, StackLayout};

/// The tabs, in order.
pub const TABS: [&str; 4] = ["Explorer", "Sizes", "Flex", "Beyond"];

/// How the explorer runs: its command line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// The tab it opens on.
    pub tab: usize,
    /// Run in linear mode.
    pub linear: bool,
}

impl Options {
    /// `--tab N` (1 to 4) and `--linear`.
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
                    let n: usize = inline
                        .or_else(|| args.next())
                        .and_then(|v| v.parse().ok())
                        .filter(|n| (1..=TABS.len()).contains(n))
                        .ok_or("--tab takes a number from 1 to 4")?;
                    options.tab = n - 1;
                }
                "--linear" => options.linear = true,
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(options)
    }
}

/// The kinds of size a block can have: ratatui's constraints, as
/// intuiTUIve writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// `Size::Fixed`: ratatui's `Length`.
    Fixed,
    /// `Size::Percent`: ratatui's `Percentage`.
    Percent,
    /// `Size::Ratio(n, 4)`: ratatui's `Ratio`.
    Ratio,
    /// `Size::Flex`: ratatui's `Fill`.
    Flex,
    /// A flexible block of at least this many cells: ratatui's `Min`.
    Min,
    /// A flexible block of at most this many cells: ratatui's `Max`.
    Max,
}

/// The kinds, in the order 1 to 6 pick them.
pub const KINDS: [Kind; 6] = [
    Kind::Fixed,
    Kind::Percent,
    Kind::Ratio,
    Kind::Flex,
    Kind::Min,
    Kind::Max,
];

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Fixed => "Fixed",
            Kind::Percent => "Percent",
            Kind::Ratio => "Ratio",
            Kind::Flex => "Flex",
            Kind::Min => "Min",
            Kind::Max => "Max",
        }
    }

    /// The value a block takes when it is given this kind.
    fn first(self) -> u16 {
        match self {
            Kind::Fixed => 12,
            Kind::Percent => 20,
            Kind::Ratio => 1,
            Kind::Flex => 1,
            Kind::Min | Kind::Max => 10,
        }
    }

    /// How far ↑/↓ move the value, and its range.
    fn step(self) -> (u16, u16, u16) {
        match self {
            Kind::Fixed | Kind::Min | Kind::Max => (2, 0, 200),
            Kind::Percent => (5, 0, 100),
            Kind::Ratio => (1, 0, 4),
            Kind::Flex => (1, 1, 9),
        }
    }
}

/// One block of the explorer's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Block {
    pub kind: Kind,
    pub value: u16,
}

impl Block {
    pub const fn new(kind: Kind, value: u16) -> Block {
        Block { kind, value }
    }

    /// The track it lays out as.
    pub fn track(self) -> Track {
        let v = self.value;
        match self.kind {
            Kind::Fixed => Track::new(Size::Fixed(v)),
            Kind::Percent => Track::new(Size::Percent(v)),
            Kind::Ratio => Track::new(Size::Ratio(u32::from(v), 4)),
            Kind::Flex => Track::new(Size::Flex(v.max(1))),
            Kind::Min => Track::new(Size::Flex(1)).min(v),
            Kind::Max => Track::new(Size::Flex(1)).max(v),
        }
    }

    /// How it reads: `Fixed 12`, `Percent 20%`, `Ratio 1/4`.
    pub fn describe(self) -> String {
        match self.kind {
            Kind::Percent => format!("Percent {}%", self.value),
            Kind::Ratio => format!("Ratio {}/4", self.value),
            kind => format!("{} {}", kind.name(), self.value),
        }
    }
}

/// The row the explorer opens with.
pub const BLOCKS: [Block; 4] = [
    Block::new(Kind::Fixed, 16),
    Block::new(Kind::Percent, 20),
    Block::new(Kind::Ratio, 1),
    Block::new(Kind::Max, 12),
];

/// The `Justify` modes, each a row of the explorer.
pub const MODES: [(Justify, &str); 6] = [
    (Justify::Start, "Start"),
    (Justify::Center, "Center"),
    (Justify::End, "End"),
    (Justify::SpaceBetween, "SpaceBetween"),
    (Justify::SpaceAround, "SpaceAround"),
    (Justify::SpaceEvenly, "SpaceEvenly"),
];

/// The blocks' colours, by position.
const COLOURS: [&str; 4] = ["#60a5fa", "#34d399", "#f472b6", "#fbbf24"];

/// The explorer's state, each part a signal of its own.
#[derive(Clone, Copy)]
pub struct Explorer {
    pub blocks: Signal<Vec<Block>>,
    pub picked: Signal<usize>,
    pub gap: Signal<u16>,
}

/// Draws the explorer: the blocks, then each mode's row.
struct Rows(Explorer);

impl Rows {
    /// A block at `area`: its box, its size on the top edge and the width
    /// it got on the bottom one.
    fn block(canvas: &mut Canvas, area: Rect, block: Block, colour: &str, picked: bool) {
        let (x, y, w) = (area.x, area.y, area.width);
        if w == 0 || area.height < 3 {
            return;
        }
        let weight = if picked { "bold " } else { "" };
        let style = rich::Style::parse(&format!("{weight}{colour}")).ok();
        let style = style.as_ref();
        if w == 1 {
            for row in 0..3 {
                canvas.print(x, y + row, "│", style);
            }
            return;
        }
        let inner = (w - 2) as usize;
        let edge = |label: &str, left: &str, right: &str| {
            let label = if label.chars().count() + 2 <= inner {
                format!(" {label} ")
            } else {
                String::new()
            };
            let rest = inner.saturating_sub(label.chars().count());
            format!("{left}{label}{}{right}", "─".repeat(rest))
        };
        canvas.print(x, y, &edge(&block.describe(), "┌", "┐"), style);
        let middle = if picked {
            format!("│{}│", "▒".repeat(inner))
        } else {
            format!("│{}│", " ".repeat(inner))
        };
        canvas.print(x, y + 1, &middle, style);
        canvas.print(x, y + 2, &edge(&format!("{w}"), "└", "┘"), style);
    }
}

impl Widget for Rows {
    fn name(&self) -> &'static str {
        "explorer"
    }

    fn focusable(&self) -> bool {
        true
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let Explorer {
            blocks,
            picked,
            gap,
        } = self.0;
        let blocks = blocks.get();
        let picked = picked.get().min(blocks.len().saturating_sub(1));
        let gap = gap.get();
        let width = canvas.width();
        let bold = rich::Style::parse("bold").ok();
        let dim = rich::Style::parse("bright_black").ok();
        // The blocks, the picked one reversed, and the gap.
        let mut x = 0;
        canvas.print(x, 0, "Blocks: ", bold.as_ref());
        x += 8;
        for (i, block) in blocks.iter().enumerate() {
            let chip = format!(" {} ", block.describe());
            let colour = COLOURS[i % COLOURS.len()];
            let look = if i == picked {
                format!("bold black on {colour}")
            } else {
                colour.to_string()
            };
            canvas.print(x, 0, &chip, rich::Style::parse(&look).ok().as_ref());
            x = x.saturating_add(chip.chars().count() as u16 + 1);
        }
        canvas.print(x, 0, &format!(" gap {gap}"), bold.as_ref());
        canvas.print(
            0,
            1,
            "←/→ pick · 1-6 fixed, percent, ratio, flex, min, max · ↑/↓ size · a add · x remove · +/- gap",
            dim.as_ref(),
        );
        let tracks: Vec<Track> = blocks.iter().map(|b| b.track()).collect();
        for (row, (justify, name)) in MODES.iter().enumerate() {
            let y = 3 + row as u16 * 4;
            if y + 4 > canvas.height() + 1 {
                break;
            }
            canvas.print(0, y, name, bold.as_ref());
            let flow = StackLayout {
                gap,
                justify: *justify,
                ..StackLayout::default()
            };
            let band = Rect::new(0, y + 1, width, 3);
            let (areas, spacers) = split_with_spacers(band, Axis::Horizontal, &tracks, flow);
            for spacer in spacers.iter().filter(|s| s.width > 0) {
                canvas.print(
                    spacer.x,
                    spacer.y + 1,
                    &"·".repeat(spacer.width as usize),
                    dim.as_ref(),
                );
            }
            for (i, (area, block)) in areas.iter().zip(&blocks).enumerate() {
                let colour = COLOURS[i % COLOURS.len()];
                Rows::block(canvas, *area, *block, colour, i == picked);
            }
        }
    }
}

/// Explorer: the row of blocks you edit, in every `Justify` mode.
fn explorer(state: Explorer) -> Node {
    let Explorer {
        blocks,
        picked,
        gap,
    } = state;
    let mut node = widget(Rows(state))
        .name("explorer")
        .label("The blocks laid out in each justify mode")
        .bind("left", "the block before", move |_| {
            picked.update(|p| *p = p.saturating_sub(1))
        })
        .bind("right", "the next block", move |_| {
            let last = blocks.with_untracked(|b| b.len().saturating_sub(1));
            picked.update(|p| *p = (*p + 1).min(last))
        })
        .bind("up", "a bigger block", move |_| change(state, 1))
        .bind("down", "a smaller block", move |_| change(state, -1))
        .bind("a", "add a block", move |_| {
            blocks.update(|b| {
                if b.len() < 8 {
                    b.push(Block::new(Kind::Fixed, 10))
                }
            });
            picked.set(blocks.with_untracked(|b| b.len() - 1));
        })
        .bind("x", "remove the block", move |_| {
            let at = picked.get_untracked();
            blocks.update(|b| {
                if b.len() > 1 {
                    b.remove(at.min(b.len() - 1));
                }
            });
            let last = blocks.with_untracked(|b| b.len() - 1);
            picked.update(|p| *p = (*p).min(last));
        })
        .bind("+ =", "a wider gap", move |_| {
            gap.update(|g| *g = (*g + 1).min(8))
        })
        .bind("-", "a narrower gap", move |_| {
            gap.update(|g| *g = g.saturating_sub(1))
        });
    for (i, kind) in KINDS.iter().enumerate() {
        let kind = *kind;
        node = node.bind(
            &(i + 1).to_string(),
            &format!("make it {}", kind.name().to_lowercase()),
            move |_| set_kind(state, kind),
        );
    }
    node
}

/// Move the picked block's value a step up (`by` 1) or down (-1).
fn change(state: Explorer, by: i32) {
    let at = state.picked.get_untracked();
    state.blocks.update(|blocks| {
        if let Some(block) = blocks.get_mut(at) {
            let (step, low, high) = block.kind.step();
            let value = i32::from(block.value) + by * i32::from(step);
            block.value = value.clamp(i32::from(low), i32::from(high)) as u16;
        }
    });
}

/// Give the picked block `kind`, at that kind's first value.
fn set_kind(state: Explorer, kind: Kind) {
    let at = state.picked.get_untracked();
    state.blocks.update(|blocks| {
        if let Some(block) = blocks.get_mut(at) {
            *block = Block::new(kind, kind.first());
        }
    });
}

/// A block of real nodes, sized by `block`: a panel titled with its size,
/// showing the width it got.
fn sized_block(block: Block, colour: &'static str) -> Node {
    let body = leaf(move |console, width, _height| {
        // The panel's border takes a cell either side.
        let text = format!("{} wide", width + 2);
        let pad = (width as usize).saturating_sub(text.chars().count()) / 2;
        let line = format!("{}{text}", " ".repeat(pad));
        console.render_lines(
            &rich::Text::from_markup(&format!("[{colour}]{line}[/]"))
                .unwrap_or_else(|_| rich::Text::new(line.clone())),
            &console.options().update_width(width.max(1) as usize),
            false,
        )
    });
    let node = body
        .panel(&block.describe())
        .border_style(colour)
        .title_style(colour);
    match block.kind {
        Kind::Fixed => node.fixed(block.value),
        Kind::Percent => node.percent(block.value),
        Kind::Ratio => node.ratio(u32::from(block.value), 4),
        Kind::Flex => node.flex(block.value.max(1)),
        Kind::Min => node.flex(1).min_size(block.value),
        Kind::Max => node.flex(1).max_size(block.value),
    }
}

/// The examples of each kind: a description and a row of blocks.
pub fn size_examples(kind: Kind) -> Vec<(&'static str, Vec<Block>)> {
    use Kind::*;
    let b = Block::new;
    match kind {
        Fixed => vec![
            (
                "Two fixed blocks: the rest is left empty",
                vec![b(Fixed, 20), b(Fixed, 30)],
            ),
            (
                "Fixed beside flex: flex takes the rest",
                vec![b(Fixed, 20), b(Flex, 1)],
            ),
            (
                "More than fits: they shrink from the end",
                vec![b(Fixed, 50), b(Fixed, 40), b(Fixed, 30)],
            ),
        ],
        Percent => vec![
            (
                "A quarter and three quarters",
                vec![b(Percent, 25), b(Percent, 75)],
            ),
            (
                "Half, and flex for the rest",
                vec![b(Percent, 50), b(Flex, 1)],
            ),
            (
                "Three thirds of percent: rounding leaves a cell",
                vec![b(Percent, 33), b(Percent, 33), b(Percent, 33)],
            ),
        ],
        Ratio => vec![
            (
                "A quarter and three quarters",
                vec![b(Ratio, 1), b(Ratio, 3)],
            ),
            (
                "Four quarters fill the row exactly",
                vec![b(Ratio, 1), b(Ratio, 1), b(Ratio, 1), b(Ratio, 1)],
            ),
            ("Half, and flex for the rest", vec![b(Ratio, 2), b(Flex, 1)]),
        ],
        Flex => vec![
            ("Equal shares", vec![b(Flex, 1), b(Flex, 1)]),
            ("One share and two", vec![b(Flex, 1), b(Flex, 2)]),
            (
                "Fixed first, then shares of the rest",
                vec![b(Fixed, 20), b(Flex, 1), b(Flex, 3)],
            ),
        ],
        Min => vec![
            (
                "At least 30, growing as flex does",
                vec![b(Min, 30), b(Fixed, 20)],
            ),
            (
                "Two minimums share what is left",
                vec![b(Min, 10), b(Min, 40)],
            ),
            (
                "A minimum holds while there is room",
                vec![b(Min, 40), b(Fixed, 70)],
            ),
        ],
        Max => vec![
            ("At most 20, the rest flex", vec![b(Max, 20), b(Flex, 1)]),
            ("Two maximums leave room over", vec![b(Max, 20), b(Max, 30)]),
            (
                "A maximum beside a fixed block",
                vec![b(Max, 10), b(Fixed, 30)],
            ),
        ],
    }
}

/// Sizes: each kind of size in rows of real nodes.
fn sizes() -> Node {
    let kind = signal(0usize);
    let strip = tabs(
        || KINDS.iter().map(|k| k.name().to_string()).collect(),
        kind,
    )
    .theme_styles([
        ("tabs.selected", "bold black on #34d399"),
        ("tabs.selected.blur", "bold black on #34d399"),
    ])
    .no_focus()
    .name("kinds")
    .fixed(1);
    let examples = switch(
        move || kind.get(),
        |k| {
            let rows = size_examples(KINDS[k]).into_iter().map(|(about, blocks)| {
                let blocks = blocks
                    .into_iter()
                    .enumerate()
                    .map(|(i, block)| sized_block(block, COLOURS[i % COLOURS.len()]));
                column([
                    label(format!("[bold]{about}[/]")).fixed(1),
                    row(blocks).fixed(3),
                ])
                .fixed(5)
            });
            column(rows.collect::<Vec<_>>())
        },
    )
    .name("sizes")
    .label("Rows of blocks of one kind of size");
    column([
        strip,
        label("[dim]←/→ the kind · each block shows the width it got[/]").fixed(1),
        label("").fixed(1),
        examples.flex(1),
    ])
    .focusable()
    .bind("right", "the next kind", move |_| {
        kind.update(|k| *k = (*k + 1) % KINDS.len())
    })
    .bind("left", "the kind before", move |_| {
        kind.update(|k| *k = (*k + KINDS.len() - 1) % KINDS.len())
    })
    .name("sizes-page")
}

/// The blocks of each Flex case.
pub const FLEX_BLOCKS: [Block; 3] = [
    Block::new(Kind::Fixed, 20),
    Block::new(Kind::Percent, 20),
    Block::new(Kind::Fixed, 10),
];

/// Flex: each mode, with no gap and a gap of 2, in a scroll.
fn flex() -> Node {
    let mut cases = Vec::new();
    for gap in [0u16, 2] {
        for (justify, name) in MODES {
            let blocks = FLEX_BLOCKS
                .iter()
                .enumerate()
                .map(|(i, block)| sized_block(*block, COLOURS[i % COLOURS.len()]));
            cases.push(
                column([
                    label(format!("[bold]{name}[/], gap {gap}")).fixed(1),
                    row(blocks).justify(justify).gap(gap).fixed(3),
                ])
                .fixed(4),
            );
        }
    }
    scroll(column(cases))
        .name("flex")
        .label("Each justify mode, with and without a gap")
}

/// Beyond: `align_items` across a row, and a row that wraps.
fn beyond() -> Node {
    let card = |name: &str, lines: usize, width: u16| {
        label(vec!["·"; lines].join("\n")).panel(name).fixed(width)
    };
    let aligned = |align: Align, name: &str| {
        column([
            label(format!("[bold]align_items({name})[/]")).fixed(1),
            row([card("a", 1, 7), card("b", 3, 7), card("c", 2, 7)])
                .gap(1)
                .align_items(align)
                .fixed(5),
        ])
        .flex(1)
    };
    let wrapping = row((1..=9).map(|n| card(&format!("card {n}"), 1, 14 + (n % 3) * 4)))
        .gap(1)
        .wrap()
        .align_content(AlignContent::Start)
        .name("wrapping");
    column([
        label(
            "[dim]What ratatui's layout has no counterpart for. The `align` example \
             changes each of these with a key.[/]",
        )
        .fixed(1),
        row([
            aligned(Align::Start, "Start"),
            aligned(Align::Center, "Center"),
            aligned(Align::End, "End"),
            aligned(Align::Stretch, "Stretch"),
        ])
        .gap(2)
        .fixed(6),
        label("[bold]wrap()[/]: cards that do not fit go on the next line").fixed(1),
        wrapping.flex(1),
    ])
    .name("beyond")
}

/// The explorer, as `options` says.
pub fn layout_app(options: Options) -> App {
    App::new(move || {
        let tab = signal(options.tab.min(TABS.len() - 1));
        let state = Explorer {
            blocks: signal(BLOCKS.to_vec()),
            picked: signal(0),
            gap: signal(1),
        };
        let strip = tabs(|| TABS.iter().map(|t| t.to_string()).collect(), tab)
            .theme_styles([
                ("tabs.selected", "bold black on #60a5fa"),
                ("tabs.selected.blur", "bold black on #60a5fa"),
            ])
            .no_focus()
            .name("tabs")
            .label("Layout tabs")
            .fixed(1);
        let body = switch(
            move || tab.get(),
            move |page| match page {
                0 => explorer(state),
                1 => sizes(),
                2 => flex(),
                _ => beyond(),
            },
        )
        .name("page");
        let footer = label("[dim]Tab/Shift+Tab tabs · ? keys · Ctrl+P commands · q quit[/]")
            .no_wrap()
            .name("footer")
            .fixed(1);
        column([strip, body.flex(1), footer])
            .bind("tab", "the next tab", move |_| {
                tab.update(|t| *t = (*t + 1) % TABS.len())
            })
            .bind("shift+tab", "the tab before", move |_| {
                tab.update(|t| *t = (*t + TABS.len() - 1) % TABS.len())
            })
            .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
    .linear(options.linear)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    layout_app(options).run()
}
