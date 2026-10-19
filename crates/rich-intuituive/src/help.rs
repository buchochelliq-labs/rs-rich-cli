//! The help's options ([`Ctx::help_with`](crate::Ctx::help_with)): its
//! title, how it says keys, and its layout.

use rich_interact::{Key, KeyCode};

/// How the help says a binding's keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyFormat {
    /// As the framework names them: `ctrl+u`, `↑/k`, `g g`.
    #[default]
    Default,
    /// Vim's key notation, as Yazi's and Helix's helps say keys: `<C-u>`,
    /// `<A-x>`, `<S-Tab>`, `<Up>`, `<Esc>`, `<Space>`, a sequence as one
    /// word (`gg`), and keys that do the same separated by commas
    /// (`<Up>, k`).
    Vim,
}

impl KeyFormat {
    /// `keys` said this way: keys pressed one after another if `sequence`,
    /// else keys that do the same.
    ///
    /// ```
    /// use intuituive::interact::Key;
    /// use intuituive::KeyFormat;
    ///
    /// let keys = [Key::parse("ctrl+u").unwrap(), Key::parse("pageup").unwrap()];
    /// assert_eq!(KeyFormat::Vim.keys(&keys, false), "<C-u>, <PageUp>");
    /// let g = Key::parse("g").unwrap();
    /// assert_eq!(KeyFormat::Vim.keys(&[g, g], true), "gg");
    /// assert_eq!(KeyFormat::Default.keys(&keys, false), "ctrl+u/pgup");
    /// ```
    pub fn keys(self, keys: &[Key], sequence: bool) -> String {
        match self {
            KeyFormat::Default => rich_interact::chrome::keys_hint_of(keys, sequence),
            KeyFormat::Vim => keys
                .iter()
                .map(|key| vim_key(*key))
                .collect::<Vec<_>>()
                .join(if sequence { "" } else { ", " }),
        }
    }
}

/// One key in vim's notation.
fn vim_key(key: Key) -> String {
    let named = match key.code {
        KeyCode::Char(' ') => Some("Space".to_string()),
        KeyCode::Char('<') => Some("lt".to_string()),
        KeyCode::Char('\\') => Some("Bslash".to_string()),
        KeyCode::Char('|') => Some("Bar".to_string()),
        KeyCode::Char(_) => None,
        KeyCode::Enter => Some("Enter".to_string()),
        KeyCode::Tab => Some("Tab".to_string()),
        KeyCode::BackTab => Some("S-Tab".to_string()),
        KeyCode::Backspace => Some("BS".to_string()),
        KeyCode::Delete => Some("Del".to_string()),
        KeyCode::Insert => Some("Insert".to_string()),
        KeyCode::Escape => Some("Esc".to_string()),
        KeyCode::Up => Some("Up".to_string()),
        KeyCode::Down => Some("Down".to_string()),
        KeyCode::Left => Some("Left".to_string()),
        KeyCode::Right => Some("Right".to_string()),
        KeyCode::Home => Some("Home".to_string()),
        KeyCode::End => Some("End".to_string()),
        KeyCode::PageUp => Some("PageUp".to_string()),
        KeyCode::PageDown => Some("PageDown".to_string()),
        KeyCode::F(n) => Some(format!("F{n}")),
        KeyCode::Menu => Some("Menu".to_string()),
        KeyCode::Modifier(which) => Some(
            match which {
                rich_interact::ModifierKey::Shift => "Shift",
                rich_interact::ModifierKey::Ctrl => "Ctrl",
                rich_interact::ModifierKey::Alt => "Alt",
                rich_interact::ModifierKey::Super => "Super",
            }
            .to_string(),
        ),
    };
    let mut held = String::new();
    if key.modifiers.ctrl {
        held.push_str("C-");
    }
    if key.modifiers.alt {
        held.push_str("A-");
    }
    if key.modifiers.shift {
        held.push_str("S-");
    }
    match (named, key.code) {
        // A character alone is itself: `g`, `G`, `=`.
        (None, KeyCode::Char(c)) if held.is_empty() => c.to_string(),
        (None, KeyCode::Char(c)) => format!("<{held}{c}>"),
        (Some(name), _) => format!("<{held}{name}>"),
        (None, _) => String::new(),
    }
}

/// How [`Ctx::help_with`](crate::Ctx::help_with) shows the help: its title,
/// how it says keys ([`KeyFormat`]), and whether it lists every binding in
/// one list under its search line (Yazi's layout) rather than under a
/// heading for each section. Rows too long for the help wrap, whatever the
/// options.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{HelpOptions, KeyFormat};
///
/// let app = App::new(|| {
///     label("files")
///         .bind("ctrl+u", "Half a page up", |_| {})
///         .on_key("?", |cx| {
///             cx.help_with(HelpOptions::new().title("mgr.help").key_format(KeyFormat::Vim))
///         })
/// });
/// let mut driver = app.driver(50, 12);
/// driver.press("?");
/// let help = driver.screen().plain().join("\n");
/// assert!(help.contains("mgr.help") && help.contains("<C-u>"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelpOptions {
    pub(crate) title: String,
    pub(crate) key_format: KeyFormat,
    pub(crate) filter_first: bool,
    pub(crate) filter_key: Option<Key>,
    pub(crate) all_panes: bool,
}

impl Default for HelpOptions {
    fn default() -> HelpOptions {
        HelpOptions::new()
    }
}

impl HelpOptions {
    /// The help as [`Ctx::help`](crate::Ctx::help) shows it: titled
    /// `Help`, keys as the framework names them, a heading per section.
    pub fn new() -> HelpOptions {
        HelpOptions {
            title: "Help".to_string(),
            key_format: KeyFormat::Default,
            filter_first: false,
            filter_key: None,
            all_panes: false,
        }
    }

    /// The help panel's title.
    pub fn title(mut self, title: impl Into<String>) -> HelpOptions {
        self.title = title.into();
        self
    }

    /// How the keys are said.
    pub fn key_format(mut self, format: KeyFormat) -> HelpOptions {
        self.key_format = format;
        self
    }

    /// The search line first, a rule under it, and every binding in one
    /// list with no section headings (Yazi's help), rather than a heading
    /// for each section.
    pub fn filter_first(mut self, on: bool) -> HelpOptions {
        self.filter_first = on;
        self
    }

    /// List the described bindings of every node on the screen, wherever
    /// the focus is, rather than only those of the focused node and its
    /// ancestors: one help for the whole app, a section per pane (each
    /// node's [`bind_in`](crate::Node::bind_in) section, else its name),
    /// those on the focus path first. termusic's and bottom's helps.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::HelpOptions;
    ///
    /// let app = App::new(|| {
    ///     row([
    ///         label("library").focusable().bind("a", "Add to playlist", |_| {}),
    ///         label("playlist").focusable().bind("d", "Remove", |_| {}),
    ///     ])
    ///     .on_key("?", |cx| cx.help_with(HelpOptions::new().all_panes(true)))
    /// });
    /// let mut driver = app.driver(60, 12);
    /// driver.press("?");
    /// let help = driver.screen().plain().join("\n");
    /// assert!(help.contains("Add to playlist") && help.contains("Remove"));
    /// ```
    pub fn all_panes(mut self, on: bool) -> HelpOptions {
        self.all_panes = on;
        self
    }

    /// Move with the letters until `key` is pressed, and search only after
    /// it, as Yazi's help does with `/`: `j` and `k` move a row, `g` and
    /// `G` go to the top and the bottom, and Escape ends the search
    /// (clearing it) before it closes the help. Without it, typing
    /// searches at once.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::HelpOptions;
    ///
    /// let app = App::new(|| {
    ///     label("files")
    ///         .bind("y", "Yank", |_| {})
    ///         .bind("p", "Paste", |_| {})
    ///         .on_key("?", |cx| cx.help_with(HelpOptions::new().filter_first(true).filter_key("/")))
    /// });
    /// let mut driver = app.driver(40, 10);
    /// // `p` moves (nothing to search for yet); after `/`, it searches.
    /// driver.press("? p");
    /// assert!(driver.screen().plain().join("\n").contains("Yank"));
    /// driver.press("/ p");
    /// let help = driver.screen().plain().join("\n");
    /// assert!(help.contains("Paste") && !help.contains("Yank"));
    /// ```
    ///
    /// # Panics
    /// On a name that is not a key.
    pub fn filter_key(mut self, key: &str) -> HelpOptions {
        let parsed = Key::parse(key).unwrap_or_else(|| panic!("filter_key: {key:?} is not a key"));
        self.filter_key = Some(parsed);
        self
    }
}

/// A binding as a [`key_hints`](crate::widgets::key_hints) footer shows it:
/// its keys, what it does, and whether it applies now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hint {
    pub keys: Vec<Key>,
    pub description: String,
    /// Its [`bind_when`](crate::Node::bind_when) condition does not hold.
    pub disabled: bool,
}

/// The text of a hint: what a hint's keys and description become, given
/// as (keys, description); console markup is not read in it.
type HintFormat = std::rc::Rc<dyn Fn(&str, &str) -> String>;

/// How [`key_hints_with`](crate::widgets::key_hints_with) shows the keys
/// that work now: each hint's text, how keys are said, whether bindings
/// that do not apply now are shown greyed, the separator, and a "more"
/// that expands the footer onto as many rows as it needs.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{key_hints_with, KeyHintsOptions};
///
/// let app = App::new(|| {
///     let staged = signal(false);
///     column([
///         label("files")
///             .bind("enter", "Stage", move |_| staged.set(true))
///             .bind_when(move || staged.get(), "D", "Reset", |_| {})
///             .autofocus(),
///         key_hints_with(
///             KeyHintsOptions::new()
///                 .format(|key, desc| format!("{desc} [{key}]"))
///                 .key_symbols(true)
///                 .show_disabled(true)
///                 .separator(" "),
///         ),
///     ])
/// });
/// let mut driver = app.driver(30, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[1].trim_end(), "Stage [⏎] Reset [D]");
/// ```
#[derive(Clone)]
pub struct KeyHintsOptions {
    pub(crate) format: Option<HintFormat>,
    pub(crate) key_format: KeyFormat,
    pub(crate) symbols: bool,
    pub(crate) disabled: bool,
    pub(crate) separator: Option<String>,
    pub(crate) more: Option<(crate::reactive::Signal<bool>, String)>,
}

impl std::fmt::Debug for KeyHintsOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyHintsOptions")
            .field("format", &self.format.is_some())
            .field("key_format", &self.key_format)
            .field("symbols", &self.symbols)
            .field("disabled", &self.disabled)
            .field("separator", &self.separator)
            .field("more", &self.more.as_ref().map(|(_, label)| label))
            .finish()
    }
}

impl Default for KeyHintsOptions {
    fn default() -> KeyHintsOptions {
        KeyHintsOptions::new()
    }
}

impl KeyHintsOptions {
    /// As [`key_hints`](crate::widgets::key_hints) shows them: `key
    /// description`, `·` apart, only what applies now, on one row.
    pub fn new() -> KeyHintsOptions {
        KeyHintsOptions {
            format: None,
            key_format: KeyFormat::Default,
            symbols: false,
            disabled: false,
            separator: None,
            more: None,
        }
    }

    /// Each hint's text from its keys and its description (`Stage [⏎]`,
    /// `<q> quit`), drawn whole in the theme's `key_hints.hint` style (the
    /// default look draws the key in `key_hints.key` and the description in
    /// `key_hints.desc`). Plain text: brackets show as they are.
    pub fn format(mut self, format: impl Fn(&str, &str) -> String + 'static) -> KeyHintsOptions {
        self.format = Some(std::rc::Rc::new(format));
        self
    }

    /// How keys are said, as in the help ([`KeyFormat::Vim`]: `<C-u>`).
    pub fn key_format(mut self, format: KeyFormat) -> KeyHintsOptions {
        self.key_format = format;
        self
    }

    /// Say keys with symbols, as gitui's and many footers do: `⏎` for
    /// Enter, `⇥` Tab, `⌫` Backspace, `⌦` Delete, `↑ ↓ ← →`, `⇞ ⇟` for
    /// the pages, `␣` Space, `^x` for Ctrl, `⌥x` for Alt and `⇧x` for
    /// Shift with a key that is not a letter.
    pub fn key_symbols(mut self, on: bool) -> KeyHintsOptions {
        self.symbols = on;
        self
    }

    /// Show the bindings whose [`bind_when`](crate::Node::bind_when)
    /// condition does not hold now too, in the theme's `key_hints.disabled`
    /// style (`bright_black` by default), rather than leaving them out:
    /// the commands are always where they are, lit when they can run.
    pub fn show_disabled(mut self, on: bool) -> KeyHintsOptions {
        self.disabled = on;
        self
    }

    /// What goes between hints (`" · "` by default), in the theme's
    /// `key_hints.separator` style.
    pub fn separator(mut self, separator: impl Into<String>) -> KeyHintsOptions {
        self.separator = Some(separator.into());
        self
    }

    /// While `expanded` is false and the hints do not fit, end the row with
    /// `label` (`more [.]`) in place of those that do not; while it is
    /// true, wrap the hints onto as many rows as they need, the footer
    /// growing to hold them. The app toggles `expanded` with a key of its
    /// own (gitui's `.`).
    pub fn more(
        mut self,
        expanded: crate::reactive::Signal<bool>,
        label: impl Into<String>,
    ) -> KeyHintsOptions {
        self.more = Some((expanded, label.into()));
        self
    }

    /// `keys` said as the options say.
    pub(crate) fn say(&self, keys: &[Key]) -> String {
        if !self.symbols {
            return self.key_format.keys(keys, false);
        }
        keys.iter()
            .map(|key| symbol_key(*key))
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// One key with symbols ([`KeyHintsOptions::key_symbols`]).
fn symbol_key(key: Key) -> String {
    let base = match key.code {
        KeyCode::Enter => "⏎".to_string(),
        KeyCode::Tab => "⇥".to_string(),
        KeyCode::BackTab => "⇤".to_string(),
        KeyCode::Backspace => "⌫".to_string(),
        KeyCode::Delete => "⌦".to_string(),
        KeyCode::Escape => "Esc".to_string(),
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        KeyCode::PageUp => "⇞".to_string(),
        KeyCode::PageDown => "⇟".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::Insert => "Ins".to_string(),
        KeyCode::Menu => "Menu".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Char(' ') => "␣".to_string(),
        KeyCode::Char(c) => c.to_string(),
    };
    let mut held = String::new();
    if key.modifiers.ctrl {
        held.push('^');
    }
    if key.modifiers.alt {
        held.push('⌥');
    }
    // A capital letter says its Shift itself.
    let letter = matches!(key.code, KeyCode::Char(c) if c.is_alphabetic());
    if key.modifiers.shift && !letter && key.code != KeyCode::BackTab {
        held.push('⇧');
    }
    held + &base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vim(name: &str) -> String {
        vim_key(Key::parse(name).unwrap())
    }

    #[test]
    fn symbols_say_keys_short() {
        let say = |name: &str| symbol_key(Key::parse(name).unwrap());
        assert_eq!(say("enter"), "⏎");
        assert_eq!(say("ctrl+u"), "^u");
        assert_eq!(say("shift+up"), "⇧↑");
        assert_eq!(say("D"), "D");
        assert_eq!(say("space"), "␣");
        assert_eq!(say("alt+x"), "⌥x");
    }

    #[test]
    fn vim_says_keys_in_angle_brackets() {
        assert_eq!(vim("ctrl+u"), "<C-u>");
        assert_eq!(vim("alt+x"), "<A-x>");
        assert_eq!(vim("shift+tab"), "<S-Tab>");
        assert_eq!(vim("ctrl+pagedown"), "<C-PageDown>");
        assert_eq!(vim("G"), "G");
        assert_eq!(vim("equals"), "=");
        assert_eq!(vim("lt"), "<lt>");
        assert_eq!(vim("space"), "<Space>");
        assert_eq!(vim("f1"), "<F1>");
        assert_eq!(vim("esc"), "<Esc>");
    }
}
