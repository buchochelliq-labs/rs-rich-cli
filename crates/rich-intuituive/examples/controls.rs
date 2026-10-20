//! A settings form built from intuiTUIve's stock controls: a select
//! field, a slider, a check box, a switch, a radio group, a collapsible
//! section, buttons in three looks, a spinner and a form shown as loading
//! while it saves.
//!
//!     cargo run -p rs-rich-intuituive --example controls
//!     cargo run -p rs-rich-intuituive --example controls -- --linear
//!
//! Tab and Shift+Tab move between the controls. On the theme, Enter opens
//! its menu and a letter jumps to the theme it starts; ←/→ move the font
//! size (Home and End to its ends, or drag it); Space ticks a box and
//! flips the switch; ↑/↓ pick the indentation; Enter opens Advanced. Save
//! saves (the form is busy for a second), Reset puts everything back and
//! Delete profile asks first. q quits, when the focus is not on a control
//! that takes the letter.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{
    button, checkbox, collapsible, radio, select_field, slider, spinner, switch_toggle,
    ButtonVariant,
};

/// The themes on offer.
pub const THEMES: [&str; 4] = ["Dark", "Light", "Solarized", "Mono"];

/// The settings, as signals the controls hold.
#[derive(Clone, Copy)]
struct Settings {
    theme: Signal<usize>,
    font_size: Signal<f64>,
    line_numbers: Signal<bool>,
    dark_title: Signal<bool>,
    indent: Signal<usize>,
    advanced: Signal<bool>,
    telemetry: Signal<bool>,
    autosave: Signal<f64>,
}

impl Settings {
    fn new() -> Settings {
        Settings {
            theme: signal(0),
            font_size: signal(14.0),
            line_numbers: signal(true),
            dark_title: signal(false),
            indent: signal(0),
            advanced: signal(false),
            telemetry: signal(false),
            autosave: signal(30.0),
        }
    }

    /// Back to how they started.
    fn reset(self) {
        self.theme.set(0);
        self.font_size.set(14.0);
        self.line_numbers.set(true);
        self.dark_title.set(false);
        self.indent.set(0);
        self.telemetry.set(false);
        self.autosave.set(30.0);
    }
}

/// A field: its name in a column of its own, then its control. The
/// control is labelled itself, so the name is not read twice.
fn field(name: &str, control: Node) -> Node {
    row([
        label(format!("[dim]{name}[/]"))
            .access_hidden(true)
            .fixed(12),
        control,
    ])
    .fixed(1)
}

/// A blank row between sections, not read out.
fn gap() -> Node {
    label("").access_hidden(true).fixed(1)
}

/// The app.
pub fn controls_app() -> App {
    App::new(|| {
        let s = Settings::new();
        let saving = signal(false);
        let saved = signal(0u32);
        let save = move |cx: &mut Ctx| {
            if saving.get_untracked() {
                return;
            }
            saving.set(true);
            cx.after(Duration::from_secs(1), move |cx| {
                saving.set(false);
                saved.update(|n| *n += 1);
                cx.toast("Settings saved");
            });
        };
        let delete = |cx: &mut Ctx| {
            cx.confirm(
                "Delete profile",
                "Delete this profile and its settings?",
                vec!["Delete".into(), "Keep".into()],
                |cx| cx.toast("Profile deleted"),
            )
        };

        let appearance = column([
            field(
                "Theme",
                select_field(|| THEMES.map(String::from).to_vec(), s.theme)
                    .label("Theme")
                    .fixed(16),
            ),
            field(
                "Font size",
                row([
                    slider(s.font_size, 8.0..=32.0, 1.0)
                        .label("Font size")
                        .fixed(25),
                    text!(" {}pt", s.font_size),
                ]),
            ),
            checkbox("Show line numbers", s.line_numbers).fixed(1),
            switch_toggle("Dark title bar", s.dark_title).fixed(1),
        ])
        .auto();
        let editor = row([
            label("[dim]Indent with[/]").access_hidden(true).fixed(12),
            radio(|| vec!["Spaces".into(), "Tabs".into()], s.indent)
                .label("Indent with")
                .fixed(12),
        ])
        .fixed(2);
        let advanced = collapsible(
            "Advanced",
            s.advanced,
            column([
                checkbox("Send anonymous usage data", s.telemetry).fixed(1),
                field(
                    "Autosave",
                    row([
                        slider(s.autosave, 0.0..=120.0, 10.0)
                            .label("Autosave every")
                            .fixed(25),
                        text!(" every {}s", s.autosave),
                    ]),
                ),
            ])
            .padding(0, 2)
            .auto(),
        );
        let form = column([
            label("[b]Appearance[/]").fixed(1),
            appearance,
            gap(),
            label("[b]Editor[/]").fixed(1),
            editor,
            gap(),
            advanced,
        ])
        .padding(0, 1)
        .panel("Settings")
        .loading_when(move || saving.get());
        let status = row([
            spinner("dots").spinning_when(move || saving.get()).fixed(2),
            text(move || match (saving.get(), saved.get()) {
                (true, _) => "Saving…".into(),
                (false, 0) => "Not saved yet".into(),
                (false, 1) => "Saved".into(),
                (false, n) => format!("Saved {n} times"),
            }),
        ])
        .live()
        .fixed(1);
        let buttons = row([
            button("Save", save).variant(ButtonVariant::Primary),
            button("Reset", move |_| s.reset()),
            button("Delete profile", delete).variant(ButtonVariant::Danger),
        ])
        .gap(2)
        .fixed(1);

        column([
            form,
            status.padding(0, 1),
            buttons.padding(0, 1),
            label("[dim]tab moves · enter opens · space ticks · ←/→ slide · q quits[/]").fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let linear = std::env::args().skip(1).any(|arg| arg == "--linear");
    controls_app().linear(linear).run()
}
