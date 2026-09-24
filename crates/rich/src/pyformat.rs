//! Python `str.format` for progress text columns.
//!
//! Port of the subset of Python's format mini-language that upstream
//! `TextColumn` reaches through `text_format.format(task=task)`: replacement
//! fields that name a value (`{task.completed}`, `{task.fields[name]}`), `{{`
//! and `}}` escapes, and format specs `[[fill]align][sign][z][#][0][width]
//! [grouping][.precision][type]` over strings, integers, floats, booleans and
//! `None`. Values print as Python's `str()` would, so `10.0` stays `10.0` and
//! `1e16` prints `1e+16`.
//!
//! The spec handling follows CPython's `Python/formatter_unicode.c` and
//! `PyOS_double_to_string` (Python 3.11+, for the `z` flag). The `n` type uses
//! the C locale, which is what a Python process that never calls `setlocale`
//! formats with. `tests/golden/pyformat.tsv` pins a few thousand cases.

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

/// Which thousands separator a spec asked for (CPython's `LT_*` locale kinds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Thousands {
    None,
    /// `,`: every three digits.
    Comma,
    /// `_`: every three digits.
    Underscore,
    /// `_` on a binary, octal or hex integer: every four digits.
    UnderscoreFour,
}

/// A parsed format spec. Port of CPython's `InternalFormatSpec`
/// (`Python/formatter_unicode.c`).
#[derive(Debug)]
struct Spec {
    fill: char,
    align: char,
    sign: Option<char>,
    no_neg_0: bool,
    alternate: bool,
    width: Option<usize>,
    thousands: Thousands,
    precision: Option<usize>,
    /// The presentation type; `'\0'` when omitted from a float spec.
    kind: char,
}

/// `parse_internal_render_format_spec`: `None` wherever Python raises
/// `ValueError` while parsing or validating the spec.
fn parse_spec(spec: &str, default_type: char, default_align: char) -> Option<Spec> {
    let chars: Vec<char> = spec.chars().collect();
    let mut at = 0;
    let mut parsed = Spec {
        fill: ' ',
        align: default_align,
        sign: None,
        no_neg_0: false,
        alternate: false,
        width: None,
        thousands: Thousands::None,
        precision: None,
        kind: default_type,
    };
    let is_align = |c: char| matches!(c, '<' | '>' | '^' | '=');
    let mut fill_specified = false;
    let mut align_specified = false;
    if chars.len() >= 2 && is_align(chars[1]) {
        parsed.fill = chars[0];
        parsed.align = chars[1];
        fill_specified = true;
        align_specified = true;
        at = 2;
    } else if !chars.is_empty() && is_align(chars[0]) {
        parsed.align = chars[0];
        align_specified = true;
        at = 1;
    }
    if let Some(&c) = chars.get(at).filter(|c| matches!(c, '+' | '-' | ' ')) {
        parsed.sign = Some(c);
        at += 1;
    }
    if chars.get(at) == Some(&'z') {
        parsed.no_neg_0 = true;
        at += 1;
    }
    if chars.get(at) == Some(&'#') {
        parsed.alternate = true;
        at += 1;
    }
    // The `0` flag: zero fill, and `=` alignment for a right-aligned type.
    if !fill_specified && chars.get(at) == Some(&'0') {
        parsed.fill = '0';
        if !align_specified && default_align == '>' {
            parsed.align = '=';
        }
        at += 1;
    }
    let start = at;
    while chars.get(at).is_some_and(char::is_ascii_digit) {
        at += 1;
    }
    if at > start {
        parsed.width = Some(chars[start..at].iter().collect::<String>().parse().ok()?);
    }
    if chars.get(at) == Some(&',') {
        parsed.thousands = Thousands::Comma;
        at += 1;
    }
    if chars.get(at) == Some(&'_') {
        if parsed.thousands != Thousands::None {
            return None; // "Cannot specify both ',' and '_'."
        }
        parsed.thousands = Thousands::Underscore;
        at += 1;
    }
    if chars.get(at) == Some(&',') {
        return None; // "Cannot specify both ',' and '_'."
    }
    if chars.get(at) == Some(&'.') {
        at += 1;
        let start = at;
        while chars.get(at).is_some_and(char::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return None; // "Format specifier missing precision"
        }
        parsed.precision = Some(chars[start..at].iter().collect::<String>().parse().ok()?);
    }
    match chars.len() - at {
        0 => {}
        1 => parsed.kind = chars[at],
        _ => return None, // "Invalid format specifier"
    }
    if parsed.thousands != Thousands::None {
        match parsed.kind {
            'd' | 'e' | 'f' | 'g' | 'E' | 'G' | '%' | 'F' | '\0' => {}
            'b' | 'o' | 'x' | 'X' if parsed.thousands == Thousands::Underscore => {
                parsed.thousands = Thousands::UnderscoreFour;
            }
            _ => return None, // "Cannot specify ',' with '…'."
        }
    }
    Some(parsed)
}

/// `format(value, spec)`, or `None` where Python would raise.
pub fn format_value(value: &FormatValue, spec: &str) -> Option<String> {
    // `format(x, "")` is `str(x)` for every type.
    if spec.is_empty() {
        return Some(match value {
            FormatValue::None => "None".to_string(),
            FormatValue::Bool(flag) => if *flag { "True" } else { "False" }.to_string(),
            FormatValue::Int(number) => number.to_string(),
            FormatValue::Float(number) => float_repr(*number),
            FormatValue::Str(text) => text.clone(),
        });
    }
    match value {
        // `object.__format__` rejects any non-empty spec.
        FormatValue::None => None,
        FormatValue::Str(text) => format_str(text, &parse_spec(spec, 's', '<')?),
        // `bool` formats through `int.__format__`.
        FormatValue::Bool(flag) => format_int(i64::from(*flag), &parse_spec(spec, 'd', '>')?),
        FormatValue::Int(number) => format_int(*number, &parse_spec(spec, 'd', '>')?),
        FormatValue::Float(number) => format_float(*number, &parse_spec(spec, '\0', '>')?),
    }
}

/// `format_string_internal`.
fn format_str(text: &str, spec: &Spec) -> Option<String> {
    if spec.kind != 's'
        || spec.sign.is_some()
        || spec.no_neg_0
        || spec.alternate
        || spec.align == '='
    {
        return None;
    }
    let body: String = match spec.precision {
        Some(precision) => text.chars().take(precision).collect(),
        None => text.to_string(),
    };
    let length = body.chars().count();
    let gap = spec.width.map_or(0, |width| width.saturating_sub(length));
    let left = match spec.align {
        '>' => gap,
        '^' => gap / 2,
        _ => 0,
    };
    let fill = |count: usize| spec.fill.to_string().repeat(count);
    Some(format!("{}{body}{}", fill(left), fill(gap - left)))
}

/// `format_long_internal`, handing the float presentation types to
/// [`format_float`] as `int.__format__` does.
fn format_int(number: i64, spec: &Spec) -> Option<String> {
    if matches!(spec.kind, 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%') {
        return format_float(number as f64, spec);
    }
    if spec.precision.is_some() || spec.no_neg_0 {
        return None;
    }
    let sign_char = number < 0;
    let magnitude = number.unsigned_abs();
    let (prefix, digits, remainder) = match spec.kind {
        'c' => {
            if spec.sign.is_some() || spec.alternate {
                return None;
            }
            // `%c arg not in range(0x110000)`; a surrogate, which Python
            // allows, cannot be a Rust `char`.
            let c = u32::try_from(number).ok().and_then(char::from_u32)?;
            // CPython formats the character as "remainder" so it is copied,
            // never grouped or zero-padded as digits.
            return Some(fill_number(
                false,
                "",
                "",
                false,
                &c.to_string(),
                spec,
                None,
            ));
        }
        'd' | 'n' => ("", magnitude.to_string(), ""),
        'b' => ("0b", format!("{magnitude:b}"), ""),
        'o' => ("0o", format!("{magnitude:o}"), ""),
        'x' => ("0x", format!("{magnitude:x}"), ""),
        'X' => ("0X", format!("{magnitude:X}"), ""),
        _ => return None,
    };
    let prefix = if spec.alternate { prefix } else { "" };
    Some(fill_number(
        sign_char,
        prefix,
        &digits,
        false,
        remainder,
        spec,
        grouping(spec),
    ))
}

/// The separator and group size for a spec's digits. `n` uses the current
/// locale, which for a Python process that never calls `setlocale` is the
/// C locale: no grouping at all.
fn grouping(spec: &Spec) -> Option<(char, usize)> {
    if spec.kind == 'n' {
        return None;
    }
    match spec.thousands {
        Thousands::None => None,
        Thousands::Comma => Some((',', 3)),
        Thousands::Underscore => Some(('_', 3)),
        Thousands::UnderscoreFour => Some(('_', 4)),
    }
}

/// `format_float_internal`.
fn format_float(number: f64, spec: &Spec) -> Option<String> {
    let mut kind = spec.kind;
    let mut add_dot_0 = false;
    let mut default_precision = 6;
    if kind == '\0' {
        // No type: `repr()` without a precision, else `g` keeping a digit
        // after the point.
        add_dot_0 = true;
        kind = 'r';
        default_precision = 0;
    }
    if kind == 'n' {
        kind = 'g';
    }
    if !matches!(kind, 'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' | 'r') {
        return None; // "Unknown format code … for object of type 'float'"
    }
    let mut value = number;
    let mut add_pct = false;
    if kind == '%' {
        kind = 'f';
        value *= 100.0;
        add_pct = true;
    }
    let precision = match spec.precision {
        None => default_precision,
        Some(precision) => {
            if kind == 'r' {
                kind = 'g';
            }
            precision
        }
    };
    let mut buffer = double_to_string(
        value,
        kind,
        precision,
        spec.alternate,
        add_dot_0,
        spec.no_neg_0,
    );
    if add_pct {
        buffer.push('%');
    }
    let (negative, body) = match buffer.strip_prefix('-') {
        Some(body) => (true, body),
        None => (false, buffer.as_str()),
    };
    // `parse_number`: the leading digits, an optional decimal point, and
    // everything after it copied as-is.
    let digit_count = body.bytes().take_while(u8::is_ascii_digit).count();
    let (digits, rest) = body.split_at(digit_count);
    let (has_decimal, remainder) = match rest.strip_prefix('.') {
        Some(remainder) => (true, remainder),
        None => (false, rest),
    };
    Some(fill_number(
        negative,
        "",
        digits,
        has_decimal,
        remainder,
        spec,
        grouping(spec),
    ))
}

/// `PyOS_double_to_string` / `format_float_short` for the `e`, `f`, `g` and
/// `r` (repr) codes, upper-case variants included.
fn double_to_string(
    value: f64,
    code: char,
    precision: usize,
    alternate: bool,
    add_dot_0: bool,
    no_neg_0: bool,
) -> String {
    let upper = code.is_ascii_uppercase();
    let code = code.to_ascii_lowercase();
    let case = |text: String| if upper { text.to_uppercase() } else { text };
    if value.is_nan() {
        // "we *never* add a sign for a nan".
        return case("nan".to_string());
    }
    if value.is_infinite() {
        return case(if value < 0.0 { "-inf" } else { "inf" }.to_string());
    }
    // `_Py_dg_dtoa`: the significant digits (no trailing zeros) and the
    // decimal point's position relative to them.
    let magnitude = value.abs();
    let (digits, mut decpt) = match code {
        'e' => dtoa_significant(magnitude, precision + 1),
        'g' => dtoa_significant(magnitude, precision.max(1)),
        'f' => dtoa_fixed(magnitude, precision),
        _ => dtoa_shortest(magnitude),
    };
    let precision = match code {
        'e' => precision + 1,
        'g' => precision.max(1),
        _ => precision,
    } as i64;
    let digits_len = digits.len() as i64;
    let mut use_exp = false;
    let mut vdigits_end = digits_len;
    match code {
        'e' => {
            use_exp = true;
            vdigits_end = precision;
        }
        'f' => vdigits_end = decpt + precision,
        'g' => {
            let limit = if add_dot_0 { precision - 1 } else { precision };
            if decpt <= -4 || decpt > limit {
                use_exp = true;
            }
            if alternate {
                vdigits_end = precision;
            }
        }
        _ => {
            if decpt <= -4 || decpt > 16 {
                use_exp = true;
            }
        }
    }
    let mut exponent = 0;
    if use_exp {
        exponent = decpt - 1;
        decpt = 1;
    }
    let vdigits_start = if decpt <= 0 { decpt - 1 } else { 0 };
    vdigits_end = if !use_exp && add_dot_0 {
        vdigits_end.max(decpt + 1)
    } else {
        vdigits_end.max(decpt)
    };

    let zero_value = digits.bytes().all(|b| b == b'0');
    let negative = value.is_sign_negative() && !(no_neg_0 && zero_value);
    let zeros = |count: i64| "0".repeat(count.max(0) as usize);
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if decpt <= 0 {
        out.push_str(&zeros(decpt - vdigits_start));
        out.push('.');
        out.push_str(&zeros(-decpt));
    } else {
        out.push_str(&zeros(-vdigits_start));
    }
    if 0 < decpt && decpt <= digits_len {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    } else {
        out.push_str(&digits);
    }
    if digits_len < decpt {
        out.push_str(&zeros(decpt - digits_len));
        out.push('.');
        out.push_str(&zeros(vdigits_end - decpt));
    } else {
        out.push_str(&zeros(vdigits_end - digits_len));
    }
    if out.ends_with('.') && !alternate {
        out.pop();
    }
    if use_exp {
        out.push_str(&format!(
            "e{}{:02}",
            exp_sign(exponent as i32),
            exponent.abs()
        ));
    }
    case(out)
}

/// Split Rust's `{:e}` rendering into dtoa's `(digits, decpt)`, trailing
/// zeros dropped. Zero is `("0", 1)`, as dtoa returns it.
fn split_scientific(text: &str) -> (String, i64) {
    let (mantissa, exponent) = text.split_once('e').expect("LowerExp has an exponent");
    let exponent: i64 = exponent.parse().expect("integer exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    if digits.is_empty() {
        return ("0".to_string(), 1);
    }
    (digits.to_string(), exponent + 1)
}

/// dtoa mode 0: the shortest digits that round-trip.
fn dtoa_shortest(value: f64) -> (String, i64) {
    split_scientific(&format!("{value:e}"))
}

/// dtoa mode 2: `count` significant digits, correctly rounded.
fn dtoa_significant(value: f64, count: usize) -> (String, i64) {
    split_scientific(&format!("{value:.*e}", count - 1))
}

/// dtoa mode 3: rounded to `precision` digits after the point. A value that
/// rounds to nothing is dtoa's empty digit string at `decpt = -precision`.
fn dtoa_fixed(value: f64, precision: usize) -> (String, i64) {
    if value == 0.0 {
        return ("0".to_string(), 1);
    }
    let text = format!("{value:.precision$}");
    let (integer, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let all = format!("{integer}{fraction}");
    let leading = all.bytes().take_while(|b| *b == b'0').count();
    let digits = all[leading..].trim_end_matches('0');
    if digits.is_empty() {
        return (String::new(), -(precision as i64));
    }
    (digits.to_string(), integer.len() as i64 - leading as i64)
}

/// `calc_number_widths` + `fill_number`: lay out
/// `<lpad><sign><prefix><spad><grouped digits><.><remainder><rpad>`.
fn fill_number(
    negative: bool,
    prefix: &str,
    digits: &str,
    has_decimal: bool,
    remainder: &str,
    spec: &Spec,
    grouping: Option<(char, usize)>,
) -> String {
    let sign = match (spec.sign, negative) {
        (_, true) => Some('-'),
        (Some('+'), false) => Some('+'),
        (Some(' '), false) => Some(' '),
        _ => None,
    };
    let width = spec.width.map_or(-1, |width| width as i64);
    let non_digit = i64::from(sign.is_some())
        + prefix.chars().count() as i64
        + i64::from(has_decimal)
        + remainder.chars().count() as i64;
    let min_width = if spec.fill == '0' && spec.align == '=' {
        width - non_digit
    } else {
        0
    };
    let grouped = if digits.is_empty() {
        String::new()
    } else {
        insert_thousands_grouping(digits, min_width, grouping)
    };
    let padding = width - (non_digit + grouped.chars().count() as i64);
    let (mut left, mut middle, mut right) = (0, 0, 0);
    if padding > 0 {
        match spec.align {
            '<' => right = padding,
            '^' => {
                left = padding / 2;
                right = padding - left;
            }
            '=' => middle = padding,
            _ => left = padding,
        }
    }
    let fill = |count: i64| spec.fill.to_string().repeat(count as usize);
    let mut out = fill(left);
    out.extend(sign);
    out.push_str(prefix);
    out.push_str(&fill(middle));
    out.push_str(&grouped);
    if has_decimal {
        out.push('.');
    }
    out.push_str(remainder);
    out.push_str(&fill(right));
    out
}

/// `_PyUnicode_InsertThousandsGrouping`: group `digits` from the right,
/// zero-padding (and grouping the zeros too) up to `min_width` characters.
fn insert_thousands_grouping(
    digits: &str,
    min_width: i64,
    grouping: Option<(char, usize)>,
) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let mut remaining = chars.len() as i64;
    let mut min_width = min_width;
    // Groups from the right; each is (zeros, digit count, separator after).
    let mut pieces: Vec<String> = Vec::new();
    let mut use_separator = false;
    let mut piece = |length: i64, remaining: i64, use_separator: bool| {
        let zeros = (length - remaining).max(0);
        let count = remaining.min(length).max(0);
        let mut text = "0".repeat(zeros as usize);
        text.extend(&chars[(remaining - count) as usize..remaining as usize]);
        if use_separator {
            text.push(grouping.map_or(',', |(separator, _)| separator));
        }
        pieces.push(text);
        count
    };
    let mut finished = false;
    if let Some((_, size)) = grouping {
        loop {
            let length = (size as i64).min(remaining.max(min_width).max(1));
            remaining -= piece(length, remaining, use_separator);
            use_separator = true;
            min_width -= length;
            if remaining <= 0 && min_width <= 0 {
                finished = true;
                break;
            }
            min_width -= 1;
        }
    }
    if !finished {
        let length = remaining.max(min_width).max(1);
        piece(length, remaining, use_separator);
    }
    pieces.iter().rev().map(String::as_str).collect()
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
        // No type with a precision: exponent once `exp >= precision - 1`.
        assert_eq!(fmt(12.5, ".2"), "1.2e+01");
        assert_eq!(fmt(2.5, ".0"), "2e+00");
        // Grouping touches only the leading digits, never an exponent.
        assert_eq!(fmt(1e16, ","), "1e+16");
        assert_eq!(fmt(f64::INFINITY, "%"), "inf%");
        assert_eq!(fmt(123.0, "#g"), "123.000");
        assert_eq!(fmt(123.0, "#.3g"), "123.");
        assert_eq!(fmt(-0.0001, "z.2f"), "0.00");
        assert_eq!(fmt(1234.5, "n"), "1234.5");
        assert_eq!(fmt("h\u{e9}llo", "08"), "h\u{e9}llo000");
        // Zero padding is grouped along with the digits.
        assert_eq!(fmt(1234i64, "010,"), "00,001,234");
        assert_eq!(fmt(65i64, "c"), "A");
        assert_eq!(format_value(&FormatValue::from("ab"), "=5"), None);
        assert_eq!(format_value(&FormatValue::Int(1234), ",n"), None);
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
