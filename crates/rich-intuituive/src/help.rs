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

#[cfg(test)]
mod tests {
    use super::*;

    fn vim(name: &str) -> String {
        vim_key(Key::parse(name).unwrap())
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
