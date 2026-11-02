//! User keymaps: a binding with an id ([`Node::bind_id`],
//! [`Node::binding_id`], [`Node::sequence_id`]) is remapped by that id
//! from [`App::keymap`], a keys file ([`App::keymap_file`]) or
//! [`Ctx::set_keymap`], as Textual's `App.set_keymap` remaps a `Binding`
//! with an `id`. The help, the palette, a `key_hints` footer and the
//! which-key pop-up show the keys the map gives. An editor's actions are
//! in the same map, by `text_area.<action>` (`text_area.undo`).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use rich_interact::Key;
use rich_interact::keymap::{Overrides, try_keys};

use super::{App, Ctx, Driver, Duration, Note, ThemeFile, with_node};
use crate::node::{KeyBinding, Node};
use crate::reactive::NodeId;

/// Where an app's keymap comes from, and what it is now.
#[derive(Default)]
pub(super) struct KeymapState {
    /// Set in code ([`App::keymap`], [`Ctx::set_keymap`]).
    code: Vec<(String, Vec<Key>)>,
    /// The keys file, and what its last good read set (over `code`).
    file: Option<ThemeFile>,
    from_file: Vec<(String, Vec<Key>)>,
    /// `code` with `from_file` over it.
    map: Rc<HashMap<String, Vec<Key>>>,
    /// Bumped on every change, so each binding is remapped once per map.
    generation: u64,
    /// Report unknown ids and conflicts at the next frame.
    check: bool,
    /// What the last check found.
    report: Vec<String>,
}

/// A binding as a check sees it: what to call it, its keys under the map,
/// and whether the map remaps it.
type Listed = (String, Vec<Key>, bool);

/// What a handler asked of the keymap.
pub(super) enum Request {
    Set(Vec<(String, Vec<Key>)>),
    Reload,
}

/// `(id, keys)` pairs from `(id, key names)`: `ctrl+s f2`, or `none` (or
/// nothing) to unbind.
fn parse_pairs<I, A, K>(pairs: I) -> Result<Vec<(String, Vec<Key>)>, String>
where
    I: IntoIterator<Item = (A, K)>,
    A: AsRef<str>,
    K: AsRef<str>,
{
    pairs
        .into_iter()
        .map(|(id, names)| {
            let id = id.as_ref().trim();
            if id.is_empty() {
                return Err("a keymap entry has no id".to_string());
            }
            let names = names.as_ref().trim();
            let names = if names.eq_ignore_ascii_case("none") {
                ""
            } else {
                names
            };
            let keys = try_keys(names).map_err(|name| format!("{id}: unknown key {name:?}"))?;
            Ok((id.to_string(), keys))
        })
        .collect()
}

/// A keys file: one `id = keys` a line, the keys separated by spaces or
/// commas, `none` to unbind; `#` starts a comment, and a `[section]` line
/// is skipped, so the file may be TOML (`editor.save = "ctrl+s f2"`).
pub(crate) fn parse_keys_file(text: &str) -> Result<Vec<(String, Vec<Key>)>, String> {
    let mut pairs = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = strip_comment(line).trim();
        if line.is_empty() || (line.starts_with('[') && line.ends_with(']')) {
            continue;
        }
        let Some((id, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected id = keys", number + 1));
        };
        let (id, mut value) = (id.trim().trim_matches('"'), value.trim());
        // TOML's quoted string of several keys: the keys inside it. A
        // single quoted key (`"#"`) stays quoted, as rs-rich-interact
        // reads it.
        if value.len() >= 2
            && value.starts_with('"')
            && value.ends_with('"')
            && value[1..value.len() - 1].contains([' ', ','])
        {
            value = &value[1..value.len() - 1];
        }
        if value.is_empty() {
            return Err(format!(
                "line {}: {id} has no keys (`none` unbinds)",
                number + 1
            ));
        }
        pairs.push((id.to_string(), value.to_string()));
    }
    parse_pairs(pairs).map_err(|e| format!("keys file: {e}"))
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

/// The keys of the editing actions an id can name: `text_area.undo`.
fn editing_ids() -> HashSet<String> {
    let mut ids = HashSet::new();
    for keymap in [
        rich_interact::edit_keys::editor_keymap(),
        rich_interact::edit_keys::emacs_keymap(),
    ] {
        for binding in keymap.declared() {
            ids.insert(binding.id());
        }
    }
    ids
}

impl KeyBinding {
    /// Take the keys `map` gives this binding's id (or its own again), once
    /// per keymap `generation`.
    fn remap(&mut self, map: &HashMap<String, Vec<Key>>, generation: u64) {
        let Some(id) = &self.id else {
            return;
        };
        if self.keymap_generation == generation {
            return;
        }
        self.keymap_generation = generation;
        let (keys, shown) = self
            .declared
            .get_or_insert_with(|| (self.keys.clone(), self.shown.clone()))
            .clone();
        match map.get(&**id) {
            Some(mapped) => {
                self.keys = mapped.clone();
                self.shown = None;
            }
            None => {
                self.keys = keys;
                self.shown = shown;
            }
        }
    }

    /// Its keys under `map`, without changing it.
    fn keys_under(&self, map: &HashMap<String, Vec<Key>>) -> Vec<Key> {
        if let Some(keys) = self.id.as_ref().and_then(|id| map.get(&**id)) {
            return keys.clone();
        }
        match &self.declared {
            Some((keys, _)) => keys.clone(),
            None => self.keys.clone(),
        }
    }

    /// What a report calls it: its id, else its description.
    fn label(&self) -> String {
        match &self.id {
            Some(id) => id.to_string(),
            None if !self.description.is_empty() => format!("“{}”", self.description),
            None => "a binding".to_string(),
        }
    }
}

/// Remap the bindings of `node`, and its editor's actions.
fn remap_node(node: &Node, state: &KeymapState, editing: Option<&Overrides>) {
    let generation = state.generation;
    for binding in node.keys.borrow_mut().iter_mut() {
        binding.remap(&state.map, generation);
    }
    for binding in node.sequences.borrow_mut().iter_mut() {
        binding.remap(&state.map, generation);
    }
    if let Some(overrides) = editing {
        crate::text_edit::apply_app_keymap(node, overrides, generation);
    }
}

/// The editing actions of `map` as rs-rich-interact's overrides.
fn editing_overrides(map: &HashMap<String, Vec<Key>>) -> Overrides {
    let mut overrides = Overrides::new();
    for (id, keys) in map {
        if let Some(action) = id.strip_prefix("text_area.") {
            overrides.set("text_area", action, keys.iter().copied());
        }
    }
    overrides
}

impl App {
    /// Remap the bindings of user keys for an app: `(id, keys)` pairs,
    /// each id one a binding was given ([`Node::bind_id`],
    /// [`Node::binding_id`], [`Node::sequence_id`]) or an editing action's
    /// (`text_area.undo`, `text_area.word-left`), the keys as
    /// [`on_key`](Node::on_key) names them, separated by spaces (`"ctrl+s
    /// f2"`), or `"none"` to unbind. A sequence's keys are the sequence
    /// (`"g g"`). The help, the palette, `key_hints` and the which-key
    /// pop-up show the new keys. An id no binding has is reported once in
    /// a warning toast, and so is a key that two ids now share on one path
    /// of the tree (the deeper node's binding takes it, as without a map).
    /// A key name that does not parse is reported in a toast, and the map
    /// is not used.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let saved = signal(0);
    ///     text!("saved {saved}")
    ///         .bind_id("editor.save", "ctrl+s", "Save", move |_| saved.update(|n| *n += 1))
    ///         .on_key("?", |cx| cx.help())
    /// })
    /// .keymap([("editor.save", "f2")]);
    /// let mut driver = app.driver(40, 12);
    /// driver.press("ctrl+s");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "saved 0");
    /// driver.press("f2");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "saved 1");
    /// driver.press("?");
    /// let help = driver.screen().plain().join("\n");
    /// assert!(help.contains("f2  Save"), "{help}");
    /// ```
    #[must_use]
    pub fn keymap<I, A, K>(mut self, pairs: I) -> App
    where
        I: IntoIterator<Item = (A, K)>,
        A: AsRef<str>,
        K: AsRef<str>,
    {
        match parse_pairs(pairs) {
            Ok(pairs) => {
                self.keymap.code = pairs;
                self.rekey();
            }
            Err(error) => self.keymap_error(&error),
        }
        self
    }

    /// [`keymap`](Self::keymap) from a keys file, over the one in code,
    /// read again whenever it changes while the app runs (and on
    /// [`Ctx::reload_keymap`]): one `id = keys` a line, `#` comments,
    /// `none` to unbind. `[section]` lines are skipped and a quoted list
    /// of keys is read inside its quotes, so a TOML file of string values
    /// works too. A missing file is no keymap; one that does not parse is
    /// reported in a toast, and the last good map stays.
    ///
    /// ```text
    /// # keys.toml
    /// editor.save = "ctrl+s f2"
    /// app.quit = "ctrl+q"
    /// text_area.undo = "ctrl+z"
    /// ```
    #[must_use]
    pub fn keymap_file(mut self, path: impl Into<std::path::PathBuf>) -> App {
        self.keymap.file = Some(ThemeFile {
            path: path.into(),
            stamp: None,
            good: None,
            read: None,
            error: None,
        });
        self.reload_keymap_file(false);
        self
    }

    /// The map changed: every binding takes its keys again, and the next
    /// frame checks it.
    fn rekey(&mut self) {
        let mut map: HashMap<String, Vec<Key>> = HashMap::new();
        for (id, keys) in self.keymap.code.iter().chain(&self.keymap.from_file) {
            map.insert(id.clone(), keys.clone());
        }
        self.keymap.map = Rc::new(map);
        self.keymap.generation += 1;
        self.keymap.check = true;
        self.poke = true;
    }

    fn keymap_error(&mut self, error: &str) {
        let markup = format!("[bad]keymap:[/] {}", rich::markup::escape(error));
        self.push_toast(Note::plain(markup), self.now + Duration::from_secs(6));
    }

    /// Read the keys file again if it changed since it was last read, or
    /// at once with `force`.
    pub(super) fn reload_keymap_file(&mut self, force: bool) {
        let Some(file) = &mut self.keymap.file else {
            return;
        };
        let stamp = std::fs::metadata(&file.path)
            .ok()
            .map(|m| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len()));
        if !force && stamp == file.stamp {
            // A file system with coarse timestamps can hide a same-length
            // edit made within one tick: while the file is that fresh, look
            // at what it says (as the theme file does).
            let fresh = stamp.is_some_and(|(modified, _)| {
                modified
                    .elapsed()
                    .map_or(true, |age| age < Duration::from_secs(2))
            });
            if !fresh {
                return;
            }
        }
        let read = std::fs::read_to_string(&file.path).ok();
        if !force && stamp == file.stamp && read == file.read {
            return;
        }
        file.stamp = stamp;
        file.read = read.clone();
        let parsed = match &read {
            Some(text) => parse_keys_file(text),
            None => Ok(Vec::new()),
        };
        match parsed {
            Ok(pairs) => {
                file.good = read;
                file.error = None;
                self.keymap.from_file = pairs;
                self.rekey();
            }
            Err(error) => {
                let repeated = file.error.as_ref() == Some(&error);
                file.error = Some(error.clone());
                if !repeated {
                    self.keymap_error(&error);
                }
            }
        }
    }

    /// What a handler asked of the keymap.
    pub(super) fn keymap_request(&mut self, request: Request) {
        match request {
            Request::Set(pairs) => {
                self.keymap.code = pairs;
                self.rekey();
            }
            Request::Reload => self.reload_keymap_file(true),
        }
    }

    /// Give the bindings of the nodes `ids` the keymap's keys, before a
    /// key goes through them or they are listed.
    pub(super) fn remap_nodes(&self, ids: &[NodeId]) {
        if self.keymap.generation == 0 {
            return;
        }
        let editing = editing_overrides(&self.keymap.map);
        for &id in ids {
            with_node(&self.top().root, id, &mut |node| {
                remap_node(node, &self.keymap, Some(&editing))
            });
        }
    }

    /// After a change of map, at a frame: remap every node, and report the
    /// ids no binding has and the keys two ids now share.
    pub(super) fn check_keymap(&mut self) {
        if !self.keymap.check {
            return;
        }
        self.keymap.check = false;
        let map = self.keymap.map.clone();
        let editing = editing_overrides(&map);
        let mut seen: HashSet<String> = HashSet::new();
        let mut report: Vec<String> = Vec::new();
        for layer in &self.layers {
            // The bindings of the nodes above, while the walk is in them.
            let mut above: Vec<(usize, Vec<Listed>)> = Vec::new();
            layer.root.walk(&mut |node, path| {
                remap_node(node, &self.keymap, Some(&editing));
                let depth = path.len();
                above.retain(|(d, _)| *d < depth);
                let mut own: Vec<Listed> = Vec::new();
                for binding in node.keys.borrow().iter() {
                    if let Some(id) = &binding.id {
                        seen.insert(id.to_string());
                    }
                    if binding.when.is_some() {
                        // Conditional bindings may share a key by design.
                        continue;
                    }
                    let mapped = binding
                        .id
                        .as_ref()
                        .is_some_and(|id| map.contains_key(&**id));
                    let keys = binding.keys_under(&map);
                    let label = binding.label();
                    let mut clash =
                        |other: &str, other_keys: &[Key], other_mapped: bool, winner: &str| {
                            if !(mapped || other_mapped) {
                                return;
                            }
                            if let Some(key) = keys.iter().find(|k| other_keys.contains(k)) {
                                report.push(format!(
                                    "{} is both {other} and {label}; {winner} wins",
                                    rich_interact::chrome::key_hint(*key),
                                ));
                            }
                        };
                    // On one node the first binding takes the key; between
                    // nodes, the deeper one.
                    for (other, other_keys, other_mapped) in &own {
                        clash(other, other_keys, *other_mapped, other);
                    }
                    for (other, other_keys, other_mapped) in above.iter().flat_map(|(_, b)| b) {
                        clash(other, other_keys, *other_mapped, &label);
                    }
                    own.push((label, keys, mapped));
                }
                for binding in node.sequences.borrow().iter() {
                    if let Some(id) = &binding.id {
                        seen.insert(id.to_string());
                    }
                }
                above.push((depth, own));
            });
        }
        let editing_ids = editing_ids();
        let mut unknown: Vec<&String> = map
            .keys()
            .filter(|id| !seen.contains(*id) && !editing_ids.contains(*id))
            .collect();
        unknown.sort();
        for id in unknown {
            report.push(format!("no binding has the id {id}"));
        }
        if !report.is_empty() {
            let body = report
                .iter()
                .map(|line| rich::markup::escape(line))
                .collect::<Vec<_>>()
                .join("\n");
            let markup = format!("[warn]keymap:[/] {body}");
            self.push_toast(Note::plain(markup), self.now + Duration::from_secs(6));
        }
        self.keymap.report = report;
    }
}

impl Ctx {
    /// Remap bindings by id at run time, as [`App::keymap`] does, in place
    /// of the map set in code (a keys file's still goes over it). A key
    /// name that does not parse is an error, and nothing changes.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let n = signal(0);
    ///     text!("{n}")
    ///         .bind_id("count.up", "+", "Count up", move |_| n.update(|n| *n += 1))
    ///         .on_key("m", |cx| cx.set_keymap([("count.up", "u")]).unwrap())
    /// });
    /// let mut driver = app.driver(4, 1);
    /// driver.press("+ m + u");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "2");
    /// ```
    pub fn set_keymap<I, A, K>(&mut self, pairs: I) -> Result<(), String>
    where
        I: IntoIterator<Item = (A, K)>,
        A: AsRef<str>,
        K: AsRef<str>,
    {
        self.keymap = Some(Request::Set(parse_pairs(pairs)?));
        Ok(())
    }

    /// Read the [keys file](App::keymap_file) again now, whether or not it
    /// looks changed.
    pub fn reload_keymap(&mut self) {
        self.keymap = Some(Request::Reload);
    }
}

impl Driver {
    /// What the keymap's last check reported: ids no binding has, and keys
    /// two ids share (each also shown in a toast).
    pub fn keymap_report(&self) -> &[String] {
        &self.app.keymap.report
    }
}

impl Node {
    /// [`bind`](Self::bind), with the id `id` a keymap remaps it by
    /// ([`App::keymap`], [`App::keymap_file`]): `editor.save`, `app.quit`.
    ///
    /// # Panics
    /// As [`bind`](Self::bind).
    pub fn bind_id(
        self,
        id: &str,
        keys: &str,
        description: &str,
        handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        self.bind(keys, description, handler).binding_id(id)
    }

    /// Give the key binding added last (by [`bind`](Self::bind),
    /// [`bind_in`](Self::bind_in), [`bind_scaled`](Self::bind_scaled),
    /// [`bind_when`](Self::bind_when), …) the id `id`, for a keymap. A
    /// remapped [`bind_scaled`](Self::bind_scaled) takes exactly the keys
    /// the map gives.
    pub fn binding_id(self, id: &str) -> Node {
        let id: Rc<str> = Rc::from(id);
        match self.keys.borrow_mut().last_mut() {
            Some(binding) => binding.id = Some(id),
            None if cfg!(debug_assertions) => {
                eprintln!("intuituive: binding_id() on a node with no key binding does nothing")
            }
            None => {}
        }
        self
    }

    /// Give the [sequence](Self::bind_sequence) added last the id `id`,
    /// for a keymap: the map's keys are the new sequence (`"space f"`).
    pub fn sequence_id(self, id: &str) -> Node {
        let id: Rc<str> = Rc::from(id);
        match self.sequences.borrow_mut().last_mut() {
            Some(binding) => binding.id = Some(id),
            None if cfg!(debug_assertions) => {
                eprintln!("intuituive: sequence_id() on a node with no sequence does nothing")
            }
            None => {}
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keys_file_may_be_toml() {
        let pairs = parse_keys_file(
            "[keys]\neditor.save = \"ctrl+s f2\" # both\napp.quit = ctrl+q\nhash = \"#\"\nx.y = none\n",
        )
        .unwrap();
        assert_eq!(pairs[0].0, "editor.save");
        assert_eq!(pairs[0].1, [Key::ctrl('s'), Key::parse("f2").unwrap()]);
        assert_eq!(pairs[1].1, [Key::ctrl('q')]);
        assert_eq!(pairs[2].1, [Key::char('#')]);
        assert!(pairs[3].1.is_empty());
        assert!(parse_keys_file("editor.save =").is_err());
        assert!(parse_keys_file("editor.save = ctrl+nope").is_err());
    }
}
