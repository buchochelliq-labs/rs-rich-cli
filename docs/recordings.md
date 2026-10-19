# Terminal recordings

Every recording on this page is a real terminal session. A script called a
**tape** types into `bash` on a pseudo-terminal, runs the `rich` binary built
from this repository, and waits for the screen before each step. Nothing is
drawn by hand. Press play, pause anywhere, and select the text: the recordings
are [asciinema](https://asciinema.org) casts, not videos.

The screenshots under each recording are taken from the same run. CI re-runs
every tape and fails when any screenshot's text no longer matches, so this page
cannot drift from what the CLI does. [How it works](#how-it-works) is at the
end.

## Gallery

Point at a card, or move to it with Tab, to play its recording. With reduced
motion turned on in your system settings the cards stay still.

<div class="tape-gallery" markdown>

[![A CSV file rendered as a table](media/tapes/hero/table.png){ data-gif="hero.gif" loading=lazy }<span class="tape-card-title">One binary, every format</span>](#one-binary-every-format){ .tape-card }
[![rich --watch after an edit](media/tapes/watch/after.png){ data-gif="watch.gif" loading=lazy }<span class="tape-card-title">Watch a file</span>](#watch-a-file){ .tape-card }
[![Search matches highlighted in less](media/tapes/pager/search.png){ data-gif="pager.gif" loading=lazy }<span class="tape-card-title">Page and search</span>](#page-and-search){ .tape-card }
[![rich input refusing an empty answer](media/tapes/input/required.png){ data-gif="input.gif" loading=lazy }<span class="tape-card-title">Ask for a line</span>](#ask-for-a-line){ .tape-card }
[![A fuzzy file picker with a preview](media/tapes/components/select.png){ data-gif="components.gif" loading=lazy }<span class="tape-card-title">Components</span>](#components){ .tape-card }
[![rich file previewing a file before picking it](media/tapes/file/preview.png){ data-gif="file.gif" loading=lazy }<span class="tape-card-title">Ask for more in a script</span>](#ask-for-more-in-a-script){ .tape-card }
[![rich explore searching a YAML document](media/tapes/explore/search.png){ data-gif="explore.gif" loading=lazy }<span class="tape-card-title">Explore a document</span>](#explore-a-document){ .tape-card }
[![A Markdown file paged in a viewport](media/tapes/viewport/paged.png){ data-gif="viewport.gif" loading=lazy }<span class="tape-card-title">An interactive component</span>](#an-interactive-component){ .tape-card }
[![The command palette over a file list](media/tapes/palette/palette.png){ data-gif="palette.gif" loading=lazy }<span class="tape-card-title">Overlays and chrome</span>](#overlays-and-chrome){ .tape-card }
[![The to-do app with the inspector docked on the right](media/tapes/intuituive/inspector.png){ data-gif="intuituive.gif" loading=lazy }<span class="tape-card-title">Terminal apps</span>](#terminal-apps){ .tape-card }
[![A Yazi-style file manager previewing a Rust file](media/tapes/files/files.png){ data-gif="files.gif" loading=lazy }<span class="tape-card-title">A file manager</span>](#a-file-manager){ .tape-card }
[![scope-tui rebuilt on intuiTUIve: a Lissajous figure in the vectorscope](media/tapes/scope/vectorscope.png){ data-gif="scope.gif" loading=lazy }<span class="tape-card-title">An oscilloscope</span>](#an-oscilloscope){ .tape-card }
[![A status page restyled after its stylesheet changed](media/tapes/stylesheet/reloaded.png){ data-gif="stylesheet.gif" loading=lazy }<span class="tape-card-title">Stylesheets</span>](#stylesheets){ .tape-card }
[![A process table sorted by user, with a cell cursor](media/tapes/tables/cells.png){ data-gif="tables.gif" loading=lazy }<span class="tape-card-title">Sorting and resizing a table</span>](#sorting-and-resizing-a-table){ .tape-card }
[![A card's tooltip on a sprint board](media/tapes/board/tooltip.png){ data-gif="board.gif" loading=lazy }<span class="tape-card-title">Lazy trees, tooltips, drag-and-drop</span>](#lazy-trees-tooltips-and-drag-and-drop){ .tape-card }
[![Five cards wrapped onto two lines of a row](media/tapes/align/wrap.png){ data-gif="align.gif" loading=lazy }<span class="tape-card-title">Flex alignment</span>](#flex-alignment){ .tape-card }
[![An app in linear mode, written as lines of text](media/tapes/access/linear.png){ data-gif="access.gif" loading=lazy }<span class="tape-card-title">Text mode and linear mode</span>](#text-mode-and-linear-mode){ .tape-card }
[![A line of Rust selected in a highlighted text area](media/tapes/editor/selected.png){ data-gif="editor.gif" loading=lazy }<span class="tape-card-title">A text area</span>](#a-text-area){ .tape-card }
[![A dashboard drawn in place below the prompt](media/tapes/surface/done.png){ data-gif="surface.gif" loading=lazy }<span class="tape-card-title">A surface</span>](#a-surface){ .tape-card }
[![ratatui's demo rebuilt on intuiTUIve](media/tapes/demo/graphs.png){ data-gif="demo.gif" loading=lazy }<span class="tape-card-title">ratatui's examples</span>](#ratatuis-examples-on-intuituive){ .tape-card }
[![rich profile on nested JSON Lines](media/tapes/profile-nested/nested.png){ data-gif="profile-nested.gif" loading=lazy }<span class="tape-card-title">Nested JSON Lines</span>](#nested-json-lines-and-style-rules){ .tape-card }
[![A two-row micro icon](media/tapes/micro-rows/icon.png){ data-gif="micro-rows.gif" loading=lazy }<span class="tape-card-title">Two-row micro assets</span>](#two-row-micro-assets){ .tape-card }
[![rich approve reviewing a queue](media/tapes/approve/queue.png){ data-gif="approve.gif" loading=lazy }<span class="tape-card-title">Review and jump</span>](#review-a-queue-and-jump-to-anything){ .tape-card }
[![A FIGlet banner in a gradient](media/tapes/banner/gradient.png){ data-gif="banner.gif" loading=lazy }<span class="tape-card-title">Banners and shell integration</span>](#banners-and-shell-integration){ .tape-card }
[![A redaction preview](media/tapes/share/preview.png){ data-gif="share.gif" loading=lazy }<span class="tape-card-title">Preview, then share</span>](#preview-then-share){ .tape-card }
[![rich date: a day picked on a month grid within a range](media/tapes/date/grid.png){ data-gif="date.gif" loading=lazy }<span class="tape-card-title">Dates and times</span>](#dates-and-times){ .tape-card }
[![A man page rendered by rich, its sections and tagged paragraphs](media/tapes/documents/man.png){ data-gif="documents.gif" loading=lazy }<span class="tape-card-title">Org, AsciiDoc and man pages</span>](#org-asciidoc-and-man-pages){ .tape-card }
[![rich log grouping records under a header for each trace id](media/tapes/log/groups.png){ data-gif="log.gif" loading=lazy }<span class="tape-card-title">Filter and group a log</span>](#filter-and-group-a-log){ .tape-card }
[![rich serve printing its address, and sessions asked for](media/tapes/serve/sessions.png){ data-gif="serve.gif" loading=lazy }<span class="tape-card-title">Serve a program to a browser</span>](#serve-a-program-to-a-browser){ .tape-card }
[![A shell in a terminal pane beside a web view](media/tapes/embed/pane.png){ data-gif="embed.gif" loading=lazy }<span class="tape-card-title">A terminal pane</span>](#a-terminal-pane){ .tape-card }
[![A CSV file drawn as a line chart by rich chart](media/tapes/chart/lines.png){ data-gif="chart.gif" loading=lazy }<span class="tape-card-title">Charts from data</span>](#charts-from-data){ .tape-card }
[![rich deps drawing a dependency graph](media/tapes/deps/graph.png){ data-gif="deps.gif" loading=lazy }<span class="tape-card-title">Dependency trees</span>](#dependency-trees){ .tape-card }
[![rich profile describing each column of a CSV file](media/tapes/profile/profile.png){ data-gif="profile.gif" loading=lazy }<span class="tape-card-title">Profile a data file</span>](#profile-a-data-file){ .tape-card }
[![rich schema --er drawing SQL tables as an ER diagram](media/tapes/schema/er.png){ data-gif="schema.gif" loading=lazy }<span class="tape-card-title">Schemas and ER diagrams</span>](#schemas-and-er-diagrams){ .tape-card }
[![rich deps --features showing what turned syn's features on](media/tapes/deps-features/features.png){ data-gif="deps-features.gif" loading=lazy }<span class="tape-card-title">Crate features</span>](#crate-features){ .tape-card }
[![rich deps --baseline flagging a crate that grew](media/tapes/deps-policy/baseline.png){ data-gif="deps-policy.gif" loading=lazy }<span class="tape-card-title">Sizes, baselines and licence policy</span>](#sizes-baselines-and-licence-policy){ .tape-card }
[![rich diff --conflicts showing ours, base and theirs side by side](media/tapes/conflicts/conflicts.png){ data-gif="conflicts.gif" loading=lazy }<span class="tape-card-title">Merge conflicts</span>](#merge-conflicts){ .tape-card }
[![A DOT fence drawn in a Markdown document](media/tapes/dot-fence/drawn.png){ data-gif="dot-fence.gif" loading=lazy }<span class="tape-card-title">DOT in Markdown</span>](#dot-in-markdown){ .tape-card }
[![fun/heart previewed, its frames magnified](media/tapes/micro-modes/modes.png){ data-gif="micro-modes.gif" loading=lazy }<span class="tape-card-title">Micro assets</span>](#micro-assets){ .tape-card }
[![The guided tour, inspecting structured data](media/tapes/tour/inspect.png){ data-gif="tour.gif" loading=lazy }<span class="tape-card-title">The guided tour</span>](demos.md#run-the-suite-in-your-terminal){ .tape-card }
[![rich choose in the 0.0.13 release recording](media/tapes/release-0.0.13/choose.png){ data-gif="release-0.0.13.gif" loading=lazy }<span class="tape-card-title">The 0.0.13 release</span>](releases/0.0.13.md){ .tape-card }

</div>

## One binary, every format

Markdown, CSV and source code, each rendered from a plain `rich FILE`.

<div class="tape-player" data-cast="../media/tapes/hero/hero.cast" data-poster="npt:0:4">
  <img src="../media/tapes/hero/hero.gif" alt="rich rendering a Markdown file, a CSV table and Python source">
</div>

| Markdown | CSV | Code |
|---|---|---|
| ![Markdown rendered by rich](media/tapes/hero/markdown.png) | ![A CSV file as a table](media/tapes/hero/table.png) | ![Python highlighted by rich](media/tapes/hero/code.png) |

[Tape](tapes/hero.tape) · [Cast](media/tapes/hero/hero.cast) · [GIF](media/tapes/hero/hero.gif) · [Page](media/tapes/hero/hero.html)

## Watch a file

`rich --watch` renders a file and renders it again on every save. Here the
tape edits `service.json` from outside the terminal while `rich` watches it.

<div class="tape-player" data-cast="../media/tapes/watch/watch.cast" data-poster="npt:0:3">
  <img src="../media/tapes/watch/watch.gif" alt="rich --watch re-rendering a JSON file after an edit">
</div>

| Before the edit | After the edit |
|---|---|
| ![replicas is 2](media/tapes/watch/before.png) | ![replicas is 5](media/tapes/watch/after.png) |

[Tape](tapes/watch.tape) · [Cast](media/tapes/watch/watch.cast) · [GIF](media/tapes/watch/watch.gif) · [Page](media/tapes/watch/watch.html)

## Page and search

`rich view --pager` numbers and highlights a file, then hands it to `less`:
page down, then `/characters` to search.

<div class="tape-player" data-cast="../media/tapes/pager/pager.cast" data-poster="npt:0:3">
  <img src="../media/tapes/pager/pager.gif" alt="rich view paging a Rust file through less and searching it">
</div>

| Opened | Paged down | Searched |
|---|---|---|
| ![The top of rule.rs](media/tapes/pager/top.png) | ![One page further](media/tapes/pager/scrolled.png) | ![Matches of characters highlighted](media/tapes/pager/search.png) |

[Tape](tapes/pager.tape) · [Cast](media/tapes/pager/pager.cast) · [GIF](media/tapes/pager/pager.gif) · [Page](media/tapes/pager/pager.html)

## Ask for a line

`rich input` reads one line for a script. `--placeholder` shows a hint while
the line is empty, and `--required` refuses an empty answer with an error under
the line. The answer is captured with `$(…)`.

<div class="tape-player" data-cast="../media/tapes/input/input.cast" data-poster="npt:0:4">
  <img src="../media/tapes/input/input.gif" alt="rich input showing a placeholder, refusing an empty answer, then taking one">
</div>

| Placeholder | Required | Answered |
|---|---|---|
| ![The prompt with its placeholder](media/tapes/input/placeholder.png) | ![An answer is required](media/tapes/input/required.png) | ![The answer echoed by the script](media/tapes/input/answer.png) |

[Tape](tapes/input.tape) · [Cast](media/tapes/input/input.cast) · [GIF](media/tapes/input/input.gif) · [Page](media/tapes/input/input.html)

## Components

Every `rs-rich-interact` component, one after another, recorded in a small
fixture project:

- a fuzzy file picker with a highlighted preview;
- a multi-select;
- an input with suggestions and validation;
- a confirmation sheet with a diff and four choices;
- a form that reports an error under its field;
- a pager that searches.

See [Interactive components](guide/interact/index.md#ready-made-components).

<div class="tape-player" data-cast="../media/tapes/components/components.cast" data-poster="npt:0:5">
  <img src="../media/tapes/components/components.gif" alt="rs-rich-interact's components, one after another">
</div>

| Select | Input | Confirm |
|---|---|---|
| ![A fuzzy pick with a preview](media/tapes/components/select.png) | ![Suggestions as you type](media/tapes/components/input-suggestions.png) | ![A confirmation sheet](media/tapes/components/confirm.png) |
| **Form** | **MultiSelect** | **Pager** |
| ![A form with an error](media/tapes/components/form-error.png) | ![Marking several](media/tapes/components/multi.png) | ![Searching](media/tapes/components/pager.png) |

[Tape](tapes/components.tape) · [Cast](media/tapes/components/components.cast) · [GIF](media/tapes/components/components.gif) · [Page](media/tapes/components/components.html)

## Ask for more in a script

`rich write`, `rich file`, `rich color` and `rich asset`, each captured into
a shell variable with `$(…)`: several lines of a commit message, a path
picked from the fixture project with its preview, a colour from the
names and the palette, and an emoji and a box style used together. See
[Ask in a script](cli.md#ask-in-a-script).

<div class="tape-player" data-cast="../media/tapes/file/file.cast" data-poster="npt:0:3">
  <img src="../media/tapes/file/file.gif" alt="rich file browsing a project and picking a file">
</div>

| rich write | rich file | rich file |
|---|---|---|
| ![Several lines typed](media/tapes/write/editing.png) | ![A directory listed](media/tapes/file/browse.png) | ![A file previewed](media/tapes/file/preview.png) |
| **rich color** | **rich color** | **rich asset** |
| ![Named colours with a swatch](media/tapes/color/names.png) | ![The 256 palette](media/tapes/color/palette.png) | ![Box styles previewed](media/tapes/asset/box.png) |

[write tape](tapes/write.tape) · [cast](media/tapes/write/write.cast) · [page](media/tapes/write/write.html) ·
[file tape](tapes/file.tape) · [cast](media/tapes/file/file.cast) · [page](media/tapes/file/file.html) ·
[color tape](tapes/color.tape) · [cast](media/tapes/color/color.cast) · [page](media/tapes/color/color.html) ·
[asset tape](tapes/asset.tape) · [cast](media/tapes/asset/asset.cast) · [page](media/tapes/asset/asset.html)

## Explore a document

`rich explore` on a YAML file: a container opened, the breadcrumbs following
the cursor, the path copied, a search that keeps the match's ancestors, and
the JSONPath captured by the script. See
[Explore it interactively](cli.md#explore-it-interactively).

<div class="tape-player" data-cast="../media/tapes/explore/explore.cast" data-poster="npt:0:4">
  <img src="../media/tapes/explore/explore.gif" alt="rich explore folding, searching and picking a node of a YAML file">
</div>

| Folded | Opened | Searched |
|---|---|---|
| ![The document folded below the root](media/tapes/explore/tree.png) | ![A container opened, its node previewed](media/tapes/explore/expanded.png) | ![A search keeping its ancestors](media/tapes/explore/search.png) |

[Tape](tapes/explore.tape) · [Cast](media/tapes/explore/explore.cast) · [GIF](media/tapes/explore/explore.gif) · [Page](media/tapes/explore/explore.html)

## An interactive component

`rs-rich-interact`'s `Viewport`, run as its example pager, pages a Markdown
file on the alternate screen. Each key repaints only the cells that changed.
Enter gives the terminal back, as it was, with the line it was left at. See
[Interactive components](guide/interact/index.md).

<div class="tape-player" data-cast="../media/tapes/viewport/viewport.cast" data-poster="npt:0:3">
  <img src="../media/tapes/viewport/viewport.gif" alt="A Markdown file paged in rich_interact's Viewport">
</div>

| Opened | Paged down | Given back |
|---|---|---|
| ![The top of the README](media/tapes/viewport/top.png) | ![One page further](media/tapes/viewport/paged.png) | ![The shell again, with the line it was left at](media/tapes/viewport/returned.png) |

[Tape](tapes/viewport.tape) · [Cast](media/tapes/viewport/viewport.cast) · [GIF](media/tapes/viewport/viewport.gif) · [Page](media/tapes/viewport/viewport.html)

## Overlays and chrome

`rs-rich-interact`'s overlays and chrome, run as its `overlays` example: a
file list wrapped in `Overlays`. Ctrl+O opens the command palette, which
lists every key the list has, by category, with its shortcut, and runs the
one picked. The help overlay groups the keys and searches them as you type.
Under the list, a status bar shows a badge, a spinner, a note and key hints
from the keymap. Breadcrumbs sit over the list, and Ctrl+K opens the
region's actions in a modal. See
[Overlays and chrome](guide/interact/overlays.md).

<div class="tape-player" data-cast="../media/tapes/palette/palette.cast" data-poster="npt:0:3">
  <img src="../media/tapes/palette/palette.gif" alt="The command palette over a file list, searched and run">
</div>

| The palette | Searched | Run |
|---|---|---|
| ![Every key of the list, by category](media/tapes/palette/palette.png) | !["move down" found](media/tapes/palette/search.png) | ![The list moved down](media/tapes/palette/ran.png) |

[Tape](tapes/palette.tape) · [Cast](media/tapes/palette/palette.cast) · [GIF](media/tapes/palette/palette.gif) · [Page](media/tapes/palette/palette.html)

| Help | Searched |
|---|---|
| ![Every key, grouped by context](media/tapes/help/help.png) | ![The keys for "page"](media/tapes/help/search.png) |

[Tape](tapes/help.tape) · [Cast](media/tapes/help/help.cast) · [GIF](media/tapes/help/help.gif) · [Page](media/tapes/help/help.html)

| The status bar | Region actions | A command ran |
|---|---|---|
| ![A badge, a spinner and key hints under the list](media/tapes/statusbar/status.png) | ![The region's actions in a modal](media/tapes/statusbar/actions.png) | ![The note changed by the command](media/tapes/statusbar/refreshed.png) |

[Tape](tapes/statusbar.tape) · [Cast](media/tapes/statusbar/statusbar.cast) · [GIF](media/tapes/statusbar/statusbar.gif) · [Page](media/tapes/statusbar/statusbar.html)

## Terminal apps

`rs-rich-intuituive`'s examples: the to-do app from the
[tutorial](guide/intuituive/tutorial.md), with a to-do added and one ticked
off; the same app with the inspector docked on the right, showing which
nodes drew; the `screens` example asking before it quits, in a modal; and
the `inline` example finishing in a few rows under the prompt, with the
scrollback above it kept. Then two examples of the newer parts: `meters`, a
board of meters that are widgets of their own (retained drawing, focus and
resize events, the pointer, a board that sees keys first), and `planner`,
built from the framework's components (a menu bar, a tree, split panes, a
calendar and a virtual list of a hundred thousand rows), with a toast and
then the command palette. See [Terminal apps](guide/intuituive/index.md).

<div class="tape-player" data-cast="../media/tapes/intuituive/intuituive.cast" data-poster="npt:0:3">
  <img src="../media/tapes/intuituive/intuituive.gif" alt="intuiTUIve's to-do, screens, inline, meters and planner examples">
</div>

| To-do | Inspector |
|---|---|
| ![A to-do added and one ticked off](media/tapes/intuituive/todo.png) | ![The inspector docked on the right](media/tapes/intuituive/inspector.png) |
| **Modal** | **Inline** |
| ![A modal asking whether to quit](media/tapes/intuituive/modal.png) | ![An inline app under the prompt](media/tapes/intuituive/inline.png) |
| **Widgets of your own** | **Components** |
| ![Four meters, each a custom widget](media/tapes/intuituive/meters.png) | ![A planner: a menu bar, a tree, a calendar and a log](media/tapes/intuituive/planner.png) |
| **Command palette** | **Menu bar** |
| ![The planner's command palette](media/tapes/intuituive/planner-palette.png) | ![The planner's File menu, opened with F10](media/tapes/intuituive/planner-menu.png) |

[Tape](tapes/intuituive.tape) · [Cast](media/tapes/intuituive/intuituive.cast) · [GIF](media/tapes/intuituive/intuituive.gif) · [Page](media/tapes/intuituive/intuituive.html)

## A game, played with held keys

`rs-rich-intuituive`'s ray caster, recorded at 35 frames a second, every
frame it draws: the title, the map, the keys and the pause menu as stills,
then a walk-through played with `Hold`, `KeyDown` and `KeyUp`, with the
kitty keyboard protocol reported (`Set KittyKeyboard on`) so the arrows'
releases reach the game. A second tape records the same walk-through with
the example drawing iTerm2 images, which the recorder decodes and draws
into the GIF. See [the recorder's README](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/crates/rich-record/README.md)
for the tape commands, `Set Framerate` and the graphics it follows.

![The raycaster's walk-through](media/tapes/raycast/raycast.gif)

| Title | Map |
|---|---|
| ![The title over the first level](media/tapes/raycast/title.png) | ![The map](media/tapes/raycast/map.png) |

[Tape](tapes/raycast.tape) · [GIF](media/tapes/raycast/raycast.gif) · [APNG](media/tapes/raycast/raycast.apng) · [In iTerm2 images](media/tapes/raycast-iterm/raycast-iterm.gif) ([tape](tapes/raycast-iterm.tape))

## A file manager

`rs-rich-intuituive`'s `files` example, a rebuild of the behaviour of
[Yazi](https://github.com/sxyazi/yazi) by sxyazi and contributors (MIT), in
the fixture project: Yazi's three columns, theme and status line, a Rust
file previewed with highlighting done on a worker thread, the which-key
pop-up after `,` and the sort by size it picks, the help, a selection, the
filter applied as it is typed, the confirmation before trashing, the
right-click menu, the command palette and a second tab. See
[Porting a ratatui app](guide/intuituive/porting.md#worked-example-a-yazi-style-file-manager).

<div class="tape-player" data-cast="../media/tapes/files/files.cast" data-poster="npt:0:3">
  <img src="../media/tapes/files/files.gif" alt="A Yazi-style file manager browsing the fixture project">
</div>

| Browsing | Which-key |
|---|---|
| ![A Rust file previewed](media/tapes/files/files.png) | ![What can follow `,`](media/tapes/files/files-which.png) |
| **Help** | **Selection** |
| ![The help](media/tapes/files/files-help.png) | ![Two directories selected](media/tapes/files/files-select.png) |
| **Filter** | **Confirmation** |
| ![The filter as it is typed](media/tapes/files/files-filter.png) | ![Trash, asked first](media/tapes/files/files-confirm.png) |
| **Tabs** | **Right-click menu** |
| ![Two tabs](media/tapes/files/files-tabs.png) | ![The menu a right click opens on a row](media/tapes/files/files-menu.png) |
| **Command palette** | |
| ![The command palette](media/tapes/files/files-palette.png) | |

[Tape](tapes/files.tape) · [Cast](media/tapes/files/files.cast) · [GIF](media/tapes/files/files.gif) · [Page](media/tapes/files/files.html)

## An oscilloscope

`rs-rich-intuituive`'s `scope` example, a rebuild of
[scope-tui](https://github.com/alemidev/scope-tui) by alemi (MIT). It shows its test
signal moving in the oscilloscope, the vectorscope and the spectroscope,
then held still and paused in each for the screenshots, and finally its
keys. See
[Porting a ratatui app](guide/intuituive/porting.md#worked-example-scope-tui-an-oscilloscope).

<div class="tape-player" data-cast="../media/tapes/scope/scope.cast" data-poster="npt:0:3">
  <img src="../media/tapes/scope/scope.gif" alt="scope-tui rebuilt on intuiTUIve, cycling through its three scopes">
</div>

| Oscilloscope | Vectorscope |
|---|---|
| ![Two channels against time](media/tapes/scope/oscilloscope.png) | ![Left against right](media/tapes/scope/vectorscope.png) |
| **Spectroscope** | **Keys** |
| ![Each channel's spectrum](media/tapes/scope/spectroscope.png) | ![The keys, over the spectroscope](media/tapes/scope/keys.png) |

[Tape](tapes/scope.tape) · [Cast](media/tapes/scope/scope.cast) · [GIF](media/tapes/scope/scope.gif) · [Page](media/tapes/scope/scope.html)

## Stylesheets

`rs-rich-intuituive`'s `styled` example (0.0.18): a status page whose
colours, borders, sizes and docking come from a stylesheet, `styled.tcss`.
Rules select by kind (`table`), name (`#title`) and class (`.card`, and
`.down` while a service is down), with `:focus` and `:hover`. The tape then
edits the sheet from outside the terminal: the app reads it again, and a
line it cannot parse is reported in a toast while the last good sheet
stays. See [Stylesheets](guide/intuituive/panels.md#stylesheets).

<div class="tape-player" data-cast="../media/tapes/stylesheet/stylesheet.cast" data-poster="npt:0:3">
  <img src="../media/tapes/stylesheet/stylesheet.gif" alt="A status page styled by a stylesheet, restyled when the sheet changes">
</div>

| Styled | `:focus` and `:hover` |
|---|---|
| ![The status page as the sheet styles it](media/tapes/stylesheet/styled.png) | ![The down service focused, another hovered](media/tapes/stylesheet/states.png) |
| **Reloaded** | **A mistake** |
| ![Heavy borders and a magenta title after the edit](media/tapes/stylesheet/reloaded.png) | ![A toast naming the line and column](media/tapes/stylesheet/error.png) |

[Tape](tapes/stylesheet.tape) · [Cast](media/tapes/stylesheet/stylesheet.cast) · [GIF](media/tapes/stylesheet/stylesheet.gif) · [Page](media/tapes/stylesheet/stylesheet.html)

## Sorting and resizing a table

`rs-rich-intuituive`'s `tables` example (0.0.18), a process table built
with `table_with`: a click on a header sorts by it and a second click
reverses the order, ←/→ move the cell cursor and `s` sorts by its column,
and dragging the gap after a header resizes the column. See
[Sorting, resizing and a cell cursor](guide/intuituive/tables.md#sorting-resizing-and-a-cell-cursor).

<div class="tape-player" data-cast="../media/tapes/tables/tables.cast" data-poster="npt:0:3">
  <img src="../media/tapes/tables/tables.gif" alt="A process table sorted by clicks on its headers, then resized">
</div>

| Sorted by a click | The cell cursor, sorted with `s` | Resized |
|---|---|---|
| ![Sorted by memory, largest first](media/tapes/tables/sorted.png) | ![Sorted by user, the cell cursor on a user](media/tapes/tables/cells.png) | ![The command column dragged wider](media/tapes/tables/resized.png) |

[Tape](tapes/tables.tape) · [Cast](media/tapes/tables/tables.cast) · [GIF](media/tapes/tables/tables.gif) · [Page](media/tapes/tables/tables.html)

## Lazy trees, tooltips and drag-and-drop

`rs-rich-intuituive`'s `board` example (0.0.18), a sprint board. Past
sprints sit in a `tree_lazy` that loads each one on another thread when it
is opened, showing "loading…" until it arrives and the error in red when it
fails. The pointer resting on a card shows its tooltip, and a card dragged
to another lane moves there. See [The mouse](guide/intuituive/input.md#the-mouse).

<div class="tape-player" data-cast="../media/tapes/board/board.cast" data-poster="npt:0:3">
  <img src="../media/tapes/board/board.gif" alt="A sprint board: a lazy tree loading, a tooltip, and a card dragged to Done">
</div>

| Loading | Loaded, and a failure |
|---|---|
| ![Sprint 41 loading](media/tapes/board/loading.png) | ![Sprint 41's tickets, and Sprint 39's error](media/tapes/board/lazy.png) |
| **A tooltip** | **Dropped** |
| ![A card's tooltip](media/tapes/board/tooltip.png) | ![The card moved to Done](media/tapes/board/dropped.png) |

[Tape](tapes/board.tape) · [Cast](media/tapes/board/board.cast) · [GIF](media/tapes/board/board.gif) · [Page](media/tapes/board/board.html)

## Flex alignment

`rs-rich-intuituive`'s `align` example (0.0.19): a row of cards of
different sizes placed by `align_items`, `justify`, `wrap` and
`align_content`, each cycled with a key; the test card keeps to the end of
its line with `align_self`. See
[Alignment and wrapping](guide/intuituive/layout.md#alignment-and-wrapping).

<div class="tape-player" data-cast="../media/tapes/align/align.cast" data-poster="npt:0:2">
  <img src="../media/tapes/align/align.gif" alt="Cards in a row, placed by align-items, justify, wrap and align-content">
</div>

| Stretched | Centred |
|---|---|
| ![Cards stretched to the row's height](media/tapes/align/stretch.png) | ![Cards centred across the row](media/tapes/align/center.png) |
| **Space between** | **Wrapped** |
| ![Cards spread to the row's ends](media/tapes/align/between.png) | ![Five cards on two lines](media/tapes/align/wrap.png) |
| **Lines centred** | |
| ![The two lines centred in the row](media/tapes/align/lines.png) | |

[Tape](tapes/align.tape) · [Cast](media/tapes/align/align.cast) · [GIF](media/tapes/align/align.gif) · [Page](media/tapes/align/align.html)

## Text mode and linear mode

`rs-rich-intuituive`'s `access` example (0.0.18), the one the
[screen reader checklist](guide/intuituive/screen-readers.md) walks
through. In text mode (`--accessible`) boxes are blanks with their titles
kept, there is no colour, `>` marks the selected item, and the terminal's
cursor sits on the focus. In linear mode (`--linear`) there is no screen:
the app is written as lines of text, then only the lines that change. See
[Accessibility](guide/intuituive/accessibility.md#accessibility).

<div class="tape-player" data-cast="../media/tapes/access/access.cast" data-poster="npt:0:3">
  <img src="../media/tapes/access/access.gif" alt="The access example in text mode, then in linear mode">
</div>

| Text mode | Linear mode |
|---|---|
| ![Blank boxes, no colour, > on the selected items](media/tapes/access/text-mode.png) | ![The app as lines of text, the focus moving as →](media/tapes/access/linear.png) |

[Tape](tapes/access.tape) · [Cast](media/tapes/access/access.cast) · [GIF](media/tapes/access/access.gif) · [Page](media/tapes/access/access.html)

## A text area

`rs-rich-intuituive`'s `editor` example (0.0.19): Rust source highlighted
with line numbers, over a notes area that wraps and shows a placeholder.
A line is typed, a line selected with Shift, the typing undone as one step
with Ctrl+Z, and Escape then Tab moves on to the notes. See
[Text areas](guide/intuituive/components.md#text-areas).

<div class="tape-player" data-cast="../media/tapes/editor/editor.cast" data-poster="npt:0:3">
  <img src="../media/tapes/editor/editor.gif" alt="Rust source in a text area: a line typed, a line selected, the typing undone, then notes typed below">
</div>

| Highlighted | A line selected |
|---|---|
| ![Rust source, highlighted, with line numbers](media/tapes/editor/editor.png) | ![A line selected with Shift+Home](media/tapes/editor/selected.png) |
| **Undone** | **Notes** |
| ![The typed line undone with Ctrl+Z](media/tapes/editor/undone.png) | ![Notes typed in the second area](media/tapes/editor/notes.png) |

[Tape](tapes/editor.tape) · [Cast](media/tapes/editor/editor.cast) · [GIF](media/tapes/editor/editor.gif) · [Page](media/tapes/editor/editor.html)

## A surface

`rs-rich-ext`'s `guide_surface` example (0.0.19): a panel, a table and a
status line drawn into rectangles of an inline `Surface`, eleven rows below
the prompt, and redrawn every half second. Each frame sends only the cells
that changed; the output above it stays, and the last frame stays on
screen when the program ends. See
[Positioned drawing](guide/ext/surface.md).

<div class="tape-player" data-cast="../media/tapes/surface/surface.cast" data-poster="npt:0:2">
  <img src="../media/tapes/surface/surface.gif" alt="A dashboard below the prompt, its rollout time ticking up in place">
</div>

| Ticking | Done |
|---|---|
| ![The dashboard at its second frame](media/tapes/surface/ticking.png) | ![The last frame left on screen, the prompt below it](media/tapes/surface/done.png) |

[Tape](tapes/surface.tape) · [Cast](media/tapes/surface/surface.cast) · [GIF](media/tapes/surface/surface.gif) · [Page](media/tapes/surface/surface.html)

## Dates and times

`rich date` (0.0.19): a day picked on a month grid, inside a range whose
days outside are out of reach, then a date and time whose time is refused
before the range's first moment, until it is moved into it. The answers
are printed in ISO 8601 form, for a script to read. See
[Ask in a script](cli.md#ask-in-a-script).

<div class="tape-player" data-cast="../media/tapes/date/date.cast" data-poster="npt:0:2">
  <img src="../media/tapes/date/date.gif" alt="rich date picking a day, then a date and time">
</div>

| A day | Refused | Answered |
|---|---|---|
| ![A month grid with the twentieth of November picked](media/tapes/date/grid.png) | ![A time before the range refused](media/tapes/date/refused.png) | ![The date and time printed](media/tapes/date/answer.png) |

[Tape](tapes/date.tape) · [Cast](media/tapes/date/date.cast) · [GIF](media/tapes/date/date.gif) · [Page](media/tapes/date/date.html)

## Org, AsciiDoc and man pages

`rich FILE` (0.0.19) on an Org file, an AsciiDoc file and a man page, each
found by its extension (the man page by its section number and the roff
in it) and drawn in the look of rich's Markdown; then the man page's
source with `--syntax`. See
[Org, AsciiDoc and man pages](cli.md#org-asciidoc-and-man-pages).

<div class="tape-player" data-cast="../media/tapes/documents/documents.cast" data-poster="npt:0:3">
  <img src="../media/tapes/documents/documents.gif" alt="rich rendering an Org file, an AsciiDoc file and a man page, then the man page's source">
</div>

| Org | AsciiDoc | Man page | Source |
|---|---|---|---|
| ![An Org file with TODO keywords, tags and a table](media/tapes/documents/org.png) | ![An AsciiDoc file with its header, admonitions and a listing](media/tapes/documents/asciidoc.png) | ![A man page with its header, sections and tagged paragraphs](media/tapes/documents/man.png) | ![The man page's roff highlighted](media/tapes/documents/source.png) |

[Tape](tapes/documents.tape) · [Cast](media/tapes/documents/documents.cast) · [GIF](media/tapes/documents/documents.gif) · [Page](media/tapes/documents/documents.html)

## Filter and group a log

`rich log` (0.0.19) on one log in four formats (JSON Lines, logfmt, a
timestamped line and an access log, with a stack trace after an error),
read as records; then filtered by level and field, folded where a message
repeats, and grouped by trace id with a summary of the levels and the
traces that failed. See [Logs: `rich log`](cli.md#logs-rich-log).

<div class="tape-player" data-cast="../media/tapes/log/log.cast" data-poster="npt:0:3">
  <img src="../media/tapes/log/log.gif" alt="rich log reading a mixed log, filtering it, folding repeats and grouping by trace">
</div>

| Records | Filtered | Folded | Groups |
|---|---|---|---|
| ![Four formats read as records](media/tapes/log/records.png) | ![Warnings and above from one service](media/tapes/log/filtered.png) | ![Repeated messages folded with a count](media/tapes/log/folded.png) | ![Records grouped under a header for each trace id](media/tapes/log/groups.png) |

[Tape](tapes/log.tape) · [Cast](media/tapes/log/log.cast) · [GIF](media/tapes/log/log.gif) · [Page](media/tapes/log/log.html)

## ratatui's examples on intuiTUIve

ratatui's `demo` and `demo2`, its widget, chart and layout examples, and
seven of its apps, rebuilt on intuiTUIve's API (0.0.19) with the mouse,
help and a command palette added. The design, layout and keys are
ratatui's (MIT, Florian Dehau and the Ratatui Developers); the code and
the sample data are ours. See
[ratatui's examples](guide/intuituive/ratatui-examples.md).

### The classic demo

<div class="tape-player" data-cast="../media/tapes/demo/demo.cast" data-poster="npt:0:3">
  <img src="../media/tapes/demo/demo.gif" alt="ratatui's demo on intuiTUIve: gauges, a sparkline, lists, bars, a chart, a world map">
</div>

| Graphs | World | Colours |
|---|---|---|
| ![Gauges, a sparkline, lists, bars and a moving chart](media/tapes/demo/graphs.png) | ![A server table beside a world map](media/tapes/demo/world.png) | ![The sixteen named colours](media/tapes/demo/colours.png) |

[Tape](tapes/demo.tape) · [Cast](media/tapes/demo/demo.cast) · [GIF](media/tapes/demo/demo.gif) · [Page](media/tapes/demo/demo.html)

### The showcase

<div class="tape-player" data-cast="../media/tapes/demo2/demo2.cast" data-poster="npt:0:3">
  <img src="../media/tapes/demo2/demo2.gif" alt="ratatui's demo2 on intuiTUIve: about, recipe, email, traceroute and weather tabs">
</div>

| About | Email | Weather |
|---|---|---|
| ![A mark over a colour swatch](media/tapes/demo2/about.png) | ![An inbox and an open email](media/tapes/demo2/email.png) | ![A calendar, bars and a gauge](media/tapes/demo2/weather.png) |

[Tape](tapes/demo2.tape) · [Cast](media/tapes/demo2/demo2.cast) · [GIF](media/tapes/demo2/demo2.gif) · [Page](media/tapes/demo2/demo2.html)

### Widgets, charts and layout

<div class="tape-player" data-cast="../media/tapes/widgets/widgets.cast" data-poster="npt:0:3">
  <img src="../media/tapes/widgets/widgets.gif" alt="A gallery of intuiTUIve widgets in the style of ratatui's widget examples">
</div>

| Blocks | Table | Popup |
|---|---|---|
| ![Panels with titles, borders and a shadow](media/tapes/widgets/block.png) | ![A table with a footer and highlights](media/tapes/widgets/table.png) | ![A modal with a shadow](media/tapes/widgets/popup.png) |

[Tape](tapes/widgets.tape) · [Cast](media/tapes/widgets/widgets.cast) · [GIF](media/tapes/widgets/widgets.gif) · [Page](media/tapes/widgets/widgets.html)

<div class="tape-player" data-cast="../media/tapes/charts/charts.cast" data-poster="npt:0:3">
  <img src="../media/tapes/charts/charts.gif" alt="A gallery of gauges, sparklines, bars, charts and the canvas">
</div>

| Gauges | Chart | Canvas |
|---|---|---|
| ![Fill and line gauges](media/tapes/charts/gauge.png) | ![Line, area and scatter charts](media/tapes/charts/chart.png) | ![The world map on the canvas](media/tapes/charts/canvas.png) |

[Tape](tapes/charts.tape) · [Cast](media/tapes/charts/charts.cast) · [GIF](media/tapes/charts/charts.gif) · [Page](media/tapes/charts/charts.html)

<div class="tape-player" data-cast="../media/tapes/layout/layout.cast" data-poster="npt:0:3">
  <img src="../media/tapes/layout/layout.gif" alt="A layout explorer: sizes, flex modes and spacing">
</div>

| Explorer | Flex |
|---|---|
| ![Blocks sized and justified, with their widths](media/tapes/layout/explorer.png) | ![Each flex mode with and without a gap](media/tapes/layout/flex.png) |

[Tape](tapes/layout.tape) · [Cast](media/tapes/layout/layout.cast) · [GIF](media/tapes/layout/layout.gif) · [Page](media/tapes/layout/layout.html)

### Apps

<div class="tape-player" data-cast="../media/tapes/apps/apps.cast" data-poster="npt:0:3">
  <img src="../media/tapes/apps/apps.gif" alt="Four of ratatui's apps on intuiTUIve: colours, the canvas, a form and a calendar">
</div>

| Colours | Canvas | Form | Calendar |
|---|---|---|---|
| ![Named colours on backgrounds](media/tapes/apps/colors-named.png) | ![The canvas app](media/tapes/apps/canvas.png) | ![A form with a field in error](media/tapes/apps/form-error.png) | ![A year of months](media/tapes/apps/calendar.png) |

[Tape](tapes/apps.tape) · [Cast](media/tapes/apps/apps.cast) · [GIF](media/tapes/apps/apps.gif) · [Page](media/tapes/apps/apps.html)

## Nested JSON Lines and style rules

`rich profile` (0.0.19) on JSON Lines, path by path with each path's mix
of types, then a table styled by value with `--style-rule`. See
[Nested JSON Lines](cli.md#nested-json-lines-0019).

<div class="tape-player" data-cast="../media/tapes/profile-nested/profile-nested.cast" data-poster="npt:0:3">
  <img src="../media/tapes/profile-nested/profile-nested.gif" alt="rich profile profiling nested JSON Lines, then style rules on a table">
</div>

| Nested | Rules |
|---|---|
| ![A profile of nested paths and their types](media/tapes/profile-nested/nested.png) | ![A table styled by value](media/tapes/profile-nested/rules.png) |

[Tape](tapes/profile-nested.tape) · [Cast](media/tapes/profile-nested/profile-nested.cast) · [GIF](media/tapes/profile-nested/profile-nested.gif) · [Page](media/tapes/profile-nested/profile-nested.html)

## Two-row micro assets

Micro assets two rows high (0.0.19): a 2×2 icon in text and a table, and
a 3×2 animation. See [Micro assets](guide/micro/index.md).

<div class="tape-player" data-cast="../media/tapes/micro-rows/micro-rows.cast" data-poster="npt:0:3">
  <img src="../media/tapes/micro-rows/micro-rows.gif" alt="A two-row micro icon in text and a table, and a two-row animation">
</div>

| Icon | Animation |
|---|---|
| ![A 2×2 icon with its row reserved](media/tapes/micro-rows/icon.png) | ![A 3×2 animation](media/tapes/micro-rows/animation.png) |

[Tape](tapes/micro-rows.tape) · [Cast](media/tapes/micro-rows/micro-rows.cast) · [GIF](media/tapes/micro-rows/micro-rows.gif) · [Page](media/tapes/micro-rows/micro-rows.html)

## Review a queue and jump to anything

`rich approve` (0.0.19) walks a queue of items, approved, rejected with a
reason or skipped, and prints the decisions as JSON Lines; `rich jump`
finds a file, a heading, a symbol or a command from one fuzzy box. See
[Review and jump](cli.md#review-and-jump).

<div class="tape-player" data-cast="../media/tapes/approve/approve.cast" data-poster="npt:0:3">
  <img src="../media/tapes/approve/approve.gif" alt="rich approve reviewing a queue of items">
</div>

| Queue | Reason | Decisions |
|---|---|---|
| ![A queue of items to review](media/tapes/approve/queue.png) | ![A rejection with a reason](media/tapes/approve/reason.png) | ![The decisions as JSON Lines](media/tapes/approve/output.png) |

[Tape](tapes/approve.tape) · [Cast](media/tapes/approve/approve.cast) · [GIF](media/tapes/approve/approve.gif) · [Page](media/tapes/approve/approve.html)

<div class="tape-player" data-cast="../media/tapes/jump/jump.cast" data-poster="npt:0:3">
  <img src="../media/tapes/jump/jump.gif" alt="rich jump searching files, headings, symbols and commands">
</div>

| Groups | Narrowed | Answer |
|---|---|---|
| ![Results grouped by source](media/tapes/jump/groups.png) | ![A search narrowed to one source](media/tapes/jump/narrowed.png) | ![The picked location printed](media/tapes/jump/answer.png) |

[Tape](tapes/jump.tape) · [Cast](media/tapes/jump/jump.cast) · [GIF](media/tapes/jump/jump.gif) · [Page](media/tapes/jump/jump.html)

## Banners and shell integration

`rich banner` (0.0.19) draws FIGlet text in a gradient, and `rich shell`
prints a status line after a command and links to files. See
[Draw a banner](cli.md#draw-a-banner-0019) and
[Shell integration](cli.md#shell-integration-0019).

<div class="tape-player" data-cast="../media/tapes/banner/banner.cast" data-poster="npt:0:3">
  <img src="../media/tapes/banner/banner.gif" alt="rich banner drawing FIGlet text in gradients">
</div>

| Gradient | Vertical | Fit and status |
|---|---|---|
| ![A banner in a horizontal gradient](media/tapes/banner/gradient.png) | ![A banner in a vertical gradient](media/tapes/banner/vertical.png) | ![A banner fitted to the width, and a status line](media/tapes/banner/fit-and-status.png) |

[Tape](tapes/banner.tape) · [Cast](media/tapes/banner/banner.cast) · [GIF](media/tapes/banner/banner.gif) · [Page](media/tapes/banner/banner.html)

## Preview, then share

`rich capture --redact --redact-preview` (0.0.19) shows each secret a
redaction would mask, and its replacement, without writing anything; then
the redacted capture is shared as a PNG. See
[Preview a redaction before you share](cli.md#preview-a-redaction-before-you-share-0019).

<div class="tape-player" data-cast="../media/tapes/share/share.cast" data-poster="npt:0:3">
  <img src="../media/tapes/share/share.gif" alt="rich capture previewing a redaction, then sharing the output">
</div>

| Preview | Shared |
|---|---|
| ![Each match, its rule and its replacement](media/tapes/share/preview.png) | ![The redacted output, shared](media/tapes/share/shared.png) |

[Tape](tapes/share.tape) · [Cast](media/tapes/share/share.cast) · [GIF](media/tapes/share/share.gif) · [Page](media/tapes/share/share.html)

## Serve a program to a browser

`rich serve` (0.0.18, behind the CLI's `serve` feature) runs a program in
a browser tab, one copy per tab, and prints the address with its token. A
tape cannot drive a browser, so `curl` asks what a browser would: the page
without the token is refused and with it is served, and a WebSocket
upgrade from the server's own page starts a session while one from another
site is refused. Ctrl+C stops the server and the programs. See
[`rich serve`](guide/intuituive/web.md#from-the-command-line-rich-serve).

<div class="tape-player" data-cast="../media/tapes/serve/serve.cast" data-poster="npt:0:3">
  <img src="../media/tapes/serve/serve.gif" alt="rich serve printing its address, then curl asking for the page and a session">
</div>

| Started | The token and the origin checked |
|---|---|
| ![rich serve printing the address with its token](media/tapes/serve/started.png) | ![A session started, another site refused](media/tapes/serve/sessions.png) |

[Tape](tapes/serve.tape) · [Cast](media/tapes/serve/serve.cast) · [GIF](media/tapes/serve/serve.gif) · [Page](media/tapes/serve/serve.html)

## A terminal pane

`rs-rich-embed`'s `embed` example (0.0.18): a shell in a terminal pane
beside a web view, with a status line built from both panes' signals. Keys
go to the program in the focused pane; F6 moves to the web view, where w3m
shows a local page and Ctrl+L opens another. See
[Embedding programs and pages](guide/intuituive/embed.md).

<div class="tape-player" data-cast="../media/tapes/embed/embed.cast" data-poster="npt:0:4">
  <img src="../media/tapes/embed/embed.gif" alt="A shell in a terminal pane running rich, beside w3m in a web view">
</div>

| The shell | Another address |
|---|---|
| ![rich run in the shell pane](media/tapes/embed/pane.png) | ![The web view on a second page](media/tapes/embed/followed.png) |

[Tape](tapes/embed.tape) · [Cast](media/tapes/embed/embed.cast) · [GIF](media/tapes/embed/embed.gif) · [Page](media/tapes/embed/embed.html)

## Charts from data

`rich chart` draws CSV, JSON or numbers piped in: bars labelled by a
column, every numeric column as a line, a sparkline from `printf`, a
heatmap, and a column that is not there refused with the ones that are. See
[the charts guide](guide/ext/charts.md#from-the-shell-rich-chart).

<div class="tape-player" data-cast="../media/tapes/chart/chart.cast" data-poster="npt:0:4">
  <img src="../media/tapes/chart/chart.gif" alt="rich chart drawing a CSV file as bars, lines, a sparkline and a heatmap">
</div>

| Bars | Lines | Piped in, and a mistake |
|---|---|---|
| ![rich chart --kind bar](media/tapes/chart/bars.png) | ![rich chart with every numeric column as a line](media/tapes/chart/lines.png) | ![A sparkline from printf, a heatmap, and a missing column](media/tapes/chart/pipe.png) |

[Tape](tapes/chart.tape) · [Cast](media/tapes/chart/chart.cast) · [GIF](media/tapes/chart/chart.gif) · [Page](media/tapes/chart/chart.html)

## Dependency trees

`rich deps` draws a Cargo workspace's dependencies from `cargo metadata`
(here a saved copy, so the recording never changes): a tree that marks
crates resolved at several versions, `--why` for what pulls a crate in, and
`--graph` for the same through the diagram layout. See
[Dependencies and schemas](guide/ext/sources.md).

<div class="tape-player" data-cast="../media/tapes/deps/deps.cast" data-poster="npt:0:4">
  <img src="../media/tapes/deps/deps.gif" alt="rich deps showing a tree, the paths to syn, and a graph">
</div>

| Tree | Why `syn` | Graph |
|---|---|---|
| ![rich deps --depth 2](media/tapes/deps/tree.png) | ![rich deps --why syn](media/tapes/deps/why.png) | ![rich deps --graph --depth 1](media/tapes/deps/graph.png) |

[Tape](tapes/deps.tape) · [Cast](media/tapes/deps/deps.cast) · [GIF](media/tapes/deps/deps.gif) · [Page](media/tapes/deps/deps.html)

## Crate features

`rich deps --features` shows which features each crate has enabled, what
each one turns on (other features, optional dependencies, features of
dependencies), and which crates asked for them (0.0.16). `--package` picks
one crate; here `syn`, resolved at two versions.

<div class="tape-player" data-cast="../media/tapes/deps-features/deps-features.cast" data-poster="npt:0:3">
  <img src="../media/tapes/deps-features/deps-features.gif" alt="rich deps --features --package syn listing both versions' features and who requested them">
</div>

![rich deps --features --package syn](media/tapes/deps-features/features.png)

[Tape](tapes/deps-features.tape) · [Cast](media/tapes/deps-features/deps-features.cast) · [GIF](media/tapes/deps-features/deps-features.gif) · [Page](media/tapes/deps-features/deps-features.html)

## Sizes, baselines and licence policy

`rich deps` (0.0.19) as a CI gate: what each crate adds to a binary, from
cargo-bloat's JSON; a size regression against a saved baseline, which
exits 5; and a licence policy refusing a crate in an SBOM. Every input is
a fixture in the repository. See
[Dependency sizes, baselines and licence policy](cli.md#dependency-sizes-baselines-and-licence-policy-0019).

<div class="tape-player" data-cast="../media/tapes/deps-policy/deps-policy.cast" data-poster="npt:0:3">
  <img src="../media/tapes/deps-policy/deps-policy.gif" alt="rich deps reporting sizes, comparing them with a baseline, and checking a licence policy">
</div>

| Sizes | Baseline | Policy |
|---|---|---|
| ![Each crate's size as a bar, largest first](media/tapes/deps-policy/sizes.png) | ![A crate that grew past the threshold, flagged](media/tapes/deps-policy/baseline.png) | ![A crate whose licence the policy refuses](media/tapes/deps-policy/policy.png) |

[Tape](tapes/deps-policy.tape) · [Cast](media/tapes/deps-policy/deps-policy.cast) · [GIF](media/tapes/deps-policy/deps-policy.gif) · [Page](media/tapes/deps-policy/deps-policy.html)

## Profile a data file

`rich profile` says what each column of a CSV file holds: its type, nulls,
distinct values, statistics and distribution, and a map of where the nulls
are (0.0.16). `rich FILE.csv --infer` puts the same types on the table
itself. See [Profile a data file](cli.md#profile-a-data-file-0016).

<div class="tape-player" data-cast="../media/tapes/profile/profile.cast" data-poster="npt:0:4">
  <img src="../media/tapes/profile/profile.gif" alt="rich profile describing three columns of orders.csv, then the table with inferred types">
</div>

| Profile | `--infer` |
|---|---|
| ![rich profile orders.csv --columns region,amount,express](media/tapes/profile/profile.png) | ![rich orders.csv --infer --head 8](media/tapes/profile/infer.png) |

[Tape](tapes/profile.tape) · [Cast](media/tapes/profile/profile.cast) · [GIF](media/tapes/profile/profile.gif) · [Page](media/tapes/profile/profile.html)

## Schemas and ER diagrams

`rich schema` reads SQL DDL as well as JSON Schema (and Arrow, with the
`arrow` feature): the tables as a tree, with what the reader skipped noted
under it, then `--er` for the same tables as an ER diagram, keys marked and
an edge per foreign key (0.0.16). See
[Schemas](cli.md#schemas-json-schema-sql-ddl-and-arrow-0016).

<div class="tape-player" data-cast="../media/tapes/schema/schema.cast" data-poster="npt:0:4">
  <img src="../media/tapes/schema/schema.gif" alt="rich schema drawing shop.sql as a tree of tables, then as an ER diagram">
</div>

| Tree | `--er` |
|---|---|
| ![rich schema shop.sql](media/tapes/schema/tree.png) | ![rich schema --er shop.sql](media/tapes/schema/er.png) |

[Tape](tapes/schema.tape) · [Cast](media/tapes/schema/schema.cast) · [GIF](media/tapes/schema/schema.gif) · [Page](media/tapes/schema/schema.html)

## Merge conflicts

`rich diff --conflicts` shows a file's merge conflicts: ours, the base (for
a diff3-style conflict) and theirs side by side, with the lines around them
and a count at the end (0.0.16). See
[Merge conflicts](cli.md#merge-conflicts).

<div class="tape-player" data-cast="../media/tapes/conflicts/conflicts.cast" data-poster="npt:0:3">
  <img src="../media/tapes/conflicts/conflicts.gif" alt="rich diff --conflicts merge.rs showing two conflicts">
</div>

![rich diff --conflicts merge.rs](media/tapes/conflicts/conflicts.png)

[Tape](tapes/conflicts.tape) · [Cast](media/tapes/conflicts/conflicts.cast) · [GIF](media/tapes/conflicts/conflicts.gif) · [Page](media/tapes/conflicts/conflicts.html)

## DOT in Markdown

A ```` ```dot ```` fence in a Markdown document is drawn in place of the
code block; `--dot-backend off` leaves it as code, as upstream renders it.
See [the diagrams guide](guide/diagram/index.md).

<div class="tape-player" data-cast="../media/tapes/dot-fence/dot-fence.cast" data-poster="npt:0:3">
  <img src="../media/tapes/dot-fence/dot-fence.gif" alt="A Markdown document with a DOT fence, drawn and then left as code">
</div>

| Drawn | `--dot-backend off` |
|---|---|
| ![The release pipeline drawn as a graph](media/tapes/dot-fence/drawn.png) | ![The DOT source left as a code block](media/tapes/dot-fence/code.png) |

[Tape](tapes/dot-fence.tape) · [Cast](media/tapes/dot-fence/dot-fence.cast) · [GIF](media/tapes/dot-fence/dot-fence.gif) · [Page](media/tapes/dot-fence/dot-fence.html)

## Micro assets

Emoji-sized images in text, `:micro:name:`. A pseudo-terminal speaks no
image protocol, so these show each asset's emoji, or its image as
half-block cells with `RICH_MICRO=blocks`; a Kitty, iTerm2 or Sixel
terminal draws the image itself in the same cells. See
[the built-in library](guide/micro/library.md) for the images.

<div class="tape-player" data-cast="../media/tapes/micro-modes/micro-modes.cast" data-poster="npt:0:6">
  <img src="../media/tapes/micro-modes/micro-modes.gif" alt="The same line with emoji and half-blocks, then fun/heart previewed">
</div>

[Tape](tapes/micro-modes.tape) · [Cast](media/tapes/micro-modes/micro-modes.cast) · [GIF](media/tapes/micro-modes/micro-modes.gif) · [Page](media/tapes/micro-modes/micro-modes.html)

Making an asset: a 128×128 PNG previewed through the pipeline, written
into the user layer with `rich micro create --add`, listed and used.

<div class="tape-player" data-cast="../media/tapes/micro-create/micro-create.cast" data-poster="npt:0:5">
  <img src="../media/tapes/micro-create/micro-create.gif" alt="rich micro create making team/rocket from a PNG">
</div>

| Through the pipeline | Added and used |
|---|---|
| ![rocket.png fitted to 16×16 pixels, magnified](media/tapes/micro-create/pipeline.png) | ![team/rocket listed and used in text](media/tapes/micro-create/added.png) |

[Tape](tapes/micro-create.tape) · [Cast](media/tapes/micro-create/micro-create.cast) · [GIF](media/tapes/micro-create/micro-create.gif) · [Page](media/tapes/micro-create/micro-create.html)

Which mode a terminal gets, and why: `rich doctor` with the variables
kitty and iTerm2 set.

![rich doctor's micro line under five terminal settings](media/tapes/micro-doctor/doctor.png)

[Tape](tapes/micro-doctor.tape) · [Cast](media/tapes/micro-doctor/micro-doctor.cast) · [GIF](media/tapes/micro-doctor/micro-doctor.gif) · [Page](media/tapes/micro-doctor/micro-doctor.html)

`rich micro`: the built-in library listed, `status/loading` previewed, and
`:micro:` codes in `--print --emoji` text.

<div class="tape-player" data-cast="../media/tapes/micro/micro.cast" data-poster="npt:0:4">
  <img src="../media/tapes/micro/micro.gif" alt="rich micro list, preview and markup">
</div>

| List | Preview | In text |
|---|---|---|
| ![rich micro list](media/tapes/micro/list.png) | ![rich micro preview status/loading](media/tapes/micro/preview.png) | ![micro assets in --print text](media/tapes/micro/markup.png) |

[Tape](tapes/micro.tape) · [Cast](media/tapes/micro/micro.cast) · [GIF](media/tapes/micro/micro.gif) · [Page](media/tapes/micro/micro.html)

Micro assets in the interactive views: an icon per row, a badge in the
status bar, an icon on the first crumb and before each of the command
palette's categories, then `rich explore --icons`.

<div class="tape-player" data-cast="../media/tapes/micro-chrome/micro-chrome.cast" data-poster="npt:0:4">
  <img src="../media/tapes/micro-chrome/micro-chrome.gif" alt="Micro assets in the status bar, breadcrumbs, rows and command palette">
</div>

| Chrome | Palette | Explorer |
|---|---|---|
| ![micro assets in the status bar, breadcrumbs and rows](media/tapes/micro-chrome/chrome.png) | ![micro assets before the palette's categories](media/tapes/micro-chrome/palette.png) | ![rich explore --icons](media/tapes/explore-icons/icons.png) |

[Tape](tapes/micro-chrome.tape) · [Cast](media/tapes/micro-chrome/micro-chrome.cast) · [GIF](media/tapes/micro-chrome/micro-chrome.gif) · [Page](media/tapes/micro-chrome/micro-chrome.html) · [Explorer tape](tapes/explore-icons.tape)

## How it works

A tape is a short script, one step per line. This is the watch tape:

```text
--8<-- "docs/tapes/watch.tape"
```

`rich record` runs it: the CLI's own recorder, built on the
[`rs-rich-record`](https://docs.rs/rs-rich-record) crate. It starts a shell
(`bash` unless the tape sets another) on a pseudo-terminal with a pinned
environment (a temporary home and working directory, `TERM=xterm-256color`,
truecolor, UTF-8, UTC), puts the binaries under test first on `PATH`, and
follows the screen with a terminal emulator. `Wait` blocks until the screen
shows the given text or matches a `/regex/`, so a tape never depends on a
fixed delay.

From one run it writes, under `docs/media/tapes/<tape>/`:

- for each `Screenshot`: a PNG, an SVG with selectable text, and a plain-text
  grid of the screen. The SVG and the text grid come from the same
  [frame](guide/ext/live-and-layout.md#frames) as the rest of rich's export, so a screenshot
  looks like any other rich SVG;
- the whole session as an asciinema cast, with the keys pressed;
- a GIF with the keys shown as they are pressed, and an MP4 when FFmpeg is
  installed;
- an HTML page (`<tape>.html`, the **Page** links above): a small player and
  the screenshots, all inline, with text you can select. It fetches nothing
  and plays only when asked;
- `provenance.json`: the tape's fingerprint, the recorder and `rich`
  versions, the commit, and the screenshots written, so the next run removes
  only the ones the tape no longer takes.

Box-drawing characters are drawn as lines rather than font glyphs, so table
borders join between rows, and block, quadrant and braille characters are
drawn as shapes, so images and progress bars have no seams. Text uses DejaVu
Sans Mono and emoji use Twemoji, in colour, both embedded in the recorder, so
a PNG or GIF looks the same on every machine; `--font FILE` chooses another
text font. An emoji cluster such as 👩‍👧, 👍🏽, ❤️ or 🇺🇸 takes one cell as
wide as rich measures it, so the columns after it line up.

| Step | Meaning |
|---|---|
| `Set Size 100x28`, `Set Title "…"`, `Set TypingDelay 40ms`, `Set Timeout 15s`, `Set Env NAME value` | Configure the session |
| `Set WindowFrame off`, `Set Caption "…"`, `Set KeyOverlay off` | Presentation: leave out the window frame (title bar and buttons) around PNGs, SVGs and video; add a line of text under them and under the page's player; leave out the keys shown in video and the player. The frame and the overlay are on by default |
| `Output gif png`, `Output demo.html` | Write only these formats (`png`, `svg`, `cast`, `gif`, `mp4`, `html`; text grids always), or name the file a per-tape format goes to, as in VHS. Several words or lines add up; `--format` can still narrow them |
| `Set Shell zsh` | Run in `bash` (the default), `zsh`, `fish` or `sh`, each without your profile or rc files and with the same `❯` prompt. CI records in all four |
| `Write FILE "text"`, `Exec "command"` | Prepare or change files, outside the terminal |
| `Type "text"` | Type into the terminal, one character at a time |
| `Enter`, `Tab`, `Shift+Tab`, `Space`, `Backspace`, `Delete`, `Insert`, `Escape`, arrows, `Home`, `End`, `PageUp`, `PageDown`, `F1` to `F24`, `Ctrl+C`, `Alt+x`, and the arrows, `Home`, `End`, `PageUp`, `PageDown`, `Delete`, `Insert` and function keys after any of `Shift+`, `Alt+` and `Ctrl+` (`Alt+Left`, `Ctrl+Shift+PageDown`) | Press a key; a number after it repeats it. Modified keys go in xterm's form, `CSI 1;3D` for `Alt+Left`. F1 to F20 are sent as xterm sends them; F21 to F24 have no xterm form, so they go as the kitty keyboard protocol's codes, which an app reads once it has turned that protocol on |
| `Click 10 4`, `RightClick`, `MiddleClick`, `DoubleClick`, `MouseMove 10 4`, `ScrollUp 10 4 3`, `ScrollDown`, `Drag 2 4 20 4` | Use the mouse at a cell (column and row, from 0 at the top left), as SGR mouse reports; a number after a click or scroll repeats it. `Drag` moves a cell at a time with the left button held. The app needs mouse reporting on, as intuiTUIve apps have |
| `Wait "text"`, `Wait /regex/` | Wait until the screen shows it, or has since the previous step began (so fast output that scrolls past is not missed) |
| `Sleep 500ms` | Pause the recording |
| `Screenshot NAME` | Save the screen |
| `Hide`, `Show` | Leave steps out of the recording |
| `Resize 80x24` | Resize the terminal |
| `Mask /regex/ "text"` | Replace matches in the text grids `--check` compares, for output that differs on every run such as temporary paths or timings; images keep what was shown |

### Shells and emoji

A tape can type and print emoji, and the recorder draws them as the program
measures them: rich, and modern terminals, give 👩‍👧 two cells. A shell's
line editor may not agree. bash, zsh and fish take character widths from the
C library, which counts 👩‍👧 as four, so moving the cursor back across a
joined emoji at the prompt (`Left`, `Home`, `Backspace`) redraws the line in
the wrong place, as it does in a real terminal. The command that runs is
still right; only the echoed line is garbled. Keep such text out of line
editing: put it in a file with `Write` and run that, or have the program
print it.

The docs' tapes use `bash`, as CI does. macOS ships bash 3.2, whose line
editing is older than CI's; `rich record` warns when it finds a bash older
than 4, and a newer one (`brew install bash`) first on `PATH` gives
recordings that match.

Regenerate everything, or check it as CI does:

```bash
cargo build -p rs-rich-cli -p rs-rich-interact -p rs-rich-intuituive -p rs-rich-embed \
  --features rs-rich-cli/serve --bins --examples
rich=target/debug/rich
$rich record --bin-dir target/debug --output docs/media/tapes docs/tapes/*.tape
$rich record --check --bin-dir target/debug --output docs/media/tapes docs/tapes/*.tape
```

The pager tape needs `less`, the serve tape `curl` and port 8080, and the
embed tape `w3m`; CI installs them.

## Record your own

`rich record` is not limited to `rich`: a tape can drive any program you can
start from a shell, in `bash`, `zsh`, `fish` or `sh` (`Set Shell`). Write a
tape, then:

```bash
rich record demo.tape                       # writes recordings/demo/
rich record --format gif,png demo.tape      # only the GIF and the PNGs
rich record --format html demo.tape         # the page: a player and the screenshots
rich record --window-frame off --caption 'Save to redraw' demo.tape
rich record --check demo.tape               # fail if a screenshot changed
```

| Option | Meaning |
|---|---|
| `--output DIR` | Write to `DIR/<tape>/` (default `recordings`) |
| `--check` | Compare each screenshot's text with `DIR/<tape>/<name>.txt` instead of writing, and report screenshots the last write listed that the tape no longer takes; exits non-zero on any difference |
| `--format LIST` | Any of `png`, `svg`, `cast`, `gif`, `mp4`, `html`, or `all` (the default). Text grids are always written. A tape's `Output` narrows it further |
| `--window-frame on\|off`, `--caption TEXT`, `--key-overlay on\|off` | Override the tape's `Set WindowFrame`, `Set Caption` and `Set KeyOverlay` |
| `--no-video` | Skip the GIF, the MP4 and the HTML page's player |
| `--bin-dir DIR` | Put `DIR` first on the session's `PATH` (default: the directory of the running `rich`) |
| `--font FILE` | Draw PNG and GIF text in another font |

`$REPO` in a tape is the directory `rich record` was started in, so a tape
can copy fixtures (`Exec "cp $REPO/fixtures/data.json ."`). Linux and macOS
are supported; on Windows it builds through ConPTY but needs the tape's shell
on `PATH`, and is experimental. Use the recorder from Rust through the
`rs-rich-record` crate: `tape::parse`, `record::record`, then
`record::write` or `record::check`.

### Limits, and what a tape can do

**A tape is code: record only tapes you trust.** `Exec` runs its command with
`sh`, and everything `Type` sends runs in the shell, with your user's
permissions. The recorder pins the environment and works in a temporary
directory so recordings repeat, not to contain the tape.

What the recorder does limit, so a mistake fails clearly rather than filling
the disk or memory:

| Limit | |
|---|---|
| Terminal size | `Set Size` and `Resize` from 2x2 to 500x200 |
| Durations | `Sleep`, `Wait` and `Set Timeout` at most `3600s` |
| `Write` | A relative path inside the working directory: no `/…`, no `..` |
| `Exec` | 60 seconds; the first 64 KiB of its error output is reported |
| Tape names | A tape's name is its output directory: `...tape` (named `..`) is refused |
| Stale screenshots | Removed only when `provenance.json` lists them from an earlier run; other files in the output directory are never touched |
| Frames | At most 12 a second; a burst over 32 KiB is kept as a repaint of the screen |
| Video | The first 5 minutes; a GIF, MP4 or HTML page of a longer recording is an error (use `--no-video`) |
| Images | At most 100 million pixels; a GIF at most 65535 pixels a side |
