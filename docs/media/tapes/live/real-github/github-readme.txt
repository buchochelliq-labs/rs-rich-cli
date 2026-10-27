 NOCTILUCA   READER + KITTY   ◀▶   GitHub - buchochelliq-labs/rs-rich-cli · GitHub
 https://github.com/buchochelliq-labs/rs-rich-cli
forms, a reactive terminal UI framework (intuiTUIve), a terminal recorder, and images in the terminal. Tracking rich   │
15.0.0 and rich-cli 1.8.1 ( UPSTREAM.toml); every parity claim is a golden test captured from the real Python library. │
📖 Documentation, tutorial and gallery rich render… Every picture here is a terminal recording made by rich record     │
from a script in docs/tapes/ , re-run by CI on every change. Install and try In Rust, cargo add rs-rich and use        │
rich::Console : Getting started has a complete program; the tutorial builds up from there. New in 0.0.22 Published on  │
2026-10-25 (release notes). Parity, the plugin marketplace and a terminal web browser: Parity with rich 15.0.0: the    │
widget, live display and document modules ported or recorded as divergences, scope.py ported, a verdict for every      │
divergence, and the differential fuzzer on Markdown, trees, columns, progress, Pretty and JSON. The marketplace: the   │
new rs-rich-marketplace crate (a signed index, verified installs, a lockfile), rich plugin with browse and publish,    │
and WASM plugins on by default. intuiTUIve: flush panel titles, a tree table, min_size with a node of its own, labels  │
that change in place, a three-state check box, options known by key and a live action list. Noctiluca (new): a web     │
browser for the terminal, on intuiTUIve and a headless Chrome. Recording: a tape writes an APNG only when asked. Memo  │
has a spot for every crate. The changelog lists every change. Early releases, and the versions say so. The crates      │
version independently by ordinary SemVer; the number is not tied to the upstream release. Expect breaking API changes. │
Which upstream version is tracked lives in UPSTREAM.toml . See AGENTS.md for why the version is not mirrored.          │
Workspace Each package follows independent SemVer. The Python releases being tracked are recorded separately in        │
UPSTREAM.toml . A vX.Y.Z tag selects a coordinated workspace release; a <crate>-vX.Y.Z tag selects only that crate.    │
See the release documentation. These are the manifest versions in this checkout. Publication status is recorded in the │
release notes; the crates.io links show available packages. The published package names carry an rs- prefix because    │
rich is already taken on crates.io by an unrelated crate. The library targets keep the short names, so you still write │
use rich::… . The dependency arrow only ever points one way: rich-cli → rich-ext → rich and rich-cli → rich-art → rich │
, with rich-ext → rich-macros behind ext's optional macros feature; the other crates sit beside them (the full graph   │
is in AGENTS.md). Core depends on none of them, and nothing depends back on the CLI. This keeps the core a clean       │
mirror and makes upstream syncs a mechanical diff-and-port. See docs/ARCHITECTURE.md. Status Usable, not complete.     │
Read that literally — the parts listed as done are byte-parity tested against real Python rich 15.0.0, and the parts   ┃
that aren't listed genuinely aren't there. Ported and parity-tested: colour (truecolor/256/standard downgrade) ·       ┃
styles · markup · Text (wrapping, justification incl. full, overflow, spans, markup() ) · Console (capture, export to  ┃
HTML/SVG, paging, control codes) · Segment · themes · highlighters · Table · Panel · Rule · Align · Padding · Columns  ┃
· Constrain · Tree · Layout · Screen · Markdown · Syntax · JSON · Pretty · Traceback · Progress (incl. track() and     ┃
RenderableColumn ) · Spinner · Status · Live · LogRender · Bar · Prompt · emoji · filesize. Not ported: Windows        ┃
legacy-console support (a console without VT processing shows raw escape sequences; use Windows Terminal), Jupyter     │
integration, and inspect / repr of Python objects (no Rust equivalent — see #19). A log / tracing handler (            │
RichHandler ) lives in rs-rich-ext . Known to differ from upstream, deliberately and with reasons, in                  │
docs/DIVERGENCES.md — most notably syntax highlighting uses syntect rather than Pygments, so highlighted code is not   │
byte-identical. Per-module detail is in docs/PORTING.md. What comes next, and why, is in docs/ROADMAP.md; the tracking │
epic is #16. Releases Each release has notes in docs/releases/ (the newest published one is 0.0.22), and CHANGELOG.md  │
 match 1 of 1
 f1 actions | f hints | o open | r reader | q quit
