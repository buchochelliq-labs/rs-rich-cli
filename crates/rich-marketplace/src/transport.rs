//! Downloads, behind a small trait so tests never touch the network.

use std::collections::HashMap;

use crate::error::FetchError;

/// Fetches a URL's body.
///
/// An implementation refuses anything but `https://`, redirects included,
/// and never returns more than `max_bytes`: a longer body is
/// [`FetchError::TooLarge`], not a truncated one.
pub trait Transport {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FetchError>;
}

impl<T: Transport + ?Sized> Transport for &T {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
        (**self).get(url, max_bytes)
    }
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
        (**self).get(url, max_bytes)
    }
}

/// Refuse a URL that is not `https://`.
pub fn require_https(url: &str) -> Result<(), FetchError> {
    match url.strip_prefix("https://") {
        Some(rest) if !rest.is_empty() => Ok(()),
        _ => Err(FetchError::NotHttps {
            url: url.to_string(),
        }),
    }
}

/// Serves fixed bodies by URL, from memory: for tests, and for a host that
/// has its files some other way.
#[derive(Clone, Debug, Default)]
pub struct MemoryTransport {
    files: HashMap<String, Vec<u8>>,
}

impl MemoryTransport {
    pub fn new() -> Self {
        MemoryTransport::default()
    }

    /// Serve `body` at `url`.
    pub fn insert(&mut self, url: impl Into<String>, body: impl Into<Vec<u8>>) {
        self.files.insert(url.into(), body.into());
    }

    /// The same, by value.
    pub fn with(mut self, url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        self.insert(url, body);
        self
    }
}

impl Transport for MemoryTransport {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
        require_https(url)?;
        let body = self.files.get(url).ok_or_else(|| FetchError::NotFound {
            url: url.to_string(),
        })?;
        if body.len() as u64 > max_bytes {
            return Err(FetchError::TooLarge {
                url: url.to_string(),
                limit: max_bytes,
            });
        }
        Ok(body.clone())
    }
}

/// Downloads with ureq (rustls): HTTPS only, redirects included, a
/// 60-second overall timeout, and the body read to at most `max_bytes`
/// after any content decoding. The system's certificate store is trusted
/// first and Mozilla's bundled roots are the fallback ([`crate::tls`]);
/// redirects (at most five) are followed by [`crate::redirect`], not by
/// ureq.
/// The redirects [`HttpsTransport`] follows (GitHub's release downloads
/// take one).
#[cfg(feature = "http")]
const MAX_REDIRECTS: u32 = 5;

#[cfg(feature = "http")]
#[derive(Debug)]
pub struct HttpsTransport {
    /// One agent per [`Trust`](crate::tls::Trust), in
    /// [`Trust::ORDER`](crate::tls::Trust::ORDER).
    agents: [ureq::Agent; 2],
}

#[cfg(feature = "http")]
impl HttpsTransport {
    pub fn new() -> Self {
        let agent = |trust: crate::tls::Trust| {
            let config = ureq::Agent::config_builder()
                .https_only(true)
                // Followed by `redirect::get_following`, five at most.
                .max_redirects(0)
                .timeout_global(Some(std::time::Duration::from_secs(60)))
                .user_agent(concat!("rs-rich-marketplace/", env!("CARGO_PKG_VERSION")))
                .tls_config(trust.tls_config())
                .build();
            ureq::Agent::new_with_config(config)
        };
        HttpsTransport {
            agents: crate::tls::Trust::ORDER.map(agent),
        }
    }

    fn agent(&self, trust: crate::tls::Trust) -> &ureq::Agent {
        match trust {
            crate::tls::Trust::System => &self.agents[0],
            crate::tls::Trust::Bundled => &self.agents[1],
        }
    }
}

#[cfg(feature = "http")]
impl Default for HttpsTransport {
    fn default() -> Self {
        HttpsTransport::new()
    }
}

#[cfg(feature = "http")]
impl Transport for HttpsTransport {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
        use std::io::Read;

        require_https(url)?;
        let failed = |message: String| FetchError::Transport {
            url: url.to_string(),
            message,
        };
        let mut response = crate::redirect::get_following(url, MAX_REDIRECTS, true, |hop| {
            crate::tls::with_fallback(|trust| self.agent(trust).get(hop).call())
        })
        .map_err(|e| match e.last() {
            ureq::Error::StatusCode(status) => FetchError::Status {
                url: url.to_string(),
                status: *status,
            },
            ureq::Error::RequireHttpsOnly(_) => FetchError::NotHttps {
                url: url.to_string(),
            },
            _ => failed(e.to_string()),
        })?;
        // Bound the decoded body (as the CLI's fetch does): one byte past
        // the limit shows it is too long.
        let mut body = Vec::new();
        response
            .body_mut()
            .with_config()
            .reader()
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut body)
            .map_err(|e| failed(e.to_string()))?;
        if body.len() as u64 > max_bytes {
            return Err(FetchError::TooLarge {
                url: url.to_string(),
                limit: max_bytes,
            });
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_transport_refuses_http_and_long_bodies() {
        let transport = MemoryTransport::new()
            .with("https://example.test/a.wasm", vec![0u8; 10])
            .with("http://example.test/a.wasm", vec![0u8; 10]);
        assert_eq!(
            transport
                .get("https://example.test/a.wasm", 10)
                .unwrap()
                .len(),
            10
        );
        assert!(matches!(
            transport.get("https://example.test/a.wasm", 9),
            Err(FetchError::TooLarge { limit: 9, .. })
        ));
        assert!(matches!(
            transport.get("http://example.test/a.wasm", 10),
            Err(FetchError::NotHttps { .. })
        ));
        assert!(matches!(
            transport.get("https://example.test/b.wasm", 10),
            Err(FetchError::NotFound { .. })
        ));
    }

    #[cfg(feature = "http")]
    #[test]
    fn https_transport_refuses_plain_http_before_connecting() {
        // Port 9 on localhost: nothing is contacted, the URL is refused first.
        let error = HttpsTransport::new()
            .get("http://127.0.0.1:9/a.wasm", 10)
            .unwrap_err();
        assert!(matches!(error, FetchError::NotHttps { .. }), "{error}");
    }
}
