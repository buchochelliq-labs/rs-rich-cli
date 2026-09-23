import unittest

import diff_rich


class DiffRichTests(unittest.TestCase):
    def test_generated_cases_are_stable(self):
        self.assertEqual(
            diff_rich.generated_cases(seed=7, count=3),
            diff_rich.generated_cases(seed=7, count=3),
        )

    def test_corpus_has_named_cases(self):
        cases = diff_rich.load_cases(diff_rich.DEFAULT_CORPUS)
        self.assertGreaterEqual(len(cases), 7)
        self.assertTrue(all(case.get("name") for case in cases))

    def test_self_test_mutation_detects_mismatch_without_rerendering(self):
        case = {
            "kind": "markup",
            "name": "unit",
            "source": "x",
            "width": 10,
            "color_system": "truecolor",
        }

        def fake_rust_render(cases, command):
            return [{"ok": True, "output": diff_rich.python_render(cases[0])}]

        original = diff_rich.rust_render
        try:
            diff_rich.rust_render = fake_rust_render
            self.assertTrue(diff_rich.mismatches([case], ["unused"], mutate_oracle=True))
            self.assertFalse(diff_rich.mismatches([case], ["unused"], mutate_oracle=False))
        finally:
            diff_rich.rust_render = original

    def test_generator_covers_every_kind(self):
        kinds = {case["kind"] for case in diff_rich.generated_cases(seed=3, count=400)}
        self.assertEqual(kinds, set(diff_rich.KINDS))

    def test_generated_tables_are_rectangular(self):
        for case in diff_rich.generated_cases(seed=9, count=300):
            if case["kind"] == "table":
                for row in case["rows"]:
                    self.assertEqual(len(row), len(case["columns"]))

    def test_oracle_renders_each_colour_system_in_isolation(self):
        # rich memoises a Style's SGR codes on the instance whatever the colour
        # system; one shared interpreter printed #ff8800 as `91` under truecolor
        # once a standard console had rendered it.
        cases = [
            {"kind": "markup", "name": "std", "source": "[#ff8800]x[/]", "width": 10,
             "color_system": "standard"},
            {"kind": "markup", "name": "true", "source": "[#ff8800]x[/]", "width": 10,
             "color_system": "truecolor"},
        ]
        standard, truecolor = diff_rich.python_render_all(cases)
        self.assertIn("\x1b[91m", standard["output"])
        self.assertIn("38;2;255;136;0", truecolor["output"])

    def test_outcome_classifies_errors_and_outputs(self):
        ok = {"ok": True, "output": "a"}
        self.assertIsNone(diff_rich.outcome(ok, ok))
        self.assertIsNone(diff_rich.outcome({"ok": False}, {"ok": False}))
        self.assertEqual(diff_rich.outcome({"ok": False}, ok), "python_error")
        self.assertEqual(diff_rich.outcome(ok, {"ok": False}), "rust_error")
        self.assertEqual(diff_rich.outcome(ok, {"ok": True, "output": "b"}), "output")

    def test_shrink_keeps_the_failure_kind(self):
        # A fake comparison: output differs while "z" is present, and removing
        # the "z" would instead produce a different failure kind.
        def fake(cases, command, mutate):
            case = cases[0]
            kind = "output" if "z" in case["source"] else "python_error"
            return [{"case": case, "kind": kind}]

        original = diff_rich.mismatches
        try:
            diff_rich.mismatches = fake
            case = {"kind": "markup", "name": "s", "source": "abzcd", "width": 5,
                    "color_system": "truecolor"}
            self.assertEqual(diff_rich.shrink(case, "output", [], False)["source"], "z")
        finally:
            diff_rich.mismatches = original

    def test_candidates_shrink_tables_structurally(self):
        case = {"kind": "table", "source": "", "columns": [{"header": "ab", "ratio": 2},
                {"header": "c"}], "rows": [["x", "yz"]], "title": "t"}
        trials = list(diff_rich.candidates(case))
        self.assertIn({**case, "rows": []}, trials)
        self.assertIn({**case, "columns": [{"header": "c"}], "rows": [["yz"]]}, trials)
        self.assertIn({**case, "columns": [{"header": "ab"}, {"header": "c"}]}, trials)
        self.assertTrue(any("title" not in trial for trial in trials))

    def test_known_divergences_name_their_issue(self):
        known = diff_rich.load_cases(diff_rich.ROOT / "scripts/fixtures/diff_rich_known.jsonl")
        self.assertTrue(known)
        corpus = diff_rich.load_cases(diff_rich.DEFAULT_CORPUS)
        names = [case["name"] for case in corpus + known]
        self.assertEqual(len(names), len(set(names)))
        for case in known:
            self.assertIsInstance(case.get("issue"), int, case["name"])


if __name__ == "__main__":
    unittest.main()
