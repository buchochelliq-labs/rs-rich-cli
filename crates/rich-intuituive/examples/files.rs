//! A file manager in the style of [Yazi](https://github.com/sxyazi/yazi),
//! the most-starred app built on ratatui, rebuilt on intuiTUIve from its
//! ready-made components: the `files` plugin's Miller columns (the parent
//! directory, the current one and a preview of the hovered entry, loaded
//! in the background), its openers, file jobs, finder and git signs, and
//! the core's status bar, breadcrumb, tabs, prompts, picks and confirms.
//!
//! Ported from Yazi (<https://github.com/sxyazi/yazi>), by sxyazi and its
//! contributors, MIT licence. Its layout, keys, theme and behaviour are
//! theirs; this rebuild reuses none of its code. The icons and type colours
//! are Yazi's defaults, from rs-rich-ext's `filetype` (whose glyphs come
//! from nvim-web-devicons, MIT).
//!
//!     cargo run -p rs-rich-intuituive --features files --example files [-- DIR] [--icons|--no-icons]
//!     cargo run -p rs-rich-intuituive --features files,archive,watch,art --example files
//!
//! It needs the crate's `files` feature. Three more add to it: `archive`
//! previews and extracts tar and zip archives, `watch` follows changes on
//! disk as Yazi does (without it the view changes only with what the app
//! itself does), and `art` draws images in the preview (without it an
//! image is described).
//!
//! Icons need a Nerd Font: they are on with `--icons`, or when
//! `RICH_NERD_FONT=1` says one is in use; without them the rows and the
//! status line are plain text, as Yazi with its icons turned off.
//! `--backend NAME` (or `INTUITUIVE_BACKEND=NAME`) drives the terminal with
//! termion or termwiz instead of crossterm, when built with the crate's
//! feature of that name.
//!
//! Yazi's default keys: j/k move, h/l leave and enter, gg/G top and
//! bottom, H/L back and forward, Enter or o opens with the first opener
//! for the type, O asks which, Space selects, v visual mode, y/x yank and
//! cut, p paste, d trash, D delete, a create, r rename, / and ? find, n/N
//! the next match, f filter, . hidden files, the `,` keys sort, `m` sets
//! the line mode, `c` copies paths, `g` goes places, z finds a file, Z
//! jumps to a directory visited, Tab shows a file's details, w the tasks,
//! `t t` opens a tab and 1-9, [ and ] switch, ~ or F1 shows the help, q
//! quits. After the first key of a sequence (g , c m t) a which-key pop-up
//! shows what can follow. Ctrl+P finds any command (not in Yazi).
//!
//! What changes files is real, and asked first: trashing moves to the
//! system's trash (the FreeDesktop trash; on macOS and Windows, whose bins
//! it does not write, it says so), deleting and overwriting say what they
//! will do and wait for Y. Each runs as a background task (`w`).
//!
//! The mouse: a click hovers an entry (in the parent column it goes there,
//! in a directory's preview it enters it), the wheel moves the cursor, a
//! right click opens a menu, the column rails drag and a click on a tab
//! switches to it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use intuituive::files::{
    coloured_permissions, finder, git_status, miller, open_with, opener_rules, tasks, tasks_panel,
    JobState, Linemode, Marker, Miller, MillerOptions, Sort, SortBy, Tasks,
};
use intuituive::interact::components::{Jump, JumpSource};
use intuituive::interact::{BackendKind, Button, MouseKind};
use intuituive::menu::{context_menu, MenuItem};
use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::rich::Style;
use intuituive::widgets::{breadcrumb, seg, status_bar, tabs, Caps};
use intuituive::{
    rule, Caret, ConfirmOptions, Corner, Level, Offset, Origin, PickOptions, PromptOptions,
};
use rich_ext::file_ops::{under, Conflict, Op};
use rich_ext::filetype;
use rich_ext::listing::{owner, permissions, readable_size, smart_contains, walk};
use rich_ext::open::Opener;

// ---------------------------------------------------------------------
// The state.
// ---------------------------------------------------------------------

/// A tab: its directory, where it has been, and a name given to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub cwd: PathBuf,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    name: Option<String>,
}

impl Tab {
    fn new(cwd: PathBuf) -> Tab {
        Tab {
            cwd,
            back: Vec::new(),
            forward: Vec::new(),
            name: None,
        }
    }

    /// What the tab strip calls it.
    fn title(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.cwd
                .file_name()
                .map_or("/".into(), |n| n.to_string_lossy().into_owned())
        })
    }
}

/// Files yanked for pasting, copied or cut.
#[derive(Clone, Debug, PartialEq)]
struct Yank {
    paths: Vec<PathBuf>,
    cut: bool,
}

/// The app's state: the Miller columns (directory, cursor, listing,
/// preview, sort, filter, find, line mode) and what Yazi keeps beside
/// them. All of it is `Copy`, so any handler takes a copy.
#[derive(Clone, Copy)]
struct Fm {
    m: Miller,
    icons: bool,
    tabs: Signal<Vec<Tab>>,
    active: Signal<usize>,
    selected: Signal<BTreeSet<PathBuf>>,
    /// Visual mode: where it started, and whether it unselects.
    visual: Signal<Option<(usize, bool)>>,
    yanked: Signal<Option<Yank>>,
    /// Every directory this session has shown (Yazi asks zoxide).
    visited: Signal<BTreeSet<PathBuf>>,
    jobs: Tasks,
}

impl Fm {
    fn new(start: PathBuf, icons: bool) -> Fm {
        let fm = Fm {
            m: miller(start.clone()).icons(icons),
            icons,
            tabs: signal(vec![Tab::new(start)]),
            active: signal(0),
            selected: signal(BTreeSet::new()),
            visual: signal(None),
            yanked: signal(None),
            visited: signal(BTreeSet::new()),
            jobs: tasks(),
        };
        // Wherever the columns go (keys, clicks, a finder), the tab keeps
        // its history for H and L, and visual mode ends.
        watch(
            move || fm.m.cwd.get(),
            move |dir, _| {
                let i = fm.active.get_untracked();
                fm.tabs.update(|tabs| {
                    let tab = &mut tabs[i];
                    if tab.cwd != dir {
                        tab.back.push(std::mem::replace(&mut tab.cwd, dir.clone()));
                        tab.forward.clear();
                    }
                });
                fm.visual.set(None);
                fm.visited.update(|v| {
                    v.insert(dir);
                });
            },
        );
        fm
    }

    fn cwd(&self) -> PathBuf {
        self.m.cwd.get_untracked()
    }

    fn len(&self) -> usize {
        self.m.current.entries().with_untracked(Vec::len)
    }

    /// Back (`H`) or forward (`L`) through this tab's history.
    fn history(&self, forward: bool) {
        let i = self.active.get_untracked();
        let mut to = None;
        self.tabs.update(|tabs| {
            let tab = &mut tabs[i];
            let (from, other) = if forward {
                (&mut tab.forward, &mut tab.back)
            } else {
                (&mut tab.back, &mut tab.forward)
            };
            if let Some(dir) = from.pop() {
                other.push(std::mem::replace(&mut tab.cwd, dir.clone()));
                to = Some(dir);
            }
        });
        if let Some(dir) = to {
            self.m.cd(dir, None);
        }
    }

    /// The range visual mode covers, if it is on.
    fn visual_range(&self) -> Option<(usize, usize, bool)> {
        let (anchor, unset) = self.visual.get()?;
        let at = self.m.cursor.get();
        Some((anchor.min(at), anchor.max(at), unset))
    }

    /// End visual mode, keeping what it selected (or unselected).
    fn end_visual(&self) -> bool {
        let Some((anchor, unset)) = self.visual.get_untracked() else {
            return false;
        };
        let at = self.m.cursor.get_untracked();
        // A filter may have left fewer entries than the range, or none.
        let paths: Vec<PathBuf> = self.m.current.entries().with_untracked(|f| {
            let (from, to) = (anchor.min(at), anchor.max(at));
            if from >= f.len() {
                return Vec::new();
            }
            f[from..=to.min(f.len() - 1)]
                .iter()
                .map(|e| self.m.path_of(e))
                .collect()
        });
        self.selected.update(|s| {
            for path in paths {
                if unset {
                    s.remove(&path);
                } else {
                    s.insert(path);
                }
            }
        });
        self.visual.set(None);
        true
    }

    /// What an action applies to: the selection, else the hovered entry.
    fn targets(&self) -> Vec<PathBuf> {
        self.end_visual();
        let chosen: Vec<PathBuf> = self
            .selected
            .with_untracked(|s| s.iter().cloned().collect());
        if !chosen.is_empty() {
            return chosen;
        }
        self.m.hovered_path().into_iter().collect()
    }

    /// The marker of `path`, at `index` in the current column (`None` in
    /// the parent column, which has no visual range).
    fn marker(&self, index: Option<usize>, path: &Path) -> Marker {
        if let (Some(i), Some((from, to, unset))) = (index, self.visual_range()) {
            if (from..=to).contains(&i) {
                return if unset { Marker::None } else { Marker::Marked };
            }
        }
        if self.selected.with(|s| s.contains(path)) {
            return Marker::Selected;
        }
        self.yanked.with(|y| match y {
            Some(y) if y.paths.iter().any(|p| p == path) => {
                if y.cut {
                    Marker::Cut
                } else {
                    Marker::Copied
                }
            }
            _ => Marker::None,
        })
    }

    /// Find the next (or previous) entry matching the find query, from
    /// the one after the cursor (or before), wrapping; `here` counts the
    /// cursor's own entry first.
    fn find_arrow(&self, previous: bool, here: bool) {
        let Some(query) = self.m.find.get_untracked().filter(|q| !q.is_empty()) else {
            return;
        };
        let names: Vec<String> = self
            .m
            .current
            .entries()
            .with_untracked(|f| f.iter().map(|e| e.name.clone()).collect());
        let len = names.len();
        let at = self.m.cursor.get_untracked();
        for step in (if here { 0 } else { 1 })..=len {
            let i = if previous {
                (at + len * 2 - step) % len.max(1)
            } else {
                (at + step) % len.max(1)
            };
            if names.get(i).is_some_and(|n| smart_contains(n, &query)) {
                self.m.cursor.set(i);
                return;
            }
        }
    }

    /// Keep the hovered entry while `change` re-sorts or re-filters.
    fn keep(&self, change: impl FnOnce()) {
        let name = self.m.hovered.get_untracked().map(|e| e.name);
        change();
        if let Some(at) = name.and_then(|n| self.m.current.position(&n)) {
            self.m.cursor.set(at);
        }
    }

    fn new_tab(&self) {
        let here = self.cwd();
        let at = self.active.get_untracked() + 1;
        self.tabs.update(|tabs| tabs.insert(at, Tab::new(here)));
        self.switch_tab(at);
    }

    fn switch_tab(&self, to: usize) {
        let Some(dir) = self
            .tabs
            .with_untracked(|t| t.get(to).map(|t| t.cwd.clone()))
        else {
            return;
        };
        if to != self.active.get_untracked() {
            self.end_visual();
            self.active.set(to);
            self.m.cd(dir, None);
        }
    }

    /// Close this tab, or quit if it is the last.
    fn close_tab(&self, cx: &mut Ctx) {
        let count = self.tabs.with_untracked(Vec::len);
        if count <= 1 {
            return quit(*self, cx);
        }
        let i = self.active.get_untracked();
        self.tabs.update(|tabs| {
            tabs.remove(i);
        });
        let to = i.min(count - 2);
        self.active.set(to);
        let dir = self.tabs.with_untracked(|t| t[to].cwd.clone());
        self.m.cd(dir, None);
    }

    fn swap_tab(&self, by: isize) {
        let count = self.tabs.with_untracked(Vec::len) as isize;
        let i = self.active.get_untracked() as isize;
        let j = (i + by).rem_euclid(count);
        if count > 1 {
            self.tabs.update(|tabs| tabs.swap(i as usize, j as usize));
            self.active.set(j as usize);
        }
    }

    /// Run a file operation as a task; then read the directories again,
    /// and say what went wrong, as Yazi's tasks report a failure.
    fn job(&self, op: Op) {
        let (fm, title) = (*self, op.title());
        self.jobs.run(op, move |report, cx| {
            fm.m.refresh();
            if !report.errors.is_empty() {
                cx.notify(
                    Level::Error,
                    escape(&title),
                    escape(&report.errors.join("\n")),
                );
            }
        });
    }
}

/// q: quit, asking first while tasks run (Yazi's `quit` confirm).
fn quit(fm: Fm, cx: &mut Ctx) {
    if fm.jobs.running() == 0 {
        return cx.quit();
    }
    cx.confirm_with(
        "Quit?",
        "There are unfinished tasks, quit anyway?",
        Vec::new(),
        ConfirmOptions::new().size(Size::Fixed(50), Size::Fixed(5)),
        |cx| cx.quit(),
    );
}

// ---------------------------------------------------------------------
// Keys: Yazi's default keymap.
// ---------------------------------------------------------------------

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Escape,
    Quit,
    Close,
    Arrow(isize),
    Top,
    Bottom,
    Page(f64),
    Leave,
    Enter,
    Back,
    Forward,
    Toggle,
    SelectAll,
    Invert,
    Visual(bool),
    Seek(i16),
    Spot,
    Open(bool),
    Yank(bool),
    Paste(bool),
    Unyank,
    Remove(bool),
    Create,
    Rename,
    Hidden,
    Jump,
    Zoxide,
    Tasks,
    Linemode(Linemode),
    CopyPath,
    CopyDir,
    CopyName,
    CopyStem,
    Filter,
    Find(bool),
    FindArrow(bool),
    Sort(SortBy, bool),
    Home,
    Go(&'static str),
    GoInteractive,
    Follow,
    TabNew,
    TabRename,
    TabSwitch(usize),
    TabStep(isize),
    TabSwap(isize),
    Help,
}

/// Keys (as bindings name them), how Yazi's help writes them, what they
/// do.
type Binding = (&'static str, &'static str, &'static str, Act);

/// The single keys of the manager, each name in `keys` one way to press
/// it.
#[rustfmt::skip]
const KEYS: &[Binding] = &[
    ("esc", "<Esc>", "Exit visual mode, clear selection, or cancel search", Act::Escape),
    ("q", "q", "Quit the process", Act::Quit),
    ("ctrl+c", "<C-c>", "Close the current tab, or quit if it's last", Act::Close),
    ("k up", "k", "Previous file", Act::Arrow(-1)),
    ("j down", "j", "Next file", Act::Arrow(1)),
    ("G end", "G", "Go to bottom", Act::Bottom),
    ("home", "<Home>", "Go to top", Act::Top),
    ("ctrl+u shift+pageup", "<C-u>", "Move cursor up half page", Act::Page(-0.5)),
    ("ctrl+d shift+pagedown", "<C-d>", "Move cursor down half page", Act::Page(0.5)),
    ("ctrl+b pageup", "<C-b>", "Move cursor up one page", Act::Page(-1.0)),
    ("ctrl+f pagedown", "<C-f>", "Move cursor down one page", Act::Page(1.0)),
    ("h left", "h", "Back to the parent directory", Act::Leave),
    ("l right", "l", "Enter the child directory", Act::Enter),
    ("H", "H", "Back to previous directory", Act::Back),
    ("L", "L", "Forward to next directory", Act::Forward),
    ("space", "<Space>", "Toggle the current selection state", Act::Toggle),
    ("ctrl+a", "<C-a>", "Select all files", Act::SelectAll),
    ("ctrl+r", "<C-r>", "Invert selection of all files", Act::Invert),
    ("v", "v", "Enter visual mode (selection mode)", Act::Visual(false)),
    ("V", "V", "Enter visual mode (unset mode)", Act::Visual(true)),
    ("K", "K", "Seek up 5 units in the preview", Act::Seek(-5)),
    ("J", "J", "Seek down 5 units in the preview", Act::Seek(5)),
    ("tab", "<Tab>", "Spot hovered file", Act::Spot),
    ("o enter", "o", "Open selected files", Act::Open(false)),
    ("O shift+enter", "O", "Open selected files interactively", Act::Open(true)),
    ("y", "y", "Yank selected files (copy)", Act::Yank(false)),
    ("x", "x", "Yank selected files (cut)", Act::Yank(true)),
    ("p", "p", "Paste yanked files", Act::Paste(false)),
    ("P", "P", "Paste yanked files (overwrite if the destination exists)", Act::Paste(true)),
    ("Y X", "Y", "Cancel the yank status", Act::Unyank),
    ("d", "d", "Trash selected files", Act::Remove(false)),
    ("D", "D", "Permanently delete selected files", Act::Remove(true)),
    ("a", "a", "Create a file (ends with / for directories)", Act::Create),
    ("r", "r", "Rename selected file", Act::Rename),
    (".", ".", "Toggle the visibility of hidden files", Act::Hidden),
    ("z", "z", "Jump to a file/directory via fzf", Act::Jump),
    ("Z", "Z", "Jump to a directory via zoxide", Act::Zoxide),
    ("w", "w", "Show task manager", Act::Tasks),
    ("f", "f", "Filter files", Act::Filter),
    ("/", "/", "Find next file", Act::Find(false)),
    ("?", "?", "Find previous file", Act::Find(true)),
    ("n", "n", "Next found", Act::FindArrow(false)),
    ("N", "N", "Previous found", Act::FindArrow(true)),
    ("[", "[", "Switch to previous tab", Act::TabStep(-1)),
    ("]", "]", "Switch to next tab", Act::TabStep(1)),
    ("{", "{", "Swap current tab with previous tab", Act::TabSwap(-1)),
    ("}", "}", "Swap current tab with next tab", Act::TabSwap(1)),
    ("~ f1", "~", "Open help", Act::Help),
];

/// The key sequences: keys pressed one after another.
#[rustfmt::skip]
const SEQUENCES: &[Binding] = &[
    ("g g", "gg", "Go to top", Act::Top),
    ("g h", "gh", "Go home", Act::Home),
    ("g c", "gc", "Go to ~/.config", Act::Go(".config")),
    ("g d", "gd", "Go to ~/Downloads", Act::Go("Downloads")),
    ("g space", "g<Space>", "Jump interactively", Act::GoInteractive),
    ("g f", "gf", "Follow hovered symlink", Act::Follow),
    ("comma m", ",m", "Sort by modified time", Act::Sort(SortBy::Mtime, false)),
    ("comma M", ",M", "Sort by modified time (reverse)", Act::Sort(SortBy::Mtime, true)),
    ("comma b", ",b", "Sort by birth time", Act::Sort(SortBy::Btime, false)),
    ("comma B", ",B", "Sort by birth time (reverse)", Act::Sort(SortBy::Btime, true)),
    ("comma e", ",e", "Sort by extension", Act::Sort(SortBy::Extension, false)),
    ("comma E", ",E", "Sort by extension (reverse)", Act::Sort(SortBy::Extension, true)),
    ("comma a", ",a", "Sort alphabetically", Act::Sort(SortBy::Alphabetical, false)),
    ("comma A", ",A", "Sort alphabetically (reverse)", Act::Sort(SortBy::Alphabetical, true)),
    ("comma n", ",n", "Sort naturally", Act::Sort(SortBy::Natural, false)),
    ("comma N", ",N", "Sort naturally (reverse)", Act::Sort(SortBy::Natural, true)),
    ("comma s", ",s", "Sort by size", Act::Sort(SortBy::Size, false)),
    ("comma S", ",S", "Sort by size (reverse)", Act::Sort(SortBy::Size, true)),
    ("comma r", ",r", "Sort randomly", Act::Sort(SortBy::Random, false)),
    ("m s", "ms", "Linemode: size", Act::Linemode(Linemode::Size)),
    ("m p", "mp", "Linemode: permissions", Act::Linemode(Linemode::Permissions)),
    ("m b", "mb", "Linemode: btime", Act::Linemode(Linemode::Btime)),
    ("m m", "mm", "Linemode: mtime", Act::Linemode(Linemode::Mtime)),
    ("m o", "mo", "Linemode: owner", Act::Linemode(Linemode::Owner)),
    ("m n", "mn", "Linemode: none", Act::Linemode(Linemode::None)),
    ("c c", "cc", "Copy file path", Act::CopyPath),
    ("c d", "cd", "Copy directory path", Act::CopyDir),
    ("c f", "cf", "Copy filename", Act::CopyName),
    ("c n", "cn", "Copy filename without extension", Act::CopyStem),
    ("t t", "tt", "Create a new tab in CWD", Act::TabNew),
    ("t r", "tr", "Rename current tab", Act::TabRename),
];

/// Yazi's input: plain titles (its confirm's are blue), at the top
/// centre, 50 wide.
fn input_look() -> PromptOptions {
    PromptOptions::new().title_style("default")
}

/// Do `act`.
fn run(fm: Fm, act: Act, cx: &mut Ctx) {
    let m = fm.m;
    match act {
        Act::Escape => {
            // Yazi's order: visual mode, then the selection, the filter,
            // the find.
            if fm.end_visual() {
            } else if !fm.selected.with_untracked(BTreeSet::is_empty) {
                fm.selected.set(BTreeSet::new());
            } else if !m.current.filter.with_untracked(String::is_empty) {
                fm.keep(|| m.current.filter.set(String::new()));
            } else {
                m.find.set(None);
            }
        }
        Act::Quit => quit(fm, cx),
        Act::Close => fm.close_tab(cx),
        Act::Arrow(by) => m.arrow(by),
        Act::Top => m.move_to(0),
        Act::Bottom => m.move_to(isize::MAX),
        Act::Page(share) => m.page(share),
        Act::Leave => m.leave(),
        Act::Enter => m.enter(),
        Act::Back => fm.history(false),
        Act::Forward => fm.history(true),
        Act::Toggle => {
            if let Some(path) = m.hovered_path() {
                fm.selected.update(|s| {
                    if !s.remove(&path) {
                        s.insert(path);
                    }
                });
                m.arrow(1);
            }
        }
        Act::SelectAll | Act::Invert => {
            let paths: Vec<PathBuf> = m
                .current
                .entries()
                .with_untracked(|f| f.iter().map(|e| m.path_of(e)).collect());
            fm.selected.update(|s| {
                for path in paths {
                    if act == Act::SelectAll || !s.remove(&path) {
                        s.insert(path);
                    }
                }
            });
        }
        Act::Visual(unset) => {
            if fm.visual.get_untracked().is_none() && fm.len() > 0 {
                fm.visual.set(Some((m.cursor.get_untracked(), unset)));
            } else {
                fm.end_visual();
            }
        }
        Act::Seek(by) => m
            .preview_top
            .update(|t| *t = (*t as i32 + by as i32).max(0) as u16),
        Act::Spot => spot(fm, cx),
        Act::Open(interactive) => open(fm, interactive, cx),
        Act::Yank(cut) => {
            let paths = fm.targets();
            if !paths.is_empty() {
                fm.yanked.set(Some(Yank { paths, cut }));
                fm.selected.set(BTreeSet::new());
            }
        }
        Act::Unyank => fm.yanked.set(None),
        Act::Paste(force) => paste(fm, force, cx),
        Act::Remove(permanently) => remove(fm, permanently, cx),
        Act::Create => {
            let dir = fm.cwd();
            cx.prompt_with("Create:", "", input_look(), move |typed, cx| {
                let path = match under(&dir, &typed) {
                    Ok(path) => path,
                    Err(e) => return cx.notify(Level::Warn, "Create", escape(&e)),
                };
                if path.exists() {
                    let body = format!("{} already exists", escape(&typed));
                    return cx.notify(Level::Warn, "Create", body);
                }
                let made = if typed.ends_with('/') || typed.ends_with('\\') {
                    std::fs::create_dir_all(&path)
                } else {
                    path.parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|_| std::fs::File::create(&path).map(|_| ()))
                };
                match made {
                    Ok(()) => {
                        m.refresh();
                        let first = Path::new(typed.trim_end_matches('/'))
                            .components()
                            .next()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned());
                        if let Some(at) = first.and_then(|n| m.current.position(&n)) {
                            m.cursor.set(at);
                        }
                    }
                    Err(e) => cx.notify(Level::Error, "Create", escape(&e.to_string())),
                }
            });
        }
        Act::Rename => {
            let Some(entry) = m.hovered.get_untracked() else {
                return;
            };
            let dir = fm.cwd();
            let old = entry.name.clone();
            // Yazi's `rename --cursor=before_ext`: the caret before the
            // extension (a directory's name has none).
            let stem = match Path::new(&old).extension() {
                Some(ext) if !entry.dir => {
                    old.chars().count() - ext.to_string_lossy().chars().count() - 1
                }
                _ => old.chars().count(),
            };
            // Below the hovered row, where it was drawn.
            let row = m.row_rects().get(m.cursor.get_untracked());
            let options = input_look()
                .at(
                    row.map_or(Origin::TopCenter, Origin::At),
                    Offset::new(0, 0, 50, 3),
                )
                .caret(Caret::At(stem));
            cx.prompt_with("Rename:", old.clone(), options, move |typed, cx| {
                if typed == old {
                    return;
                }
                let to = match under(&dir, &typed) {
                    Ok(to) => to,
                    Err(e) => return cx.notify(Level::Warn, "Rename", escape(&e)),
                };
                if std::fs::symlink_metadata(&to).is_ok() {
                    let body = format!("{} already exists", escape(&typed));
                    return cx.notify(Level::Warn, "Rename", body);
                }
                match std::fs::rename(dir.join(&old), &to) {
                    Ok(()) => {
                        m.refresh();
                        if let Some(at) = m.current.position(&typed) {
                            m.cursor.set(at);
                        }
                    }
                    Err(e) => cx.notify(Level::Error, "Rename", escape(&e.to_string())),
                }
            });
        }
        Act::Hidden => fm.keep(|| m.hidden.update(|h| *h = !*h)),
        Act::Jump => jump(fm, cx),
        Act::Zoxide => zoxide(fm, cx),
        Act::Tasks => task_manager(fm, cx),
        Act::Linemode(mode) => m.linemode.set(mode),
        Act::CopyPath | Act::CopyDir | Act::CopyName | Act::CopyStem => {
            let Some(path) = m.hovered_path() else {
                return;
            };
            let text = match act {
                Act::CopyPath => path.display().to_string(),
                Act::CopyDir => fm.cwd().display().to_string(),
                Act::CopyName => path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                _ => path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
            };
            cx.copy(text);
        }
        Act::Filter => {
            let before = m.current.filter.get_untracked();
            let options = input_look()
                .on_change(move |typed, _| fm.keep(|| m.current.filter.set(typed.to_string())))
                .on_cancel(move |_| fm.keep(|| m.current.filter.set(before)));
            cx.prompt_with(
                "Filter:",
                m.current.filter.get_untracked(),
                options,
                |_, _| {},
            );
        }
        Act::Find(previous) => {
            let title = if previous {
                "Find previous:"
            } else {
                "Find next:"
            };
            let from = m.cursor.get_untracked();
            let options = input_look()
                .on_change(move |typed, _| {
                    m.cursor.set(from);
                    m.find.set((!typed.is_empty()).then(|| typed.to_string()));
                    fm.find_arrow(previous, true);
                })
                .on_cancel(move |_| {
                    m.find.set(None);
                    m.cursor.set(from);
                });
            cx.prompt_with(title, "", options, |_, _| {});
        }
        Act::FindArrow(previous) => fm.find_arrow(previous, false),
        Act::Sort(by, reverse) => {
            fm.keep(|| m.sort.set(Sort::by(by).reverse(reverse)));
            // As Yazi's keymap: sorting by size or time shows it.
            match by {
                SortBy::Size => m.linemode.set(Linemode::Size),
                SortBy::Mtime => m.linemode.set(Linemode::Mtime),
                SortBy::Btime => m.linemode.set(Linemode::Btime),
                _ => {}
            }
        }
        Act::Home => {
            if let Some(home) = home() {
                m.cd(home, None);
            }
        }
        Act::Go(rel) => match home().map(|h| h.join(rel)).filter(|d| d.is_dir()) {
            Some(dir) => m.cd(dir, None),
            None => cx.notify(Level::Warn, "Cd", format!("~/{rel} does not exist")),
        },
        Act::GoInteractive => {
            let here = fm.cwd();
            cx.prompt_with("Change directory:", "", input_look(), move |typed, cx| {
                let typed = typed.trim();
                let target = match typed.strip_prefix('~') {
                    Some(rest) => home().map(|h| h.join(rest.trim_start_matches('/'))),
                    None => Some(here.join(typed)),
                };
                match target.filter(|t| t.is_dir()) {
                    Some(dir) => m.cd(absolute(dir), None),
                    None => cx.notify(Level::Warn, "Cd", format!("No directory {}", escape(typed))),
                }
            });
        }
        Act::Follow => {
            if let Some(to) = m.hovered.get_untracked().and_then(|e| e.link) {
                reveal(fm, &absolute(fm.cwd().join(to)));
            }
        }
        Act::TabNew => fm.new_tab(),
        Act::TabRename => {
            let i = fm.active.get_untracked();
            let now = fm.tabs.with_untracked(|t| t[i].title());
            cx.prompt_with("Rename tab:", now, input_look(), move |typed, _| {
                let name = (!typed.trim().is_empty()).then(|| typed.trim().to_string());
                fm.tabs.update(|t| t[i].name = name);
            });
        }
        Act::TabSwitch(to) => fm.switch_tab(to),
        Act::TabStep(by) => {
            let count = fm.tabs.with_untracked(Vec::len) as isize;
            let to = (fm.active.get_untracked() as isize + by).rem_euclid(count);
            fm.switch_tab(to as usize);
        }
        Act::TabSwap(by) => fm.swap_tab(by),
        Act::Help => help(cx),
    }
}

/// `p` and `P`: paste the yanked files here as a task, copies under a
/// free name; `P` replaces what has the same name, after saying which.
fn paste(fm: Fm, force: bool, cx: &mut Ctx) {
    let Some(yank) = fm.yanked.get_untracked() else {
        return;
    };
    let into = fm.cwd();
    let clashes: Vec<String> = yank
        .paths
        .iter()
        .filter_map(|p| p.file_name().map(|n| into.join(n)))
        .filter(|p| force && std::fs::symlink_metadata(p).is_ok())
        .map(|p| readable_path(&p))
        .collect();
    let conflict = if force {
        Conflict::Overwrite
    } else {
        Conflict::Rename
    };
    let start = move |_: &mut Ctx| {
        if yank.cut {
            fm.yanked.set(None);
        }
        let (from, into) = (yank.paths, into);
        fm.job(if yank.cut {
            Op::Move {
                from,
                into,
                conflict,
            }
        } else {
            Op::Copy {
                from,
                into,
                conflict,
            }
        });
    };
    if clashes.is_empty() {
        return start(cx);
    }
    cx.confirm_with(
        "Overwrite file?",
        "Will overwrite the following file:",
        clashes,
        ConfirmOptions::new().size(Size::Fixed(50), Size::Fixed(15)),
        start,
    );
}

/// `d` and `D`: trash (the system's trash) or delete for good, as a task,
/// after asking in Yazi's 70 x 20 confirm.
fn remove(fm: Fm, permanently: bool, cx: &mut Ctx) {
    let paths = fm.targets();
    if paths.is_empty() {
        return;
    }
    let n = paths.len();
    let s = if n == 1 { "" } else { "s" };
    let title = if permanently {
        format!("Permanently delete {n} selected file{s}?")
    } else {
        format!("Trash {n} selected file{s}?")
    };
    let shown = paths.iter().map(|p| readable_path(p)).collect();
    let look = ConfirmOptions::new().size(Size::Fixed(70), Size::Fixed(20));
    cx.confirm_with(title, "", shown, look, move |_| {
        fm.selected.set(BTreeSet::new());
        fm.job(if permanently {
            Op::Delete(paths)
        } else {
            Op::Trash(paths)
        });
    });
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Run `opener` on `paths`: extraction as a task (with the `archive`
/// feature), anything else through `open_with`.
fn launch(fm: Fm, opener: &Opener, paths: Vec<PathBuf>, cx: &mut Ctx) {
    #[cfg(feature = "archive")]
    if opener.launch == rich_ext::open::Launch::Extract {
        for path in paths {
            let dest = rich_ext::archive::default_dest(&path);
            fm.jobs.extract(path, dest, move |report, cx| {
                fm.m.refresh();
                if !report.errors.is_empty() {
                    cx.notify(Level::Error, "Extract", escape(&report.errors.join("\n")));
                }
            });
        }
        return;
    }
    open_with(cx, opener, paths, move |_| fm.m.refresh());
}

/// Enter and `o` open the selection (or the hovered entry) with the first
/// opener Yazi's rules give its type; `O` asks which, in Yazi's "Open
/// with:" pick below the hovered row.
fn open(fm: Fm, interactive: bool, cx: &mut Ctx) {
    let rules = opener_rules();
    let files: Vec<(PathBuf, &str)> = fm
        .targets()
        .into_iter()
        .map(|path| {
            let mime = filetype::mime_for(&path).unwrap_or("application/octet-stream");
            (path, mime)
        })
        .collect();
    if !interactive {
        // Each file with its own first opener; the editor once for all.
        let mut by: Vec<(Opener, Vec<PathBuf>)> = Vec::new();
        for (path, mime) in files {
            let Some(first) = rules.matching(&path, mime).first().cloned().cloned() else {
                continue;
            };
            match by.iter_mut().find(|(o, _)| *o == first) {
                Some((_, all)) => all.push(path),
                None => by.push((first, vec![path])),
            }
        }
        for (opener, paths) in by {
            launch(fm, &opener, paths, cx);
        }
        return;
    }
    let openers: Vec<Opener> = rules.shared(&files).into_iter().cloned().collect();
    if openers.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = files.into_iter().map(|(p, _)| p).collect();
    let row = fm.m.row_rects().get(fm.m.cursor.get_untracked());
    let origin = row.map_or(Origin::Center, Origin::At);
    let names = openers.iter().map(|o| escape(&o.desc)).collect();
    let height = (openers.len() as u16 + 2).min(7);
    let look = PickOptions::new().offset(Offset::new(0, 0, 50, height));
    cx.pick_with("Open with:", origin, names, look, move |i, cx| {
        launch(fm, &openers[i], paths, cx);
    });
}

// ---------------------------------------------------------------------
// Pop-ups: help, spot, tasks, jumps.
// ---------------------------------------------------------------------

/// Yazi's help: every key and what it does, in a box in the middle; `/`
/// filters, j/k move, Esc closes.
fn help(cx: &mut Ctx) {
    let rows: Vec<(String, String)> = KEYS
        .iter()
        .chain(SEQUENCES)
        .map(|(_, on, desc, _)| (on.to_string(), desc.to_string()))
        .chain((1..=9).map(|n| (n.to_string(), format!("Switch to tab {n}"))))
        .chain([(
            "<C-p>".to_string(),
            "Find a command (not in Yazi)".to_string(),
        )])
        .collect();
    cx.modal(Size::Fixed(80), Size::Fixed(25), move || {
        let query = signal(String::new());
        let at = signal(0usize);
        let shown = memo(move || {
            let q = query.get();
            rows.iter()
                .filter(|(on, desc)| smart_contains(on, &q) || smart_contains(desc, &q))
                .map(|(on, desc)| {
                    let pad = " ".repeat(20usize.saturating_sub(on.chars().count()));
                    format!("[cyan]{}[/]{pad}{}", escape(on), escape(desc))
                })
                .collect::<Vec<_>>()
        });
        watch(move || query.get(), move |_, _| at.set(0));
        let field = input(query).placeholder("Type / to filter");
        let field_id = field.id();
        let keys = list(move || shown.get(), at)
            .theme_styles([("selected", "reverse bold")])
            .autofocus();
        let keys_id = keys.id();
        column([
            field
                .on_submit(move |_, cx| cx.focus(keys_id))
                .on_key("esc", move |cx| cx.focus(keys_id))
                .fixed(1),
            rule(),
            keys.on_key("/", move |cx| cx.focus(field_id))
                .on_key("esc q ~ f1", |cx| cx.pop())
                .flex(1),
        ])
        .padding(0, 1)
        .panel("mgr.help")
        .title_align(Align::Center)
    });
}

/// Tab: what Yazi's spotter shows about the hovered file.
fn spot(fm: Fm, cx: &mut Ctx) {
    let Some(entry) = fm.m.hovered.get_untracked() else {
        return;
    };
    let path = fm.m.path_of(&entry);
    let mut facts = vec![
        ("Name", entry.name.clone()),
        ("Path", path.display().to_string()),
        ("Mime", fm.m.current.mime(&entry).to_string()),
        ("Size", readable_size(entry.size)),
        ("Mode", permissions(entry.mode)),
        ("Owner", owner(entry.uid, entry.gid)),
    ];
    let full = |t: SystemTime| rich_ext::fs::local_time(t, "%Y-%m-%d %H:%M:%S");
    if let Some(t) = entry.mtime {
        facts.push(("Modified", full(t)));
    }
    if let Some(t) = entry.btime {
        facts.push(("Created", full(t)));
    }
    if let Some(to) = &entry.link {
        facts.push(("Target", to.display().to_string()));
    }
    let height = facts.len() as u16 + 2;
    cx.modal(Size::Fixed(60), Size::Fixed(height), move || {
        let lines: Vec<String> = facts
            .iter()
            .map(|(k, v)| format!("[blue]{k:<10}[/][yellow]{}[/]", escape(v)))
            .collect();
        label(lines.join("\n"))
            .no_wrap()
            .overflow(Overflow::Ellipsis)
            .padding(0, 1)
            .panel("Spot")
            .title_align(Align::Center)
            .on_key("esc q tab enter", |cx| cx.pop())
    });
}

/// w: Yazi's task manager, the file jobs with their progress; x cancels
/// one, Esc or w closes it.
fn task_manager(fm: Fm, cx: &mut Ctx) {
    let jobs = fm.jobs;
    cx.modal(Size::Percent(80), Size::Percent(50), move || {
        tasks_panel(jobs)
            .autofocus()
            .padding(0, 1)
            .panel("Tasks")
            .title_align(Align::Center)
            .on_key("esc w q", |cx| cx.pop())
    });
}

/// z: find a file or directory under this one (Yazi asks fzf over fd),
/// with the `files` finder: its walk streams in while you type, Enter
/// goes there.
fn jump(fm: Fm, cx: &mut Ctx) {
    let tree = walk(fm.cwd()).hidden(fm.m.hidden.get_untracked());
    cx.modal(Size::Percent(80), Size::Percent(70), move || {
        finder(tree, move |path, cx| {
            cx.pop();
            reveal(fm, &path);
        })
        .on_key("esc", |cx| cx.pop())
        .panel("")
    });
}

/// Z: jump to a directory this session has been in (Yazi asks zoxide),
/// with rs-rich-interact's `Jump`.
fn zoxide(fm: Fm, cx: &mut Ctx) {
    let seen: Vec<String> = fm.visited.with_untracked(|v| {
        v.iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    });
    let picker = Jump::new("Directory").source(JumpSource::new("dirs", "Visited").items(seen));
    cx.modal(Size::Percent(80), Size::Percent(70), move || {
        component(picker, move |jumped, cx| {
            cx.pop();
            fm.m.cd(PathBuf::from(&jumped.value), None);
        })
        .on_cancel(|cx| cx.pop())
        .panel("")
    });
}

/// Go to `path`'s directory, hovering it.
fn reveal(fm: Fm, path: &Path) {
    if let Some(dir) = path.parent() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        fm.m.cd(dir.to_path_buf(), name);
    }
}

// ---------------------------------------------------------------------
// The screen.
// ---------------------------------------------------------------------

/// `path` as Yazi writes it: `~` for the home directory.
fn readable_path(path: &Path) -> String {
    match home().as_ref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// The top row: the directory (cut from the left when it is long) with
/// the filter and find in brackets, and at the right how many files are
/// selected or yanked.
fn header(fm: Fm) -> Node {
    let count = text(move || {
        let selected = fm.selected.with(BTreeSet::len);
        let (style, n) = match fm.yanked.get() {
            _ if selected > 0 => ("black on yellow", selected),
            Some(y) if y.cut => ("white on red", y.paths.len()),
            Some(y) => ("white on green", y.paths.len()),
            None => return String::new(),
        };
        format!("[{style}] {n} [/] ")
    })
    .no_wrap()
    .auto();
    let flags = move || {
        let mut flags = Vec::new();
        let filter = fm.m.current.filter.get();
        if !filter.is_empty() {
            flags.push(format!("filter: {filter}"));
        }
        if let Some(find) = fm.m.find.get() {
            flags.push(format!("find: {find}"));
        }
        if flags.is_empty() {
            return String::new();
        }
        format!("[cyan] ({})[/]", escape(&flags.join(", ")))
    };
    let cwd = breadcrumb(move || fm.m.cwd.get())
        .home_tilde()
        .crumb_suffix(flags)
        .flex(1);
    row([cwd, count]).fixed(1)
}

/// The tab strip, shown once there is more than one tab: Yazi's, the
/// active tab on blue. A click picks a tab as the keys do, so visual
/// mode ends the same way.
fn tab_strip(fm: Fm) -> Node {
    let size = viewport();
    let titles = move || {
        let width = size.get().0;
        let active = fm.active.get();
        fm.tabs.with(|tabs| {
            let max = (width as usize / tabs.len().max(1)).max(4);
            tabs.iter()
                .enumerate()
                .map(|(i, tab)| {
                    let mut title = format!(" {} {} ", i + 1, tab.title());
                    if title.chars().count() > max {
                        title = title.chars().take(max - 1).collect::<String>() + "…";
                    }
                    if i == active {
                        escape(&title)
                    } else {
                        format!("[blue on white]{}[/]", escape(&title))
                    }
                })
                .collect()
        })
    };
    tabs(titles, fm.active)
        .auto_hide()
        .on_select(move |to, _| fm.switch_tab(to))
        .divider("")
        .tab_padding("", "")
        .no_focus()
        .fixed(1)
}

/// The status line, as Yazi's: the mode, the hovered entry's size and
/// name on the left; the tasks' progress, its permissions, where the
/// cursor is as a share and as a count on the right. Rounded caps with
/// icons on.
fn status(fm: Fm) -> Node {
    let m = fm.m;
    let main = move || match fm.visual.get() {
        Some(_) => "bold on red".to_string(),
        None => "bold on blue".to_string(),
    };
    let alt = move || match fm.visual.get() {
        Some(_) => "red on white".to_string(),
        None => "blue on white".to_string(),
    };
    let mode = seg(move || {
        let mode = match fm.visual.get() {
            Some((_, false)) => "SEL",
            Some((_, true)) => "UNS",
            None => "NOR",
        };
        format!(" {mode} ")
    })
    .style_with(main);
    let size = seg(move || {
        let size = m.hovered.with(|e| e.as_ref().map_or(0, |e| e.size));
        format!(" {} ", readable_size(size))
    })
    .style_with(alt);
    let name = seg(move || {
        m.hovered.with(|e| {
            e.as_ref()
                .map_or(String::new(), |e| format!(" {}", escape(&e.name)))
        })
    });
    // Yazi's progress: how far the running tasks have got, and how many.
    let progress = seg(move || {
        fm.jobs.jobs().with(|jobs| {
            let running: Vec<_> = jobs
                .iter()
                .filter(|j| j.state == JobState::Running)
                .collect();
            if running.is_empty() {
                return String::new();
            }
            let done: f64 = running.iter().map(|j| j.progress.fraction()).sum();
            let percent = (done * 100.0 / running.len() as f64).floor() as u32;
            format!("[bright_white] {percent:>3}%, {} left [/] ", running.len())
        })
    });
    let perm = seg(move || {
        m.hovered.with(|e| {
            e.as_ref().map_or(String::new(), |e| {
                format!("{} ", coloured_permissions(e.mode))
            })
        })
    });
    let percent = seg(move || {
        let len = m.current.len();
        let cursor = m.cursor.get().min(len.saturating_sub(1));
        match (cursor, len) {
            (0, _) | (_, 0) => " Top ".to_string(),
            _ => match (cursor + 1) * 100 / len {
                100 => " Bot ".to_string(),
                p => format!(" {p:>2}% "),
            },
        }
    })
    .style_with(alt);
    let position = seg(move || {
        let len = m.current.len();
        let cursor = m.cursor.get().min(len.saturating_sub(1));
        format!(" {:>2}/{:<2} ", (cursor + 1).min(len), len)
    })
    .style_with(main);
    status_bar([mode, size, name], [progress, perm, percent, position]).caps(if fm.icons {
        Caps::Rounded
    } else {
        Caps::None
    })
}

/// What a right click on an entry offers.
fn menu(fm: Fm) -> Vec<MenuItem> {
    let entry = fm.m.hovered.get_untracked();
    let mut items = Vec::new();
    let item = |label: &str, keys: &str, act: Act| {
        MenuItem::new(label, move |cx| run(fm, act, cx)).hint(keys)
    };
    if entry.as_ref().is_some_and(|e| e.dir) {
        items.push(item("Enter", "l", Act::Enter));
    }
    items.push(item("Open", "o", Act::Open(false)));
    items.push(item("Open with…", "O", Act::Open(true)));
    items.push(item("Yank", "y", Act::Yank(false)));
    items.push(item("Cut", "x", Act::Yank(true)));
    if fm.yanked.with_untracked(Option::is_some) {
        items.push(item("Paste", "p", Act::Paste(false)));
    }
    items.push(item("Copy path", "cc", Act::CopyPath));
    items.push(item("Rename", "r", Act::Rename));
    items.push(MenuItem::separator());
    items.push(item("Trash", "d", Act::Remove(false)));
    items.push(item("Delete", "D", Act::Remove(true)));
    items.push(MenuItem::separator());
    items.push(item("Create", "a", Act::Create));
    let shown = if fm.m.hidden.get_untracked() {
        "Hide hidden files"
    } else {
        "Show hidden files"
    };
    items.push(item(shown, ".", Act::Hidden));
    items.push(item("New tab here", "tt", Act::TabNew));
    items
}

/// The Miller columns, with Yazi's markers and git signs; a right click
/// on a row of the current column (which the list hovers first) opens
/// the menu.
fn columns(fm: Fm) -> Node {
    let options = MillerOptions::new()
        .markers(move |i, path| fm.marker(i, path))
        .git(git_status(fm.m.current));
    fm.m.view_with(options).on_mouse(move |cx, mouse| {
        if mouse.kind != MouseKind::Down(Button::Right) {
            return false;
        }
        let Some((x, y)) = cx.pointer() else {
            return false;
        };
        let on_row =
            fm.m.row_rects()
                .shown()
                .iter()
                .any(|(_, r)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height);
        if on_row {
            context_menu(cx, menu(fm));
        }
        on_row
    })
}

/// `path` made absolute, as a person would write it: on Windows,
/// `canonicalize` gives a verbatim path (`\\?\C:\…`), whose prefix is
/// dropped when what follows is an ordinary drive path.
fn absolute(path: PathBuf) -> PathBuf {
    let full = path.canonicalize().unwrap_or(path);
    let text = full.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => full,
    }
}

/// Yazi's default dark theme, as the framework's named styles: the rails
/// gray, the header cyan, the active tab on blue, the hovered row
/// reversed, pop-up borders and the confirm's title blue, which-key and
/// notification colours.
fn theme() -> Theme {
    let parse = |s: &str| Style::parse(s).expect("a style");
    let mut theme = Theme::dark()
        .style("selected", parse("reverse"))
        .style("split.divider", parse("white"))
        .style("split.divider.hover", parse("bold bright_white"))
        .style("tabs.selected", parse("bold on blue"))
        .style("tabs.selected.blur", parse("bold on blue"))
        .style("breadcrumb", parse("cyan"))
        .style("breadcrumb.last", parse("cyan"))
        .style("breadcrumb.separator", parse("cyan"))
        .style("popup.title", parse("blue"))
        .style("which_key.key", parse("bright_cyan"))
        .style("which_key.sep", parse("bright_black"))
        .style("which_key.desc", parse("bright_magenta"))
        .style("notify.info", parse("green"))
        .style("notify.warn", parse("yellow"))
        .style("notify.error", parse("red"));
    theme.border = parse("blue");
    theme.border_focused = parse("blue");
    theme.title = Style::default();
    theme
}

/// The app, starting in `start`, with Nerd Font icons if `icons`.
pub fn files_app_with(start: PathBuf, icons: bool) -> App {
    App::new(move || {
        let fm = Fm::new(absolute(start), icons);
        // The column holds Yazi's keymap and sees each key before the
        // focused list, whose own j/k/g it overrides.
        let mut root =
            column([header(fm), tab_strip(fm), columns(fm).flex(1), status(fm)]).previews_keys();
        for &(keys, _, desc, act) in KEYS {
            root = root.bind(keys, desc, move |cx| run(fm, act, cx));
        }
        for &(keys, _, desc, act) in SEQUENCES {
            root = root.bind_sequence(keys, desc, move |cx| run(fm, act, cx));
        }
        for n in 1..=9usize {
            let desc = format!("Switch to tab {n}");
            root = root.bind(&n.to_string(), &desc, move |cx| {
                run(fm, Act::TabSwitch(n - 1), cx)
            });
        }
        root
    })
    .theme(theme())
    .which_key(true)
    .toasts_at(Corner::TopRight)
}

/// The app, starting in `start`, in plain text.
pub fn files_app(start: PathBuf) -> App {
    files_app_with(start, false)
}

/// The backend called `name`, if this build has it: termion and termwiz
/// are this crate's features of those names.
#[allow(dead_code)]
fn backend_named(name: &str) -> Result<BackendKind, String> {
    BackendKind::from_name(name).ok_or_else(|| {
        let have: Vec<&str> = BackendKind::ALL.iter().map(|kind| kind.name()).collect();
        format!(
            "no backend {name:?} in this build, which has {} (termion and termwiz \
             are features of rs-rich-intuituive)",
            have.join(", ")
        )
    })
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let invalid = |error: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, error);
    // `--backend NAME` (or `--backend=NAME`), else INTUITUIVE_BACKEND, else
    // crossterm; `--icons` or `--no-icons`; the other argument is the
    // directory to start in.
    let mut backend = std::env::var("INTUITUIVE_BACKEND")
        .ok()
        .filter(|name| !name.is_empty());
    let (mut start, mut icons) = (None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--backend" {
            let name = args.next().ok_or_else(|| {
                invalid("--backend takes a name: crossterm, termion or termwiz".into())
            })?;
            backend = Some(name);
        } else if let Some(name) = arg.strip_prefix("--backend=") {
            backend = Some(name.to_string());
        } else if arg == "--icons" || arg == "--no-icons" {
            icons = Some(arg == "--icons");
        } else {
            start = Some(PathBuf::from(arg));
        }
    }
    let backend = match backend {
        Some(name) => backend_named(&name).map_err(invalid)?,
        None => BackendKind::default(),
    };
    // Only a setting turns icons on by itself: an installed Nerd Font may
    // not be the terminal's.
    let icons = icons
        .unwrap_or_else(|| matches!(filetype::nerd_font(), filetype::NerdFont::Configured(true)));
    let start =
        start.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // The grammars a first preview is likely to need, warmed on a worker
    // so it does not stall.
    rich_ext::syntax::preload(&["markdown", "rust", "toml", "json", "python", "sh"]);
    files_app_with(start, icons)
        .palette_key("ctrl+p")
        .run_with(backend)
}
