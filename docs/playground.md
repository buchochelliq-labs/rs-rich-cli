# Playground

Try rs-rich in your browser, with nothing to install. The library is
compiled to WebAssembly and runs in this tab: what you type never leaves it.

Pick a kind of input, edit it, and the output updates as you type. **Copy
link** puts what you typed in the link (compressed, in the part after `#`,
which is never sent to a server), so you can share it.

<div class="rp-page" markdown>

```text { .rich-playground data-kind="markup" data-kinds="all" data-width="80" }
[bold magenta]Hello[/], [u]world[/]! :rocket:
[#ff8800]#ff8800[/] [white on dark_blue] white on dark_blue [/] [black on #ffcc00] on hex [/]
[link=https://github.com/Textualize/rich]a hyperlink[/link], [s]struck[/s], [reverse] reverse [/]
```

</div>

Without JavaScript, or on a docs version built without the playground, the
block above is the markup itself. The guide's examples have the same
**Try it** button wherever their input is text, with the Rust that renders
it beside them; [Text and style](guide/core/text-and-style.md) is a good
place to start.

## What each kind takes

| Kind | Input | Rust |
|---|---|---|
| `markup` | console markup, one `print_str` per line | `console.print_str(line)` |
| `panel` | markup, inside a panel | `Panel::new(Box::new(Text::from_markup(..)?))` |
| `rule` | one rule per line, its title in markup | `Rule::new(line)` |
| `columns` | one item per line | `Columns::from_cells(..)` |
| `table` | CSV: a header line, then rows; cells are markup | `Table::new()`, `add_column`, `add_row_text` |
| `tree` | an indented outline; labels are markup | `Tree::new(..)`, `add(..)` |
| `json` | a JSON document | `Json::new(..)?` |
| `syntax` | source code | `Syntax::new(code, "rust")` |
| `markdown` | Markdown | `Markdown::new(..)` |
| `sparkline`, `histogram` | numbers | `rich_ext::chart::{Sparkline, Histogram}` |
| `bars` | `label,value` lines | `rich_ext::chart::BarChart::from_pairs(..)` |
| `diff` | the old text, a `=======` line, the new text | `rich_ext::diff::DiffView::new(old, new)` |
| `data` | a JSON or YAML document | `rich_ext::data::Explorer::new(&doc)` |
| `data-diff` | two documents, split by `=======` | `rich_ext::data::DiffView::new(&old, &new)` |

The output is drawn from rs-rich's own HTML export
(`Console::export_html`); **Terminal view** draws the ANSI text with
xterm.js instead, as a terminal would.

## The `rich` command

A terminal where the `rich` command runs: the real CLI, compiled for WASI
and run in a worker in this tab, with a few sample files (`ls` lists them,
`help` says what else there is). **Try it** opens it with the commands
below filled in at the prompt, one at a time: press Enter to run each.

```bash { .rich-playground data-kind="cli" data-rows="26" }
rich README.md
rich sales.csv --title "Sales"
rich chart sales.csv --kind bar --x month --y api
echo '# Hello from a pipe' | rich - --markdown
rich main.rs --line-numbers
```

The prompt understands pipes (`|`), `;`, `&&`, `||`, `<`, `>` and `>>`,
and a few commands of its own: `echo`, `printf`, `seq`, `cat`, `ls`,
`clear`, `pwd`, `help` and `history`. Up and Down recall what you typed.

- **Stop** ends a command that runs too long (an endless `--loop 0` GIF,
  say): it starts a fresh session, and the files go back to the samples.
- **Save as tape** downloads what you typed as `session.tape`, with the
  sample files it used as `Write` lines. A native `rich record session.tape`
  replays it and records it ([Recordings](recordings.md)).

Some things need a real computer and say so here: URLs (no network),
`--watch`, `--batch`, `--pager`, `rich capture` and `rich record` (no
processes), `--demo`, and the interactive commands (`choose`, `filter`,
`explore`, …), which need a terminal's raw keys at this prompt; they run
on their own under [Terminal apps](#terminal-apps) below. Every CLI
example in these docs that runs here has the same **Try it** button.

## Terminal apps

The interactive commands and intuiTUIve apps run here too, in a terminal
(xterm.js) you drive with the keyboard and the mouse. **Run it** starts
one; click the terminal to type into it. Each runs in a Web Worker, so
**Stop** ends it whatever it is doing, and **Restart** runs it again on
what is in the box above it.

### Pick from a list: `rich choose` and `rich filter`

Arrow keys or the mouse move, Enter picks; type to narrow the list. Esc
cancels and Ctrl+C interrupts, as in a shell.

```text { .rich-playground data-kind="app" data-app="choose" data-width="60" data-rows="10" }
Rust
Python
TypeScript
Go
Zig
Haskell
```

`rich choose --multi`: Tab marks a line and moves down, Enter returns the
lines marked.

```text { .rich-playground data-kind="app" data-app="choose-multi" data-width="60" data-rows="10" }
syntax highlighting
tables
progress bars
Markdown
trees
panels
```

### Read a long text: `rich pager`

Arrow keys, Page Up and Down, `g` and `G` for the ends, `/` to search, `q`
to quit.

```text { .rich-playground data-kind="app" data-app="pager" data-width="72" data-rows="12" }
rs-rich is a Rust port of Python's rich: styled text, tables, trees,
panels, progress bars, Markdown and syntax highlighting in the terminal.

rs-rich-interact adds interactive components on top of it: pickers,
inputs, confirmations, forms, a pager and a data explorer. The rich
command line uses them for `rich choose`, `rich filter`, `rich input`,
`rich confirm`, `rich pager` and `rich explore`.

intuiTUIve is a framework for whole terminal apps, built on both:
signals and a retained tree, a cell screen with damage tracking, focus,
mouse and accessibility.

Everything on this page runs in your browser tab, compiled to
WebAssembly. Nothing you type is sent anywhere.

The pager wraps long lines to the terminal's width, keeps the position
when the terminal is resized, and searches with `/`, `n` and `N`.
```

### Browse a document: `rich explore`

A JSON or YAML document as a tree: arrow keys open and close branches,
Enter returns the path of the line you are on.

```text { .rich-playground data-kind="app" data-app="explore" data-width="72" data-rows="14" }
{
  "server": {"host": "localhost", "port": 8080, "tls": false},
  "users": [
    {"name": "ada", "roles": ["admin"]},
    {"name": "grace", "roles": ["dev", "ops"]}
  ],
  "debug": true
}
```

### intuiTUIve apps

The [tutorial](guide/intuituive/tutorial.md)'s to-do list, unchanged, on
the lines below: type a to-do and Enter adds it, Tab moves to the list,
Space ticks, `d` deletes, Ctrl+Q quits, and the mouse works.

```text { .rich-playground data-kind="app" data-app="todo" data-width="72" data-rows="14" }
Read the intuiTUIve tutorial
Run it in a browser
Water the plants
```

Three more of intuiTUIve's examples: a form that checks what you type, level
meters, and sprites on a stage, all on the app's clock. They take no input.

```text { .rich-playground data-kind="app" data-app="form" data-width="72" data-rows="16" }
```

```text { .rich-playground data-kind="app" data-app="meters" data-width="72" data-rows="14" }
```

```text { .rich-playground data-kind="app" data-app="sprites" data-width="72" data-rows="16" }
```

In the browser there is no file system, no other threads and no wall
clock, so an app that reads files, runs work on a thread
([`latest`](https://docs.rs/rs-rich-intuituive/latest/rich_intuituive/fn.latest.html)),
or holds a meter's peak on the wall clock does less here than in a
terminal; see [the design](design/docs-playground.md#tier-3-intuituive-apps-with-keyboard-and-mouse).
