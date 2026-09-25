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
#[derive(Clone, Default)]
pub struct CancelToken {
    node: Arc<Node>,
}

/// One token's flag and a shared link to its parent's. Clones share the
/// node, and children share their ancestors, so a deep chain costs one node
/// per level rather than a copy of every ancestor.
#[derive(Default)]
struct Node {
    flag: AtomicBool,
    parent: Option<Arc<Node>>,
}

impl Drop for Node {
    /// Unlink the ancestors one at a time: dropping a long chain recursively
    /// would overflow the stack.
    fn drop(&mut self) {
        let mut parent = self.parent.take();
        while let Some(node) = parent {
            match Arc::try_unwrap(node) {
                Ok(mut node) => parent = node.parent.take(),
                // Still shared: whoever holds it drops the rest.
                Err(_) => break,
            }
        }
    }
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
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
            node: Arc::new(Node {
                flag: AtomicBool::new(false),
                parent: Some(Arc::clone(&self.node)),
            }),
        }
    }

    /// Cancel this token, its clones and its children.
    pub fn cancel(&self) {
        self.node.flag.store(true, Ordering::SeqCst);
    }

    /// Whether this token or any of its ancestors was cancelled.
    pub fn is_cancelled(&self) -> bool {
        let mut node = Some(&self.node);
        while let Some(current) = node {
            if current.flag.load(Ordering::SeqCst) {
                return true;
            }
            node = current.parent.as_ref();
        }
        false
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

    #[test]
    fn a_deep_chain_is_shared_checked_and_dropped_without_recursion() {
        let root = CancelToken::new();
        let mut leaf = root.clone();
        let mut chain = Vec::new();
        for _ in 0..200_000 {
            leaf = leaf.child();
            chain.push(leaf.clone());
        }
        assert!(!leaf.is_cancelled());
        root.cancel();
        assert!(leaf.is_cancelled());
        assert_eq!(format!("{leaf:?}"), "CancelToken { cancelled: true }");
        // Drop the chain leaf-first and root-first: neither recurses.
        drop(chain);
        drop(leaf);
        let mut leaf = CancelToken::new();
        for _ in 0..200_000 {
            leaf = leaf.child();
        }
        drop(leaf);
    }
}
