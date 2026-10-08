//! How a backend's thread wakes the app: its [`Notify`] sends one job
//! through the app's [`Proxy`](rich_intuituive::Proxy), and the job reads
//! the backend on the app's thread, between frames.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rich_intuituive::{signal, watch, Ctx};

use crate::host::Notify;

/// A widget's state that a backend feeds.
pub(crate) trait Fed: 'static {
    /// Give the backend its notify.
    fn install(&mut self, notify: Notify);
    /// Read the backend, on the app's thread. Called with the state not
    /// borrowed, so it may run handlers that reach the widget again.
    fn pump(this: &Rc<RefCell<Self>>, cx: &mut Ctx);
}

/// Connect `state`'s backend to the app being built. The app's proxy is
/// reached through a watch, which runs with a [`Ctx`] on the app's first
/// turn; from then on every notify from the backend's thread queues one
/// job (however many come before it runs), and the job pumps `state` if
/// its widget is still there.
pub(crate) fn connect<S: Fed>(state: &Rc<RefCell<S>>) {
    // A signal's handle may go to another thread; its value, the weak
    // reference, stays on the app's.
    let cell = signal(Rc::downgrade(state));
    let mut connected = false;
    watch(
        || (),
        move |(), cx| {
            if connected {
                return;
            }
            connected = true;
            let Some(state) = cell.with_untracked(Weak::upgrade) else {
                return;
            };
            let proxy = cx.proxy();
            let queued = Arc::new(AtomicBool::new(false));
            let notify: Notify = Arc::new(move || {
                if queued.swap(true, Ordering::SeqCst) {
                    return;
                }
                let queued = Arc::clone(&queued);
                proxy.run_with(move |cx| {
                    queued.store(false, Ordering::SeqCst);
                    if let Some(state) = cell.with_untracked(Weak::upgrade) {
                        S::pump(&state, cx);
                    }
                });
            });
            state.borrow_mut().install(notify);
            S::pump(&state, cx);
        },
    );
}
