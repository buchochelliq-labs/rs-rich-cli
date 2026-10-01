# Terminal compatibility

A micro asset always takes exactly its cells. What fills them depends on
the terminal, chosen once per terminal by `rich_micro::select` (see
[Drawing on a terminal](index.md#drawing-on-a-terminal)): Kitty, then
iTerm2, then Sixel, then half-blocks, then the emoji or text fallback.
`rich doctor` says which one it picked and why (`micro` in
`--report json`), and `rich micro preview` shows the result.

## The matrix

| Terminal | Drawn with | Animation | Notes |
|---|---|---|---|
| kitty | Kitty Unicode placeholders | native (Kitty frames) | images live in the cell grid: they scroll, survive redraws, and are sent once per session |
| Ghostty | Kitty placeholders with `RICH_MICRO=kitty` | native | Ghostty speaks Kitty's protocol but is not detected as kitty |
| WezTerm | iTerm2 inline images (detected); Kitty with `RICH_MICRO=kitty` | native (GIF) | |
| iTerm2 | iTerm2 inline images | native (GIF) | |
| Konsole | Kitty with `RICH_MICRO=kitty`, else half-blocks | native with Kitty | |
| foot, mlterm, Windows Terminal ≥ 1.22, mintty, xterm `-ti vt340` | Sixel, once the cell size in pixels is known | frame by frame, on redraw | Sixel is used only when the terminal is found (or said) to support it |
| Alacritty, GNOME Terminal (VTE), Terminal.app, the Linux console with colour | the emoji fallback; half-blocks with `RICH_MICRO=blocks` | frame by frame (blocks) | no image protocol |
| tmux, GNU screen | the emoji fallback unless passthrough is configured | — | set `RICH_MICRO=blocks` or `text`, or allow passthrough and set `RICH_MICRO` to the outer terminal's protocol |
| `TERM=dumb`, no colour | the text fallback, else the alt text | none | |
| pipes, log files, `--export-html`/`--export-svg`, captures | the text fallback, byte for byte | none | never an escape sequence, whatever `RICH_MICRO` says |

"Native" animation is played by the terminal itself; "frame by frame"
means the frame that is due is drawn whenever the view redraws (a live
region, an interactive view), and a printed line keeps the frame it was
printed with. `RICH_A11Y=reduced-motion` and `RICH_ANIMATION=0` always show
the still image.

## Overrides

| Variable | Effect |
|---|---|
| `RICH_MICRO=kitty\|iterm\|sixel\|blocks\|text` | Use this way on a terminal (never off one). `blocks` draws half-blocks from the image before the emoji; `text` never draws images |
| `RICH_CELL_PIXELS=WxH` | The cell size images are fitted to, when the terminal does not report it |
| `RICH_GRAPHICS`, `RICH_SIXEL` | The graphics protocol detection `rich --image` uses, which micro assets share |
| `RICH_ANIMATION=0`, `RICH_A11Y=reduced-motion` | Still images only |

## Checking your terminal

```console
$ rich doctor --report json | jq .micro
$ rich micro preview status/loading
$ RICH_MICRO=kitty rich micro preview status/success
```

If an image protocol draws garbage (a terminal that claims one it does not
have, or a multiplexer in the way), set `RICH_MICRO=blocks` or `text` in
your shell profile. Layout never changes with the choice: every mode puts
exactly the asset's columns in the line.

## What CI sees

The documentation's recordings come from `rich record`, a pseudo-terminal
with no image protocol, so they show the emoji fallback (and half-blocks
where a tape sets `RICH_MICRO=blocks`). The per-protocol PTY tests in
`crates/rich-micro/tests/pty.rs` check the Kitty, iTerm2 and Sixel bytes and
cursor positions through an emulator; the protocols themselves are checked
by hand in real Kitty, iTerm2 and a Sixel terminal before a release.

WebP and frame-sequence animations draw their still image in this release,
and every asset is one row high.
