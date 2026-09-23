//! Bounded allocation independent of the faithful core ratio resolver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Constraint {
    pub min: usize,
    pub max: Option<usize>,
    pub preferred: Option<usize>,
    pub flex: usize,
}
impl Default for Constraint {
    fn default() -> Self {
        Self {
            min: 0,
            max: None,
            preferred: None,
            flex: 1,
        }
    }
}
impl Constraint {
    pub fn fixed(size: usize) -> Self {
        Self {
            preferred: Some(size),
            flex: 0,
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<(), ConstraintError> {
        if self.max.is_some_and(|m| m < self.min) {
            return Err(ConstraintError::InvalidBounds);
        }
        if let Some(p) = self.preferred {
            if p < self.min || self.max.is_some_and(|m| p > m) {
                return Err(ConstraintError::InvalidPreference);
            }
            if self.flex != 0 {
                return Err(ConstraintError::InvalidWeight);
            }
        } else if self.flex == 0 {
            return Err(ConstraintError::InvalidWeight);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub sizes: Vec<usize>,
    pub padding: usize,
    pub relaxed: Vec<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintError {
    InvalidBounds,
    InvalidPreference,
    InvalidWeight,
    ArithmeticOverflow,
}
impl std::fmt::Display for ConstraintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid layout constraint: {self:?}")
    }
}
impl std::error::Error for ConstraintError {}
fn sum(mut values: impl Iterator<Item = usize>) -> Result<u128, ConstraintError> {
    values.try_fold(0u128, |a, v| {
        a.checked_add(v as u128)
            .ok_or(ConstraintError::ArithmeticOverflow)
    })
}
fn proportional(total: usize, weights: &[usize]) -> Result<Vec<usize>, ConstraintError> {
    let weight = sum(weights.iter().copied())?;
    if weight == 0 {
        return Ok(vec![0; weights.len()]);
    }
    let mut sizes = weights
        .iter()
        .map(|&w| {
            (total as u128)
                .checked_mul(w as u128)
                .map(|v| (v / weight) as usize)
                .ok_or(ConstraintError::ArithmeticOverflow)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut left = total - sizes.iter().sum::<usize>();
    for (size, &w) in sizes.iter_mut().zip(weights) {
        if left == 0 {
            break;
        }
        if w > 0 {
            *size += 1;
            left -= 1;
        }
    }
    Ok(sizes)
}
pub fn allocate(total: usize, constraints: &[Constraint]) -> Result<Allocation, ConstraintError> {
    for c in constraints {
        c.validate()?;
    }
    let minima: Vec<_> = constraints.iter().map(|c| c.min).collect();
    let min_sum = sum(minima.iter().copied())?;
    let mut sizes: Vec<_> = constraints
        .iter()
        .map(|c| c.preferred.unwrap_or(c.min))
        .collect();
    let initial = sum(sizes.iter().copied())?;
    if min_sum > total as u128 {
        sizes = proportional(total, &minima)?;
    } else if initial > total as u128 {
        let slack: Vec<_> = sizes.iter().zip(&minima).map(|(p, m)| p - m).collect();
        let extras = proportional(total - min_sum as usize, &slack)?;
        sizes = minima.iter().zip(extras).map(|(m, e)| m + e).collect();
    } else {
        let mut remaining = total - initial as usize;
        while remaining > 0 {
            let weights: Vec<_> = constraints
                .iter()
                .zip(&sizes)
                .map(|(c, &s)| {
                    if s < c.max.unwrap_or(usize::MAX) {
                        c.flex
                    } else {
                        0
                    }
                })
                .collect();
            if weights.iter().all(|&w| w == 0) {
                break;
            }
            let portions = proportional(remaining, &weights)?;
            let mut used = 0;
            for ((size, c), portion) in sizes.iter_mut().zip(constraints).zip(portions) {
                let increment = portion.min(c.max.unwrap_or(usize::MAX) - *size);
                *size += increment;
                used += increment;
            }
            if used == 0 {
                break;
            }
            remaining -= used;
        }
    }
    let padding = total - sizes.iter().sum::<usize>();
    let relaxed = constraints
        .iter()
        .zip(&sizes)
        .enumerate()
        .filter_map(|(i, (c, &s))| {
            if s < c.preferred.unwrap_or(c.min) {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    Ok(Allocation {
        sizes,
        padding,
        relaxed,
    })
}
