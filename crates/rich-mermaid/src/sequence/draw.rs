//! Draw a [`Sequence`] as text.
//!
//! Rows run down the page in statement order: the title, the participants'
//! heads, then each message (its text above its arrow), note and block
//! border on rows of its own. Lifelines run down from each head to the end.
//!
//! Columns are sized to what sits between them: each lifeline is placed as
//! far left as every constraint on it allows (head boxes apart, a message's
//! text between its two lifelines, a note or a self-message beside its
//! lifeline, block frames clear of the lifelines outside them). The drawing
//! takes no width; [`Drawing::cropped`] fits it to one.

use rich::cells::{cell_len, char_cell_width};
use rich_diagram::layout::MAX_CELLS;
use rich_diagram::Drawing;

use super::{BlockKind, Event, LineStyle, NotePlacement, ParticipantKind, Sequence, Tip};

const N: u8 = 1;
const E: u8 = 2;
const S: u8 = 4;
const W: u8 = 8;

/// Columns between two neighbouring heads.
const HEAD_GAP: i64 = 3;

/// Lay out and draw `diagram`. `ascii` restricts the output to ASCII.
///
/// Fails when the drawing would cover more than
/// [`MAX_CELLS`](rich_diagram::layout::MAX_CELLS) cells. A diagram with no
/// participants draws nothing.
pub fn draw(diagram: &Sequence, ascii: bool) -> Result<Drawing, String> {
    let n = diagram.participants.len();
    if n == 0 {
        return Ok(Drawing::default());
    }
    let plan = Plan::new(diagram);
    let x = plan.columns();
    let title = diagram.title.as_deref().map(clean);
    let title_width = title.as_deref().map_or(0, |t| cell_len(t) as i64);
    let width = x[n].max(title_width);
    let height = plan.height as i64;
    if width.saturating_mul(height) > MAX_CELLS {
        return Err(format!(
            "it would take {width} {} {height} cells to draw",
            if ascii { "x" } else { "×" }
        ));
    }
    let mut canvas = Canvas::new(width, height, ascii);
    plan.render(diagram, &x, &mut canvas);
    if let Some(title) = &title {
        canvas.text((width - title_width) / 2, 0, title);
    }
    let lines = canvas.lines();
    let width = lines.iter().map(|l| cell_len(l)).max().unwrap_or(0);
    Ok(Drawing {
        lines,
        width,
        notes: Vec::new(),
    })
}

/// Text with control characters removed (tabs become spaces), so it cannot
/// reach the terminal as an escape sequence.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\t' { ' ' } else { c })
        .filter(|&c| c == '\n' || !c.is_control())
        .collect()
}

fn text_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    clean(text).split('\n').map(str::to_string).collect()
}

fn widest(lines: &[String]) -> i64 {
    lines.iter().map(|l| cell_len(l) as i64).max().unwrap_or(0)
}

/// A participant and an offset from its lifeline.
type Anchor = (usize, i64);

/// Something drawn between or beside lifelines, which frames must hold and
/// other lifelines must clear.
struct Item {
    left: Anchor,
    right: Anchor,
    /// How many blocks it is in.
    depth: usize,
}

/// A block's frame.
struct Frame {
    kind: BlockKind,
    label: String,
    depth: usize,
    top: usize,
    bottom: usize,
    dividers: Vec<(usize, String)>,
    /// Its items, by index into [`Plan::items`].
    items: std::ops::Range<usize>,
}

impl Frame {
    /// The text in the top border.
    fn heading(&self) -> String {
        match (self.kind, self.label.is_empty()) {
            (BlockKind::Rect, _) => String::new(),
            (kind, true) => kind.keyword().to_string(),
            (kind, false) => format!("{} [{}]", kind.keyword(), clean(&self.label)),
        }
    }

    /// The widest text in its borders.
    fn text_width(&self) -> i64 {
        let dividers = self
            .dividers
            .iter()
            .map(|(_, label)| divider_text(label))
            .map(|t| cell_len(&t) as i64);
        dividers
            .chain([cell_len(&self.heading()) as i64])
            .max()
            .unwrap_or(0)
    }
}

fn divider_text(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!("[{}]", clean(label))
    }
}

/// What is drawn on which rows.
enum Op {
    /// A message between two participants: its text rows, then its arrow.
    Message {
        event: usize,
        text: Vec<String>,
        row: usize,
    },
    /// A message to its sender: its text rows, then two rows of loop.
    SelfMessage {
        event: usize,
        text: Vec<String>,
        row: usize,
    },
    /// A note's box, its first row and its item.
    Note {
        text: Vec<String>,
        row: usize,
        item: usize,
    },
}

/// The rows of a diagram and the constraints on its columns.
struct Plan {
    /// Head widths and heights.
    heads: Vec<(i64, i64)>,
    /// The first row of the heads.
    head_row: usize,
    ops: Vec<Op>,
    items: Vec<Item>,
    frames: Vec<Frame>,
    /// Active rows per participant, inclusive.
    active: Vec<Vec<(usize, usize)>>,
    height: usize,
    /// `(a, b, d)`: lifeline `b` at least `d` right of lifeline `a`
    /// (`None`: of column 0). `b == n` is the drawing's width.
    constraints: Vec<(Option<usize>, usize, i64)>,
}

impl Plan {
    fn new(diagram: &Sequence) -> Self {
        let n = diagram.participants.len();
        let heads: Vec<(i64, i64)> = diagram
            .participants
            .iter()
            .map(|p| {
                let lines = text_lines(&p.label);
                let (w, h) = (widest(&lines).max(1), lines.len().max(1) as i64);
                match p.kind {
                    ParticipantKind::Participant => (w + 4, h + 2),
                    ParticipantKind::Actor => (w.max(3), h + 3),
                }
            })
            .collect();
        let head_row = if diagram.title.is_some() { 2 } else { 0 };
        let tallest = heads.iter().map(|h| h.1).max().unwrap_or(0) as usize;
        let mut plan = Plan {
            heads,
            head_row,
            ops: Vec::new(),
            items: Vec::new(),
            frames: Vec::new(),
            active: vec![Vec::new(); n],
            height: 0,
            constraints: Vec::new(),
        };
        let mut y = head_row + tallest + 1;
        let mut open: Vec<usize> = Vec::new();
        let mut starts: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut last_arrow: Option<usize> = None;
        for (index, event) in diagram.events.iter().enumerate() {
            let depth = open.len();
            match event {
                Event::Message(message) => {
                    let mut text = text_lines(&message.text);
                    if let Some(number) = message.number {
                        match text.first_mut() {
                            Some(first) => *first = format!("{number}. {first}"),
                            None => text.push(format!("{number}.")),
                        }
                    }
                    let width = widest(&text);
                    let row = y;
                    y += text.len();
                    if message.from == message.to {
                        let p = message.from;
                        plan.item((p, 0), (p, (width + 1).max(3)), depth);
                        plan.ops.push(Op::SelfMessage {
                            event: index,
                            text,
                            row,
                        });
                        y += 1;
                    } else {
                        let (lo, hi) = (message.from.min(message.to), message.from.max(message.to));
                        plan.constraints.push((Some(lo), hi, width + 4));
                        plan.item((lo, 0), (hi, 0), depth);
                        plan.ops.push(Op::Message {
                            event: index,
                            text,
                            row,
                        });
                    }
                    last_arrow = Some(y);
                    y += 1;
                }
                Event::Note { placement, text } => {
                    let text = text_lines(text);
                    let w = widest(&text) + 4;
                    let (left, right) = match *placement {
                        NotePlacement::RightOf(p) => ((p, 2), (p, w + 1)),
                        NotePlacement::LeftOf(p) => ((p, -1 - w), (p, -2)),
                        NotePlacement::Over(p, q) if p == q => {
                            let w = w.max(5);
                            ((p, -(w / 2)), (p, w - w / 2 - 1))
                        }
                        NotePlacement::Over(p, q) => {
                            plan.constraints.push((Some(p), q, w - 5));
                            ((p, -2), (q, 2))
                        }
                    };
                    let item = plan.item(left, right, depth);
                    let rows = text.len().max(1) + 2;
                    plan.ops.push(Op::Note { text, row: y, item });
                    y += rows;
                    last_arrow = None;
                }
                Event::Activate(p) => starts[*p].push(last_arrow.unwrap_or(y)),
                Event::Deactivate(p) => {
                    if let Some(start) = starts[*p].pop() {
                        let end = last_arrow.unwrap_or(y.saturating_sub(1)).max(start);
                        plan.active[*p].push((start, end));
                    }
                }
                Event::Start { kind, label } => {
                    open.push(plan.frames.len());
                    plan.frames.push(Frame {
                        kind: *kind,
                        label: label.clone(),
                        depth: depth + 1,
                        top: y,
                        bottom: y,
                        dividers: Vec::new(),
                        items: plan.items.len()..plan.items.len(),
                    });
                    y += 1;
                    last_arrow = None;
                }
                Event::Divider { label } => {
                    if let Some(&frame) = open.last() {
                        plan.frames[frame].dividers.push((y, label.clone()));
                        y += 1;
                    }
                    last_arrow = None;
                }
                Event::End => {
                    if let Some(frame) = open.pop() {
                        let start = plan.frames[frame].items.start;
                        if start == plan.items.len() {
                            // An empty block still frames something.
                            plan.item((0, 0), (0, 0), depth);
                        }
                        plan.frames[frame].bottom = y;
                        plan.frames[frame].items = start..plan.items.len();
                        y += 1;
                    }
                    last_arrow = None;
                }
            }
        }
        plan.height = y + 1;
        for (p, starts) in starts.into_iter().enumerate() {
            for start in starts {
                plan.active[p].push((start, y));
            }
        }
        plan.head_constraints();
        plan
    }

    /// Record an item, and keep the lifelines beside it, and the frames
    /// around it, clear of it.
    fn item(&mut self, left: Anchor, right: Anchor, depth: usize) -> usize {
        let n = self.heads.len();
        let pad = 2 * depth as i64;
        let (lo, offl) = left;
        let (hi, offr) = right;
        self.constraints.push(match lo {
            0 => (None, lo, -offl + pad),
            _ => (Some(lo - 1), lo, -offl + pad + 2),
        });
        self.constraints.push(if hi + 1 < n {
            (Some(hi), hi + 1, offr + pad + 2)
        } else {
            (Some(hi), n, offr + pad + 1)
        });
        self.items.push(Item { left, right, depth });
        self.items.len() - 1
    }

    /// Heads side by side, apart, and inside the drawing.
    fn head_constraints(&mut self) {
        let n = self.heads.len();
        let (first, _) = self.heads[0];
        self.constraints.push((None, 0, first / 2));
        for i in 0..n - 1 {
            let (a, b) = (self.heads[i].0, self.heads[i + 1].0);
            self.constraints
                .push((Some(i), i + 1, (a - a / 2) + HEAD_GAP + b / 2));
        }
        let (last, _) = self.heads[n - 1];
        self.constraints.push((Some(n - 1), n, last - last / 2));
    }

    /// Each lifeline's column, then the drawing's width: as far left as the
    /// constraints allow. Frames whose text does not fit widen to the right,
    /// pushing the lifelines they would reach.
    fn columns(&self) -> Vec<i64> {
        let n = self.heads.len();
        let mut constraints = self.constraints.clone();
        let mut x = solve(n, &constraints);
        // Each round adds a constraint for each frame that did not fit, which
        // then fits; pushing lifelines right can unsettle another, so a few
        // rounds, bounded.
        for _ in 0..8 {
            let mut added = false;
            for frame in &self.frames {
                let ((lo, _), left, right) = self.frame_extent(frame, &x);
                let need = left + frame.text_width() + 5;
                if frame.text_width() == 0 || right >= need {
                    continue;
                }
                let reach = left - x[lo];
                match (0..n).find(|&m| x[m] > right) {
                    Some(m) if x[m] <= need + 1 => {
                        constraints.push((Some(lo), m, reach + frame.text_width() + 7));
                        added = true;
                    }
                    Some(_) => {}
                    None if x[n] <= need => {
                        constraints.push((Some(lo), n, reach + frame.text_width() + 6));
                        added = true;
                    }
                    None => {}
                }
            }
            if !added {
                break;
            }
            x = solve(n, &constraints);
        }
        x
    }

    /// A frame's leftmost item anchor, and its left and right columns
    /// before any widening for its text.
    fn frame_extent(&self, frame: &Frame, x: &[i64]) -> (Anchor, i64, i64) {
        let mut left = (i64::MAX, (0, 0));
        let mut right = i64::MIN;
        for item in &self.items[frame.items.clone()] {
            let pad = 2 * (item.depth as i64 - frame.depth as i64 + 1);
            let l = x[item.left.0] + item.left.1 - pad;
            if l < left.0 {
                left = (l, (item.left.0, item.left.1 - pad));
            }
            right = right.max(x[item.right.0] + item.right.1 + pad);
        }
        (left.1, left.0, right)
    }

    fn render(&self, diagram: &Sequence, x: &[i64], canvas: &mut Canvas) {
        let n = diagram.participants.len();
        let top = self.head_row as i64;
        let last = self.height as i64 - 1;
        // Lifelines, heavy where active.
        for ((&column, &(_, h)), active) in x.iter().zip(&self.heads).zip(&self.active) {
            for y in top + h..=last {
                canvas.bits(column, y, N | S, false);
            }
            for &(start, end) in active {
                for y in start..=end {
                    canvas.heavy(column, y as i64);
                }
            }
        }
        // Heads.
        for (p, participant) in diagram.participants.iter().enumerate() {
            let (w, h) = self.heads[p];
            let lines = text_lines(&participant.label);
            match participant.kind {
                ParticipantKind::Participant => {
                    let left = x[p] - w / 2;
                    canvas.rect(left, top, left + w - 1, top + h - 1);
                    canvas.bits(x[p], top + h - 1, S, false);
                    for (i, line) in lines.iter().enumerate() {
                        let at = left + (w - cell_len(line) as i64) / 2;
                        canvas.text(at, top + 1 + i as i64, line);
                    }
                }
                ParticipantKind::Actor => {
                    let figure: [&str; 3] = if canvas.ascii {
                        [" o ", "/|\\", "/ \\"]
                    } else {
                        [" ○ ", "─┼─", "╱ ╲"]
                    };
                    for (i, row) in figure.iter().enumerate() {
                        canvas.text(x[p] - 1, top + i as i64, row);
                    }
                    for (i, line) in lines.iter().enumerate() {
                        let at = x[p] - cell_len(line) as i64 / 2;
                        canvas.text(at, top + 3 + i as i64, line);
                    }
                }
            }
        }
        // Frames: borders, then texts.
        let mut texts: Vec<(i64, i64, String)> = Vec::new();
        for frame in &self.frames {
            let (_, left, right) = self.frame_extent(frame, x);
            let right = right.max(match frame.text_width() {
                0 => right,
                width => left + width + 5,
            });
            let (y0, y1) = (frame.top as i64, frame.bottom as i64);
            canvas.rect(left, y0, right, y1);
            for (row, label) in &frame.dividers {
                let y = *row as i64;
                canvas.bits(left, y, E, false);
                canvas.bits(right, y, W, false);
                for x in left + 1..right {
                    canvas.bits(x, y, E | W, true);
                }
                let text = divider_text(label);
                if !text.is_empty() {
                    texts.push((left + 2, y, format!(" {text} ")));
                }
            }
            let heading = frame.heading();
            if !heading.is_empty() {
                texts.push((left + 2, y0, format!(" {heading} ")));
            }
        }
        // Messages.
        for op in &self.ops {
            match op {
                Op::Message { event, text, row } => {
                    let Event::Message(message) = &diagram.events[*event] else {
                        continue;
                    };
                    let (from, to) = (x[message.from], x[message.to]);
                    let (lo, hi) = (from.min(to), from.max(to));
                    for (i, line) in text.iter().enumerate() {
                        let at = lo + 1 + (hi - lo - 1 - cell_len(line) as i64) / 2;
                        texts.push((at, (*row + i) as i64, line.clone()));
                    }
                    let y = (*row + text.len()) as i64;
                    let dotted = message.line == LineStyle::Dotted;
                    let dir = (to - from).signum();
                    let (out, back) = if dir > 0 { (E, W) } else { (W, E) };
                    for x in lo + 1..hi {
                        canvas.bits(x, y, E | W, dotted);
                    }
                    match tip(message.start, -dir, canvas.ascii) {
                        Some(glyph) => canvas.glyph(from + dir, y, glyph),
                        None => canvas.bits(from, y, out, false),
                    }
                    match tip(message.end, dir, canvas.ascii) {
                        Some(glyph) => canvas.glyph(to - dir, y, glyph),
                        None => canvas.bits(to, y, back, false),
                    }
                }
                Op::SelfMessage { event, text, row } => {
                    let Event::Message(message) = &diagram.events[*event] else {
                        continue;
                    };
                    let p = x[message.from];
                    for (i, line) in text.iter().enumerate() {
                        texts.push((p + 2, (*row + i) as i64, line.clone()));
                    }
                    let y = (*row + text.len()) as i64;
                    let dotted = message.line == LineStyle::Dotted;
                    canvas.bits(p, y, E, false);
                    canvas.bits(p + 1, y, E | W, dotted);
                    canvas.bits(p + 2, y, E | W, dotted);
                    canvas.bits(p + 3, y, W | S, false);
                    canvas.bits(p + 3, y + 1, N | W, false);
                    canvas.bits(p + 2, y + 1, E | W, dotted);
                    match tip(message.end, -1, canvas.ascii) {
                        Some(glyph) => canvas.glyph(p + 1, y + 1, glyph),
                        None => {
                            canvas.bits(p + 1, y + 1, E | W, dotted);
                            canvas.bits(p, y + 1, E, false);
                        }
                    }
                }
                Op::Note { .. } => {}
            }
        }
        // Notes cover the lifelines behind them.
        for op in &self.ops {
            let Op::Note { text, row, item } = op else {
                continue;
            };
            let item = &self.items[*item];
            let left = x[item.left.0] + item.left.1;
            let right = x[item.right.0] + item.right.1;
            let (y0, y1) = (*row as i64, (*row + text.len().max(1) + 1) as i64);
            for y in y0..=y1 {
                for x in left..=right {
                    canvas.clear(x, y);
                }
            }
            canvas.rect(left, y0, right, y1);
            for &lifeline in &x[..n] {
                if lifeline > left && lifeline < right {
                    canvas.bits(lifeline, y0, N, false);
                    canvas.bits(lifeline, y1, S, false);
                }
            }
            for (i, line) in text.iter().enumerate() {
                let at = left + (right - left + 1 - cell_len(line) as i64) / 2;
                canvas.text(at, y0 + 1 + i as i64, line);
            }
        }
        for (at, y, text) in texts {
            canvas.text(at, y, &text);
        }
    }
}

/// Lifeline columns, then the width, as far left as `constraints` allow.
fn solve(n: usize, constraints: &[(Option<usize>, usize, i64)]) -> Vec<i64> {
    let mut by_target: Vec<Vec<(Option<usize>, i64)>> = vec![Vec::new(); n + 1];
    for &(a, b, d) in constraints {
        by_target[b].push((a, d));
    }
    let mut x = vec![0i64; n + 1];
    for b in 0..=n {
        x[b] = by_target[b]
            .iter()
            .map(|&(a, d)| a.map_or(0, |a| x[a]) + d)
            .max()
            .unwrap_or(0)
            .max(0);
    }
    x
}

/// The glyph for a tip pointing `dir` (1 right, -1 left).
fn tip(tip: Tip, dir: i64, ascii: bool) -> Option<&'static str> {
    let right = dir > 0;
    Some(match (tip, ascii) {
        (Tip::None, _) => return None,
        (Tip::Arrow, false) => {
            if right {
                "►"
            } else {
                "◄"
            }
        }
        (Tip::Arrow, true) => {
            if right {
                ">"
            } else {
                "<"
            }
        }
        (Tip::Open, false) => {
            if right {
                "▷"
            } else {
                "◁"
            }
        }
        (Tip::Open, true) => {
            if right {
                ")"
            } else {
                "("
            }
        }
        (Tip::Cross, false) => "×",
        (Tip::Cross, true) => "x",
    })
}

#[derive(Clone, Debug)]
enum Cell {
    Empty,
    /// Line directions, whether horizontal runs are dotted, and whether the
    /// vertical run is an activation bar.
    Line {
        bits: u8,
        dotted: bool,
        heavy: bool,
    },
    Glyph(String),
    /// The second cell of a wide character.
    Wide,
}

struct Canvas {
    cells: Vec<Vec<Cell>>,
    ascii: bool,
}

impl Canvas {
    fn new(width: i64, height: i64, ascii: bool) -> Self {
        Canvas {
            cells: vec![vec![Cell::Empty; width.max(0) as usize]; height.max(0) as usize],
            ascii,
        }
    }

    fn get(&mut self, x: i64, y: i64) -> Option<&mut Cell> {
        if x < 0 || y < 0 {
            return None;
        }
        self.cells.get_mut(y as usize)?.get_mut(x as usize)
    }

    fn bits(&mut self, x: i64, y: i64, add: u8, dotted_run: bool) {
        if let Some(cell) = self.get(x, y) {
            match cell {
                Cell::Line { bits, dotted, .. } => {
                    *bits |= add;
                    *dotted |= dotted_run;
                }
                Cell::Empty => {
                    *cell = Cell::Line {
                        bits: add,
                        dotted: dotted_run,
                        heavy: false,
                    }
                }
                Cell::Glyph(_) | Cell::Wide => {}
            }
        }
    }

    fn heavy(&mut self, x: i64, y: i64) {
        if let Some(Cell::Line { heavy, .. }) = self.get(x, y) {
            *heavy = true;
        }
    }

    fn clear(&mut self, x: i64, y: i64) {
        if let Some(cell) = self.get(x, y) {
            *cell = Cell::Empty;
        }
    }

    fn glyph(&mut self, x: i64, y: i64, glyph: &str) {
        if let Some(cell) = self.get(x, y) {
            *cell = Cell::Glyph(glyph.to_string());
        }
    }

    /// A box outline from (x0, y0) to (x1, y1), joining what it crosses.
    fn rect(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        for x in x0 + 1..x1 {
            self.bits(x, y0, E | W, false);
            self.bits(x, y1, E | W, false);
        }
        for y in y0 + 1..y1 {
            self.bits(x0, y, N | S, false);
            self.bits(x1, y, N | S, false);
        }
        self.bits(x0, y0, E | S, false);
        self.bits(x1, y0, W | S, false);
        self.bits(x0, y1, N | E, false);
        self.bits(x1, y1, N | W, false);
    }

    /// Write text left to right from (x, y), over whatever was there.
    fn text(&mut self, x: i64, y: i64, text: &str) {
        let mut at = x;
        for c in text.chars() {
            match char_cell_width(c) {
                0 => {
                    if let Some(Cell::Glyph(previous)) = self.get(at - 1, y) {
                        previous.push(c);
                    }
                }
                width => {
                    self.glyph(at, y, &c.to_string());
                    if width == 2 {
                        if let Some(cell) = self.get(at + 1, y) {
                            *cell = Cell::Wide;
                        }
                    }
                    at += width as i64;
                }
            }
        }
    }

    fn line_char(&self, bits: u8, dotted: bool, heavy: bool) -> char {
        let vertical = bits & (E | W) == 0;
        let horizontal = bits & (N | S) == 0;
        let through = bits & (N | S) == N | S;
        if self.ascii {
            return match (vertical, horizontal) {
                _ if heavy && through => '#',
                (true, _) => '|',
                (_, true) if dotted => '.',
                (_, true) => '-',
                _ => '+',
            };
        }
        if vertical {
            return if heavy { '┃' } else { '│' };
        }
        if horizontal {
            return if dotted { '┄' } else { '─' };
        }
        if heavy && through {
            return match bits & (E | W) {
                E => '┠',
                W => '┨',
                _ => '╂',
            };
        }
        match bits {
            b if b == E | S => '┌',
            b if b == W | S => '┐',
            b if b == N | E => '└',
            b if b == N | W => '┘',
            b if b == N | S | E => '├',
            b if b == N | S | W => '┤',
            b if b == E | W | S => '┬',
            b if b == E | W | N => '┴',
            _ => '┼',
        }
    }

    fn lines(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|row| {
                let mut line = String::new();
                for cell in row {
                    match cell {
                        Cell::Empty => line.push(' '),
                        Cell::Line {
                            bits,
                            dotted,
                            heavy,
                        } => line.push(self.line_char(*bits, *dotted, *heavy)),
                        Cell::Glyph(text) => line.push_str(text),
                        Cell::Wide => {}
                    }
                }
                line.trim_end().to_string()
            })
            .collect()
    }
}
