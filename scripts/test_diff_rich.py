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


if __name__ == "__main__":
    unittest.main()
