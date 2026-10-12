"""The pictures on docs/python/ext/diffs.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import diff


def text(console):
    # --8<-- [start:text]
    OLD = '[server]\nhost = "0.0.0.0"\nport = 8080\nlog = "info"\n'
    NEW = '[server]\nhost = "0.0.0.0"\nport = 8443\nlog = "debug"\ntls = true\n'
    text_diff = diff.TextDiff(OLD, NEW, context=1)
    print(text_diff.stats())
    print(text_diff.unified("a/server.toml", "b/server.toml"), end="")
    console.print(diff.DiffView(OLD, NEW, layout="side_by_side", titles=("before", "after"), context=1))
    # --8<-- [end:text]


# --8<-- [start:patch-source]
PATCH = """\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 pub fn total(items: &[u32]) -> u32 {
-    let mut sum = 0;
+    let mut sum: u32 = 0;
     for item in items {
"""
# --8<-- [end:patch-source]


def patched(console):
    # --8<-- [start:patch]
    patch = diff.parse_patch(PATCH)
    print([(f.path, f.status) for f in patch.files], patch.stats())
    console.print(diff.PatchView(
        patch,
        tree=False,
        annotations=[diff.Annotation("src/lib.rs", 2, "consider `iter().sum()`", level="note")],
    ))
    # --8<-- [end:patch]


def tests(console):
    # --8<-- [start:tests]
    run = diff.TestRun([diff.TestSuite("parser", [
        diff.TestCase("empty"),
        diff.TestCase("quoted", status="failed", message="assertion failed", expected="1\n2\n3", actual="1\n2\n4"),
    ])])
    # --8<-- [end:tests]
    # --8<-- [start:tests-print]
    console.print(diff.TestReport(run))
    # --8<-- [end:tests-print]


shots = Shots("ext/diffs")
shots.shot("text", text, width=60)
shots.shot("patch", patched, width=60)
shots.shot("tests", tests, width=60)
shots.finish()
