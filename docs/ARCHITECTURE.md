# Architecture

A three-crate Cargo workspace with a strict, one-directional dependency rule.

```
┌────────────┐     ┌────────────┐     ┌───────────────────────────┐
│  rich-cli  │ ──▶ │  rich-ext  │ ──▶ │  rich (faithful core)     │
│  (binary)  │     │ (our code) │     │  mirrors upstream `rich`  │
│  = 1.8.1   │     │  = 0.1.0   │     │  = 15.0.0                 │
└────────────┘     └────────────┘     └───────────────────────────┘
        the arrow never points left ─────────────▶
```

- **`crates/rich`** — the faithful port of the Python `rich` *library*. Mirrors
  upstream module-for-module; version equals the upstream release it reflects.
  Depends on nothing in this workspace and knows nothing about the other crates.
- **`crates/rich-ext`** — everything that is *ours*: extra highlighters,
  renderables, and the internal plugin registry. Independent SemVer. Talks to core
  only through public APIs and the extension traits.
- **`crates/rich-cli`** — the binary mirroring the Python `rich-cli` tool (a
  separate upstream project with its own version). Built on `rich` + `rich-ext`.

Why the split: it makes upstream syncs a mechanical diff-and-port of `crates/rich`
only, and guarantees our features can never make that harder. See
[AGENTS.md](../AGENTS.md).

## Render pipeline (first slice)

```
&str (markup)
   │  markup::render + Theme          → Text (plain string + Style spans)
   ▼
Text                                   crates/rich/src/text.rs
   │  Highlighter(s) add spans         (rich-ext, via Console)
   │  Text::render(base, system)       → Vec<Segment>
   ▼
Segment { text, style, control }       crates/rich/src/segment.rs
   │  Console applies color system     (downgrade + SGR)
   ▼
ANSI bytes → stdout                    crates/rich/src/console.rs
```

`ColorSystem` (Standard / EightBit / Truecolor / Windows) is detected by the
`Console` and applied last: styles carry rich color information and are *downgraded*
to the target system at the final step via the redmean nearest-color search
(`color::match_color`, ported from `rich.palette`).

## Key types

| type | file | upstream |
|------|------|----------|
| `Color`, `ColorTriplet`, `ColorSystem` | `color.rs` | `rich.color` |
| `Style` | `style.rs` | `rich.style` |
| `Text`, `Span` | `text.rs` | `rich.text` |
| `Segment` | `segment.rs` | `rich.segment` |
| `Console`, `ConsoleBuilder` | `console.rs` | `rich.console` |
| `Renderable`, `Highlighter` (extension points) | `protocol.rs` | `rich.protocol` / `rich.abc` |
| `Theme` | `theme.rs` | `rich.theme` |

See [PORTING.md](PORTING.md) for the full module map and per-module status, and
[PLUGINS.md](PLUGINS.md) for the extension model.
