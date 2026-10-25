//! Addresses: local files as `file://` URLs, links resolved against their
//! page, queries escaped, and what the address bar means by what was typed.

use std::io;
use std::path::Path;

/// The `file://` address of `path`.
pub fn file_url(path: &Path) -> io::Result<String> {
    let path = path.canonicalize()?;
    let mut url = String::from("file://");
    let text = path.to_string_lossy().replace('\\', "/");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    Ok(url)
}

/// `text` percent-encoded for a query string: everything but unreserved
/// characters is escaped, a space as `+`.
pub fn encode_query(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The scheme of `url`, if it has one (`https`, `file`, `about`).
pub fn scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    (chars.next()?.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

/// The host of `url` (`github.com`), lower case, without a port or user.
pub fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if authority.starts_with('[') {
        authority.split(']').next()?.trim_start_matches('[')
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// `href` resolved against `base`, as a browser resolves a link.
pub fn resolve(base: &str, href: &str) -> String {
    let href = href.trim();
    if scheme(href).is_some() {
        return href.to_string();
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return href.to_string();
    };
    let (authority, path_etc) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    if let Some(net) = href.strip_prefix("//") {
        return format!("{scheme}://{net}");
    }
    let path_only = path_etc.split(['?', '#']).next().unwrap_or("/");
    if href.is_empty() {
        return base.split('#').next().unwrap_or(base).to_string();
    }
    if href.starts_with('#') {
        let without = base.split('#').next().unwrap_or(base);
        return format!("{without}{href}");
    }
    if href.starts_with('?') {
        return format!("{scheme}://{authority}{path_only}{href}");
    }
    let (href_path, tail) = match href.find(['?', '#']) {
        Some(at) => (&href[..at], &href[at..]),
        None => (href, ""),
    };
    let joined = if href_path.starts_with('/') {
        href_path.to_string()
    } else {
        let dir = &path_only[..path_only.rfind('/').map_or(0, |i| i + 1)];
        format!("{dir}{href_path}")
    };
    let mut parts: Vec<&str> = Vec::new();
    let segments: Vec<&str> = joined.split('/').collect();
    for (i, segment) in segments.iter().enumerate() {
        match *segment {
            "." => {
                if i == segments.len() - 1 {
                    parts.push("");
                }
            }
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
                if i == segments.len() - 1 {
                    parts.push("");
                }
            }
            other => parts.push(other),
        }
    }
    let mut path = parts.join("/");
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    format!("{scheme}://{authority}{path}{tail}")
}

/// Whether `text`, typed in the address bar, looks like an address rather
/// than words: a scheme, a path, `localhost`, or a host with a dot and no
/// spaces (`example.com`, `docs.rs/serde`).
pub fn looks_like_address(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return false;
    }
    if text.contains("://") || matches!(scheme(text), Some("about" | "data" | "file" | "mailto")) {
        return true;
    }
    if text.starts_with(['/', '.', '~']) {
        return true;
    }
    let host = text.split(['/', '?', '#']).next().unwrap_or_default();
    let name = host.rsplit_once(':').map_or(host, |(name, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            name
        } else {
            host
        }
    });
    name == "localhost"
        || name.parse::<std::net::Ipv4Addr>().is_ok()
        || (name.contains('.')
            && !name.starts_with('.')
            && !name.ends_with('.')
            && name
                .split('.')
                .next_back()
                .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())))
}

/// An address typed without a scheme, made whole: a path becomes a
/// `file://` address, `localhost` and bare IPs get `http://`, anything
/// else `https://`.
pub fn complete_address(text: &str) -> String {
    let text = text.trim();
    if text.contains("://")
        || scheme(text).is_some_and(|s| matches!(s, "about" | "data" | "mailto"))
    {
        return text.to_string();
    }
    if text.starts_with(['/', '.', '~']) {
        let expanded = match text.strip_prefix('~') {
            Some(rest) => std::env::var("HOME").unwrap_or_default() + rest,
            None => text.to_string(),
        };
        if let Ok(url) = file_url(Path::new(&expanded)) {
            return url;
        }
        return format!("file://{expanded}");
    }
    let host = text.split(['/', ':']).next().unwrap_or_default();
    if host == "localhost" || host.parse::<std::net::Ipv4Addr>().is_ok() {
        format!("http://{text}")
    } else {
        format!("https://{text}")
    }
}

/// Decode a percent-escaped string, as an address shows it.
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_resolve_as_a_browser_resolves_them() {
        let base = "https://a.example/docs/x/page.html?q=1#top";
        assert_eq!(
            resolve(base, "other.html"),
            "https://a.example/docs/x/other.html"
        );
        assert_eq!(
            resolve(base, "../up.html"),
            "https://a.example/docs/up.html"
        );
        assert_eq!(resolve(base, "/root"), "https://a.example/root");
        assert_eq!(resolve(base, "//cdn.example/a"), "https://cdn.example/a");
        assert_eq!(
            resolve(base, "#frag"),
            "https://a.example/docs/x/page.html?q=1#frag"
        );
        assert_eq!(
            resolve(base, "?z=2"),
            "https://a.example/docs/x/page.html?z=2"
        );
        assert_eq!(resolve(base, "mailto:a@b"), "mailto:a@b");
        assert_eq!(
            resolve("file:///home/u/f/a.html", "b.html?x=1"),
            "file:///home/u/f/b.html?x=1"
        );
        assert_eq!(resolve("https://a.example", "b"), "https://a.example/b");
    }

    #[test]
    fn hosts_and_schemes() {
        assert_eq!(
            host("https://User@GitHub.com:443/x").as_deref(),
            Some("github.com")
        );
        assert_eq!(host("file:///tmp/x"), None);
        assert_eq!(scheme("about:blank"), Some("about"));
        assert_eq!(scheme("gh rs-rich"), None);
    }

    #[test]
    fn queries_are_escaped() {
        assert_eq!(encode_query("a b&c=d/é"), "a+b%26c%3Dd%2F%C3%A9");
        assert_eq!(decode("a%20b%2"), "a b%2");
    }

    #[test]
    fn what_looks_like_an_address() {
        for yes in [
            "https://x",
            "example.com",
            "docs.rs/serde",
            "localhost:8080",
            "127.0.0.1/x",
            "/tmp/a.html",
            "./a.html",
            "about:blank",
        ] {
            assert!(looks_like_address(yes), "{yes}");
        }
        for no in [
            "gh rs-rich",
            "rust",
            "what is rust",
            "a.b c",
            "v1.2",
            "",
            "e.g.",
        ] {
            assert!(!looks_like_address(no), "{no}");
        }
        assert_eq!(complete_address("example.com/x"), "https://example.com/x");
        assert_eq!(complete_address("localhost:3000"), "http://localhost:3000");
    }
}
