//! Asset names: `[a-z0-9_-]` segments joined by `/` namespaces.

use crate::error::MicroError;

/// The longest name accepted, in bytes.
pub const MAX_NAME_LEN: usize = 128;

/// Whether `name` is a valid asset name: one or more non-empty segments of
/// lowercase ASCII letters, digits, `_` and `-`, joined by `/`
/// (`rocket`, `status/success`, `team/ci/build-ok`).
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
        })
}

/// [`is_valid_name`] as a `Result` whose error says what is allowed.
pub fn check_name(name: &str) -> Result<(), MicroError> {
    if is_valid_name(name) {
        Ok(())
    } else {
        Err(MicroError::InvalidName(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for good in ["rocket", "status/success", "a/b/c", "x-1_y", "0"] {
            assert!(is_valid_name(good), "{good}");
        }
        for bad in [
            "", "Rocket", "a//b", "/a", "a/", "a b", "a.b", "a:b", "ä", "../x", "a\\b",
        ] {
            assert!(!is_valid_name(bad), "{bad}");
        }
        assert!(!is_valid_name(&"a".repeat(MAX_NAME_LEN + 1)));
    }
}
