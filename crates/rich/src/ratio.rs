//! Ratio-based size resolution.
//!
//! Port of `rich/_ratio.py`'s `ratio_resolve` — distributes a total span among
//! a set of edges, each of which may pin a fixed `size`, or flex by `ratio`
//! down to a `minimum_size`. Used by [`Layout`](crate::layout::Layout) to size
//! its split regions. (`Table` has its own `_ratio` helpers inline.)

/// One participant in a [`ratio_resolve`] distribution.
#[derive(Debug, Clone, Copy)]
pub struct Edge {
    /// A fixed size, if pinned.
    pub size: Option<usize>,
    /// Flex weight when `size` is `None` (defaults to 1 upstream).
    pub ratio: usize,
    /// The smallest size a flexible edge may shrink to.
    pub minimum_size: usize,
}

impl Edge {
    pub fn new(size: Option<usize>, ratio: usize, minimum_size: usize) -> Self {
        Edge {
            size,
            ratio,
            minimum_size,
        }
    }
}

/// Distribute `total` across `edges`, returning a concrete size per edge.
///
/// Direct port of `rich._ratio.ratio_resolve`, with its `Fraction`
/// arithmetic done exactly on integers (`portion = remaining / ratio_sum`
/// is kept as a numerator over `ratio_sum`), so no float rounding can drop
/// a cell.
pub fn ratio_resolve(total: usize, edges: &[Edge]) -> Vec<usize> {
    // `edge.size or None`: a size of 0 is flexible, like no size.
    let mut sizes: Vec<Option<usize>> = edges
        .iter()
        .map(|e| e.size.filter(|&size| size > 0))
        .collect();

    // Resolve one flexible edge per pass until all are fixed.
    while sizes.iter().any(Option::is_none) {
        let flexible: Vec<usize> = sizes
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_none())
            .map(|(i, _)| i)
            .collect();

        let fixed_sum: i128 = sizes.iter().flatten().map(|&s| s as i128).sum();
        let remaining = total as i128 - fixed_sum;
        if remaining <= 0 {
            // No room for flexible edges: give each its minimum (or its size).
            return sizes
                .iter()
                .zip(edges)
                .map(|(size, edge)| match size {
                    Some(s) => *s,
                    None => edge.minimum_size.max(1),
                })
                .collect();
        }

        // `portion = Fraction(remaining, sum(edge.ratio or 1 ...))`. `u128`
        // holds any `usize` product, so nothing here overflows.
        let remaining = remaining as u128;
        let ratio_sum: u128 = flexible
            .iter()
            .map(|&i| edges[i].ratio.max(1) as u128)
            .fold(0, u128::saturating_add);

        // If any flexible edge would fall below its minimum
        // (`portion * edge.ratio <= edge.minimum_size`), pin it and retry —
        // a newly fixed size changes the remaining distribution.
        let mut pinned = false;
        for &i in &flexible {
            let edge = &edges[i];
            if remaining * edge.ratio as u128
                <= (edge.minimum_size as u128).saturating_mul(ratio_sum)
            {
                sizes[i] = Some(edge.minimum_size);
                pinned = true;
                break;
            }
        }
        if !pinned {
            // `size, remainder = divmod(portion * edge.ratio + remainder, 1)`,
            // in units of `1 / ratio_sum`.
            let mut remainder = 0u128;
            for &i in &flexible {
                let value = (remaining * edges[i].ratio as u128).saturating_add(remainder);
                sizes[i] = Some(usize::try_from(value / ratio_sum).unwrap_or(usize::MAX));
                remainder = value % ratio_sum;
            }
            break;
        }
    }

    sizes.into_iter().map(|s| s.unwrap_or(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(specs: &[(Option<usize>, usize)]) -> Vec<Edge> {
        specs
            .iter()
            .map(|&(size, ratio)| Edge::new(size, ratio, 1))
            .collect()
    }

    #[test]
    fn even_split_carries_remainder() {
        // 23 across two ratio-1 edges → 11, 12 (matches upstream's divmod).
        assert_eq!(
            ratio_resolve(23, &edges(&[(None, 1), (None, 1)])),
            vec![11, 12]
        );
    }

    #[test]
    fn even_split_exact() {
        assert_eq!(
            ratio_resolve(24, &edges(&[(None, 1), (None, 1)])),
            vec![12, 12]
        );
    }

    #[test]
    fn fixed_and_flex() {
        // One flexible (ratio 3) + one fixed size 5, total 24 → 19, 5.
        assert_eq!(
            ratio_resolve(24, &edges(&[(None, 3), (Some(5), 1)])),
            vec![19, 5]
        );
    }

    #[test]
    fn ratio_weighting() {
        // 3:1 across 24 → 18, 6.
        assert_eq!(
            ratio_resolve(24, &edges(&[(None, 3), (None, 1)])),
            vec![18, 6]
        );
    }
}
