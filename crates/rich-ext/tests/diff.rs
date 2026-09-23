//! The diff engine, views, git patches, test reports and assertions.
use std::path::{Path, PathBuf};
use std::process::Command;

use rich::{ColorSystem, Console, Renderable, Segment};
use rich_ext::diagnostic::Level;
use rich_ext::diff::git::*;
use rich_ext::diff::*;

fn plain(r: &dyn Renderable, w: usize) -> String {
    let c = Console::builder()
        .width(w)
        .height(200)
        .no_color(true)
        .build();
    r.rich_render(&c, &c.options())
        .iter()
        .map(|s| s.text.as_str())
        .collect()
}

fn colour_segments(r: &dyn Renderable, w: usize) -> Vec<Segment> {
    let c = Console::builder()
        .width(w)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    r.rich_render(&c, &c.options())
}

fn assert_clean(out: &str, width: usize) {
    for line in out.lines() {
        assert_eq!(line, line.trim_end(), "trailing space in {line:?}");
        assert!(
            rich::cells::cell_len(line) <= width,
            "{line:?} is wider than {width}"
        );
    }
}

// ---------------------------------------------------------------- engine

/// A deterministic LCG, so the corpus is the same on every run.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Replay `ops` against `old`, checking every range lines up.
fn apply<T: Clone + PartialEq + std::fmt::Debug>(old: &[T], new: &[T], ops: &[Op]) -> Vec<T> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    for op in ops {
        let (o, n) = (op.old(), op.new_range());
        assert_eq!((o.start, n.start), (i, j), "gap before {op:?}");
        match op {
            Op::Equal { .. } => {
                assert_eq!(o.len(), n.len());
                assert_eq!(&old[o.clone()], &new[n.clone()]);
                out.extend_from_slice(&old[o.clone()]);
            }
            Op::Delete { .. } => assert!(n.is_empty() && !o.is_empty()),
            Op::Insert { .. } => {
                assert!(o.is_empty() && !n.is_empty());
                out.extend_from_slice(&new[n.clone()]);
            }
        }
        i = o.end;
        j = n.end;
    }
    assert_eq!(
        (i, j),
        (old.len(), new.len()),
        "ops do not cover both sides"
    );
    out
}

fn lcs(a: &[u8], b: &[u8]) -> usize {
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    dp[0][0]
}

#[test]
fn edit_scripts_rebuild_the_new_side_and_are_minimal() {
    let mut rng = Lcg(0x5eed);
    for case in 0..400 {
        let alphabet = 2 + rng.below(6) as u8;
        let a: Vec<u8> = (0..rng.below(40))
            .map(|_| rng.below(alphabet as u64) as u8)
            .collect();
        // Half the cases are edits of `a`, half unrelated.
        let b: Vec<u8> = if case % 2 == 0 {
            let mut b = a.clone();
            for _ in 0..rng.below(6) {
                let at = rng.below(b.len() as u64 + 1) as usize;
                match rng.below(3) {
                    0 if at < b.len() => {
                        b.remove(at);
                    }
                    1 if at < b.len() => b[at] = rng.below(alphabet as u64) as u8,
                    _ => b.insert(at, rng.below(alphabet as u64) as u8),
                }
            }
            b
        } else {
            (0..rng.below(40))
                .map(|_| rng.below(alphabet as u64) as u8)
                .collect()
        };
        let ops = diff_slices(&a, &b);
        assert_eq!(apply(&a, &b, &ops), b, "case {case}: {a:?} -> {b:?}");
        let equal: usize = ops
            .iter()
            .filter(|o| o.is_equal())
            .map(|o| o.old().len())
            .sum();
        assert_eq!(
            equal,
            lcs(&a, &b),
            "case {case} not minimal: {a:?} -> {b:?}"
        );
        // Runs alternate: no two adjacent runs of one kind, deletes first.
        for w in ops.windows(2) {
            assert_ne!(std::mem::discriminant(&w[0]), std::mem::discriminant(&w[1]));
            assert!(!matches!(
                (&w[0], &w[1]),
                (Op::Insert { .. }, Op::Delete { .. })
            ));
        }
    }
}

#[test]
fn large_inputs_stay_fast_and_correct() {
    // 10k lines with scattered edits, a fully rewritten 10k, and 10k lines
    // drawn from a tiny alphabet (the worst case for plain Myers). Run
    // times are not asserted; a quadratic engine would time the suite out.
    let old: Vec<String> = (0..10_000).map(|i| format!("line {i}")).collect();
    let mut new = old.clone();
    let mut rng = Lcg(7);
    for _ in 0..200 {
        let at = rng.below(new.len() as u64) as usize;
        match rng.below(3) {
            0 => {
                new.remove(at);
            }
            1 => new[at] = format!("changed {at}"),
            _ => new.insert(at, format!("inserted {at}")),
        }
    }
    let a: Vec<&str> = old.iter().map(String::as_str).collect();
    let b: Vec<&str> = new.iter().map(String::as_str).collect();
    let ops = diff_lines(&a, &b);
    assert_eq!(apply(&a, &b, &ops), b);

    let other: Vec<String> = (0..10_000).map(|i| format!("other {i}")).collect();
    let c: Vec<&str> = other.iter().map(String::as_str).collect();
    let ops = diff_lines(&a, &c);
    assert_eq!(ops.len(), 2);
    assert_eq!(apply(&a, &c, &ops), c);

    let x: Vec<u8> = (0..10_000).map(|_| rng.below(3) as u8).collect();
    let y: Vec<u8> = (0..10_000).map(|_| rng.below(3) as u8).collect();
    let ops = diff_slices(&x, &y);
    assert_eq!(apply(&x, &y, &ops), y);
}

#[test]
fn tokens_and_word_diffs_use_byte_ranges() {
    let s = "let x_1 = foo(été);";
    let tokens: Vec<&str> = tokenize(s).into_iter().map(|r| &s[r]).collect();
    assert_eq!(
        tokens,
        ["let", " ", "x_1", " ", "=", " ", "foo", "(", "été", ")", ";"]
    );
    let old = "let x = 1;";
    let new = "let x = 22;";
    let changed: Vec<(&str, &str)> = diff_words(old, new)
        .into_iter()
        .filter(|op| !op.is_equal())
        .map(|op| (&old[op.old()], &new[op.new_range()]))
        .collect();
    assert_eq!(changed, [("1", ""), ("", "22")]);
    let chars = diff_chars("café", "cafe");
    assert_eq!(
        chars
            .iter()
            .filter(|o| !o.is_equal())
            .map(|o| (o.old(), o.new_range()))
            .collect::<Vec<_>>(),
        [(3..5, 3..3), (5..5, 3..4)]
    );
}

#[test]
fn hunks_group_changes_with_context() {
    let old: String = (1..=20).map(|i| format!("{i}\n")).collect();
    let new: String = (1..=20)
        .filter(|&i| i != 17)
        .map(|i| {
            if i == 3 {
                "three\n".into()
            } else {
                format!("{i}\n")
            }
        })
        .collect();
    let diff = TextDiff::new(&old, &new);
    let headers: Vec<String> = diff.hunks().iter().map(Hunk::header).collect();
    assert_eq!(headers, ["@@ -1,6 +1,6 @@", "@@ -14,7 +14,6 @@"]);
    let wide = diff.clone().context(8);
    assert_eq!(
        wide.hunks().iter().map(Hunk::header).collect::<Vec<_>>(),
        ["@@ -1,20 +1,19 @@"]
    );
    let tight = diff.clone().context(0);
    assert_eq!(
        tight.hunks().iter().map(Hunk::header).collect::<Vec<_>>(),
        ["@@ -3 +3 @@", "@@ -17 +16,0 @@"]
    );
    assert_eq!(diff.stats(), (1, 2));
    assert!(!diff.is_equal());
    assert!(TextDiff::new("a\n", "a\n").is_equal());
    assert_eq!(TextDiff::new("a\n", "a\n").unified("a", "b"), "");
    assert_eq!(hunk_header(0..0, 0..2), "@@ -0,0 +1,2 @@");
}

// ------------------------------------------------------ GNU diff parity

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-ext-diff-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gnu_diff(dir: &Path, old: &str, new: &str, context: usize) -> Option<String> {
    std::fs::write(dir.join("old"), old).unwrap();
    std::fs::write(dir.join("new"), new).unwrap();
    let out = Command::new("diff")
        .arg(format!("-U{context}"))
        .arg("old")
        .arg("new")
        .current_dir(dir)
        .output()
        .ok()?;
    Some(String::from_utf8(out.stdout).unwrap())
}

/// Drop the `---`/`+++` lines, which carry timestamps in `diff -u`.
fn body(unified: &str) -> String {
    unified.lines().skip(2).map(|l| format!("{l}\n")).collect()
}

#[test]
fn unified_output_matches_gnu_diff() {
    let numbered = |n: usize| -> String { (1..=n).map(|i| format!("line {i}\n")).collect() };
    let long = numbered(30);
    let fixtures: Vec<(&str, String, String)> = vec![
        ("modify", "a\nb\nc\n".into(), "a\nB\nc\n".into()),
        ("prepend", "x\ny\n".into(), "new\nx\ny\n".into()),
        ("truncate", "x\ny\nz\n".into(), "x\n".into()),
        ("no-newline-both", "a\nb".into(), "a\nc".into()),
        ("no-newline-old", "a\nb".into(), "a\nb\n".into()),
        ("no-newline-new", "a\nb\n".into(), "a\nb\nc".into()),
        ("from-empty", String::new(), "one\ntwo\n".into()),
        ("to-empty", "one\ntwo\n".into(), String::new()),
        (
            "two-hunks",
            long.clone(),
            long.replace("line 2\n", "line two\n")
                .replace("line 25\n", "line 25\nextra\n"),
        ),
        (
            "unicode",
            "naïve café\nzeile\n".into(),
            "naïve cafe\nzeile\n".into(),
        ),
    ];
    let dir = scratch_dir("gnu");
    let mut checked = 0;
    for (name, old, new) in &fixtures {
        for context in [3, 1, 0] {
            let ours = TextDiff::new(old, new)
                .context(context)
                .unified("old", "new");
            if !ours.is_empty() {
                assert!(ours.starts_with("--- old\n+++ new\n"), "{name}: {ours}");
            }
            let Some(theirs) = gnu_diff(&dir, old, new, context) else {
                eprintln!("note: `diff` is not on PATH; skipping GNU parity for {name}");
                continue;
            };
            assert_eq!(body(&ours), body(&theirs), "{name} at -U{context}");
            checked += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("compared {checked} fixtures with GNU diff");
}

#[test]
fn unified_marks_a_missing_final_newline() {
    assert_eq!(
        TextDiff::new("a\nb", "a\nc").unified("x", "y"),
        "--- x\n+++ y\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
    );
}

// ----------------------------------------------------------------- views

const OLD: &str = "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n";
const NEW: &str = "fn main() {\n    let x = 2;\n    println!(\"{x}\");\n    done();\n}";

#[test]
fn unified_view_renders_exactly() {
    let view = DiffView::new(OLD, NEW).titles("a.rs", "b.rs");
    let out = plain(&view, 60);
    assert_eq!(
        out,
        "\
--- a.rs
+++ b.rs
@@ -1,4 +1,5 @@
1 1   fn main() {
2   -     let x = 1;
  2 +     let x = 2;
3 3       println!(\"{x}\");
4   - }
  4 +     done();
  5 + }
      \\ No newline at end of file"
    );
    assert_clean(&out, 60);
    // Narrow: long lines wrap under their gutter and keep the marker.
    let narrow = plain(
        &DiffView::new("short\n", "a much longer line of text\n"),
        20,
    );
    assert_eq!(
        narrow,
        "@@ -1 +1 @@\n1   - short\n  1 + a much longer\n    + line of text"
    );
    assert_clean(&narrow, 20);
    let no_numbers = plain(&DiffView::new("a\nb\n", "a\nc\n").line_numbers(false), 40);
    assert_eq!(no_numbers, "@@ -1,2 +1,2 @@\n  a\n- b\n+ c");
    assert_eq!(
        plain(&DiffView::new("same\n", "same\n"), 40),
        "no differences"
    );
}

#[test]
fn side_by_side_renders_at_two_widths() {
    let view = DiffView::new(OLD, NEW)
        .titles("a.rs", "b.rs")
        .layout(Layout::SideBySide);
    let wide = plain(&view, 60);
    assert_eq!(
        wide,
        "\
a.rs                         │ b.rs
@@ -1,4 +1,5 @@
1   fn main() {              │ 1   fn main() {
2 -     let x = 1;           │ 2 +     let x = 2;
3       println!(\"{x}\");     │ 3       println!(\"{x}\");
4 - }                        │ 4 +     done();
                             │ 5 + }
                                 \\ No newline at end of file"
    );
    assert_clean(&wide, 60);
    let narrow = plain(&view.clone().wrap(false), 40);
    assert_eq!(
        narrow,
        "\
a.rs               │ b.rs
@@ -1,4 +1,5 @@
1   fn main() {    │ 1   fn main() {
2 -     let x = 1; │ 2 +     let x = 2;
3       println!(… │ 3       println!(\"…
4 - }              │ 4 +     done();
                   │ 5 + }
             \\ No newline at end of file"
    );
    assert_clean(&narrow, 40);
    let wrapped = plain(&view, 40);
    assert!(
        wrapped.contains(
            "3       println!(\" │ 3       println!(\"{\n    {x}\");         │     x}\");\n"
        ),
        "{wrapped}"
    );
    assert_clean(&wrapped, 40);
    // Wide characters are measured in cells.
    let cjk = plain(
        &DiffView::new("漢字漢字漢字漢字\n", "漢字漢字漢字漢字漢\n").layout(Layout::SideBySide),
        30,
    );
    assert_clean(&cjk, 30);
}

#[test]
fn measure_reports_content_width() {
    let c = Console::builder().width(200).build();
    let m = DiffView::new("abc\n", "abcdef\n").measure(&c, &c.options());
    // "1 1 + abcdef": two digits, spaces, marker.
    assert_eq!(m.maximum, 12);
    let m = DiffView::new("abc\n", "abcdef\n")
        .layout(Layout::SideBySide)
        .measure(&c, &c.options());
    assert_eq!(m.maximum, 2 * (4 + 6) + 3);
}

#[test]
fn emphasis_styles_land_on_changed_tokens() {
    let segments = colour_segments(&DiffView::new(OLD, NEW), 60);
    let underlined: Vec<&str> = segments
        .iter()
        .filter(|s| s.style.as_ref().and_then(|st| st.attr(3)) == Some(true))
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(underlined, ["1", "2"], "{segments:?}");
    let red = rich::Style::parse("bold underline red").unwrap();
    let green = rich::Style::parse("bold underline green").unwrap();
    let find = |t: &str| {
        segments
            .iter()
            .find(|s| s.text == t && s.style.as_ref().and_then(|st| st.attr(3)) == Some(true))
            .unwrap()
    };
    assert_eq!(find("1").style.as_ref().unwrap().color(), red.color());
    assert_eq!(find("2").style.as_ref().unwrap().color(), green.color());
    // Unrelated lines are not emphasised.
    assert!(segments
        .iter()
        .filter(|s| s.text.contains("done"))
        .all(|s| s.style.as_ref().and_then(|st| st.attr(3)) != Some(true)));
}

#[test]
fn ansi_inputs_report_style_only_changes() {
    let old = "\x1b[32mok\x1b[0m\nsame\n\x1b[1mbold\x1b[0m\n";
    let new = "\x1b[31mok\x1b[0m\nsame\nnot bold\n";
    let view = DiffView::ansi(old, new).line_numbers(false);
    assert_eq!(view.stats(), (1, 1, 1));
    assert_eq!(view.style_changed_lines(), [1]);
    assert_eq!(
        plain(&view, 40),
        "@@ -1,3 +1,3 @@\n~ ok\n~ ok\n  same\n- bold\n+ not bold"
    );
    // Each side keeps its own styling.
    let segments = colour_segments(&view, 40);
    let oks: Vec<_> = segments.iter().filter(|s| s.text == "ok").collect();
    assert_eq!(oks.len(), 2);
    assert_ne!(oks[0].style, oks[1].style);
    // Same text, same style: no change at all.
    assert!(DiffView::ansi("\x1b[1mx\x1b[0m\n", "\x1b[1mx\x1b[22m\n").is_equal());
}

#[cfg(feature = "testing")]
#[test]
fn snapshot_views_and_diffs_use_the_engine() {
    use rich::protocol::{Support, TargetCapabilities};
    use rich_ext::target::{RenderTarget, TargetKind};
    use rich_ext::testing::RenderSnapshot;
    let target = RenderTarget::new(
        TargetKind::Capture,
        TargetCapabilities {
            width: 20,
            height: 8,
            color_system: Some(ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        rich::Theme::default_theme(),
    );
    let a = RenderSnapshot::capture(&target, &rich::Text::styled("one\ntwo", "red"));
    let b = RenderSnapshot::capture(&target, &rich::Text::styled("one\ntwo", "blue"));
    let view = DiffView::snapshots(&a, &b);
    assert_eq!(view.style_changed_lines(), [1, 2]);
    let diff = a.diff(&b).unwrap();
    assert!(diff.starts_with("style changed on line 1, 2\n"), "{diff}");
    assert!(diff.contains("foreground"), "{diff}");
    let c = RenderSnapshot::capture(&target, &rich::Text::new("one\nthree"));
    let d = RenderSnapshot::capture(&target, &rich::Text::new("one\ntwo"));
    assert_eq!(
        d.diff(&c).unwrap(),
        "--- self\n+++ other\n@@ -1,2 +1,2 @@\n one\n-two\n\\ No newline at end of file\n+three\n\\ No newline at end of file\n"
    );
}

// ------------------------------------------------------------ source diff

#[test]
fn source_diffs_highlight_both_sides() {
    let old = "/* a\n   comment */\nfn a() -> u8 { 1 }\n";
    let new = "/* a\n   comment */\nfn a() -> u8 { 2 }\n";
    let diff = SourceDiff::new(old, new).path("src/lib.rs");
    let text = plain(&diff, 60);
    assert_eq!(
        text,
        "\
--- src/lib.rs
+++ src/lib.rs
@@ -1,3 +1,3 @@
1 1   /* a
2 2      comment */
3   - fn a() -> u8 { 1 }
  3 + fn a() -> u8 { 2 }"
    );
    let segments = colour_segments(&diff, 60);
    let colour_of = |t: &str| {
        segments
            .iter()
            .find(|s| s.text == t)
            .and_then(|s| s.style.as_ref())
            .and_then(|s| s.color().cloned())
    };
    // Keywords and multi-line comments are coloured, and differently.
    let keyword = colour_of("fn").expect("`fn` has a colour");
    let comment = colour_of("   comment */").expect("the comment's second line is highlighted");
    assert_ne!(keyword, comment);
    // No syntax background bleeds through the diff styles.
    assert!(segments.iter().filter(|s| s.text == "fn").all(|s| s
        .style
        .as_ref()
        .unwrap()
        .bgcolor()
        .is_none()));
    // Emphasis layers on top of highlighting.
    assert!(segments
        .iter()
        .any(|s| s.text == "2" && s.style.as_ref().and_then(|st| st.attr(3)) == Some(true)));
    // Line numbers link through the template.
    let linked = SourceDiff::new(old, new)
        .path("src/lib.rs")
        .link_template("vscode://file/{path}:{line}");
    let links: Vec<_> = colour_segments(&linked, 60)
        .into_iter()
        .filter_map(|s| s.style.and_then(|st| st.link().map(str::to_owned)))
        .collect();
    assert!(
        links.contains(&"vscode://file/src/lib.rs:3".to_string()),
        "{links:?}"
    );
    // Side by side keeps the width.
    let sbs = plain(&diff.clone().layout(Layout::SideBySide), 50);
    assert_clean(&sbs, 50);
    assert!(
        sbs.contains("3 - fn a() -> u8 { 1 }  │ 3 + fn a() -> u8 { 2 }"),
        "{sbs}"
    );
}

// ------------------------------------------------------------------- git

const FIXTURE: &str = "\
From 1234 Mon Sep 17 00:00:00 2001
Subject: [PATCH] a commit message is skipped

diff --git a/src/lib.rs b/src/lib.rs
index 3b18e51..a9c2f4d 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,5 +1,6 @@ mod util;
 fn main() {
-    let x = 1;
+    let x = 2;
+    let y = x;
     println!(\"{x}\");
 }
 // end
diff --git a/old_name.py b/new_name.py
similarity index 90%
rename from old_name.py
rename to new_name.py
index 1111111..2222222 100644
--- a/old_name.py
+++ b/new_name.py
@@ -1,2 +1,2 @@
 def f():
-    return 1
\\ No newline at end of file
+    return 2
\\ No newline at end of file
diff --git a/docs/added.md b/docs/added.md
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/docs/added.md
@@ -0,0 +1,2 @@
+# Title
+text
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index e69de29..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
diff --git a/logo.png b/logo.png
index 0123456..789abcd 100644
Binary files a/logo.png and b/logo.png differ
diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
diff --git a/lib/a.rs b/lib/b.rs
similarity index 100%
copy from lib/a.rs
copy to lib/b.rs
";

#[test]
fn parses_a_multi_file_git_diff() {
    type Row<'a> = (Option<&'a str>, Option<&'a str>, FileStatus, usize, usize);
    let patch = parse_unified(FIXTURE).unwrap();
    let summary: Vec<Row> = patch
        .files
        .iter()
        .map(|f| {
            (
                f.old_path.as_deref(),
                f.new_path.as_deref(),
                f.status,
                f.additions,
                f.deletions,
            )
        })
        .collect();
    use FileStatus::*;
    assert_eq!(
        summary,
        [
            (Some("src/lib.rs"), Some("src/lib.rs"), Modified, 2, 1),
            (Some("old_name.py"), Some("new_name.py"), Renamed, 1, 1),
            (None, Some("docs/added.md"), Added, 2, 0),
            (Some("gone.txt"), None, Deleted, 0, 1),
            (Some("logo.png"), Some("logo.png"), Binary, 0, 0),
            (Some("run.sh"), Some("run.sh"), Modified, 0, 0),
            (Some("lib/a.rs"), Some("lib/b.rs"), Copied, 0, 0),
        ]
    );
    let lib = &patch.files[0];
    assert_eq!(lib.hunks[0].header(), "@@ -1,5 +1,6 @@ mod util;");
    let added: Vec<_> = lib.hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Added)
        .map(|l| (l.new_line, l.text.as_str()))
        .collect();
    assert_eq!(
        added,
        [(Some(2), "    let x = 2;"), (Some(3), "    let y = x;")]
    );
    let renamed = &patch.files[1];
    assert_eq!(renamed.similarity, Some(90));
    assert!(
        renamed.hunks[0]
            .lines
            .iter()
            .filter(|l| l.no_newline)
            .count()
            == 2
    );
    assert!(patch.files[5].mode_changed());
    assert_eq!(patch.files[5].new_mode.as_deref(), Some("100755"));
    assert!(patch.files[4].binary);
    assert_eq!(patch.stats(), (5, 3));

    let err =
        parse_unified("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a\n").unwrap_err();
    assert!(err.message.contains("ended early"), "{err}");
    assert!(parse_unified("diff --git a/x b/x\n@@ nonsense @@\n").is_err());
    // Plain `diff -u` with timestamps parses too.
    let plain_diff =
        "--- a.txt\t2024-01-01 00:00:00\n+++ b.txt\t2024-01-02 00:00:00\n@@ -1 +1 @@\n-a\n+b\n";
    let p = parse_unified(plain_diff).unwrap();
    assert_eq!(p.files[0].old_path.as_deref(), Some("a.txt"));
    assert_eq!(p.files[0].new_path.as_deref(), Some("b.txt"));
    // Quoted paths.
    let quoted = "diff --git \"a/sp ace\\tx\" \"b/sp ace\\tx\"\nnew file mode 100644\n";
    assert_eq!(
        parse_unified(quoted).unwrap().files[0].new_path.as_deref(),
        Some("sp ace\tx")
    );
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "core.autocrlf=false", "-c", "diff.noprefix=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn parses_real_git_output() {
    let dir = scratch_dir("git");
    if git(&dir, &["init", "-q"]).is_none() {
        eprintln!("note: `git` is unavailable; the checked-in fixture covers parsing");
        return;
    }
    let write = |name: &str, content: &[u8]| {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    };
    let body: String = (1..=12).map(|i| format!("line {i}\n")).collect();
    write("keep.txt", b"one\ntwo\nthree\n");
    write("gone.txt", b"bye\n");
    write("move/me.py", body.as_bytes());
    write("bin.dat", &[0, 1, 2, 3, 255]);
    write("run.sh", b"echo hi\n");
    git(&dir, &["add", "-A"]).unwrap();
    git(&dir, &["commit", "-qm", "base"]).unwrap();

    write("keep.txt", b"one\n2\nthree\n");
    std::fs::remove_file(dir.join("gone.txt")).unwrap();
    std::fs::remove_file(dir.join("move/me.py")).unwrap();
    write(
        "moved/me.py",
        body.replace("line 12\n", "line twelve\n").as_bytes(),
    );
    write("bin.dat", &[0, 1, 2, 4, 255]);
    write("new.txt", b"fresh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.join("run.sh"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    git(&dir, &["add", "-A"]).unwrap();
    let out = git(
        &dir,
        &["diff", "--cached", "-M", "--no-color", "--no-ext-diff"],
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    let patch = parse_unified(&out).unwrap();
    let find = |p: &str| {
        patch
            .files
            .iter()
            .find(|f| f.path() == p)
            .unwrap_or_else(|| panic!("{p} missing from {out}"))
    };
    assert_eq!(find("keep.txt").status, FileStatus::Modified);
    assert_eq!(
        (find("keep.txt").additions, find("keep.txt").deletions),
        (1, 1)
    );
    assert_eq!(find("gone.txt").status, FileStatus::Deleted);
    let moved = find("moved/me.py");
    assert_eq!(moved.status, FileStatus::Renamed);
    assert_eq!(moved.old_path.as_deref(), Some("move/me.py"));
    assert_eq!(find("bin.dat").status, FileStatus::Binary);
    let new = find("new.txt");
    assert_eq!(new.status, FileStatus::Added);
    assert!(new.hunks[0].lines[0].no_newline);
    #[cfg(unix)]
    assert!(find("run.sh").mode_changed());
}

#[test]
fn patch_view_renders_tree_hunks_and_annotations() {
    let patch = parse_unified(FIXTURE).unwrap();
    let view = PatchView::new(patch)
        .annotate(Annotation::new(
            "src/lib.rs",
            3,
            Level::Warning,
            "unused variable `y`",
        ))
        .links(
            TemplateLinks::new("https://example.test/{repo}/blob/{rev}/{path}#L{line}")
                .file_template("https://example.test/{repo}/blob/{rev}/{path}")
                .var("repo", "demo")
                .var("rev", "abc123"),
        );
    let out = plain(&view, 80);
    assert_clean(&out, 80);
    let expected_tree = "\
├── docs/
│   └── added.md (added)                    +2 -0 ++
├── lib/
│   └── b.rs (copied from lib/a.rs)         +0 -0
├── src/
│   └── lib.rs                              +2 -1 ++-
├── new_name.py (renamed from old_name.py)  +1 -1 +-
├── gone.txt (deleted)                      +0 -1 -
├── logo.png                                binary
└── run.sh (mode 100755)                    +0 -0
";
    assert!(out.starts_with(expected_tree), "{out}");
    let expected_file = "\
modified src/lib.rs  +2 -1
@@ -1,5 +1,6 @@ mod util;
1 1   fn main() {
2   -     let x = 1;
  2 +     let x = 2;
  3 +     let y = x;
      warning: unused variable `y`
3 4       println!(\"{x}\");
";
    assert!(out.contains(expected_file), "{out}");
    assert!(out.contains("renamed old_name.py -> new_name.py (90%)  +1 -1"));
    assert!(out.contains("mode 100644 -> 100755"));
    assert!(out.contains("Binary file differs"));
    assert!(out.ends_with("7 files changed, 5 insertions(+), 3 deletions(-)"));

    let segments = colour_segments(&view, 80);
    let links: Vec<String> = segments
        .iter()
        .filter_map(|s| s.style.as_ref().and_then(|st| st.link().map(str::to_owned)))
        .collect();
    assert!(links.contains(&"https://example.test/demo/blob/abc123/src/lib.rs#L3".into()));
    assert!(links.contains(&"https://example.test/demo/blob/abc123/src/lib.rs".into()));
    // Annotations take their level's style.
    let warning = Level::Warning.style(&Console::new());
    assert!(segments
        .iter()
        .any(|s| s.text == "warning: " && s.style.as_ref() == Some(&warning)));
    let sbs = plain(
        &parse_unified(FIXTURE)
            .map(PatchView::new)
            .unwrap()
            .layout(Layout::SideBySide),
        70,
    );
    assert_clean(&sbs, 70);
}

#[test]
fn hyperlinkers_are_link_providers() {
    let linker = rich_ext::hyperlink::Hyperlinker::new().editor("vscode://file/{path}:{line}");
    assert_eq!(
        LinkProvider::line_url(&linker, "/src/a b.rs", 4).as_deref(),
        Some("vscode://file//src/a%20b.rs:4")
    );
    let links = TemplateLinks::new("https://h/{path}#L{line}");
    assert_eq!(links.file_url("x"), None);
    assert_eq!(
        links.line_url("a b", 2).as_deref(),
        Some("https://h/a%20b#L2")
    );
}

// ----------------------------------------------------------- test report

#[cfg(feature = "test-report")]
mod reports {
    use super::*;
    use rich_ext::diff::test_report::*;
    use std::time::Duration;

    /// Maven Surefire 3 (JUnit 5), trimmed: bare `testsuite` root,
    /// properties, JUnit 4-style `expected:<…> but was:<…>`.
    const SUREFIRE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" version="3.0" name="com.example.CalcTest" time="0.052" tests="3" errors="1" skipped="0" failures="1">
  <properties>
    <property name="java.version" value="21"/>
  </properties>
  <testcase name="adds" classname="com.example.CalcTest" time="0.004"/>
  <testcase name="divides" classname="com.example.CalcTest" time="0.011">
    <failure message="expected: &lt;2&gt; but was: &lt;3&gt;" type="org.opentest4j.AssertionFailedError"><![CDATA[org.opentest4j.AssertionFailedError: expected:<2> but was:<3>
	at com.example.CalcTest.divides(CalcTest.java:14)
]]></failure>
    <system-out><![CDATA[dividing 6 by 3
]]></system-out>
  </testcase>
  <testcase name="loads" classname="com.example.CalcTest" time="0,002">
    <error message="config missing" type="java.io.FileNotFoundException">java.io.FileNotFoundException: config missing</error>
  </testcase>
</testsuite>
"#;

    /// pytest 8 `--junitxml`.
    const PYTEST: &str = r#"<?xml version="1.0" encoding="utf-8"?><testsuites name="pytest tests"><testsuite name="pytest" errors="0" failures="1" skipped="1" tests="3" time="0.031" timestamp="2026-09-01T10:00:00.000000+00:00" hostname="ci"><testcase classname="tests.test_math" name="test_ok" time="0.001" /><testcase classname="tests.test_math" name="test_bad" time="0.002"><failure message="assert 1 == 2">def test_bad():
&gt;       assert 1 == 2
E       assert 1 == 2

tests/test_math.py:5: AssertionError</failure><system-out>--- captured ---
</system-out></testcase><testcase classname="tests.test_math" name="test_skip" time="0.000"><skipped type="pytest.skip" message="not on CI">tests/test_math.py:8: not on CI</skipped></testcase></testsuite></testsuites>"#;

    /// jest-junit 16 defaults: classname and name are both the full title,
    /// and failures carry no `message` attribute.
    const JEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="jest tests" tests="2" failures="1" errors="0" time="1.234">
  <testsuite name="sum" errors="0" failures="1" skipped="0" timestamp="2026-09-01T10:00:00" time="0.8" tests="2">
    <testcase classname="sum adds 1 + 2" name="sum adds 1 + 2" time="0.003">
    </testcase>
    <testcase classname="sum adds 2 + 2" name="sum adds 2 + 2" time="0.004">
      <failure>Error: expect(received).toBe(expected) // Object.is equality

Expected: 4
Received: 5
    at Object.&lt;anonymous&gt; (sum.test.js:9:22)</failure>
    </testcase>
  </testsuite>
</testsuites>
"#;

    /// libtest's JSON stream, hand-written to the format `cargo test -- -Z
    /// unstable-options --format json --report-time` prints on nightly
    /// (libtest's `formatters/json.rs`), with cargo's `Running` lines from
    /// stderr interleaved. Two test binaries.
    const LIBTEST: &str = r#"     Running unittests src/lib.rs (target/debug/deps/demo-1a2b3c)
{ "type": "suite", "event": "started", "test_count": 3 }
{ "type": "test", "event": "started", "name": "math::adds" }
{ "type": "test", "event": "started", "name": "math::subtracts" }
{ "type": "test", "event": "started", "name": "math::slow" }
{ "type": "test", "name": "math::adds", "event": "ok", "exec_time": 0.000412 }
{ "type": "test", "name": "math::subtracts", "event": "failed", "exec_time": 0.001203, "stdout": "computing\n\nthread 'math::subtracts' panicked at src/math.rs:21:9:\nassertion `left == right` failed: difference\n  left: \"1\\n2\\n3\"\n right: \"1\\n2\\n4\"\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n" }
{ "type": "test", "event": "ignored", "name": "math::slow", "message": "takes a minute" }
{ "type": "suite", "event": "failed", "passed": 1, "failed": 1, "ignored": 1, "measured": 0, "filtered_out": 0, "exec_time": 0.002514 }
     Running tests/api.rs (target/debug/deps/api-4d5e6f)
{ "type": "suite", "event": "started", "test_count": 1 }
{ "type": "test", "event": "started", "name": "serves" }
{ "type": "test", "name": "serves", "event": "ok", "exec_time": 0.01 }
{ "type": "suite", "event": "ok", "passed": 1, "failed": 0, "ignored": 0, "measured": 0, "filtered_out": 0, "exec_time": 0.011 }
"#;

    fn statuses(run: &TestRun) -> Vec<(String, Status)> {
        run.suites
            .iter()
            .flat_map(|s| s.cases.iter().map(|c| (c.full_name(), c.status)))
            .collect()
    }

    #[test]
    fn junit_dialects_parse() {
        use Status::*;
        let run = junit::parse(SUREFIRE).unwrap();
        assert_eq!(
            statuses(&run),
            [
                ("com.example.CalcTest.adds".into(), Passed),
                ("com.example.CalcTest.divides".into(), Failed),
                ("com.example.CalcTest.loads".into(), Errored),
            ]
        );
        let divides = &run.suites[0].cases[1];
        assert_eq!(divides.expected.as_deref(), Some("2"));
        assert_eq!(divides.actual.as_deref(), Some("3"));
        assert_eq!(divides.stdout.as_deref(), Some("dividing 6 by 3"));
        assert!(divides
            .details
            .as_deref()
            .unwrap()
            .contains("CalcTest.java:14"));
        assert_eq!(
            run.suites[0].cases[2].duration,
            Some(Duration::from_millis(2))
        );
        assert_eq!(run.suites[0].duration, Some(Duration::from_millis(52)));

        let run = junit::parse(PYTEST).unwrap();
        assert_eq!(
            statuses(&run),
            [
                ("tests.test_math.test_ok".into(), Passed),
                ("tests.test_math.test_bad".into(), Failed),
                ("tests.test_math.test_skip".into(), Skipped),
            ]
        );
        let bad = &run.suites[0].cases[1];
        assert_eq!(bad.message.as_deref(), Some("assert 1 == 2"));
        assert!(bad
            .details
            .as_deref()
            .unwrap()
            .contains(">       assert 1 == 2"));
        assert_eq!(run.suites[0].cases[2].message.as_deref(), Some("not on CI"));

        let run = junit::parse(JEST).unwrap();
        assert_eq!(
            statuses(&run),
            [
                ("sum adds 1 + 2".into(), Passed),
                ("sum adds 2 + 2".into(), Failed)
            ]
        );
        let failed = &run.suites[0].cases[1];
        assert_eq!(
            failed.message.as_deref(),
            Some("Error: expect(received).toBe(expected) // Object.is equality")
        );
        assert_eq!(
            (failed.expected.as_deref(), failed.actual.as_deref()),
            (Some("4"), Some("5"))
        );
        assert_eq!(run.duration, Some(Duration::from_millis(1234)));

        assert!(junit::parse("<html/>").is_err());
        assert!(junit::JUnit.parse(PYTEST).is_ok());
    }

    #[test]
    fn libtest_json_parses() {
        let run = libtest::parse(LIBTEST).unwrap();
        assert_eq!(
            run.suites
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["unittests src/lib.rs", "tests/api.rs"]
        );
        use Status::*;
        assert_eq!(
            statuses(&run),
            [
                ("math::adds".into(), Passed),
                ("math::subtracts".into(), Failed),
                ("math::slow".into(), Skipped),
                ("serves".into(), Passed),
            ]
        );
        let failed = &run.suites[0].cases[1];
        assert_eq!(failed.classname, "math");
        assert_eq!(
            failed.message.as_deref(),
            Some("assertion `left == right` failed: difference")
        );
        assert_eq!(failed.expected.as_deref(), Some("\"1\\n2\\n4\""));
        assert_eq!(failed.actual.as_deref(), Some("\"1\\n2\\n3\""));
        assert_eq!(failed.duration, Some(Duration::from_nanos(1_203_000)));
        assert_eq!(
            run.suites[0].cases[2].message.as_deref(),
            Some("takes a minute")
        );
        assert_eq!(run.suites[1].duration, Some(Duration::from_millis(11)));
        assert!(libtest::parse("no events here").is_err());
        assert!(libtest::LibTest.parse(LIBTEST).is_ok());
    }

    #[test]
    fn report_puts_failures_first() {
        let run = junit::parse(SUREFIRE).unwrap();
        let report = TestReport::new(run.clone());
        let out = plain(&report, 72);
        assert_clean(&out, 72);
        let failure = out
            .find("FAILED com.example.CalcTest > com.example.CalcTest.divides")
            .unwrap();
        let error = out.find("ERROR com.example.CalcTest").unwrap();
        let table = out.find("Suite").unwrap();
        assert!(failure < error && error < table, "{out}");
        assert!(
            out.contains("--- expected\n  +++ actual\n  @@ -1 +1 @@\n  1   - 2\n    1 + 3"),
            "{out}"
        );
        assert!(
            out.contains("captured stdout:\n    dividing 6 by 3"),
            "{out}"
        );
        assert!(!out.contains("PASSED"));
        assert!(
            out.ends_with("1 passed, 1 failed, 1 errored in 0.052s"),
            "{out}"
        );
        let all = plain(&TestReport::new(run).show_passed(true), 72);
        assert!(
            all.contains("PASSED com.example.CalcTest > com.example.CalcTest.adds (0.004s)"),
            "{all}"
        );
        assert!(all.find("PASSED").unwrap() > all.find("FAILED").unwrap());
        // Suite table rows.
        assert!(all.contains("com.example.CalcTest"), "{all}");
    }

    #[test]
    fn report_prints_bracketed_names_literally() {
        // pytest parametrised ids look like markup; they must stay data.
        let xml = r#"<testsuite name="test_colours[red]" tests="1"><testcase classname="c" name="test_x[bold]"/></testsuite>"#;
        let run = junit::parse(xml).unwrap();
        let all = plain(&TestReport::new(run).show_passed(true), 72);
        assert!(all.contains("test_x[bold]"), "{all}");
        // The suite table's row, not only the PASSED line, keeps the brackets.
        assert!(
            all.lines()
                .any(|l| l.contains("test_colours[red]") && l.contains(" 1 ")),
            "{all}"
        );
    }

    #[test]
    fn junit_export_round_trips() {
        for source in [SUREFIRE, PYTEST, JEST] {
            let run = junit::parse(source).unwrap();
            let xml = run.to_junit_xml();
            let back = junit::parse(&xml).unwrap();
            assert_eq!(back, run, "{xml}");
        }
        let run = libtest::parse(LIBTEST).unwrap();
        let back = junit::parse(&run.to_junit_xml()).unwrap();
        assert_eq!(back, run);
    }
}

// ------------------------------------------------------------ assertions

#[cfg(feature = "testing")]
mod assertions {
    use rich_ext::diff::assert::Report;
    use rich_ext::diff::{DiffView, Layout};
    use rich_ext::{assert_render_eq, assert_rich_eq, assert_rich_json_eq, assert_snapshot_eq};

    fn panic_message(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
        let err = std::panic::catch_unwind(f).expect_err("assertion should fail");
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap()
    }

    #[test]
    fn assertion_macros_panic_with_a_diff() {
        // SAFETY: the only reader of these variables in this binary is the
        // assertion code under test, which runs on this thread.
        std::env::set_var("RICH_ASSERT_COLOR", "0");
        std::env::remove_var("RICH_ASSERT_LAYOUT");

        assert_rich_eq!("same\n", String::from("same\n"));
        let message = panic_message(|| assert_rich_eq!("a\nb\n", "a\nc\n"));
        assert_eq!(
            message,
            "assertion `left == right` failed\n--- left\n+++ right\n@@ -1,2 +1,2 @@\n1 1   a\n2   - b\n  2 + c"
        );
        let message = panic_message(|| assert_rich_eq!("x", "y", "case {}", 7));
        assert!(
            message.starts_with("assertion `left == right` failed: case 7\n"),
            "{message}"
        );
        assert!(message.is_ascii());

        #[derive(serde::Serialize)]
        struct P {
            name: &'static str,
            port: u16,
        }
        assert_rich_json_eq!(
            P { name: "a", port: 1 },
            serde_json::json!({"name": "a", "port": 1})
        );
        let message = panic_message(|| {
            assert_rich_json_eq!(
                P { name: "a", port: 1 },
                serde_json::json!({"name": "a", "port": 2})
            )
        });
        assert!(
            message.contains("-   \"port\": 1\n") && message.contains("+   \"port\": 2"),
            "{message}"
        );

        assert_render_eq!(rich::Text::new("hello world"), "hello\nworld\n", width = 6);
        let message = panic_message(|| {
            assert_render_eq!(
                rich::Text::new("hello world"),
                "hello world",
                width = 6,
                "narrow"
            )
        });
        assert!(
            message.starts_with(
                "assertion `rendered == expected` failed: narrow\n--- expected\n+++ rendered\n"
            ),
            "{message}"
        );
        assert!(
            message.contains("- hello world") && message.contains("+ hello"),
            "{message}"
        );

        std::env::set_var("RICH_ASSERT_LAYOUT", "side-by-side");
        let message = panic_message(|| assert_rich_eq!("a\nb\n", "a\nc\n"));
        assert!(
            message.contains("2 - b") && message.contains(" | 2 + c"),
            "{message}"
        );
        std::env::remove_var("RICH_ASSERT_LAYOUT");
    }

    #[test]
    fn snapshot_assertions_show_style_changes() {
        use rich::protocol::{Support, TargetCapabilities};
        use rich_ext::target::{RenderTarget, TargetKind};
        use rich_ext::testing::RenderSnapshot;
        let target = RenderTarget::new(
            TargetKind::Capture,
            TargetCapabilities {
                width: 20,
                height: 4,
                color_system: Some(rich::ColorSystem::Truecolor),
                interactive: false,
                unicode: true,
                hyperlinks: false,
                sixel: Support::Unsupported,
            },
            rich::Theme::default_theme(),
        );
        let a = RenderSnapshot::capture(&target, &rich::Text::styled("hi", "red"));
        let b = RenderSnapshot::capture(&target, &rich::Text::styled("hi", "blue"));
        assert_snapshot_eq!(a, a.clone());
        let message = panic_message(|| assert_snapshot_eq!(a, b));
        assert!(message.contains("style changed on line 1"), "{message}");
        assert!(message.contains("~ hi"), "{message}");
    }

    #[test]
    fn reports_render_colour_and_layout_on_request() {
        let view = DiffView::new("a\nb\n", "a\nc\n");
        let coloured = Report {
            layout: Layout::Unified,
            color: true,
            width: 40,
        }
        .render(&view);
        assert!(coloured.contains("\x1b["), "{coloured:?}");
        let plain = Report {
            layout: Layout::SideBySide,
            color: false,
            width: 40,
        }
        .render(&view);
        assert!(!plain.contains('\x1b') && plain.is_ascii(), "{plain:?}");
        assert!(plain.contains(" | "), "{plain}");
    }
}
