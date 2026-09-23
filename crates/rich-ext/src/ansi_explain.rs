//! Explain terminal output: tokenize text with escape sequences and describe
//! every sequence in words.
//!
//! [`explain`] splits input into [`Token`]s — text, SGR, other CSI, OSC,
//! plain `ESC` sequences, DCS/APC/PM/SOS strings, C0/C1 controls and invalid
//! (truncated or interrupted) sequences — and computes the visible text: the
//! text runs plus tabs and newlines. There is no cursor emulation, so a
//! carriage return, backspace or cursor move does not overwrite anything in
//! `visible_text`.
//!
//! Both 7-bit (`ESC [`) and 8-bit C1 introducers (U+009B CSI, U+009D OSC,
//! U+0090 DCS, U+009C ST…) are recognised. [`explain_bytes`] maps raw C1 bytes
//! in non-UTF-8 input onto those characters. DCS payloads (sixel images) are
//! summarised by length, never dumped.

use rich::{Color, Console, ConsoleOptions, Renderable, Segment, Table, Text};

/// One SGR effect, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Effect {
    /// The parameters that produced it (`"38;5;208"`).
    pub code: String,
    /// What it does (`"fg 256-colour 208 (#ff8700)"`).
    pub description: String,
}

impl std::fmt::Display for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.description)
    }
}

/// How an OSC or string sequence ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Terminator {
    Bel,
    St,
}

/// The kind of control string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum StringKind {
    Dcs,
    Apc,
    Pm,
    Sos,
}

impl StringKind {
    pub fn name(self) -> &'static str {
        match self {
            StringKind::Dcs => "DCS",
            StringKind::Apc => "APC",
            StringKind::Pm => "PM",
            StringKind::Sos => "SOS",
        }
    }
}

/// A piece of terminal output.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(
    feature = "serde",
    serde(tag = "type", content = "value", rename_all = "snake_case")
)]
pub enum Token {
    /// Printable text.
    Text(String),
    /// Select Graphic Rendition (`CSI … m`).
    Sgr {
        raw: String,
        params: String,
        effects: Vec<Effect>,
    },
    /// Any other control sequence.
    Csi {
        raw: String,
        final_byte: char,
        params: String,
        intermediates: String,
        meaning: String,
    },
    /// Operating System Command.
    Osc {
        raw: String,
        code: Option<u32>,
        meaning: String,
        terminator: Terminator,
    },
    /// A plain escape sequence (`ESC 7`, `ESC ( B`…).
    Esc { raw: String, meaning: String },
    /// A control string; the payload is summarised, not kept in `meaning`.
    Dcs {
        raw: String,
        kind: StringKind,
        data_len: usize,
        meaning: String,
    },
    /// A C0 or C1 control character.
    Control { byte: u8, name: &'static str },
    /// A malformed, truncated or interrupted sequence.
    Invalid { raw: String, reason: String },
}

impl Token {
    /// A short kind label (`text`, `SGR`, `CSI`, `OSC`, `ESC`, `DCS`, `control`,
    /// `invalid`).
    pub fn kind(&self) -> &'static str {
        match self {
            Token::Text(_) => "text",
            Token::Sgr { .. } => "SGR",
            Token::Csi { .. } => "CSI",
            Token::Osc { .. } => "OSC",
            Token::Esc { .. } => "ESC",
            Token::Dcs { kind, .. } => kind.name(),
            Token::Control { .. } => "control",
            Token::Invalid { .. } => "invalid",
        }
    }

    /// The input this token covers.
    pub fn raw(&self) -> String {
        match self {
            Token::Text(t) => t.clone(),
            Token::Sgr { raw, .. }
            | Token::Csi { raw, .. }
            | Token::Osc { raw, .. }
            | Token::Esc { raw, .. }
            | Token::Dcs { raw, .. }
            | Token::Invalid { raw, .. } => raw.clone(),
            Token::Control { byte, .. } => char::from_u32(*byte as u32).unwrap_or('?').to_string(),
        }
    }

    /// What it does, in words.
    pub fn meaning(&self) -> String {
        match self {
            Token::Text(t) => {
                let n = t.chars().count();
                format!("{n} character{}", if n == 1 { "" } else { "s" })
            }
            Token::Sgr { effects, .. } => effects
                .iter()
                .map(|e| e.description.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            Token::Csi { meaning, .. }
            | Token::Osc { meaning, .. }
            | Token::Esc { meaning, .. }
            | Token::Dcs { meaning, .. } => meaning.clone(),
            Token::Control { name, .. } => control_meaning(name).to_owned(),
            Token::Invalid { reason, .. } => reason.clone(),
        }
    }
}

/// A token and where it starts in the input (byte offset).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Spanned {
    pub offset: usize,
    pub len: usize,
    pub token: Token,
}

/// The tokens of an input and the text a terminal would show.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Explanation {
    pub tokens: Vec<Spanned>,
    pub visible_text: String,
}

impl Explanation {
    /// Tokens other than text.
    pub fn escapes(&self) -> impl Iterator<Item = &Spanned> {
        self.tokens
            .iter()
            .filter(|t| !matches!(t.token, Token::Text(_)))
    }
    /// Invalid tokens.
    pub fn invalid(&self) -> impl Iterator<Item = &Spanned> {
        self.tokens
            .iter()
            .filter(|t| matches!(t.token, Token::Invalid { .. }))
    }
}

/// Explain `input`.
pub fn explain(input: &str) -> Explanation {
    let tokens = Lexer { s: input, pos: 0 }.run();
    let mut visible_text = String::new();
    for t in &tokens {
        match &t.token {
            Token::Text(text) => visible_text.push_str(text),
            Token::Control { byte: b'\t', .. } => visible_text.push('\t'),
            Token::Control { byte: b'\n', .. } => visible_text.push('\n'),
            _ => {}
        }
    }
    Explanation {
        tokens,
        visible_text,
    }
}

/// Explain raw bytes: valid UTF-8 is decoded, lone bytes 0x80–0x9F become the
/// C1 controls they are in 8-bit mode, and other invalid bytes become U+FFFD.
/// Offsets are into the decoded string.
pub fn explain_bytes(input: &[u8]) -> Explanation {
    explain(&decode_bytes(input))
}

/// The decoding [`explain_bytes`] uses.
pub fn decode_bytes(mut input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len());
    loop {
        match std::str::from_utf8(input) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&input[..valid]).expect("valid prefix"));
                let bad = e.error_len().unwrap_or(input.len() - valid).max(1);
                for &b in &input[valid..valid + bad] {
                    out.push(if (0x80..=0x9f).contains(&b) {
                        char::from_u32(b as u32).expect("C1")
                    } else {
                        '\u{fffd}'
                    });
                }
                input = &input[valid + bad..];
            }
        }
    }
}

const ESC: char = '\u{1b}';
const C1_ST: char = '\u{9c}';

struct Lexer<'a> {
    s: &'a str,
    pos: usize,
}

fn is_special(c: char) -> bool {
    (c as u32) < 0x20 || c == '\u{7f}' || ('\u{80}'..='\u{9f}').contains(&c)
}

impl Lexer<'_> {
    fn peek_at(&self, at: usize) -> Option<char> {
        self.s[at..].chars().next()
    }

    fn run(mut self) -> Vec<Spanned> {
        let mut out = Vec::new();
        while self.pos < self.s.len() {
            let start = self.pos;
            let c = self.peek_at(start).expect("in bounds");
            let token = match c {
                ESC => self.escape(start),
                '\u{9b}' => self.csi(start, start + c.len_utf8()),
                '\u{9d}' => self.osc(start, start + c.len_utf8()),
                '\u{90}' => self.string(start, start + c.len_utf8(), StringKind::Dcs),
                '\u{98}' => self.string(start, start + c.len_utf8(), StringKind::Sos),
                '\u{9e}' => self.string(start, start + c.len_utf8(), StringKind::Pm),
                '\u{9f}' => self.string(start, start + c.len_utf8(), StringKind::Apc),
                c if is_special(c) => {
                    self.pos += c.len_utf8();
                    Token::Control {
                        byte: c as u32 as u8,
                        name: control_name(c as u32 as u8),
                    }
                }
                _ => {
                    let end = self.s[start..]
                        .char_indices()
                        .find(|(_, c)| *c == ESC || is_special(*c))
                        .map_or(self.s.len(), |(i, _)| start + i);
                    self.pos = end;
                    Token::Text(self.s[start..end].to_owned())
                }
            };
            out.push(Spanned {
                offset: start,
                len: self.pos - start,
                token,
            });
        }
        out
    }

    fn invalid(&mut self, start: usize, end: usize, reason: impl Into<String>) -> Token {
        self.pos = end;
        Token::Invalid {
            raw: self.s[start..end].to_owned(),
            reason: reason.into(),
        }
    }

    fn escape(&mut self, start: usize) -> Token {
        let mut at = start + 1;
        let Some(next) = self.peek_at(at) else {
            return self.invalid(start, at, "truncated escape sequence at end of input");
        };
        match next {
            '[' => return self.csi(start, at + 1),
            ']' => return self.osc(start, at + 1),
            'P' => return self.string(start, at + 1, StringKind::Dcs),
            'X' => return self.string(start, at + 1, StringKind::Sos),
            '^' => return self.string(start, at + 1, StringKind::Pm),
            '_' => return self.string(start, at + 1, StringKind::Apc),
            _ => {}
        }
        let mut intermediates = String::new();
        while let Some(c) = self.peek_at(at).filter(|c| ('\x20'..='\x2f').contains(c)) {
            intermediates.push(c);
            at += 1;
        }
        match self.peek_at(at) {
            Some(f) if ('\x30'..='\x7e').contains(&f) => {
                self.pos = at + 1;
                Token::Esc {
                    raw: self.s[start..self.pos].to_owned(),
                    meaning: esc_meaning(&intermediates, f),
                }
            }
            None => self.invalid(start, at, "truncated escape sequence at end of input"),
            Some(c) => self.invalid(
                start,
                at,
                format!("escape sequence interrupted by {}", describe_char(c)),
            ),
        }
    }

    fn csi(&mut self, start: usize, mut at: usize) -> Token {
        let body = at;
        while let Some(c) = self.peek_at(at).filter(|c| ('\x30'..='\x3f').contains(c)) {
            at += c.len_utf8();
        }
        let params = self.s[body..at].to_owned();
        let inter_start = at;
        while let Some(c) = self.peek_at(at).filter(|c| ('\x20'..='\x2f').contains(c)) {
            at += c.len_utf8();
        }
        let intermediates = self.s[inter_start..at].to_owned();
        match self.peek_at(at) {
            Some(f) if ('\x40'..='\x7e').contains(&f) => {
                self.pos = at + 1;
                let raw = self.s[start..self.pos].to_owned();
                let private = params.starts_with(['<', '=', '>', '?']);
                if f == 'm' && intermediates.is_empty() && !private {
                    let effects = sgr_effects(&params);
                    Token::Sgr {
                        raw,
                        params,
                        effects,
                    }
                } else {
                    let meaning = csi_meaning(&params, &intermediates, f);
                    Token::Csi {
                        raw,
                        final_byte: f,
                        params,
                        intermediates,
                        meaning,
                    }
                }
            }
            None => self.invalid(
                start,
                at,
                "truncated control sequence (CSI) at end of input",
            ),
            Some(c) => self.invalid(
                start,
                at,
                format!("control sequence (CSI) interrupted by {}", describe_char(c)),
            ),
        }
    }

    /// Find the end of a string body: `(body_end, sequence_end, terminator)`.
    fn terminated(&self, from: usize, bel: bool) -> Result<(usize, usize, Terminator), usize> {
        let mut iter = self.s[from..].char_indices().peekable();
        while let Some((i, c)) = iter.next() {
            let at = from + i;
            match c {
                '\x07' if bel => return Ok((at, at + 1, Terminator::Bel)),
                C1_ST => return Ok((at, at + c.len_utf8(), Terminator::St)),
                ESC => match iter.peek() {
                    Some((_, '\\')) => return Ok((at, at + 2, Terminator::St)),
                    // A doubled ESC inside a control string is a literal ESC
                    // (tmux passthrough wraps sequences this way).
                    Some((_, ESC)) if !bel => {
                        iter.next();
                    }
                    _ => return Err(at),
                },
                _ => {}
            }
        }
        Err(self.s.len())
    }

    fn osc(&mut self, start: usize, body: usize) -> Token {
        match self.terminated(body, true) {
            Ok((end, next, terminator)) => {
                self.pos = next;
                let data = &self.s[body..end];
                let code = data.split(';').next().and_then(|c| c.parse::<u32>().ok());
                let by = match terminator {
                    Terminator::Bel => "BEL",
                    Terminator::St => "ST",
                };
                Token::Osc {
                    raw: self.s[start..next].to_owned(),
                    code,
                    meaning: format!("{} ({by}-terminated)", osc_meaning(data)),
                    terminator,
                }
            }
            Err(at) if at == self.s.len() => {
                self.invalid(start, at, "unterminated OSC at end of input")
            }
            Err(at) => self.invalid(start, at, "OSC interrupted by ESC without ST"),
        }
    }

    fn string(&mut self, start: usize, body: usize, kind: StringKind) -> Token {
        match self.terminated(body, false) {
            Ok((end, next, _)) => {
                self.pos = next;
                let data = &self.s[body..end];
                let (meaning, data_len) = string_meaning(kind, data);
                Token::Dcs {
                    raw: self.s[start..next].to_owned(),
                    kind,
                    data_len,
                    meaning,
                }
            }
            Err(at) => {
                let len = at - body;
                let what = if at == self.s.len() {
                    "at end of input"
                } else {
                    "interrupted by ESC"
                };
                self.invalid(
                    start,
                    at,
                    format!("unterminated {} {what} ({len} bytes)", kind.name()),
                )
            }
        }
    }
}

fn describe_char(c: char) -> String {
    if is_special(c) || c == ESC {
        control_name(c as u32 as u8).to_owned()
    } else {
        format!("{c:?}")
    }
}

/// The mnemonic of a C0/C1 control (or `DEL`).
pub fn control_name(byte: u8) -> &'static str {
    const C0: [&str; 32] = [
        "NUL", "SOH", "STX", "ETX", "EOT", "ENQ", "ACK", "BEL", "BS", "TAB", "LF", "VT", "FF",
        "CR", "SO", "SI", "DLE", "DC1", "DC2", "DC3", "DC4", "NAK", "SYN", "ETB", "CAN", "EM",
        "SUB", "ESC", "FS", "GS", "RS", "US",
    ];
    const C1: [&str; 32] = [
        "PAD", "HOP", "BPH", "NBH", "IND", "NEL", "SSA", "ESA", "HTS", "HTJ", "VTS", "PLD", "PLU",
        "RI", "SS2", "SS3", "DCS", "PU1", "PU2", "STS", "CCH", "MW", "SPA", "EPA", "SOS", "SGCI",
        "SCI", "CSI", "ST", "OSC", "PM", "APC",
    ];
    match byte {
        0..=0x1f => C0[byte as usize],
        0x7f => "DEL",
        0x80..=0x9f => C1[(byte - 0x80) as usize],
        _ => "?",
    }
}

fn control_meaning(name: &str) -> &'static str {
    match name {
        "NUL" => "null (ignored)",
        "BEL" => "bell",
        "BS" => "backspace",
        "TAB" => "horizontal tab",
        "LF" => "line feed (newline)",
        "VT" => "vertical tab",
        "FF" => "form feed",
        "CR" => "carriage return",
        "SO" => "shift out (G1 character set)",
        "SI" => "shift in (G0 character set)",
        "CAN" => "cancel sequence",
        "SUB" => "substitute (cancel sequence)",
        "DEL" => "delete (ignored)",
        "IND" => "index (8-bit)",
        "NEL" => "next line (8-bit)",
        "HTS" => "set tab stop (8-bit)",
        "RI" => "reverse index (8-bit)",
        "SS2" => "single shift G2 (8-bit)",
        "SS3" => "single shift G3 (8-bit)",
        "ST" => "string terminator with no string (8-bit)",
        _ => "control character",
    }
}

fn esc_meaning(intermediates: &str, f: char) -> String {
    let charset = |set: char| match set {
        'B' => "US ASCII",
        '0' => "DEC special graphics (line drawing)",
        'A' => "UK",
        '<' => "DEC supplemental",
        '4' => "Dutch",
        _ => "another character set",
    };
    match (intermediates, f) {
        ("", '7') => "save cursor (DECSC)".into(),
        ("", '8') => "restore cursor (DECRC)".into(),
        ("", 'c') => "full reset (RIS)".into(),
        ("", '=') => "application keypad mode (DECKPAM)".into(),
        ("", '>') => "numeric keypad mode (DECKPNM)".into(),
        ("", 'D') => "index: move down one line (IND)".into(),
        ("", 'E') => "next line (NEL)".into(),
        ("", 'H') => "set tab stop (HTS)".into(),
        ("", 'M') => "reverse index: move up one line (RI)".into(),
        ("", 'N') => "single shift G2 (SS2)".into(),
        ("", 'O') => "single shift G3 (SS3)".into(),
        ("", 'Z') => "request terminal identity (DECID)".into(),
        ("", '\\') => "string terminator with no string (ST)".into(),
        ("", 'n') => "invoke G2 character set (LS2)".into(),
        ("", 'o') => "invoke G3 character set (LS3)".into(),
        ("(", s) => format!("G0 character set: {}", charset(s)),
        (")", s) => format!("G1 character set: {}", charset(s)),
        ("*", s) => format!("G2 character set: {}", charset(s)),
        ("+", s) => format!("G3 character set: {}", charset(s)),
        ("#", '8') => "screen alignment test (DECALN)".into(),
        ("#", '3') => "double-height line, top half (DECDHL)".into(),
        ("#", '4') => "double-height line, bottom half (DECDHL)".into(),
        ("#", '5') => "single-width line (DECSWL)".into(),
        ("#", '6') => "double-width line (DECDWL)".into(),
        ("%", 'G') => "select UTF-8 character set".into(),
        ("%", '@') => "select default character set".into(),
        (" ", 'F') => "7-bit controls (S7C1T)".into(),
        (" ", 'G') => "8-bit controls (S8C1T)".into(),
        (i, f) => format!("unrecognised escape sequence (ESC {i}{f})"),
    }
}

const COLOR_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

fn indexed(n: u32) -> String {
    if n < 8 {
        format!("256-colour {n} ({})", COLOR_NAMES[n as usize])
    } else if n < 16 {
        format!("256-colour {n} (bright {})", COLOR_NAMES[n as usize - 8])
    } else if n < 256 {
        let hex = Color::from_ansi(n as u8)
            .get_truecolor()
            .map(|t| t.hex())
            .unwrap_or_default();
        format!("256-colour {n} ({hex})")
    } else {
        format!("256-colour {n} (out of range)")
    }
}

fn rgb(r: u32, g: u32, b: u32) -> String {
    if r > 255 || g > 255 || b > 255 {
        format!("rgb({r},{g},{b}) (out of range)")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

fn simple_sgr(n: u32) -> String {
    let s = match n {
        0 => "reset",
        1 => "bold on",
        2 => "dim on",
        3 => "italic on",
        4 => "underline on",
        5 => "slow blink on",
        6 => "rapid blink on",
        7 => "reverse on",
        8 => "conceal on",
        9 => "strikethrough on",
        10 => "primary font",
        20 => "fraktur on",
        21 => "double underline on",
        22 => "bold and dim off",
        23 => "italic off",
        24 => "underline off",
        25 => "blink off",
        26 => "proportional spacing on",
        27 => "reverse off",
        28 => "conceal off",
        29 => "strikethrough off",
        39 => "fg default",
        49 => "bg default",
        50 => "proportional spacing off",
        51 => "framed on",
        52 => "encircled on",
        53 => "overline on",
        54 => "framed and encircled off",
        55 => "overline off",
        59 => "underline colour default",
        73 => "superscript on",
        74 => "subscript on",
        75 => "superscript and subscript off",
        _ => "",
    };
    if !s.is_empty() {
        return s.into();
    }
    match n {
        11..=19 => format!("alternative font {}", n - 10),
        30..=37 => format!("fg {}", COLOR_NAMES[(n - 30) as usize]),
        40..=47 => format!("bg {}", COLOR_NAMES[(n - 40) as usize]),
        60..=65 => format!("ideogram attribute {n}"),
        90..=97 => format!("fg bright {}", COLOR_NAMES[(n - 90) as usize]),
        100..=107 => format!("bg bright {}", COLOR_NAMES[(n - 100) as usize]),
        _ => format!("unknown SGR {n}"),
    }
}

fn target(n: u32) -> &'static str {
    match n {
        38 => "fg",
        48 => "bg",
        _ => "underline colour",
    }
}

/// Describe every effect of SGR parameters (`"1;38;5;208"`).
pub fn sgr_effects(params: &str) -> Vec<Effect> {
    let groups: Vec<&str> = params.split(';').collect();
    let mut out = Vec::new();
    let mut i = 0;
    let num = |s: &str| -> Option<u32> {
        if s.is_empty() {
            Some(0)
        } else {
            s.parse().ok()
        }
    };
    while i < groups.len() {
        let group = groups[i];
        let mut push = |code: String, description: String| {
            out.push(Effect { code, description });
        };
        if group.contains(':') {
            let subs: Vec<&str> = group.split(':').collect();
            let head = num(subs[0]);
            let values: Vec<Option<u32>> = subs[1..]
                .iter()
                .map(|s| if s.is_empty() { None } else { s.parse().ok() })
                .collect();
            let description = match head {
                Some(4) => match values.first().copied().flatten() {
                    Some(0) => "underline off".into(),
                    Some(1) => "underline on (single)".into(),
                    Some(2) => "underline on (double)".into(),
                    Some(3) => "underline on (curly)".into(),
                    Some(4) => "underline on (dotted)".into(),
                    Some(5) => "underline on (dashed)".into(),
                    _ => format!("unknown underline style {group}"),
                },
                Some(t @ (38 | 48 | 58)) => match values.first().copied().flatten() {
                    Some(5) => match values.get(1).copied().flatten() {
                        Some(n) => format!("{} {}", target(t), indexed(n)),
                        None => format!("incomplete {t} (expected {t}:5:n)"),
                    },
                    Some(2) => {
                        // 38:2:cs:r:g:b (ITU T.416) or the common 38:2:r:g:b.
                        let rgbs = if values.len() >= 5 {
                            &values[2..5]
                        } else {
                            &values[1..]
                        };
                        match rgbs {
                            [Some(r), Some(g), Some(b)] => {
                                format!("{} {}", target(t), rgb(*r, *g, *b))
                            }
                            _ => format!("incomplete {t} (expected {t}:2::r:g:b)"),
                        }
                    }
                    _ => format!("unknown colour form {group}"),
                },
                Some(n) => format!("{} (sub-parameters ignored)", simple_sgr(n)),
                None => format!("invalid parameter {group:?}"),
            };
            push(group.to_owned(), description);
            i += 1;
            continue;
        }
        let Some(n) = num(group) else {
            push(group.to_owned(), format!("invalid parameter {group:?}"));
            i += 1;
            continue;
        };
        if matches!(n, 38 | 48 | 58) {
            let mode = groups.get(i + 1).and_then(|s| num(s));
            let take = |k: usize| groups.get(i + k).and_then(|s| s.parse::<u32>().ok());
            match mode {
                Some(5) => {
                    let code = groups[i..(i + 3).min(groups.len())].join(";");
                    match take(2) {
                        Some(v) => push(code, format!("{} {}", target(n), indexed(v))),
                        None => push(code, format!("incomplete {n} (expected {n};5;n)")),
                    }
                    i += 3;
                }
                Some(2) => {
                    let code = groups[i..(i + 5).min(groups.len())].join(";");
                    match (take(2), take(3), take(4)) {
                        (Some(r), Some(g), Some(b)) => {
                            push(code, format!("{} {}", target(n), rgb(r, g, b)))
                        }
                        _ => push(code, format!("incomplete {n} (expected {n};2;r;g;b)")),
                    }
                    i += 5;
                }
                _ => {
                    let code = groups[i..(i + 2).min(groups.len())].join(";");
                    push(
                        code,
                        format!("incomplete {n} (expected ;5 or ;2 colour form)"),
                    );
                    i += 2;
                }
            }
            continue;
        }
        push(group.to_owned(), simple_sgr(n));
        i += 1;
    }
    out
}

fn dec_mode(n: u32) -> String {
    match n {
        1 => "application cursor keys".into(),
        3 => "132-column mode".into(),
        5 => "reverse video".into(),
        6 => "origin mode".into(),
        7 => "auto-wrap".into(),
        9 => "X10 mouse reporting".into(),
        12 => "cursor blinking".into(),
        25 => "cursor visibility".into(),
        47 | 1047 => "alternate screen".into(),
        1000 => "mouse click tracking".into(),
        1002 => "mouse drag tracking".into(),
        1003 => "mouse motion tracking".into(),
        1004 => "focus reporting".into(),
        1005 => "UTF-8 mouse encoding".into(),
        1006 => "SGR mouse encoding".into(),
        1007 => "alternate scroll".into(),
        1015 => "urxvt mouse encoding".into(),
        1048 => "saved cursor".into(),
        1049 => "alternate screen with saved cursor".into(),
        2004 => "bracketed paste".into(),
        2026 => "synchronized output".into(),
        2027 => "grapheme clustering".into(),
        _ => format!("private mode {n}"),
    }
}

/// Describe a CSI sequence other than SGR.
pub fn csi_meaning(params: &str, intermediates: &str, f: char) -> String {
    let (prefix, rest) = match params.chars().next() {
        Some(p @ ('<' | '=' | '>' | '?')) => (Some(p), &params[1..]),
        _ => (None, params),
    };
    let nums: Vec<Option<u32>> = if rest.is_empty() {
        Vec::new()
    } else {
        rest.split(';')
            .map(|s| if s.is_empty() { None } else { s.parse().ok() })
            .collect()
    };
    let n = |i: usize, d: u32| nums.get(i).copied().flatten().unwrap_or(d);
    let plural = |k: u32, what: &str| format!("{k} {what}{}", if k == 1 { "" } else { "s" });
    match (prefix, intermediates, f) {
        (None, "", 'A') => format!("cursor up {}", n(0, 1)),
        (None, "", 'B') => format!("cursor down {}", n(0, 1)),
        (None, "", 'C') => format!("cursor forward {}", n(0, 1)),
        (None, "", 'D') => format!("cursor back {}", n(0, 1)),
        (None, "", 'E') => format!("cursor to start of line {} down", n(0, 1)),
        (None, "", 'F') => format!("cursor to start of line {} up", n(0, 1)),
        (None, "", 'G') => format!("cursor to column {}", n(0, 1)),
        (None, "", 'H' | 'f') => format!("cursor to row {}, column {}", n(0, 1), n(1, 1)),
        (None, "", 'd') => format!("cursor to row {}", n(0, 1)),
        (None | Some('?'), "", 'J') => match n(0, 0) {
            0 => "erase from cursor to end of screen".into(),
            1 => "erase from start of screen to cursor".into(),
            2 => "erase entire screen".into(),
            3 => "erase scrollback".into(),
            k => format!("erase in display ({k})"),
        },
        (None | Some('?'), "", 'K') => match n(0, 0) {
            0 => "erase from cursor to end of line".into(),
            1 => "erase from start of line to cursor".into(),
            2 => "erase entire line".into(),
            k => format!("erase in line ({k})"),
        },
        (None, "", 'L') => format!("insert {}", plural(n(0, 1), "line")),
        (None, "", 'M') => format!("delete {}", plural(n(0, 1), "line")),
        (None, "", 'P') => format!("delete {}", plural(n(0, 1), "character")),
        (None, "", '@') => format!("insert {}", plural(n(0, 1), "blank character")),
        (None, "", 'X') => format!("erase {}", plural(n(0, 1), "character")),
        (None, "", 'S') => format!("scroll up {}", plural(n(0, 1), "line")),
        (None, "", 'T') => format!("scroll down {}", plural(n(0, 1), "line")),
        (None, "", 'I') => format!("cursor forward {}", plural(n(0, 1), "tab stop")),
        (None, "", 'Z') => format!("cursor back {}", plural(n(0, 1), "tab stop")),
        (None, "", 'b') => format!("repeat previous character {} times", n(0, 1)),
        (None, "", 'g') => match n(0, 0) {
            3 => "clear all tab stops".into(),
            _ => "clear tab stop at cursor".into(),
        },
        (Some('?'), "", 'h' | 'l') => {
            let on = f == 'h';
            let modes: Vec<String> = nums
                .iter()
                .map(|m| match (m, on) {
                    (Some(25), true) => "show cursor".into(),
                    (Some(25), false) => "hide cursor".into(),
                    (Some(m), _) => {
                        format!("{} {}", if on { "enable" } else { "disable" }, dec_mode(*m))
                    }
                    (None, _) => "missing mode".into(),
                })
                .collect();
            format!(
                "{} ({})",
                modes.join(", "),
                if on { "DECSET" } else { "DECRST" }
            )
        }
        (None, "", 'h' | 'l') => {
            let verb = if f == 'h' { "set" } else { "reset" };
            let modes: Vec<String> = nums
                .iter()
                .map(|m| match m {
                    Some(4) => format!("{verb} insert mode"),
                    Some(20) => format!("{verb} automatic newline"),
                    Some(m) => format!("{verb} mode {m}"),
                    None => "missing mode".into(),
                })
                .collect();
            modes.join(", ")
        }
        (None, "", 'r') if nums.is_empty() => "reset scrolling region".into(),
        (None, "", 'r') => format!("scrolling region rows {} to {}", n(0, 1), n(1, 0)),
        (None, "", 's') if nums.is_empty() => "save cursor position".into(),
        (None, "", 'u') if nums.is_empty() => "restore cursor position".into(),
        (Some('>'), "", 'u') => format!("push keyboard enhancement flags {}", n(0, 0)),
        (Some('<'), "", 'u') => "pop keyboard enhancement flags".into(),
        (Some('?'), "", 'u') => "query keyboard enhancement flags".into(),
        (Some('='), "", 'u') => format!("set keyboard enhancement flags {}", n(0, 0)),
        (None | Some('?'), "", 'n') => match n(0, 0) {
            5 => "request device status".into(),
            6 => "request cursor position".into(),
            k => format!("device status report ({k})"),
        },
        (None, "", 'c') => "request primary device attributes (DA1)".into(),
        (Some('>'), "", 'c') => "request secondary device attributes (DA2)".into(),
        (Some('='), "", 'c') => "request tertiary device attributes (DA3)".into(),
        (Some('>'), "", 'q') => "request terminal name and version (XTVERSION)".into(),
        (None, "", 't') => match n(0, 0) {
            8 => format!("resize window to {} rows, {} columns", n(1, 0), n(2, 0)),
            14 => "report window size in pixels".into(),
            16 => "report cell size in pixels".into(),
            18 => "report text area size in characters".into(),
            22 => "push window title".into(),
            23 => "pop window title".into(),
            k => format!("window manipulation ({k})"),
        },
        (None, " ", 'q') => match n(0, 0) {
            0 | 1 => "cursor style: blinking block".into(),
            2 => "cursor style: steady block".into(),
            3 => "cursor style: blinking underline".into(),
            4 => "cursor style: steady underline".into(),
            5 => "cursor style: blinking bar".into(),
            6 => "cursor style: steady bar".into(),
            k => format!("cursor style {k}"),
        },
        (None, "!", 'p') => "soft terminal reset (DECSTR)".into(),
        (Some('?'), "$", 'p') => format!("request private mode {} (DECRQM)", n(0, 0)),
        (Some('>'), "", 'm') => format!("set key modifier options ({rest})"),
        (Some('>'), "", 'n') => format!("reset key modifier options ({rest})"),
        (p, i, f) => format!(
            "unrecognised control sequence (CSI {}{}{})",
            p.map(String::from).unwrap_or_default(),
            rest,
            format_args!("{i}{f}")
        ),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

/// Describe an OSC body (between the introducer and the terminator).
pub fn osc_meaning(data: &str) -> String {
    let (code, rest) = data.split_once(';').unwrap_or((data, ""));
    match code {
        "0" => format!("set window and icon title to {:?}", truncate(rest, 60)),
        "1" => format!("set icon name to {:?}", truncate(rest, 60)),
        "2" => format!("set window title to {:?}", truncate(rest, 60)),
        "4" => {
            let parts: Vec<&str> = rest.split(';').collect();
            let pairs: Vec<String> = parts
                .chunks(2)
                .map(|p| match p {
                    [n, "?"] => format!("query palette colour {n}"),
                    [n, spec] => format!("set palette colour {n} to {spec}"),
                    [n] => format!("palette colour {n} (missing value)"),
                    _ => String::new(),
                })
                .collect();
            pairs.join(", ")
        }
        "7" => format!("report working directory {rest}"),
        "8" => {
            let (params, uri) = rest.split_once(';').unwrap_or((rest, ""));
            if uri.is_empty() {
                "close hyperlink".into()
            } else if params.is_empty() {
                format!("open hyperlink to {uri}")
            } else {
                format!("open hyperlink to {uri} ({params})")
            }
        }
        "9" => match rest.split_once(';') {
            Some(("4", progress)) => format!("progress indicator ({progress})"),
            _ => format!("notification: {:?}", truncate(rest, 60)),
        },
        "10" | "11" | "12" => {
            let what = match code {
                "10" => "default foreground colour",
                "11" => "default background colour",
                _ => "cursor colour",
            };
            if rest == "?" {
                format!("query {what}")
            } else {
                format!("set {what} to {rest}")
            }
        }
        "22" => format!("set pointer shape to {rest}"),
        "52" => {
            let (target, payload) = rest.split_once(';').unwrap_or((rest, ""));
            let target = if target.is_empty() { "c" } else { target };
            match payload {
                "?" => format!("query clipboard ({target})"),
                "" => format!("clear clipboard ({target})"),
                p => format!("set clipboard ({target}) to {} bytes of base64", p.len()),
            }
        }
        "104" if rest.is_empty() => "reset all palette colours".into(),
        "104" => format!("reset palette colours {rest}"),
        "110" => "reset default foreground colour".into(),
        "111" => "reset default background colour".into(),
        "112" => "reset cursor colour".into(),
        "133" | "633" => {
            let (mark, extra) = rest.split_once(';').unwrap_or((rest, ""));
            let what = match mark {
                "A" => "prompt start".into(),
                "B" => "command input start".into(),
                "C" => "command output start".into(),
                "D" if extra.is_empty() => "command finished".into(),
                "D" => format!("command finished (exit status {extra})"),
                "E" => "command line".into(),
                "P" => format!("property {extra}"),
                m => format!("mark {m}"),
            };
            let who = if code == "133" {
                "shell integration"
            } else {
                "VS Code shell integration"
            };
            format!("{who}: {what}")
        }
        "777" => format!("desktop notification ({})", truncate(rest, 60)),
        "1337" => {
            if let Some(file) = rest.strip_prefix("File=") {
                let (args, payload) = file.split_once(':').unwrap_or((file, ""));
                format!(
                    "iTerm2 inline file ({}), {} bytes of base64",
                    truncate(args, 40),
                    payload.len()
                )
            } else {
                format!("iTerm2 command {}", truncate(rest, 40))
            }
        }
        c if c.parse::<u32>().is_ok() => format!("unrecognised OSC {c}"),
        _ => "OSC with no numeric code".into(),
    }
}

/// Describe a control string; returns the meaning and the payload length.
fn string_meaning(kind: StringKind, data: &str) -> (String, usize) {
    match kind {
        StringKind::Dcs => {
            if let Some(inner) = data.strip_prefix("tmux;") {
                return (
                    format!("tmux passthrough, {} bytes", inner.len()),
                    inner.len(),
                );
            }
            let params_end = data
                .find(|c: char| !('\x30'..='\x3f').contains(&c))
                .unwrap_or(data.len());
            let params = &data[..params_end];
            let after = &data[params_end..];
            let inter_end = after
                .find(|c: char| !('\x20'..='\x2f').contains(&c))
                .unwrap_or(after.len());
            let intermediates = &after[..inter_end];
            let mut chars = after[inter_end..].chars();
            let Some(f) = chars.next() else {
                return ("empty device control string".into(), 0);
            };
            let payload = chars.as_str();
            let len = payload.len();
            let meaning = match (intermediates, f) {
                ("", 'q') => format!(
                    "sixel image, {len} bytes of data{}",
                    if params.is_empty() {
                        String::new()
                    } else {
                        format!(" (params {params})")
                    }
                ),
                ("$", 'q') => format!("request setting {payload:?} (DECRQSS)"),
                ("+", 'q') => format!("request terminfo capabilities {payload} (XTGETTCAP)"),
                ("", 's') if params == "=1" => "begin synchronized update".into(),
                ("", 's') if params == "=2" => "end synchronized update".into(),
                (i, f) => format!("device control string (final {i}{f}), {len} bytes"),
            };
            (meaning, len)
        }
        StringKind::Apc => {
            if let Some(cmd) = data.strip_prefix('G') {
                let (keys, payload) = cmd.split_once(';').unwrap_or((cmd, ""));
                (
                    format!(
                        "kitty graphics command ({}), {} bytes of payload",
                        truncate(keys, 40),
                        payload.len()
                    ),
                    payload.len(),
                )
            } else {
                (
                    format!("application program command, {} bytes", data.len()),
                    data.len(),
                )
            }
        }
        StringKind::Pm => (format!("privacy message, {} bytes", data.len()), data.len()),
        StringKind::Sos => (format!("start of string, {} bytes", data.len()), data.len()),
    }
}

/// `raw` with every control shown by name: `ESC[1;31m`, `<BEL>`, `<CSI>`.
pub fn escape_visible(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c == ESC {
            out.push_str("ESC");
        } else if is_special(c) {
            out.push('<');
            out.push_str(control_name(c as u32 as u8));
            out.push('>');
        } else {
            out.push(c);
        }
    }
    out
}

/// How an [`ExplanationView`] lays out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// A table of tokens, then the visible text.
    #[default]
    Table,
    /// The text with `⟨…⟩` markers where the escapes were.
    Inline,
}

/// A renderable [`Explanation`]. Colourless output carries the full meaning.
pub struct ExplanationView<'a> {
    explanation: &'a Explanation,
    mode: ViewMode,
    escapes_only: bool,
    show_visible: bool,
    raw_width: usize,
}

impl<'a> ExplanationView<'a> {
    pub fn new(explanation: &'a Explanation) -> Self {
        Self {
            explanation,
            mode: ViewMode::Table,
            escapes_only: false,
            show_visible: true,
            raw_width: 40,
        }
    }
    pub fn mode(mut self, mode: ViewMode) -> Self {
        self.mode = mode;
        self
    }
    /// Leave text tokens out of the table.
    pub fn escapes_only(mut self, value: bool) -> Self {
        self.escapes_only = value;
        self
    }
    /// Show the visible text after the table (default on).
    pub fn show_visible(mut self, value: bool) -> Self {
        self.show_visible = value;
        self
    }
    /// Characters of raw input shown per row before eliding (default 40).
    pub fn raw_width(mut self, value: usize) -> Self {
        self.raw_width = value.max(8);
        self
    }

    /// The inline form: text with markers.
    pub fn inline_text(&self, ascii: bool) -> String {
        let (open, close) = if ascii { ("<", ">") } else { ("⟨", "⟩") };
        let mut out = String::new();
        for t in &self.explanation.tokens {
            match &t.token {
                Token::Text(text) => out.push_str(text),
                Token::Control { byte: b'\n', .. } => out.push('\n'),
                Token::Control { byte: b'\t', .. } => out.push('\t'),
                Token::Control { name, .. } => {
                    out.push_str(&format!("{open}{name}{close}"));
                }
                Token::Invalid { reason, .. } => {
                    out.push_str(&format!("{open}invalid: {reason}{close}"));
                }
                other => out.push_str(&format!("{open}{}{close}", other.meaning())),
            }
        }
        out
    }
}

impl Renderable for ExplanationView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.mode == ViewMode::Inline {
            let text = Text::new(self.inline_text(console.ascii_only()));
            return text.rich_render(console, options);
        }
        let mut table = Table::new();
        for header in ["Offset", "Raw", "Kind", "Meaning"] {
            table.add_column(header);
        }
        for t in &self.explanation.tokens {
            if self.escapes_only && matches!(t.token, Token::Text(_)) {
                continue;
            }
            let raw = escape_visible(&t.token.raw());
            let raw = if raw.chars().count() > self.raw_width {
                let head: String = raw.chars().take(self.raw_width).collect();
                format!("{head}… ({} bytes)", t.len)
            } else {
                raw
            };
            let offset = t.offset.to_string();
            // Captured text and escapes are data: `[info]` must not become a style.
            table.add_row_text(vec![
                Text::new(offset),
                Text::new(raw),
                Text::new(t.token.kind()),
                Text::new(t.token.meaning()),
            ]);
        }
        let mut out = table.rich_render(console, options);
        if out.last().is_some_and(|s| !s.text.ends_with('\n')) {
            out.push(Segment::line());
        }
        if self.show_visible {
            out.push(Segment::new("Visible text:", None));
            out.push(Segment::line());
            if !self.explanation.visible_text.is_empty() {
                out.extend(Text::new(&self.explanation.visible_text).rich_render(console, options));
            }
        }
        out
    }
}
