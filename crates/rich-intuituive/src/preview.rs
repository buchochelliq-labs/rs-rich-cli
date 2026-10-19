//! A pane that loads a value off the app's thread for whatever key is
//! current, drops the results for keys left behind, and draws the value.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::node::Node;
use crate::reactive::{signal, watch, Signal};
use crate::task::{spawn, spawn_then};

type OnChange<K> = Rc<dyn Fn(Option<&K>)>;
type OnLoaded<K, T> = Rc<dyn Fn(&K, &T)>;

/// How a [`preview_of_with`] pane behaves, and the signal it shares.
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
    /// than `delay`, so a slow one does not leave the last key's value on
    /// show. Off by default: the last value stays until the next is ready.
    pub fn clear_after(mut self, delay: Duration) -> PreviewOfOptions<K, T> {
        self.clear_after = Some(delay);
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
/// it is given, made once. Nothing loads before the pane is laid out; a
/// result for a key already left behind is dropped, so a slow load never
/// replaces a newer one. With no key the value is `T::default()`. The
/// `files` plugin's preview is one: a path to a file's preview, picked by
/// its type.
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
    let generation = signal(0u64);
    let load = Arc::new(load);
    let PreviewOfOptions {
        clear_after,
        on_change,
        on_loaded,
        ..
    } = options;
    watch(
        move || {
            let width = area.get().map(|r| r.width);
            (width, key(width.unwrap_or(0)))
        },
        move |(width, key), _| {
            generation.update(|g| *g += 1);
            let mine = generation.get_untracked();
            if let Some(change) = &on_change {
                change(key.as_ref());
            }
            let Some(key) = key else {
                state.set(T::default());
                return;
            };
            // Not laid out yet: the width comes with the first frame.
            if width.is_none_or(|w| w == 0) {
                return;
            }
            if let Some(delay) = clear_after {
                spawn(
                    move || std::thread::sleep(delay),
                    move |_, _| {
                        if generation.get_untracked() == mine {
                            state.set(T::default());
                        }
                    },
                );
            }
            let load = load.clone();
            let loading = key.clone();
            let on_loaded = on_loaded.clone();
            spawn_then(
                move || load(&loading),
                move |loaded, _| {
                    if generation.get_untracked() != mine {
                        return;
                    }
                    // Done: a pending clear has nothing left to do.
                    generation.update(|g| *g += 1);
                    if let Some(on_loaded) = &on_loaded {
                        on_loaded(&key, &loaded);
                    }
                    state.set(loaded);
                },
            );
        },
    );
    pane
}
