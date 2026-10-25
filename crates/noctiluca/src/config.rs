//! Noctiluca's configuration: `noctiluca/config.toml` in the platform
//! config directory, or the file `--config` names. A documented sample is
//! `config.sample.toml` beside this crate's manifest.
//!
//! - `[shortcuts]`: keywords (`gh = "https://github.com/search?q={}"`),
//!   over the built-ins, which a `false` removes.
//! - `[search]`: the search engine (`url`, with `{}` for the query).
//! - `[keys]`: `preset` (`vim` or `plain`) and `action = "keys"` lines.
//! - `[startup]`: `home` and `view` (`projection`, `reader`, `raster`).
//! - `[media]`: `audio`, `fps`, `images`.
//! - `[profile]`: `name`, a kept profile (opt-in).
//! - `[site."PATTERN"]`: per-site `view`, `images`, `audio`, `fps`,
//!   `width`, `keyword` and `[site."PATTERN".keys]`.
//!
//! An unknown key is a warning, never an error, so an old config opens.
//! Edits ([`save_site_key`], [`remove_site_key`]) go through `toml_edit`,
//! so the user's comments and layout survive, and are written atomically.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::url::{encode_query, host, looks_like_address, complete_address};

/// A view of a page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// Chrome's geometry snapped to cells (decision 9's default).
    #[default]
    Projection,
    /// The blocks reflowed into one column.
    Reader,
    /// Chrome's own pixels, in half blocks: a debug view.
    Raster,
}

impl View {
    pub fn parse(text: &str) -> Option<View> {
        match text {
            "projection" => Some(View::Projection),
            "reader" => Some(View::Reader),
            "raster" => Some(View::Raster),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            View::Projection => "projection",
            View::Reader => "reader",
            View::Raster => "raster",
        }
    }
}

/// How images reach the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Images {
    /// What the terminal is likely to speak.
    #[default]
    Auto,
    /// Half blocks: any terminal with colour.
    Ansi,
    Kitty,
    Sixel,
    Iterm2,
    /// No pictures: alt text only.
    None,
}

impl Images {
    pub fn parse(text: &str) -> Option<Images> {
        Some(match text {
            "auto" => Images::Auto,
            "ansi" => Images::Ansi,
            "kitty" => Images::Kitty,
            "sixel" => Images::Sixel,
            "iterm2" => Images::Iterm2,
            "none" => Images::None,
            _ => return None,
        })
    }
}

/// Which default key set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    /// `o`, `f`, `/`, `H` and `L`, `g t`: the vim-like browsers' keys.
    #[default]
    Vim,
    /// Arrows, `Alt+Left` and `Alt+Right`, `Ctrl+F`, `Ctrl+T`.
    Plain,
}

/// A keyword: a template with `{}` for the query, and a home address for
/// the keyword alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub template: Option<String>,
    pub home: String,
}

/// What a site key does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A built-in action from the registry, by id.
    Action(String),
    /// Activate the `nth` (from 1) element with this role and accessible
    /// name, looked up in the AX tree when the key is pressed.
    Element { role: String, name: String, nth: usize },
    /// Several, in order.
    Sequence(Vec<Target>),
}

impl Target {
    /// How the overlay and `noctiluca keys` say it.
    pub fn describe(&self) -> String {
        match self {
            Target::Action(id) => id.clone(),
            Target::Element { role, name, nth } if *nth > 1 => {
                format!("{role} {name:?} (match {nth})")
            }
            Target::Element { role, name, .. } => format!("{role} {name:?}"),
            Target::Sequence(all) => all.iter().map(Target::describe).collect::<Vec<_>>().join(", then "),
        }
    }
}

/// A key bound on a site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteKey {
    pub keys: String,
    pub target: Target,
}

/// A `[site."…"]` table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Site {
    pub pattern: String,
    pub keys: Vec<SiteKey>,
    pub view: Option<View>,
    pub images: Option<bool>,
    pub audio: Option<bool>,
    pub fps: Option<u32>,
    pub width: Option<u16>,
    pub keyword: Option<String>,
}

/// How specifically `pattern` matches `url`, if it does: a URL prefix
/// over an exact host over a wildcard, a longer one over a shorter.
pub fn site_match(pattern: &str, url: &str) -> Option<(u8, usize)> {
    let pattern = pattern.trim();
    if pattern.contains("://") {
        return url.starts_with(pattern).then_some((3, pattern.len()));
    }
    let host = host(url)?;
    let pattern = pattern.to_ascii_lowercase();
    if let Some(domain) = pattern.strip_prefix("*.") {
        let under = host == domain || host.ends_with(&format!(".{domain}"));
        return under.then_some((1, domain.len()));
    }
    (host == pattern).then_some((2, pattern.len()))
}

/// Noctiluca's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub shortcuts: BTreeMap<String, Shortcut>,
    /// The search engine, `{}` for the query.
    pub search: String,
    pub preset: Preset,
    /// `action = "keys"` overrides, as written.
    pub keys: Vec<(String, String)>,
    pub home: Option<String>,
    pub view: View,
    pub audio: bool,
    pub fps: u32,
    pub images: Images,
    /// A kept profile's name, if one is asked for.
    pub profile: Option<String>,
    pub sites: Vec<Site>,
    /// What was not understood, for the user.
    pub warnings: Vec<String>,
    /// The file it came from, which edits go to.
    pub path: Option<PathBuf>,
}

/// The search engine when none is set: DuckDuckGo's HTML search, with no
/// tracking parameters.
pub const DEFAULT_SEARCH: &str = "https://html.duckduckgo.com/html/?q={}";

/// The built-in keywords.
pub fn builtin_shortcuts() -> BTreeMap<String, Shortcut> {
    let make = |template: &str, home: &str| Shortcut {
        template: Some(template.to_string()),
        home: home.to_string(),
    };
    BTreeMap::from([
        ("rs".to_string(), make("https://docs.rs/releases/search?query={}", "https://docs.rs")),
        ("crates".to_string(), make("https://crates.io/search?q={}", "https://crates.io")),
        ("mdn".to_string(), make("https://developer.mozilla.org/en-US/search?q={}", "https://developer.mozilla.org")),
        ("gh".to_string(), make("https://github.com/search?q={}", "https://github.com")),
    ])
}

impl Default for Config {
    fn default() -> Self {
        Config {
            shortcuts: builtin_shortcuts(),
            search: DEFAULT_SEARCH.to_string(),
            preset: Preset::Vim,
            keys: Vec::new(),
            home: None,
            view: View::Projection,
            audio: false,
            fps: 8,
            images: Images::Auto,
            profile: None,
            sites: Vec::new(),
            warnings: Vec::new(),
            path: None,
        }
    }
}

/// Where a platform keeps an app's configuration.
pub fn config_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".config")))
    };
    base.map(|b| b.join("noctiluca"))
}

/// Where Noctiluca keeps its data: kept profiles and bookmarks.
pub fn data_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        var("XDG_DATA_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".local/share")))
    };
    base.map(|b| b.join("noctiluca"))
}

/// The config file's usual place.
pub fn default_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

fn template_home(template: &str) -> String {
    match template.split_once("://") {
        Some((scheme, rest)) => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
            format!("{scheme}://{host}")
        }
        None => template.to_string(),
    }
}

impl Config {
    /// Read `path`, or the usual place; a missing file is the defaults.
    pub fn load(path: Option<&Path>) -> io::Result<Config> {
        let explicit = path.is_some();
        let Some(path) = path.map(Path::to_path_buf).or_else(default_path) else {
            return Ok(Config::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut config = Config::parse(&text).map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display()))
                })?;
                config.path = Some(path);
                Ok(config)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && !explicit => Ok(Config {
                path: Some(path),
                ..Config::default()
            }),
            Err(e) => Err(io::Error::new(e.kind(), format!("{}: {e}", path.display()))),
        }
    }

    /// The config in `text`. A TOML syntax error is an error; anything
    /// not understood is a warning.
    pub fn parse(text: &str) -> Result<Config, String> {
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let mut config = Config::default();
        let mut warn = |w: String| config.warnings.push(w);
        let mut shortcuts = builtin_shortcuts();
        let mut search = None;
        let mut preset = Preset::Vim;
        let mut keys = Vec::new();
        let (mut home, mut view) = (None, View::Projection);
        let (mut audio, mut fps, mut images) = (false, 8u32, Images::Auto);
        let mut profile = None;
        let mut sites = Vec::new();
        for (section, value) in &table {
            let Some(body) = value.as_table() else {
                warn(format!("{section}: expected a table"));
                continue;
            };
            match section.as_str() {
                "shortcuts" => {
                    for (word, value) in body {
                        match value {
                            toml::Value::Boolean(false) => {
                                shortcuts.remove(word);
                            }
                            toml::Value::String(template) => {
                                let has_query = template.contains("{}");
                                shortcuts.insert(
                                    word.clone(),
                                    Shortcut {
                                        template: has_query.then(|| template.clone()),
                                        home: if has_query { template_home(template) } else { template.clone() },
                                    },
                                );
                            }
                            toml::Value::Table(t) => {
                                let template = t.get("search").and_then(|v| v.as_str()).map(str::to_string);
                                let home = t
                                    .get("home")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                                    .or_else(|| template.as_deref().map(template_home));
                                for key in t.keys().filter(|k| !matches!(k.as_str(), "search" | "home")) {
                                    warn(format!("shortcuts.{word}.{key}: unknown key"));
                                }
                                match home {
                                    Some(home) => {
                                        shortcuts.insert(word.clone(), Shortcut { template, home });
                                    }
                                    None => warn(format!("shortcuts.{word}: needs search or home")),
                                }
                            }
                            _ => warn(format!("shortcuts.{word}: expected an address, a table or false")),
                        }
                    }
                }
                "search" => {
                    for (key, value) in body {
                        match (key.as_str(), value.as_str()) {
                            ("url", Some(url)) if url.contains("{}") => search = Some(url.to_string()),
                            ("url", _) => warn("search.url: expected an address with {} for the query".into()),
                            _ => warn(format!("search.{key}: unknown key")),
                        }
                    }
                }
                "keys" => {
                    for (key, value) in body {
                        match (key.as_str(), value) {
                            ("preset", toml::Value::String(p)) => match p.as_str() {
                                "vim" => preset = Preset::Vim,
                                "plain" => preset = Preset::Plain,
                                other => warn(format!("keys.preset: {other:?} is not vim or plain")),
                            },
                            (action, toml::Value::String(k)) => keys.push((action.to_string(), k.clone())),
                            (action, _) => warn(format!("keys.{action}: expected keys, as a string")),
                        }
                    }
                }
                "startup" => {
                    for (key, value) in body {
                        match (key.as_str(), value.as_str()) {
                            ("home", Some(h)) => home = Some(h.to_string()),
                            ("view", Some(v)) => match View::parse(v) {
                                Some(v) => view = v,
                                None => warn(format!("startup.view: {v:?} is not projection, reader or raster")),
                            },
                            _ => warn(format!("startup.{key}: unknown key")),
                        }
                    }
                }
                "media" => {
                    for (key, value) in body {
                        match (key.as_str(), value) {
                            ("audio", toml::Value::Boolean(b)) => audio = *b,
                            ("fps", toml::Value::Integer(n)) if (1..=60).contains(n) => fps = *n as u32,
                            ("images", toml::Value::String(s)) => match Images::parse(s) {
                                Some(i) => images = i,
                                None => warn(format!("media.images: {s:?} is not auto, ansi, kitty, sixel, iterm2 or none")),
                            },
                            _ => warn(format!("media.{key}: unknown key or value")),
                        }
                    }
                }
                "profile" => {
                    for (key, value) in body {
                        match (key.as_str(), value.as_str()) {
                            ("name", Some(n)) if valid_profile(n) => profile = Some(n.to_string()),
                            ("name", _) => warn("profile.name: letters, digits, - and _ only".into()),
                            _ => warn(format!("profile.{key}: unknown key")),
                        }
                    }
                }
                "site" => {
                    for (pattern, value) in body {
                        match value.as_table() {
                            Some(t) => sites.push(parse_site(pattern, t, &mut warn)),
                            None => warn(format!("site.{pattern:?}: expected a table")),
                        }
                    }
                }
                other => warn(format!("[{other}]: unknown table")),
            }
        }
        config.shortcuts = shortcuts;
        config.search = search.unwrap_or_else(|| DEFAULT_SEARCH.to_string());
        config.preset = preset;
        config.keys = keys;
        config.home = home;
        config.view = view;
        config.audio = audio;
        config.fps = fps;
        config.images = images;
        config.profile = profile;
        config.sites = sites;
        Ok(config)
    }

    /// The most specific site table for `url`.
    pub fn site(&self, url: &str) -> Option<&Site> {
        self.sites
            .iter()
            .filter_map(|s| site_match(&s.pattern, url).map(|rank| (rank, s)))
            .max_by_key(|(rank, _)| *rank)
            .map(|(_, s)| s)
    }

    /// What the address bar should open for `input`: an address, a
    /// keyword's expansion, or a search, in that order. `url` is the page
    /// open now (for a site's own keyword); `bookmarks` adds theirs.
    pub fn destination(&self, input: &str, url: &str, bookmarks: &[crate::bookmarks::Bookmark]) -> Destination {
        let input = input.trim();
        if input.is_empty() {
            return Destination::Url(self.home.clone().unwrap_or_else(|| "about:blank".into()));
        }
        if looks_like_address(input) {
            return Destination::Url(complete_address(input));
        }
        let (word, rest) = match input.split_once(char::is_whitespace) {
            Some((w, r)) => (w, r.trim()),
            None => (input, ""),
        };
        if let Some(site) = self.site(url) {
            if site.keyword.as_deref() == Some(word) && !rest.is_empty() {
                if let Some(host) = host(url) {
                    let query = format!("site:{host} {rest}");
                    return Destination::Search(self.search.replace("{}", &encode_query(&query)));
                }
            }
        }
        let expand = |shortcut: &Shortcut| match (&shortcut.template, rest.is_empty()) {
            (Some(template), false) => template.replace("{}", &encode_query(rest)),
            _ => shortcut.home.clone(),
        };
        if let Some(mark) = bookmarks.iter().find(|b| b.keyword.as_deref() == Some(word)) {
            let shortcut = Shortcut {
                template: mark.url.contains("{}").then(|| mark.url.clone()),
                home: if mark.url.contains("{}") { template_home(&mark.url) } else { mark.url.clone() },
            };
            return Destination::Keyword(expand(&shortcut));
        }
        if let Some(shortcut) = self.shortcuts.get(word) {
            return Destination::Keyword(expand(shortcut));
        }
        Destination::Search(self.search.replace("{}", &encode_query(input)))
    }
}

/// Where the address bar goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    Url(String),
    Keyword(String),
    Search(String),
}

impl Destination {
    pub fn url(&self) -> &str {
        match self {
            Destination::Url(u) | Destination::Keyword(u) | Destination::Search(u) => u,
        }
    }
}

fn valid_profile(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Parse a site key's value: an action id, `{ role, name, nth }`, or an
/// array of those.
fn parse_target(value: &toml::Value) -> Result<Target, String> {
    match value {
        toml::Value::String(id) => Ok(Target::Action(id.clone())),
        toml::Value::Table(t) => {
            let role = t.get("role").and_then(|v| v.as_str()).ok_or("needs role")?;
            let name = t.get("name").and_then(|v| v.as_str()).ok_or("needs name")?;
            let nth = match t.get("nth") {
                None => 1,
                Some(toml::Value::Integer(n)) if *n >= 1 => *n as usize,
                Some(_) => return Err("nth counts from 1".into()),
            };
            if let Some(other) = t.keys().find(|k| !matches!(k.as_str(), "role" | "name" | "nth")) {
                // No selectors and no script: a role and a name only.
                return Err(format!("{other:?} is not role, name or nth"));
            }
            Ok(Target::Element {
                role: role.to_string(),
                name: name.to_string(),
                nth,
            })
        }
        toml::Value::Array(all) if !all.is_empty() => Ok(Target::Sequence(
            all.iter().map(parse_target).collect::<Result<_, _>>()?,
        )),
        _ => Err("expected an action id, { role, name } or a list of them".into()),
    }
}

fn parse_site(pattern: &str, table: &toml::Table, warn: &mut impl FnMut(String)) -> Site {
    let mut site = Site {
        pattern: pattern.to_string(),
        ..Site::default()
    };
    for (key, value) in table {
        let at = format!("site.{pattern:?}.{key}");
        match (key.as_str(), value) {
            ("view", toml::Value::String(v)) => match View::parse(v) {
                Some(v) => site.view = Some(v),
                None => warn(format!("{at}: {v:?} is not projection, reader or raster")),
            },
            ("images", toml::Value::Boolean(b)) => site.images = Some(*b),
            ("audio", toml::Value::Boolean(b)) => site.audio = Some(*b),
            ("fps", toml::Value::Integer(n)) if (1..=60).contains(n) => site.fps = Some(*n as u32),
            ("width", toml::Value::Integer(n)) if (20..=1000).contains(n) => site.width = Some(*n as u16),
            ("keyword", toml::Value::String(k)) if !k.contains(char::is_whitespace) => {
                site.keyword = Some(k.clone())
            }
            ("keys", toml::Value::Table(keys)) => {
                for (keys, target) in keys {
                    if rich_intuituive::interact::keymap::try_keys(keys).is_err() {
                        warn(format!("{at}.{keys:?}: not a key"));
                        continue;
                    }
                    match parse_target(target) {
                        Ok(target) => site.keys.push(SiteKey {
                            keys: keys.clone(),
                            target,
                        }),
                        Err(e) => warn(format!("{at}.{keys:?}: {e}")),
                    }
                }
            }
            _ => warn(format!("{at}: unknown key or value")),
        }
    }
    site
}

/// A target as a `toml_edit` value.
fn target_value(target: &Target) -> toml_edit::Value {
    match target {
        Target::Action(id) => toml_edit::Value::from(id.as_str()),
        Target::Element { role, name, nth } => {
            let mut table = toml_edit::InlineTable::new();
            table.insert("role", role.as_str().into());
            table.insert("name", name.as_str().into());
            if *nth > 1 {
                table.insert("nth", (*nth as i64).into());
            }
            toml_edit::Value::InlineTable(table)
        }
        Target::Sequence(all) => {
            let mut array = toml_edit::Array::new();
            for t in all {
                array.push(target_value(t));
            }
            toml_edit::Value::Array(array)
        }
    }
}

fn edit<T>(path: &Path, change: impl FnOnce(&mut toml_edit::DocumentMut) -> T) -> io::Result<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let out = change(&mut doc);
    rich_intuituive::write_atomic(path, doc.to_string().as_bytes())?;
    Ok(out)
}

/// The `[site."pattern".keys]` table of `doc`, made if missing.
fn site_keys<'d>(doc: &'d mut toml_edit::DocumentMut, pattern: &str) -> &'d mut toml_edit::Table {
    let site = doc
        .entry("site")
        .or_insert_with(|| {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            toml_edit::Item::Table(t)
        })
        .as_table_mut()
        .expect("[site] is a table");
    let table = site
        .entry(pattern)
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .expect("a site is a table");
    table
        .entry("keys")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .expect("keys is a table")
}

/// Bind `keys` to `target` on the site `pattern`, in the config file at
/// `path`, keeping everything else in it as it is. Returns what the key
/// did on that site before, if anything.
pub fn save_site_key(path: &Path, pattern: &str, keys: &str, target: &Target) -> io::Result<Option<String>> {
    edit(path, |doc| {
        let table = site_keys(doc, pattern);
        let mut value = target_value(target);
        match table.get_mut(keys) {
            // Replace the value in place: the key, and the comments around
            // it, stay as the user wrote them.
            Some(item) => {
                let before = item.to_string().trim().to_string();
                if let Some(old) = item.as_value() {
                    *value.decor_mut() = old.decor().clone();
                }
                *item = toml_edit::Item::Value(value);
                Some(before)
            }
            None => {
                table.insert(keys, toml_edit::Item::Value(value));
                None
            }
        }
    })
}

/// Remove `keys` from the site `pattern` in the file at `path`.
pub fn remove_site_key(path: &Path, pattern: &str, keys: &str) -> io::Result<bool> {
    edit(path, |doc| {
        doc.get_mut("site")
            .and_then(|s| s.get_mut(pattern))
            .and_then(|s| s.get_mut("keys"))
            .and_then(|k| k.as_table_mut())
            .is_some_and(|k| k.remove(keys).is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_expand_with_their_query_escaped() {
        let config = Config::default();
        let go = |input: &str| config.destination(input, "about:blank", &[]);
        assert_eq!(
            go("gh rs-rich & co"),
            Destination::Keyword("https://github.com/search?q=rs-rich+%26+co".into())
        );
        assert_eq!(go("gh"), Destination::Keyword("https://github.com".into()));
        assert_eq!(go("crates serde"), Destination::Keyword("https://crates.io/search?q=serde".into()));
    }

    #[test]
    fn the_address_bar_tries_an_address_then_a_keyword_then_a_search() {
        let config = Config::default();
        let go = |input: &str| config.destination(input, "about:blank", &[]);
        assert_eq!(go("docs.rs/serde"), Destination::Url("https://docs.rs/serde".into()));
        assert_eq!(go("https://gh.example"), Destination::Url("https://gh.example".into()));
        assert!(matches!(go("gh x"), Destination::Keyword(_)));
        assert_eq!(
            go("what is noctiluca"),
            Destination::Search("https://html.duckduckgo.com/html/?q=what+is+noctiluca".into())
        );
        // An unknown word is a search, not an address.
        assert!(matches!(go("noctiluca"), Destination::Search(_)));
    }

    #[test]
    fn the_config_overrides_and_removes_keywords_and_sets_the_search() {
        let config = Config::parse(
            r#"
            [shortcuts]
            gh = "https://gh.example/find?q={}"
            mdn = false
            wiki = { search = "https://w.example/s?q={}", home = "https://w.example/main" }
            home = "https://home.example"
            [search]
            url = "https://search.example/?q={}"
            "#,
        )
        .unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        let go = |input: &str| config.destination(input, "about:blank", &[]);
        assert_eq!(go("gh a b").url(), "https://gh.example/find?q=a+b");
        assert_eq!(go("gh").url(), "https://gh.example");
        assert_eq!(go("wiki").url(), "https://w.example/main");
        assert_eq!(go("home").url(), "https://home.example");
        assert_eq!(go("mdn fetch").url(), "https://search.example/?q=mdn+fetch");
    }

    #[test]
    fn unknown_keys_warn_and_the_rest_still_reads() {
        let config = Config::parse(
            r#"
            colour = "blue"
            [startup]
            view = "reader"
            zoom = 2
            [media]
            audio = true
            images = "kitty"
            fps = 12
            [keys]
            preset = "plain"
            hints = "ctrl+g"
            [profile]
            name = "work"
            [nonsense]
            "#,
        )
        .unwrap();
        assert_eq!(config.view, View::Reader);
        assert!(config.audio);
        assert_eq!((config.images, config.fps), (Images::Kitty, 12));
        assert_eq!(config.preset, Preset::Plain);
        assert_eq!(config.keys, [("hints".to_string(), "ctrl+g".to_string())]);
        assert_eq!(config.profile.as_deref(), Some("work"));
        assert_eq!(
            config.warnings,
            ["colour: expected a table", "startup.zoom: unknown key", "[nonsense]: unknown table"]
        );
        assert!(Config::parse("[startup").is_err());
    }

    #[test]
    fn the_most_specific_site_wins() {
        let config = Config::parse(
            r#"
            [site."*.example.com"]
            view = "raster"
            [site."docs.example.com"]
            view = "reader"
            [site."https://docs.example.com/api/"]
            view = "projection"
            [site."*.docs.example.com"]
            images = false
            "#,
        )
        .unwrap();
        let view = |url: &str| config.site(url).and_then(|s| s.view);
        assert_eq!(view("https://docs.example.com/api/x"), Some(View::Projection));
        assert_eq!(view("https://docs.example.com/guide"), Some(View::Reader));
        assert_eq!(view("https://www.example.com/"), Some(View::Raster));
        assert_eq!(view("https://example.com/"), Some(View::Raster));
        assert_eq!(view("https://example.org/"), None);
        assert_eq!(site_match("*.example.com", "https://evil-example.com/"), None);
        assert_eq!(site_match("github.com", "https://GitHub.com/x"), Some((2, 10)));
    }

    #[test]
    fn site_keys_bind_actions_elements_and_sequences_never_script() {
        let config = Config::parse(
            r##"
            [site."github.com"]
            keyword = "s"
            [site."github.com".keys]
            n = "scroll.down"
            "]" = { role = "link", name = "Next page" }
            x = [ "back", { role = "button", name = "Close", nth = 2 } ]
            y = { selector = "#evil" }
            z = { role = "button", name = "A", script = "alert(1)" }
            "##,
        )
        .unwrap();
        let site = config.site("https://github.com/rs").unwrap();
        assert_eq!(site.keys.len(), 3);
        assert_eq!(site.keys[0].target, Target::Action("scroll.down".into()));
        assert_eq!(
            site.keys[2].target.describe(),
            "back, then button \"Close\" (match 2)"
        );
        assert_eq!(config.warnings.len(), 2, "{:?}", config.warnings);
        assert_eq!(
            config.destination("s noctiluca", "https://github.com/rs", &[]).url(),
            "https://html.duckduckgo.com/html/?q=site%3Agithub.com+noctiluca"
        );
    }

    #[test]
    fn a_saved_site_key_keeps_the_users_comments() {
        let dir = std::env::temp_dir().join(format!("noctiluca-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# My browser.\n[startup]\nview = \"reader\" # always\n\n[site.\"github.com\".keys]\n# next\nn = \"scroll.down\"\n",
        )
        .unwrap();
        let target = Target::Element {
            role: "button".into(),
            name: "Next page".into(),
            nth: 1,
        };
        assert_eq!(save_site_key(&path, "github.com", "]", &target).unwrap(), None);
        let before = save_site_key(&path, "github.com", "n", &Target::Action("back".into())).unwrap();
        assert_eq!(before.as_deref(), Some("\"scroll.down\""));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# My browser.\n"), "{text}");
        assert!(text.contains("view = \"reader\" # always"), "{text}");
        assert!(text.contains("# next\n"), "{text}");
        assert!(text.contains("\"]\" = { role = \"button\", name = \"Next page\" }"), "{text}");
        let config = Config::parse(&text).unwrap();
        let site = config.site("https://github.com/").unwrap();
        assert_eq!(site.keys.len(), 2);
        assert!(remove_site_key(&path, "github.com", "]").unwrap());
        assert!(!std::fs::read_to_string(&path).unwrap().contains("Next page"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
