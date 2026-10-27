//! The marketplace's `stats.json`: download counts and ratings per package,
//! shown by `rich plugin info` and used by `rich plugin search --sort
//! downloads` (docs/design/marketplace.md, "Stats").
//!
//! The file is written nightly by the marketplace repository's `stats.yml`
//! to its `stats` branch: for the official marketplace,
//! [`OFFICIAL_STATS`], the marketplace's base URL with `stats` in place of
//! `main`. Another `https://` marketplace serves it as `stats.json` beside
//! its index, and a local one from its directory. It is not signed and not
//! in the index, so it is display-only: nothing here decides what is
//! installed or trusted. It is read with a short timeout, and a missing,
//! slow or malformed file is silently left out. Reading it sends nothing
//! but the request for the file.
use super::*;

/// Where the official marketplace's stats are.
pub(crate) const OFFICIAL_STATS: &str =
    "https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/stats/stats.json";

/// The schema version this reads; another is ignored.
const STATS_SCHEMA_VERSION: u64 = 1;
/// The most read of `stats.json`.
const MAX_STATS_BYTES: u64 = 4 << 20;
/// How long a remote `stats.json` may take, all in.
#[cfg(feature = "fetch")]
const STATS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The marketplace's stats, as read.
pub(crate) struct Stats {
    /// Where they came from.
    pub source: String,
    pub generated_at: String,
    packages: serde_json::Map<String, Value>,
}

/// Where `location`'s `stats.json` is: a local marketplace's own file; for
/// an `https://` base ending in `/main/`, the same base on the `stats`
/// branch; else beside the index.
pub(crate) fn stats_source(location: &Location) -> String {
    match location {
        Location::Local(root) => root.join("stats.json").display().to_string(),
        Location::Remote(base) if base == DEFAULT_MARKETPLACE => OFFICIAL_STATS.to_string(),
        Location::Remote(base) => match base.strip_suffix("/main/") {
            Some(prefix) => format!("{prefix}/stats/stats.json"),
            None => format!("{base}stats.json"),
        },
    }
}

/// Read the stats, or `None` when they are missing, slow, too large or not
/// what this version reads.
pub(crate) fn load(location: &Location) -> Option<Stats> {
    let source = stats_source(location);
    let bytes = match location {
        Location::Local(root) => read_bounded(&root.join("stats.json"), MAX_STATS_BYTES)
            .ok()
            .flatten()?,
        Location::Remote(_) => fetch_remote(&source)?,
    };
    parse(&bytes, source)
}

#[cfg(feature = "fetch")]
fn fetch_remote(url: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    rich_marketplace::require_https(url).ok()?;
    // The system's certificates first, the bundled roots as the fallback;
    // redirects (three at most) followed one request at a time.
    let mut response = rich_marketplace::redirect::get_following(url, 3, true, |hop| {
        rich_marketplace::tls::with_fallback(|trust| {
            let config = ureq::Agent::config_builder()
                .https_only(true)
                .max_redirects(0)
                .timeout_global(Some(STATS_TIMEOUT))
                .user_agent("rs-rich-marketplace-stats")
                .tls_config(trust.tls_config())
                .build();
            ureq::Agent::new_with_config(config).get(hop).call()
        })
    })
    .ok()?;
    let mut body = Vec::new();
    response
        .body_mut()
        .with_config()
        .reader()
        .take(MAX_STATS_BYTES + 1)
        .read_to_end(&mut body)
        .ok()?;
    (body.len() as u64 <= MAX_STATS_BYTES).then_some(body)
}

#[cfg(not(feature = "fetch"))]
fn fetch_remote(_: &str) -> Option<Vec<u8>> {
    None
}

fn parse(bytes: &[u8], source: String) -> Option<Stats> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if value.get("schema_version")?.as_u64()? != STATS_SCHEMA_VERSION {
        return None;
    }
    Some(Stats {
        source,
        generated_at: value.get("generated_at")?.as_str()?.to_string(),
        packages: value.get("packages")?.as_object()?.clone(),
    })
}

/// A count: a non-negative integer, else not counted.
fn count(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

impl Stats {
    /// One package's downloads in total, when counted.
    pub(crate) fn total(&self, id: &str) -> Option<u64> {
        count(self.packages.get(id)?.pointer("/downloads/total"))
    }

    /// One package's stats as the JSON report shows them, cleaned: counts
    /// are integers or null, and a review link only one into the
    /// marketplace's own Discussions ([`review_link`]).
    pub(crate) fn package_json(&self, id: &str) -> Value {
        let Some(entry) = self.packages.get(id) else {
            return json!({"source": self.source, "generated_at": self.generated_at,
                          "downloads": Value::Null, "reviews": Value::Null});
        };
        let versions: serde_json::Map<String, Value> = entry
            .pointer("/downloads/versions")
            .and_then(Value::as_object)
            .map(|versions| {
                versions
                    .iter()
                    .map(|(version, n)| (line(version), json!(count(Some(n)))))
                    .collect()
            })
            .unwrap_or_default();
        let reviews = entry.get("reviews").and_then(|r| {
            let url = review_link(r.get("url")?.as_str()?)?;
            Some(json!({
                "url": line(&url),
                "up": count(r.get("up")).unwrap_or(0),
                "down": count(r.get("down")).unwrap_or(0),
                "comments": count(r.get("comments")).unwrap_or(0),
            }))
        });
        json!({
            "source": self.source,
            "generated_at": self.generated_at,
            "downloads": {"total": self.total(id), "versions": versions},
            "reviews": reviews,
        })
    }

    /// `info`'s rows for one package: downloads (per version, in the
    /// order of `versions`, the listing's), rating and reviews.
    pub(crate) fn rows(&self, id: &str, versions: &[String]) -> Vec<(&'static str, String)> {
        let value = self.package_json(id);
        let mut rows = Vec::new();
        if let Some(total) = value.pointer("/downloads/total").and_then(Value::as_u64) {
            let counted = value.pointer("/downloads/versions");
            let per_version: Vec<String> = versions
                .iter()
                .map(
                    |version| match counted.and_then(|c| c.get(version)).and_then(Value::as_u64) {
                        Some(n) => format!("{version}: {}", thousands(n)),
                        None => format!("{version}: not counted"),
                    },
                )
                .collect();
            rows.push((
                "Downloads",
                format!("{} ({})", thousands(total), per_version.join(", ")),
            ));
        }
        if let Some(reviews) = value.get("reviews").filter(|r| !r.is_null()) {
            let n = |key: &str| reviews.get(key).and_then(Value::as_u64).unwrap_or(0);
            let comments = n("comments");
            rows.push((
                "Rating",
                format!(
                    "👍 {}  👎 {}  ({comments} comment{})",
                    n("up"),
                    n("down"),
                    if comments == 1 { "" } else { "s" }
                ),
            ));
            if let Some(url) = reviews.get("url").and_then(Value::as_str) {
                rows.push(("Reviews", url.to_string()));
            }
        }
        if !rows.is_empty() {
            let date = self.generated_at.split('T').next().unwrap_or_default();
            rows.push((
                "Stats",
                format!("as of {}; not signed, shown only", line(date)),
            ));
        }
        rows
    }
}

/// Where every review link must point: the official marketplace's own
/// Discussions. The docs' Marketplace page (`docs/assets/marketplace.js`,
/// `readStats`) holds links to the same rule.
const REVIEWS: &str = "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/";

/// A review link from `stats.json`, if it may be shown: it starts with
/// [`REVIEWS`] as written and still does once normalised as a browser
/// would (dot segments, `%2e` spelled dots and `\` resolved), and has no
/// query, fragment, `..` or `%2e%2e`. Anything else is not shown, so an
/// unsigned file cannot send a reader elsewhere.
fn review_link(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    if !raw.starts_with(REVIEWS)
        || raw.contains(['?', '#', '\\'])
        || raw.contains("..")
        || lower.contains("%2e%2e")
        || lower.contains(".%2e")
        || lower.contains("%2e.")
        || raw.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    let normalised = normalise_path(raw);
    normalised.starts_with(REVIEWS).then_some(normalised)
}

/// `url`'s path with its dot segments removed, as the WHATWG URL parser
/// does for an `https://` URL: a `.` or `%2e` segment goes, and a `..` in
/// any spelling takes the segment before it too.
fn normalise_path(url: &str) -> String {
    let Some(rest) = url.strip_prefix("https://") else {
        return url.to_string();
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    for (i, segment) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match segment.to_ascii_lowercase().as_str() {
            "." | "%2e" => {
                if last {
                    out.push("");
                }
            }
            ".." | ".%2e" | "%2e." | "%2e%2e" => {
                out.pop();
                if last {
                    out.push("");
                }
            }
            _ => out.push(segment),
        }
    }
    format!("https://{host}/{}", out.join("/"))
}

/// `1234567` as `1,234,567`.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_follows_the_marketplace() {
        let official = Location::parse(DEFAULT_MARKETPLACE).unwrap();
        assert_eq!(stats_source(&official), OFFICIAL_STATS);
        let other = Location::parse("https://example.test/market").unwrap();
        assert_eq!(
            stats_source(&other),
            "https://example.test/market/stats.json"
        );
    }

    #[test]
    fn counts_are_cleaned() {
        let stats = parse(
            br#"{"schema_version": 1, "generated_at": "2026-10-27T03:41:07Z",
                 "packages": {"a": {"downloads": {"total": 1234,
                                    "versions": {"1.0.0": 1234, "0.9.0": null, "0.8.0": -3}},
                                    "reviews": {"url": "https://evil.test/", "up": 1,
                                                "down": 0, "comments": 0}}}}"#,
            "here".into(),
        )
        .unwrap();
        let value = stats.package_json("a");
        assert_eq!(value["downloads"]["versions"]["0.8.0"], Value::Null);
        assert_eq!(value["reviews"], Value::Null);
        let versions = ["1.0.0", "0.9.0", "0.8.0"].map(String::from);
        assert_eq!(
            stats.rows("a", &versions)[0].1,
            "1,234 (1.0.0: 1,234, 0.9.0: not counted, 0.8.0: not counted)"
        );
        assert_eq!(stats.package_json("b")["downloads"], Value::Null);
        assert!(parse(br#"{"schema_version": 2}"#, String::new()).is_none());
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
    }

    #[test]
    fn a_review_link_stays_in_the_marketplaces_discussions() {
        let good = "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7";
        assert_eq!(review_link(good).as_deref(), Some(good));
        for hostile in [
            "https://evil.test/",
            "https://github.com/someone-else/rs-rich-marketplace/discussions/7",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/issues/7",
            "http://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7",
            "https://github.com.evil.test/buchochelliq-labs/rs-rich-marketplace/discussions/7",
            // Out of the discussions after normalising.
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/../../evil/repo",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/%2e%2e/%2E%2E/x",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/.%2e/settings",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/%2E./settings",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/..",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7\\..\\..\\x",
            // Queries and fragments.
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7?next=https://evil.test",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7#x",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/?",
            // Hidden characters.
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7\u{1b}]8;;https://evil.test\u{7}",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7 https://evil.test",
            "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/.\t./x",
            " https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7",
        ] {
            assert_eq!(review_link(hostile), None, "{hostile:?}");
        }
        // A single dot segment normalises away and stays inside.
        assert_eq!(
            review_link("https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/./7")
                .as_deref(),
            Some(good)
        );
        assert_eq!(
            normalise_path("https://h/a/b/%2e%2e/c/./d/.."),
            "https://h/a/c/"
        );
    }
}
