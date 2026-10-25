//! Ratio-based size resolution.
//!
//! Port of `rich/_ratio.py`. [`ratio_resolve`] distributes a total span among
//! a set of edges, each of which may pin a fixed `size`, or flex by `ratio`
//! down to a `minimum_size`; [`Layout`](crate::layout::Layout) sizes its split
//! regions with it. [`ratio_reduce`] and [`ratio_distribute`] size `Table`
//! columns.

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

/// Reduce `values` by `total`, distributed across slots by `ratios` (capped by
/// `maximums`): a slot whose maximum is 0 takes no share. Port of
/// `rich._ratio.ratio_reduce`.
pub fn ratio_reduce(total: i64, ratios: &[i64], maximums: &[i64], values: &[i64]) -> Vec<i64> {
    let ratios: Vec<i64> = ratios
        .iter()
        .zip(maximums)
        .map(|(&r, &m)| if m != 0 { r } else { 0 })
        .collect();
    let mut total_ratio: i128 = ratios.iter().map(|&r| i128::from(r)).sum();
    if total_ratio == 0 {
        return values.to_vec();
    }
    let mut total_remaining = total;
    let mut result = Vec::with_capacity(values.len());
    for ((&ratio, &maximum), &value) in ratios.iter().zip(maximums).zip(values) {
        if ratio != 0 && total_ratio > 0 {
            // `round(ratio * total_remaining / total_ratio)`: Python rounds
            // half to even.
            let distributed = maximum.min(
                (ratio as f64 * total_remaining as f64 / total_ratio as f64).round_ties_even()
                    as i64,
            );
            result.push(value - distributed);
            total_remaining -= distributed;
            total_ratio -= i128::from(ratio);
        } else {
            result.push(value);
        }
    }
    result
}

/// Divide `total` across slots proportionally to `ratios` (ceil each share),
/// each share floored at the matching `minimums` entry when given. Port of
/// `rich._ratio.ratio_distribute`; where upstream asserts the ratios sum to
/// more than 0, the whole total goes to the first slot.
pub fn ratio_distribute(total: i64, ratios: &[i64], minimums: Option<&[i64]>) -> Vec<i64> {
    // Upstream zeroes the ratio of any slot whose minimum is 0 (falsy).
    let ratios: Vec<i64> = match minimums {
        Some(mins) => ratios
            .iter()
            .zip(mins)
            .map(|(&r, &m)| if m != 0 { r } else { 0 })
            .collect(),
        None => ratios.to_vec(),
    };
    // Python ints never overflow; `ratio * total_remaining` can exceed i64
    // for a huge ratio, so the arithmetic runs in i128.
    let mut total_ratio: i128 = ratios.iter().map(|&r| i128::from(r)).sum();
    let mut total_remaining = i128::from(total);
    let mut result = Vec::with_capacity(ratios.len());
    for (index, &ratio) in ratios.iter().enumerate() {
        let ratio = i128::from(ratio);
        let minimum = i128::from(minimums.map_or(0, |m| m[index]));
        let distributed = if total_ratio > 0 {
            // `max(minimum, ceil(ratio * total_remaining / total_ratio))`;
            // the remaining total goes negative once a minimum overshoots.
            let numerator = ratio * total_remaining;
            minimum.max(-(-numerator).div_euclid(total_ratio))
        } else {
            total_remaining
        };
        result.push(i64::try_from(distributed).unwrap_or(if distributed < 0 {
            i64::MIN
        } else {
            i64::MAX
        }));
        total_ratio -= ratio;
        total_remaining -= distributed;
    }
    result
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
