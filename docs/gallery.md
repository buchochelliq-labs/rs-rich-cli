# Gallery

These are committed snapshots of actual core library output, exported with
`Console::export_svg` by `scripts/capture_screenshots.sh`. Regenerate them when
rendering changes. Progress and spinner GIFs replay the exported SVG frames.
The [extensions, art and CLI](#beyond-the-core) are shown at the end, and every
guide page has more.

| What you want to do | Start here |
|---|---|
| Animate images in the terminal | [CLI and art videos](demos.md) |
| Style text | [Markup tutorial](tutorial/02-markup.md) |
| Present structured data | [Table tutorial](tutorial/03-tables.md) |
| Arrange output | [Layout tutorial](tutorial/04-layout.md) |
| Show running work | [Progress and live output](tutorial/05-live.md) |
| Format files from the shell | [CLI guide](cli.md) |

## Rich-art in motion

![Half-block, Braille and ASCII renderings of the same image](assets/demos/release-image-modes.gif)

[Watch or download the videos](demos.md), including fresh image, fit/crop, watch and batch clips. Capture details are on that page.

## Markup

Tags nest, and `\[` escapes a literal bracket.

![Console markup](assets/markup.svg)

## Automatic highlighting

No markup required — numbers, paths, URLs, UUIDs and booleans are recognised and
coloured, exactly as upstream does.

![Automatic highlighting](assets/colour.svg)

## Tables

![Table](assets/table.svg)

Columns size themselves to their content, wrap when they must, and take
per-column justification. Borders, titles and captions are all styleable.

![Styled table](assets/table-styled.svg)

## Panels

![Panel](assets/panel.svg)

## Trees

![Tree](assets/tree.svg)

## Columns

Items are packed into as many columns as the width allows, each as wide as its
widest item (`Columns::equal` makes them all the same width).

![Columns](assets/columns.svg)

## Rules

![Rule](assets/rule.svg)

## Alignment and padding

![Alignment](assets/align.svg)

## Markdown

![Markdown](assets/markdown.svg)

## Syntax highlighting

![Syntax highlighting](assets/syntax.svg)

!!! note "Not byte-parity"

    Syntax highlighting uses `syntect`, not Pygments. The colours are close but
    not identical to Python `rich` — see [Divergences](DIVERGENCES.md).

## JSON

![JSON](assets/json.svg)

## Pretty-printing

![Pretty](assets/pretty.svg)

## Progress

![Progress](assets/demos/progress.gif)

## Spinners

![Spinner](assets/demos/spinner.gif)

## Text overflow

The same over-long word under each overflow method.

![Overflow](assets/overflow.svg)

## Beyond the core

These come from `rs-rich-ext`, `rs-rich-art` and the `rich` command. Each image
is the real output of a guide example or CLI command; follow the link for the
code.

### Diagnostics and stack traces

![A chained Python traceback, cause first](media/guide/guide_diagnostics-trace-python.svg)

[Diagnostics guide](guide/ext/diagnostics.md)

### Structured data

![A document explored as a tree](media/guide/guide_data-explorer.svg)

[Structured data guide](guide/ext/structured-data.md)

### Workflows

![A task tree of running steps](media/guide/guide_workflow-tree.svg)

[Workflows guide](guide/ext/workflows.md)

### Diffs

![A unified diff with highlighted changes](media/guide/guide_diff-unified.svg)

[Diffs and test reports guide](guide/ext/diffs-and-test-reports.md)

### Images

![One image in each colour mode](media/guide/guide_images-color-modes.svg)

[Images guide](guide/art/images.md)

### CLI viewers

![rich inspect drawing a YAML file as a tree](media/guide/cli_inspect.svg)

![rich view showing a Python file with line numbers](media/guide/cli_view.svg)

[CLI walkthrough](guide/cli/walkthrough.md)
