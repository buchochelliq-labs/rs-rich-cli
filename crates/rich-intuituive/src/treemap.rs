//! A treemap node: a tile per item, its area in proportion to its weight
//! ([`layout::squarify`](crate::layout::squarify)), each a node built for
//! its key and its rectangle.

use std::hash::Hash;
use std::rc::Rc;

use crate::layout::squarify;
use crate::node::Node;
use crate::reactive::Runtime;
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget};

/// How a [`treemap_with`] lays its tiles out: the smallest tile, a cell's
/// shape, and the node for the items too small for a tile of their own.
#[derive(Clone)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct TreemapOptions {
    pub(crate) min: (u16, u16),
    pub(crate) aspect: f64,
    pub(crate) rest: Option<Rc<dyn Fn(usize) -> Node>>,
}

impl Default for TreemapOptions {
    fn default() -> TreemapOptions {
        TreemapOptions::new()
    }
}

impl std::fmt::Debug for TreemapOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreemapOptions")
            .field("min", &self.min)
            .field("aspect", &self.aspect)
            .field("rest", &self.rest.is_some())
            .finish()
    }
}

impl TreemapOptions {
    /// Tiles at least 1 x 1, cells twice as tall as they are wide, and the
    /// rest left blank.
    pub fn new() -> TreemapOptions {
        TreemapOptions {
            min: (1, 1),
            aspect: 2.0,
            rest: None,
        }
    }

    /// No tile narrower than `columns` or shorter than `rows`: the smallest
    /// items that would make one share a tile for the rest.
    pub fn min_tile(mut self, columns: u16, rows: u16) -> TreemapOptions {
        self.min = (columns, rows);
        self
    }

    /// A cell's height over its width, so tiles look square.
    pub fn aspect(mut self, aspect: f64) -> TreemapOptions {
        self.aspect = aspect;
        self
    }

    /// The node in the tile the items too small for their own share,
    /// given how many they are (`+ 12 more`).
    pub fn rest(mut self, build: impl Fn(usize) -> Node + 'static) -> TreemapOptions {
        self.rest = Some(Rc::new(build));
        self
    }
}

/// A treemap of `items`, each `(key, weight)`: a tile per item, its area
/// in proportion to its weight, as near square as the weights let it be,
/// each holding the node `build` makes for its key and its rectangle (on
/// the screen). `items` may read signals: the map lays out again when they
/// change. A tile keeps its node while its key and size stay; a new key,
/// or a tile of another size, is built afresh (what it shows may depend
/// on its room), and the old one is dropped. What takes the room: a disk
/// by folder, memory by process, a bundle by module. Tiles that take the
/// focus move between each other by direction with
/// [`bind_directional_focus`](Node::bind_directional_focus).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::treemap;
///
/// let app = App::new(|| {
///     treemap(
///         || vec![("big", 3.0), ("small", 1.0)],
///         |name, rect| label(format!("{name} {}", rect.width)).panel(""),
///     )
/// });
/// let mut driver = app.driver(24, 4);
/// driver.settle();
/// assert!(driver.screen().plain()[1].starts_with("│big 18"));
/// ```
pub fn treemap<K, F>(items: impl Fn() -> Vec<(K, f64)> + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K, Rect) -> Node + 'static,
{
    treemap_with(items, build, TreemapOptions::new())
}

/// [`treemap`] laid out as `options` say: the smallest tile, a cell's
/// shape, and a tile for the rest.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{treemap_with, TreemapOptions};
///
/// let app = App::new(|| {
///     treemap_with(
///         || vec![("a", 6.0), ("b", 3.0), ("c", 0.4), ("d", 0.4)],
///         |name, _| label(name),
///         TreemapOptions::new().min_tile(8, 3).rest(|n| label(format!("+{n}"))),
///     )
/// });
/// let mut driver = app.driver(20, 10);
/// driver.settle();
/// assert!(driver.screen().plain().join("\n").contains("+2"));
/// ```
pub fn treemap_with<K, F>(
    items: impl Fn() -> Vec<(K, f64)> + 'static,
    build: F,
    options: TreemapOptions,
) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K, Rect) -> Node + 'static,
{
    Node::from_widget(
        Box::new(Treemap {
            items: Box::new(items),
            build,
            options,
            keys: Vec::new(),
            children: Vec::new(),
            areas: Vec::new(),
        }),
        "treemap",
    )
}

/// What a tile holds: an item's key, or the rest's count; and its size.
#[derive(Clone, PartialEq, Eq)]
enum Tile<K> {
    Item(K, (u16, u16)),
    Rest(usize, (u16, u16)),
}

/// What [`treemap`] makes.
struct Treemap<K, F> {
    items: Box<dyn Fn() -> Vec<(K, f64)>>,
    build: F,
    options: TreemapOptions,
    keys: Vec<Tile<K>>,
    children: Vec<Node>,
    areas: Vec<Rect>,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K, Rect) -> Node> Treemap<K, F> {
    /// The tiles for `rect` now, their nodes kept or built.
    fn reconcile(&mut self, rect: Rect) {
        let items = (self.items)();
        let weights: Vec<f64> = items.iter().map(|(_, w)| *w).collect();
        let map = squarify(rect, &weights, self.options.min, self.options.aspect);
        let mut wanted: Vec<(Tile<K>, Rect)> = items
            .into_iter()
            .zip(map.tiles)
            .filter(|(_, r)| !r.is_empty())
            .map(|((key, _), r)| (Tile::Item(key, (r.width, r.height)), r))
            .collect();
        if let (Some((r, n)), Some(_)) = (map.rest, &self.options.rest) {
            wanted.push((Tile::Rest(n, (r.width, r.height)), r));
        }
        let runtime = Runtime::current();
        let mut old: Vec<(Tile<K>, Node)> =
            self.keys.drain(..).zip(self.children.drain(..)).collect();
        self.areas.clear();
        for (tile, r) in wanted {
            let node = match old.iter().position(|(t, _)| *t == tile) {
                Some(at) => old.swap_remove(at).1,
                // Built untracked: what a tile reads is its own.
                None => runtime.untracked(|| match &tile {
                    Tile::Item(key, _) => (self.build)(key.clone(), r),
                    Tile::Rest(n, _) => (self.options.rest.as_ref().expect("a rest builder"))(*n),
                }),
            };
            self.keys.push(tile);
            self.children.push(node);
            self.areas.push(r);
        }
        for (_, gone) in old {
            gone.forget(&runtime);
        }
    }
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K, Rect) -> Node + 'static> Widget for Treemap<K, F> {
    fn name(&self) -> &'static str {
        "treemap"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("[{}]", self.children.len()))
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.reconcile(rect);
        self.areas.clone()
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}
