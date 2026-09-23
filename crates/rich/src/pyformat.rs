//! Python `str.format` for progress text columns.
//!
//! Port of the subset of Python's format mini-language that upstream
//! `TextColumn` reaches through `text_format.format(task=task)`: replacement
//! fields that name a value (`{task.completed}`, `{task.fields[name]}`), `{{`
//! and `}}` escapes, and format specs `[[fill]align][sign][#][0][width]
//! [grouping][.precision][type]` over strings, integers, floats, booleans and
//! `None`. Values print as Python's `str()` would, so `10.0` stays `10.0` and
//! `1e16` prints `1e+16`.

/// A value a replacement field resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatValue {
    /// Python `None`.
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl From<&str> for FormatValue {
    fn from(value: &str) -> Self {
        FormatValue::Str(value.to_string())
    }
}

impl From<String> for FormatValue {
    fn from(value: String) -> Self {
        FormatValue::Str(value)
    }
}

impl From<i64> for FormatValue {
    fn from(value: i64) -> Self {
        FormatValue::Int(value)
    }
}

impl From<i32> for FormatValue {
    fn from(value: i32) -> Self {
        FormatValue::Int(i64::from(value))
    }
}

impl From<usize> for FormatValue {
    fn from(value: usize) -> Self {
        FormatValue::Int(value as i64)
    }
}

impl From<f64> for FormatValue {
    fn from(value: f64) -> Self {
        FormatValue::Float(value)
    }
}

impl From<bool> for FormatValue {
    fn from(value: bool) -> Self {
        FormatValue::Bool(value)
    }
}

impl<T: Into<FormatValue>> From<Option<T>> for FormatValue {
    fn from(value: Option<T>) -> Self {
        value.map_or(FormatValue::None, Into::into)
    }
}

/// Expand `template`, resolving each field name through `lookup`.
///
/// A field that `lookup` does not know, or a spec the value rejects, is left
/// in place verbatim: upstream would raise from `str.format`, which in a
/// progress display means a crash mid-render.
pub fn format(template: &str, lookup: impl Fn(&str) -> Option<FormatValue>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '{' if chars.peek().map(|&(_, n)| n) == Some('{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek().map(|&(_, n)| n) == Some('}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let Some(close) = template[index..].find('}') else {
                    out.push_str(&template[index..]);
                    break;
                };
                let field = &template[index + 1..index + close];
                // Skip past the closing brace.
                while let Some(&(at, _)) = chars.peek() {
                    if at > index + close {
                        break;
                    }
                    chars.next();
                }
                let (name, spec) = match field.split_once(':') {
                    Some((name, spec)) => (name, spec),
                    None => (field, ""),
                };
                // `!s` / `!r` conversions: `str()` is the default rendering.
                let name = name.split_once('!').map_or(name, |(name, _)| name);
                match lookup(name).and_then(|value| format_value(&value, spec)) {
                    Some(text) => out.push_str(&text),
                    None => out.push_str(&template[index..=index + close]),
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Python's `repr(float)` (also its `str()`): the shortest round-tripping
/// digits, in scientific notation when the exponent is below -4 or at least 16.
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let sci = format!("{value:e}");
    let (mantissa, exponent) = sci.split_once('e').expect("LowerExp has an exponent");
    let exponent: i32 = exponent.parse().expect("integer exponent");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    if !(-4..16).contains(&exponent) {
        let (head, tail) = digits.split_at(1);
        let fraction = if tail.is_empty() {
            String::new()
        } else {
            format!(".{tail}")
        };
        return format!(
            "{sign}{head}{fraction}e{}{:02}",
            exp_sign(exponent),
            exponent.abs()
        );
    }
    let point = exponent + 1;
    let text = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!("{digits}{}.0", "0".repeat(point as usize - digits.len()))
    } else {
        let (integer, fraction) = digits.split_at(point as usize);
        format!("{integer}.{fraction}")
    };
    format!("{sign}{text}")
}

fn exp_sign(exponent: i32) -> char {
    if exponent < 0 {
        '-'
    } else {
        '+'
    }
}

/// A parsed format spec.
#[derive(Debug, Default)]
struct Spec {
    fill: Option<char>,
    align: Option<char>,
    sign: Option<char>,
    alternate: bool,
    zero: bool,
    width: Option<usize>,
    grouping: Option<char>,
    precision: Option<usize>,
    kind: Option<char>,
}

fn parse_spec(spec: &str) -> Option<Spec> {
    let chars: Vec<char> = spec.chars().collect();
    let mut at = 0;
    let mut parsed = Spec::default();
    let is_align = |c: char| matches!(c, '<' | '>' | '^' | '=');
    if chars.len() >= 2 && is_align(chars[1]) {
        parsed.fill = Some(chars[0]);
        parsed.align = Some(chars[1]);
        at = 2;
    } else if !chars.is_empty() && is_align(chars[0]) {
        parsed.align = Some(chars[0]);
        at = 1;
    }
    if let Some(&c) = chars.get(at).filter(|c| matches!(c, '+' | '-' | ' ')) {
        parsed.sign = Some(c);
        at += 1;
    }
    if chars.get(at) == Some(&'z') {
        at += 1;
    }
    if chars.get(at) == Some(&'#') {
        parsed.alternate = true;
        at += 1;
    }
    if chars.get(at) == Some(&'0') {
        parsed.zero = true;
        at += 1;
    }
    let start = at;
    while chars.get(at).is_some_and(char::is_ascii_digit) {
        at += 1;
    }
    if at > start {
        parsed.width = chars[start..at].iter().collect::<String>().parse().ok();
    }
    if let Some(&c) = chars.get(at).filter(|c| matches!(c, ',' | '_')) {
        parsed.grouping = Some(c);
        at += 1;
    }
    if chars.get(at) == Some(&'.') {
        at += 1;
        let start = at;
        while chars.get(at).is_some_and(char::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return None;
        }
        parsed.precision = chars[start..at].iter().collect::<String>().parse().ok();
    }
    if let Some(&c) = chars.get(at) {
        parsed.kind = Some(c);
        at += 1;
    }
    (at == chars.len()).then_some(parsed)
}

/// `format(value, spec)`, or `None` where Python would raise.
pub fn format_value(value: &FormatValue, spec: &str) -> Option<String> {
    let spec_parsed = parse_spec(spec)?;
    match value {
        FormatValue::None => spec.is_empty().then(|| "None".to_string()),
        FormatValue::Str(text) => format_str(text, &spec_parsed),
        FormatValue::Bool(flag) if spec.is_empty() => {
            Some(if *flag { "True" } else { "False" }.to_string())
        }
        FormatValue::Bool(flag) => format_int(i64::from(*flag), &spec_parsed),
        FormatValue::Int(number) => format_int(*number, &spec_parsed),
        FormatValue::Float(number) => format_float(*number, &spec_parsed),
    }
}

fn format_str(text: &str, spec: &Spec) -> Option<String> {
    if !matches!(spec.kind, None | Some('s')) || spec.sign.is_some() || spec.grouping.is_some() {
        return None;
    }
    let body: String = match spec.precision {
        Some(precision) => text.chars().take(precision).collect(),
        None => text.to_string(),
    };
    Some(pad(String::new(), body, spec, '<'))
}

fn format_int(number: i64, spec: &Spec) -> Option<String> {
    let body = match spec.kind {
        None | Some('d') | Some('n') => {
            if spec.precision.is_some() {
                return None;
            }
            group(&number.unsigned_abs().to_string(), spec.grouping)
        }
        Some('x') => format!(
            "{}{:x}",
            if spec.alternate { "0x" } else { "" },
            number.unsigned_abs()
        ),
        Some('X') => format!(
            "{}{:X}",
            if spec.alternate { "0X" } else { "" },
            number.unsigned_abs()
        ),
        Some('o') => format!(
            "{}{:o}",
            if spec.alternate { "0o" } else { "" },
            number.unsigned_abs()
        ),
        Some('b') => format!(
            "{}{:b}",
            if spec.alternate { "0b" } else { "" },
            number.unsigned_abs()
        ),
        Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%') => {
            return format_float(number as f64, spec);
        }
        _ => return None,
    };
    Some(pad(sign_prefix(number < 0, spec), body, spec, '>'))
}

fn format_float(number: f64, spec: &Spec) -> Option<String> {
    if !number.is_finite() {
        let body = if number.is_nan() { "nan" } else { "inf" };
        let body = if spec.kind.is_some_and(|k| k.is_ascii_uppercase()) {
            body.to_uppercase()
        } else {
            body.to_string()
        };
        return Some(pad(sign_prefix(number < 0.0, spec), body, spec, '>'));
    }
    let negative =
        number.is_sign_negative() && number != 0.0 || number.to_bits() == (-0.0f64).to_bits();
    let magnitude = number.abs();
    let body = match spec.kind {
        Some('f' | 'F') => group_fixed(
            &format!("{:.*}", spec.precision.unwrap_or(6), magnitude),
            spec.grouping,
        ),
        Some('%') => format!(
            "{}%",
            group_fixed(
                &format!("{:.*}", spec.precision.unwrap_or(6), magnitude * 100.0),
                spec.grouping
            )
        ),
        Some('e' | 'E') => {
            let text = scientific(magnitude, spec.precision.unwrap_or(6));
            if spec.kind == Some('E') {
                text.to_uppercase()
            } else {
                text
            }
        }
        Some('g' | 'G') => {
            let text = general(magnitude, spec.precision.unwrap_or(6), spec.alternate);
            if spec.kind == Some('G') {
                text.to_uppercase()
            } else {
                text
            }
        }
        None => match spec.precision {
            // No type and no precision: `str(float)`.
            None => group_fixed(&float_repr(magnitude), spec.grouping),
            // No type with a precision: `g`, but a fixed result keeps a
            // digit after the point.
            Some(precision) => {
                let text = general(magnitude, precision, spec.alternate);
                if text.contains(['.', 'e', 'n', 'i']) {
                    text
                } else {
                    format!("{text}.0")
                }
            }
        },
        _ => return None,
    };
    Some(pad(sign_prefix(negative, spec), body, spec, '>'))
}

/// `%e` with `precision` digits: `1.500000e+03`.
fn scientific(value: f64, precision: usize) -> String {
    let text = format!("{value:.precision$e}");
    let (mantissa, exponent) = text.split_once('e').expect("LowerExp has an exponent");
    let exponent: i32 = exponent.parse().expect("integer exponent");
    format!("{mantissa}e{}{:02}", exp_sign(exponent), exponent.abs())
}

/// `%g` with `precision` significant digits.
fn general(value: f64, precision: usize, alternate: bool) -> String {
    let precision = precision.max(1);
    if value == 0.0 {
        return if alternate {
            format!("0.{}", "0".repeat(precision - 1))
        } else {
            "0".to_string()
        };
    }
    // The exponent after rounding to `precision` significant digits.
    let rounded = format!("{value:.*e}", precision - 1);
    let exponent: i32 = rounded
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0);
    let text = if exponent < -4 || exponent >= precision as i32 {
        scientific(value, precision - 1)
    } else {
        format!("{value:.*}", (precision as i32 - 1 - exponent) as usize)
    };
    if alternate {
        return text;
    }
    // Strip trailing zeros from the fraction (and a bare point).
    match text.split_once('e') {
        Some((mantissa, exponent)) => format!("{}e{exponent}", strip_fraction(mantissa)),
        None => strip_fraction(&text),
    }
}

fn strip_fraction(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text.to_string()
    }
}

/// Insert a thousands separator into a run of integer digits.
fn group(digits: &str, separator: Option<char>) -> String {
    let Some(separator) = separator else {
        return digits.to_string();
    };
    let mut out = String::new();
    for (index, c) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}

/// Group the integer part of a fixed-point number.
fn group_fixed(text: &str, separator: Option<char>) -> String {
    match text.split_once('.') {
        Some((integer, fraction)) => format!("{}.{fraction}", group(integer, separator)),
        None => group(text, separator),
    }
}

fn sign_prefix(negative: bool, spec: &Spec) -> String {
    match (negative, spec.sign) {
        (true, _) => "-".to_string(),
        (false, Some('+')) => "+".to_string(),
        (false, Some(' ')) => " ".to_string(),
        _ => String::new(),
    }
}

/// Apply width, fill and alignment. A `0` flag is `fill='0', align='='`.
fn pad(sign: String, body: String, spec: &Spec, default_align: char) -> String {
    let (fill, align) = match (spec.fill, spec.align, spec.zero) {
        (fill, Some(align), _) => (fill.unwrap_or(' '), align),
        (_, None, true) => ('0', '='),
        (_, None, false) => (' ', default_align),
    };
    let length = sign.chars().count() + body.chars().count();
    let Some(width) = spec.width.filter(|width| *width > length) else {
        return format!("{sign}{body}");
    };
    let gap = width - length;
    let fill_run = |count: usize| fill.to_string().repeat(count);
    match align {
        '<' => format!("{sign}{body}{}", fill_run(gap)),
        '^' => format!(
            "{}{sign}{body}{}",
            fill_run(gap / 2),
            fill_run(gap - gap / 2)
        ),
        '=' => format!("{sign}{}{body}", fill_run(gap)),
        _ => format!("{}{sign}{body}", fill_run(gap)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(value: impl Into<FormatValue>, spec: &str) -> String {
        format_value(&value.into(), spec).expect("valid spec")
    }

    #[test]
    fn floats_print_like_python_str() {
        // Each expected value is Python 3.11's `str(float)`.
        for (value, expected) in [
            (10.0, "10.0"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.5, "-2.5"),
            (123456.789, "123456.789"),
            (f64::INFINITY, "inf"),
        ] {
            assert_eq!(float_repr(value), expected, "{value}");
        }
    }

    #[test]
    fn specs_match_python_format() {
        // Each expected value is Python 3.11's `format(value, spec)`.
        assert_eq!(fmt(42.0, ">3.0f"), " 42");
        assert_eq!(fmt(99.95, ">3.0f"), "100");
        assert_eq!(fmt(2.5, ".0f"), "2");
        assert_eq!(fmt(3.5, ".0f"), "4");
        assert_eq!(fmt(0.425, ".1%"), "42.5%");
        assert_eq!(fmt(1234567.891, ",.2f"), "1,234,567.89");
        assert_eq!(fmt(1234567i64, ","), "1,234,567");
        assert_eq!(fmt(42i64, "05d"), "00042");
        assert_eq!(fmt(-42i64, "05d"), "-0042");
        assert_eq!(fmt(42i64, "+d"), "+42");
        assert_eq!(fmt(255i64, "#x"), "0xff");
        assert_eq!(fmt("ab", "*^6"), "**ab**");
        assert_eq!(fmt("abcdef", ".3"), "abc");
        assert_eq!(fmt("ab", "5"), "ab   ");
        assert_eq!(fmt(7i64, "5"), "    7");
        assert_eq!(fmt(1234.5, ".2"), "1.2e+03");
        assert_eq!(fmt(10.0, ".3"), "10.0");
        assert_eq!(fmt(10.0, ".3g"), "10");
        assert_eq!(fmt(0.000012345, "g"), "1.2345e-05");
        assert_eq!(fmt(1500.0, "e"), "1.500000e+03");
        assert_eq!(fmt(true, ""), "True");
        assert_eq!(fmt(true, "d"), "1");
        assert_eq!(fmt(FormatValue::None, ""), "None");
        assert_eq!(fmt(3i64, ".1f"), "3.0");
    }

    #[test]
    fn templates_expand_fields_and_escapes() {
        let lookup = |name: &str| match name {
            "task.completed" => Some(FormatValue::Float(3.0)),
            "task.fields[name]" => Some(FormatValue::from("disk")),
            _ => None,
        };
        assert_eq!(
            format("{{x}} {task.fields[name]}: {task.completed:>5.1f}", lookup),
            "{x} disk:   3.0"
        );
        // Unknown fields and rejected specs stay verbatim.
        assert_eq!(
            format("{task.nope} {task.completed:q}", lookup),
            "{task.nope} {task.completed:q}"
        );
        assert_eq!(format("{task.completed!s}", lookup), "3.0");
        assert_eq!(format("open {", lookup), "open {");
    }
}
