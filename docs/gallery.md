# Gallery

Everything here is rendered by the library and exported with
`Console::export_svg`, by `scripts/capture_screenshots.sh`. If rendering changes,
these images change with it — they cannot go stale.

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

Items are packed into as many equal columns as the width allows.

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

![Progress](assets/progress-animated.svg)

## Spinners

![Spinner](assets/spinner-animated.svg)

## Text overflow

The same over-long word under each overflow method.

![Overflow](assets/overflow.svg)
