//! The action registry: one list of what Noctiluca can do. The keys, the
//! link hints, site keys and the F1 overlay all run actions from it, and
//! each checks the action is available when it runs.
//!
//! Keys are not a system of Noctiluca's own. Each action is a binding of a
//! `rich_interact::keymap::Keymap` (context `noctiluca`), rebound from the
//! config's `[keys]` through that crate's `Overrides` (`Keymap::apply`),
//! and bound on the app with intuiTUIve's `Node::bind` and
//! `Node::bind_sequence`.

use rich_intuituive::interact::keymap::{try_keys, Binding, Keymap, Overrides};
use rich_intuituive::interact::Key;

use crate::config::Preset;

/// The keymap context of Noctiluca's actions: `noctiluca.hints`.
pub const CONTEXT: &str = "noctiluca";

/// An action Noctiluca can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    /// Its id, for config files and site keys: `hints`, `scroll.down`.
    pub id: &'static str,
    /// What it does, for people.
    pub label: &'static str,
    /// Its group in the overlay.
    pub section: &'static str,
    /// Its keys in the vim-like set and in the plain set; a space in a
    /// set's keys that `sequence` marks is a sequence (`g t`), else it
    /// separates alternatives.
    pub vim: &'static str,
    pub plain: &'static str,
    pub sequence: bool,
}

const fn action(
    id: &'static str,
    label: &'static str,
    section: &'static str,
    vim: &'static str,
    plain: &'static str,
) -> Action {
    Action {
        id,
        label,
        section,
        vim,
        plain,
        sequence: false,
    }
}

const fn sequence(
    id: &'static str,
    label: &'static str,
    section: &'static str,
    vim: &'static str,
    plain: &'static str,
) -> Action {
    Action {
        id,
        label,
        section,
        vim,
        plain,
        sequence: true,
    }
}

/// Every action, in the overlay's order.
pub const ACTIONS: &[Action] = &[
    action(
        "open",
        "Open an address, a keyword or a search",
        "go",
        "o",
        "ctrl+o",
    ),
    action("open_tab", "Open in a new tab", "go", "O", "ctrl+t"),
    action(
        "address",
        "Edit this page's address",
        "go",
        "ctrl+l",
        "ctrl+l f6",
    ),
    action(
        "hints",
        "Link hints: follow a link or use a control",
        "go",
        "f",
        "alt+h",
    ),
    action("back", "Back", "go", "H", "alt+left"),
    action("forward", "Forward", "go", "L", "alt+right"),
    action("reload", "Reload", "go", "R", "ctrl+r f5"),
    sequence("tab.next", "Next tab", "go", "g t", "ctrl+pagedown"),
    sequence("tab.previous", "Previous tab", "go", "g T", "ctrl+pageup"),
    action("find", "Find in page", "page", "/", "ctrl+f"),
    action("find.next", "Next match", "page", "n", "f3"),
    action("find.previous", "Previous match", "page", "N", "shift+f3"),
    action("scroll.down", "Scroll down", "page", "j down", "down"),
    action("scroll.up", "Scroll up", "page", "k up", "up"),
    action(
        "page.down",
        "Page down",
        "page",
        "space pagedown ctrl+d",
        "pagedown space",
    ),
    action("page.up", "Page up", "page", "pageup ctrl+u", "pageup"),
    sequence("top", "To the top", "page", "g g", "home"),
    action("bottom", "To the bottom", "page", "G", "end"),
    action("reader", "Reader mode on or off", "view", "r", "alt+r"),
    action(
        "media.play_pause",
        "Play or pause the media",
        "media",
        "p",
        "alt+p",
    ),
    action(
        "media.mute",
        "Mute or unmute the media",
        "media",
        "alt+m",
        "alt+m",
    ),
    action(
        "media.seek_back",
        "Back five seconds in the media",
        "media",
        "[",
        "ctrl+left",
    ),
    action(
        "media.seek_forward",
        "On five seconds in the media",
        "media",
        "]",
        "ctrl+right",
    ),
    action("media.volume_up", "Volume up", "media", "+", "alt+up"),
    action("media.volume_down", "Volume down", "media", "-", "alt+down"),
    action("bookmark", "Bookmark this page", "keep", "m", "ctrl+d"),
    action("bookmarks", "Bookmarks", "keep", "M", "ctrl+b"),
    action(
        "site.bind",
        "Bind a key on this site…",
        "keep",
        "ctrl+k",
        "ctrl+k",
    ),
    action("site.keys", "Site shortcuts…", "keep", "K", "ctrl+shift+k"),
    action("actions", "Actions (this list)", "help", "f1", "f1"),
    action("quit", "Quit", "help", "q", "ctrl+q"),
];

/// The action with this id.
pub fn get(id: &str) -> Option<&'static Action> {
    ACTIONS.iter().find(|a| a.id == id)
}

/// The keymap for `preset`, with `overrides` (`action = "keys"` pairs, the
/// config's `[keys]`) applied. An override that names no action, or no
/// key, is a warning.
pub fn keymap(preset: Preset, overrides: &[(String, String)]) -> (Keymap, Vec<String>) {
    let mut keymap = Keymap::new(CONTEXT);
    for action in ACTIONS {
        let names = match preset {
            Preset::Vim => action.vim,
            Preset::Plain => action.plain,
        };
        let keys = try_keys(names).unwrap_or_default();
        let binding = if action.sequence && names.contains(' ') {
            Binding::sequence(CONTEXT, action.id, keys, action.label)
        } else {
            Binding::new(CONTEXT, action.id, keys, action.label)
        };
        keymap.add(binding);
    }
    let mut warnings = Vec::new();
    let mut pairs = Vec::new();
    for (id, keys) in overrides {
        if get(id).is_none() {
            warnings.push(format!("keys.{id}: no such action"));
            continue;
        }
        pairs.push((format!("{CONTEXT}.{id}"), keys.clone()));
    }
    match Overrides::from_pairs(pairs) {
        Ok(overrides) => keymap.apply(&overrides),
        Err(e) => warnings.push(format!("keys: {e}")),
    }
    (keymap, warnings)
}

/// The keys of `action` in `keymap`, as intuiTUIve's bindings take them
/// (`"j down"`), and whether they are a sequence.
pub fn keys_of(keymap: &Keymap, action: &str) -> Option<(String, bool)> {
    let binding = keymap.bindings().into_iter().find(|b| b.action == action)?;
    if binding.keys.is_empty() {
        return None;
    }
    let names = binding
        .keys
        .iter()
        .map(Key::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    // A rebound sequence action whose override is one key is a key.
    Some((names, binding.sequence && binding.keys.len() > 1))
}

/// How the overlay shows an action's keys: `j/↓`, `g t`.
pub fn keys_label(keymap: &Keymap, action: &str) -> String {
    keymap
        .bindings()
        .into_iter()
        .find(|b| b.action == action)
        .map(|b| b.keys_label())
        .unwrap_or_default()
}

/// The action `key` runs in `keymap`, if it runs one by itself (not as
/// part of a sequence).
pub fn action_for(keymap: &Keymap, key: Key) -> Option<String> {
    keymap
        .bindings()
        .into_iter()
        .find(|b| !(b.sequence && b.keys.len() > 1) && b.keys.contains(&key))
        .map(|b| b.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_presets_bind_every_action() {
        for preset in [Preset::Vim, Preset::Plain] {
            let (keymap, warnings) = keymap(preset, &[]);
            assert!(warnings.is_empty());
            for action in ACTIONS {
                assert!(
                    keys_of(&keymap, action.id).is_some(),
                    "{} in {preset:?}",
                    action.id
                );
            }
        }
        let (vim, _) = keymap(Preset::Vim, &[]);
        assert_eq!(keys_of(&vim, "tab.next"), Some(("g t".into(), true)));
        assert_eq!(keys_of(&vim, "scroll.down"), Some(("j down".into(), false)));
        assert_eq!(action_for(&vim, Key::char('f')).as_deref(), Some("hints"));
        assert_eq!(action_for(&vim, Key::char('g')), None);
    }

    #[test]
    fn the_config_rebinds_and_warns() {
        let (keymap, warnings) = keymap(
            Preset::Vim,
            &[
                ("hints".into(), "ctrl+g".into()),
                ("fly".into(), "x".into()),
            ],
        );
        assert_eq!(keys_label(&keymap, "hints"), "ctrl+g");
        assert_eq!(action_for(&keymap, Key::char('f')), None);
        assert_eq!(warnings, ["keys.fly: no such action"]);
        let (_, warnings) = super::keymap(Preset::Vim, &[("hints".into(), "nokey".into())]);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn no_two_actions_share_a_key_in_a_preset() {
        for preset in [Preset::Vim, Preset::Plain] {
            let (keymap, _) = keymap(preset, &[]);
            let mut seen = std::collections::HashMap::new();
            for b in keymap.bindings().into_iter().filter(|b| !b.sequence) {
                for key in &b.keys {
                    if let Some(other) = seen.insert(*key, b.action.clone()) {
                        panic!("{key} is {other} and {} in {preset:?}", b.action);
                    }
                }
            }
        }
    }
}
