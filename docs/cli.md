# CLI reference

The `rich` command, installed by `cargo install rs-rich-cli`.

## Rendering a file

With no mode flag, the type is detected from the extension:

```bash
rich README.md          # markdown
rich data.json          # pretty JSON
rich main.rs            # syntax highlighted
rich table.csv          # a table
rich notebook.ipynb     # a Jupyter notebook
```

Force a mode when the extension lies, or when reading stdin:

```bash
rich --markdown notes.txt
cat main.rs | rich --syntax -
```

`-` means standard input.

## Printing markup

```bash
rich -p "[bold red]Alert[/] disk at [bold]91%[/]"
```

Bad markup is reported rather than printed literally:

```console
$ rich -p "[/nope]"
rich: markup error: closing tag '[/nope]' at position 0 doesn't match any open tag
$ echo $?
1
```

## Fetching a URL

```bash
rich https://raw.githubusercontent.com/Textualize/rich/master/README.md
```

The render mode comes from the flag, else the URL's extension, else the response
`Content-Type`.

## Decorating output

```bash
rich --panel rounded --title "Notes" --style "bold blue" notes.md
rich --padding 1,4 --center report.md
rich --width 60 --rule "Section"
```

## Exporting

Both exports write to a **path** and leave the terminal output intact, so you get
both at once:

```bash
rich -m README.md --export-html readme.html
rich -m README.md --export-svg readme.svg
rich -m README.md -o readme.html --export-svg readme.svg   # both
```

`-o` is short for `--export-html`. The output is self-contained — no external
CSS, fonts or images — which is exactly how every picture on this site is made.

## Paging

```bash
rich --pager long-document.md
```

Pages through `$PAGER` (falling back to `less -R`, then `more`), keeping the
styling, unlike piping to a pager yourself.

## Full option list

```console
--8<-- "docs/assets/cli-help.txt"
```
