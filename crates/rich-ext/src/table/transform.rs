//! Transforms over [`TableData`]: sort and group, for a
//! [`Pipeline`](crate::transform::Pipeline).

use super::{GroupBy, SortKey, TableData};
use crate::transform::{Transform, TransformError};

/// Sorts rows by these keys, replacing any earlier sort.
#[derive(Clone, Debug)]
pub struct Sort(pub Vec<SortKey>);

impl Transform<TableData> for Sort {
    fn apply(&self, mut data: TableData) -> Result<TableData, TransformError> {
        data.set_sort(self.0.iter().copied());
        Ok(data)
    }
}

/// Groups rows, replacing any earlier grouping.
#[derive(Clone)]
pub struct Group(pub GroupBy);

impl Transform<TableData> for Group {
    fn apply(&self, data: TableData) -> Result<TableData, TransformError> {
        Ok(data.group_by(self.0.clone()))
    }
}
