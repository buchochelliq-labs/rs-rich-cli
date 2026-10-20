# Buttons and form controls

The small controls a form is made of: buttons, check boxes, radio groups,
switches, sliders, a drop-down select field, collapsible sections, and
spinners, with a node shown as loading. Each is a node bound to a signal
(a button, to an action), takes the keys and the mouse, tells assistive
technology what it is and what state it is in, and draws in theme styles,
so a theme, a theme file or `.theme_styles(..)` restyles it. Text fields,
calendars and date pickers are on [Inputs, text and dates](components.md).

`examples/controls.rs` is a settings form built from all of them:

```bash
cargo run -p rs-rich-intuituive --example controls
```

```text
--8<-- "crates/rich-intuituive/tests/snapshots/controls_form_changed.txt"
```

Each control is a function at the crate's root:

```rust
use intuituive::{
    button, checkbox, collapsible, radio, select_field, slider, spinner, switch_toggle,
    ButtonVariant,
};
```

Sized `.auto()`, a control takes one row in a column and its natural
width in a row; a column stretches it across unless it is given a size.
`.disabled_when(..)` greys any of them out (the theme's
`control.disabled`, else `dim`), takes it out of the Tab order and stops
its keys and clicks.

## Buttons

```rust
let saved = signal(0);
row([
    button("Save", move |_| saved.update(|n| *n += 1)).variant(ButtonVariant::Primary),
    button("Cancel", |cx| cx.pop()),
    button("Delete", delete).variant(ButtonVariant::Danger).disabled_when(move || locked.get()),
])
.gap(2)
```

`button(label, on_press)` runs `on_press` (it gets the `Ctx`) when Enter
or Space is pressed while it has the focus, or when it is clicked: a press
let go over it. While the mouse holds it, it looks pressed: `[Save]`.
Its label is console markup, with a cell of padding either side, centred
in whatever width it is given.

Its face is a theme style: `button`, or with `.variant(..)`
`button.primary` or `button.danger`. Over the face goes `.focus` while it
has the focus or the pointer, or is held (`button.primary.focus`, else
`button.focus`, else `bold reverse`), and `.disabled` while it is
disabled (else `dim`). The dark, light and mono themes each name the three
faces.

Three rows tall, a button draws a lit top edge and a shaded bottom one
when the theme names their colours (`button.light` and `button.shade`,
`button.focus.light` and so on), swapped while it is held. The
[widget gallery](ratatui-examples.md#widgets-a-widget-gallery)'s
Custom tab gives each of its three buttons its own colours this way:

```rust
button("Red", press)
    .theme_styles([
        ("button", "#f0d0d0 on #802020"),
        ("button.light", "#c04040"),
        ("button.shade", "#401010"),
        ("button.focus", "#ffffff on #b03030"),
        ("button.focus.light", "#f06060"),
        ("button.focus.shade", "#601818"),
    ])
    .fixed(15)
```

A screen reader hears `Save, button`, and `disabled` when it is.

## Check boxes and switches

```rust
let wrap = signal(true);
let dark = signal(false);
column([
    checkbox("Wrap lines", wrap),
    switch_toggle("Dark title bar", dark),
])
```

```text
[x] Wrap lines
●── Dark title bar
```

`checkbox(label, on)` and `switch_toggle(label, on)` hold a
`Signal<bool>`: Space, Enter or a click flips it, and a switch also takes
→ for on and ← for off. The tick is in the theme's `checkbox.on` style
(else `accent`); the switch's track is in `switch.on` (else `good`) or
`switch.off` (else `muted`). With the focus, the control's text takes
`control.focus` (else `reverse`). (The node `switch(..)` is the content
switcher of [Nodes and layout](layout.md#nodes), hence `switch_toggle`.)

They are read as `Wrap lines, check box, checked` and
`Dark title bar, switch, not checked`. For a node of your own that is on
or off, `.checked_when(..)` gives it the same state (see
[Accessibility](accessibility.md)).

## Radio groups

```rust
let indent = signal(0usize);
radio(|| vec!["Spaces".into(), "Tabs".into()], indent).label("Indent with")
```

```text
(•) Spaces
( ) Tabs
```

`radio(options, selected)` draws one option a row, the one `selected`
holds marked. ↑/↓ (or ←/→) move the choice within the group, round from
the last to the first, Home and End go to the ends, and a click chooses an
option. The dot is in `radio.on` (else `accent`); with the focus, the
chosen option takes `control.focus`. Name the group with `.label(..)`: it
is read as `Indent with, radio group, 1 of 2: Spaces, checked`.

## Sliders

```rust
let size = signal(14.0);
row([
    slider(size, 8.0..=32.0, 1.0).label("Font size").fixed(25),
    text!(" {size}pt"),
])
```

```text
━━━━━━●────────────────── 14pt
```

`slider(value, range, step)` holds a `Signal<f64>`, kept in the range and
on a step (a step of 0 moves freely). ←/↓ and →/↑ move it a step,
PageDown and PageUp a tenth of the range, Home and End to its ends. A
click puts the knob under the pointer and a drag moves it (the slider
keeps the mouse until the button is let go); the wheel moves it a step.
The filled side is `slider.filled` (else `accent`), the rest
`slider.track` (else `muted`), the knob `slider.knob`; with the focus the
knob takes `control.focus`. It is twenty cells wide unless sized.

It is read as `Font size, slider: 14`, its value with as many decimals as
its step has, and a browser gets `aria-valuenow`.

## Select fields

```rust
let theme = signal(0usize);
select_field(|| vec!["Dark".into(), "Light".into(), "Solarized".into()], theme)
    .label("Theme")
    .fixed(16)
```

`select_field(options, selected)` shows the chosen option and a `▾`.
Enter, Space or a click opens a [menu](screens.md#menus) of the options under
it, at the chosen one: ↑/↓ move, Enter or a click chooses, Esc closes it.
Closed, ↑/↓ choose the option before or after. In either, letters jump: a
letter goes to the next option it starts (again, round those it starts),
and letters typed together, under a second apart, to the option they
start. A letter no option starts goes on up to the node's ancestors, so
`q` still quits. The field is drawn in the theme's `select` style with
`control.focus` over it; the menu is a menu's.

It is read as `Theme, combo box, 3 of 3: Solarized, collapsed`
(`expanded` while its menu is open).

## Collapsible sections

```rust
let open = signal(false);
collapsible(
    "Advanced",
    open,
    column([checkbox("Send usage data", telemetry), slider(autosave, 0.0..=120.0, 10.0)]),
)
```

```text
▸ Advanced
```

`collapsible(title, open, child)` is a header over `child`, shown while
`open` holds. Enter, Space or a click on the header opens or closes it, →
opens it and ← closes it. Closed, the child takes no room and is out of
the Tab order. The section is as tall as what it shows. The title is in
`collapsible.title` (else `bold`); in text mode the marker is `[+]` or
`[-]`. The header is read as `Advanced, button, collapsed` (or
`expanded`).

## Spinners and loading

```rust
let saving = signal(false);
row([
    spinner("dots").spinning_when(move || saving.get()).fixed(2),
    text!("{status}"),
])
.live()
```

`spinner(name)` is one of rich's spinners (`dots`, `line`, `arc`,
`bouncingBar`, … all of `rich::spinner::spinner_names()`; `dots` for a
name it does not know), turning at its own speed on the app's clock, in
the theme's `spinner` style (else `accent`). It asks for a frame only when
its next glyph is due, and none while it is stopped:
`.spinning_when(condition)` spins it while the condition holds and leaves
it blank otherwise. Its glyph is decoration, hidden from screen readers:
put what is happening in text beside it, as above. In text mode it is a
still `*`.

`.loading_when(condition)` shows any node as loading while the condition
holds: dimmed (the theme's `loading` style, else `dim`) under a spinner in
its middle, and busy, so a screen reader hears
`Settings, region, busy`. The node still takes keys and clicks: give it
`.disabled_when(..)` with the same condition to stop them.

```rust
form.panel("Settings").loading_when(move || saving.get())
```

## The theme styles

| Style | Fallback | What it styles |
|---|---|---|
| `button`, `button.primary`, `button.danger` | named by each theme | a button's face |
| `<face>.focus`, then `button.focus` | `bold reverse` | over the face, picked or held |
| `<face>.disabled`, then `button.disabled` | `dim` | over the face, disabled |
| `<face>.light`, `<face>.shade` (and `.focus.light`, …) | none | a tall button's edges |
| `checkbox.on`, `radio.on` | `accent` | the tick, the dot |
| `switch.on`, `switch.off` | `good`, `muted` | a switch's track |
| `slider.filled`, `slider.track`, `slider.knob` | `accent`, `muted`, `slider.filled` | a slider |
| `select` | none | a select field |
| `collapsible.title` | `bold` | a section's header |
| `spinner` | `accent` | a spinner's glyph |
| `loading` | `dim` | a node shown as loading |
| `control.focus` | `reverse` | a focused control's text, a slider's knob |
| `control.disabled` | `dim` | a disabled control |

A [theme file](panels.md#theme-files-reloaded-live) names them as it names any style
(`button.primary = bold white on purple`), and `.theme_styles(..)` sets
them for one node and what is inside it.
