//! Small readers the rest of the crate shares: strict JSON, base64, hex,
//! SHA-256 and the index's timestamps. Nothing here is public API.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

/// The largest listing, index, keys or lockfile document read, in bytes
/// (the validator's `MAX_FILE_BYTES`).
pub(crate) const MAX_DOCUMENT_BYTES: usize = 1 << 20;

/// A JSON value read with the marketplace's rules: duplicate keys and
/// non-finite numbers are refused, as `scripts/validate_marketplace.py`
/// refuses them, so every reader agrees on what signed bytes mean.
struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("a non-finite number is not JSON"))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(Strict(value)) = seq.next_element()? {
            out.push(value);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut map = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if map.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key {key:?}")));
            }
            let Strict(value) = access.next_value()?;
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }
}

/// Parse one JSON document strictly (see [`Strict`]), at most
/// [`MAX_DOCUMENT_BYTES`] long.
pub(crate) fn parse_json(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("larger than {MAX_DOCUMENT_BYTES} bytes"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8".to_string())?;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let Strict(value) = Strict::deserialize(&mut deserializer).map_err(|e| e.to_string())?;
    deserializer.end().map_err(|e| e.to_string())?;
    Ok(value)
}

/// Lowercase hex SHA-256 of `bytes`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Whether `text` is `len` lowercase hex digits.
pub(crate) fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Standard base64 with padding, as minisign writes it.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.trim().as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let chunks = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let mut values = [0u32; 4];
        let mut padding = 0;
        for (j, &c) in chunk.iter().enumerate() {
            values[j] = match c {
                b'=' if i + 1 == chunks && j >= 2 => {
                    padding += 1;
                    0
                }
                _ if padding > 0 => return None,
                b'A'..=b'Z' => u32::from(c - b'A'),
                b'a'..=b'z' => u32::from(c - b'a') + 26,
                b'0'..=b'9' => u32::from(c - b'0') + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            };
        }
        let n = values[0] << 18 | values[1] << 12 | values[2] << 6 | values[3];
        out.push((n >> 16) as u8);
        if padding < 2 {
            out.push((n >> 8) as u8);
        }
        if padding < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

fn digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A real day written `YYYY-MM-DD`, as days since the Unix epoch.
pub(crate) fn parse_date(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let year = i64::from(digits(&text[0..4])?);
    let month = digits(&text[5..7])?;
    let day = digits(&text[8..10])?;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    Some(days_from_civil(year, month, day))
}

/// A real UTC time written `YYYY-MM-DDTHH:MM:SSZ`, as Unix seconds.
pub(crate) fn parse_timestamp(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() != 20 || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let days = parse_date(&text[0..10])?;
    let hour = digits(&text[11..13])?;
    let minute = digits(&text[14..16])?;
    let second = digits(&text[17..19])?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(days * 86_400 + i64::from(hour * 3600 + minute * 60 + second))
}

/// `time` as Unix seconds (negative before the epoch).
pub(crate) fn unix_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_json_refuses_duplicates_and_trailing_data() {
        assert!(parse_json(br#"{"a": 1, "b": [1, 2.5, null, true]}"#).is_ok());
        let error = parse_json(br#"{"a": 1, "a": 2}"#).unwrap_err();
        assert!(error.contains("duplicate key"), "{error}");
        assert!(parse_json(br#"{"a": {"b": 1, "b": 1}}"#).is_err());
        assert!(parse_json(b"{} {}").is_err());
        assert!(parse_json(b"NaN").is_err());
        assert!(parse_json(&vec![b' '; MAX_DOCUMENT_BYTES + 1]).is_err());
    }

    #[test]
    fn base64_round_trips_minisign_shapes() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVsbA==").unwrap(), b"hell");
        assert_eq!(base64_decode("aGVs").unwrap(), b"hel");
        assert!(base64_decode("aGV").is_none());
        assert!(base64_decode("a=Vs").is_none());
        assert!(base64_decode("aG=s").is_none());
        assert!(base64_decode("aGVs!!!!").is_none());
    }

    #[test]
    fn timestamps_are_real_times() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2026-10-20T12:00:00Z"), Some(1_792_497_600));
        assert_eq!(parse_timestamp("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert!(parse_timestamp("2023-02-29T00:00:00Z").is_none());
        assert!(parse_timestamp("2026-10-20T24:00:00Z").is_none());
        assert!(parse_timestamp("2026-10-20 12:00:00Z").is_none());
        assert!(parse_timestamp("2026-10-20T12:00:00+00:00").is_none());
        assert!(parse_date("2026-13-01").is_none());
    }
}
