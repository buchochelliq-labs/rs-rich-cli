# Markdown for GitHub

A table printed by a CI job reads well in the terminal and badly in a pull
request comment, where its box-drawing characters land in a proportional
font. `rich_ext::markdown_export` writes renderables as GitHub-flavoured
Markdown instead, so GitHub draws the table, the list and the code block
itself. No feature flag is needed.

```rust
--8<-- "crates/rich-ext/examples/guide_markdown_export.rs:document"
```

gives

````markdown
## CI report

**Checks**

| check | time | result |
| --- | ---: | :---: |
| lint | 1.2 s | **pass** |
| unit \| integration | 48 s | ~~flaky~~ |

---

- **changed**
  - crates/rich-ext
    - `markdown_export.rs`

```sql
SELECT count(*) FROM runs;
```
````

`GfmDocument` collects blocks and separates them with blank lines; `push`
takes anything that implements `ToGfm`, and `GfmExporter::export` writes a
single value. Each renderable becomes:

| renderable | Markdown |
|---|---|
| `Table` | a table: header row, an alignment row from each column's justify, `\|` escaped, cell styles as inline Markdown; a title in bold above, a caption in italics below, a footer as the last row |
| `Tree` | nested lists; a hidden root lists its children, a collapsed node hides them |
| `Panel` | a blockquote with the title in bold, or a `<details>` block (below) |
| `Rule` | `---`, with its title in bold after it |
| `Text`, a markup string | paragraphs: line breaks become hard breaks, blank lines paragraph breaks |
| `Syntax` (feature `syntax`) | a fenced block with its language |
| `Renderables` (a group) | each item as its own block |
| `Columns`, and anything else through `Plain` or `push_plain` | the plain rendering in a ```` ```text ```` fence |

`push_markdown` takes Markdown as it is: a core `Markdown` does not give its
source back, so push the string you built it from. `push_code` writes a
fence, and `push_heading` a heading from console markup.

## Styles

Styles become inline Markdown where Markdown has an equivalent: bold `**`,
italic `*`, strike `~~` and links `[text](url)`. Colour, dim, underline and
reverse have none, so they are dropped. Inline code comes from style names:
a span styled `code`, `markdown.code` or `inline_code` (change the list with
`GfmOptions::code_styles`) becomes `` `code` ``, with a longer backtick fence
when the code holds backticks.

Text that Markdown would read as syntax is escaped: `*`, `_`, `` ` ``, `[`,
`<`, `|`, `&` and the rest anywhere, and `#`, `-`, `+` and `1.` at the start
of a line. Leading spaces become no-break spaces, so an indented line does
not turn into a code block.

## Panels

A panel is a blockquote by default. `PanelStyle::Details` makes it a
collapsible block, which suits long logs in a comment:

```rust
--8<-- "crates/rich-ext/examples/guide_markdown_export.rs:panels"
```

```markdown
> **Lint**
>
> 3 warnings\
> 1 note
```

```markdown
<details><summary>Lint</summary>

3 warnings\
1 note

</details>
```

A panel around a table or another drawing keeps the drawing in a
```` ```text ```` fence inside the block.

## How tables are read

The core `Table` is a faithful port of upstream's and has no accessors for
its cells. The exporter renders it wide and reads the cells back from the
rendering, between the column borders of its top edge: the text, its
styles, and where the text sits in the column, which gives the alignment
(left is GitHub's default `---`, right `---:`, centred `:---:`).

Two kinds of table cannot be read that way and are written as their plain
rendering in a ```` ```text ```` fence instead:

- one with no borders to read: `Table::grid()`, `show_edge(false)`, or a box
  whose edges are blank;
- one whose rows cannot be told apart: a cell of two or more lines in a
  table without `show_lines`, where the lines look like rows.

Header cells are written without their style (GitHub bolds headers itself).
A footer is told from a row by the `table.footer` bold it is drawn in.

## GitHub's limit

A comment holds at most 65 536 characters (`GITHUB_COMMENT_LIMIT`).
`fit_github_comment` returns Markdown that fits unchanged; longer Markdown is
cut at a line break, any code fence or `<details>` block left open is
closed, and a note says how much was kept:

```text
*Truncated: 65412 of 120007 characters shown.*
```

`truncate_markdown(markdown, limit)` does the same for another limit.
