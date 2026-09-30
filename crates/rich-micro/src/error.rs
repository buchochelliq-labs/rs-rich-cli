//! Errors.

use std::fmt;

/// Why an asset, a package or a lookup was rejected. Messages name the
/// offending value and, where it helps, what would be accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicroError {
    /// A name outside `[a-z0-9_-]` segments joined by `/`.
    InvalidName(String),
    /// A cell size that is not allowed (zero, too wide, or more than one row).
    InvalidSize(String),
    /// Alt text is mandatory and must be printable.
    InvalidAlt(String),
    /// A fallback that does not fit the asset's cells, or holds whitespace or
    /// control characters.
    InvalidFallback(String),
    /// A malformed or unsupported manifest or pack file.
    Manifest(String),
    /// A package that breaks a hard limit (sizes, dimensions, frames).
    Limit(String),
    /// A path that escapes the package: absolute, `..`, or a link out of it.
    UnsafePath(String),
    /// An image whose header could not be read or whose format is not allowed.
    Image(String),
    /// No asset of this name in any layer.
    UnknownAsset(String),
    /// An I/O or archive error, with the path it happened on.
    Io(String),
}

impl fmt::Display for MicroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MicroError::InvalidName(name) => write!(
                f,
                "invalid micro asset name {name:?}: use lowercase letters, digits, `_` and `-`, \
                 with `/` between namespaces"
            ),
            MicroError::InvalidSize(message)
            | MicroError::InvalidAlt(message)
            | MicroError::InvalidFallback(message)
            | MicroError::Manifest(message)
            | MicroError::Limit(message)
            | MicroError::UnsafePath(message)
            | MicroError::Image(message)
            | MicroError::Io(message) => f.write_str(message),
            MicroError::UnknownAsset(name) => write!(f, "unknown micro asset {name:?}"),
        }
    }
}

impl std::error::Error for MicroError {}
