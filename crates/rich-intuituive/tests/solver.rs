//! `Solver::Ratatui`: rows and columns laid out as ratatui's `Layout` lays
//! them out, checked against ratatui itself over many widths.

use intuituive::layout::{split, Track};
use intuituive::node::Axis;
use intuituive::screen::Rect;
use intuituive::{Justify, Size, Solver, StackLayout};
use ratatui::layout::{Constraint, Flex, Layout};

/// Ours and ratatui's, for one set of sizes.
fn compare(sizes: &[Size], gap: u16, justify: Justify, solver: Solver, widths: &[u16]) {
    let flex = match (solver, justify) {
        (Solver::RatatuiLegacy, _) => Flex::Legacy,
        (_, Justify::Start) => Flex::Start,
        (_, Justify::Center) => Flex::Center,
        (_, Justify::End) => Flex::End,
        (_, Justify::SpaceBetween) => Flex::SpaceBetween,
        (_, Justify::SpaceAround) => Flex::SpaceAround,
        (_, Justify::SpaceEvenly) => Flex::SpaceEvenly,
    };
    let constraints: Vec<Constraint> = sizes
        .iter()
        .map(|size| match *size {
            Size::Fixed(n) => Constraint::Length(n),
            Size::Percent(p) => Constraint::Percentage(p),
            Size::Ratio(a, b) => Constraint::Ratio(a, b),
            Size::Flex(w) => Constraint::Fill(w),
            _ => unreachable!(),
        })
        .collect();
    let tracks: Vec<Track> = sizes.iter().map(|&size| Track::new(size)).collect();
    let flow = StackLayout {
        gap,
        justify,
        solver,
        ..StackLayout::default()
    };
    for &width in widths {
        let area = ratatui::layout::Rect::new(3, 0, width, 1);
        let theirs: Vec<(u16, u16)> = Layout::horizontal(constraints.clone())
            .flex(flex)
            .spacing(gap)
            .split(area)
            .iter()
            .map(|r| (r.x, r.width))
            .collect();
        let ours: Vec<(u16, u16)> =
            split(Rect::new(3, 0, width, 1), Axis::Horizontal, &tracks, flow)
                .iter()
                .map(|r| (r.x, r.width))
                .collect();
        // Fixed lengths that do not fit have no one answer: ratatui's then
        // depends on how many variables its solver made before (a hash
        // order), so it differs from run to run. Everything else matches.
        let fixed: u16 = sizes
            .iter()
            .map(|size| match size {
                Size::Fixed(n) => *n,
                _ => 0,
            })
            .sum::<u16>()
            + gap * (sizes.len() as u16 - 1);
        if fixed > width {
            continue;
        }
        assert_eq!(ours, theirs, "{sizes:?} gap {gap} {flex:?} at {width}");
    }
}

#[test]
fn scope_tuis_header_lands_where_ratatuis_table_puts_it() {
    let header = [35, 25, 7, 13, 6, 6, 6].map(Size::Percent);
    let widths: Vec<u16> = (10..=200).collect();
    compare(&header, 1, Justify::Start, Solver::Ratatui, &widths);
}

#[test]
fn every_flex_matches_ratatui() {
    let sets: Vec<Vec<Size>> = vec![
        vec![Size::Percent(20), Size::Percent(30), Size::Percent(50)],
        vec![Size::Percent(33), Size::Percent(33), Size::Percent(33)],
        vec![Size::Fixed(10), Size::Flex(1), Size::Fixed(10)],
        vec![Size::Flex(1), Size::Flex(2), Size::Percent(25)],
        vec![Size::Ratio(1, 3), Size::Ratio(1, 3), Size::Ratio(1, 3)],
        vec![Size::Fixed(5), Size::Fixed(8), Size::Fixed(3)],
        vec![Size::Percent(10), Size::Fixed(30), Size::Flex(1)],
    ];
    let widths = [7, 16, 31, 40, 63, 80, 81, 99, 120];
    for sizes in &sets {
        for gap in [0, 1, 2] {
            for justify in [
                Justify::Start,
                Justify::Center,
                Justify::End,
                Justify::SpaceBetween,
                Justify::SpaceAround,
                Justify::SpaceEvenly,
            ] {
                compare(sizes, gap, justify, Solver::Ratatui, &widths);
            }
            compare(sizes, gap, Justify::Start, Solver::RatatuiLegacy, &widths);
        }
    }
}

#[test]
fn a_row_with_the_ratatui_solver_lays_out_its_children_so() {
    use intuituive::prelude::*;
    let app = App::new(|| {
        row([35, 25, 40].map(|p| label("|").percent(p)))
            .gap(1)
            .solver(Solver::Ratatui)
    });
    let mut driver = app.driver(80, 1);
    driver.settle();
    // 28 | 20 | 30 with two gaps: the last squeezed by the gaps.
    let row = &driver.screen().plain()[0];
    let bars: Vec<usize> = row.match_indices('|').map(|(at, _)| at).collect();
    let theirs: Vec<u16> = Layout::horizontal([35, 25, 40].map(Constraint::Percentage))
        .flex(Flex::Start)
        .spacing(1)
        .split(ratatui::layout::Rect::new(0, 0, 80, 1))
        .iter()
        .map(|r| r.x)
        .collect();
    assert_eq!(bars, theirs.iter().map(|&x| x as usize).collect::<Vec<_>>());
}

#[test]
fn the_native_solver_is_unchanged() {
    // Percentages of what the gap leaves, rounded down, as before.
    let tracks = [Track::new(Size::Percent(50)), Track::new(Size::Percent(50))];
    let flow = StackLayout {
        gap: 1,
        ..StackLayout::default()
    };
    let areas = split(Rect::new(0, 0, 20, 1), Axis::Horizontal, &tracks, flow);
    assert_eq!(areas, [Rect::new(0, 0, 9, 1), Rect::new(10, 0, 9, 1)]);
}
