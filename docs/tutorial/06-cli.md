# 6. The CLI

```bash
cargo install rs-rich-cli      # installs a binary called `rich`
```

The pictures on this page are the commands' own output, exported with
`--export-svg` from the fixtures `scripts/smoke_cli.py` writes
(`python3 scripts/smoke_cli.py --screenshots docs/media/guide`).

## Rendering a file

With no mode flag, the type is detected from the extension:

```bash { .rich-playground data-kind="cli" }
rich README.md          # markdown
rich data.json          # pretty JSON
rich main.rs            # syntax highlighted
rich table.csv          # a table
rich notebook.ipynb     # a Jupyter notebook
```

`rich notes.md`, for example:

![Markdown rendered with a centred heading, bullets, a quote and a code block](../media/guide/cli_markdown.svg)

and `rich team.csv --title Team`:

![A table with a title, a header row and a right-aligned numeric column](../media/guide/cli_csv.svg)

Force a mode when the extension lies, or when reading stdin:

```bash { .rich-playground data-kind="cli" }
rich --markdown notes.txt
cat main.rs | rich --syntax -
```

`-` means standard input.

## Printing markup

```bash { .rich-playground data-kind="cli" }
rich -p "[bold red]Alert[/] disk at [bold]91%[/]"
```

![Alert in bold red, then disk at 91%, with the number highlighted](../media/guide/cli_print_alert.svg)

Bad markup is reported rather than printed literally:

```console
$ rich -p "[/nope]"
rich: markup error: closing tag '[/nope]' at position 0 doesn't match any open tag
$ echo $?
4
```

Exit code 4 means a parse or render error in the input; the
[CLI guide](../guide/cli/index.md) lists every exit code.

## Fetching a URL

```bash
rich https://raw.githubusercontent.com/Textualize/rich/master/README.md
```

The render mode comes from the flag, else the URL's extension, else the response
`Content-Type`.

## Decorating output

```bash { .rich-playground data-kind="cli" }
rich --panel rounded --title "Notes" --style "bold blue" notes.md
rich --padding 1,4 --center report.md
rich --width 60 --rule "Section"
```

The first, on the same `notes.md`:

![The Markdown notes in a rounded panel titled Notes, the text in bold blue](../media/guide/cli_panel_notes.svg)

and the last:

![A rule 60 columns wide with Section in the middle](../media/guide/cli_rule_section.svg)

## Exporting

Both exports write to a **path** and leave the terminal output intact, so you get
both at once:

```bash { .rich-playground data-kind="cli" }
rich -m README.md --export-html readme.html
rich -m README.md --export-svg readme.svg
rich -m README.md -o readme.html --export-svg readme.svg   # both
```

`-o` is short for `--export-html`. The HTML is self-contained — no external CSS,
fonts or images. The SVG loads its font from a CDN, so it needs a network
connection to show in the intended typeface; every picture on this site is an
SVG export like this one.

## Paging

```bash { .rich-playground data-kind="cli" }
rich --pager long-document.md
```

Pages through `$PAGER` (falling back to `less -R`, then `more`), keeping the
styling, unlike piping to a pager yourself.

## More commands

Beyond rendering files, `rich` has tool commands: `inspect` explores JSON, YAML,
TOML, XML, INI and dotenv as a tree; `diff` compares text files or renders a
patch; `view` shows any file; `hex`, `unicode` and `ansi explain` look inside
bytes, characters and escape sequences; `env` lists environment variables with
secrets masked; `capture` runs a command and shows or exports its output; and
`doctor` reports what `rich` detected. The [CLI walkthrough](../guide/cli/walkthrough.md)
shows each one. Two of them:

```bash { .rich-playground data-kind="cli" }
rich inspect deploy.yaml
```

![deploy.yaml as a tree of keys and values](../media/guide/cli_inspect.svg)

```bash { .rich-playground data-kind="cli" }
rich diff greet.py greet_v2.py
```

![A unified diff with line numbers and highlighted changes](../media/guide/cli_diff_text.svg)

## Full option list

`rich --help` prints every option, grouped by topic, and `rich COMMAND --help`
shows one command. The [CLI reference](../cli-reference.md) is generated from
that help text.
