//! A shared cancellation flag for workflows: task trees, transfers, retries.
//!
//! One [`CancelToken`] is cloned into everything that should stop together.
//! Cancelling any clone cancels them all; work checks
//! [`is_cancelled`](CancelToken::is_cancelled) at its own safe points. A
//! [`child`](CancelToken::child) token is cancelled with its parent but can
//! also be cancelled alone, which is how one branch of a task tree stops
//! without stopping its siblings.
//!
//! ```
//! use rich_ext::cancel::CancelToken;
//!
//! let root = CancelToken::new();
//! let download = root.child();
//! let worker = download.clone();
//!
//! download.cancel();
//! assert!(worker.is_cancelled());
//! assert!(!root.is_cancelled());
//!
//! let other = root.child();
//! root.cancel();
//! assert!(other.is_cancelled());
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A cloneable, thread-safe cancellation flag. See the [module docs](self).
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    parent: Option<Box<CancelToken>>,
}

impl CancelToken {
    /// A token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// A token cancelled when this one is, which can also be cancelled on its
    /// own without affecting this one.
    pub fn child(&self) -> Self {
        CancelToken {
            flag: Arc::new(AtomicBool::new(false)),
            parent: Some(Box::new(self.clone())),
        }
    }

    /// Cancel this token, its clones and its children.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether this token or any of its ancestors was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst) || self.parent.as_ref().is_some_and(|p| p.is_cancelled())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_state_across_threads() {
        let token = CancelToken::new();
        let remote = token.clone();
        std::thread::spawn(move || remote.cancel()).join().unwrap();
        assert!(token.is_cancelled());
    }

    #[test]
    fn grandchildren_follow_the_root_but_not_siblings() {
        let root = CancelToken::new();
        let a = root.child();
        let a1 = a.child();
        let b = root.child();
        a.cancel();
        assert!(a1.is_cancelled());
        assert!(!b.is_cancelled());
        assert!(!root.is_cancelled());
        root.cancel();
        assert!(b.is_cancelled());
    }
}
