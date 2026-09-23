//! Explicit text decoding for CLI and application inputs.
//!
//! This extension never guesses a headerless encoding. Callers retain their
//! existing default decoding policy unless an encoding is explicitly selected.

use std::io::{Error, ErrorKind, Result};

/// Supported explicit text encodings. All decoding is strict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// Requires a UTF-16 byte-order mark.
    Utf16,
    Utf16Le,
    Utf16Be,
}

impl std::str::FromStr for Encoding {
    type Err = String;

    fn from_str(name: &str) -> std::result::Result<Self, Self::Err> {
        match name.to_ascii_lowercase().as_str() {
            "utf-8" => Ok(Self::Utf8),
            "utf-16" => Ok(Self::Utf16),
            "utf-16le" => Ok(Self::Utf16Le),
            "utf-16be" => Ok(Self::Utf16Be),
            _ => Err(format!(
                "unsupported encoding {name:?}; use utf-8, utf-16, utf-16le or utf-16be"
            )),
        }
    }
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

/// Recognize a likely UTF-16 BOM, excluding ambiguous UTF-32 signatures.
pub fn has_utf16_bom(bytes: &[u8]) -> bool {
    !bytes.starts_with(&[0xff, 0xfe, 0, 0])
        && (bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]))
}

impl Encoding {
    /// Decode bytes, consuming a matching BOM and rejecting malformed data.
    /// `Utf16` requires a BOM; endian-specific variants also accept headerless
    /// data, but reject a contradictory BOM. Generic `Utf16` rejects ambiguous
    /// UTF-32 signatures; select an endian explicitly for BOM + leading NUL.
    /// Newline policy belongs to callers.
    pub fn decode(self, bytes: &[u8]) -> Result<String> {
        if self == Self::Utf8 {
            let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
            return std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| invalid("input is not valid UTF-8"));
        }
        if self == Self::Utf16
            && (bytes.starts_with(&[0xff, 0xfe, 0, 0]) || bytes.starts_with(&[0, 0, 0xfe, 0xff]))
        {
            return Err(invalid(
                "UTF-32 is not supported; convert the input to UTF-8",
            ));
        }
        let bom = if bytes.starts_with(&[0xff, 0xfe]) {
            Some(Self::Utf16Le)
        } else if bytes.starts_with(&[0xfe, 0xff]) {
            Some(Self::Utf16Be)
        } else {
            None
        };
        let endian = match (self, bom) {
            (Self::Utf16, None) => return Err(invalid(
                "UTF-16 needs a BOM; select --encoding utf-16le or utf-16be for headerless input",
            )),
            (Self::Utf16, Some(endian)) => endian,
            (selected, Some(endian)) if selected != endian => {
                return Err(invalid("UTF-16 BOM conflicts with the selected encoding"))
            }
            (selected, _) => selected,
        };
        let bytes = if bom.is_some() { &bytes[2..] } else { bytes };
        if bytes.len() % 2 != 0 {
            return Err(invalid("invalid UTF-16: odd byte count"));
        }
        let units = bytes.as_chunks::<2>().0.iter().map(|&pair| {
            if endian == Self::Utf16Le {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        char::decode_utf16(units)
            .collect::<std::result::Result<String, _>>()
            .map_err(|_| invalid("invalid UTF-16: unpaired surrogate"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_endianness_preserves_unicode() {
        let text = "Hello 漢字 🙂\r\n";
        for encoding in [Encoding::Utf16Le, Encoding::Utf16Be] {
            let mut bytes: Vec<u8> = text
                .encode_utf16()
                .flat_map(|unit| {
                    if encoding == Encoding::Utf16Le {
                        unit.to_le_bytes()
                    } else {
                        unit.to_be_bytes()
                    }
                })
                .collect();
            assert_eq!(encoding.decode(&bytes).unwrap(), text);
            assert!(Encoding::Utf16.decode(&bytes).is_err());
            let bom = if encoding == Encoding::Utf16Le {
                [0xff, 0xfe]
            } else {
                [0xfe, 0xff]
            };
            bytes.splice(0..0, bom);
            assert_eq!(Encoding::Utf16.decode(&bytes).unwrap(), text);
            assert_eq!(encoding.decode(&bytes).unwrap(), text);
        }
    }

    #[test]
    fn malformed_and_conflicting_input_is_rejected() {
        assert!(Encoding::Utf16Le.decode(&[1]).is_err());
        assert!(Encoding::Utf16Le.decode(&[0, 0xd8]).is_err());
        assert!(Encoding::Utf16Be.decode(&[0xff, 0xfe, 65, 0]).is_err());
        assert!(Encoding::Utf16.decode(&[0xff, 0xfe, 0, 0]).is_err());
        assert!(!has_utf16_bom(&[0xff, 0xfe, 0, 0]));
        assert_eq!(Encoding::Utf16Le.decode(&[0xff, 0xfe, 0, 0]).unwrap(), "\0");
        assert!(Encoding::Utf8.decode(&[0xff]).is_err());
        assert_eq!(
            Encoding::Utf8.decode(b"\xef\xbb\xbfhello").unwrap(),
            "hello"
        );
    }
}
