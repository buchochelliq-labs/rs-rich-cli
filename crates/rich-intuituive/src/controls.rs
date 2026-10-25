//! Stock form controls: a [`button`], a [`checkbox`], a [`radio`] group, a
//! [`switch_toggle`] and a [`slider`]. Each is a [`Widget`] bound to a
//! signal (or an action), with keys and the mouse, an accessibility role
//! and state, and a look taken from theme styles, so a theme, a theme file
//! or [`Node::theme_styles`] restyles it. The select field, the spinner and
//! the collapsible section are in their own modules.
//!
//! ```
//! use intuituive::prelude::*;
//! use intuituive::{button, checkbox, slider, ButtonVariant};
//!
//! let app = App::new(|| {
//!     let wrap = signal(true);
//!     let volume = signal(40.0);
//!     let saved = signal(0);
//!     column([
//!         checkbox("Wrap lines", wrap),
//!         slider(volume, 0.0..=100.0, 10.0).label("Volume"),
//!         button("Save", move |_| saved.update(|n| *n += 1)).variant(ButtonVariant::Primary),
//!         text!("saved {saved}"),
//!     ])
//! });
//! let mut driver = app.driver(20, 4);
//! driver.press("space tab right tab enter");
//! let rows = driver.screen().plain();
//! assert_eq!(rows[0].trim_end(), "[ ] Wrap lines");
//! assert_eq!(rows[3].trim_end(), "saved 1");
//! ```
//!
//! The theme styles they draw with, each with a fallback:
//!
//! | Style | What it styles |
//! |---|---|
//! | `button`, `button.primary`, `button.danger` | a button's face, by [variant](ButtonVariant) |
//! | `<face>.focus` (else `button.focus`) | over the face, with the focus or the pointer on it, or held (`bold reverse`) |
//! | `<face>.disabled` (else `button.disabled`) | over the face, while it is disabled (`dim`) |
//! | `<face>.light`, `<face>.shade` (and `<face>.focus.light`, …) | a button three rows tall: its top and bottom edges' colours |
//! | `checkbox.on`, `radio.on` | the tick and the dot (the theme's `accent`) |
//! | `switch.on`, `switch.off` | the switch's track (`good`, `muted`) |
//! | `slider.filled`, `slider.track`, `slider.knob` | the slider's two sides and its knob |
//! | `control.focus` | a focused control's text, or the slider's knob (`reverse`) |
//! | `control.disabled` | a disabled control (`dim`) |

use std::ops::RangeInclusive;

use rich::Style;
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::{AccessState, Role};
use crate::app::Ctx;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;
use crate::select_field::{Choices, SelectOption};
use crate::widget::{
    markup_width, named_style, widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget,
    WidgetEvent,
};

/// Console `markup` as plain text: what a control's label says.
pub(crate) fn plain(markup: &str) -> String {
    rich::Text::from_markup(markup)
        .map(|text| text.plain().to_string())
        .unwrap_or_else(|_| markup.to_string())
}

/// The first of the theme styles `names` the console has, else `fallback`
/// parsed.
pub(crate) fn look(cx: &DrawCx, names: &[&str], fallback: &str) -> Style {
    names
        .iter()
        .find_map(|name| named_style(cx.console(), name))
        .unwrap_or_else(|| Style::parse(fallback).unwrap_or_default())
}

/// What lies over a control's text: dimmed while it is disabled, the focus
/// style while it has the focus.
fn over(cx: &DrawCx, focused: bool) -> Option<Style> {
    if cx.disabled() {
        Some(cx.style("control.disabled", "dim"))
    } else if focused {
        Some(cx.style("control.focus", "reverse"))
    } else {
        None
    }
}

/// A label read without subscribing to what it reads: for what is said
/// about a control, outside its drawing.
fn untracked_label(label: &Label) -> String {
    crate::reactive::untracked(label)
}

/// Whether `key` presses a control: Enter or Space.
fn presses(code: KeyCode) -> bool {
    matches!(code, KeyCode::Enter | KeyCode::Char(' '))
}

/// A button's look: which theme styles its face comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ButtonVariant {
    /// The theme's `button` style.
    #[default]
    Default,
    /// `button.primary`: the action a form is for.
    Primary,
    /// `button.danger`: an action that destroys something.
    Danger,
}

impl ButtonVariant {
    /// The theme style its face is drawn in.
    fn style(self) -> &'static str {
        match self {
            ButtonVariant::Default => "button",
            ButtonVariant::Primary => "button.primary",
            ButtonVariant::Danger => "button.danger",
        }
    }
}

type Press = Box<dyn FnMut(&mut Ctx)>;

/// A control's label: console markup from a closure that may read signals.
type Label = Box<dyn Fn() -> String>;

/// What [`button`] makes.
struct ButtonWidget {
    /// Its label, read each time it is measured, drawn or described.
    label: Label,
    variant: ButtonVariant,
    on_press: Press,
    /// Held down with the mouse: it looks pressed until it is let go.
    held: bool,
}

/// A button labelled `label` (console markup) that runs `on_press` when
/// it is pressed: Enter or Space while it has the focus, or a click (a
/// press let go over it). Its face is the theme's `button` style, or
/// another [variant](ButtonVariant)'s ([`Node::variant`]); it looks picked
/// while it has the focus or the pointer, and pressed (`[Save]`) while
/// the mouse holds it. Three rows tall, it draws lit top and shaded bottom
/// edges when the theme names their colours. [`Node::disabled_when`]
/// greys it out and stops it. Its role is
/// [`Button`](crate::a11y::Role::Button), named by its label.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::button;
///
/// let app = App::new(|| {
///     let count = signal(0);
///     let locked = signal(false);
///     column([
///         button("Count", move |_| count.update(|n| *n += 1))
///             .disabled_when(move || locked.get()),
///         text!("{count}"),
///     ])
///     .on_key("l", move |_| locked.set(true))
/// });
/// let mut driver = app.driver(10, 2);
/// driver.press("enter space l enter");
/// let rows = driver.screen().plain();
/// // A column stretches it across: the label in the middle.
/// assert_eq!(rows[0].trim_end(), "  Count");
/// assert_eq!(rows[1].trim_end(), "2");
/// assert_eq!(driver.accessibility()[0].describe(), "Count, button, disabled");
/// ```
pub fn button(label: impl Into<String>, on_press: impl FnMut(&mut Ctx) + 'static) -> Node {
    let label = label.into();
    button_of(move || label.clone(), on_press)
}

/// A [`button`] whose label is read from `label`, a closure that may read
/// signals: Play that becomes Pause, a count, a job's state. A change
/// draws it again, and lays it out again when its width changes. It stays
/// the same node: it keeps the focus, and a press the mouse holds, while
/// its label changes, and its accessible name follows the label.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::button_of;
///
/// let app = App::new(|| {
///     let playing = signal(false);
///     button_of(
///         move || if playing.get() { "Pause".into() } else { "Play".into() },
///         move |_| playing.update(|p| *p = !*p),
///     )
/// });
/// let mut driver = app.driver(9, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim(), "Play");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[0].trim(), "Pause");
/// assert_eq!(driver.accessibility()[0].describe(), "Pause, button");
/// ```
pub fn button_of(
    label: impl Fn() -> String + 'static,
    on_press: impl FnMut(&mut Ctx) + 'static,
) -> Node {
    widget(ButtonWidget {
        label: Box::new(label),
        variant: ButtonVariant::Default,
        on_press: Box::new(on_press),
        held: false,
    })
    .auto()
}

impl Node {
    /// For a [`button`]: its look, the theme's `button.primary` or
    /// `button.danger` rather than `button`. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{button, ButtonVariant};
    ///
    /// let app = App::new(|| {
    ///     row([
    ///         button("Delete", |_| {}).variant(ButtonVariant::Danger),
    ///         button("Keep", |_| {}),
    ///     ])
    /// })
    /// .theme(Theme::dark().style("button.danger", rich::Style::parse("white on red").unwrap()));
    /// let mut driver = app.driver(14, 1);
    /// driver.press("tab");
    /// // The focus is on Keep: Delete shows its face as the theme has it.
    /// let screen = driver.screen();
    /// assert_eq!(screen.plain()[0], " Delete  Keep ");
    /// let face = screen.style(screen.cell(0, 0).style).unwrap().definition();
    /// assert_eq!(face, "white on red");
    /// ```
    pub fn variant(self, variant: ButtonVariant) -> Node {
        self.widget_mut(|button: &mut ButtonWidget| button.variant = variant);
        self
    }
}

impl ButtonWidget {
    fn press(&mut self, cx: &mut EventCx) {
        (self.on_press)(cx.app());
    }
}

impl Widget for ButtonWidget {
    fn name(&self) -> &'static str {
        "button"
    }

    fn describe(&self) -> Option<String> {
        Some(plain(&untracked_label(&self.label)))
    }

    fn role(&self) -> Role {
        Role::Button
    }

    fn access_text(&self) -> Option<String> {
        Some(plain(&untracked_label(&self.label)))
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => markup_width(&(self.label)()).saturating_add(2),
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let face_name = self.variant.style();
        let disabled = cx.disabled();
        // Asked every time, so the pointer coming and going draws it again.
        let hovered = cx.hovered();
        let picked = !disabled && (self.held || cx.focused() || hovered);
        let state = match (disabled, picked) {
            (true, _) => "disabled",
            (false, true) => "focus",
            (false, false) => "",
        };
        let mut face = look(cx, &[face_name, "button"], "reverse");
        if !state.is_empty() {
            let own = format!("{face_name}.{state}");
            let shared = format!("button.{state}");
            let fallback = if disabled { "dim" } else { "bold reverse" };
            face = face.combine(&look(cx, &[&own, &shared], fallback));
        }
        let (width, height) = (canvas.width(), canvas.height());
        canvas.fill(0, 0, width, height, Some(&face));
        let text_mode = crate::a11y::text_mode();
        if height >= 3 && !text_mode {
            // The edges, in the colours the theme names: lit above and
            // shaded below, swapped while it is held.
            let edge = |part: &str| {
                let names = match state.is_empty() {
                    true => vec![format!("{face_name}.{part}"), format!("button.{part}")],
                    false => vec![
                        format!("{face_name}.{state}.{part}"),
                        format!("{face_name}.{part}"),
                        format!("button.{part}"),
                    ],
                };
                names
                    .iter()
                    .find_map(|name| named_style(cx.console(), name))
                    .map(|style| face.combine(&style))
            };
            let (top, bottom) = match self.held {
                true => (edge("shade"), edge("light")),
                false => (edge("light"), edge("shade")),
            };
            if let Some(top) = top {
                canvas.print(0, 0, &"▔".repeat(width as usize), Some(&top));
            }
            if let Some(bottom) = bottom {
                canvas.print(0, height - 1, &"▁".repeat(width as usize), Some(&bottom));
            }
        }
        let markup = (self.label)();
        let label = markup_width(&markup).min(width);
        let x = width.saturating_sub(label) / 2;
        let y = height / 2;
        canvas.markup(cx.console(), x, y, label, &markup, Some(&face));
        // Held, or in text mode, the label is in brackets, in the padding.
        if (self.held || text_mode) && x > 0 && x + label < width {
            canvas.print(x - 1, y, "[", Some(&face));
            canvas.print(x + label, y, "]", Some(&face));
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Key(key) if presses(key.code) => self.press(cx),
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(MouseButton::Left) => {
                    self.held = true;
                    cx.capture_mouse();
                    cx.redraw();
                }
                MouseKind::Up(_) if self.held => {
                    self.held = false;
                    cx.release_mouse();
                    cx.redraw();
                    // Let go over the button: a click.
                    let (width, height) = cx.size();
                    if mouse.column < width && mouse.row < height {
                        self.press(cx);
                    }
                }
                MouseKind::Drag(MouseButton::Left) if self.held => {}
                _ => return Used::No,
            },
            WidgetEvent::Focus(false) if self.held => {
                self.held = false;
                cx.redraw();
            }
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// A check box's state: ticked, clear, or mixed, the state of a parent
/// whose children are partly ticked (a folder of files, a group of
/// permissions). Drawn `[x]`, `[ ]` and `[-]`; assistive technology reads
/// checked, not checked and mixed (ARIA's `aria-checked="mixed"`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CheckState {
    #[default]
    Off,
    On,
    Mixed,
}

impl CheckState {
    /// On for `true`, off for `false`.
    pub fn from_bool(on: bool) -> CheckState {
        if on {
            CheckState::On
        } else {
            CheckState::Off
        }
    }

    /// Whether it is [`On`](Self::On).
    pub fn is_on(self) -> bool {
        self == CheckState::On
    }
}

impl From<bool> for CheckState {
    fn from(on: bool) -> CheckState {
        CheckState::from_bool(on)
    }
}

/// What a [`Toggle`] holds: on or off, or one of three states.
enum Value {
    Two(Signal<bool>),
    Three {
        state: Signal<CheckState>,
        /// Where a press takes it from mixed ([`Node::on_mixed`]).
        from_mixed: CheckState,
    },
}

impl Value {
    fn get(&self, tracked: bool) -> CheckState {
        match (self, tracked) {
            (Value::Two(on), true) => on.get().into(),
            (Value::Two(on), false) => on.get_untracked().into(),
            (Value::Three { state, .. }, true) => state.get(),
            (Value::Three { state, .. }, false) => state.get_untracked(),
        }
    }

    /// What a press makes it: on from off, off from on, and from mixed
    /// what [`Node::on_mixed`] said (on, unless told).
    fn pressed(&self) -> CheckState {
        match (self.get(false), self) {
            (CheckState::Mixed, Value::Three { from_mixed, .. }) => *from_mixed,
            (CheckState::On, _) => CheckState::Off,
            _ => CheckState::On,
        }
    }

    fn set(&self, to: CheckState) {
        match self {
            Value::Two(on) => on.set(to == CheckState::On),
            Value::Three { state, .. } => state.set(to),
        }
    }
}

/// What [`checkbox`] and [`switch_toggle`] make.
struct Toggle {
    label: Label,
    value: Value,
    /// A switch, rather than a check box.
    switch: bool,
}

/// A check box labelled `label` (console markup), ticked while `on` holds:
/// `[x] Wrap lines`. Space, Enter or a click ticks it or clears it. The
/// tick is drawn in the theme's `checkbox.on` style (else `accent`). Its
/// role is [`CheckBox`](crate::a11y::Role::CheckBox), named by its label,
/// with its checked state. [`checkbox_tri`] has a third, mixed state, and
/// [`checkbox_of`] a label that changes.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::checkbox;
///
/// let app = App::new(|| checkbox("Wrap lines", signal(false)));
/// let mut driver = app.driver(16, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[ ] Wrap lines");
/// driver.press("space");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[x] Wrap lines");
/// assert_eq!(driver.accessibility()[0].describe(), "Wrap lines, check box, checked");
/// ```
pub fn checkbox(label: impl Into<String>, on: Signal<bool>) -> Node {
    let label = label.into();
    toggle(Box::new(move || label.clone()), Value::Two(on), false)
}

/// A check box of three states, labelled `label` (console markup), in the
/// state `state` holds: `[x]` on, `[ ]` off, `[-]` mixed. Space, Enter or
/// a click turn it off from on and on from off; from mixed they turn it
/// on, or what [`Node::on_mixed`] says. Only code sets it mixed: a parent
/// that sums up its children. Read as checked, not checked, or mixed.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{checkbox_tri, CheckState};
///
/// let app = App::new(|| checkbox_tri("All files", signal(CheckState::Mixed)));
/// let mut driver = app.driver(16, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[-] All files");
/// assert_eq!(driver.accessibility()[0].describe(), "All files, check box, mixed");
/// driver.press("space");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[x] All files");
/// ```
pub fn checkbox_tri(label: impl Into<String>, state: Signal<CheckState>) -> Node {
    let label = label.into();
    checkbox_of(move || label.clone(), state)
}

/// A [`checkbox_tri`] whose label is read from `label`, a closure that
/// may read signals (`3 of 5 chosen`). The node, its focus and its state
/// stay as the label changes, and its accessible name follows it.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{checkbox_of, CheckState};
///
/// let app = App::new(|| {
///     let state = signal(CheckState::Off);
///     checkbox_of(move || format!("Sync ({:?})", state.get()), state)
/// });
/// let mut driver = app.driver(16, 1);
/// driver.press("space");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "[x] Sync (On)");
/// assert_eq!(driver.accessibility()[0].describe(), "Sync (On), check box, checked");
/// ```
pub fn checkbox_of(label: impl Fn() -> String + 'static, state: Signal<CheckState>) -> Node {
    let value = Value::Three {
        state,
        from_mixed: CheckState::On,
    };
    toggle(Box::new(label), value, false)
}

/// A switch labelled `label` (console markup), on while `on` holds: a
/// knob at the right of its track when on (`──●`), at the left when off
/// (`●──`), the track in the theme's `switch.on` style (else `good`) or
/// `switch.off` (else `muted`). Space, Enter or a click flips it; → turns
/// it on and ← off. Its role is [`Switch`](crate::a11y::Role::Switch),
/// named by its label, with its checked state.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::switch_toggle;
///
/// let app = App::new(|| switch_toggle("Dark theme", signal(false)));
/// let mut driver = app.driver(16, 1);
/// driver.press("right");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "──● Dark theme");
/// assert_eq!(driver.accessibility()[0].describe(), "Dark theme, switch, checked");
/// ```
pub fn switch_toggle(label: impl Into<String>, on: Signal<bool>) -> Node {
    let label = label.into();
    switch_of(move || label.clone(), on)
}

/// A [`switch_toggle`] whose label is read from `label`, a closure that
/// may read signals.
pub fn switch_of(label: impl Fn() -> String + 'static, on: Signal<bool>) -> Node {
    toggle(Box::new(label), Value::Two(on), true)
}

fn toggle(label: Label, value: Value, switch: bool) -> Node {
    widget(Toggle {
        label,
        value,
        switch,
    })
    .auto()
}

impl Node {
    /// For a [`checkbox_tri`] or [`checkbox_of`]: the state a press takes
    /// it to from mixed. On by default; [`Off`](CheckState::Off) clears a
    /// partly chosen group first. Ignored on other nodes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{checkbox_tri, CheckState};
    ///
    /// let app = App::new(|| {
    ///     checkbox_tri("All", signal(CheckState::Mixed)).on_mixed(CheckState::Off)
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("space");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "[ ] All");
    /// ```
    pub fn on_mixed(self, next: CheckState) -> Node {
        self.widget_mut(|toggle: &mut Toggle| {
            if let Value::Three { from_mixed, .. } = &mut toggle.value {
                *from_mixed = next;
            }
        });
        self
    }
}

impl Toggle {
    /// The marker, and the cells it takes with the space after it.
    fn marker(&self, state: CheckState) -> &'static str {
        let on = state == CheckState::On;
        match (self.switch, state, crate::a11y::text_mode()) {
            (false, CheckState::Mixed, _) => "[-]",
            (false, CheckState::On, _) => "[x]",
            (false, CheckState::Off, _) => "[ ]",
            (true, _, false) if on => "──●",
            (true, _, false) => "●──",
            (true, _, true) if on => "[on ]",
            (true, _, true) => "[off]",
        }
    }
}

impl Widget for Toggle {
    fn name(&self) -> &'static str {
        if self.switch {
            "switch"
        } else {
            "checkbox"
        }
    }

    fn describe(&self) -> Option<String> {
        Some(plain(&untracked_label(&self.label)))
    }

    fn role(&self) -> Role {
        if self.switch {
            Role::Switch
        } else {
            Role::CheckBox
        }
    }

    fn access_text(&self) -> Option<String> {
        Some(plain(&untracked_label(&self.label)))
    }

    fn access_state(&self) -> AccessState {
        let state = self.value.get(false);
        AccessState {
            checked: (state != CheckState::Mixed).then_some(state == CheckState::On),
            mixed: state == CheckState::Mixed,
            ..AccessState::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => {
                let marker = rich::cells::cell_len(self.marker(CheckState::Off)) as u16;
                marker
                    .saturating_add(1)
                    .saturating_add(markup_width(&(self.label)()))
            }
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let state = self.value.get(true);
        let on = state == CheckState::On;
        let marker = self.marker(state);
        let style = match (self.switch, state) {
            (false, CheckState::Off) => None,
            (false, _) => Some(look(cx, &["checkbox.on", "accent"], "bold")),
            (true, _) if on => Some(look(cx, &["switch.on", "good"], "green")),
            (true, _) => Some(look(cx, &["switch.off", "muted"], "dim")),
        };
        let (x, used) = if self.switch {
            let cells = rich::cells::cell_len(marker) as u16;
            canvas.print(0, 0, marker, style.as_ref());
            (cells, cells)
        } else {
            // The brackets plain, the tick styled.
            canvas.print(0, 0, "[", None);
            canvas.print(1, 0, &marker[1..2], style.as_ref());
            canvas.print(2, 0, "]", None);
            (3, 3)
        };
        let room = canvas.width().saturating_sub(x + 1);
        let label = (self.label)();
        let shown = canvas.markup(cx.console(), x + 1, 0, room, &label, None);
        let used = used + 1 + shown;
        if let Some(over) = over(cx, cx.focused()) {
            canvas.restyle(0, 0, used, 1, &over);
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let to = match event {
            WidgetEvent::Key(key) if presses(key.code) => self.value.pressed(),
            WidgetEvent::Key(key) if self.switch && key.code == KeyCode::Right => CheckState::On,
            WidgetEvent::Key(key) if self.switch && key.code == KeyCode::Left => CheckState::Off,
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                self.value.pressed()
            }
            _ => return Used::No,
        };
        self.value.set(to);
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// What [`radio`] and [`radio_keyed`] make.
struct Radio {
    choices: Choices,
    /// Letters jump to the option they start ([`radio_keyed`]'s).
    typeahead: Option<crate::select_field::Typeahead>,
}

/// A group of radio buttons, one per row: `options` (console markup; may
/// read signals), with the one `selected` holds chosen: `(•) Medium`. ↑/↓
/// (or ←/→) move the choice within the group, Home and End go to the first
/// and the last, and a click chooses an option. The dot is drawn in the
/// theme's `radio.on` style (else `accent`). Its role is
/// [`RadioGroup`](crate::a11y::Role::RadioGroup): name it with
/// [`label`](Node::label); its value is the chosen option, checked, and
/// its place among them. [`radio_keyed`] knows its options by key, and
/// may disable some.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::radio;
///
/// let app = App::new(|| {
///     let size = signal(0usize);
///     radio(|| vec!["Small".into(), "Medium".into(), "Large".into()], size).label("Size")
/// });
/// let mut driver = app.driver(12, 3);
/// driver.press("down");
/// assert_eq!(driver.screen().plain()[..2], ["( ) Small   ", "(•) Medium  "]);
/// assert_eq!(
///     driver.accessibility()[0].describe(),
///     "Size, radio group, 2 of 3: Medium, checked"
/// );
/// ```
pub fn radio(options: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    radio_of(Choices::indexed(options, selected), false)
}

/// A [`radio`] group whose options are known by key, as
/// [`select_field_keyed`](crate::select_field_keyed)'s are: `selected`
/// holds the chosen option's key (`None`: no dot), which stays with its
/// option through a reorder and becomes `None` when that option goes. A
/// disabled option is drawn greyed with its reason; the arrows, Home,
/// End, a click and typing pass it by. Typing letters jumps to the
/// option they start, as in a select field.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{radio_keyed, SelectOption};
///
/// let app = App::new(|| {
///     let plan = signal(Some(1u8));
///     radio_keyed(
///         || {
///             vec![
///                 SelectOption::new(1, "Free"),
///                 SelectOption::new(2, "Team").disabled("needs an invite"),
///                 SelectOption::new(3, "Pro"),
///             ]
///         },
///         plan,
///     )
///     .label("Plan")
/// });
/// let mut driver = app.driver(30, 3);
/// driver.press("down");
/// let rows = driver.screen().plain();
/// assert_eq!(rows[1].trim_end(), "( ) Team (needs an invite)");
/// assert_eq!(rows[2].trim_end(), "(•) Pro");
/// ```
pub fn radio_keyed<K: Clone + PartialEq + 'static>(
    options: impl Fn() -> Vec<SelectOption<K>> + 'static,
    selected: Signal<Option<K>>,
) -> Node {
    radio_of(Choices::keyed(options, selected), true)
}

fn radio_of(choices: Choices, typeahead: bool) -> Node {
    widget(Radio {
        choices,
        typeahead: typeahead.then(crate::select_field::Typeahead::default),
    })
    .auto()
}

impl Widget for Radio {
    fn name(&self) -> &'static str {
        "radio"
    }

    fn role(&self) -> Role {
        Role::RadioGroup
    }

    fn cursor(&self) -> Option<Rect> {
        let at = self.choices.current(false).unwrap_or(0);
        Some(Rect::new(0, at as u16, u16::MAX, 1))
    }

    fn access_value(&self) -> Option<String> {
        let options = self.choices.options();
        match self.choices.current(false) {
            Some(at) => options.get(at).map(|option| plain(&option.label)),
            // A keyed group whose choice went.
            None if !options.is_empty() => Some("none".to_string()),
            None => None,
        }
    }

    fn access_state(&self) -> AccessState {
        let count = self.choices.options().len();
        let at = self.choices.current(false).filter(|_| count > 0);
        AccessState {
            checked: at.map(|_| true),
            position: at.map(|at| (at.min(count - 1) + 1, count)),
            ..AccessState::default()
        }
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        let options = self.choices.options();
        match axis {
            Axis::Horizontal => options
                .iter()
                .map(|o| markup_width(&o.markup()))
                .max()
                .unwrap_or(0)
                .saturating_add(4),
            Axis::Vertical => options.len().min(u16::MAX as usize) as u16,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let options = self.choices.options();
        let selected = self.choices.current(true);
        let dot = look(cx, &["radio.on", "accent"], "bold");
        let over = over(cx, cx.focused());
        let disabled = cx.disabled();
        let off = cx.style("control.disabled", "dim");
        for (row, option) in options.iter().enumerate() {
            let y = row as u16;
            let chosen = Some(row) == selected;
            canvas.print(0, y, "(", None);
            match chosen {
                true => canvas.print(1, y, "•", Some(&dot)),
                false => canvas.print(1, y, " ", None),
            }
            canvas.print(2, y, ")", None);
            let room = canvas.width().saturating_sub(4);
            let shown = canvas.markup(cx.console(), 4, y, room, &option.markup(), None);
            // The focus marks the chosen option; disabled greys them all.
            if let Some(over) = over.as_ref().filter(|_| chosen || disabled) {
                canvas.restyle(0, y, 4 + shown, 1, over);
            } else if !option.enabled {
                canvas.restyle(0, y, 4 + shown, 1, &off);
            }
        }
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let options = self.choices.options();
        let count = options.len();
        if count == 0 {
            return Used::No;
        }
        let now = self.choices.current(false);
        let to = match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Up | KeyCode::Left => Choices::step(&options, now, -1, true),
                KeyCode::Down | KeyCode::Right => Choices::step(&options, now, 1, true),
                KeyCode::Home => Choices::end(&options, false),
                KeyCode::End => Choices::end(&options, true),
                // Space chooses the option the focus is on: it is.
                KeyCode::Char(' ') => now.or_else(|| Choices::end(&options, false)),
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
                    let Some(typeahead) = self.typeahead.as_mut() else {
                        return Used::No;
                    };
                    let labels = Choices::typeahead_labels(&options);
                    match typeahead.key(c, &labels, now.unwrap_or(count - 1)) {
                        Some(to) => Some(to),
                        None => return Used::No,
                    }
                }
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) if mouse.is_click() => {
                let row = mouse.row as usize;
                if row >= count {
                    return Used::No;
                }
                // A click on a disabled option is used, and does nothing.
                Some(row)
            }
            _ => return Used::No,
        };
        if let Some(to) = to {
            self.choices.choose(to);
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// What [`slider`] makes.
struct Slider {
    value: Signal<f64>,
    min: f64,
    max: f64,
    step: f64,
    /// The width it drew at: where a click lands.
    width: u16,
}

/// A slider over `range`, holding `value`, moved in steps of `step` (none
/// when 0): a track with a knob, filled up to it. ←/↓ and →/↑ move it a
/// step, PageDown and PageUp a tenth of the range, Home and End to its
/// ends; a click puts the knob under the pointer, and a drag moves it, as
/// does the wheel. The filled side is in the theme's `slider.filled` style
/// (else `accent`), the rest in `slider.track` (else `muted`), the knob in
/// `slider.knob`; with the focus the knob takes `control.focus`. Its role
/// is [`Slider`](crate::a11y::Role::Slider): name it with
/// [`label`](Node::label); its value is the number. Twenty cells wide
/// unless sized.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::slider;
///
/// let app = App::new(|| {
///     let volume = signal(50.0);
///     column([
///         slider(volume, 0.0..=100.0, 5.0).label("Volume").fixed(1),
///         text!("{volume}"),
///     ])
/// });
/// let mut driver = app.driver(11, 2);
/// driver.press("right right end left");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "95");
/// assert_eq!(driver.accessibility()[0].describe(), "Volume, slider: 95");
/// ```
pub fn slider(value: Signal<f64>, range: RangeInclusive<f64>, step: f64) -> Node {
    let (min, max) = (*range.start(), *range.end());
    let (min, max) = if min <= max { (min, max) } else { (max, min) };
    widget(Slider {
        value,
        min,
        max,
        step: step.abs(),
        width: 0,
    })
    .auto()
}

impl Slider {
    /// `value` kept in the range and on a step.
    fn snap(&self, value: f64) -> f64 {
        let value = value.clamp(self.min, self.max);
        if self.step <= 0.0 {
            return value;
        }
        let steps = ((value - self.min) / self.step).round();
        (self.min + steps * self.step).clamp(self.min, self.max)
    }

    /// Where the knob is, from 0 to 1.
    fn fraction(&self, value: f64) -> f64 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        ((value - self.min) / span).clamp(0.0, 1.0)
    }

    /// The value at column `x` of a track `width` wide.
    fn at(&self, x: u16, width: u16) -> f64 {
        let last = width.saturating_sub(1).max(1) as f64;
        let fraction = (x as f64 / last).clamp(0.0, 1.0);
        self.snap(self.min + fraction * (self.max - self.min))
    }

    /// A tenth of the range, in whole steps (one at least).
    fn page(&self) -> f64 {
        let tenth = (self.max - self.min) / 10.0;
        if self.step <= 0.0 {
            return tenth;
        }
        (tenth / self.step).round().max(1.0) * self.step
    }

    /// The value as it is read: as many decimals as the step has.
    fn spoken(&self, value: f64) -> String {
        let decimals = if self.step > 0.0 {
            let mut decimals = 0;
            let mut step = self.step;
            while (step - step.round()).abs() > 1e-9 && decimals < 6 {
                step *= 10.0;
                decimals += 1;
            }
            decimals
        } else {
            2
        };
        format!("{value:.decimals$}")
    }
}

impl Widget for Slider {
    fn name(&self) -> &'static str {
        "slider"
    }

    fn describe(&self) -> Option<String> {
        Some(self.spoken(self.value.get_untracked()))
    }

    fn role(&self) -> Role {
        Role::Slider
    }

    fn access_value(&self) -> Option<String> {
        Some(self.spoken(self.value.get_untracked()))
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => 20,
            Axis::Vertical => 1,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let width = canvas.width();
        self.width = width;
        if width == 0 {
            return;
        }
        let value = self.value.get();
        let knob_at = (self.fraction(value) * (width - 1) as f64).round() as u16;
        let text_mode = crate::a11y::text_mode();
        let (filled, track, knob) = match text_mode {
            true => ("=", "-", "|"),
            false => ("━", "─", "●"),
        };
        let filled_style = look(cx, &["slider.filled", "accent"], "bold");
        let track_style = look(cx, &["slider.track", "muted"], "dim");
        let mut knob_style = look(cx, &["slider.knob", "slider.filled", "accent"], "bold");
        canvas.print(0, 0, &filled.repeat(knob_at as usize), Some(&filled_style));
        let rest = width.saturating_sub(knob_at + 1) as usize;
        canvas.print(knob_at + 1, 0, &track.repeat(rest), Some(&track_style));
        if cx.disabled() {
            canvas.print(knob_at, 0, knob, Some(&knob_style));
            canvas.restyle(0, 0, width, 1, &cx.style("control.disabled", "dim"));
            return;
        }
        if cx.focused() {
            knob_style = knob_style.combine(&cx.style("control.focus", "reverse"));
        }
        canvas.print(knob_at, 0, knob, Some(&knob_style));
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let now = self.value.get_untracked();
        let step = if self.step > 0.0 {
            self.step
        } else {
            (self.max - self.min) / 100.0
        };
        let to = match event {
            WidgetEvent::Key(key) => match key.code {
                KeyCode::Left | KeyCode::Down => now - step,
                KeyCode::Right | KeyCode::Up => now + step,
                KeyCode::PageDown => now - self.page(),
                KeyCode::PageUp => now + self.page(),
                KeyCode::Home => self.min,
                KeyCode::End => self.max,
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(MouseButton::Left) => {
                    cx.capture_mouse();
                    self.at(mouse.column, self.width.max(cx.size().0))
                }
                MouseKind::Drag(MouseButton::Left) => {
                    self.at(mouse.column, self.width.max(cx.size().0))
                }
                MouseKind::Up(_) => {
                    cx.release_mouse();
                    return Used::Yes;
                }
                MouseKind::ScrollUp => now + step,
                MouseKind::ScrollDown => now - step,
                _ => return Used::No,
            },
            _ => return Used::No,
        };
        let to = self.snap(to);
        if to != now {
            self.value.set(to);
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}
