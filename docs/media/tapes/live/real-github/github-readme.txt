 NOCTILUCA   READER + KITTY   ◀▶   GitHub - buchochelliq-labs/rs-rich-cli · GitHub
 https://github.com/buchochelliq-labs/rs-rich-cli
Every picture here is a terminal recording made by rich record from a script in docs/tapes/, re-run by CI on every     │
change.                                                                                                                │
                                                                                                                       │
Install and try                                                                                                        │
                                                                                                                       │
drawing                                                                                                                │
                                                                                                                       │
▏cargo install rs-rich-cli                                                                                             │
▏rich --print '[bold magenta]Hello[/] [green]World[/]'                                                                 │
▏rich README.md          # Markdown, JSON, CSV, source, notebooks, images…                                             │
▏rich --demo             # the guided tour; Ctrl+C stops                                                               │
                                                                                                                       │
In Rust, cargo add rs-rich and use rich::Console:                                                                      │
                                                                                                                       │
▏use rich::Console;                                                                                                    │
▏                                                                                                                      │
▏let console = Console::new();                                                                                         │
▏console.print_str("[bold red]Hello[/] [green]World[/] — 42");                                                         │
                                                                                                                       │
Getting started has a complete program; the tutorial builds up from there.                                             │
                                                                                                                       │
New in 0.0.22                                                                                                          │
                                                                                                                       ┃
drawing                                                                                                                ┃
                                                                                                                       │
Published on 2026-10-25 (release notes). Parity, the plugin marketplace and a terminal web browser:                    │
                                                                                                                       │
• Parity with rich 15.0.0: the widget, live display and document modules ported or recorded as divergences, scope.py   │
  ported, a verdict for every divergence, and the differential fuzzer on Markdown, trees, columns, progress, Pretty    │
  and JSON.                                                                                                            │
• The marketplace: the new rs-rich-marketplace crate (a signed index, verified installs, a lockfile), rich plugin with │
  browse and publish, and WASM plugins on by default.                                                                  │
• intuiTUIve: flush panel titles, a tree table, min_size with a node of its own, labels that change in place, a        │
  three-state check box, options known by key and a live action list.                                                  │
• Noctiluca (new): a web browser for the terminal, on intuiTUIve and a headless Chrome.                                │
• Recording: a tape writes an APNG only when asked. Memo has a spot for every crate.                                   │
 match 1 of 1
 f1 actions | f hints | o open | r reader | q quit
