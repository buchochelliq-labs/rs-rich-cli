//! The gitui rebuild (examples/git.rs), on a repository made for each
//! test with the `git` command line: its status lists, its diff with hunk
//! and line staging, the commit popup, the log, the branches and the
//! stashes, driven by gitui's keys.

mod common;

#[path = "../examples/git.rs"]
#[allow(dead_code)]
mod git;

use std::path::{Path, PathBuf};
use std::process::Command;

use git::{git_app, hunk_patch, FileTree, Git, Kind};
use intuituive::{App, Driver};

/// No global or system git configuration: what the tests see is the
/// repository's own.
fn isolate() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_EDITOR", "true");
    });
}

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_DATE", "2024-03-01T12:00:00Z")
        .env("GIT_COMMITTER_DATE", "2024-03-01T12:00:00Z")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const LINES: &str = "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\neleven\ntwelve\n";

/// A repository with two commits on `main`, a tag, a `feature` branch, and
/// changes: `src/lib.rs` changed in two places (two hunks), `notes.md`
/// new and untracked, `staged.txt` added to the index.
fn repo(name: &str) -> PathBuf {
    isolate();
    let root = std::env::temp_dir().join(format!("intuituive-git-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    sh(&root, &["init", "-q", "-b", "main"]);
    sh(&root, &["config", "user.name", "Ada Lovelace"]);
    sh(&root, &["config", "user.email", "ada@example.com"]);
    write(&root, "README.md", "# demo\n");
    sh(&root, &["add", "."]);
    sh(&root, &["commit", "-q", "-m", "Initial commit"]);
    write(&root, "src/lib.rs", LINES);
    sh(&root, &["add", "."]);
    sh(&root, &["commit", "-q", "-m", "Add the library"]);
    sh(&root, &["tag", "v0.1"]);
    sh(&root, &["branch", "feature"]);
    write(
        &root,
        "src/lib.rs",
        &LINES
            .replace("two\n", "TWO\n")
            .replace("eleven\n", "ELEVEN\n"),
    );
    write(&root, "notes.md", "a note\n");
    write(&root, "staged.txt", "ready\n");
    sh(&root, &["add", "staged.txt"]);
    root
}

fn app(root: &Path) -> App {
    git_app(Git::open(root).expect("a repository"))
}

fn driver(root: &Path) -> Driver {
    let mut driver = app(root).driver(100, 24);
    driver.settle();
    driver
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn dump(driver: &Driver) -> String {
    rows(driver).join("\n")
}

fn screen_has(driver: &Driver, needle: &str) -> bool {
    rows(driver).iter().any(|r| r.contains(needle))
}

/// The row containing `needle`, or a failure showing the screen.
fn row_with(driver: &Driver, needle: &str) -> String {
    let rows = rows(driver);
    rows.iter()
        .find(|r| r.contains(needle))
        .cloned()
        .unwrap_or_else(|| panic!("no {needle:?} on\n{}", rows.join("\n")))
}

fn cached(root: &Path) -> String {
    sh(root, &["diff", "--cached", "--", "src/lib.rs"])
}

fn worktree(root: &Path) -> String {
    sh(root, &["diff", "--", "src/lib.rs"])
}

/// The selected file of the unstaged list is `src/lib.rs` (rows: notes.md,
/// src, lib.rs), and its diff is focused.
fn on_lib_diff(root: &Path) -> Driver {
    let mut d = driver(root);
    d.press("down down");
    assert!(screen_has(&d, "Diff: src/lib.rs"), "{}", dump(&d));
    d.press("right");
    d
}

#[test]
fn the_status_tab_shows_both_trees_and_the_diff() {
    let root = repo("status");
    let d = driver(&root);
    let top = row_with(&d, "Status [1]");
    assert!(top.contains("Status [1]  |  Log [2]  |  Files [3]  |  Stashing [4]  |  Stashes [5]"));
    assert!(row_with(&d, "Unstaged Changes").contains("Diff: notes.md"));
    // gitui's status letter first, before the indent (`TreeItem::prefix`).
    assert!(screen_has(&d, "+   notes.md"), "{}", dump(&d));
    assert!(screen_has(&d, "  ▾ src"));
    assert!(screen_has(&d, "M     lib.rs"), "{}", dump(&d));
    assert!(row_with(&d, "Staged Changes").starts_with("┌Staged Changes"));
    assert!(screen_has(&d, "+   staged.txt"));
    // The branch on the unstaged list's bottom edge, while it is focused.
    assert!(row_with(&d, "{main}").starts_with("└"), "{}", dump(&d));
    // An untracked file's diff, and gitui's hunk markers.
    assert!(screen_has(&d, "┌@@ -0,0 +1 @@"));
    assert!(screen_has(&d, "└+a note"));
    // The key bar, in gitui's look: the focused list's keys first.
    assert!(rows(&d)
        .last()
        .unwrap()
        .starts_with("Stage [⏎] Stage All [a] Reset [D]"));
}

#[test]
fn enter_stages_a_file_and_unstages_it() {
    let root = repo("stage");
    let mut d = driver(&root);
    d.press("down down enter");
    assert!(cached(&root).contains("+TWO"), "staged");
    assert!(worktree(&root).is_empty());
    // lib.rs left the unstaged list for the staged one.
    let staged_row = rows(&d)
        .iter()
        .position(|r| r.contains("Staged Changes"))
        .unwrap();
    let lib_row = rows(&d)
        .iter()
        .rposition(|r| r.contains("M     lib.rs"))
        .unwrap();
    assert!(lib_row > staged_row, "{}", dump(&d));
    // w to the staged list: its selection stays on staged.txt (kept by
    // key), under the src folder and lib.rs.
    d.press("w");
    assert!(row_with(&d, "Staged Changes").contains("{main}"));
    assert!(rows(&d).last().unwrap().starts_with("Unstage [⏎]"));
    d.press("up enter");
    assert!(cached(&root).is_empty(), "unstaged again");
    assert!(worktree(&root).contains("+TWO"));
}

#[test]
fn enter_on_a_folder_stages_what_is_in_it() {
    let root = repo("folder");
    write(&root, "src/more.rs", "fn more() {}\n");
    let mut d = driver(&root);
    d.press("down enter");
    let staged = sh(&root, &["diff", "--cached", "--name-only"]);
    assert_eq!(
        staged.lines().collect::<Vec<_>>(),
        ["src/lib.rs", "src/more.rs", "staged.txt"]
    );
}

#[test]
fn a_stages_everything_and_d_resets_a_file_after_asking() {
    let root = repo("all");
    let mut d = driver(&root);
    d.press("down down D");
    assert!(screen_has(&d, "confirm file reset?"), "{}", dump(&d));
    d.press("y");
    assert!(worktree(&root).is_empty(), "reset");
    d.press("a");
    let staged = sh(&root, &["diff", "--cached", "--name-only"]);
    assert_eq!(
        staged.lines().collect::<Vec<_>>(),
        ["notes.md", "staged.txt"]
    );
}

#[test]
fn the_diff_moves_by_line_and_by_hunk() {
    let root = repo("hunks");
    let mut d = on_lib_diff(&root);
    assert!(rows(&d).last().unwrap().contains("Back"), "the diff's keys");
    assert!(screen_has(&d, "┌@@ -1,5 +1,5 @@"));
    assert!(screen_has(&d, "┌@@ -8,5 +8,5 @@"));
    assert!(screen_has(&d, "│-two"));
    assert!(screen_has(&d, "│+TWO"));
    assert!(screen_has(&d, "└ five"));
    // n to the second hunk: Enter stages that hunk alone.
    d.press("n enter");
    let staged = cached(&root);
    assert!(
        staged.contains("+ELEVEN") && !staged.contains("+TWO"),
        "{staged}"
    );
    let left = worktree(&root);
    assert!(left.contains("+TWO") && !left.contains("+ELEVEN"), "{left}");
}

#[test]
fn shift_selects_lines_and_s_stages_only_them() {
    let root = repo("lines");
    let mut d = on_lib_diff(&root);
    // Rows: the header, " one", "-two", "+TWO": select "-two" alone.
    d.press("down down s");
    let staged = cached(&root);
    assert!(
        staged.contains("-two") && !staged.contains("+TWO"),
        "{staged}"
    );
    let index = sh(&root, &["show", ":src/lib.rs"]);
    assert!(index.starts_with("one\nthree\n"), "{index}");
    // Unstage it from the staged diff: w goes to the staged list, whose
    // selection stayed on staged.txt (kept by key), under lib.rs.
    d.press("left w up right");
    assert!(screen_has(&d, "Diff: src/lib.rs"), "{}", dump(&d));
    d.press("down down s");
    assert!(cached(&root).is_empty(), "{}", cached(&root));
}

#[test]
fn a_range_of_lines_is_staged_together() {
    let root = repo("range");
    let mut d = on_lib_diff(&root);
    d.press("down down shift+down");
    // Both lines of the change are lit as a selection.
    d.press("s");
    let staged = cached(&root);
    assert!(
        staged.contains("-two") && staged.contains("+TWO"),
        "{staged}"
    );
    assert!(!staged.contains("ELEVEN"), "{staged}");
}

#[test]
fn d_resets_lines_and_shift_d_a_hunk_after_asking() {
    let root = repo("reset");
    let mut d = on_lib_diff(&root);
    d.press("n down down down down d");
    assert!(screen_has(&d, "confirm reset lines?"), "{}", dump(&d));
    d.press("y");
    // "-eleven" (row 3 of the second hunk) put back; "+ELEVEN" kept.
    let text = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert!(text.contains("eleven\nELEVEN\n"), "{text}");
    d.press("home D y");
    let text = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert!(text.starts_with("one\ntwo\n"), "{text}");
}

#[test]
fn y_copies_the_selected_lines() {
    let root = repo("copy");
    let mut d = on_lib_diff(&root);
    d.press("down down shift+down y");
    assert_eq!(d.take_copies(), ["-two\n+TWO"]);
}

#[test]
fn the_commit_popup_commits_the_index() {
    let root = repo("commit");
    let mut d = driver(&root);
    d.press("c");
    let title = row_with(&d, "Commit");
    assert!(title.contains("{main}"), "{title}");
    assert!(screen_has(&d, "type commit message.."));
    assert!(rows(&d).last().unwrap().starts_with("Do Commit [^d]"));
    d.type_text("Add the staged file");
    d.press("ctrl+d");
    assert!(!screen_has(&d, "type commit message"), "closed");
    assert_eq!(
        sh(&root, &["log", "-1", "--format=%s"]).trim(),
        "Add the staged file"
    );
    assert!(!screen_has(&d, "staged.txt"), "{}", dump(&d));
    // Nothing staged now: c is not a key.
    d.press("c");
    assert!(!screen_has(&d, "type commit message"));
}

#[test]
fn a_long_subject_is_warned_and_esc_keeps_the_message() {
    let root = repo("subject");
    let mut d = driver(&root);
    d.press("c");
    d.type_text(&"x".repeat(55));
    assert!(screen_has(&d, "[subject length: 55]"), "{}", dump(&d));
    d.press("esc c");
    assert!(screen_has(&d, &"x".repeat(55)), "kept");
}

#[test]
fn ctrl_a_amends_the_last_commit() {
    let root = repo("amend");
    let mut d = driver(&root);
    d.press("c ctrl+a");
    assert!(screen_has(&d, "Commit (Amend)"), "{}", dump(&d));
    assert!(screen_has(&d, "Add the library"));
    d.press("ctrl+d");
    assert_eq!(sh(&root, &["rev-list", "--count", "HEAD"]).trim(), "2");
    let files = sh(&root, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(files.contains("staged.txt"), "{files}");
}

#[test]
fn ctrl_e_hands_the_message_to_the_editor() {
    let root = repo("editor");
    let mut d = driver(&root);
    d.press("c");
    d.type_text("draft");
    d.press("ctrl+e");
    let handoff = d.take_handoff().expect("the editor");
    let args: Vec<String> = (handoff.command.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(handoff.command.get_program(), "sh");
    assert!(args.last().unwrap().ends_with("COMMIT_EDITMSG"), "{args:?}");
    let file = PathBuf::from(args.last().unwrap());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("draft\n# Edit your commit message"),
        "{text}"
    );
    std::fs::write(&file, "Edited in the editor\n# a comment\n").unwrap();
    d.handed_back(handoff, Ok(Some(0)));
    d.settle();
    assert!(screen_has(&d, "Edited in the editor"), "{}", dump(&d));
    assert!(!screen_has(&d, "a comment"));
}

#[test]
fn the_log_lists_commits_with_their_refs() {
    let root = repo("log");
    let mut d = driver(&root);
    d.press("2");
    assert!(screen_has(&d, "┌Commit 2/2"), "{}", dump(&d));
    let first = row_with(&d, "Add the library");
    assert!(first.contains("2024-03-01 Ada Lovelace"), "{first}");
    assert!(
        first.contains("<v0.1> {feature} {main} Add the library"),
        "{first}"
    );
    assert!(screen_has(&d, "Initial commit"));
    d.press("down");
    assert!(screen_has(&d, "┌Commit 1/2"));
    d.press("y");
    let hash = sh(&root, &["rev-parse", "HEAD~1"]);
    assert_eq!(d.take_copies(), [hash.trim()]);
}

#[test]
fn enter_shows_a_commits_details_and_right_inspects_it() {
    let root = repo("details");
    let mut d = driver(&root);
    d.press("2 enter");
    assert!(screen_has(&d, "┌Info"), "{}", dump(&d));
    assert!(row_with(&d, "Author:").contains("Ada Lovelace"));
    assert!(row_with(&d, "Tags:").contains("v0.1"));
    assert!(row_with(&d, "Subject:").contains("Add the library"));
    assert!(screen_has(&d, "+ src/lib.rs"));
    // The list keeps the selection when the details close.
    d.press("down");
    assert!(row_with(&d, "Subject:").contains("Initial commit"));
    d.press("enter");
    assert!(!screen_has(&d, "Subject:"));
    assert!(screen_has(&d, "┌Commit 1/2"));
    d.press("up right");
    assert!(screen_has(&d, "Diff: src/lib.rs"), "{}", dump(&d));
    assert!(screen_has(&d, "│+one"));
    d.press("esc");
    assert!(screen_has(&d, "┌Commit 2/2"), "{}", dump(&d));
}

#[test]
fn the_branch_list_checks_out_and_creates_branches() {
    let root = repo("branches");
    let mut d = driver(&root);
    d.press("b");
    assert!(screen_has(&d, "Branches"), "{}", dump(&d));
    assert!(screen_has(&d, "Local  |  Remote"));
    let head = row_with(&d, "main");
    assert!(head.contains("*  main"), "{head}");
    // Checkout `feature` (the first row), with the changes kept.
    d.press("up enter");
    assert_eq!(sh(&root, &["branch", "--show-current"]).trim(), "feature");
    assert!(screen_has(&d, "{feature}"), "{}", dump(&d));
    d.press("b c");
    d.type_text("topic");
    d.press("enter");
    assert_eq!(sh(&root, &["branch", "--show-current"]).trim(), "topic");
}

#[test]
fn stashing_saves_the_changes_and_stashes_pops_them() {
    let root = repo("stash");
    let mut d = driver(&root);
    d.press("4");
    assert!(screen_has(&d, "┌Files to Stash"));
    assert!(screen_has(&d, "[x] stash untracked"));
    assert!(screen_has(&d, "[_] keep index"));
    d.press("s");
    d.type_text("wip");
    d.press("enter");
    assert!(sh(&root, &["status", "--porcelain"]).is_empty(), "stashed");
    d.press("5");
    assert!(screen_has(&d, "┌Stashes 1/1"), "{}", dump(&d));
    assert!(screen_has(&d, "On main: wip"));
    d.press("p");
    assert!(!sh(&root, &["status", "--porcelain"]).is_empty(), "popped");
    assert!(screen_has(&d, "┌Stashes 0/0"));
}

#[test]
fn tabs_switch_by_number_and_tab_and_h_shows_the_help() {
    let root = repo("tabs");
    let mut d = driver(&root);
    d.press("tab");
    assert!(screen_has(&d, "┌Commit 2/2"));
    d.press("shift+tab shift+tab");
    assert!(screen_has(&d, "┌Stashes 0/0"));
    d.press("3");
    assert!(screen_has(&d, "┌Files at HEAD"));
    assert!(
        row_with(&d, "Files at HEAD").contains("┐┌README.md"),
        "{}",
        dump(&d)
    );
    assert!(screen_has(&d, "# demo"), "{}", dump(&d));
    d.press("1 h");
    assert!(screen_has(&d, "Help: all commands"), "{}", dump(&d));
    assert!(screen_has(&d, "Stage All"));
    d.press("esc q");
    assert!(d.is_done());
}

#[test]
fn a_failure_is_shown_in_an_error_box() {
    let root = repo("error");
    let mut d = driver(&root);
    d.press("b");
    // Renaming `feature` onto `main`, which exists, fails.
    d.press("up r");
    for _ in 0.."feature".len() {
        d.press("backspace");
    }
    d.type_text("main");
    d.press("enter");
    assert!(screen_has(&d, "Error"), "{}", dump(&d));
    assert!(screen_has(&d, "already exists"), "{}", dump(&d));
}

#[test]
fn a_hunk_patch_keeps_the_lines_left_out_as_git_needs() {
    let text = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
    let patch = rich_ext::diff::git::parse_unified(text).unwrap();
    let file = &patch.files[0];
    // Staging only the removal: the addition is dropped.
    let only_removed = hunk_patch(file, 0, |l| l == 1, false).unwrap();
    assert!(
        only_removed.ends_with("@@ -1,3 +1,2 @@\n a\n-b\n c\n"),
        "{only_removed}"
    );
    // Unstaging only the addition: the removal is dropped, as `-R` reads it.
    let only_added = hunk_patch(file, 0, |l| l == 2, true).unwrap();
    assert!(
        only_added.ends_with("@@ -1,2 +1,3 @@\n a\n+B\n c\n"),
        "{only_added}"
    );
    // Unstaging only the removal: the addition is context, on both sides.
    let removed_back = hunk_patch(file, 0, |l| l == 1, true).unwrap();
    assert!(
        removed_back.ends_with("@@ -1,4 +1,3 @@\n a\n-b\n B\n c\n"),
        "{removed_back}"
    );
    assert!(
        hunk_patch(file, 0, |l| l == 0, false).is_none(),
        "no change selected"
    );
}

#[test]
fn a_file_tree_folds_folders_with_one_folder_in_them() {
    let file = |path: &str| git::FileEntry {
        path: path.into(),
        kind: Kind::Modified,
        untracked: false,
    };
    let files = [file("a/b/c/x.rs"), file("a/b/c/y.rs"), file("z.rs")];
    let tree = FileTree::of(&files);
    assert_eq!(tree.items.len(), 2);
    assert_eq!(tree.items[0].label, "a/b/c");
    assert_eq!(tree.items[0].children.len(), 2);
    // Rows by key: a folded folder's is its whole path.
    assert_eq!(tree.rows[0], ("a/b/c".into(), None, vec![0, 1]));
    assert_eq!(tree.rows[2], ("a/b/c/y.rs".into(), Some(1), vec![1]));
    assert_eq!(tree.rows[3], ("z.rs".into(), Some(2), vec![2]));
}

#[test]
fn the_layout_follows_the_focus_and_period_expands_the_key_bar() {
    let root = repo("layout");
    let mut d = driver(&root);
    // The unstaged list 60%, the staged one 40%, of the left half.
    let at = |d: &Driver, title: &str| rows(d).iter().position(|r| r.contains(title)).unwrap();
    assert_eq!(at(&d, "Staged Changes"), 14, "{}", dump(&d));
    // 40/60 while the staged list is focused.
    d.press("w");
    assert_eq!(at(&d, "Staged Changes"), 10, "{}", dump(&d));
    // The diff alone while it has the focus.
    d.press("right");
    assert!(
        row_with(&d, "Diff: staged.txt").starts_with("┌Diff"),
        "{}",
        dump(&d)
    );
    assert!(!screen_has(&d, "Staged Changes"));
    d.press("left");
    assert!(screen_has(&d, "Staged Changes"));
    // `more [.]` where the bar is cut; `.` shows the rest on more rows.
    assert!(
        rows(&d).last().unwrap().ends_with("more [.]"),
        "{}",
        dump(&d)
    );
    d.press("period");
    assert!(screen_has(&d, "Quit [q]"), "{}", dump(&d));
}

#[test]
fn a_folder_closed_stays_closed_when_the_files_change() {
    let root = repo("closed");
    let mut d = driver(&root);
    // Close src (row 2), then a new file appears beside it: the tree
    // keeps its open folders by key, and src stays closed.
    d.press("down left");
    assert!(screen_has(&d, "▸ src"), "{}", dump(&d));
    write(&root, "other.txt", "x\n");
    d.advance(std::time::Duration::from_secs(6));
    d.settle();
    assert!(screen_has(&d, "other.txt"), "{}", dump(&d));
    assert!(screen_has(&d, "▸ src"), "{}", dump(&d));
    assert!(!screen_has(&d, "lib.rs"), "{}", dump(&d));
}
