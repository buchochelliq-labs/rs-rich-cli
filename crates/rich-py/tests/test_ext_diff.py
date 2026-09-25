"""rs_rich.ext.diff: the diff engine, views, git patches and test reports.

Expected output comes from `rich-ext` itself (see test_ext_expected.py).
"""

from __future__ import annotations

import pytest
from conftest import render
from test_ext_expected import EXPECTED

from rs_rich.ext import diff
from rs_rich.ext.transform import Pipeline, TransformError

OLD = '[server]\nhost = "0.0.0.0"\nport = 8080\nworkers = 4\nlog = "info"\n'
NEW = '[server]\nhost = "0.0.0.0"\nport = 8443\nworkers = 4\nlog = "debug"\ntls = true\n'
PATCH = """\
diff --git a/src/lib.rs b/src/lib.rs
index 3b18e51..a2c4f0d 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,5 +1,6 @@
 pub fn total(items: &[u32]) -> u32 {
-    let mut sum = 0;
+    let mut sum: u32 = 0;
+    let unused = items.len();
     for item in items {
         sum += item;
     }
diff --git a/README.md b/README.md
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/README.md
@@ -0,0 +1,2 @@
+# totals
+Adds numbers.
"""
LIBTEST = r"""     Running unittests src/lib.rs (target/debug/deps/totals-5e1c0a)
{ "type": "suite", "event": "started", "test_count": 3 }
{ "type": "test", "event": "started", "name": "math::adds" }
{ "type": "test", "name": "math::adds", "event": "ok", "exec_time": 0.000412 }
{ "type": "test", "event": "started", "name": "math::lists" }
{ "type": "test", "name": "math::lists", "event": "failed", "exec_time": 0.001203, "stdout": "thread 'math::lists' panicked at src/math.rs:21:9:\nassertion `left == right` failed\n  left: \"1\\n2\\n3\"\n right: \"1\\n2\\n4\"\n" }
{ "type": "test", "event": "ignored", "name": "math::slow", "message": "takes a minute" }
{ "type": "suite", "event": "failed", "passed": 1, "failed": 1, "ignored": 1, "measured": 0, "filtered_out": 0, "exec_time": 0.002514 }
"""
JUNIT = """<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="api.tests" tests="2" failures="1" time="0.031">
  <testcase classname="api.tests.UserTest" name="creates_user" time="0.012"/>
  <testcase classname="api.tests.UserTest" name="rejects_duplicate" time="0.019">
    <failure message="expected:&lt;409&gt; but was:&lt;500&gt;">at UserTest.java:42</failure>
  </testcase>
</testsuite>
"""


def check(name, renderable, width):
    assert render(renderable, width=width) == EXPECTED[name]
    assert render(renderable, width=width, color=True) == EXPECTED[f"{name}/color"]


def test_text_diff():
    text_diff = diff.TextDiff(OLD, NEW, context=1)
    assert text_diff.stats() == (3, 2)
    assert not text_diff.is_equal
    assert text_diff.unified("a/server.toml", "b/server.toml") == EXPECTED["diff/unified-text"]
    assert [h.header for h in text_diff.hunks()] == ["@@ -2,4 +2,5 @@"]
    assert diff.TextDiff("a\n", "a\n").is_equal


def test_engine_functions():
    assert diff.diff_lines(["a", "b"], ["a", "c"]) == [
        ("equal", (0, 1), (0, 1)),
        ("delete", (1, 2), (1, 1)),
        ("insert", (2, 2), (1, 2)),
    ]
    assert diff.diff_sequences([1, (2, 3), "x"], [1, "x"]) == [
        ("equal", (0, 1), (0, 1)),
        ("delete", (1, 2), (1, 1)),
        ("equal", (2, 3), (1, 2)),
    ]
    # Character offsets, even past a multi-byte character.
    assert diff.diff_words("café au lait", "café noir") == [
        ("equal", (0, 5), (0, 5)),
        ("delete", (5, 12), (5, 5)),
        ("insert", (12, 12), (5, 9)),
    ]
    assert diff.tokenize("hé, you") == [(0, 2), (2, 3), (3, 4), (4, 7)]
    assert diff.hunk_header((0, 2), (0, 3)) == "@@ -1,2 +1,3 @@"
    hunks = diff.group_hunks(diff.diff_lines(list("abcdefgh"), list("abcXefgh")), context=1)
    assert [h.header for h in hunks] == ["@@ -3,3 +3,3 @@"]


def test_diff_views():
    check("diff/unified", diff.DiffView(OLD, NEW, titles=("server.toml (old)", "server.toml (new)")), 60)
    side = diff.DiffView(OLD, NEW, layout="side_by_side", titles=("before", "after"), context=1)
    check("diff/side-by-side", side, 80)
    before = "test parse ... \x1b[32mok\x1b[0m\ntest render ... \x1b[31mFAILED\x1b[0m\n"
    after = "test parse ... \x1b[32mok\x1b[0m\ntest render ... FAILED\n"
    ansi = diff.DiffView(before, after, ansi=True)
    assert ansi.style_changed_lines() == [2]
    assert ansi.stats() == (0, 0, 1)
    check("diff/ansi", ansi, 60)
    from_diff = diff.DiffView.from_diff(diff.TextDiff(OLD, NEW), titles=("server.toml (old)", "server.toml (new)"))
    check("diff/unified", from_diff, 60)


def test_source_diff():
    old = "fn area(w: u32, h: u32) -> u32 {\n    w * h\n}\n"
    new = "fn area(w: u64, h: u64) -> u64 {\n    w.saturating_mul(h)\n}\n"
    source = diff.SourceDiff(old, new, path="src/geometry.rs", link_template="vscode://file/{path}:{line}")
    check("diff/source", source, 72)


def test_patch():
    patch = diff.parse_patch(PATCH)
    assert patch.stats() == (4, 1)
    assert [(f.path, f.status) for f in patch.files] == [("src/lib.rs", "modified"), ("README.md", "added")]
    header, lines = patch.files[0].hunks[0]
    assert header == "@@ -1,5 +1,6 @@"
    assert lines[1] == ("removed", "    let mut sum = 0;", 2, None)
    links = diff.TemplateLinks(
        "https://github.com/{owner}/{repo}/blob/{rev}/{path}#L{line}",
        file_template="https://github.com/{owner}/{repo}/blob/{rev}/{path}",
        owner="octo",
        repo="totals",
        rev="a2c4f0d",
    )
    assert links.line_url("src/lib.rs", 7) == "https://github.com/octo/totals/blob/a2c4f0d/src/lib.rs#L7"
    view = diff.PatchView(
        patch,
        annotations=[diff.Annotation("src/lib.rs", 3, "unused variable: `unused`", level="warning")],
        links=links,
    )
    check("diff/patch", view, 80)
    kept = Pipeline().then("files", diff.KeepFiles(["*.md"])).apply(PATCH)
    assert [f.path for f in kept.files] == ["README.md"]
    check("diff/patch-kept", diff.PatchView(kept, tree=False, highlight=False), 60)
    with pytest.raises(diff.PatchParseError):
        diff.parse_patch("@@ nonsense")


def test_test_reports():
    run = diff.TestRun.from_libtest(LIBTEST)
    assert not run.is_success
    assert run.totals() == {"passed": 1, "failed": 1, "errored": 0, "skipped": 1, "total": 3}
    check("diff/test-report-libtest", diff.TestReport(run, show_passed=True), 80)
    junit = diff.TestRun.from_junit(JUNIT)
    case = junit.suites[0].cases[1]
    assert (case.full_name, case.status, case.expected, case.actual) == (
        "api.tests.UserTest.rejects_duplicate",
        "failed",
        "409",
        "500",
    )
    assert junit.to_junit_xml() == EXPECTED["diff/junit-xml"]
    check("diff/test-report-junit", diff.TestReport(JUNIT), 80)
    with pytest.raises(diff.TestParseError):
        diff.TestRun.from_junit("<nope")


def test_build_a_run_by_hand():
    run = diff.TestRun(
        [diff.TestSuite("s", [diff.TestCase("ok"), diff.TestCase("bad", "m", "failed", message="boom")])]
    )
    assert run.totals()["failed"] == 1
    assert "boom" in render(diff.TestReport(run), width=60)


def test_stage_errors_name_the_stage():
    def fail(value):
        raise TransformError("nope")

    with pytest.raises(TransformError) as caught:
        Pipeline().then("first", diff.KeepFiles(["x"])).then("second", fail).apply(PATCH)
    assert str(caught.value) == "second: nope"
    assert caught.value.stage == "second"
