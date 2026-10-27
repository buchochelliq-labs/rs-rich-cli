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
    if location.starts_with('/') {
        return Some(format!("{scheme}://{authority}{location}"));
    }
    let path = path.split(['?', '#']).next().unwrap_or("");
    if location.starts_with(['?', '#']) {
        // Only a query or fragment: on `base`'s own path.
        return Some(format!("{scheme}://{authority}{path}{location}"));
    }
    // A relative reference: beside the last segment of `base`'s path.
    let directory = match path.rfind('/') {
        Some(slash) => &path[..=slash],
        None => "/",
    };
    Some(format!("{scheme}://{authority}{directory}{location}"))
}

/// GET `url` through `hop`, which makes one request with ureq's own
/// redirects off (`max_redirects(0)`), and follow at most `max_redirects`
/// redirects here. With `https_only`, a redirect to anything but
/// `https://` is [`ureq::Error::RequireHttpsOnly`]; one hop too many is
/// [`ureq::Error::TooManyRedirects`]. A redirect without a `Location` is
/// returned as it is, as ureq does.
pub fn get_following<E>(
    url: &str,
    max_redirects: u32,
    https_only: bool,
    mut hop: impl FnMut(&str) -> Result<ureq::http::Response<ureq::Body>, E>,
) -> Result<ureq::http::Response<ureq::Body>, E>
where
    E: From<ureq::Error>,
{
    let mut current = url.to_string();
    let mut redirects = 0;
    loop {
        let response = hop(&current)?;
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
    }

    #[test]
    fn redirects_are_followed_within_the_rules() {
        // Two hops, then the body.
        let mut asked = Vec::new();
        let got = get_following("https://a.test/x", 5, true, |url| {
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
        let error = get_following("https://a.test/x", 5, true, |_| {
            Ok::<_, ureq::Error>(response(302, Some("http://a.test/y")))
        })
        .unwrap_err();
        assert!(matches!(error, ureq::Error::RequireHttpsOnly(ref u) if u == "http://a.test/y"));
        // Without it, the downgrade is followed.
        let got = get_following("https://a.test/x", 5, false, |url| {
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
        let error = get_following("https://a.test/x", 2, true, |_| {
            hops += 1;
            Ok::<_, ureq::Error>(response(307, Some("/again")))
        })
        .unwrap_err();
        assert!(matches!(error, ureq::Error::TooManyRedirects));
        assert_eq!(hops, 3);

        // A redirect without a location, or another status, is the answer.
        for (status, location) in [(302, None), (404, Some("/elsewhere")), (304, Some("/x"))] {
            let got = get_following("https://a.test/x", 5, true, |_| {
                Ok::<_, ureq::Error>(response(status, location))
            })
            .unwrap();
            assert_eq!(got.status(), status);
        }
    }
}
