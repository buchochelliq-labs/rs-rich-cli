# Compatibility with Rich

`rs_rich` 0.0.1 implements a slice of Rich 15.0.0's API. Within the slice,
output is byte-for-byte Rich's. Outside it, calls raise instead of rendering
something different.

## What is covered

| Rich | rs_rich 0.0.1 |
|---|---|
| `rich.print`, `rich.get_console` | yes |
| `Console` | `file`, `width`, `height`, `color_system`, `force_terminal`, `no_color`, `record`, `highlight`, `emoji`, `safe_box`; `print`, `rule`, `export_text` |
| `Text` | constructor, `from_markup`, `append`, `stylize`, `plain`, `len` |
| `Style` | keyword constructor, `parse`, `+`, `==`, `hash` |
| `Table` | the constructor options and `add_column` options listed in [Table](table.md), `add_row` with `str`/`Text` cells |
| `Panel` | constructor and `Panel.fit` |
| `rich.box` | every box constant |
| `rich.markup` | `escape` |
| `rich.errors` | `ConsoleError`, `MarkupError`, `StyleSyntaxError` |
| `Rule`, `Columns`, `Padding`, `Align`, `Group`, `Markdown`, `Syntax`, `Progress`, `Live`, `Tree`, `Pretty`, `inspect`, logging, tracebacks, `Console.log`, `input`, `status`, `export_html`/`export_svg` | not yet |

## Known differences

| Difference | Why |
|---|---|
| `Console.print` accepts only `end="\n"`, `justify=` only for strings, and no `style=`, `markup=`, `highlight=` or `overflow=` arguments | Not implemented in this slice. It raises `NotImplementedError` or `TypeError`. |
| Consecutive `str` arguments to `print` are joined with `sep` before their markup is read, so a tag can span arguments | Rich parses each separately. Output differs only when a tag spans arguments. |
| Table cells are `str`, `Text` or `None` | Renderables in cells come in a later slice. |
| Hyperlinks carry no `id=` | Rich tags each link with a random id. The Rust port leaves it out so output is reproducible ([Divergences #20](../DIVERGENCES.md)). |
| `repr(box.ROUNDED)` is `box.ROUNDED` | Rich prints `Box(...)` with the box's characters. Boxes compare and render the same. |
| On a terminal, `width=None` uses the process's terminal size | Rich asks the file's own descriptor. They differ only when `file` is a different terminal from standard output. |
| `Console(width=...)` and a table column's `width`, `min_width` and `max_width` are at most 65536, and a column's `ratio` at most 4294967295; larger values raise `ValueError` | Rich accepts them, then runs out of memory or takes minutes to print. The Rust port would abort, overflow or take as long, so the binding refuses them up front. |
| A `Panel`'s padding is at most 65536 on each side; more raises `ValueError` (in `Panel` and `Panel.fit`) | Rich renders any padding: `padding=10**6` prints about 160 MB in some 15 seconds, and a horizontal padding wider than the console squeezes the content out, which rs_rich matches. Up to the limit, output is Rich's. |
| Panels nest at most 100 deep; deeper raises `RecursionError` | Rich also raises `RecursionError`, at a depth that depends on Python's recursion limit (between 100 and 150 by default). |
| A `print` from inside the same console's `file.write` raises `RuntimeError` | Rich recurses until it hits Python's recursion limit. |

## How compatibility is tested

`crates/rich-py/tests` holds the tests, run by the `python` workflow on Python
3.9 and 3.13:

| File | What it checks |
|---|---|
| `test_compat.py` | Each program runs twice, once with Rich 15.0.0's modules and once with `rs_rich`'s, and the output must be identical, in truecolor and without colour. The programs cover markup and highlighting, `Text`, styles, Rich's README table, table options, header-less tables, panels, rules, justification, `export_text` and links (except for Rich's random ids). |
| `test_api.py` | Rich's README example (`examples/star_wars.py`) runs with only its imports changed, with identical output; errors and refusals. |
| `test_console.py`, `test_text.py`, `test_style.py`, `test_table.py`, `test_panel.py`, `test_modules.py` | Each class's arguments, defaults, validation, errors and exact output. They also check that the type stubs describe exactly the compiled module. |
| `test_docs.py` | Runs every example in these pages and compares its output with the page. |

The tests assert that the installed reference really is Rich 15.0.0.
