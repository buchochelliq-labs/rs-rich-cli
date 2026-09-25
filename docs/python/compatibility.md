# Compatibility with Rich

`rs_rich` implements a growing part of Rich 15.0.0's API. Within it, output is
byte-for-byte Rich's. Outside it, calls raise instead of rendering something
different.

## What is covered

| Rich | rs_rich |
|---|---|
| `rich.print`, `rich.get_console`, `rich.print_json`, `rich.reconfigure` | yes (`reconfigure` replaces the global console object) |
| `Console` | the constructor, `print`, `log`, `out`, `rule`, `line`, `input`, `print_json`, `capture`, `begin_capture`/`end_capture`, `with console:`, `measure`, `render`, `render_lines`, `render_str`, `get_style`, `push_theme`/`pop_theme`/`use_theme`, `export_text`/`export_html`/`export_svg`, `save_text`/`save_html`/`save_svg`, `clear`, `bell`, `show_cursor`, `set_alt_screen`, and the properties in [Console](console.md) |
| The render protocol | `__rich__`, `__rich_console__`, `__rich_measure__`; `ConsoleOptions`, `Measurement`, `Segment` ([The render protocol](protocol.md)) |
| `Text` | constructor, `from_markup`, `append`, `stylize`, `plain`, `len` |
| `Style` | keyword constructor, `parse`, `+`, `==`, `hash` |
| `Theme` | `Theme(styles, inherit=True)`, `styles`, `config` |
| `Table` | the constructor options and `add_column` options listed in [Table](table.md), `add_row` with any renderable |
| `Panel` | constructor and `Panel.fit`, around any renderable |
| `rich.box` | every box constant |
| `rich.markup` | `escape` |
| `rich.errors` | every exception class |
| `rich.terminal_theme` | `TerminalTheme` and Rich's palettes |
| `Rule`, `Columns`, `Padding`, `Align`, `Group`, `Tree`, `Layout`, `Markdown`, `Syntax`, `JSON`, `Pretty`, `inspect`, `Progress`, `Live`, `Status`, prompts, logging, tracebacks | not yet: their modules exist and are empty |

## Known differences

| Difference | Why |
|---|---|
| Printing a container, dataclass or other object Rich pretty-prints raises `NotImplementedError` | `Pretty` is not bound yet. |
| `Console.log` takes text only (strings, `Text`, numbers); another renderable, or `log_locals=True`, raises `NotImplementedError` | Core's `LogRender` takes a `Text` message. |
| `print_json` supports `indent=2`, `highlight=True` and `ensure_ascii=False` only; others raise `NotImplementedError` | Core's `Json` has no such options. |
| `Console(tab_size=...)` other than 8, `emoji_variant`, and `force_jupyter=True` raise `NotImplementedError` | Core has no console tab size or default emoji variant, and no Jupyter output. |
| `export_html` leaves hyperlinks out | Core's HTML export does not write Rich's `<a href>`. |
| `export_svg` differs from Rich where a panel or rule has a title; `unique_id=None` gives a different (stable) id | Core splits a title from the border beside it into two segments where Rich has one, and Rich derives the default id from Python reprs. With an explicit `unique_id` and no titles the SVG is Rich's. A custom `code_format` or `font_aspect_ratio` raises `NotImplementedError`. |
| A `Text` with a style of its own, rendered justified inside a container (a table cell), has its padding in a separate ANSI run | Core renders the padding as a second segment in the same style; the terminal shows the same. |
| The theme stack, and `capture()`, belong to the console | Rich keeps the theme stack per thread. Captures are per thread, as in Rich. |
| Hyperlinks carry no `id=` | Rich tags each link with a random id. The Rust port leaves it out so output is reproducible ([Divergences #20](../DIVERGENCES.md)). |
| `repr(box.ROUNDED)` is `box.ROUNDED` | Rich prints `Box(...)` with the box's characters. Boxes compare and render the same. |
| `Console(width=...)` and a table column's `width`, `min_width` and `max_width` are at most 65536, and a column's `ratio` at most 4294967295; larger values raise `ValueError` | Rich accepts them, then runs out of memory or takes minutes to print. The Rust port would abort, overflow or take as long, so the binding refuses them up front. |
| A `Panel`'s padding is at most 65536 on each side; more raises `ValueError` (in `Panel` and `Panel.fit`) | Rich renders any padding: `padding=10**6` prints about 160 MB in some 15 seconds, and a horizontal padding wider than the console squeezes the content out, which rs_rich matches. Up to the limit, output is Rich's. |
| Renderables nest at most 100 deep; deeper raises `RecursionError` | Rich also raises `RecursionError`, at a depth that depends on Python's recursion limit (between 100 and 150 panels by default). |
| A `print` from inside the same console's `file.write` raises `RuntimeError` | Rich recurses until it hits Python's recursion limit. |

## How compatibility is tested

`crates/rich-py/tests` holds the tests, run by the `python` workflow on Python
3.9 and 3.13:

| File | What it checks |
|---|---|
| `test_compat.py` | Each program runs twice, once with Rich 15.0.0's modules and once with `rs_rich`'s, and the output must be identical, in truecolor and without colour. The programs cover markup and highlighting, `Text`, styles, Rich's README table, table options, header-less tables, panels, rules, justification, every `print` argument, `out`, `line`, console styles and themes, capturing, `render_str`, `print_json`, `measure`/`render`/`render_lines` and `ConsoleOptions`, `log` (except Rich's random link ids), `input`, `export_text`, `export_html`, `export_svg` and the `save_*` files. |
| `test_protocol.py` | User classes with `__rich__`, `__rich_console__` (yielding strings, `Text`, `Segment`s and nested renderables) and `__rich_measure__`, printed alone and inside panels and tables, compared with Rich the same way; and errors, re-entrant calls, recursion and threads during a render. |
| `test_api.py` | Rich's README example (`examples/star_wars.py`) runs with only its imports changed, with identical output; errors and refusals. |
| `test_console.py`, `test_text.py`, `test_style.py`, `test_table.py`, `test_panel.py`, `test_modules.py` | Each class's arguments, defaults, validation, errors and exact output. They also check that the type stubs describe exactly the compiled module, and that every module path exists. |
| `test_docs.py` | Runs every example in these pages and compares its output with the page. |

The tests assert that the installed reference really is Rich 15.0.0.
