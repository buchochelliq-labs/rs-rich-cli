//! A pane that loads a value off the app's thread for whatever key is
//! current, drops the results for keys left behind, and draws the value.

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::node::Node;
use crate::reactive::{memo, on_write_ref, signal, watch, Signal};
use crate::task::{resource_for, Load};

type OnChange<K> = Rc<dyn Fn(Option<&K>)>;
type OnLoaded<K, T> = Rc<dyn Fn(&K, &T)>;

/// How a [`preview_of_with`] pane behaves, and the signal it shares.
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct PreviewOfOptions<K: 'static, T: 'static> {
    state: Option<Signal<T>>,
    clear_after: Option<Duration>,
    on_change: Option<OnChange<K>>,
    on_loaded: Option<OnLoaded<K, T>>,
}

impl<K, T> Default for PreviewOfOptions<K, T> {
    fn default() -> Self {
        PreviewOfOptions {
            state: None,
            clear_after: None,
            on_change: None,
            on_loaded: None,
        }
    }
}

impl<K, T> PreviewOfOptions<K, T> {
    pub fn new() -> PreviewOfOptions<K, T> {
        PreviewOfOptions::default()
    }

    /// Keep what is shown in `state` (to read it elsewhere, or to set it).
    pub fn state(mut self, state: Signal<T>) -> PreviewOfOptions<K, T> {
        self.state = Some(state);
        self
    }

    /// Show the empty value (`T::default()`) when a load takes longer
    /// than `delay` (on the app's clock), so a slow one does not leave the
    /// last key's value on show. Off by default: the last value stays
    /// until the next is ready (or with `None`).
    pub fn clear_after(mut self, delay: impl Into<Option<Duration>>) -> PreviewOfOptions<K, T> {
        self.clear_after = delay.into();
        self
    }

    /// Called on the app's thread whenever the key changes, before
    /// anything loads (`None`: nothing to show): reset a scroll, say.
    pub fn on_change(mut self, change: impl Fn(Option<&K>) + 'static) -> PreviewOfOptions<K, T> {
        self.on_change = Some(Rc::new(change));
        self
    }

    /// Called on the app's thread with a value just loaded for the current
    /// key, before it is shown.
    pub fn on_loaded(mut self, loaded: impl Fn(&K, &T) + 'static) -> PreviewOfOptions<K, T> {
        self.on_loaded = Some(Rc::new(loaded));
        self
    }
}

/// A pane showing a value loaded for a key, with default options: see
/// [`preview_of_with`].
///
/// ```
/// use intuituive::preview_of;
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let id = signal(1u32);
///     preview_of(
///         move |_width| Some(id.get()),
///         |id| format!("record {id}"),
///         |shown| text(move || shown.get()),
///     )
///     .on_key("n", move |_| id.update(|i| *i += 1))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(20, 1);
/// driver.press("n");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "record 2");
/// ```
pub fn preview_of<K, T>(
    key: impl Fn(u16) -> Option<K> + 'static,
    load: impl Fn(&K) -> T + Send + Sync + 'static,
    render: impl FnOnce(Signal<T>) -> Node,
) -> Node
where
    K: Clone + PartialEq + Send + 'static,
    T: Default + PartialEq + Send + 'static,
{
    preview_of_with(key, load, render, PreviewOfOptions::new())
}

/// A pane that previews whatever is current: `key` names it from the
/// pane's width (which may read signals: the hovered row, a setting;
/// `None` for nothing), `load` makes its value on a worker thread
/// whenever the key changes, and `render` draws the value from the signal
/// it is given, made once. Nothing loads before the pane is laid out (the
/// first frame lays it out and, under
/// [`wait_for_tasks`](crate::App::wait_for_tasks), shows the first value
/// already); a result for a key already left behind is dropped, so a slow
/// load never replaces a newer one. With no key the value is
/// `T::default()`. It is a [`resource_for`] keyed by
/// the width and the key, [`stale_after`](crate::Resource::stale_after)
/// the [`clear_after`](PreviewOfOptions::clear_after) wait. The `files`
/// plugin's preview is one: a path to a file's preview, picked by its
/// type.
pub fn preview_of_with<K, T>(
    key: impl Fn(u16) -> Option<K> + 'static,
    load: impl Fn(&K) -> T + Send + Sync + 'static,
    render: impl FnOnce(Signal<T>) -> Node,
    options: PreviewOfOptions<K, T>,
) -> Node
where
    K: Clone + PartialEq + Send + 'static,
    T: Default + PartialEq + Send + 'static,
{
    let state = options.state.unwrap_or_else(|| signal(T::default()));
    let pane = render(state);
    let area = pane.rect_handle();
    let PreviewOfOptions {
        clear_after,
        on_change,
        on_loaded,
        ..
    } = options;
    // The width once laid out, and the key for it.
    let current = memo(move || {
        let width = area.get().map(|r| r.width).filter(|w| *w > 0);
        (width, key(width.unwrap_or(0)))
    });
    watch(
        move || current.with(|(_, key)| key.clone()),
        move |key, _| {
            if let Some(change) = &on_change {
                change(key.as_ref());
            }
            if key.is_none() {
                state.set(T::default());
            }
        },
    );
    // What loads: the key at a width, the key handed back with the value
    // (taken out once, so the value is never cloned).
    let load = Arc::new(load);
    let loaded = resource_for(
        move || current.with(|(width, key)| Some((*width)?).zip(key.clone())),
        move |what: Option<(u16, K)>| {
            let value = what.map(|(_, key)| {
                let value = load(&key);
                (key, Taken(Arc::new(Mutex::new(Some(value)))))
            });
            Ok::<_, std::convert::Infallible>(value)
        },
    );
    if let Some(delay) = clear_after {
        loaded.stale_after(delay);
    }
    on_write_ref(loaded.state, move |load, _| match load {
        Load::Ready(Some((key, value))) => {
            let Some(value) = value.take() else {
                return;
            };
            if let Some(on_loaded) = &on_loaded {
                on_loaded(key, &value);
            }
            state.set(value);
        }
        // A load slower than `clear_after`: the old value goes.
        Load::Loading if clear_after.is_some() => state.set(T::default()),
        _ => {}
    });
    pane
}

/// A loaded value, handed from the worker to the pane once.
struct Taken<T>(Arc<Mutex<Option<T>>>);

impl<T> Taken<T> {
    fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}
