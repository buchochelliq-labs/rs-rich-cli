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
| `Markdown` (feature `markdown`) | its source, as it is |
| `Columns`, and anything else through `Plain` or `push_plain` | the plain rendering in a ```` ```text ```` fence |

`push_markdown` takes Markdown source as it is, and `push(&markdown)` writes
a core `Markdown` as the source it was parsed from. `push_code` writes a
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

A panel's title and subtitle are read from the panel. Its body is
written as paragraphs when it is a `Text` (or prints as one); any other
body is rendered and read back, and a table or another drawing stays a
drawing in a ```` ```text ```` fence inside the block.

## How content is read

Tables, panels, rules and Markdown are read through core's read-only
accessors, which mirror upstream's public attributes: `Table::columns`
(each column's header, footer, justify and style), `Table::rows` (each
row's cells and style), `get_title`, `get_caption`, `get_show_header`
and `get_show_footer`; `Panel::get_title`, `get_subtitle` and
`renderable`; `Rule::get_title`; `Markdown::markup`. So every table is
written exactly, whatever its box, width or wrapping: a grid, a table
without edges and a cell of several lines (joined with `<br>`) included.
The alignment row comes from each column's justify (left is GitHub's
default `---`, right `---:`, centred `:---:`).

Header cells are written without their style (GitHub bolds headers
itself); body cells keep their own styles over the column's and row's; a
footer is written as the last row, in the table's `footer_style`. A cell
that is a renderable with no text is its plain rendering.

Only renderables that expose nothing are still rendered and read back, or
written plain: `Columns`, a panel's body that is not text, and anything
passed to `Plain`.

## GitHub's limit

A comment holds at most 65 536 characters (`GITHUB_COMMENT_LIMIT`).
`fit_github_comment` returns Markdown that fits unchanged; longer Markdown is
cut at a line break, any code fence or `<details>` block left open is
closed, and a note says how much was kept:

```text
*Truncated: 65412 of 120007 characters shown.*
```

`truncate_markdown(markdown, limit)` does the same for another limit.
