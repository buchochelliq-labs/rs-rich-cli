//! Link destination normalisation for [`Markdown`](crate::markdown::Markdown).
//!
//! Upstream `rich.markdown` parses with `markdown-it-py`, which runs every link,
//! image and autolink destination through `normalizeLink` before it reaches
//! rich (and an autolink's visible text through `normalizeLinkText`). This is a
//! port of `markdown_it/common/normalize_url.py` and the parts of `mdurl`
//! (`parse`, `format`, `encode`, `decode`) and `markdown_it/_punycode.py` it
//! uses, so a destination prints and links exactly as upstream's does.
//!
//! It also matters for safety: a destination is percent-encoded down to a
//! fixed ASCII safe set, so a control character in a URL (an ESC starting a
//! nested OSC sequence, say) can never reach the terminal raw.

/// `mdurl.ENCODE_DEFAULT_CHARS`: kept as-is by [`encode`] besides ASCII
/// alphanumerics.
const ENCODE_DEFAULT_CHARS: &str = ";/?:@&=+$,-_.!~*'()#";
/// `mdurl.DECODE_DEFAULT_CHARS + "%"`, as `normalizeLinkText` decodes.
const DECODE_LINK_TEXT_EXCLUDE: &str = ";/?:@&=+$,#%";
/// `RECODE_HOSTNAME_FOR`: protocols whose host is punycoded (case-sensitive).
const RECODE_HOSTNAME_FOR: [&str; 3] = ["http:", "https:", "mailto:"];

/// `normalizeLink`: punycode the host of an `http:`/`https:`/`mailto:` (or
/// protocol-relative) URL, then percent-encode everything outside the safe set.
pub(crate) fn normalize_link(url: &str) -> String {
    let mut parsed = Url::parse(url);
    if recode_hostname(&parsed) {
        let hostname = parsed.hostname.take().unwrap_or_default();
        parsed.hostname = Some(
            map_domain(&hostname, |label| {
                // `REGEX_NON_ASCII = [^\0-\x7E]`: DEL counts as non-ASCII here.
                if label.chars().any(|c| c > '\x7e') {
                    Some(format!("xn--{}", punycode_encode(label)))
                } else {
                    Some(label.to_string())
                }
            })
            .unwrap_or(hostname),
        );
    }
    encode(&parsed.format())
}

/// `normalizeLinkText`: the autolink text, with a punycoded host decoded back
/// to Unicode and percent-escapes decoded (except reserved characters and `%`).
pub(crate) fn normalize_link_text(url: &str) -> String {
    let mut parsed = Url::parse(url);
    if recode_hostname(&parsed) {
        let hostname = parsed.hostname.take().unwrap_or_default();
        // `with suppress(Exception)`: a label that fails to decode leaves the
        // whole hostname as it was.
        parsed.hostname = Some(
            map_domain(&hostname, |label| match label.strip_prefix("xn--") {
                Some(encoded) => punycode_decode(&encoded.to_lowercase()),
                None => Some(label.to_string()),
            })
            .unwrap_or(hostname),
        );
    }
    decode(&parsed.format(), DECODE_LINK_TEXT_EXCLUDE)
}

/// `validateLink` on a normalised destination: markdown-it refuses to make a
/// link or image of a `javascript:`, `vbscript:`, `file:` or `data:` URL
/// (except `data:image/{gif,png,jpeg,webp};`), leaving its source as text.
pub(crate) fn validate_link(url: &str) -> bool {
    // A normalised URL is ASCII, so ASCII lowering is Python's `lower()`.
    let url = url.trim_matches(is_python_space).to_ascii_lowercase();
    let bad = ["vbscript:", "javascript:", "file:", "data:"]
        .iter()
        .any(|scheme| url.starts_with(scheme));
    !bad || ["gif", "png", "jpeg", "webp"]
        .iter()
        .any(|kind| url.starts_with(&format!("data:image/{kind};")))
}

/// `parsed.hostname and (not parsed.protocol or parsed.protocol in RECODE_HOSTNAME_FOR)`.
fn recode_hostname(parsed: &Url) -> bool {
    parsed
        .hostname
        .as_deref()
        .is_some_and(|host| !host.is_empty())
        && parsed
            .protocol
            .as_deref()
            .is_none_or(|protocol| protocol.is_empty() || RECODE_HOSTNAME_FOR.contains(&protocol))
}

/// `_punycode.map_domain`: keep an email's local part, split the domain on
/// the IDNA label separators and rejoin the mapped labels with `.`. `None`
/// when any label fails to map.
fn map_domain(string: &str, mut map: impl FnMut(&str) -> Option<String>) -> Option<String> {
    let parts: Vec<&str> = string.split('@').collect();
    let (mut result, domain) = if parts.len() > 1 {
        (format!("{}@", parts[0]), parts[1])
    } else {
        (String::new(), string)
    };
    let labels: Vec<&str> = domain
        .split(['.', '\u{3002}', '\u{ff0e}', '\u{ff61}'])
        .collect();
    let mut mapped = Vec::with_capacity(labels.len());
    for label in labels {
        mapped.push(map(label)?);
    }
    result.push_str(&mapped.join("."));
    Some(result)
}

/// Python's `str.isspace`, for `str.strip()`: Rust's `White_Space` plus the
/// four ASCII information separators.
fn is_python_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// A character of `[a-z0-9.+-]` under `re.IGNORECASE`, which also folds in
/// `İ`, `ı`, `ſ` and the Kelvin sign.
fn is_protocol_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '.' | '+' | '-' | '\u{130}' | '\u{131}' | '\u{17f}' | '\u{212a}'
        )
}

/// `HOSTNAME_PART_PATTERN = ^[+a-z0-9A-Z_-]{0,63}$`, measured in characters.
fn is_hostname_part(part: &str) -> bool {
    part.chars().count() <= 63
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '-'))
}

/// `mdurl`'s `URL`: the pieces `parse` splits a destination into.
#[derive(Debug, Default)]
struct Url {
    protocol: Option<String>,
    slashes: bool,
    auth: Option<String>,
    port: Option<String>,
    hostname: Option<String>,
    hash: Option<String>,
    search: Option<String>,
    pathname: Option<String>,
}

/// `str.find` over a char vector: the first index of any of `needles`.
fn find_any(chars: &[char], needles: &[char]) -> Option<usize> {
    chars.iter().position(|c| needles.contains(c))
}

impl Url {
    /// `mdurl.parse(url, slashes_denote_host=True)` (`MutableURL.parse`),
    /// working on characters as Python indexes a `str`.
    fn parse(url: &str) -> Url {
        // `HOSTLESS_PROTOCOL` / `SLASHED_PROTOCOL` membership.
        fn hostless(protocol: &str) -> bool {
            matches!(protocol, "javascript" | "javascript:")
        }
        fn slashed(protocol: &str) -> bool {
            matches!(
                protocol,
                "http"
                    | "https"
                    | "ftp"
                    | "gopher"
                    | "file"
                    | "http:"
                    | "https:"
                    | "ftp:"
                    | "gopher:"
                    | "file:"
            )
        }
        const HOST_ENDING_CHARS: [char; 3] = ['/', '?', '#'];
        // `NON_HOST_CHARS`: `%/?;#` plus `AUTO_ESCAPE` (`'`, unwise, delims).
        const NON_HOST_CHARS: [char; 19] = [
            '%', '/', '?', ';', '#', '\'', '{', '}', '|', '\\', '^', '`', '<', '>', '"', ' ', '\r',
            '\n', '\t',
        ];

        let mut result = Url::default();
        let mut rest: Vec<char> = url.trim_matches(is_python_space).chars().collect();

        // `PROTOCOL_PATTERN = ^([a-z0-9.+-]+:)`, case-insensitive.
        let mut proto = String::new();
        let mut lower_proto = String::new();
        let run = rest.iter().take_while(|c| is_protocol_char(**c)).count();
        if run > 0 && rest.get(run) == Some(&':') {
            proto = rest[..=run].iter().collect();
            lower_proto = proto.to_lowercase();
            result.protocol = Some(proto.clone());
            rest.drain(..=run);
        }

        // `slashes_denote_host` is always set, so a leading `//` is a host.
        let slashes = rest.starts_with(&['/', '/']);
        if slashes && !(!proto.is_empty() && hostless(&proto)) {
            rest.drain(..2);
            result.slashes = true;
        }

        if !hostless(&proto) && (slashes || (!proto.is_empty() && !slashed(&proto))) {
            // The first host-ending character bounds where the auth's `@` may be.
            let host_end = find_any(&rest, &HOST_ENDING_CHARS);
            let search_to = host_end.map_or(rest.len(), |end| (end + 1).min(rest.len()));
            if let Some(at_sign) = rest[..search_to].iter().rposition(|c| *c == '@') {
                result.auth = Some(rest[..at_sign].iter().collect());
                rest.drain(..=at_sign);
            }

            let mut host_end = find_any(&rest, &NON_HOST_CHARS).unwrap_or(rest.len());
            if host_end > 0 && rest[host_end - 1] == ':' {
                host_end -= 1;
            }
            let host: Vec<char> = rest.drain(..host_end).collect();
            result.parse_host(&host);

            let mut hostname: Vec<char> =
                result.hostname.take().unwrap_or_default().chars().collect();
            let ipv6 = hostname.first() == Some(&'[') && hostname.last() == Some(&']');
            if !ipv6 {
                let text: String = hostname.iter().collect();
                let parts: Vec<&str> = text.split('.').collect();
                for (index, part) in parts.iter().enumerate() {
                    if part.is_empty() || is_hostname_part(part) {
                        continue;
                    }
                    // Non-ASCII characters stand in as `x` for the check.
                    let placeholder: String = part
                        .chars()
                        .map(|c| if (c as u32) > 127 { 'x' } else { c })
                        .collect();
                    if is_hostname_part(&placeholder) {
                        continue;
                    }
                    // `HOSTNAME_PART_START = ^([+a-z0-9A-Z_-]{0,63})(.*)$`.
                    let valid_len = part
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '-'))
                        .take(63)
                        .count();
                    let split = part
                        .char_indices()
                        .nth(valid_len)
                        .map_or(part.len(), |(i, _)| i);
                    let mut valid_parts: Vec<&str> = parts[..index].to_vec();
                    valid_parts.push(&part[..split]);
                    let mut not_host: Vec<&str> = vec![&part[split..]];
                    not_host.extend(&parts[index + 1..]);
                    let moved: Vec<char> = not_host.join(".").chars().collect();
                    rest.splice(0..0, moved);
                    hostname = valid_parts.join(".").chars().collect();
                    break;
                }
            }
            if hostname.len() > 255 {
                hostname.clear();
            }
            if ipv6 {
                hostname = hostname[1..hostname.len() - 1].to_vec();
            }
            result.hostname = Some(hostname.into_iter().collect());
        }

        if let Some(hash) = rest.iter().position(|c| *c == '#') {
            result.hash = Some(rest.drain(hash..).collect());
        }
        if let Some(query) = rest.iter().position(|c| *c == '?') {
            result.search = Some(rest.drain(query..).collect());
        }
        if !rest.is_empty() {
            result.pathname = Some(rest.into_iter().collect());
        }
        if slashed(&lower_proto)
            && result
                .hostname
                .as_deref()
                .is_some_and(|host| !host.is_empty())
            && result.pathname.as_deref().is_none_or(str::is_empty)
        {
            result.pathname = Some(String::new());
        }
        result
    }

    /// `MutableURL.parse_host`: split a trailing `:digits` port off `host`.
    fn parse_host(&mut self, host: &[char]) {
        let mut host = host;
        // `PORT_PATTERN = :[0-9]*$`: the last `:` when only digits follow it.
        let digits = host.iter().rev().take_while(|c| c.is_ascii_digit()).count();
        if digits < host.len() && host[host.len() - digits - 1] == ':' {
            let colon = host.len() - digits - 1;
            if digits > 0 {
                self.port = Some(host[colon + 1..].iter().collect());
            }
            host = &host[..colon];
        }
        if !host.is_empty() {
            self.hostname = Some(host.iter().collect());
        }
    }

    /// `mdurl.format`.
    fn format(&self) -> String {
        let mut out = String::new();
        out.push_str(self.protocol.as_deref().unwrap_or(""));
        if self.slashes {
            out.push_str("//");
        }
        if let Some(auth) = self.auth.as_deref().filter(|auth| !auth.is_empty()) {
            out.push_str(auth);
            out.push('@');
        }
        match self.hostname.as_deref() {
            Some(host) if host.contains(':') => {
                out.push('[');
                out.push_str(host);
                out.push(']');
            }
            host => out.push_str(host.unwrap_or("")),
        }
        if let Some(port) = self.port.as_deref().filter(|port| !port.is_empty()) {
            out.push(':');
            out.push_str(port);
        }
        out.push_str(self.pathname.as_deref().unwrap_or(""));
        out.push_str(self.search.as_deref().unwrap_or(""));
        out.push_str(self.hash.as_deref().unwrap_or(""));
        out
    }
}

/// `mdurl.encode` with the default exclude set and `keep_escaped=True`:
/// percent-encode (as UTF-8) every character but ASCII alphanumerics, the
/// safe set and an already valid `%XX` escape.
fn encode(string: &str) -> String {
    let chars: Vec<char> = string.chars().collect();
    let mut out = String::with_capacity(string.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '%'
            && i + 2 < chars.len()
            && chars[i + 1..i + 3].iter().all(char::is_ascii_hexdigit)
        {
            out.extend(&chars[i..i + 3]);
            i += 3;
            continue;
        }
        if c.is_ascii_alphanumeric() || ENCODE_DEFAULT_CHARS.contains(c) {
            out.push(c);
        } else {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        i += 1;
    }
    out
}

/// `mdurl.decode`: decode each run of `%XX` escapes as UTF-8, leaving the
/// ASCII characters in `exclude` escaped (upper-cased) and turning invalid
/// sequences into U+FFFD, as mdurl's hand-rolled decoder does.
fn decode(string: &str, exclude: &str) -> String {
    let bytes = string.as_bytes();
    let hex = |at: usize| -> Option<u8> {
        let pair = string.get(at..at + 2)?;
        u8::from_str_radix(pair, 16)
            .ok()
            .filter(|_| pair.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    let mut out = String::with_capacity(string.len());
    let mut at = 0;
    while at < bytes.len() {
        // A maximal run of `%XX` escapes (`(%[a-f0-9]{2})+`, ignoring case).
        let mut run = Vec::new();
        let mut end = at;
        while bytes.get(end) == Some(&b'%') {
            match hex(end + 1) {
                Some(value) => {
                    run.push(value);
                    end += 3;
                }
                None => break,
            }
        }
        if run.is_empty() {
            let c = string[at..].chars().next().expect("in bounds");
            out.push(c);
            at += c.len_utf8();
            continue;
        }
        decode_run(&run, exclude, &mut out);
        at = end;
    }
    out
}

/// `repl_func_with_cache` over one run of escaped bytes. `l` there is the
/// run's length in *characters* (three per byte), hence the `3 *` bounds.
fn decode_run(run: &[u8], exclude: &str, out: &mut String) {
    let length = run.len() * 3;
    let continuation = |b: u8| b & 0xc0 == 0x80;
    let push_utf8 = |bytes: &[u8], out: &mut String| match std::str::from_utf8(bytes) {
        Ok(text) => out.push_str(text),
        Err(_) => out.extend(std::iter::repeat_n('\u{fffd}', bytes.len())),
    };
    let mut index = 0;
    while index < run.len() {
        let i = index * 3;
        let b1 = run[index];
        if b1 < 0x80 {
            let c = b1 as char;
            if exclude.contains(c) {
                out.push_str(&format!("%{b1:02X}"));
            } else {
                out.push(c);
            }
            index += 1;
            continue;
        }
        if b1 & 0xe0 == 0xc0 && i + 3 < length && continuation(run[index + 1]) {
            push_utf8(&run[index..index + 2], out);
            index += 2;
            continue;
        }
        if b1 & 0xf0 == 0xe0
            && i + 6 < length
            && continuation(run[index + 1])
            && continuation(run[index + 2])
        {
            push_utf8(&run[index..index + 3], out);
            index += 3;
            continue;
        }
        if b1 & 0xf8 == 0xf0
            && i + 9 < length
            && continuation(run[index + 1])
            && continuation(run[index + 2])
            && continuation(run[index + 3])
        {
            push_utf8(&run[index..index + 4], out);
            index += 4;
            continue;
        }
        out.push('\u{fffd}');
        index += 1;
    }
}

// Punycode (RFC 3492) parameters, as Python's `encodings.punycode` uses them.
const BASE: u64 = 36;
const T_MIN: u64 = 1;
const T_MAX: u64 = 26;
const SKEW: u64 = 38;
const DAMP: u64 = 700;
const INITIAL_BIAS: u64 = 72;
const INITIAL_N: u64 = 0x80;

/// `T(j, bias)`: the threshold for digit `j`.
fn threshold(k: u64, bias: u64) -> u64 {
    // Python: `36 * (j + 1) - bias` clamped to 1..=26, with `k = 36 * (j + 1)`.
    k.saturating_sub(bias).clamp(T_MIN, T_MAX)
}

/// RFC 3492 `adapt`.
fn adapt(delta: u64, first: bool, points: u64) -> u64 {
    let mut delta = if first { delta / DAMP } else { delta / 2 };
    delta += delta / points;
    let mut k = 0;
    while delta > ((BASE - T_MIN) * T_MAX) / 2 {
        delta /= BASE - T_MIN;
        k += BASE;
    }
    k + (BASE - T_MIN + 1) * delta / (delta + SKEW)
}

fn digit(value: u64) -> char {
    b"abcdefghijklmnopqrstuvwxyz0123456789"[value as usize] as char
}

/// `codecs.encode(label, "punycode")`: RFC 3492 encoding of code points,
/// basic (`< 0x80`) characters copied as-is, case preserved.
fn punycode_encode(input: &str) -> String {
    let points: Vec<u64> = input.chars().map(|c| c as u64).collect();
    let mut output: String = input.chars().filter(|c| (*c as u32) < 0x80).collect();
    let basic = output.chars().count() as u64;
    if basic > 0 {
        output.push('-');
    }
    let (mut n, mut delta, mut bias, mut handled) = (INITIAL_N, 0u64, INITIAL_BIAS, basic);
    while (handled as usize) < points.len() {
        let m = points
            .iter()
            .copied()
            .filter(|p| *p >= n)
            .min()
            .expect("unhandled point");
        delta += (m - n) * (handled + 1);
        n = m;
        for &point in &points {
            if point < n {
                delta += 1;
            }
            if point == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = threshold(k, bias);
                    if q < t {
                        break;
                    }
                    output.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                output.push(digit(q));
                bias = adapt(delta, handled == basic, handled + 1);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    output
}

/// `codecs.decode(label, "punycode")` (strict): `None` wherever Python raises
/// (a non-ASCII input, a bad digit, a truncated number or a code point past
/// U+10FFFF). A decoded surrogate, which a Rust `char` cannot hold, also
/// fails, leaving the host undecoded.
fn punycode_decode(input: &str) -> Option<String> {
    if !input.is_ascii() {
        return None;
    }
    let (base, extended) = match input.rfind('-') {
        Some(position) => (&input[..position], &input[position + 1..]),
        None => ("", input),
    };
    let mut output: Vec<char> = base.chars().collect();
    let extended = extended.to_ascii_uppercase().into_bytes();
    let (mut n, mut position, mut bias) = (INITIAL_N, 0u64, INITIAL_BIAS);
    let mut at = 0;
    let mut first = true;
    while at < extended.len() {
        // `decode_generalized_number`.
        let (mut value, mut weight, mut k) = (0u64, 1u64, BASE);
        loop {
            let byte = *extended.get(at)?;
            at += 1;
            let digit = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'0'..=b'9' => byte - 22,
                _ => return None,
            } as u64;
            let t = threshold(k, bias);
            value = value.checked_add(digit.checked_mul(weight)?)?;
            if digit < t {
                break;
            }
            weight = weight.checked_mul(BASE - t)?;
            k += BASE;
        }
        // `insertion_sort`: `pos` starts at -1, so it advances by `delta + 1`
        // on the first pass and by `delta` plus the inserted point after.
        position = position.checked_add(value)?;
        let length = output.len() as u64 + 1;
        n = n.checked_add(position / length)?;
        position %= length;
        let point = char::from_u32(u32::try_from(n).ok()?)?;
        output.insert(position as usize, point);
        bias = adapt(value, first, output.len() as u64);
        first = false;
        position += 1;
    }
    Some(output.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_link_matches_markdown_it() {
        // (url, normalizeLink(url), normalizeLinkText(url)), captured from
        // markdown-it-py as pinned by rich 15.0.0.
        let cases: &[(&str, &str, &str)] = &[
            (
                "http://a\x1b]0;PWN\x07",
                "http://a%1B%5D0;PWN%07",
                "http://a\x1b]0;PWN\x07",
            ),
            (
                "http://é.com/ü",
                "http://xn--9ca.com/%C3%BC",
                "http://é.com/ü",
            ),
            ("http://a b", "http://a%20b", "http://a b"),
            ("HTTP://é.com", "HTTP://%C3%A9.com", "HTTP://é.com"),
            ("http://@é.com", "http://xn--9ca.com", "http://é.com"),
            ("mailto:ü@é.de", "mailto:%C3%BC@xn--9ca.de", "mailto:ü@é.de"),
            ("//é.com/x", "//xn--9ca.com/x", "//é.com/x"),
            (
                "http://[::1]:80/x",
                "http://%5B::1%5D:80/x",
                "http://[::1]:80/x",
            ),
            ("http://a%zz%41", "http://a%25zz%41", "http://a%zzA"),
            (
                "javascript://é.com",
                "javascript://%C3%A9.com",
                "javascript://é.com",
            ),
            ("http://ﬀ.com", "http://xn--im6c.com", "http://ﬀ.com"),
            ("http://a。é．b", "http://a.xn--9ca.b", "http://a.é.b"),
            ("x y", "x%20y", "x y"),
            ("ftp://é.com", "ftp://%C3%A9.com", "ftp://é.com"),
            (
                "http://é.com:8080",
                "http://xn--9ca.com:8080",
                "http://é.com:8080",
            ),
            ("  http://é.com  ", "http://xn--9ca.com", "http://é.com"),
            ("http://a*b.com/", "http://a*b.com/", "http://a*b.com/"),
            (
                "http://xn--9ca.com/%C3%BC%2F%25",
                "http://xn--9ca.com/%C3%BC%2F%25",
                "http://é.com/ü%2F%25",
            ),
            (
                "http://a\u{2028}b",
                "http://xn--ab-x3t",
                "http://a\u{2028}b",
            ),
            (
                "http://ä.com?q=ü#frag ü",
                "http://xn--4ca.com?q=%C3%BC#frag%20%C3%BC",
                "http://ä.com?q=ü#frag ü",
            ),
            (
                "http://user:pw@é.com",
                "http://user:pw@xn--9ca.com",
                "http://user:pw@é.com",
            ),
            ("ｈttp://x", "%EF%BD%88ttp://x", "ｈttp://x"),
            ("http://é.com:", "http://xn--9ca.com:", "http://é.com:"),
            ("k\u{212a}:x", "k%E2%84%AA:x", "k\u{212a}:x"),
            ("http://%e9.com", "http://%e9.com", "http://\u{fffd}.com"),
            (
                "http://a/%F0%9F%98%80%ED%A0%80%C3",
                "http://a/%F0%9F%98%80%ED%A0%80%C3",
                "http://a/😀\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
            ),
            (
                "http://xn--zz.com",
                "http://xn--zz.com",
                "http://xn--zz.com",
            ),
            (
                "http://xn--ä.com",
                "http://xn--xn---ooa.com",
                "http://xn--ä.com",
            ),
            ("http://A.É.com", "http://A.xn--dca.com", "http://A.É.com"),
        ];
        for (url, link, text) in cases {
            assert_eq!(normalize_link(url), *link, "normalizeLink({url:?})");
            assert_eq!(
                normalize_link_text(url),
                *text,
                "normalizeLinkText({url:?})"
            );
        }
        let long = format!("http://{}.com", "é".repeat(70));
        assert_eq!(
            normalize_link(&long),
            format!("http://{}.com", "%C3%A9".repeat(70))
        );
        assert_eq!(normalize_link_text(&long), long);
    }

    #[test]
    fn punycode_round_trips_rfc_samples() {
        // RFC 3492 section 7.1 samples (lower-cased, as `to_unicode` passes
        // them) and Python's case-preserving encode.
        for (unicode, ascii) in [
            ("ü", "tda"),
            ("bücher", "bcher-kva"),
            ("他们为什么不说中文", "ihqwcrb4cv8a8dqg056pqjye"),
            ("Pročprostěnemluvíčesky", "Proprostnemluvesky-uyb24dma41a"),
            ("-> $1.00 <-", "-> $1.00 <--"),
        ] {
            assert_eq!(punycode_encode(unicode), ascii);
            assert_eq!(
                punycode_decode(&ascii.to_lowercase()).as_deref(),
                Some(unicode.to_lowercase().as_str())
            );
        }
        assert_eq!(punycode_decode("bcher-kva").as_deref(), Some("bücher"));
        assert_eq!(punycode_decode("zz"), None);
    }
}
