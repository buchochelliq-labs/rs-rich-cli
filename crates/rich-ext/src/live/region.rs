use rich::Segment;
use std::sync::Arc;
/// Opaque region identity. IDs cannot be reused by another coordinator.
#[derive(Clone, Debug)]
pub struct RegionId {
    pub(super) owner: Arc<()>,
    pub(super) serial: u64,
}
pub(super) struct Region {
    pub id: RegionId,
    pub content: Vec<Segment>,
}
