//! A disk-space treemap in the style of
//! [diskonaut](https://github.com/imsnif/diskonaut), rebuilt on intuiTUIve:
//! the current folder's children as tiles sized by the space they take,
//! drawn while the folder is scanned (the scan runs on a thread of its own
//! and the map grows as it goes, with a running total and the last path
//! read), then browsed and pruned: move between tiles, enter a folder, go
//! back up, zoom past the largest items to see the small ones, and delete
//! what is selected.
//!
//! Ported from diskonaut (<https://github.com/imsnif/diskonaut>), by Aram
//! Drevekenin and its contributors, MIT licence. Its look, keys and
//! wording are its; this rebuild reuses none of its code. The squarified
//! layout follows its rules (a tile is at least 8 columns by 3 rows, cells
//! counted 2.5 times as tall as wide, the rest of the items grouped in a
//! block of `x`s), with a simpler stopping rule.
//!
//!     cargo run -p rs-rich-intuituive --example disk [-- DIR] [-a|--apparent-size] [-d|--disable-delete-confirmation]
//!
//! Keys, diskonaut's: the arrows, `hjkl` or Ctrl+F/B/N/P move between
//! tiles, Enter enters the selected folder (the largest one when nothing
//! is selected), Esc goes up, Backspace (or Ctrl+D) deletes the selected
//! item once the scan has finished (after `y`), `+`, `-` and `0` zoom in,
//! out and back, `q` or Ctrl+C asks before quitting.
//!
//! Deleting is real: the item is removed from the disk.
//!
//! On intuiTUIve's components: the layout is `layout::squarify`, moving
//! between tiles `focus::neighbour`, every line that picks the longest
//! form that fits a `fit_text`, the map rebuilt by `keyed`, the small
//! window `App::min_size`. Still the example's own: the tiles turned into
//! one grid of joined lines (`joined`, a treemap node does not collapse
//! its tiles' borders) and the folder tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use intuituive::focus::{neighbour, Direction};
use intuituive::layout::squarify;
use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::screen::Rect;
use intuituive::{fit_text, spawn_stream, ModalOptions};
use rich_ext::listing::walk;

// ---------------------------------------------------------------------
// The folder tree, filled as the scan goes.
// ---------------------------------------------------------------------

/// A folder: its items by name, the bytes under it and how many entries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folder {
    pub items: BTreeMap<String, Item>,
    pub size: u64,
    pub descendants: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    File(u64),
    Folder(Folder),
}

impl Item {
    pub fn size(&self) -> u64 {
        match self {
            Item::File(size) => *size,
            Item::Folder(folder) => folder.size,
        }
    }

    /// The entries this item takes away when it is deleted.
    fn count(&self) -> u64 {
        match self {
            Item::File(_) => 1,
            Item::Folder(folder) => folder.descendants + 1,
        }
    }
}

impl Folder {
    fn add(&mut self, parts: &[String], dir: bool, size: u64) {
        let Some((name, rest)) = parts.split_first() else {
            return;
        };
        self.descendants += 1;
        self.size += size;
        if rest.is_empty() {
            let item = if dir {
                Item::Folder(Folder::default())
            } else {
                Item::File(size)
            };
            // A folder seen again (its entries came first) keeps them.
            if !(dir && matches!(self.items.get(name), Some(Item::Folder(_)))) {
                self.items.insert(name.clone(), item);
            }
            return;
        }
        let entry = self
            .items
            .entry(name.clone())
            .or_insert_with(|| Item::Folder(Folder::default()));
        if let Item::Folder(folder) = entry {
            folder.add(rest, dir, size);
        }
    }

    /// The folder at `parts` under this one.
    pub fn folder(&self, parts: &[String]) -> Option<&Folder> {
        match parts.split_first() {
            None => Some(self),
            Some((name, rest)) => match self.items.get(name)? {
                Item::Folder(folder) => folder.folder(rest),
                Item::File(_) => None,
            },
        }
    }

    /// Take the item at `parts` out, and its size and count from every
    /// folder above it.
    fn remove(&mut self, parts: &[String]) -> Option<Item> {
        let (name, rest) = parts.split_first()?;
        let removed = if rest.is_empty() {
            self.items.remove(name)?
        } else {
            match self.items.get_mut(name)? {
                Item::Folder(folder) => folder.remove(rest)?,
                Item::File(_) => return None,
            }
        };
        self.size -= removed.size();
        self.descendants -= removed.count();
        Some(removed)
    }
}

/// What was scanned under the folder diskonaut was started in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub base: PathBuf,
    pub root: Folder,
    /// Bytes deleted so far.
    pub freed: u64,
    /// Entries whose metadata could not be read.
    pub failed: u64,
}

impl Tree {
    pub fn add(&mut self, found: &Found) {
        match found {
            Found::Entry { path, dir, size } => {
                let parts = parts_of(path);
                self.root.add(&parts, *dir, *size);
            }
            Found::Failed => self.failed += 1,
        }
    }
}

fn parts_of(path: &Path) -> Vec<String> {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect()
}

// ---------------------------------------------------------------------
// The scan.
// ---------------------------------------------------------------------

/// One entry the scan found, its path relative to the base folder; or
/// one it could not read.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    Entry { path: PathBuf, dir: bool, size: u64 },
    Failed,
}

/// What scans: it hands each entry to `emit`, and stops when `emit`
/// returns `false` (the app is closing). A test gives one of its own.
pub type Walker = Box<dyn FnOnce(&mut dyn FnMut(Found) -> bool) + Send>;

/// The size on disk (blocks), or with `apparent` the length, as `du` and
/// its `--apparent-size` count them.
fn size_of(meta: &std::fs::Metadata, apparent: bool) -> u64 {
    #[cfg(unix)]
    if !apparent {
        use std::os::unix::fs::MetadataExt;
        return meta.blocks() * 512;
    }
    let _ = apparent;
    meta.len()
}

/// The disk under `base`, hidden entries included and links not followed.
pub fn disk_walker(base: PathBuf, apparent: bool) -> Walker {
    Box::new(move |emit| {
        for path in walk(&base).hidden(true) {
            let found = match std::fs::symlink_metadata(&path) {
                Ok(meta) => Found::Entry {
                    path: path.strip_prefix(&base).unwrap_or(&path).to_path_buf(),
                    dir: meta.is_dir(),
                    size: if meta.is_dir() {
                        0
                    } else {
                        size_of(&meta, apparent)
                    },
                },
                Err(_) => Found::Failed,
            };
            if !emit(found) {
                break;
            }
        }
    })
}

// ---------------------------------------------------------------------
// The treemap.
// ---------------------------------------------------------------------

/// A tile at least this wide and tall (in cells from one line to the next).
pub const MIN_WIDTH: u16 = 8;
pub const MIN_HEIGHT: u16 = 3;
/// A cell is about this many times as tall as it is wide.
const RATIO: f64 = 2.5;

/// One of the folder's children, as the map shows it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    pub name: String,
    pub size: u64,
    /// A folder's entries; `None` for a file.
    pub descendants: Option<u64>,
    /// Its share of the folder (of what the zoom leaves), in thousandths.
    pub permille: u32,
    /// Where its box is: from its left line at `x` to the next at `x + w`,
    /// its top line at `y` to its bottom at `y + h`, the map's lines shared.
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Tile {
    pub fn is_dir(&self) -> bool {
        self.descendants.is_some()
    }

    /// Its box, from line to line.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

/// The items too small for a tile of their own, sharing one block.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Small {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub count: usize,
}

/// The folder laid out: the tiles, and the small items' block.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Map {
    pub tiles: Vec<Tile>,
    pub small: Option<Small>,
    /// The items the zoom hides, the largest.
    pub zoomed: usize,
}

/// The children of `folder`, largest first (by name among equals), the
/// `zoom` largest left out and the rest's shares taken of what remains.
pub fn children(folder: &Folder, zoom: usize) -> Vec<(String, &Item)> {
    let mut items: Vec<(String, &Item)> = folder
        .items
        .iter()
        .map(|(name, item)| (name.clone(), item))
        .collect();
    items.sort_by(|a, b| b.1.size().cmp(&a.1.size()).then_with(|| a.0.cmp(&b.0)));
    items.drain(..zoom.min(items.len()));
    items
}

/// `items` squarified into a map `w` by `h` (counted from line to line),
/// tiles of the minimum size or more, the rest in the small items' block.
pub fn treemap(items: &[(String, &Item)], w: u16, h: u16, zoomed: usize) -> Map {
    let sizes: Vec<f64> = items.iter().map(|(_, item)| item.size() as f64).collect();
    let total: f64 = sizes.iter().sum();
    // Empty files share the map equally, as diskonaut's do; among larger
    // ones they go with the small items.
    let weights: Vec<f64> = match total > 0.0 {
        true => sizes.iter().map(|s| s.max(f64::MIN_POSITIVE)).collect(),
        false => vec![1.0; items.len()],
    };
    let total: f64 = weights.iter().sum();
    let min = (MIN_WIDTH, MIN_HEIGHT);
    let mut laid = squarify(Rect::new(0, 0, w, h), &weights, min, RATIO);
    // squarify gives up (everything in the rest) when the rest's own tile
    // is narrower than the minimum; then the small items get a strip at
    // the bottom and the others the room above it.
    let room = f64::from(w) * f64::from(h - h.min(MIN_HEIGHT));
    let big = weights
        .iter()
        .take_while(|wt| *wt / total * room >= f64::from(MIN_WIDTH * MIN_HEIGHT))
        .count();
    if laid.tiles.iter().all(Rect::is_empty) && big > 0 && big < weights.len() {
        let above = Rect::new(0, 0, w, h - MIN_HEIGHT);
        laid = squarify(above, &weights[..big], min, RATIO);
        laid.tiles.resize(weights.len(), Rect::default());
        let strip = Rect::new(0, h - MIN_HEIGHT, w, MIN_HEIGHT);
        laid.rest = Some((strip, weights.len() - big));
    }
    let tiles = items
        .iter()
        .zip(&weights)
        .zip(laid.tiles)
        .filter(|(_, r)| !r.is_empty())
        .map(|(((name, item), weight), r)| Tile {
            name: name.clone(),
            size: item.size(),
            descendants: match item {
                Item::Folder(folder) => Some(folder.descendants),
                Item::File(_) => None,
            },
            permille: (weight / total * 1000.0).round() as u32,
            x: r.x,
            y: r.y,
            w: r.width,
            h: r.height,
        })
        .collect();
    let small = laid.rest.map(|(r, count)| Small {
        x: r.x,
        y: r.y,
        w: r.width,
        h: r.height,
        count,
    });
    Map {
        tiles,
        small,
        zoomed,
    }
}

/// diskonaut's moves: to the tile beside this one, on the side asked, that
/// shares the most of its edge; off the edge of the map, nothing is
/// selected; with nothing selected, the first.
pub fn step(tiles: &[Tile], selected: Option<usize>, dir: Direction) -> Option<usize> {
    // Last first: of tiles alike, diskonaut goes to the last.
    let rects: Vec<Rect> = tiles.iter().rev().map(Tile::rect).collect();
    match selected.and_then(|i| tiles.get(i)) {
        Some(at) => neighbour(&rects, at.rect(), dir).map(|i| tiles.len() - 1 - i),
        None => (!rects.is_empty()).then_some(0),
    }
}

// ---------------------------------------------------------------------
// What the screen says.
// ---------------------------------------------------------------------

/// diskonaut's sizes: 1000 and up in K, then M and G (of 1024s).
pub fn display_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b > 999_999_999.0 {
        format!("{:.1}G", b / 1_073_741_824.0)
    } else if b > 999_999.0 {
        format!("{:.1}M", b / 1_048_576.0)
    } else if b > 999.0 {
        format!("{:.1}K", b / 1024.0)
    } else {
        format!("{bytes}")
    }
}

fn display_size_rounded(bytes: u64) -> String {
    let b = bytes as f64;
    if b > 999_999_999.0 {
        format!("{:.0}G", b / 1_073_741_824.0)
    } else if b > 999_999.0 {
        format!("{:.0}M", b / 1_048_576.0)
    } else if b > 999.0 {
        format!("{:.0}K", b / 1024.0)
    } else {
        format!("{bytes}")
    }
}

/// A tile's first line, longest first: the name (a folder's with `/` and
/// its count while there is room), the last cut in the middle.
fn tile_name(tile: &Tile) -> Vec<String> {
    match tile.descendants {
        None => vec![tile.name.clone()],
        Some(count) => {
            let name = format!("{}/", tile.name);
            vec![
                format!("{name} (+{count} descendants)"),
                format!("{name} (+{count})"),
                name,
            ]
        }
    }
}

/// A tile's second line, longest first: the size and share, or as much
/// as fits.
fn tile_size(tile: &Tile) -> Vec<String> {
    let percent = (f64::from(tile.permille) / 10.0).round();
    vec![
        format!("{} ({percent:.0}%)", display_size(tile.size)),
        display_size(tile.size),
        display_size_rounded(tile.size),
        format!("({percent:.0}%)"),
        format!("{percent:.0}%"),
    ]
}

// ---------------------------------------------------------------------
// The app's state.
// ---------------------------------------------------------------------

/// Everything the handlers share; `Copy`, so each takes a copy.
#[derive(Clone, Copy)]
struct Disk {
    tree: Signal<Tree>,
    /// The folder shown, as names under the base.
    cwd: Signal<Vec<String>>,
    zoom: Signal<usize>,
    selected: Signal<Option<usize>>,
    /// Where each folder entered was left: its selection and zoom.
    back: Signal<Vec<(Option<usize>, usize)>>,
    loading: Signal<bool>,
    /// The scanning indicator's step.
    tick: Signal<u64>,
    last_read: Signal<String>,
    /// The total lit for a moment after a delete; the path after Esc at
    /// the top.
    flash: Signal<bool>,
    path_red: Signal<bool>,
    deleting: Signal<bool>,
    map: Memo<Map>,
    confirm: bool,
    root_user: bool,
}

/// The rows round the map: the title above, the status and keys below.
const CHROME: u16 = 3;

impl Disk {
    fn selected_tile(&self) -> Option<Tile> {
        let selected = self.selected.get_untracked()?;
        self.map.with(|m| m.tiles.get(selected).cloned())
    }

    fn go(&self, dir: Direction) {
        let next = self
            .map
            .with(|m| step(&m.tiles, self.selected.get_untracked(), dir));
        self.selected.set(next);
    }

    fn enter(&self) {
        if self.selected.get_untracked().is_none() {
            let largest = self.map.with(|m| m.tiles.iter().position(Tile::is_dir));
            self.selected.set(largest);
        }
        let Some(tile) = self.selected_tile().filter(Tile::is_dir) else {
            return;
        };
        let (selected, zoom) = (self.selected.get_untracked(), self.zoom.get_untracked());
        self.back.update(|b| b.push((selected, zoom)));
        self.cwd.update(|c| c.push(tile.name));
        self.zoom.set(0);
        self.selected.set(None);
    }

    fn up(&self) {
        if self.cwd.with_untracked(Vec::is_empty) {
            self.path_red.set(true);
            let red = self.path_red;
            after(Duration::from_millis(250), move |_| red.set(false));
            return;
        }
        self.cwd.update(|c| {
            c.pop();
        });
        let (selected, zoom) = self
            .back
            .with_untracked(|b| b.last().copied())
            .unwrap_or((None, 0));
        self.back.update(|b| {
            b.pop();
        });
        self.zoom.set(zoom);
        self.selected.set(selected);
    }

    fn zoom_by(&self, by: i32) {
        let count = self
            .map
            .with(|m| m.tiles.len() + m.small.as_ref().map_or(0, |s| s.count));
        let zoom = self.zoom.get_untracked();
        match by {
            1 if count > 0 => self.zoom.set(zoom + 1),
            -1 if zoom > 0 => self.zoom.set(zoom - 1),
            0 => self.zoom.set(0),
            _ => {}
        }
    }

    /// The selected item's path on disk, and under the base.
    fn target(&self, tile: &Tile) -> (PathBuf, Vec<String>) {
        let mut parts = self.cwd.get_untracked();
        parts.push(tile.name.clone());
        let mut path = self.tree.with_untracked(|t| t.base.clone());
        path.extend(&parts);
        (path, parts)
    }

    fn delete(&self, cx: &mut Ctx) {
        if self.loading.get_untracked() {
            warning(cx);
            return;
        }
        let Some(tile) = self.selected_tile() else {
            return;
        };
        if self.confirm {
            let d = *self;
            cx.modal_with(
                Size::Percent(50),
                Size::Fixed(10),
                ModalOptions::new(),
                move || delete_box(d, tile),
            );
        } else {
            self.remove(cx, tile);
        }
    }

    /// Delete `tile`'s item from the disk on a thread, then from the map.
    fn remove(&self, _cx: &mut Ctx, tile: Tile) {
        let (path, parts) = self.target(&tile);
        let d = *self;
        d.deleting.set(true);
        spawn(
            move || {
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                removed.map_err(|e| e.to_string())
            },
            move |result, cx| {
                d.deleting.set(false);
                if d.confirm {
                    cx.pop();
                }
                match result {
                    Ok(()) => {
                        d.tree.update(|t| {
                            if let Some(item) = t.root.remove(&parts) {
                                t.freed += item.size();
                            }
                        });
                        d.selected.set(None);
                        d.flash.set(true);
                        let flash = d.flash;
                        cx.after(Duration::from_millis(250), move |_| flash.set(false));
                    }
                    Err(message) => error_box(cx, message),
                }
            },
        );
    }
}

// ---------------------------------------------------------------------
// The screen.
// ---------------------------------------------------------------------

/// `forms` (plain text, longest first) as one line: the longest that
/// fits, in `style`, centred, the last cut in the middle.
fn fitted(forms: Vec<String>, style: &str) -> Node {
    let forms: Vec<String> = forms
        .iter()
        .map(|f| match style {
            "" => escape(f),
            style => format!("[{style}]{}[/]", escape(f)),
        })
        .collect();
    fit_text(move || forms.clone())
        .truncate_middle()
        .text_align(TextAlign::Center)
        .fixed(1)
}

/// A tile's box: its name and size centred, filled when selected.
fn tile_node(tile: &Tile, selected: bool) -> Node {
    let (fill, first, second) = match (selected, tile.is_dir()) {
        (true, true) => ("on blue", "bold white", "black"),
        (true, false) => ("on white", "bold magenta", "bold magenta"),
        (false, true) => ("", "bold blue", ""),
        (false, false) => ("", "", ""),
    };
    let inside = tile.h.saturating_sub(1);
    let mut lines = vec![fitted(tile_name(tile), first)];
    if inside >= 4 {
        lines.push(label("").fixed(1));
    }
    if inside >= 2 {
        lines.push(fitted(tile_size(tile), second));
    }
    let content = column(lines).justify(Justify::Center);
    let content = if fill.is_empty() {
        content
    } else {
        content.style(fill)
    };
    content.panel("").border(Border::Plain)
}

/// Boxes at rectangles that tile an area (counted from line to line) as
/// one grid of joined lines: a track between each two lines a box has.
fn joined(mut boxes: Vec<(Rect, Node)>) -> Node {
    let lines = |ends: Vec<u16>| {
        let mut ends = ends;
        ends.sort_unstable();
        ends.dedup();
        ends
    };
    let xs = lines(boxes.iter().flat_map(|(r, _)| [r.x, r.right()]).collect());
    let ys = lines(boxes.iter().flat_map(|(r, _)| [r.y, r.bottom()]).collect());
    // A collapsed track's size counts the line it shares with the next.
    let tracks = |at: &[u16]| -> Vec<Size> {
        at.windows(2)
            .map(|p| Size::Fixed(p[1] - p[0] + 1))
            .collect()
    };
    let span = |at: &[u16], from: u16, to: u16| {
        let index = |v: u16| at.binary_search(&v).unwrap_or(0);
        (index(to) - index(from)) as u16
    };
    // In reading order, each box lands where its top left corner is.
    boxes.sort_by_key(|(r, _)| (r.y, r.x));
    let nodes: Vec<Node> = boxes
        .into_iter()
        .map(|(r, node)| node.span(span(&xs, r.x, r.right()), span(&ys, r.y, r.bottom())))
        .collect();
    grid(tracks(&xs), nodes)
        .rows(tracks(&ys))
        .collapse_borders()
}

/// The map, one frame of joined lines.
fn board(map: &Map, selected: Option<usize>) -> Node {
    let mut boxes: Vec<(Rect, Node)> = map
        .tiles
        .iter()
        .enumerate()
        .map(|(i, tile)| (tile.rect(), tile_node(tile, selected == Some(i))))
        .collect();
    if let Some(s) = &map.small {
        let block = fill("x", "black on white").panel("").border(Border::Plain);
        boxes.push((Rect::new(s.x, s.y, s.w, s.h), block));
    }
    let inside = if boxes.is_empty() {
        layers([
            fill(" ", "on white"),
            label("Folder is empty").place(Align::Center, Align::Center),
        ])
        .panel("")
        .border(Border::Plain)
    } else {
        joined(boxes)
    };
    column([inside])
        .collapse_borders()
        .border(Border::Plain)
        .border_style("default")
}

/// diskonaut's title: the total and what was freed, then the path, each
/// part as long as the width allows; its forms, longest first.
fn title(d: Disk) -> Vec<String> {
    let loading = d.loading.get();
    let (total, files, freed, failed, current) = d.tree.with(|t| {
        let cwd = d.cwd.get();
        let current = t
            .root
            .folder(&cwd)
            .map(|f| (f.size, f.descendants))
            .unwrap_or_default();
        (t.root.size, t.root.descendants, t.freed, t.failed, current)
    });
    let (total, freed) = (display_size(total), display_size(freed));
    let bold = if loading { "" } else { " bold" };
    let base_style = format!("yellow{bold}");
    let left_style = if d.flash.get() {
        format!("black on yellow{bold}")
    } else {
        base_style.clone()
    };
    let right_style = if d.path_red.get() {
        format!("white on red{bold}")
    } else {
        "green".to_string() + bold
    };
    // Each part: its versions, longest first, and its style.
    let mut left: Vec<(Vec<String>, String)> = vec![(
        if loading {
            vec![
                format!("Scanning: {total} ({files} files)"),
                format!("Scanning: {total}"),
                total.clone(),
            ]
        } else {
            vec![
                format!("Total: {total} ({files} files), freed: {freed}"),
                format!("Total: {total}, freed: {freed}"),
                format!("Total: {total}"),
                total.clone(),
            ]
        },
        left_style,
    )];
    if failed > 0 {
        left.push((
            vec![
                format!(" (failed to read {failed} files)"),
                format!(" ({failed} errors)"),
                " (errors)".into(),
            ],
            format!("red{bold}"),
        ));
    }
    if d.root_user {
        left.push((
            vec![
                " (CAUTION: running as root)".into(),
                " (running as root)".into(),
                " (root)".into(),
            ],
            format!("red{bold}"),
        ));
    }
    let base = d.tree.with(|t| t.base.display().to_string());
    let base_right = if d.path_red.get() {
        right_style.clone()
    } else {
        base_style.clone()
    };
    let mut right: Vec<(Vec<String>, String)> = vec![(vec![base.clone()], base_right)];
    let cwd = d.cwd.get();
    if !cwd.is_empty() {
        let sep = if base.ends_with(std::path::MAIN_SEPARATOR) {
            ""
        } else {
            std::path::MAIN_SEPARATOR_STR
        };
        let rel = cwd.join(std::path::MAIN_SEPARATOR_STR);
        let size = display_size(current.0);
        right.push((
            vec![
                format!("{sep}{rel} ({size}, {} files)", current.1),
                format!("{sep}{rel} ({size})"),
                format!("{sep}{rel}"),
            ],
            right_style.clone(),
        ));
    }
    let zoomed = d.zoom.get();
    if zoomed > 0 {
        right.push((
            vec![
                format!(" (+{zoomed} larger file(s), zoom out to show)"),
                format!(" (+{zoomed} larger file(s))"),
            ],
            right_style,
        ));
    }
    let depth = left
        .iter()
        .chain(&right)
        .map(|p| p.0.len())
        .max()
        .unwrap_or(1);
    let pick = |parts: &[(Vec<String>, String)], i: usize| -> Vec<(String, String)> {
        parts
            .iter()
            .map(|(v, s)| {
                (
                    v.get(i).unwrap_or_else(|| v.last().unwrap()).clone(),
                    s.clone(),
                )
            })
            .collect()
    };
    let sep = (" | ".to_string(), format!("white{bold}"));
    let tick = d.tick.get() as usize;
    (0..depth)
        .map(|i| {
            let mut line = pick(&left, i);
            line.push(sep.clone());
            line.extend(pick(&right, i));
            // While scanning, a few letters in bold run along the title.
            let len: usize = line.iter().map(|(t, _)| t.chars().count()).sum();
            let lit = if loading && len > 0 {
                let at = tick % len;
                at..at + 4
            } else {
                0..0
            };
            let mut markup = String::from(" ");
            let chars = line
                .iter()
                .flat_map(|(text, style)| text.chars().map(move |ch| (ch, style)));
            for (n, (ch, style)) in chars.enumerate() {
                let bold = if lit.contains(&n) { " bold" } else { "" };
                markup.push_str(&format!("[{style}{bold}]{}[/]", escape(&ch.to_string())));
            }
            markup
        })
        .collect()
}

/// The status line's left: what is selected (or, while scanning, the path
/// read last); its forms, longest first.
fn selection(d: Disk) -> Vec<String> {
    let tile = d
        .selected
        .get()
        .and_then(|i| d.map.with(|m| m.tiles.get(i).cloned()));
    let Some(tile) = tile else {
        return match d.loading.get() {
            true => vec![escape(&d.last_read.get())],
            false => vec![String::new()],
        };
    };
    let size = display_size(tile.size);
    let (variants, style) = match tile.descendants {
        Some(count) => (
            vec![
                format!("SELECTED: {} ({size}, {count} files)", tile.name),
                format!("SELECTED: {} ({size})", tile.name),
                format!("SELECTED: {}", tile.name),
                tile.name.clone(),
            ],
            "bold blue",
        ),
        None => (
            vec![
                format!("SELECTED: {} ({size})", tile.name),
                format!("SELECTED: {}", tile.name),
                tile.name.clone(),
            ],
            "bold",
        ),
    };
    variants
        .iter()
        .map(|v| format!("[{style}]{}[/]", escape(v)))
        .collect()
}

/// The status line, with the small items' legend at the right.
fn status(d: Disk) -> Node {
    let map = d.map;
    row([
        fit_text(move || selection(d)).truncate_middle(),
        label("([black on white]x[/] = Small files)")
            .auto()
            .shown_when(move || map.with(|m| m.small.is_some())),
    ])
    .padding(0, 1)
}

/// The keys, as many as fit; their forms, longest first.
fn controls(loading: bool) -> Vec<String> {
    let (long, short) = if loading {
        (
            "<arrows> - move around, <ENTER> - enter folder, <ESC> - parent folder, <+/-/0> - zoom in/out/reset, <q> - quit",
            "←↓↑→/<ENTER>/<ESC>: navigate",
        )
    } else {
        (
            "<arrows> - move around, <ENTER> - enter folder, <ESC> - parent folder, <BACKSPACE> - delete, <+/-/0> - zoom in/out/reset, <q> - quit",
            "←↓↑→/<ENTER>/<ESC>: navigate, <BACKSPACE>: del",
        )
    };
    [long, short, "(...)"]
        .iter()
        .map(|line| format!(" [bold]{}[/]", escape(line)))
        .collect()
}

/// A line of a box: the longest of `forms` that fits, centred, the last
/// cut at its end.
fn say(forms: &[impl AsRef<str>]) -> Node {
    let forms: Vec<String> = forms.iter().map(|f| escape(f.as_ref())).collect();
    fit_text(move || forms.clone())
        .text_align(TextAlign::Center)
        .fixed(1)
}

/// A box over the map, diskonaut's: `lines` centred, in `style`.
fn message(lines: Vec<Node>, style: &str) -> Node {
    column(lines)
        .gap(2)
        .justify(Justify::Center)
        .panel("")
        .border(Border::Plain)
        .style(style)
}

fn delete_box(d: Disk, tile: Tile) -> Node {
    let (path, _) = d.target(&tile);
    let deleting = d.deleting;
    let file_line = vec![path.display().to_string(), tile.name.clone()];
    let question = match tile.descendants {
        None => vec!["Delete this file?".to_string()],
        Some(count) => vec![
            format!("Delete folder with {count} children?"),
            "Delete folder?".into(),
        ],
    };
    switch(
        move || deleting.get(),
        move |now| {
            // The path whole, or the name cut in the middle.
            let file = fitted(file_line.clone(), "");
            let lines = if now {
                vec![say(&["Deleting"]), file]
            } else {
                vec![say(&question), file, say(&["(y/n)"])]
            };
            message(lines, "bold red on black")
        },
    )
    .on_key("y", move |cx| {
        if !d.deleting.get_untracked() {
            d.remove(cx, tile.clone());
        }
    })
    .on_key("n esc q ctrl+c", |cx| cx.pop())
}

fn warning(cx: &mut Ctx) {
    cx.modal_with(
        Size::Percent(50),
        Size::Fixed(10),
        ModalOptions::new(),
        || {
            message(
                vec![
                    say(&[
                        "Sorry, deletion is only allowed once the scanning has completed",
                        "Deletion is not allowed while scanning",
                        "Can't delete while scanning",
                        "Can't delete now",
                    ]),
                    say(&["(Press any key to dismiss)"]),
                ],
                "bold yellow on black",
            )
            .on_any_key(|cx, _| {
                cx.pop();
                true
            })
        },
    );
}

fn error_box(cx: &mut Ctx, error: String) {
    cx.modal_with(
        Size::Percent(50),
        Size::Fixed(10),
        ModalOptions::new(),
        move || {
            message(
                vec![
                    say(&[format!("Error: {error}")]),
                    say(&["(Press <ESC> to dismiss)"]),
                ],
                "bold red on black",
            )
            .on_key("esc q ctrl+c", |cx| cx.pop())
        },
    );
}

fn quit_box(cx: &mut Ctx) {
    cx.modal_with(
        Size::Percent(50),
        Size::Fixed(10),
        ModalOptions::new(),
        || {
            message(
                vec![say(&["Are you sure you want to quit?"]), say(&["(y/n)"])],
                "bold white on black",
            )
            .on_key("y", |cx| cx.quit())
            .on_key("n esc q ctrl+c", |cx| cx.pop())
        },
    );
}

fn main_screen(d: Disk) -> Node {
    let (map, selected) = (d.map, d.selected);
    column([
        fit_text(move || title(d)).fixed(1),
        // The map, built again whenever it or the selection changes.
        keyed(
            move || (map.get(), selected.get()),
            |(map, selected)| board(&map, selected),
        ),
        status(d).fixed(1),
        fit_text(move || controls(d.loading.get())).fixed(1),
    ])
}

// ---------------------------------------------------------------------
// The app.
// ---------------------------------------------------------------------

pub struct Options {
    pub base: PathBuf,
    /// Count files by their length, not the blocks they take.
    pub apparent: bool,
    /// Delete without asking.
    pub no_confirm: bool,
    /// The scan; the disk under `base` when `None`.
    pub walker: Option<Walker>,
    /// Say so in the title when running as root.
    pub root_user: bool,
}

impl Options {
    pub fn new(base: impl Into<PathBuf>) -> Options {
        Options {
            base: base.into(),
            apparent: false,
            no_confirm: false,
            walker: None,
            root_user: running_as_root(),
        }
    }

    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::new(std::env::current_dir().map_err(|e| e.to_string())?);
        for arg in args {
            match arg.as_str() {
                "-a" | "--apparent-size" => options.apparent = true,
                "-d" | "--disable-delete-confirmation" => options.no_confirm = true,
                flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
                path => {
                    let path = PathBuf::from(path);
                    if !path.is_dir() {
                        return Err(format!("{} is not a folder", path.display()));
                    }
                    options.base = path.canonicalize().map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(options)
    }
}

fn running_as_root() -> bool {
    std::fs::read_to_string("/proc/self/status").is_ok_and(|s| {
        s.lines()
            .find(|l| l.starts_with("Uid:"))
            .and_then(|l| l.split_whitespace().nth(1))
            == Some("0")
    })
}

/// Run `walker` on a thread, sending what it finds to the tree in
/// batches (at most every 50 ms), then end the scan.
fn scan(d: Disk, walker: Walker) {
    let (tree, last_read, loading) = (d.tree, d.last_read, d.loading);
    spawn_stream(move |stream| {
        let proxy = stream.proxy().clone();
        let mut batch: Vec<Found> = Vec::new();
        let mut since = Instant::now();
        let flush = |batch: Vec<Found>| {
            let last = batch.iter().rev().find_map(|f| match f {
                Found::Entry { path, .. } => Some(path.display().to_string()),
                Found::Failed => None,
            });
            proxy.run(move || {
                tree.update(|t| batch.iter().for_each(|f| t.add(f)));
                if let Some(last) = last {
                    last_read.set(last);
                }
            });
        };
        walker(&mut |found| {
            if stream.cancelled() {
                return false;
            }
            batch.push(found);
            if batch.len() >= 1000 || since.elapsed() >= Duration::from_millis(50) {
                flush(std::mem::take(&mut batch));
                since = Instant::now();
            }
            true
        });
        flush(batch);
        proxy.run(move || loading.set(false));
    });
}

pub fn disk_app(options: Options) -> App {
    App::new(move || {
        let tree = signal(Tree {
            base: options.base.clone(),
            ..Tree::default()
        });
        let cwd = signal(Vec::<String>::new());
        let zoom = signal(0usize);
        let size = viewport();
        let map = memo(move || {
            let (w, h) = size.get();
            let zoom = zoom.get();
            let cwd = cwd.get();
            tree.with(|t| {
                let folder = t.root.folder(&cwd).unwrap_or(&t.root);
                let items = children(folder, zoom);
                let rows = h.saturating_sub(CHROME);
                treemap(&items, w.saturating_sub(1), rows.saturating_sub(1), zoom)
            })
        });
        let d = Disk {
            tree,
            cwd,
            zoom,
            selected: signal(None),
            back: signal(Vec::new()),
            loading: signal(true),
            tick: signal(0),
            last_read: signal(String::new()),
            flash: signal(false),
            path_red: signal(false),
            deleting: signal(false),
            map,
            confirm: !options.no_confirm,
            root_user: options.root_user,
        };
        let walker = options
            .walker
            .unwrap_or_else(|| disk_walker(options.base.clone(), options.apparent));
        scan(d, walker);
        every(Duration::from_millis(100), move |_| {
            if d.loading.get_untracked() {
                d.tick.update(|t| *t += 1);
            }
        });
        main_screen(d)
            .on_key("right l ctrl+f", move |_| d.go(Direction::Right))
            .on_key("left h ctrl+b", move |_| d.go(Direction::Left))
            .on_key("down j ctrl+n", move |_| d.go(Direction::Down))
            .on_key("up k ctrl+p", move |_| d.go(Direction::Up))
            .on_key("+", move |_| d.zoom_by(1))
            .on_key("-", move |_| d.zoom_by(-1))
            .on_key("0", move |_| d.zoom_by(0))
            .on_key("enter", move |_| d.enter())
            .on_key("esc", move |_| d.up())
            .on_key("backspace ctrl+d", move |cx| d.delete(cx))
            .on_key("q ctrl+c", quit_box)
    })
    // Under 50 x 15, diskonaut's message; only `q` and Ctrl+C work there.
    .min_size(
        50,
        15,
        format!(
            "[bold]{}[/]",
            escape("Terminal window is too small ¯\\_(ツ)_/¯")
        ),
    )
}

fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    disk_app(options).run()
}
