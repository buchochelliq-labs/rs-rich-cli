//! A live theme picker (#460): each theme's preview is the same sample,
//! rendered with that theme's styles, so moving through the list shows the
//! difference as you go.

use std::sync::Arc;
use std::time::Duration;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};

use crate::component::{Component, Context, Flow, View};
use crate::components::{PreviewLayout, Select, Theme};
use crate::event::Event;
use crate::item::{Item, Preview};
use crate::keymap::Keymap;
use crate::policy::{LineIo, NotInteractive};

/// The markup a [`ThemePicker`] previews by default: the semantic names
/// (`info` … `error`), repr and JSON highlighting, logging and a rule, so
/// most of what a theme restyles shows.
pub const THEME_SAMPLE: &str = "[bold]Theme preview[/]
[info]info[/]  [success]success[/]  [warning]warning[/]  [error]error[/]
[repr.number]1234[/] [repr.str]'a string'[/] [repr.bool_true]True[/] [repr.bool_false]False[/] [repr.none]None[/]
[repr.url]https://example.com[/]  [repr.path]/usr/local/bin[/]
[json.key]\"name\"[/]: [json.str]\"rich\"[/], [json.key]\"ok\"[/]: [json.bool_true]true[/]
[log.time][12:00:01][/] [logging.level.info]INFO[/]     [log.message]started[/]
[logging.level.warning]WARNING[/]  [logging.level.error]ERROR[/]  [rule.line]────────[/]
[markdown.h1]Heading[/]  [markdown.code]code[/]  [markdown.link]link[/]";

/// `sample` rendered with `theme` layered over the extended theme
/// ([`rich_ext::theme::extended_theme`]), in the colours of the console
/// rendering it.
struct Themed {
    theme: rich::Theme,
    sample: Arc<dyn Renderable + Send + Sync>,
}

impl Themed {
    fn console(&self, outer: &Console, options: &ConsoleOptions) -> Console {
        let mut theme = rich_ext::theme::extended_theme();
        theme.extend_from(&self.theme);
        Console::builder()
            .width(options.max_width.max(1))
            .force_terminal(true)
            .color_system(outer.color_system())
            .no_color(outer.no_color())
            .theme(theme)
            .build()
    }
}

impl Renderable for Themed {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let inner = self.console(console, options);
        let options = inner.options().update_width(options.max_width.max(1));
        inner.render(&*self.sample, Some(&options))
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let inner = self.console(console, options);
        self.sample.measure(&inner, options)
    }
}

/// Pick a theme while its preview shows a sample rendered in it (#460).
/// Returns the theme's name.
///
/// Themes come from anywhere: a configuration file's `[themes.NAME]`, a
/// plugin's (`ExtensionRegistry::theme`), or code. Each is layered over
/// the [extended theme](rich_ext::theme::extended_theme), so a theme that
/// sets only a few names previews with the rest at their defaults.
///
/// ```
/// use rich::{Style, Theme};
/// use rich_interact::components::ThemePicker;
/// use rich_interact::headless::{self, Script};
///
/// let mut night = Theme::new();
/// night.insert("info", Style::parse("bold blue").unwrap());
/// let picker = ThemePicker::new("Theme", [("default", Theme::new()), ("night", night)]);
/// let (outcome, record) = headless::run(picker, Script::new().keys("down enter"), 80, 14);
/// assert_eq!(outcome.unwrap().value().as_deref(), Some("night"));
/// assert!(record.frames[0].contains("Theme preview"));
/// ```
pub struct ThemePicker {
    select: Select<String>,
}

impl ThemePicker {
    /// Pick one of `themes`, by name, previewing [`THEME_SAMPLE`].
    pub fn new<I, S>(prompt: impl Into<String>, themes: I) -> ThemePicker
    where
        I: IntoIterator<Item = (S, rich::Theme)>,
        S: Into<String>,
    {
        let sample = Text::from_markup(THEME_SAMPLE).expect("the sample is valid markup");
        Self::with_sample(prompt, themes, Arc::new(sample))
    }

    /// Pick one of `themes`, previewing `sample` in each.
    pub fn with_sample<I, S>(
        prompt: impl Into<String>,
        themes: I,
        sample: Arc<dyn Renderable + Send + Sync>,
    ) -> ThemePicker
    where
        I: IntoIterator<Item = (S, rich::Theme)>,
        S: Into<String>,
    {
        let items: Vec<Item<String>> = themes
            .into_iter()
            .map(|(name, theme)| {
                let name = name.into();
                let preview = Preview::Renderable(Arc::new(Themed {
                    theme,
                    sample: Arc::clone(&sample),
                }));
                Item::new(name.clone(), name).preview(preview)
            })
            .collect();
        ThemePicker {
            select: Select::new(prompt, items),
        }
    }

    /// Show at most `rows` themes at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.select = self.select.height(rows);
        self
    }

    /// Where the preview goes (default: beside the list from 72 columns).
    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.select = self.select.preview(layout);
        self
    }

    /// Rows the preview takes when it is below the list (default 10).
    pub fn preview_height(mut self, rows: usize) -> Self {
        self.select = self.select.preview_height(rows);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.select = self.select.theme(theme);
        self
    }

    /// Focus the theme called `name` first; it is also the answer without
    /// a terminal.
    pub fn default(mut self, name: &str) -> Self {
        if let Some(index) = self
            .select
            .items()
            .iter()
            .position(|item| item.label == name)
        {
            self.select = self.select.default(index);
        }
        self
    }

    /// Report the mouse: see [`Select::with_mouse`].
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.select = self.select.with_mouse(on);
        self
    }

    /// The themes' names, in order.
    pub fn names(&self) -> Vec<&str> {
        self.select
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect()
    }
}

impl Component for ThemePicker {
    type Output = String;

    fn keymap(&self) -> Keymap {
        self.select.visible_keymap()
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<String> {
        self.select.handle(event, context)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.select.render(context)
    }

    fn tick(&self) -> Option<Duration> {
        self.select.tick_interval()
    }

    fn mouse(&self) -> bool {
        Component::mouse(&self.select)
    }

    fn default_value(&self) -> Option<String> {
        self.select.default_value()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<String>, NotInteractive> {
        self.select.prompt(io)
    }
}
