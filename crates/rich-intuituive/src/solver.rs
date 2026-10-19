//! Lengths solved as ratatui solves them: [`Solver::Ratatui`].
//!
//! intuiTUIve's own solver takes percentages of the room the gaps leave,
//! rounded down, and gives flexible children what is left. Ratatui's
//! `Layout` takes them of the whole length and lets a constraint solver
//! (kasuari, ratatui's fork of cassowary) squeeze what does not fit, so a
//! row ported from ratatui puts its cells 1–3 columns off with the native
//! solver. This module sets up the same constraints, at the same strengths,
//! as ratatui-core's `Layout::split` (MIT), and rounds the answer as it
//! does, so the cells land where ratatui puts them.

use kasuari::WeightedRelation::{EQ, GE, LE};
use kasuari::{Expression, Solver as Kasuari, Strength, Variable};

use crate::layout::{Justify, Size, Track};

/// How a [`column`](crate::column) or [`row`](crate::row) shares its
/// length among its children: [`Node::solver`](crate::Node::solver).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Solver {
    /// intuiTUIve's own (the default): percentages and ratios of the length
    /// the gaps leave, rounded down; flexible children share what is left,
    /// the last taking the remainder; too much is taken back from the end.
    #[default]
    Native,
    /// Ratatui's `Layout` with the stack's [`Justify`] as its `Flex`
    /// (`Start` by default: a ratatui `Table`'s columns). Percentages and
    /// ratios are of the whole length, gaps are its `spacing`, and what
    /// does not fit is squeezed as ratatui's solver squeezes it.
    /// [`Size::Fixed`] is `Length`, [`Size::Percent`] `Percentage`,
    /// [`Size::Ratio`] `Ratio`, [`Size::Flex`] `Fill`, [`Size::Auto`] a
    /// `Length` of the content; a minimum or maximum is a `Min` or `Max`
    /// over it. Not for wrapping stacks, which keep the native solver.
    /// Where the answer is not unique (fixed lengths against equal spacers,
    /// or that do not fit), which one the solver finds follows a hash order
    /// that changes from run to run, as ratatui's does: the cells can then
    /// land a column apart between runs.
    Ratatui,
    /// [`Ratatui`](Self::Ratatui) with `Flex::Legacy`, ratatui's default
    /// for a `Layout`: the last child takes what is left over.
    RatatuiLegacy,
}

/// Ratatui's `Flex`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flex {
    Legacy,
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

const PRECISION: f64 = 100.0;

/// Ratatui's strengths (`layout::strengths`).
const SPACER_SIZE_EQ: Strength = Strength::REQUIRED.div_f64(10.0);
const MIN_SIZE_GE: Strength = Strength::STRONG.mul_f64(100.0);
const MAX_SIZE_LE: Strength = Strength::STRONG.mul_f64(100.0);
const LENGTH_SIZE_EQ: Strength = Strength::STRONG.mul_f64(10.0);
const PERCENTAGE_SIZE_EQ: Strength = Strength::STRONG;
const RATIO_SIZE_EQ: Strength = Strength::STRONG.div_f64(10.0);
const FILL_GROW: Strength = Strength::MEDIUM;
const GROW: Strength = Strength::MEDIUM.div_f64(10.0);
const SPACE_GROW: Strength = Strength::WEAK.mul_f64(10.0);
const ALL_SEGMENT_GROW: Strength = Strength::WEAK;

/// A segment or a spacer: two variables.
#[derive(Clone, Copy)]
struct Element {
    start: Variable,
    end: Variable,
}

impl Element {
    fn size(&self) -> Expression {
        self.end - self.start
    }

    fn has_max_size(&self, size: u16, strength: Strength) -> kasuari::Constraint {
        self.size() | LE(strength) | (f64::from(size) * PRECISION)
    }

    fn has_min_size(&self, size: i16, strength: Strength) -> kasuari::Constraint {
        self.size() | GE(strength) | (f64::from(size) * PRECISION)
    }

    fn has_int_size(&self, size: u16, strength: Strength) -> kasuari::Constraint {
        self.size() | EQ(strength) | (f64::from(size) * PRECISION)
    }

    fn has_size<E: Into<Expression>>(&self, size: E, strength: Strength) -> kasuari::Constraint {
        self.size() | EQ(strength) | size.into()
    }

    fn has_double_size<E: Into<Expression>>(
        &self,
        size: E,
        strength: Strength,
    ) -> kasuari::Constraint {
        self.size() | EQ(strength) | (size.into() * 2.0)
    }

    fn is_empty(&self) -> kasuari::Constraint {
        self.size() | EQ(Strength::REQUIRED - Strength::WEAK) | 0.0
    }
}

impl From<Element> for Expression {
    fn from(element: Element) -> Expression {
        element.size()
    }
}

/// The flex `solver` and `justify` make.
fn flex_of(solver: Solver, justify: Justify) -> Flex {
    match (solver, justify) {
        (Solver::RatatuiLegacy, _) => Flex::Legacy,
        (_, Justify::Start) => Flex::Start,
        (_, Justify::Center) => Flex::Center,
        (_, Justify::End) => Flex::End,
        (_, Justify::SpaceBetween) => Flex::SpaceBetween,
        (_, Justify::SpaceAround) => Flex::SpaceAround,
        (_, Justify::SpaceEvenly) => Flex::SpaceEvenly,
    }
}

/// Each track's offset and length when they share the `total` cells from
/// `start`, `gap` apart, as ratatui's `Layout::split` would lay them out.
pub(crate) fn split(
    start: u16,
    total: u16,
    gap: u16,
    tracks: &[Track],
    solver: Solver,
    justify: Justify,
) -> Vec<(u16, u16)> {
    if tracks.is_empty() {
        return Vec::new();
    }
    // A constraint the solver refuses (it never should) falls back to
    // nothing at all, rather than a panic in a frame.
    solve(start, total, gap, tracks, flex_of(solver, justify))
        .unwrap_or_else(|_| vec![(start, 0); tracks.len()])
}

fn solve(
    start: u16,
    total: u16,
    gap: u16,
    tracks: &[Track],
    flex: Flex,
) -> Result<Vec<(u16, u16)>, kasuari::AddConstraintError> {
    let mut solver = Kasuari::new();
    let variables: Vec<Variable> = (0..tracks.len() * 2 + 2).map(|_| Variable::new()).collect();
    let pairs = |from: usize| -> Vec<Element> {
        variables[from..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[start, end]| Element { start, end })
            .collect()
    };
    let spacers = pairs(0);
    let segments = pairs(1);
    let area = Element {
        start: variables[0],
        end: *variables.last().expect("two at least"),
    };
    let end = u32::from(start) + u32::from(total);
    let (from, to) = (f64::from(start) * PRECISION, f64::from(end) * PRECISION);
    solver.add_constraint(area.start | EQ(Strength::REQUIRED) | from)?;
    solver.add_constraint(area.end | EQ(Strength::REQUIRED) | to)?;
    for &variable in &variables {
        solver.add_constraint(variable | GE(Strength::REQUIRED) | area.start)?;
        solver.add_constraint(variable | LE(Strength::REQUIRED) | area.end)?;
    }
    for &[left, right] in variables[1..].as_chunks::<2>().0 {
        solver.add_constraint(left | LE(Strength::REQUIRED) | right)?;
    }
    flex_constraints(&mut solver, area, &spacers, flex, gap as i16)?;
    for (track, segment) in tracks.iter().zip(&segments) {
        match track.size {
            Size::Fixed(cells) => {
                solver.add_constraint(segment.has_int_size(cells, LENGTH_SIZE_EQ))?;
            }
            Size::Auto => {
                solver.add_constraint(segment.has_int_size(track.content, LENGTH_SIZE_EQ))?;
            }
            Size::Percent(p) => {
                let size = area.size() * f64::from(p) / 100.0;
                solver.add_constraint(segment.has_size(size, PERCENTAGE_SIZE_EQ))?;
            }
            Size::Ratio(a, b) => {
                let size = area.size() * f64::from(a) / f64::from(b.max(1));
                solver.add_constraint(segment.has_size(size, RATIO_SIZE_EQ))?;
            }
            Size::Flex(_) => {
                solver.add_constraint(segment.has_size(area, FILL_GROW))?;
            }
        }
        if track.min > 0 {
            let min = track.min.min(i16::MAX as u16) as i16;
            solver.add_constraint(segment.has_min_size(min, MIN_SIZE_GE))?;
        }
        if track.max < u16::MAX {
            solver.add_constraint(segment.has_max_size(track.max, MAX_SIZE_LE))?;
        }
    }
    // Fill shares, each against each, by weight.
    let fills: Vec<(f64, Element)> = tracks
        .iter()
        .zip(&segments)
        .filter_map(|(track, segment)| match track.size {
            Size::Flex(weight) => Some((f64::from(weight).max(1e-6), *segment)),
            _ => None,
        })
        .collect();
    for (i, &(left_weight, left)) in fills.iter().enumerate() {
        for &(right_weight, right) in &fills[i + 1..] {
            solver.add_constraint(
                (right_weight * left.size()) | EQ(GROW) | (left_weight * right.size()),
            )?;
        }
    }
    if flex != Flex::Legacy {
        for pair in segments.windows(2) {
            solver.add_constraint(pair[0].has_size(pair[1], ALL_SEGMENT_GROW))?;
        }
    }
    let changes: std::collections::HashMap<Variable, f64> =
        solver.fetch_changes().iter().copied().collect();
    let at = |variable: &Variable| {
        let value = changes.get(variable).copied().unwrap_or(0.0);
        (value.round() / PRECISION).round() as u16
    };
    Ok(segments
        .iter()
        .map(|segment| {
            let (start, end) = (at(&segment.start), at(&segment.end));
            (start, end.saturating_sub(start))
        })
        .collect())
}

/// The spacers' constraints for `flex`: ratatui's
/// `configure_flex_constraints`.
fn flex_constraints(
    solver: &mut Kasuari,
    area: Element,
    spacers: &[Element],
    flex: Flex,
    spacing: i16,
) -> Result<(), kasuari::AddConstraintError> {
    let inner = spacers.get(1..spacers.len() - 1).unwrap_or(&[]);
    let spacing_f64 = f64::from(spacing) * PRECISION;
    let (first, last) = (spacers[0], spacers[spacers.len() - 1]);
    let all_equal = |solver: &mut Kasuari, list: &[Element]| {
        for (i, left) in list.iter().enumerate() {
            for right in &list[i + 1..] {
                solver.add_constraint(left.has_size(*right, SPACER_SIZE_EQ))?;
            }
        }
        Ok::<(), kasuari::AddConstraintError>(())
    };
    match flex {
        Flex::Legacy => {
            for spacer in inner {
                solver.add_constraint(spacer.has_size(spacing_f64, SPACER_SIZE_EQ))?;
            }
            solver.add_constraint(first.is_empty())?;
            solver.add_constraint(last.is_empty())?;
        }
        Flex::SpaceAround if spacers.len() <= 2 => {
            all_equal(solver, spacers)?;
            for spacer in spacers {
                solver.add_constraint(spacer.has_min_size(spacing, SPACER_SIZE_EQ))?;
                solver.add_constraint(spacer.has_size(area, SPACE_GROW))?;
            }
        }
        Flex::SpaceAround => {
            all_equal(solver, inner)?;
            if let Some(first_middle) = inner.first() {
                solver.add_constraint(first_middle.has_double_size(first, SPACER_SIZE_EQ))?;
                solver.add_constraint(first_middle.has_double_size(last, SPACER_SIZE_EQ))?;
            }
            for spacer in spacers {
                solver.add_constraint(spacer.has_min_size(spacing, SPACER_SIZE_EQ))?;
                solver.add_constraint(spacer.has_size(area, SPACE_GROW))?;
            }
        }
        Flex::SpaceEvenly => {
            all_equal(solver, spacers)?;
            for spacer in spacers {
                solver.add_constraint(spacer.has_min_size(spacing, SPACER_SIZE_EQ))?;
                solver.add_constraint(spacer.has_size(area, SPACE_GROW))?;
            }
        }
        Flex::SpaceBetween => {
            all_equal(solver, inner)?;
            for spacer in inner {
                solver.add_constraint(spacer.has_min_size(spacing, SPACER_SIZE_EQ))?;
                solver.add_constraint(spacer.has_size(area, SPACE_GROW))?;
            }
            solver.add_constraint(first.is_empty())?;
            solver.add_constraint(last.is_empty())?;
        }
        Flex::Start | Flex::Center | Flex::End => {
            for spacer in inner {
                solver.add_constraint(spacer.has_size(spacing_f64, SPACER_SIZE_EQ))?;
            }
            match flex {
                Flex::Start => {
                    solver.add_constraint(first.is_empty())?;
                    solver.add_constraint(last.has_size(area, GROW))?;
                }
                Flex::Center => {
                    solver.add_constraint(first.has_size(area, GROW))?;
                    solver.add_constraint(last.has_size(area, GROW))?;
                    solver.add_constraint(first.has_size(last, SPACER_SIZE_EQ))?;
                }
                _ => {
                    solver.add_constraint(last.is_empty())?;
                    solver.add_constraint(first.has_size(area, GROW))?;
                }
            }
        }
    }
    Ok(())
}
