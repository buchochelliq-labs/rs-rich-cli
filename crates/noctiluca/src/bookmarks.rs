//! Bookmarks, in a file of Noctiluca's own (decision 4):
//! `bookmarks.toml` in its data directory, written only when the user adds
//! or removes one. Each is a `[[bookmark]]` with a `url`, a `title` and an
//! optional `keyword`, which the address bar expands like a shortcut's.

use std::io;
use std::path::{Path, PathBuf};

use crate::clean::clean;

/// A bookmark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    /// Typed in the address bar, it opens the bookmark; with `{}` in the
    /// address, the words after it fill it in.
    pub keyword: Option<String>,
}

/// The bookmarks file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bookmarks {
    pub path: Option<PathBuf>,
    pub items: Vec<Bookmark>,
}

/// The bookmarks file's usual place.
pub fn default_path() -> Option<PathBuf> {
    crate::config::data_dir().map(|d| d.join("bookmarks.toml"))
}

impl Bookmarks {
    /// Read `path`; a missing file is no bookmarks.
    pub fn load(path: &Path) -> io::Result<Bookmarks> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| {
            io::Error::new(io::ErrorKind::InvalidData, e.to_string())
        })?;
        let items = table
            .get("bookmark")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|b| {
                let b = b.as_table()?;
                Some(Bookmark {
                    url: b.get("url")?.as_str()?.to_string(),
                    title: clean(b.get("title").and_then(|t| t.as_str()).unwrap_or_default()),
                    keyword: b
                        .get("keyword")
                        .and_then(|k| k.as_str())
                        .map(str::to_string),
                })
            })
            .collect();
        Ok(Bookmarks {
            path: Some(path.to_path_buf()),
            items,
        })
    }

    /// Add `bookmark` (replacing one with its address) and save.
    pub fn add(&mut self, bookmark: Bookmark) -> io::Result<()> {
        self.items.retain(|b| b.url != bookmark.url);
        self.items.push(bookmark);
        self.save()
    }

    /// Remove the bookmark of `url` and save; whether there was one.
    pub fn remove(&mut self, url: &str) -> io::Result<bool> {
        let before = self.items.len();
        self.items.retain(|b| b.url != url);
        self.save()?;
        Ok(self.items.len() != before)
    }

    pub fn contains(&self, url: &str) -> bool {
        self.items.iter().any(|b| b.url == url)
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut doc = toml_edit::DocumentMut::new();
        let mut all = toml_edit::ArrayOfTables::new();
        for b in &self.items {
            let mut t = toml_edit::Table::new();
            t.insert("url", toml_edit::value(b.url.as_str()));
            t.insert("title", toml_edit::value(b.title.as_str()));
            if let Some(k) = &b.keyword {
                t.insert("keyword", toml_edit::value(k.as_str()));
            }
            all.push(t);
        }
        doc.insert("bookmark", toml_edit::Item::ArrayOfTables(all));
        rich_intuituive::write_atomic(path, doc.to_string().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_round_trip_with_keywords() {
        let path =
            std::env::temp_dir().join(format!("noctiluca-marks-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut marks = Bookmarks::load(&path).unwrap();
        assert!(marks.items.is_empty());
        marks
            .add(Bookmark {
                url: "https://docs.rs/serde".into(),
                title: "serde".into(),
                keyword: Some("sd".into()),
            })
            .unwrap();
        let again = Bookmarks::load(&path).unwrap();
        assert_eq!(again.items, marks.items);
        let config = crate::config::Config::default();
        assert_eq!(
            config.destination("sd", "about:blank", &again.items).url(),
            "https://docs.rs/serde"
        );
        assert!(marks.remove("https://docs.rs/serde").unwrap());
        assert!(Bookmarks::load(&path).unwrap().items.is_empty());
        std::fs::remove_file(&path).ok();
    }
}
