//! Nested layout composition using bounded cell allocations.
use super::{allocate, fit_segments, Constraint, ConstraintError, OverflowPolicy};
use rich::{Console, ConsoleOptions, Renderable, Segment};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Start,
    Center,
    End,
}
enum Content {
    Leaf(Box<dyn Renderable>),
    Split(Axis, Vec<LayoutNode>),
}
pub struct LayoutNode {
    content: Content,
    width: Constraint,
    height: Constraint,
    horizontal: Alignment,
    vertical: Alignment,
    overflow: OverflowPolicy,
    content_width: bool,
    content_height: bool,
}
impl LayoutNode {
    fn new(content: Content) -> Self {
        Self {
            content,
            width: Constraint::default(),
            height: Constraint::default(),
            horizontal: Alignment::Start,
            vertical: Alignment::Start,
            overflow: OverflowPolicy::Fold,
            content_width: false,
            content_height: false,
        }
    }
    pub fn leaf(value: Box<dyn Renderable>) -> Self {
        Self::new(Content::Leaf(value))
    }
    pub fn split(axis: Axis, children: Vec<Self>) -> Self {
        Self::new(Content::Split(axis, children))
    }
    pub fn width(mut self, value: Constraint) -> Self {
        self.width = value;
        self.content_width = false;
        self
    }
    pub fn height(mut self, value: Constraint) -> Self {
        self.height = value;
        self.content_height = false;
        self
    }
    pub fn content_width(mut self) -> Self {
        self.content_width = true;
        self
    }
    pub fn content_height(mut self) -> Self {
        self.content_height = true;
        self
    }
    pub fn align(mut self, horizontal: Alignment, vertical: Alignment) -> Self {
        self.horizontal = horizontal;
        self.vertical = vertical;
        self
    }
    pub fn overflow(mut self, value: OverflowPolicy) -> Self {
        self.overflow = value;
        self
    }
    pub fn validate(&self) -> Result<(), ConstraintError> {
        self.width.validate()?;
        self.height.validate()?;
        if let Content::Split(_, children) = &self.content {
            for child in children {
                child.validate()?;
            }
        }
        Ok(())
    }
    fn constraint(&self, axis: Axis, console: &Console, options: &ConsoleOptions) -> Constraint {
        let mut c = if axis == Axis::Horizontal {
            self.width
        } else {
            self.height
        };
        let content = if axis == Axis::Horizontal {
            self.content_width
        } else {
            self.content_height
        };
        if content {
            let preferred = self.intrinsic(axis, console, options);
            c.preferred = Some(preferred.max(c.min).min(c.max.unwrap_or(usize::MAX)));
            c.flex = 0;
        }
        c
    }
    // Measure natural content independently of opting into content sizing.
    // Height always follows the width allocation, including nested row splits.
    fn intrinsic(&self, axis: Axis, console: &Console, options: &ConsoleOptions) -> usize {
        let mut opts = options.clone();
        if axis == Axis::Vertical {
            let width = self.constraint(Axis::Horizontal, console, options);
            opts =
                opts.update_width(allocate(options.max_width, &[width]).map_or(0, |a| a.sizes[0]));
        }
        let natural = match (&self.content, axis) {
            (Content::Leaf(value), Axis::Horizontal) => value.measure(console, &opts).maximum,
            (Content::Leaf(value), Axis::Vertical) => {
                opts.height = None;
                opts.overflow = Some(rich::Overflow::Ignore);
                opts.no_wrap = Some(true);
                fit_segments(
                    &value.rich_render(console, &opts),
                    opts.max_width,
                    self.overflow,
                )
                .len()
            }
            (Content::Split(child_axis, children), _) => {
                let widths = if *child_axis == Axis::Horizontal && axis == Axis::Vertical {
                    let constraints: Vec<_> = children
                        .iter()
                        .map(|child| child.constraint(Axis::Horizontal, console, &opts))
                        .collect();
                    allocate(opts.max_width, &constraints)
                        .map_or_else(|_| vec![0; children.len()], |a| a.sizes)
                } else {
                    vec![opts.max_width; children.len()]
                };
                let sizes = children.iter().zip(widths).map(|(child, width)| {
                    child.intrinsic(axis, console, &opts.update_width(width))
                });
                if *child_axis == axis {
                    sizes.fold(0usize, usize::saturating_add)
                } else {
                    sizes.max().unwrap_or(0)
                }
            }
        };
        let c = if axis == Axis::Horizontal {
            self.width
        } else {
            self.height
        };
        c.preferred
            .unwrap_or(natural)
            .max(c.min)
            .min(c.max.unwrap_or(usize::MAX))
    }
    fn block(&self, console: &Console, options: &ConsoleOptions) -> Vec<Vec<Segment>> {
        let available_width = options.max_width;
        let available_height = options.height.unwrap_or(console.height());
        if available_width == 0 || available_height == 0 {
            return Vec::new();
        }
        let width_constraint = self.constraint(Axis::Horizontal, console, options);
        let width = allocate(available_width, &[width_constraint]).map_or(0, |a| a.sizes[0]);
        if width == 0 {
            return (0..available_height)
                .map(|_| vec![Segment::new(" ".repeat(available_width), None)])
                .collect();
        }
        let height_constraint =
            self.constraint(Axis::Vertical, console, &options.update_width(width));
        let height = allocate(available_height, &[height_constraint]).map_or(0, |a| a.sizes[0]);
        let rows = self.render_block(console, &options.update_dimensions(width, height));
        let left = offset(available_width - width, self.horizontal);
        let top = offset(available_height - height, self.vertical);
        let blank = || vec![Segment::new(" ".repeat(available_width), None)];
        let mut output: Vec<_> = (0..top).map(|_| blank()).collect();
        for mut row in rows {
            if left > 0 {
                row.insert(0, Segment::new(" ".repeat(left), None));
            }
            let right = available_width - width - left;
            if right > 0 {
                row.push(Segment::new(" ".repeat(right), None));
            }
            output.push(row);
        }
        while output.len() < available_height {
            output.push(blank());
        }
        output
    }
    fn render_block(&self, console: &Console, options: &ConsoleOptions) -> Vec<Vec<Segment>> {
        let width = options.max_width;
        let height = options.height.unwrap_or(console.height());
        if width == 0 || height == 0 {
            return Vec::new();
        }
        let mut rows = match &self.content {
            Content::Leaf(value) => {
                let mut opts = options.clone();
                opts.height = None;
                opts.overflow = Some(rich::Overflow::Ignore);
                opts.no_wrap = Some(true);
                let segments = value.rich_render(console, &opts);
                fit_segments(&segments, width, self.overflow)
            }
            Content::Split(axis, children) => {
                let constraints: Vec<_> = children
                    .iter()
                    .map(|c| c.constraint(*axis, console, options))
                    .collect();
                let Ok(a) = allocate(
                    if *axis == Axis::Horizontal {
                        width
                    } else {
                        height
                    },
                    &constraints,
                ) else {
                    return Vec::new();
                };
                let blocks: Vec<_> = children
                    .iter()
                    .zip(&a.sizes)
                    .map(|(child, &size)| {
                        if size == 0 {
                            Vec::new()
                        } else {
                            child.block(
                                console,
                                &options.update_dimensions(
                                    if *axis == Axis::Horizontal {
                                        size
                                    } else {
                                        width
                                    },
                                    if *axis == Axis::Vertical {
                                        size
                                    } else {
                                        height
                                    },
                                ),
                            )
                        }
                    })
                    .collect();
                if *axis == Axis::Vertical {
                    blocks.into_iter().flatten().collect()
                } else {
                    let mut rows = vec![Vec::new(); height];
                    for block in blocks {
                        for (row, part) in rows.iter_mut().zip(block) {
                            row.extend(part);
                        }
                    }
                    rows
                }
            }
        };
        rows.truncate(height);
        // Visible overflow still obeys the container boundary.
        for row in &mut rows {
            let clipped = fit_segments(row, width, OverflowPolicy::Crop)
                .into_iter()
                .next()
                .unwrap_or_default();
            let cells = clipped.iter().map(Segment::cell_length).sum::<usize>();
            let left = offset(width.saturating_sub(cells), self.horizontal);
            *row = Vec::new();
            if left > 0 {
                row.push(Segment::new(" ".repeat(left), None));
            }
            row.extend(clipped);
            let right = width - cells - left;
            if right > 0 {
                row.push(Segment::new(" ".repeat(right), None));
            }
        }
        let top = offset(height - rows.len(), self.vertical);
        let blank = || vec![Segment::new(" ".repeat(width), None)];
        let mut output: Vec<_> = (0..top).map(|_| blank()).collect();
        output.extend(rows);
        while output.len() < height {
            output.push(blank());
        }
        output
    }
}
fn offset(extra: usize, alignment: Alignment) -> usize {
    match alignment {
        Alignment::Start => 0,
        Alignment::Center => extra / 2,
        Alignment::End => extra,
    }
}
impl Renderable for LayoutNode {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.validate().is_err() {
            return Vec::new();
        }
        let rows = self.block(console, options);
        let count = rows.len();
        rows.into_iter()
            .enumerate()
            .flat_map(|(i, mut row)| {
                if i + 1 < count {
                    row.push(Segment::line());
                }
                row
            })
            .collect()
    }
}
