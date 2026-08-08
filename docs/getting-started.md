# Getting started

## Install

=== "Library"

    ```bash
    cargo add rs-rich
    ```

=== "CLI"

    ```bash
    cargo install rs-rich-cli
    ```

=== "Both, in a Cargo.toml"

    ```toml
    [dependencies]
    rs-rich = "0.0.1"
    ```

## The package is `rs-rich`, the crate is `rich`

`rich` was already taken on crates.io by an unrelated crate, so the published
package carries an `rs-` prefix. The library target keeps the short name, so the
dependency and the `use` line differ:

```toml
rs-rich = "0.0.1"      # what you depend on
```

```rust
use rich::Console;      // what you write
```

The same applies to the others: `rs-rich-ext` is `rich_ext`, `rs-rich-art` is
`rich_art`. The CLI package `rs-rich-cli` installs a binary called `rich`.

## Hello, world

```rust
use rich::Console;

fn main() {
    let console = Console::new();
    console.print_str("[bold magenta]Hello[/] [green]World[/]");
}
```

`Console::new()` detects the terminal: whether it *is* one, how wide it is, and
which colour system it supports (truecolor, 256 or 16 colours). When output is
redirected to a file, styling is dropped automatically — so piping to `less` or a
log file gives clean text without you doing anything.

## Controlling the console

Detection is right most of the time. When it isn't — in tests, in CI, when
generating documentation — configure it explicitly:

```rust
use rich::{ColorSystem, Console};

let console = Console::builder()
    .force_terminal(true)                          // style even when redirected
    .color_system(Some(ColorSystem::Truecolor))
    .width(80)                                     // ignore the real width
    .build();
```

!!! tip "Reproducible output"

    For snapshot tests, pin `width`, `force_terminal` and `color_system`.
    Otherwise the same code produces different bytes on a different terminal, and
    your snapshots will fight you.

## Capturing instead of printing

Any render can be captured as a string rather than written out:

```rust
let ansi = console.capture(|c| c.print_str("[bold]hi[/]"));
assert_eq!(ansi, "\x1b[1mhi\x1b[0m\n");
```

That is also how the exports work — see [Exporting](tutorial/06-cli.md#exporting).

## Where next

- The [tutorial](tutorial/index.md) builds up from a line of text to a live
  progress display.
- The [gallery](gallery.md) is the fastest way to see what exists.
- [Parity](parity.md) explains what "a port" means here, and where it stops.
