//! The keymap registry: what each key does, declared rather than matched
//! inline, so it can be rebound and listed (0.0.14 workstream 1).
//!
//! A component declares its [`Binding`]s in a [`Keymap`]: an action name
//! (`down`), the keys that do it (`down`, `ctrl+n`), a description for
//! people (`move down`) and a context (`select`) that tells one
//! component's `down` from another's. It then asks the keymap what a key
//! means ([`Keymap::action`]) instead of matching the key itself.
//!
//! Keys are rebound at two levels, the first that has the action winning:
//!
//! 1. on one keymap, with [`Keymap::rebind`];
//! 2. for the whole process, with [`install`]ed [`Overrides`]: the hook a
//!    configuration file (the CLI's, say) uses, one `context.action = keys`
//!    per line.
//!
//! [`Component::keymap`](crate::Component::keymap) returns the bindings a
//! component has right now; a container's includes its focused child's.
//! That is the query a help overlay, a shortcut list or a status bar's
//! hints read, so a component you write gets them by declaring its keys.
//!
//! ```
//! use rich_interact::keymap::{keys, Keymap};
//! use rich_interact::Key;
//!
//! let mut keymap = Keymap::new("counter")
//!     .bind("up", keys("up k"), "count up")
//!     .bind("done", keys("enter"), "finish");
//! assert_eq!(keymap.action(Key::char('k')), Some("up"));
//! keymap.rebind("up", keys("+"));
//! assert_eq!(keymap.action(Key::char('k')), None);
//! assert_eq!(keymap.action(Key::char('+')), Some("up"));
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;

use crate::event::Key;

/// Keys from their names separated by spaces (see [`Key::parse`]).
///
/// # Panics
/// On a name that does not parse: bindings are written in code. Use
/// [`try_keys`] for text from a user.
pub fn keys(names: &str) -> Vec<Key> {
    try_keys(names).unwrap_or_else(|name| panic!("unknown key {name:?}"))
}

/// Keys from names separated by spaces or commas, or the first name that
/// does not parse. A name in quotes (`"#"`, `','`, `"ctrl+,"`) is taken as
/// it is, which is how to write the comma key, or the hash key in a file
/// where `#` starts a comment. Text that holds only separators (a bare `,`)
/// is an error, not "no keys"; text with nothing in it is no keys.
///
/// ```
/// use rich_interact::keymap::try_keys;
/// use rich_interact::Key;
///
/// assert_eq!(try_keys("\",\" ctrl+n").unwrap(), [Key::char(','), Key::ctrl('n')]);
/// assert_eq!(try_keys("").unwrap(), []);
/// assert!(try_keys(",").is_err());
/// ```
pub fn try_keys(names: &str) -> Result<Vec<Key>, String> {
    let names = key_names(names)?;
    names
        .iter()
        .map(|name| Key::parse(name).ok_or_else(|| name.clone()))
        .collect()
}

/// The key names in `text`: separated by whitespace or commas, or quoted.
/// An unclosed quote, or separators with no name, is an error (the text).
fn key_names(text: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut name = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' if name.is_empty() => {
                let mut quoted = String::new();
                let mut closed = false;
                for d in chars.by_ref() {
                    if d == c {
                        closed = true;
                        break;
                    }
                    quoted.push(d);
                }
                if !closed || quoted.is_empty() {
                    return Err(text.trim().to_string());
                }
                names.push(quoted);
            }
            c if c.is_whitespace() || c == ',' => {
                if !name.is_empty() {
                    names.push(std::mem::take(&mut name));
                }
            }
            c => name.push(c),
        }
    }
    if !name.is_empty() {
        names.push(name);
    }
    if names.is_empty() && !text.trim().is_empty() {
        return Err(text.trim().to_string());
    }
    Ok(names)
}

/// One thing a key does: an action, in a context, with a description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Which component or container it belongs to: `select`, `split`.
    pub context: String,
    /// What it does, for code and configuration: `down`, `focus-next`.
    pub action: String,
    /// The keys that do it, first the one to show.
    pub keys: Vec<Key>,
    /// What it does, for people: `move down`.
    pub description: String,
}

impl Binding {
    pub fn new(
        context: impl Into<String>,
        action: impl Into<String>,
        keys: impl IntoIterator<Item = Key>,
        description: impl Into<String>,
    ) -> Binding {
        Binding {
            context: context.into(),
            action: action.into(),
            keys: keys.into_iter().collect(),
            description: description.into(),
        }
    }

    /// `context.action`: how configuration names it.
    pub fn id(&self) -> String {
        format!("{}.{}", self.context, self.action)
    }

    /// The keys as people read them: `up/ctrl+p`.
    pub fn keys_label(&self) -> String {
        self.keys
            .iter()
            .map(Key::to_string)
            .collect::<Vec<_>>()
            .join("/")
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}  {}", self.keys_label(), self.description)
    }
}

/// A component's bindings, and the keys rebound on it.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    context: String,
    bindings: Vec<Binding>,
    /// Actions rebound on this keymap, by `context.action`.
    rebound: HashMap<String, Vec<Key>>,
}

impl Keymap {
    /// An empty keymap whose bindings are in `context`.
    pub fn new(context: impl Into<String>) -> Keymap {
        Keymap {
            context: context.into(),
            ..Keymap::default()
        }
    }

    /// The context [`bind`](Self::bind) puts bindings in.
    pub fn context(&self) -> &str {
        &self.context
    }

    /// Declare that `keys` do `action`, described as `description`.
    pub fn bind(
        mut self,
        action: impl Into<String>,
        keys: impl IntoIterator<Item = Key>,
        description: impl Into<String>,
    ) -> Keymap {
        let binding = Binding::new(self.context.clone(), action, keys, description);
        self.add(binding);
        self
    }

    /// Add a binding, in whatever context it has. A binding for an action
    /// already declared in that context replaces it.
    pub fn add(&mut self, binding: Binding) {
        match self
            .bindings
            .iter_mut()
            .find(|b| b.context == binding.context && b.action == binding.action)
        {
            Some(existing) => *existing = binding,
            None => self.bindings.push(binding),
        }
    }

    /// Add every binding of `other` after this one's, keeping what `other`
    /// rebound: how a container lists its focused child's keys with its
    /// own.
    pub fn extend(&mut self, other: Keymap) {
        for (id, keys) in other.rebound {
            self.rebound.entry(id).or_insert(keys);
        }
        for binding in other.bindings {
            if !self
                .bindings
                .iter()
                .any(|b| b.context == binding.context && b.action == binding.action)
            {
                self.bindings.push(binding);
            }
        }
    }

    /// Make `keys` do `action` (in this keymap's context) instead of what
    /// was declared or installed. No keys unbinds it.
    pub fn rebind(&mut self, action: &str, keys: impl IntoIterator<Item = Key>) {
        let id = format!("{}.{action}", self.context);
        self.rebound.insert(id, keys.into_iter().collect());
    }

    /// Builder form of [`rebind`](Self::rebind).
    pub fn rebound(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Keymap {
        self.rebind(action, keys);
        self
    }

    /// Apply `overrides` on this keymap only, over anything installed.
    pub fn apply(&mut self, overrides: &Overrides) {
        for (id, keys) in &overrides.keys {
            self.rebound.insert(id.clone(), keys.clone());
        }
    }

    /// The keys set for `binding`'s action, and where they come from:
    /// 2 rebound on this keymap, 1 installed, 0 declared.
    fn source(&self, binding: &Binding) -> (u8, Vec<Key>) {
        let id = binding.id();
        if let Some(keys) = self.rebound.get(&id) {
            return (2, keys.clone());
        }
        if let Some(keys) = installed_keys(&id) {
            return (1, keys);
        }
        (0, binding.keys.clone())
    }

    /// The keys that do `binding`'s action now: what was set for it, less
    /// any key a closer level gave another action in the same context (a
    /// key rebound onto `down` is no longer the declared `next`'s).
    fn effective(&self, binding: &Binding) -> Vec<Key> {
        let (level, mut keys) = self.source(binding);
        if level < 2 {
            for other in &self.bindings {
                if other.context != binding.context || other.action == binding.action {
                    continue;
                }
                let (other_level, taken) = self.source(other);
                if other_level > level {
                    keys.retain(|key| !taken.contains(key));
                }
            }
        }
        keys
    }

    /// The index of the binding `key` triggers: one whose keys were rebound
    /// or installed before one declared, then the first. A binding that
    /// names the key wins over one it only [`matches`](Key::matches), so a
    /// Tab from a legacy terminal does `tab`'s action before `ctrl+i`'s.
    fn find(&self, key: Key) -> Option<usize> {
        self.find_by(|keys| keys.contains(&key))
            .or_else(|| self.find_by(|keys| key.matches_any(keys)))
    }

    fn find_by(&self, fires: impl Fn(&[Key]) -> bool) -> Option<usize> {
        let mut found: Option<(u8, usize)> = None;
        for (index, binding) in self.bindings.iter().enumerate() {
            let (level, _) = self.source(binding);
            if found.is_some_and(|(best, _)| best >= level) {
                continue;
            }
            if fires(&self.effective(binding)) {
                found = Some((level, index));
            }
        }
        found.map(|(_, index)| index)
    }

    /// The binding `key` triggers, if any: see [`action`](Self::action).
    pub fn lookup(&self, key: Key) -> Option<Binding> {
        self.find(key).map(|index| {
            let binding = &self.bindings[index];
            Binding {
                keys: self.effective(binding),
                ..binding.clone()
            }
        })
    }

    /// The action `key` triggers, if any: a binding whose keys were rebound
    /// (or installed) wins over one with its declared keys, so a key moved
    /// onto an action does it even when an earlier action declared it;
    /// otherwise the first declared.
    pub fn action(&self, key: Key) -> Option<&str> {
        self.find(key)
            .map(|index| self.bindings[index].action.as_str())
    }

    /// Whether `key` triggers `action`.
    pub fn is(&self, key: Key, action: &str) -> bool {
        key.matches_any(&self.keys(action))
    }

    /// The keys that trigger `action` (in any context) now.
    pub fn keys(&self, action: &str) -> Vec<Key> {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .map(|binding| self.effective(binding))
            .unwrap_or_default()
    }

    /// The first key for `action`, to show in a hint.
    pub fn key(&self, action: &str) -> Option<Key> {
        self.keys(action).first().copied()
    }

    /// Every binding, with the keys that do it now.
    pub fn bindings(&self) -> Vec<Binding> {
        self.bindings
            .iter()
            .map(|binding| Binding {
                keys: self.effective(binding),
                ..binding.clone()
            })
            .collect()
    }

    /// Every binding as declared, before rebinding.
    pub fn declared(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }
}

/// Keys rebound by `context.action`: what a configuration file sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    keys: HashMap<String, Vec<Key>>,
}

impl Overrides {
    pub fn new() -> Overrides {
        Overrides::default()
    }

    /// Make `keys` do `context.action`. No keys unbinds it.
    pub fn set(
        &mut self,
        context: &str,
        action: &str,
        keys: impl IntoIterator<Item = Key>,
    ) -> &mut Self {
        self.keys
            .insert(format!("{context}.{action}"), keys.into_iter().collect());
        self
    }

    /// The keys set for `context.action`, if any.
    pub fn get(&self, id: &str) -> Option<&[Key]> {
        self.keys.get(id).map(Vec::as_slice)
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// `(context.action, keys)` pairs, as a configuration table holds them
    /// (`"select.down" = "ctrl+n, down"`).
    pub fn from_pairs<I, A, K>(pairs: I) -> Result<Overrides, String>
    where
        I: IntoIterator<Item = (A, K)>,
        A: AsRef<str>,
        K: AsRef<str>,
    {
        let mut overrides = Overrides::new();
        for (id, names) in pairs {
            let id = id.as_ref().trim();
            let Some((context, action)) = id.rsplit_once('.') else {
                return Err(format!("{id:?} is not context.action"));
            };
            if context.is_empty() || action.is_empty() {
                return Err(format!("{id:?} is not context.action"));
            }
            let keys = try_keys(names.as_ref()).map_err(|name| {
                if name.contains([',', '"', '\'']) {
                    format!("{id}: no key in {name:?} (quote a comma key: \",\")")
                } else {
                    format!("{id}: unknown key {name:?}")
                }
            })?;
            overrides.set(context, action, keys);
        }
        Ok(overrides)
    }

    /// One `context.action = key, key` per line; blank lines are skipped,
    /// and `#` at the start of a line or after a space starts a comment
    /// (`ctrl+#` is a key). Quote a key that is a separator or a comment:
    /// `"#"`, `","`. `none` unbinds the action; a line with no keys is an
    /// error, so a stray `#` or `,` never unbinds one by accident.
    ///
    /// ```
    /// use rich_interact::keymap::Overrides;
    /// use rich_interact::Key;
    ///
    /// let overrides = Overrides::parse("select.down = \"#\" j # keys\nselect.up = none").unwrap();
    /// assert_eq!(overrides.get("select.down").unwrap(), [Key::char('#'), Key::char('j')]);
    /// assert_eq!(overrides.get("select.up").unwrap(), []);
    /// assert!(Overrides::parse("select.down = #").is_err());
    /// ```
    pub fn parse(text: &str) -> Result<Overrides, String> {
        let mut pairs = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            let Some((id, names)) = line.split_once('=') else {
                return Err(format!(
                    "line {}: expected context.action = keys",
                    number + 1
                ));
            };
            let (id, names) = (id.trim(), names.trim());
            if names.is_empty() {
                return Err(format!(
                    "line {}: {id} has no keys (quote a \"#\" key; `none` unbinds)",
                    number + 1
                ));
            }
            let names = if names.eq_ignore_ascii_case("none") {
                ""
            } else {
                names
            };
            pairs.push((id.to_string(), names.to_string()));
        }
        Overrides::from_pairs(pairs)
    }
}

/// `line` without its comment: from a `#` at the start or after
/// whitespace, outside quotes.
fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut previous = ' ';
    for (index, c) in line.char_indices() {
        match quote {
            Some(open) if c == open => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' && previous.is_whitespace() => return &line[..index],
            None => {}
        }
        previous = c;
    }
    line
}

static INSTALLED: RwLock<Option<Overrides>> = RwLock::new(None);

/// Rebind keys for every keymap in the process, from now on: the hook for
/// a configuration file. Replaces what was installed before.
pub fn install(overrides: Overrides) {
    let mut installed = INSTALLED.write().unwrap_or_else(|e| e.into_inner());
    *installed = (!overrides.is_empty()).then_some(overrides);
}

/// What [`install`] set, if anything.
pub fn installed() -> Overrides {
    INSTALLED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

fn installed_keys(id: &str) -> Option<Vec<Key>> {
    let installed = INSTALLED.read().unwrap_or_else(|e| e.into_inner());
    installed.as_ref()?.get(id).map(<[Key]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_overrides() {
        let overrides = Overrides::parse(
            "# mine\nselect.down = ctrl+n, down\n\ntabs.next = alt+l # comment\nx.y = none\n",
        )
        .unwrap();
        assert_eq!(
            overrides.get("select.down").unwrap(),
            [Key::ctrl('n'), Key::new(crate::KeyCode::Down)]
        );
        assert_eq!(overrides.get("x.y").unwrap(), []);
        assert!(Overrides::parse("nodot = up").is_err());
        assert!(Overrides::parse("a.b = nokey").is_err());
        assert!(Overrides::parse("a.b up").is_err());
    }

    #[test]
    fn installed_overrides_apply_below_local_ones() {
        let keymap = Keymap::new("keymap-test-context").bind("go", keys("g"), "go");
        let mut overrides = Overrides::new();
        overrides.set("keymap-test-context", "go", keys("h"));
        install(overrides);
        assert_eq!(keymap.action(Key::char('h')), Some("go"));
        assert_eq!(keymap.action(Key::char('g')), None);
        let local = keymap.clone().rebound("go", keys("j"));
        assert_eq!(local.action(Key::char('j')), Some("go"));
        install(Overrides::new());
        assert_eq!(keymap.action(Key::char('g')), Some("go"));
    }

    #[test]
    fn extends_and_lists() {
        let mut outer = Keymap::new("outer").bind("quit", keys("q"), "quit");
        let inner = Keymap::new("inner")
            .bind("down", keys("down j"), "move down")
            .rebound("down", keys("n"));
        outer.extend(inner);
        let listed: Vec<String> = outer.bindings().iter().map(ToString::to_string).collect();
        assert_eq!(listed, ["q  quit", "n  move down"]);
        assert_eq!(outer.lookup(Key::char('n')).unwrap().id(), "inner.down");
    }

    #[test]
    fn a_legacy_key_does_the_action_of_any_key_it_could_be() {
        use crate::KeyCode;
        let keymap = Keymap::new("legacy-test")
            .bind("indent", keys("ctrl+i"), "indent")
            .bind("submit", keys("ctrl+m"), "submit");
        let tab = Key::new(KeyCode::Tab);
        assert_eq!(keymap.action(tab), Some("indent"));
        assert!(keymap.is(tab, "indent"));
        assert_eq!(keymap.action(Key::new(KeyCode::Enter)), Some("submit"));
        // With the kitty protocol, Tab is only Tab and Ctrl+I only Ctrl+I.
        assert_eq!(keymap.action(tab.exact()), None);
        assert!(!keymap.is(tab.exact(), "indent"));
        assert_eq!(keymap.action(Key::ctrl('i').exact()), Some("indent"));
        // A binding that names the key wins over one it could be.
        let keymap = keymap.bind("next", keys("tab"), "next field");
        assert_eq!(keymap.action(tab), Some("next"));
        assert_eq!(keymap.action(Key::ctrl('i')), Some("indent"));
    }

    #[test]
    fn a_rebound_key_moves_from_an_earlier_action() {
        let keymap = Keymap::new("move-test")
            .bind("next", keys("n"), "next match")
            .bind("down", keys("down j"), "scroll down")
            .rebound("down", keys("n"));
        assert_eq!(keymap.action(Key::char('n')), Some("down"));
        assert_eq!(keymap.lookup(Key::char('n')).unwrap().action, "down");
        // `next` lost the key it had: nothing else is listed against `n`.
        assert_eq!(keymap.keys("next"), []);
        assert!(!keymap.is(Key::char('n'), "next"));
        // A key no one rebound keeps its declared action.
        let keymap = keymap.rebound("next", keys("m"));
        assert_eq!(keymap.action(Key::char('m')), Some("next"));
    }
}
