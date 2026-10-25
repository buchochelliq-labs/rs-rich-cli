"""Golden cases for workstream 6 of the 0.0.22 plan: documents, `scope.py`.

One fixture, `crates/rich/tests/golden/parity_docs.tsv`:
`name<TAB>json(case)<TAB>json(expected output)`. Each case is data, so the Rust
test (`crates/rich/tests/golden_parity_docs.rs`) rebuilds it without a
builder per name. `capture_golden.py` calls `capture_parity_docs()`.

Python values are written as JSON with a few tagged forms (`python_value`):
plain JSON is what `json.loads` makes, and an object with a `"$"` key is a
tuple, set, frozenset, dict with non-string keys, bytes, special float, big
int, opaque repr, or an object with a dataclass, namedtuple or
`__rich_repr__` repr. `scripts/diff_rich.py` uses the same form.
"""

from __future__ import annotations

import collections
import dataclasses
import json
import pathlib

from rich.console import Console
from rich.pretty import Pretty, pretty_repr
from rich.scope import render_scope

FIXTURE = (
    pathlib.Path(__file__).resolve().parent.parent
    / "crates"
    / "rich"
    / "tests"
    / "golden"
    / "parity_docs.tsv"
)


class _Repr:
    """An atom whose repr is given."""

    def __init__(self, text: str) -> None:
        self.text = text

    def __repr__(self) -> str:
        return self.text


def _rich_repr_class(name: str, angular: bool):
    def __rich_repr__(self):
        for key, value in self._fields:
            if key is None:
                yield value
            else:
                yield key, value

    __rich_repr__.angular = angular  # type: ignore[attr-defined]

    def __init__(self, fields):
        self._fields = fields

    return type(name, (), {"__rich_repr__": __rich_repr__, "__init__": __init__})


def python_value(spec):
    """The Python object a value spec describes."""
    if isinstance(spec, list):
        return [python_value(item) for item in spec]
    if isinstance(spec, dict) and "$" not in spec:
        return {key: python_value(value) for key, value in spec.items()}
    if not isinstance(spec, dict):
        return spec
    tag = spec["$"]
    if tag in ("tuple", "set", "frozenset", "list"):
        items = [python_value(item) for item in spec["items"]]
        return {"tuple": tuple, "set": set, "frozenset": frozenset, "list": list}[tag](items)
    if tag == "dict":
        return {python_value(k): python_value(v) for k, v in spec["items"]}
    if tag == "bytes":
        return bytes.fromhex(spec["hex"])
    if tag == "float":
        return float(spec["value"])
    if tag == "int":
        return int(spec["value"])
    if tag == "repr":
        return _Repr(spec["value"])
    if tag == "obj":
        name, kind = spec["name"], spec["kind"]
        fields = [(key, python_value(value)) for key, value in spec["fields"]]
        if kind == "dataclass":
            cls = dataclasses.make_dataclass(name, [key for key, _ in fields])
            return cls(*[value for _, value in fields])
        if kind == "namedtuple":
            cls = collections.namedtuple(name, [key for key, _ in fields])
            return cls(*[value for _, value in fields])
        return _rich_repr_class(name, kind == "angular")(fields)
    raise ValueError(f"unknown value tag {tag!r}")


def _console(width: int) -> Console:
    return Console(
        force_terminal=True,
        color_system="truecolor",
        width=width,
        highlight=False,
        safe_box=False,
        legacy_windows=False,
        no_color=False,
    )


_JUSTIFY = {None: None, "left": "left", "center": "center", "right": "right", "full": "full"}


def render_case(case: dict) -> str:
    """The output upstream produces for one case."""
    kind = case["kind"]
    options = dict(case.get("options", {}))
    if kind == "pretty_repr":
        return pretty_repr(python_value(case["value"]), **options)
    console = _console(case.get("width", 40))
    if kind == "pretty":
        renderable = Pretty(python_value(case["value"]), **options)
    elif kind == "markdown":
        from rich.markdown import Markdown

        renderable = Markdown(case["source"], **options)
    elif kind == "json":
        from rich.json import JSON

        try:
            if case.get("from_data"):
                renderable = JSON.from_data(json.loads(case["source"]), **options)
            else:
                renderable = JSON(case["source"], **options)
        except Exception as error:  # noqa: BLE001 - the error class is the output
            return f"<{type(error).__name__}>"
    elif kind == "syntax":
        from rich.syntax import Syntax

        options.setdefault("theme", "ansi_dark")
        if "line_range" in options:
            options["line_range"] = tuple(options["line_range"])
        if "path" in case:
            path = pathlib.Path(__file__).resolve().parent.parent / case["path"]
            renderable = Syntax.from_path(str(path), lexer="text", **options)
        else:
            renderable = Syntax(case["code"], "text", **options)
    elif kind == "scope":
        scope = {key: python_value(value) for key, value in case["scope"]}
        renderable = render_scope(scope, **options)
    else:
        raise ValueError(f"unknown case kind {kind!r}")
    wrap = case.get("wrap")
    if wrap == "panel_fit":
        from rich.panel import Panel

        renderable = Panel.fit(renderable)
    elif wrap == "table":
        from rich.table import Table

        table = Table("k", "v")
        table.add_row("x", renderable)
        renderable = table
    with console.capture() as capture:
        console.print(renderable)
    return capture.get()


def _obj(kind: str, name: str, *fields):
    return {"$": "obj", "kind": kind, "name": name, "fields": [list(f) for f in fields]}


def _t(*items):
    return {"$": "tuple", "items": list(items)}


def _set(*items):
    return {"$": "set", "items": list(items)}


NESTED = {
    "version": "1.1",
    "method": "confirmFruitPurchase",
    "params": [["apple", "orange", "mangoes", "pomelo"], 1.123],
    "id": "194521489",
    "flags": _t(True, False, None),
    "empty": [[], {}, _t(), _set(), {"$": "frozenset", "items": []}],
}

ATOMS = [
    None,
    True,
    False,
    0,
    -12,
    {"$": "int", "value": str(2**100)},
    1.5,
    1e16,
    1e-7,
    {"$": "float", "value": "inf"},
    {"$": "float", "value": "nan"},
    "plain",
    "it's",
    "both ' and \"",
    "tab\tnew\nline\\",
    "\x00\x7f\x9f­ é界🙂",
    {"$": "bytes", "hex": "00412227ff5c0a"},
    {"$": "repr", "value": "<object at 0x0>"},
    _t(1),
    _t(),
    {"$": "frozenset", "items": [1, 2]},
    _set(1, 2, 3),
    {"$": "dict", "items": [[1, "a"], [_t(1, 2), "b"], [None, _t("x",)]]},
]

OBJECTS = [
    _obj("dataclass", "Point", ["x", 1], ["y", [1, 2, 3]]),
    _obj("dataclass", "Empty"),
    _obj("namedtuple", "Pair", ["left", "a" * 20], ["right", {"k": "b" * 20}]),
    _obj("namedtuple", "One", ["only", 1]),
    _obj("rich", "Bird", [None, "gull"], ["eats", ["fish", "chips", "ice cream"]], ["fly", True]),
    _obj("angular", "Egg", [None, "white"], ["size", 3]),
    _obj("rich", "Nothing"),
    _obj("angular", "Nothing"),
]

PARITY_DOCS_CASES: list[tuple[str, dict]] = [
    # pretty_repr: the layout alone.
    *[
        (f"pretty_repr_nested_w{width}", {"kind": "pretty_repr", "value": NESTED, "options": {"max_width": width}})
        for width in (10, 30, 50, 80, 200)
    ],
    ("pretty_repr_atoms", {"kind": "pretty_repr", "value": ATOMS, "options": {"max_width": 40}}),
    ("pretty_repr_atoms_flat", {"kind": "pretty_repr", "value": ATOMS, "options": {"max_width": 1000}}),
    ("pretty_repr_objects", {"kind": "pretty_repr", "value": OBJECTS, "options": {"max_width": 40}}),
    ("pretty_repr_objects_wide", {"kind": "pretty_repr", "value": OBJECTS, "options": {"max_width": 400}}),
    ("pretty_repr_indent_2", {"kind": "pretty_repr", "value": NESTED, "options": {"max_width": 30, "indent_size": 2}}),
    ("pretty_repr_expand_all", {"kind": "pretty_repr", "value": [_t(1), [1, [2]], {"a": {}}], "options": {"expand_all": True}}),
    ("pretty_repr_tuple_of_one", {"kind": "pretty_repr", "value": _t(["x" * 30]), "options": {"max_width": 20}}),
    ("pretty_repr_max_length", {"kind": "pretty_repr", "value": [list(range(10)), {str(i): i for i in range(5)}, _set(0, 1, 2, 3)], "options": {"max_length": 2}}),
    ("pretty_repr_max_length_0", {"kind": "pretty_repr", "value": [1, [2, 3]], "options": {"max_length": 0}}),
    ("pretty_repr_max_string", {"kind": "pretty_repr", "value": ["abcdefgh", {"$": "bytes", "hex": "6162636465"}, {"keykeykey": "valuevalue"}, "ab", "界界界界"], "options": {"max_string": 3}}),
    *[
        (f"pretty_repr_max_depth_{depth}", {"kind": "pretty_repr", "value": [NESTED, OBJECTS], "options": {"max_depth": depth, "max_width": 60}})
        for depth in (0, 1, 2, 3)
    ],
    ("pretty_repr_wide_chars", {"kind": "pretty_repr", "value": ["界" * 8, "🙂" * 8, ["é" * 9]], "options": {"max_width": 20}}),
    ("pretty_repr_width_0", {"kind": "pretty_repr", "value": [[1], {"a": _t(2)}], "options": {"max_width": 0}}),
    # Pretty: the renderable.
    ("pretty_nested", {"kind": "pretty", "width": 40, "value": NESTED}),
    ("pretty_atoms", {"kind": "pretty", "width": 60, "value": ATOMS}),
    ("pretty_objects", {"kind": "pretty", "width": 50, "value": OBJECTS}),
    ("pretty_indent_guides", {"kind": "pretty", "width": 30, "value": NESTED, "options": {"indent_guides": True}}),
    ("pretty_indent_guides_2", {"kind": "pretty", "width": 30, "value": NESTED, "options": {"indent_guides": True, "indent_size": 2}}),
    ("pretty_margin", {"kind": "pretty", "width": 40, "value": [1, 2, 3, 4, 5, 6, 7], "options": {"margin": 20}}),
    ("pretty_insert_line", {"kind": "pretty", "width": 20, "value": [[1, 2, 3], "abcdefghijk"], "options": {"insert_line": True}}),
    ("pretty_insert_line_one", {"kind": "pretty", "width": 20, "value": [1], "options": {"insert_line": True}}),
    ("pretty_justify_center", {"kind": "pretty", "width": 30, "value": {"a": [1, 2], "b": "cc" * 8}, "options": {"justify": "center"}}),
    ("pretty_justify_right", {"kind": "pretty", "width": 30, "value": [1, 2], "options": {"justify": "right"}}),
    ("pretty_long_string_fold", {"kind": "pretty", "width": 12, "value": ["a" * 30]}),
    ("pretty_long_string_ellipsis", {"kind": "pretty", "width": 12, "value": ["a b" * 10], "options": {"overflow": "ellipsis"}}),
    ("pretty_no_wrap_none", {"kind": "pretty", "width": 12, "value": "a b c d e f g h", "options": {"no_wrap": None}}),
    ("pretty_no_wrap", {"kind": "pretty", "width": 12, "value": "a b c d e f g h", "options": {"no_wrap": True, "overflow": "crop"}}),
    ("pretty_expand_all", {"kind": "pretty", "width": 40, "value": {"a": [1, 2], "b": _t()}, "options": {"expand_all": True}}),
    ("pretty_limits", {"kind": "pretty", "width": 40, "value": NESTED, "options": {"max_length": 2, "max_string": 4, "max_depth": 2}}),
    ("pretty_in_panel_fit", {"kind": "pretty", "width": 40, "value": NESTED, "wrap": "panel_fit"}),
    ("pretty_in_table", {"kind": "pretty", "width": 40, "value": NESTED, "wrap": "table"}),
    ("pretty_highlights", {"kind": "pretty", "width": 80, "value": ["https://example.com/x?y=1", "/usr/bin/env", "192.168.0.1", "a1b2c3d4-0000-1111-2222-333344445555", {"$": "repr", "value": "<Foo bar=1 baz='x'>"}, -1.5e-3, "#ff0000"]}),
    # scope.py
    ("scope_upstream_demo", {"kind": "scope", "width": 80, "options": {"title": "[i]locals", "sort_keys": False}, "scope": [
        ["foo", 20.3423],
        ["bar", 3.1427],
        ["list_of_things", [1, 2, 3, None, 4, True, False, "Hello World"]],
        ["dict_of_things", {
            "version": "1.1",
            "method": "confirmFruitPurchase",
            "params": [["apple", "orange", "mangoes", "pomelo"], 1.123],
            "id": "194521489",
        }],
    ]}),
    ("scope_sorted", {"kind": "scope", "width": 60, "scope": [
        ["zeta", 1], ["Alpha", "a"], ["__name__", "__main__"], ["beta", [1, 2]], ["_private", None], ["__doc__", None], ["alpha", 2.5],
    ]}),
    ("scope_narrow", {"kind": "scope", "width": 30, "options": {"title": "narrow"}, "scope": [
        ["config", NESTED], ["x", 1],
    ]}),
    ("scope_options", {"kind": "scope", "width": 50, "options": {"indent_guides": True, "max_length": 2, "max_string": 5, "max_depth": 2, "overflow": "ellipsis"}, "scope": [
        ["data", NESTED], ["long_name_here", "a very long string value"], ["objects", OBJECTS],
    ]}),
    ("scope_empty", {"kind": "scope", "width": 40, "scope": []}),
    # markdown.py: tables nested in containers (DIVERGENCES §33).
    *[
        (f"markdown_nested_table_{index}", {"kind": "markdown", "width": 30, "source": source})
        for index, source in enumerate([
            "> a|b\n> -|:\n> x|y\n",
            "> a|b\n> -|-\n> x|y\n",
            "- a|b\n  -|:\n  x|y\n",
            "- item\n\n  a|b\n  -|-\n  x|y\n",
            "1. item\n\n   a|b\n   -|:|-\n",
            "> - a|b\n>   :|-\n>   x|y\n",
            "> > a | b\n> > --|\n",
            "- a|b|c\n  -||-\n",
        ])
    ],
    # markdown.py: refused reference definitions (DIVERGENCES §24).
    *[
        (f"markdown_refused_definition_{index}", {"kind": "markdown", "width": 40, "source": source, "options": {"hyperlinks": False}})
        for index, source in enumerate([
            "[r][1]\n\n[1]: javascript:x\n",
            "[1]: javascript:alert(1)\n[r][1] after\n",
            "a [r][1] b\n\n[1]: vbscript:y \"title\"\n\n[2]: https://ok.example\n\n[s][2]\n",
            "> [1]: javascript:x\n> text\n",
            "  [1]: data:text/html,x\n\n[r][1]\n",
            "[1]: data:image/png;base64,AAA\n\n![i][1]\n",
        ])
    ],
    # syntax.py: `dedent` and `from_path`.
    *[
        (f"syntax_dedent_{index}", {"kind": "syntax", "width": 30, "code": code, "options": {"dedent": True, **extra}})
        for index, (code, extra) in enumerate([
            ("    def f():\n        return 1\n", {}),
            ("    a\n\n      \n    b", {"line_numbers": True}),
            ("\t\tx\n\t\ty\n", {"tab_size": 2}),
            ("\t x\n\t\ty\n", {}),
            ("  a\n b\n", {}),
            ("x\n    y\n", {}),
            ("   \n", {}),
            ("    a\r\n    b\r\n", {}),
        ])
    ],
    ("syntax_no_dedent", {"kind": "syntax", "width": 30, "code": "    a\n    b\n", "options": {}}),
    ("syntax_from_path", {"kind": "syntax", "width": 40, "path": "LICENSE", "options": {"line_numbers": True, "line_range": [1, 3]}}),
    # json.py: `JSON.from_data` and the options.
    *[
        (f"json_{name}", {"kind": "json", "width": 40, "source": source, **extra})
        for name, source, extra in [
            ("from_data", '{"b": [1, 2.5, 1e16, -0.0, "x"], "a": {"n": null, "t": true}, "é": "ü"}', {"from_data": True}),
            ("from_data_sorted_ascii", '{"b": 1, "a": "é\\u2028"}', {"from_data": True, "options": {"sort_keys": True, "ensure_ascii": True, "indent": 4}}),
            ("from_data_flat", '[1, {"a": []}, {}]', {"from_data": True, "options": {"indent": None}}),
            ("from_data_tab_indent", '{"a": [1]}', {"from_data": True, "options": {"indent": "\t"}}),
            ("no_highlight", '{"a": [1, "b"]}', {"options": {"highlight": False}}),
            ("indent_zero", '{"a": [1]}', {"options": {"indent": 0}}),
        ]
    ],
    # markdown.py: found by the differential fuzzer.
    *[
        (f"markdown_fuzz_{index}", {"kind": "markdown", "width": 24, "source": source, "options": options})
        for index, (source, options) in enumerate([
            ("[k]()", {}),
            ("[k](<>) ![i]()", {}),
            ("-", {}),
            ("x\n\n-\n- b\n\n2)", {}),
            ("- a\n-\n- b", {}),
            ("_\na|b\n-|-", {}),
            ("p\nq\na|b\n-|-\nc|d\n\nr", {"hyperlinks": False}),
            ("p\na|b\n-|:|-", {}),
            ("- ![]()", {"hyperlinks": False}),
            ("1. []()\n2.", {}),
        ])
    ],
    ("scope_wide_keys", {"kind": "scope", "width": 24, "scope": [["界界", "界"], ["🙂", [1, 2, 3, 4, 5, 6, 7, 8]]]}),
]


def capture_parity_docs() -> None:
    lines = [
        "# name\tjson(case)\tjson(expected output) — see PARITY_DOCS_CASES in scripts/parity_docs.py"
    ]
    for name, case in PARITY_DOCS_CASES:
        expected = render_case(case)
        lines.append(
            f"{name}\t{json.dumps(case, ensure_ascii=False)}\t{json.dumps(expected, ensure_ascii=False)}"
        )
    FIXTURE.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {len(PARITY_DOCS_CASES)} documents parity cases to {FIXTURE}")


if __name__ == "__main__":
    capture_parity_docs()
