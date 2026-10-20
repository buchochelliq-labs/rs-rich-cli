//! Time and threads where an app can run without them.
//!
//! On `wasm32-unknown-unknown` (an app in a browser tab, as the docs
//! playground runs one) std has no clock: `Instant::now` and
//! `SystemTime::now` panic, and so does `std::thread::spawn`. The places an
//! app reaches them go through here instead. Natively everything is std's,
//! unchanged.
//!
//! - [`Instant`] is `std::time::Instant` natively; on wasm32 it is the
//!   app's clock (what its `Driver` was last given), so short spans (a
//!   double click, a throttle) are measured on that clock, and the
//!   inspector's frame timings read zero.
//! - [`off_thread`] runs work on a thread of its own natively; on wasm32 it
//!   runs it at once, on the app's thread, so a [`spawn`](crate::spawn)'s
//!   result arrives on the next turn.

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::Instant;

/// The app's clock as an instant (see the [module docs](self)).
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Instant(std::time::Duration);

#[cfg(target_arch = "wasm32")]
impl Instant {
    pub(crate) fn now() -> Instant {
        Instant(crate::sprite::now())
    }

    pub(crate) fn elapsed(&self) -> std::time::Duration {
        Instant::now().duration_since(*self)
    }

    pub(crate) fn duration_since(&self, earlier: Instant) -> std::time::Duration {
        self.0.saturating_sub(earlier.0)
    }

    pub(crate) fn saturating_duration_since(&self, earlier: Instant) -> std::time::Duration {
        self.duration_since(earlier)
    }
}

#[cfg(target_arch = "wasm32")]
impl std::ops::Add<std::time::Duration> for Instant {
    type Output = Instant;
    fn add(self, other: std::time::Duration) -> Instant {
        Instant(self.0 + other)
    }
}

/// Seconds since 1970-01-01 UTC. On wasm32 there is no wall clock: the
/// app's clock in seconds stands in.
pub(crate) fn unix_seconds() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
    #[cfg(target_arch = "wasm32")]
    {
        crate::sprite::now().as_secs()
    }
}

/// Run `work` on a thread of its own; on wasm32, which has no threads, at
/// once on this one.
pub(crate) fn off_thread(work: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(work);
    #[cfg(target_arch = "wasm32")]
    work();
}
