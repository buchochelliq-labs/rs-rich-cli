//! orbit as a library: the scheduler and the status page.

pub mod scheduler {
    /// How many jobs run at once.
    pub const DEFAULT_WORKERS: usize = 4;
    /// Tries before a job fails.
    pub const RETRIES: u32 = 3;
}
