# Compatibility with Rich

`rs_rich` implements Rich 15.0.0's API. Its output is Rich's, byte for byte,
except for the differences listed below. What the port cannot do raises
instead of rendering something different. The port's own crates (`ext`,
`art`, `mermaid`, `plugins`, the command line) have no Rich counterpart; their
output is compared with the Rust crates'.

## What is covered

| Rich | rs_rich |
|---|---|
| `rich.print`, `rich.get_console`, `rich.print_json`, `rich.reconfigure`, `rich.inspect` | yes (`reconfigure` replaces the global console object) |
| `Console` | everything but Jupyter output: the constructor (with `highlighter`, `tab_size`, `emoji_variant`), `print`, `log` (any renderable, `log_locals`), `out`, `rule`, `line`, `input`, `print_json` (every option), `capture`, `with console:`, `measure`, `render`, `render_lines`, `render_str`, themes, the exports (with `code_format` and `font_aspect_ratio`), render hooks and the live stack, `status`, `pager`, `screen`, `print_exception`, `set_window_title`, terminal control ([Console](console.md)) |
| The render protocol | `__rich__`, `__rich_console__`, `__rich_measure__`; `ConsoleOptions`, `Measurement`, `Segment` ([The render protocol](protocol.md)) |
| `Text` | the whole class, `Span`, `Lines`, meta data ([Text](text.md)) |
| `Style`, `rich.theme` | the whole class, `StyleStack`; `Theme`, `from_file`, `read`, `config`, `ThemeStack` ([Style](style.md)) |
| `rich.color` | `Color`, `ColorTriplet`, `ColorSystem`, `ColorType`, `ColorParseError`, `parse_rgb_hex`, `blend_rgb` ([Color](color.md)) |
| `rich.markup`, `rich.emoji` | `escape`, `render`, `Tag`; `Emoji`, `NoEmoji` |
| `rich.box`, `rich.errors`, `rich.terminal_theme` | every box, exception and palette |
| `Table` | the constructor and `add_column` options core supports, `Table.grid`, rows of any renderable; see below for the rest ([Table](table.md)) |
| `Panel` | every option, around any renderable ([Panel](panel.md)) |
| `Rule`, `Padding`, `Align`, `VerticalCenter`, `Constrain`, `Styled`, `Bar`, `Spinner`, `SPINNERS` | yes ([Rules, padding, alignment and bars](rule.md)) |
| `Columns`, `Group`, `group`, `Layout` (splitters, `Region`, `LayoutRender`), `containers.Renderables`, `measure_renderables` | yes, except `Layout.refresh_screen` ([Layout, columns and groups](layout.md)) |
| `Tree` | yes ([Tree](tree.md)) |
| `Markdown` | yes, with `highlighter=` and `fences=` for the port's code highlighters ([Markdown](markdown.md)) |
| `Syntax` | every option ([Syntax](syntax.md)); colours as below |
| `Pretty`, `pprint`, `pretty_repr`, `install`, `JSON`, `inspect`, `rich.highlighter` | yes; containers, dataclasses and `__rich_repr__` objects print as in Rich ([Pretty, JSON, inspect and highlighters](pretty.md)) |
| `Traceback`, `print_exception`, `install` | yes ([Traceback](traceback.md)) |
| `Live`, `LiveRender`, `Status`, `Screen`, `Pager` | yes ([Live, status, screen and pager](live.md)) |
| `Progress` (every column, `track`, `wrap_file`, `open`), `ProgressBar` | yes ([Progress](progress.md)) |
| `rich.prompt` | `Prompt`, `Confirm`, `IntPrompt`, `FloatPrompt`, `InvalidResponse` ([Prompts](prompt.md)) |
| `RichHandler` | yes ([Logging](logging.md)) |
| rich-cli's `rich` command | `python -m rs_rich` and the `rich-rs` script: the rs-rich `rich` binary, byte for byte ([The command line](cli.md)) |

The port's own crates:

| Crate | rs_rich |
|---|---|
| `rs_rich.ext.*` (38 modules) | No Rich counterpart; output compared byte for byte with `rs-rich-ext` ([Extensions](ext/index.md)) |
| `rs_rich.art` (images, FIGlet, GIFs, image diff) | No Rich counterpart; matches `rs-rich-art` byte for byte. Printing `ImageArt` is strict: it raises `ImageArtError` rather than falling back to ASCII ([Art](art.md)) |
| `rs_rich.mermaid` | No Rich counterpart; matches `rs-rich-mermaid`. The `mmdc` backend only in wheels built with `mmdc` ([Mermaid](mermaid.md)) |
| `rs_rich.plugins` | No Rich counterpart: the `rs-rich-plugin-api` contract and rich-ext's `ExtensionRegistry`. Python plugins go through the Rust host and match the Rust plugins' output ([Plugins](plugins.md)) |

## Known differences

| Difference | Why |
|---|---|
| Code colours (`Syntax`, and code in `Markdown` and `Traceback`) come from syntect, not Pygments: `monokai` is not a theme, and some token classes differ | The port highlights with syntect ([Divergences #18](../DIVERGENCES.md)); layout is Rich's byte for byte ([Syntax: colours](syntax.md#colours)). |
| `Table`'s `width`, `min_width`, footers, `leading`, `row_styles`, table-level `header_style`/`title_style`/`caption_style`, `title_justify`/`caption_justify`, row styles and sections raise `NotImplementedError`; a `Panel` subtitle must be a `str` | Core's `Table` and `Panel` have no such options yet. |
| `Console(force_jupyter=True)` raises `NotImplementedError` | There is no Jupyter output. |
| `Layout.refresh_screen` raises `NotImplementedError` | It needs a live screen the bindings' layout does not hold. |
| `export_svg(unique_id=None)` gives a different (stable) id | Rich derives the default id from Python reprs. With an explicit `unique_id` the SVG is Rich's. |
| Hyperlinks carry no `id=` | Rich tags each link with a random id. The Rust port leaves it out so output is reproducible ([Divergences #20](../DIVERGENCES.md)). |
| `text.spans` returns a copy | Spans live in the core `Text`; assign `text.spans` to change them. |
| Meta data on `Text` spans holds `None`, `bool`, `int`, `float`, `str` and lists or tuples of them (a tuple reads back as a list); other values raise `TypeError` | Core's style meta keeps that subset of what `marshal` can store. A `Style` alone keeps any meta. |
| `Spinner.render()` with renderable text returns a grid, not a `Table`; `Status.renderable` is not a `Spinner` | The live area's spinner is core's; both print the same. |
| `Progress.make_tasks_table()` returns a renderable grid; `get_table_column()` returns `None` for a column made without `table_column=`; `SpinnerColumn` has no `spinner` attribute (use `set_spinner()`) | There is no `rs_rich.table.Column`. |
| A `Text` with a style of its own, rendered justified inside a container (a table cell), has its padding in a separate ANSI run | Core renders the padding as a second segment in the same style; the terminal shows the same. |
| The theme stack, and `capture()`, belong to the console | Rich keeps the theme stack per thread. Captures are per thread, as in Rich. |
| `repr(box.ROUNDED)` is `box.ROUNDED` | Rich prints `Box(...)` with the box's characters. Boxes compare and render the same. |
| `Console(width=...)` and a table column's `width`, `min_width` and `max_width` are at most 65536, and a column's `ratio` at most 4294967295; larger values raise `ValueError` | Rich accepts them, then runs out of memory or takes minutes to print. The Rust port would abort, overflow or take as long, so the binding refuses them up front. |
| A `Panel`'s padding is at most 65536 on each side; more raises `ValueError` | Rich renders any padding, slowly; up to the limit output is Rich's. |
| Renderables nest at most 100 deep; deeper raises `RecursionError` | Rich also raises `RecursionError`, at a depth that depends on Python's recursion limit. |
| A `print` from inside the same console's `file.write` raises `RuntimeError` | Rich recurses until it hits Python's recursion limit. |
| The command is `rich-rs`, not `rich` | `rich` is installed by rich-cli. |

## How compatibility is tested

`crates/rich-py/tests` holds the tests, run by the `python` workflow on Python
3.9 and 3.13 against the compiled wheel:

| File | What it checks |
|---|---|
| `test_compat.py`, `test_protocol.py`, `test_integration.py` | Programs written once against Rich's API run under Rich 15.0.0 and under `rs_rich`, in colour and without, and the output must be identical: printing, the console's options, exports, the render protocol, highlighters, `log`, `print_json`, render hooks and live displays, tables and panels. |
| `test_text_style.py`, `test_renderables.py`, `test_code.py`, `test_live.py` | The same comparison for each area: `Text`, `Style`, colours and themes; rules, layout, columns and trees; Markdown, Syntax, Pretty and tracebacks; live displays, progress, prompts and logging. |
| `test_ext*.py`, `test_art.py`, `test_mermaid.py`, `test_plugins.py` | The port's crates, compared with what the Rust crates render for the same input (expected outputs generated by Rust; see `crates/rich-py/oracles`). |
| `test_cli.py` | `python -m rs_rich` against the `rich` binary built from the same source: stdout, stderr and exit status. |
| `test_api.py`, `test_console.py`, `test_text.py`, `test_style.py`, `test_table.py`, `test_panel.py`, `test_modules.py` | Rich's README example runs with only its imports changed; each class's arguments, validation and errors; the type stubs describe exactly the compiled module, and every module path exists. |
| `test_docs.py` | Runs every example in these pages (and `ext/`) and compares its output with the page. |

The tests assert that the installed reference really is Rich 15.0.0.
