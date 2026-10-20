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

*Coming later:* a small terminal where `rich` commands run against sample
files (`rich data.csv --csv`, `echo '# Hi' | rich - --markdown`), from the
CLI compiled for WASI.

## Terminal apps

*Coming later:* intuiTUIve apps and the interactive commands (`choose`,
`pager`, `explore`), driven with the keyboard and the mouse.
