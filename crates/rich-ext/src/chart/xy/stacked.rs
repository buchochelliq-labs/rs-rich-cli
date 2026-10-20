//! Stacked areas: [`Chart::stacked`] puts each [`GraphType::Area`] dataset
//! on top of the ones before it.

use std::borrow::Cow;

use super::super::describe::extent;
use super::{AxisScale, Chart, GraphType};

impl Chart {
    /// Stack the [`GraphType::Area`] datasets (default off): each area's
    /// values are drawn on top of the areas before it, so the top edge is
    /// their running total and each band is one dataset's share. The axes
    /// fit the totals (a linear y axis without bounds runs from the fill
    /// level), and every stacked area fills down to the first
    /// one's [`fill_to`](super::Dataset::fill_to) level (0 by default), the top layer
    /// drawn first, so each band shows in its own style. Datasets that
    /// share x values stack exactly; between another area's points the
    /// area below is interpolated along its line. Other datasets are drawn
    /// as they are, over the areas. The legend and the accessible text
    /// keep each dataset's own values.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::chart::{Axis, Chart, Dataset, GraphType, LegendPosition, Marker};
    ///
    /// let console = Console::builder().width(8).color_system(None).build();
    /// let area = |points: [(f64, f64); 2], c: char| {
    ///     Dataset::new(points).graph_type(GraphType::Area).marker(Marker::Ascii(c))
    /// };
    /// let chart = Chart::new(vec![
    ///     area([(0.0, 1.0), (7.0, 1.0)], '#'),
    ///     area([(0.0, 1.0), (7.0, 2.0)], '+'),
    /// ])
    /// .x_axis(Axis::default().bounds(0.0, 7.0))
    /// .y_axis(Axis::default().bounds(0.0, 3.0))
    /// .legend(LegendPosition::None)
    /// .stacked(true)
    /// .width(8)
    /// .height(4);
    /// assert_eq!(
    ///     console.render_export(&chart),
    ///     concat!(
    ///         "     +++\n",
    ///         "++++++++\n",
    ///         "########\n",
    ///         "########\n",
    ///     )
    /// );
    /// ```
    pub fn stacked(mut self, stacked: bool) -> Self {
        self.stacked = stacked;
        self
    }

    /// Whether any areas are stacked.
    pub(super) fn stacks(&self) -> bool {
        self.stacked
            && self
                .datasets
                .iter()
                .any(|d| d.graph_type == GraphType::Area)
    }

    /// The order the datasets are drawn in: stacked areas first, the top
    /// one first, then the others in order.
    pub(super) fn draw_order(&self) -> Vec<usize> {
        let all = 0..self.datasets.len();
        if !self.stacks() {
            return all.collect();
        }
        let area = |i: &usize| self.datasets[*i].graph_type == GraphType::Area;
        let mut order: Vec<usize> = all.clone().filter(area).rev().collect();
        order.extend(all.filter(|i| !area(i)));
        order
    }

    /// This chart with each stacked area's y values replaced by the running
    /// total, and every one filling to the first one's level; the chart
    /// itself when nothing is stacked.
    pub(super) fn stacked_view(&self) -> Cow<'_, Chart> {
        if !self.stacks() {
            return Cow::Borrowed(self);
        }
        let mut view = self.clone();
        let mut below: Vec<(f64, f64)> = Vec::new();
        let mut level = None;
        for dataset in view
            .datasets
            .iter_mut()
            .filter(|d| d.graph_type == GraphType::Area)
        {
            let base = *level.get_or_insert(dataset.fill_to);
            dataset.fill_to = base;
            let stacked: Vec<(f64, f64)> = dataset
                .points
                .iter()
                .map(|&(x, y)| (x, y + under(&below, x)))
                .collect();
            below = stacked
                .iter()
                .copied()
                .filter(|(x, y)| x.is_finite() && y.is_finite())
                .collect();
            below.sort_by(|a, b| a.0.total_cmp(&b.0));
            dataset.points = stacked;
        }
        // Without bounds, a linear y axis runs from the fill level.
        if let (None, AxisScale::Linear, Some(base)) =
            (view.y_axis.bounds, view.y_axis.scale, level)
        {
            let ys = view
                .datasets
                .iter()
                .flat_map(|d| d.points.iter().map(|p| p.1));
            if let Some((lo, hi)) = extent(ys.chain([base])) {
                if lo < hi {
                    view.y_axis.bounds = Some([lo, hi]);
                }
            }
        }
        Cow::Owned(view)
    }
}

/// The height of the line through `points` (sorted by x) at `x`: linear
/// between them, 0 outside them.
fn under(points: &[(f64, f64)], x: f64) -> f64 {
    let Some(at) = points.iter().position(|p| p.0 >= x) else {
        return 0.0;
    };
    let (x1, y1) = points[at];
    if x1 == x {
        return y1;
    }
    let Some(&(x0, y0)) = at.checked_sub(1).map(|i| &points[i]) else {
        return 0.0;
    };
    y0 + (y1 - y0) * (x - x0) / (x1 - x0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layer_below_is_interpolated_and_zero_outside() {
        let line = [(0.0, 1.0), (2.0, 3.0)];
        assert_eq!(under(&line, 0.0), 1.0);
        assert_eq!(under(&line, 1.0), 2.0);
        assert_eq!(under(&line, 2.0), 3.0);
        assert_eq!(under(&line, 3.0), 0.0);
        assert_eq!(under(&line, -1.0), 0.0);
        assert_eq!(under(&[], 1.0), 0.0);
    }
}
