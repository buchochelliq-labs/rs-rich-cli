//! Following redirects one request at a time, instead of in ureq.
//!
//! ureq 3 (to 3.5.0 at least), when it follows redirects itself and has no
//! cookie store, accepts a redirect whose header block has not finished
//! arriving (a `Location` header is enough: a workaround for servers that
//! never send the final `\r\n`). When the blank line that ends the headers
//! then arrives in a later read, as it does through a proxy that forwards a
//! response in many small TLS records, it is taken for the start of the
//! body, and a chunked body fails: `protocol: chunk length cannot be read
//! as a number`, or `chunk expected crlf as next character`. GitHub's
//! release downloads are such redirects (a chunked `302` to the asset's
//! host). With ureq's own redirects off (`max_redirects(0)`) that shortcut
//! is never taken; [`get_following`] follows them here instead, with the
//! same rules: a bounded number of hops and, when asked, `https://` only.

/// A redirect status ureq would follow: 301, 302, 303, 307 or 308.
fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// `location` resolved against `base`: an absolute URL as it is, a
/// network-path reference (`//host/path`) with `base`'s scheme, an
/// absolute path on `base`'s host, and a relative path beside `base`'s.
pub fn resolve_location(base: &str, location: &str) -> Option<String> {
    let location = location.trim();
    let has_scheme = location.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if has_scheme {
        return Some(location.to_string());
    }
    let (scheme, rest) = base.split_once("://")?;
    if let Some(network_path) = location.strip_prefix("//") {
        return Some(format!("{scheme}://{network_path}"));
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    let path = path.split(['?', '#']).next().unwrap_or("");
    if location.starts_with(['?', '#']) {
        // Only a query or fragment: on `base`'s own path.
        return Some(format!("{scheme}://{authority}{path}{location}"));
    }
    let (target, tail) = location.split_at(location.find(['?', '#']).unwrap_or(location.len()));
    let merged = if target.starts_with('/') {
        target.to_string()
    } else {
        // A relative reference: beside the last segment of `base`'s path.
        let directory = match path.rfind('/') {
            Some(slash) => &path[..=slash],
            None => "/",
        };
        format!("{directory}{target}")
    };
    Some(format!(
        "{scheme}://{authority}{}{tail}",
        remove_dot_segments(&merged)
    ))
}

/// RFC 3986's `remove_dot_segments` (section 5.2.4) on an absolute path:
/// `.` segments go, and `..` takes the segment before it with it.
fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let last = segments.len().saturating_sub(1);
    for (i, segment) in segments.iter().enumerate() {
        match *segment {
            "." => {
                if i == last {
                    out.push("");
                }
            }
            ".." => {
                out.pop();
                if i == last {
                    out.push("");
                }
            }
            other => out.push(other),
        }
    }
    format!("/{}", out.join("/"))
}

/// The time left to a redirect chain: [`get_following`] starts it, and each
/// request (a fallback attempt and the body read included) gets only what
/// remains, so the chain as a whole keeps to one overall timeout.
#[derive(Clone, Copy, Debug)]
pub struct Deadline(std::time::Instant);

impl Deadline {
    /// A deadline `budget` from now.
    pub fn after(budget: std::time::Duration) -> Deadline {
        Deadline(std::time::Instant::now() + budget)
    }

    /// The time left, or ureq's global timeout error once none is.
    pub fn remaining(&self) -> Result<std::time::Duration, ureq::Error> {
        let left = self.0.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        } else {
            Ok(left)
        }
    }
}

/// GET `url` through `hop`, which makes one request with ureq's own
/// redirects off (`max_redirects(0)`) and its global timeout set to the
/// [`Deadline::remaining`] time, and follow at most `max_redirects`
/// redirects here, all within `budget`. With `https_only`, a redirect to
/// anything but `https://` is [`ureq::Error::RequireHttpsOnly`]; one hop
/// too many is [`ureq::Error::TooManyRedirects`]; a chain past its budget
/// is ureq's global timeout. A redirect without a `Location` is returned
/// as it is, as ureq does.
pub fn get_following<E>(
    url: &str,
    max_redirects: u32,
    https_only: bool,
    budget: std::time::Duration,
    mut hop: impl FnMut(&str, Deadline) -> Result<ureq::http::Response<ureq::Body>, E>,
) -> Result<ureq::http::Response<ureq::Body>, E>
where
    E: From<ureq::Error>,
{
    let deadline = Deadline::after(budget);
    let mut current = url.to_string();
    let mut redirects = 0;
    loop {
        deadline.remaining()?;
        let response = hop(&current, deadline)?;
        if !is_redirect(response.status().as_u16()) {
            return Ok(response);
        }
        let Some(location) = response
            .headers()
            .get_all(ureq::http::header::LOCATION)
            .into_iter()
            .next_back()
            .and_then(|value| value.to_str().ok())
        else {
            return Ok(response);
        };
        let Some(next) = resolve_location(&current, location) else {
            return Err(ureq::Error::RedirectFailed.into());
        };
        if redirects >= max_redirects {
            return Err(ureq::Error::TooManyRedirects.into());
        }
        if https_only
            && !next
                .get(..8)
                .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
        {
            return Err(ureq::Error::RequireHttpsOnly(next).into());
        }
        redirects += 1;
        current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

    fn response(status: u16, location: Option<&str>) -> ureq::http::Response<ureq::Body> {
        let mut builder = ureq::http::Response::builder().status(status);
        if let Some(location) = location {
            builder = builder.header("location", location);
        }
        builder.body(ureq::Body::builder().data("")).unwrap()
    }

    #[test]
    fn locations_resolve_against_the_current_url() {
        let base = "https://github.com/o/r/releases/download/v1/a.ini?x=1";
        for (location, expected) in [
            ("https://cdn.test/a?sig=1", "https://cdn.test/a?sig=1"),
            ("//cdn.test/a", "https://cdn.test/a"),
            ("/b/c", "https://github.com/b/c"),
            ("b.ini", "https://github.com/o/r/releases/download/v1/b.ini"),
            (
                "?y=2",
                "https://github.com/o/r/releases/download/v1/a.ini?y=2",
            ),
        ] {
            assert_eq!(
                resolve_location(base, location).as_deref(),
                Some(expected),
                "{location}"
            );
        }
        assert_eq!(
            resolve_location("https://host.test", "a").as_deref(),
            Some("https://host.test/a")
        );
        // Dot segments are removed (RFC 3986, 5.2.4), the query kept.
        let base = "https://host.test/releases/v1/index.json";
        for (location, expected) in [
            ("../asset.ini", "https://host.test/releases/asset.ini"),
            ("./a/../b?x=../y", "https://host.test/releases/v1/b?x=../y"),
            ("/a/./b/../c", "https://host.test/a/c"),
            ("../../../..", "https://host.test/"),
            ("..", "https://host.test/releases/"),
            ("sub/.", "https://host.test/releases/v1/sub/"),
        ] {
            assert_eq!(
                resolve_location(base, location).as_deref(),
                Some(expected),
                "{location}"
            );
        }
    }

    #[test]
    fn a_chain_keeps_to_one_budget() {
        // Each hop sees less time than the last, and a chain past its
        // budget stops with the global timeout, not one more request.
        let mut seen = Vec::new();
        let error = get_following(
            "https://a.test/x",
            10,
            true,
            std::time::Duration::from_millis(120),
            |_, deadline| {
                seen.push(deadline.remaining()?);
                std::thread::sleep(std::time::Duration::from_millis(50));
                Ok::<_, ureq::Error>(response(302, Some("/again")))
            },
        )
        .unwrap_err();
        assert!(matches!(error, ureq::Error::Timeout(ureq::Timeout::Global)));
        assert!(seen.len() <= 3, "{seen:?}");
        assert!(seen.windows(2).all(|w| w[1] < w[0]), "{seen:?}");
    }

    #[test]
    fn redirects_are_followed_within_the_rules() {
        // Two hops, then the body.
        let mut asked = Vec::new();
        let got = get_following("https://a.test/x", 5, true, BUDGET, |url, _| {
            asked.push(url.to_string());
            Ok::<_, ureq::Error>(match url {
                "https://a.test/x" => response(302, Some("/y")),
                "https://a.test/y" => response(301, Some("https://b.test/z")),
                _ => response(200, None),
            })
        })
        .unwrap();
        assert_eq!(got.status(), 200);
        assert_eq!(
            asked,
            ["https://a.test/x", "https://a.test/y", "https://b.test/z"]
        );

        // HTTPS only refuses a downgrade.
        let error = get_following("https://a.test/x", 5, true, BUDGET, |_, _| {
            Ok::<_, ureq::Error>(response(302, Some("http://a.test/y")))
        })
        .unwrap_err();
        assert!(matches!(error, ureq::Error::RequireHttpsOnly(ref u) if u == "http://a.test/y"));
        // Without it, the downgrade is followed.
        let got = get_following("https://a.test/x", 5, false, BUDGET, |url, _| {
            Ok::<_, ureq::Error>(if url.starts_with("https") {
                response(302, Some("http://a.test/y"))
            } else {
                response(200, None)
            })
        })
        .unwrap();
        assert_eq!(got.status(), 200);

        // One hop past the limit.
        let mut hops = 0;
        let error = get_following("https://a.test/x", 2, true, BUDGET, |_, _| {
            hops += 1;
            Ok::<_, ureq::Error>(response(307, Some("/again")))
        })
        .unwrap_err();
        assert!(matches!(error, ureq::Error::TooManyRedirects));
        assert_eq!(hops, 3);

        // A redirect without a location, or another status, is the answer.
        for (status, location) in [(302, None), (404, Some("/elsewhere")), (304, Some("/x"))] {
            let got = get_following("https://a.test/x", 5, true, BUDGET, |_, _| {
                Ok::<_, ureq::Error>(response(status, location))
            })
            .unwrap();
            assert_eq!(got.status(), status);
        }
    }
}
