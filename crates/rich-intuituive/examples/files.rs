//! A file manager in the style of [Yazi](https://github.com/sxyazi/yazi),
//! the most-starred app built on ratatui, rebuilt on intuiTUIve from its
//! ready-made components: the `files` plugin's Miller columns (the parent
//! directory, the current one and a preview of the hovered entry, loaded
//! in the background), its tabs, selection, yank, openers, file jobs,
//! finder, frecency, search, shell, bulk rename, spotter, git signs and
//! path header, and the core's status bar, help, fuzzy lists, prompts,
//! picks and confirms.
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
//! cut, p paste, d trash, D delete, a create, r rename (a selection in
//! `$EDITOR`), ; and : run a shell command, / and ? find, n/N the next
//! match, f filter, s and S search by name and content, . hidden files,
//! the `,` keys sort, `m` sets the line mode, `c` copies paths, `g` goes
//! places, z finds a file, Z jumps to a directory often visited (kept
//! between sessions), Tab shows a file's details, w the tasks, `t t` opens
//! a tab and 1-9, [ and ] switch, ~ or F1 shows the help, q quits. After
//! the first key of a sequence (g , c m t) a which-key pop-up shows what
//! can follow. Ctrl+P finds any command (not in Yazi).
//!
//! What changes files is real, and asked first: trashing moves to the
//! system's trash, deleting and overwriting say what they will do and wait
//! for Y. Each runs as a background task (`w`).
//!
//! The mouse: a click hovers an entry (in the parent column it goes there,
//! in a directory's preview it enters it), the wheel moves the cursor, a
//! right click (or the menu key) opens a menu, the column rails drag and a
//! click on a tab switches to it.

use std::path::{Path, PathBuf};

use intuituive::files::{
    bulk_rename, coloured_permissions, finder_with, frecency, git_status, home_dir, miller,
    open_with_tasks, opener_rules, path_crumb, path_parts, search, selection, shell, spot,
    spot_facts, tabs as file_tabs, tasks, tasks_panel_with, BinaryPreview, FileTabs, Frecency,
    JobsPanelOptions, Linemode, Miller, MillerOptions, SearchKind, ShellOptions, Sort, SortBy,
    Tasks, Yank,
};
use intuituive::finder::{fuzzy_list_with, FuzzyOptions, FuzzySource};
use intuituive::interact::BackendKind;
use intuituive::menu::{context_menu, open_menu, MenuItem};
use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::rich::Style;
use intuituive::widgets::{seg, seg_gauge, status_bar, tabs, Caps};
use intuituive::{
    Caret, ConfirmOptions, Corner, HelpOptions, KeyFormat, Level, MultiSelect, Offset, Origin,
    PickOptions, Placement, PromptOptions,
};
use rich_ext::file_ops::{under, Conflict, Op};
use rich_ext::filetype;
use rich_ext::listing::{readable_size, smart_contains, walk};
use rich_ext::open::Opener;

// ---------------------------------------------------------------------
// The state.
// ---------------------------------------------------------------------

/// The app's state: the Miller columns (directory, cursor, listing,
/// preview, sort, filter, find, line mode, search) and what Yazi keeps
/// beside them, each a `files` component. All of it is `Copy`, so any
/// handler takes a copy.
#[derive(Clone, Copy)]
struct Fm {
    m: Miller,
    icons: bool,
    /// The tabs, each with its directory and back/forward history.
    tabs: FileTabs,
    /// Space, Ctrl+A, Ctrl+R, v and V.
    picked: MultiSelect<PathBuf>,
    yanked: Signal<Option<Yank>>,
    /// The directories visited, ranked as zoxide ranks them for Z, kept
    /// between sessions.
    zoxide: Frecency,
    jobs: Tasks,
}

impl Fm {
    fn new(start: PathBuf, icons: bool) -> Fm {
        let m = miller(start).icons(icons);
        // A failed job stays in the task manager and is told once, in a
        // notification, as Yazi's tasks report one.
        let jobs = tasks().on_fail(|job, cx| {
            let errors = match &job.state {
                intuituive::files::JobState::Failed(errors) => errors.join("\n"),
                _ => String::new(),
            };
            cx.notify(Level::Error, escape(&job.title), escape(&errors));
        });
        let fm = Fm {
            m,
            icons,
            tabs: file_tabs(m),
            picked: selection(m),
            yanked: signal(None),
            zoxide: frecency().follow(m),
            jobs,
        };
        // Visual mode is a tab's (Yazi keeps a mode per tab): another tab
        // is not in it, even at the same directory.
        let (active, picked) = (fm.tabs.active(), fm.picked);
        watch(move || active.get(), move |_, _| picked.cancel_visual());
        fm
    }

    fn cwd(&self) -> PathBuf {
        self.m.cwd.get_untracked()
    }

    /// What an action acts on: a visual range ended (and kept) first,
    /// then the selection, else the hovered entry.
    fn targets(&self) -> Vec<PathBuf> {
        self.picked.end_visual();
        self.m.targets()
    }

    /// Run a file operation as a task; then read the directories again.
    /// A failure is the task list's `on_fail`.
    fn job(&self, op: Op) {
        let m = self.m;
        self.jobs.run(op, move |_, _| m.refresh());
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
    Shell(bool),
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
    Search(SearchKind),
    SearchCancel,
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

/// Keys (as bindings name them; the help says them in Vim's notation),
/// with Yazi's descriptions, and what they do.
type Binding = (&'static str, &'static str, Act);

/// The single keys of the manager, each name in `keys` one way to press
/// it.
#[rustfmt::skip]
const KEYS: &[Binding] = &[
    ("esc", "Exit visual mode, clear selection, or cancel search", Act::Escape),
    ("q", "Quit the process", Act::Quit),
    ("ctrl+c", "Close the current tab, or quit if it's last", Act::Close),
    ("k up", "Previous file", Act::Arrow(-1)),
    ("j down", "Next file", Act::Arrow(1)),
    ("G end", "Go to bottom", Act::Bottom),
    ("home", "Go to top", Act::Top),
    ("ctrl+u shift+pageup", "Move cursor up half page", Act::Page(-0.5)),
    ("ctrl+d shift+pagedown", "Move cursor down half page", Act::Page(0.5)),
    ("ctrl+b pageup", "Move cursor up one page", Act::Page(-1.0)),
    ("ctrl+f pagedown", "Move cursor down one page", Act::Page(1.0)),
    ("h left", "Back to the parent directory", Act::Leave),
    ("l right", "Enter the child directory", Act::Enter),
    ("H", "Back to previous directory", Act::Back),
    ("L", "Forward to next directory", Act::Forward),
    ("space", "Toggle the current selection state", Act::Toggle),
    ("ctrl+a", "Select all files", Act::SelectAll),
    ("ctrl+r", "Invert selection of all files", Act::Invert),
    ("v", "Enter visual mode (selection mode)", Act::Visual(false)),
    ("V", "Enter visual mode (unset mode)", Act::Visual(true)),
    ("K", "Seek up 5 units in the preview", Act::Seek(-5)),
    ("J", "Seek down 5 units in the preview", Act::Seek(5)),
    ("tab", "Spot hovered file", Act::Spot),
    ("o enter", "Open selected files", Act::Open(false)),
    ("O shift+enter", "Open selected files interactively", Act::Open(true)),
    ("y", "Yank selected files (copy)", Act::Yank(false)),
    ("x", "Yank selected files (cut)", Act::Yank(true)),
    ("p", "Paste yanked files", Act::Paste(false)),
    ("P", "Paste yanked files (overwrite if the destination exists)", Act::Paste(true)),
    ("Y X", "Cancel the yank status", Act::Unyank),
    ("d", "Trash selected files", Act::Remove(false)),
    ("D", "Permanently delete selected files", Act::Remove(true)),
    ("a", "Create a file (ends with / for directories)", Act::Create),
    ("r", "Rename selected file(s)", Act::Rename),
    ("semicolon", "Run a shell command", Act::Shell(false)),
    ("colon", "Run a shell command (block until finishes)", Act::Shell(true)),
    (".", "Toggle the visibility of hidden files", Act::Hidden),
    ("s", "Search files by name via fd", Act::Search(SearchKind::Name)),
    ("S", "Search files by content via ripgrep", Act::Search(SearchKind::Grep)),
    ("ctrl+s", "Cancel the ongoing search", Act::SearchCancel),
    ("z", "Jump to a file/directory via fzf", Act::Jump),
    ("Z", "Jump to a directory via zoxide", Act::Zoxide),
    ("w", "Show task manager", Act::Tasks),
    ("f", "Filter files", Act::Filter),
    ("/", "Find next file", Act::Find(false)),
    ("?", "Find previous file", Act::Find(true)),
    ("n", "Next found", Act::FindArrow(false)),
    ("N", "Previous found", Act::FindArrow(true)),
    ("[", "Switch to previous tab", Act::TabStep(-1)),
    ("]", "Switch to next tab", Act::TabStep(1)),
    ("{", "Swap current tab with previous tab", Act::TabSwap(-1)),
    ("}", "Swap current tab with next tab", Act::TabSwap(1)),
    ("~ f1", "Open help", Act::Help),
];

/// The key sequences: keys pressed one after another.
#[rustfmt::skip]
const SEQUENCES: &[Binding] = &[
    ("g g", "Go to top", Act::Top),
    ("g h", "Go home", Act::Home),
    ("g c", "Go to ~/.config", Act::Go(".config")),
    ("g d", "Go to ~/Downloads", Act::Go("Downloads")),
    ("g space", "Jump interactively", Act::GoInteractive),
    ("g f", "Follow hovered symlink", Act::Follow),
    ("comma m", "Sort by modified time", Act::Sort(SortBy::Mtime, false)),
    ("comma M", "Sort by modified time (reverse)", Act::Sort(SortBy::Mtime, true)),
    ("comma b", "Sort by birth time", Act::Sort(SortBy::Btime, false)),
    ("comma B", "Sort by birth time (reverse)", Act::Sort(SortBy::Btime, true)),
    ("comma e", "Sort by extension", Act::Sort(SortBy::Extension, false)),
    ("comma E", "Sort by extension (reverse)", Act::Sort(SortBy::Extension, true)),
    ("comma a", "Sort alphabetically", Act::Sort(SortBy::Alphabetical, false)),
    ("comma A", "Sort alphabetically (reverse)", Act::Sort(SortBy::Alphabetical, true)),
    ("comma n", "Sort naturally", Act::Sort(SortBy::Natural, false)),
    ("comma N", "Sort naturally (reverse)", Act::Sort(SortBy::Natural, true)),
    ("comma s", "Sort by size", Act::Sort(SortBy::Size, false)),
    ("comma S", "Sort by size (reverse)", Act::Sort(SortBy::Size, true)),
    ("comma r", "Sort randomly", Act::Sort(SortBy::Random, false)),
    ("m s", "Linemode: size", Act::Linemode(Linemode::Size)),
    ("m p", "Linemode: permissions", Act::Linemode(Linemode::Permissions)),
    ("m b", "Linemode: btime", Act::Linemode(Linemode::Btime)),
    ("m m", "Linemode: mtime", Act::Linemode(Linemode::Mtime)),
    ("m o", "Linemode: owner", Act::Linemode(Linemode::Owner)),
    ("m n", "Linemode: none", Act::Linemode(Linemode::None)),
    ("c c", "Copy file path", Act::CopyPath),
    ("c d", "Copy directory path", Act::CopyDir),
    ("c f", "Copy filename", Act::CopyName),
    ("c n", "Copy filename without extension", Act::CopyStem),
    ("t t", "Create a new tab in CWD", Act::TabNew),
    ("t r", "Rename current tab", Act::TabRename),
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
            // the find, the search.
            if fm.picked.in_visual() {
                fm.picked.end_visual();
            } else if !fm.picked.is_empty() {
                fm.picked.clear();
            } else if !m.current.filter.with_untracked(String::is_empty) {
                m.current.filter.set(String::new());
            } else if m.find.with_untracked(Option::is_some) {
                m.find.set(None);
            } else {
                m.end_search();
            }
        }
        Act::Quit => quit(fm, cx),
        Act::Close => {
            if !fm.tabs.close_active() {
                quit(fm, cx);
            }
        }
        Act::Arrow(by) => m.arrow(by),
        Act::Top => m.move_to(0),
        Act::Bottom => m.move_to(isize::MAX),
        Act::Page(share) => m.page(share),
        Act::Leave => m.leave(),
        Act::Enter => m.enter(),
        Act::Back => {
            fm.tabs.back();
        }
        Act::Forward => {
            fm.tabs.forward();
        }
        Act::Toggle => {
            if let Some(path) = m.hovered_path() {
                fm.picked.toggle(path);
                m.arrow(1);
            }
        }
        Act::SelectAll => fm.picked.select_all(),
        Act::Invert => fm.picked.invert(),
        Act::Visual(unset) => {
            if fm.picked.in_visual() {
                fm.picked.end_visual();
            } else if !m.current.is_empty() {
                fm.picked.visual(unset);
            }
        }
        Act::Seek(by) => m
            .preview_top
            .update(|t| *t = (*t as i32 + by as i32).max(0) as u16),
        Act::Spot => spotter(fm, cx),
        Act::Open(interactive) => open(fm, interactive, cx),
        Act::Yank(cut) => {
            let paths = fm.targets();
            if !paths.is_empty() {
                fm.yanked.set(Some(if cut {
                    Yank::cut(paths)
                } else {
                    Yank::copy(paths)
                }));
                fm.picked.clear();
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
        Act::Rename => rename(fm, cx),
        Act::Shell(block) => {
            let paths = fm.targets();
            let options = ShellOptions::new()
                .block(block)
                .hovered(m.hovered_path())
                .tasks(fm.jobs)
                .prompt(input_look())
                .done(move |_| m.refresh());
            shell(cx, paths, options);
        }
        Act::Hidden => m.hidden.update(|h| *h = !*h),
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
                .on_change(move |typed, _| m.current.filter.set(typed.to_string()))
                .on_cancel(move |_| m.current.filter.set(before));
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
                    // From where the find started, its own entry first.
                    m.cursor.set(from);
                    m.find.set((!typed.is_empty()).then(|| typed.to_string()));
                    let here = m
                        .hovered
                        .get_untracked()
                        .is_some_and(|e| smart_contains(&e.name, typed));
                    if !here {
                        m.find_next(previous);
                    }
                })
                .on_cancel(move |_| {
                    m.find.set(None);
                    m.cursor.set(from);
                });
            cx.prompt_with(title, "", options, |_, _| {});
        }
        Act::FindArrow(previous) => {
            m.find_next(previous);
        }
        Act::Search(kind) => {
            let title = match kind {
                SearchKind::Name => "Search via fd:",
                SearchKind::Grep => "Search via ripgrep:",
            };
            let root = fm.cwd();
            cx.prompt_with(title, "", input_look(), move |typed, _| {
                if !typed.is_empty() {
                    m.show_search(search(&root, &typed, kind));
                }
            });
        }
        Act::SearchCancel => {
            if let Some(found) = m.searching() {
                found.cancel();
            }
        }
        Act::Sort(by, reverse) => {
            m.sort.set(Sort::by(by).reverse(reverse));
            // As Yazi's keymap: sorting by size or time shows it.
            match by {
                SortBy::Size => m.linemode.set(Linemode::Size),
                SortBy::Mtime => m.linemode.set(Linemode::Mtime),
                SortBy::Btime => m.linemode.set(Linemode::Btime),
                _ => {}
            }
        }
        Act::Home => {
            if let Some(home) = home_dir() {
                m.cd(home, None);
            }
        }
        Act::Go(rel) => match home_dir().map(|h| h.join(rel)).filter(|d| d.is_dir()) {
            Some(dir) => m.cd(dir, None),
            None => cx.notify(Level::Warn, "Cd", format!("~/{rel} does not exist")),
        },
        Act::GoInteractive => {
            let here = fm.cwd();
            cx.prompt_with("Change directory:", "", input_look(), move |typed, cx| {
                let typed = typed.trim();
                let target = match typed.strip_prefix('~') {
                    Some(rest) => home_dir().map(|h| h.join(rest.trim_start_matches('/'))),
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
        Act::TabNew => {
            fm.tabs.open();
        }
        Act::TabRename => {
            let now = fm.tabs.current().title();
            cx.prompt_with("Rename tab:", now, input_look(), move |typed, _| {
                let name = (!typed.trim().is_empty()).then(|| typed.trim().to_string());
                fm.tabs.rename(name);
            });
        }
        Act::TabSwitch(to) => {
            // Yazi ignores a tab that is not there.
            if to < fm.tabs.set().len() {
                fm.tabs.switch(to);
            }
        }
        Act::TabStep(by) => fm.tabs.cycle(by),
        Act::TabSwap(by) => fm.tabs.swap(by),
        Act::Help => cx.help_with(
            HelpOptions::new()
                .title("mgr.help")
                .key_format(KeyFormat::Vim)
                .filter_first(true),
        ),
    }
}

/// `r`: rename the hovered entry in a prompt below it, the caret before
/// the extension; with a selection, rename them all in `$EDITOR` (Yazi's
/// bulk rename).
fn rename(fm: Fm, cx: &mut Ctx) {
    let m = fm.m;
    fm.picked.end_visual();
    let chosen = fm.picked.selected();
    if !chosen.is_empty() {
        let picked = fm.picked;
        return bulk_rename(cx, chosen, move |_, _| {
            picked.clear();
            m.refresh();
        });
    }
    let Some(entry) = m.hovered.get_untracked() else {
        return;
    };
    let dir = fm.cwd();
    let old = entry.name.clone();
    // Yazi's `rename --cursor=before_ext`: the caret before the extension
    // (a directory's name has none).
    let stem = match Path::new(&old).extension() {
        Some(ext) if !entry.dir => old.chars().count() - ext.to_string_lossy().chars().count() - 1,
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
        fm.job(yank.paste(into, conflict));
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
        fm.picked.clear();
        fm.job(if permanently {
            Op::Delete(paths)
        } else {
            Op::Trash(paths)
        });
    });
}

/// Run `opener` on `paths`, extraction as tasks.
fn launch(fm: Fm, opener: &Opener, paths: Vec<PathBuf>, cx: &mut Ctx) {
    let m = fm.m;
    open_with_tasks(cx, opener, paths, fm.jobs, move |_| m.refresh());
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
// Pop-ups: spot, tasks, jumps, the menu.
// ---------------------------------------------------------------------

/// Tab: Yazi's spotter, what is known about the hovered file.
fn spotter(fm: Fm, cx: &mut Ctx) {
    let Some(path) = fm.m.hovered_path() else {
        return;
    };
    let height = spot_facts(&path).len() as u16 + 2;
    cx.modal(Size::Fixed(60), Size::Fixed(height), move || {
        spot(&path)
            .key_style("blue")
            .padding(0, 1)
            .panel("Spot")
            .title_align(Align::Center)
            .on_key("esc q tab enter", |cx| cx.pop())
    });
}

/// w: Yazi's task manager, the file jobs with their progress; x cancels
/// one, Esc or w closes it. A failed job's row leaves out its title, which
/// the notification gave.
fn task_manager(fm: Fm, cx: &mut Ctx) {
    let jobs = fm.jobs;
    cx.modal(Size::Percent(80), Size::Percent(50), move || {
        tasks_panel_with(jobs, JobsPanelOptions::new().failed_title(false))
            .autofocus()
            .padding(0, 1)
            .panel("Tasks")
            .title_align(Align::Center)
            .on_key("esc w q", |cx| cx.pop())
    });
}

/// How the z and Z pickers look: the same fuzzy list.
fn picker_look() -> FuzzyOptions {
    FuzzyOptions::new().border(Border::Rounded)
}

/// z: find a file or directory under this one (Yazi asks fzf over fd),
/// with the `files` finder: its walk streams in while you type, Enter
/// goes there, Esc closes it.
fn jump(fm: Fm, cx: &mut Ctx) {
    let tree = walk(fm.cwd()).hidden(fm.m.hidden.get_untracked());
    cx.modal(Size::Percent(80), Size::Percent(70), move || {
        finder_with(
            tree,
            move |path, cx| {
                cx.pop();
                reveal(fm, &path);
            },
            picker_look(),
        )
    });
}

/// Z: jump to a directory visited, best first by frecency (Yazi asks
/// zoxide), kept between sessions; directories gone since are forgotten.
fn zoxide(fm: Fm, cx: &mut Ctx) {
    let (store, here) = (fm.zoxide, fm.cwd());
    let ranked: Vec<PathBuf> = store
        .ranked()
        .into_iter()
        .filter(|dir| {
            let there = dir.is_dir();
            if !there {
                store.remove(dir);
            }
            there && *dir != here
        })
        .collect();
    cx.modal(Size::Percent(80), Size::Percent(70), move || {
        fuzzy_list_with(
            FuzzySource::iter(ranked),
            |dir| escape(&dir.display().to_string()),
            move |dir, cx| {
                cx.pop();
                fm.m.cd(dir, None);
            },
            picker_look(),
        )
    });
}

/// Go to `path`'s directory, hovering it.
fn reveal(fm: Fm, path: &Path) {
    if let Some(dir) = path.parent() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        fm.m.cd(dir.to_path_buf(), name);
    }
}

/// What a right click (or the menu key) on an entry offers: below the
/// pointer, else below the hovered row.
fn menu(fm: Fm, cx: &mut Ctx) {
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
    let row = fm.m.row_rects().get(fm.m.cursor.get_untracked());
    match (cx.pointer(), row) {
        (None, Some(row)) => open_menu(cx, row, Placement::Below, items),
        _ => context_menu(cx, items),
    }
}

// ---------------------------------------------------------------------
// The screen.
// ---------------------------------------------------------------------

/// `path` as Yazi writes it: `~` for the home directory.
fn readable_path(path: &Path) -> String {
    path_parts(path, home_dir().as_deref()).join(std::path::MAIN_SEPARATOR_STR)
}

/// The top row: the directory (cut from the left when it is long) with
/// the filter, find and search in brackets, and at the right how many
/// files are selected or yanked.
fn header(fm: Fm) -> Node {
    let count = text(move || {
        let selected = fm.picked.len();
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
        if let Some(found) = fm.m.searching() {
            flags.push(format!("search: {}", found.query()));
        }
        if flags.is_empty() {
            return String::new();
        }
        format!("[cyan] ({})[/]", escape(&flags.join(", ")))
    };
    let cwd = path_crumb(move || fm.m.cwd.get())
        .crumb_suffix(flags)
        .flex(1);
    row([cwd, count]).fixed(1)
}

/// The tab strip, shown once there is more than one tab: Yazi's, numbered,
/// the active tab on blue, the titles cut to share the width. A click
/// switches to a tab.
fn tab_strip(fm: Fm) -> Node {
    let set = fm.tabs.set();
    let titles = move || {
        set.tabs().with(|tabs| {
            tabs.iter()
                .enumerate()
                .map(|(i, tab)| escape(&format!(" {} {} ", i + 1, tab.title())))
                .collect()
        })
    };
    tabs(titles, fm.tabs.active())
        .fit()
        .auto_hide()
        .divider("")
        .tab_padding("", "")
        .no_focus()
        .fixed(1)
}

/// The status line, as Yazi's: the mode, the hovered entry's size and
/// name on the left; the tasks' progress (while tasks run), its
/// permissions, where the cursor is as a share and as a count on the
/// right. Rounded caps with icons on.
fn status(fm: Fm) -> Node {
    let running = move || fm.jobs.summary().1 > 0;
    switch(running, move |running| status_line(fm, running)).fixed(1)
}

fn status_line(fm: Fm, progress: bool) -> Node {
    let (m, picked) = (fm.m, fm.picked);
    let main = move || match picked.in_visual() {
        true => "bold on red".to_string(),
        false => "bold on blue".to_string(),
    };
    let alt = move || match picked.in_visual() {
        true => "red on white".to_string(),
        false => "blue on white".to_string(),
    };
    let mode = seg(move || {
        let mode = match (picked.in_visual(), picked.visual_unsets()) {
            (true, false) => "SEL",
            (true, true) => "UNS",
            _ => "NOR",
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
    let mut right = Vec::new();
    if progress {
        // Yazi's progress: how far the running tasks have got and how
        // many are left, red once one has failed.
        let jobs = fm.jobs;
        let gauge = seg_gauge(
            move || jobs.summary().0,
            move || {
                let (share, running, _) = jobs.summary();
                format!("{:>3}%, {running} left", (share * 100.0).floor() as u32)
            },
        )
        .width(21)
        .style_with(move || match jobs.summary().2 {
            0 => "status_bar.gauge".to_string(),
            _ => "bright_white on red".to_string(),
        });
        right.push(gauge);
        right.push(seg(|| " ".into()));
    }
    right.extend([perm, percent, position]);
    status_bar([mode, size, name], right).caps(if fm.icons { Caps::Rounded } else { Caps::None })
}

/// The Miller columns, with the selection's and the yank's markers, git
/// signs, a binary file as `file` classifies it, the hovered row's
/// rounded ends with icons on, and the menu on a right click.
fn columns(fm: Fm) -> Node {
    let mut options = MillerOptions::new()
        .markers(Yank::markers(fm.yanked))
        .git(git_status(fm.m.current))
        .binary(BinaryPreview::FileType)
        .on_context(move |_, cx| menu(fm, cx));
    if fm.icons {
        options = options.hovered_caps("\u{e0b6}", "\u{e0b4}");
    }
    fm.m.view_with(options)
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
/// notification colours, the progress gauge blue.
fn theme() -> Theme {
    let parse = |s: &str| Style::parse(s).expect("a style");
    let mut theme = Theme::dark()
        .style("selected", parse("reverse"))
        .style("split.divider", parse("white"))
        .style("split.divider.hover", parse("bold bright_white"))
        .style("tabs.selected", parse("bold on blue"))
        .style("tabs.selected.blur", parse("bold on blue"))
        .style("tabs.unselected", parse("blue on white"))
        .style("breadcrumb", parse("cyan"))
        .style("breadcrumb.last", parse("cyan"))
        .style("breadcrumb.separator", parse("cyan"))
        .style("popup.title", parse("blue"))
        .style("which_key.key", parse("bright_cyan"))
        .style("which_key.sep", parse("bright_black"))
        .style("which_key.desc", parse("bright_magenta"))
        .style("notify.info", parse("green"))
        .style("notify.warn", parse("yellow"))
        .style("notify.error", parse("red"))
        .style("status_bar.gauge", parse("bright_white on blue"))
        .style("status_bar.gauge.rest", parse("bright_white on black"));
    theme.border = parse("blue");
    theme.border_focused = parse("blue");
    theme.title = Style::default();
    theme
}

/// The app, starting in `start`, with Nerd Font icons if `icons`.
pub fn files_app_with(start: PathBuf, icons: bool) -> App {
    App::new(move || {
        let fm = Fm::new(absolute(start), icons);
        // The column holds Yazi's keymap (its `mgr` section) and sees each
        // key before the focused list, whose own j/k/g it overrides.
        column([header(fm), tab_strip(fm), columns(fm).flex(1), status(fm)])
            .previews_keys()
            .section("mgr", |mut root| {
                for &(keys, desc, act) in KEYS {
                    root = root.bind(keys, desc, move |cx| run(fm, act, cx));
                }
                for &(keys, desc, act) in SEQUENCES {
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
