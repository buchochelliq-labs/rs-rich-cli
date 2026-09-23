//! Semantic hyperlinks for URLs, paths, source locations and references.
//!
//! A [`Hyperlinker`] turns what it recognises in a [`Text`] into OSC 8 links:
//! `http(s)://` URLs, file paths with an optional `:line[:column]`, and — when
//! a repository is known — `#123` and `owner/repo#123` references. Links are
//! plain style attributes, so a console that is not a terminal renders the same
//! text with no escape codes: the fallback is the text itself.
//!
//! File links use `file://` URLs (`file:///abs/path#12`, the form core's
//! `LogRender` uses) unless an editor template such as
//! `vscode://file{path}:{line}:{column}` is set (`{path}` is absolute, so it
//! already starts with `/`). Relative paths resolve
//! against a base directory when one is given.
//!
//! It is a [`Highlighter`], so it plugs into anything that takes one, such as
//! the log handler; diagnostics and stack traces link their locations through it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fancy_regex::Regex;
use rich::{Highlighter, Style, Text};

/// Recognises linkable spans in text and builds their URLs.
#[derive(Clone, Debug)]
pub struct Hyperlinker {
    enabled: bool,
    urls: bool,
    paths: bool,
    base_dir: Option<PathBuf>,
    editor: Option<String>,
    repository: Option<String>,
}

impl Default for Hyperlinker {
    fn default() -> Self {
        Hyperlinker {
            enabled: true,
            urls: true,
            paths: true,
            base_dir: None,
            editor: None,
            repository: None,
        }
    }
}

/// What a scan found, as byte offsets into the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Start byte of the linked span.
    pub start: usize,
    /// End byte of the linked span.
    pub end: usize,
    /// The link target.
    pub url: String,
}

fn url_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"https?://[^\s<>"'`]+[^\s<>"'`.,;:!?)\]}]"#).expect("valid URL pattern")
    })
}

fn path_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        // An absolute, `./`, `../` or `~/` path; a relative path with a
        // directory whose last part has an extension; or a bare file name
        // followed by a line. Then an optional `:line[:column]`.
        Regex::new(concat!(
            r"(?<![\w/.:~-])(?P<path>",
            r"(?:/|\./|\.\./|~/)[\w.\-/+@]+",
            r"|[\w.\-+@]+(?:/[\w.\-+@]+)*/[\w\-+@]+\.[A-Za-z0-9]{1,8}",
            r"|[\w\-+@]+\.[A-Za-z][A-Za-z0-9]{0,7}(?=:\d)",
            r")(?::(?P<line>\d+)(?::(?P<column>\d+))?)?(?![\w/])",
        ))
        .expect("valid path pattern")
    })
}

fn reference_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?<![\w/#])(?P<repo>[\w.-]+/[\w.-]+)?#(?P<number>\d+)\b")
            .expect("valid reference pattern")
    })
}

impl Hyperlinker {
    /// Link URLs and paths; references need [`repository`](Self::repository).
    pub fn new() -> Self {
        Hyperlinker::default()
    }

    /// A linker that links nothing (the plain fallback, chosen explicitly).
    pub fn disabled() -> Self {
        Hyperlinker {
            enabled: false,
            ..Hyperlinker::default()
        }
    }

    /// Turn linking on or off.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether this linker adds links at all.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Link `http(s)://` URLs (default on).
    pub fn urls(mut self, urls: bool) -> Self {
        self.urls = urls;
        self
    }

    /// Link file paths and `path:line:column` locations (default on).
    pub fn paths(mut self, paths: bool) -> Self {
        self.paths = paths;
        self
    }

    /// Resolve relative paths against `dir`.
    pub fn base_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.base_dir = Some(dir.into());
        self
    }

    /// Link files through an editor URL template instead of `file://`, for
    /// example `vscode://file{path}:{line}:{column}`. `{path}` is absolute and
    /// already starts with `/`, so the template has no slash of its own before
    /// it. `{line}` and `{column}` default to 1 when a location has none.
    pub fn editor(mut self, template: impl Into<String>) -> Self {
        self.editor = Some(template.into());
        self
    }

    /// The repository web URL (`https://github.com/owner/repo`) that `#123`
    /// references link into. `owner/repo#123` links into that repository on
    /// the same host.
    pub fn repository(mut self, url: impl Into<String>) -> Self {
        self.repository = Some(url.into().trim_end_matches('/').to_string());
        self
    }

    /// The URL for a file location, or `None` when linking is off.
    pub fn file_url(
        &self,
        path: &str,
        line: Option<usize>,
        column: Option<usize>,
    ) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let expanded = self.resolve(path);
        let path = expanded.to_string_lossy();
        Some(match &self.editor {
            Some(template) => template
                .replace("{path}", &encode_path(&path))
                .replace("{line}", &line.unwrap_or(1).to_string())
                .replace("{column}", &column.unwrap_or(1).to_string()),
            None => {
                let mut url = format!("file://{}", encode_path(&path));
                if let Some(line) = line {
                    url.push_str(&format!("#{line}"));
                }
                url
            }
        })
    }

    /// The URL for issue or pull request `number`, in `repo` (`owner/name`)
    /// or the configured repository.
    pub fn reference_url(&self, repo: Option<&str>, number: u64) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let base = self.repository.as_deref()?;
        let base = match repo {
            Some(repo) => {
                // Same host, other repository: keep the scheme and host.
                let host_end = base
                    .find("://")
                    .and_then(|scheme| base[scheme + 3..].find('/').map(|slash| scheme + 3 + slash))
                    .unwrap_or(base.len());
                format!("{}/{repo}", &base[..host_end])
            }
            None => base.to_string(),
        };
        Some(format!("{base}/issues/{number}"))
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = path.strip_prefix("./").unwrap_or(path);
        let path = match path.strip_prefix("~/") {
            Some(rest) => match std::env::var_os("HOME") {
                Some(home) => Path::new(&home).join(rest),
                None => PathBuf::from(path),
            },
            None => PathBuf::from(path),
        };
        match (&self.base_dir, path.is_absolute()) {
            (Some(base), false) => base.join(path),
            _ => path,
        }
    }

    /// Everything linkable in `text`, in order, without overlaps: URLs first,
    /// then references, then paths outside those.
    pub fn find(&self, text: &str) -> Vec<Link> {
        if !self.enabled {
            return Vec::new();
        }
        let mut links: Vec<Link> = Vec::new();
        let taken = |links: &[Link], start: usize, end: usize| {
            links
                .iter()
                .any(|link| start < link.end && link.start < end)
        };
        if self.urls {
            for found in url_pattern().find_iter(text).flatten() {
                links.push(Link {
                    start: found.start(),
                    end: found.end(),
                    url: found.as_str().to_string(),
                });
            }
        }
        if self.repository.is_some() {
            for captures in reference_pattern().captures_iter(text).flatten() {
                let whole = captures.get(0).expect("whole match");
                if taken(&links, whole.start(), whole.end()) {
                    continue;
                }
                let repo = captures.name("repo").map(|repo| repo.as_str());
                let Ok(number) = captures["number"].parse() else {
                    continue;
                };
                if let Some(url) = self.reference_url(repo, number) {
                    links.push(Link {
                        start: whole.start(),
                        end: whole.end(),
                        url,
                    });
                }
            }
        }
        if self.paths {
            for captures in path_pattern().captures_iter(text).flatten() {
                let whole = captures.get(0).expect("whole match");
                if taken(&links, whole.start(), whole.end()) {
                    continue;
                }
                let number = |name: &str| captures.name(name).and_then(|m| m.as_str().parse().ok());
                if let Some(url) =
                    self.file_url(&captures["path"], number("line"), number("column"))
                {
                    links.push(Link {
                        start: whole.start(),
                        end: whole.end(),
                        url,
                    });
                }
            }
        }
        links.sort_by_key(|link| link.start);
        links
    }

    /// Add a link span for everything [`find`](Self::find) recognises.
    pub fn link(&self, text: &mut Text) {
        for link in self.find(text.plain()) {
            text.stylize(Style::new().with_link(link.url), link.start, link.end);
        }
    }

    /// `path:line:column` as text linked to the location, in `style`.
    pub fn location(
        &self,
        path: &str,
        line: Option<usize>,
        column: Option<usize>,
        style: impl Into<rich::StyleType>,
    ) -> Text {
        let mut label = path.to_string();
        if let Some(line) = line {
            label.push_str(&format!(":{line}"));
            if let Some(column) = column {
                label.push_str(&format!(":{column}"));
            }
        }
        let mut text = Text::styled(label, style);
        if let Some(url) = self.file_url(path, line, column) {
            let end = text.plain().len();
            text.stylize(Style::new().with_link(url), 0, end);
        }
        text
    }
}

impl Highlighter for Hyperlinker {
    fn highlight(&self, text: &mut Text) {
        self.link(text);
    }
}

/// Percent-encode the characters that would end or confuse a URL path.
fn encode_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            _ => out.push(ch),
        }
    }
    // `C:/x` becomes `/C:/x`, so `file://` + path is a valid file URL.
    if out.as_bytes().get(1) == Some(&b':') {
        out.insert(0, '/');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(linker: &Hyperlinker, text: &str) -> Vec<(String, String)> {
        linker
            .find(text)
            .into_iter()
            .map(|link| (text[link.start..link.end].to_string(), link.url))
            .collect()
    }

    #[test]
    fn urls_drop_trailing_punctuation() {
        let links = found(
            &Hyperlinker::new(),
            "see https://example.com/a?b=1. Then (https://x.io)",
        );
        assert_eq!(links[0].0, "https://example.com/a?b=1");
        assert_eq!(links[1].0, "https://x.io");
    }

    #[test]
    fn locations_link_with_line_and_column() {
        let linker = Hyperlinker::new().base_dir("/work");
        let links = found(&linker, "error at src/main.rs:12:5 and /etc/hosts");
        assert_eq!(
            links[0],
            (
                "src/main.rs:12:5".into(),
                "file:///work/src/main.rs#12".into()
            )
        );
        assert_eq!(links[1], ("/etc/hosts".into(), "file:///etc/hosts".into()));
    }

    #[test]
    fn editor_templates_fill_line_and_column() {
        let linker = Hyperlinker::new().editor("vscode://file{path}:{line}:{column}");
        assert_eq!(
            linker.file_url("/a b/c.rs", Some(3), None).unwrap(),
            "vscode://file/a%20b/c.rs:3:1"
        );
    }

    #[test]
    fn references_need_a_repository() {
        assert!(found(&Hyperlinker::new(), "fixes #12").is_empty());
        let linker = Hyperlinker::new().repository("https://github.com/o/r/");
        assert_eq!(
            found(&linker, "fixes #12 and other/repo#3"),
            vec![
                ("#12".into(), "https://github.com/o/r/issues/12".into()),
                (
                    "other/repo#3".into(),
                    "https://github.com/other/repo/issues/3".into()
                ),
            ]
        );
    }

    #[test]
    fn plain_words_and_versions_are_not_paths() {
        let links = found(&Hyperlinker::new(), "version 1.2.3 of rich, e.g. done.");
        assert!(links.is_empty(), "{links:?}");
    }

    #[test]
    fn disabled_links_nothing() {
        let mut text = Text::new("https://example.com src/lib.rs:1");
        Hyperlinker::disabled().link(&mut text);
        assert!(text.spans().is_empty());
    }
}
