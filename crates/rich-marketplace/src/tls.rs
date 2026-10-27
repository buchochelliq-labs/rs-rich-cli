//! TLS trust for HTTPS downloads: the system's certificate store first,
//! Mozilla's bundled roots as the fallback.
//!
//! The one place that chooses, for [`HttpsTransport`](crate::HttpsTransport)
//! and for the CLI's other ureq clients (`rich <url>`, the marketplace's
//! download statistics). A company network's TLS-inspecting proxy, or a
//! sandbox's, presents certificates from a root CA that is in the system's
//! store but in no bundled list; the bundled roots alone refuse it
//! (`invalid peer certificate: UnknownIssuer`). The system store is tried
//! first (ureq's platform verifier: the OS verifier on Windows and macOS,
//! the system's CA files on Linux, where `SSL_CERT_FILE` and
//! `SSL_CERT_DIR` are honoured); when that attempt fails at the transport
//! or TLS level, the request is made once more with the bundled roots, so
//! a machine without a usable store still works.
//!
//! TLS is transport only: the marketplace's trust comes from its minisign
//! signatures and SHA-256 pins, which are checked the same either way.

use std::fmt;

use ureq::tls::{RootCerts, TlsConfig};

/// Where a connection's root certificates come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Trust {
    /// The operating system's certificate store (ureq's platform verifier).
    System,
    /// Mozilla's roots, bundled in the binary (webpki-roots).
    Bundled,
}

impl Trust {
    /// The order a request tries them in.
    pub const ORDER: [Trust; 2] = [Trust::System, Trust::Bundled];

    /// ureq's TLS configuration for this trust (rustls, verification on).
    pub fn tls_config(self) -> TlsConfig {
        let roots = match self {
            Trust::System => RootCerts::PlatformVerifier,
            Trust::Bundled => RootCerts::WebPki,
        };
        TlsConfig::builder().root_certs(roots).build()
    }

    /// How an error message names it.
    pub fn describe(self) -> &'static str {
        match self {
            Trust::System => "the system's certificates",
            Trust::Bundled => "the bundled root certificates",
        }
    }
}

/// A request that failed: with the system's certificates, and, when that
/// failure was one other roots could change, with the bundled roots too.
/// (Boxed: ureq's errors are large.)
#[derive(Debug)]
pub struct TrustError {
    /// The first attempt's error (with [`Trust::System`]).
    pub system: Box<ureq::Error>,
    /// The fallback's error, if the request was retried.
    pub bundled: Option<Box<ureq::Error>>,
}

impl TrustError {
    /// The error that decides the outcome: the fallback's when there was
    /// one (it got furthest), else the first attempt's.
    pub fn last(&self) -> &ureq::Error {
        self.bundled.as_deref().unwrap_or(&self.system)
    }

    /// The same, by value.
    pub fn into_last(self) -> ureq::Error {
        *self.bundled.unwrap_or(self.system)
    }
}

impl fmt::Display for TrustError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.bundled {
            None => write!(f, "{}", self.system),
            Some(bundled) => write!(
                f,
                "{} (with {}); {} (with {})",
                self.system,
                Trust::System.describe(),
                bundled,
                Trust::Bundled.describe()
            ),
        }
    }
}

/// An error made before any request (a refused redirect, say): no fallback.
impl From<ureq::Error> for TrustError {
    fn from(system: ureq::Error) -> Self {
        TrustError {
            system: Box::new(system),
            bundled: None,
        }
    }
}

impl std::error::Error for TrustError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.last())
    }
}

/// Whether trying other roots could change `error`'s outcome: a TLS or
/// I/O failure (an unknown issuer arrives as I/O), not a status, a refused
/// `http://` URL, a timeout or a host that does not resolve.
pub fn may_be_trust(error: &ureq::Error) -> bool {
    matches!(
        error,
        ureq::Error::Io(_) | ureq::Error::Tls(_) | ureq::Error::Rustls(_)
    )
}

/// Run `attempt` trusting the system's certificates and, if that fails as
/// [`may_be_trust`] says another set of roots could change, once more with
/// the bundled roots. `attempt` builds its client from
/// [`Trust::tls_config`], so the two tries differ only in their roots.
pub fn with_fallback<T>(
    mut attempt: impl FnMut(Trust) -> Result<T, ureq::Error>,
) -> Result<T, TrustError> {
    let system = match attempt(Trust::System) {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };
    if !may_be_trust(&system) {
        return Err(system.into());
    }
    attempt(Trust::Bundled).map_err(|bundled| TrustError {
        system: Box::new(system),
        bundled: Some(Box::new(bundled)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_first_then_bundled() {
        assert_eq!(Trust::ORDER, [Trust::System, Trust::Bundled]);
        assert!(matches!(
            Trust::System.tls_config().root_certs(),
            RootCerts::PlatformVerifier
        ));
        assert!(matches!(
            Trust::Bundled.tls_config().root_certs(),
            RootCerts::WebPki
        ));
        // Verification stays on for both.
        for trust in Trust::ORDER {
            assert!(!trust.tls_config().disable_verification());
        }
    }

    #[test]
    fn falls_back_only_when_the_roots_could_matter() {
        let unknown_issuer = || {
            ureq::Error::Io(std::io::Error::other(
                "invalid peer certificate: UnknownIssuer",
            ))
        };
        // Success first time: one attempt.
        let mut tried = Vec::new();
        let value = with_fallback(|trust| {
            tried.push(trust);
            Ok::<_, ureq::Error>(7)
        })
        .unwrap();
        assert_eq!((value, tried.as_slice()), (7, &[Trust::System][..]));

        // A TLS failure is retried with the bundled roots.
        let mut tried = Vec::new();
        let value = with_fallback(|trust| {
            tried.push(trust);
            match trust {
                Trust::System => Err(unknown_issuer()),
                Trust::Bundled => Ok(8),
            }
        })
        .unwrap();
        assert_eq!((value, tried.as_slice()), (8, &Trust::ORDER[..]));

        // Both failing names both.
        let error = with_fallback(|_| Err::<(), _>(unknown_issuer())).unwrap_err();
        assert!(error.bundled.is_some());
        let message = error.to_string();
        assert!(
            message.contains("(with the system's certificates)")
                && message.contains("(with the bundled root certificates)"),
            "{message}"
        );

        // A status, a timeout or a plain-http refusal is not retried.
        for error in [
            ureq::Error::StatusCode(404),
            ureq::Error::HostNotFound,
            ureq::Error::RequireHttpsOnly("http://x".into()),
        ] {
            let mut tried = 0;
            let failed = with_fallback(|_| {
                tried += 1;
                Err::<(), _>(match &error {
                    ureq::Error::StatusCode(s) => ureq::Error::StatusCode(*s),
                    ureq::Error::HostNotFound => ureq::Error::HostNotFound,
                    _ => ureq::Error::RequireHttpsOnly("http://x".into()),
                })
            })
            .unwrap_err();
            assert_eq!(tried, 1, "{error}");
            assert!(failed.bundled.is_none());
            assert_eq!(failed.to_string(), error.to_string());
        }
    }
}
