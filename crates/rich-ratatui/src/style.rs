//! Style and colour mapping between rich and ratatui.
//!
//! The mapping is lossless for the common subset: no colour, the terminal
//! default (rich's `default` ⇄ ratatui's [`Color::Reset`](RColor::Reset)),
//! the 16 named colours, the 256-colour palette, 24-bit RGB, and nine
//! attributes (bold, dim, italic, underline, blink, rapid blink, reverse,
//! conceal/hidden, strike), each tri-state: on, explicitly off (`not bold`,
//! ratatui's `sub_modifier`), or unset. What it loses is listed on each
//! function, and together in the [crate docs](crate#what-converts-and-what-is-lost).

use ratatui_core::style::{Color as RColor, Modifier, Style as RStyle};
use rich::color::ColorType;
use rich::{Color, Style};

/// rich's attribute indices (see [`Style::attr`]), names, and the ratatui
/// modifier each maps to. The four rich attributes missing here
/// (`underline2`, `frame`, `encircle`, `overline`: indices 9–12) have no
/// ratatui modifier.
pub(crate) const ATTRIBUTES: [(usize, &str, Modifier); 9] = [
    (0, "bold", Modifier::BOLD),
    (1, "dim", Modifier::DIM),
    (2, "italic", Modifier::ITALIC),
    (3, "underline", Modifier::UNDERLINED),
    (4, "blink", Modifier::SLOW_BLINK),
    (5, "blink2", Modifier::RAPID_BLINK),
    (6, "reverse", Modifier::REVERSED),
    (7, "conceal", Modifier::HIDDEN),
    (8, "strike", Modifier::CROSSED_OUT),
];

/// The 16 standard colours in ANSI number order, as ratatui names them.
pub(crate) const NAMED: [RColor; 16] = [
    RColor::Black,
    RColor::Red,
    RColor::Green,
    RColor::Yellow,
    RColor::Blue,
    RColor::Magenta,
    RColor::Cyan,
    RColor::Gray,
    RColor::DarkGray,
    RColor::LightRed,
    RColor::LightGreen,
    RColor::LightYellow,
    RColor::LightBlue,
    RColor::LightMagenta,
    RColor::LightCyan,
    RColor::White,
];

/// The same 16, as rich names them (upstream `rich/color.py`'s
/// `ANSI_COLOR_NAMES`).
const RICH_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

/// Map a rich colour to ratatui's.
///
/// - rich's `default` is ratatui's [`Reset`](RColor::Reset);
/// - a standard colour (`red`, `bright_cyan`) is ratatui's named colour with
///   the same ANSI number (rich's `white`, number 7, is ratatui's `Gray`;
///   `bright_white` is `White`);
/// - an 8-bit colour (`color(200)`, `grey0`) is [`Indexed`](RColor::Indexed),
///   even below 16, so an indexed colour round-trips as indexed;
/// - a truecolor (`#ff8001`, `rgb(…)`) is [`Rgb`](RColor::Rgb).
///
/// **Lossy:** rich's legacy Windows colours become the named colour with the
/// same number. A malformed colour (neither a number nor an RGB triplet)
/// becomes `Reset`, the harmless reading.
pub fn to_ratatui_color(color: &Color) -> RColor {
    match (color.kind, color.number, color.triplet) {
        (ColorType::Default, _, _) => RColor::Reset,
        (ColorType::Standard | ColorType::Windows, Some(n), _) => NAMED[usize::from(n & 15)],
        (ColorType::EightBit, Some(n), _) => RColor::Indexed(n),
        (_, _, Some(t)) => RColor::Rgb(t.red, t.green, t.blue),
        _ => RColor::Reset,
    }
}

/// Map a ratatui colour to rich's: the inverse of [`to_ratatui_color`].
///
/// The names are rich's canonical ones: `bright_red`, `color(16)`,
/// `#rrggbb`. **Lossy** in the other direction only: a rich colour that
/// went to ratatui comes back by number, so `grey0` returns as `color(16)`
/// and `#FF0000` as `#ff0000`. The colour itself (kind, number, RGB) is the
/// same; only [`Color::name`] differs.
pub fn to_rich_color(color: RColor) -> Color {
    let number = match color {
        RColor::Reset => return Color::default_color(),
        RColor::Indexed(n) => {
            return Color {
                name: format!("color({n})"),
                kind: ColorType::EightBit,
                number: Some(n),
                triplet: None,
            }
        }
        RColor::Rgb(r, g, b) => return Color::from_rgb(r, g, b),
        RColor::Black => 0,
        RColor::Red => 1,
        RColor::Green => 2,
        RColor::Yellow => 3,
        RColor::Blue => 4,
        RColor::Magenta => 5,
        RColor::Cyan => 6,
        RColor::Gray => 7,
        RColor::DarkGray => 8,
        RColor::LightRed => 9,
        RColor::LightGreen => 10,
        RColor::LightYellow => 11,
        RColor::LightBlue => 12,
        RColor::LightMagenta => 13,
        RColor::LightCyan => 14,
        RColor::White => 15,
    };
    Color {
        name: RICH_NAMES[usize::from(number)].to_string(),
        kind: ColorType::Standard,
        number: Some(number),
        triplet: None,
    }
}

/// Map a rich style to a ratatui style.
///
/// Unset stays unset (a `None` colour, a modifier in neither set), so the
/// result *patches* whatever a cell had, as rich's own `Style::combine`
/// does and as ratatui's `Buffer::set_style` expects. An attribute that is
/// explicitly off (`not bold`) goes to `sub_modifier`.
///
/// **Lossy:** rich's `underline2`, `frame`, `encircle` and `overline` have no
/// ratatui modifier and are dropped, as are a style's hyperlink and meta
/// (a ratatui cell holds a symbol and a style, nothing else).
pub fn to_ratatui_style(style: &Style) -> RStyle {
    let mut out = RStyle {
        fg: style.color().map(to_ratatui_color),
        bg: style.bgcolor().map(to_ratatui_color),
        ..RStyle::default()
    };
    for (index, _, modifier) in ATTRIBUTES {
        match style.attr(index) {
            Some(true) => out.add_modifier |= modifier,
            Some(false) => out.sub_modifier |= modifier,
            None => {}
        }
    }
    out
}

/// Map a ratatui style to a rich style, or `None` when it sets nothing: the
/// inverse of [`to_ratatui_style`].
///
/// An explicit [`Reset`](RColor::Reset) colour becomes rich's `default`
/// here (unlike in a [buffer](crate::buffer), where `Reset` means unset).
///
/// **Lossy:** ratatui's underline colour (its `underline-color` feature) has
/// no rich equivalent and is dropped; colour names are rich's canonical ones
/// (see [`to_rich_color`]).
pub fn to_rich_style(style: RStyle) -> Option<Style> {
    let mut words = Vec::new();
    for (_, name, modifier) in ATTRIBUTES {
        if style.add_modifier.contains(modifier) {
            words.push(name.to_string());
        } else if style.sub_modifier.contains(modifier) {
            words.push(format!("not {name}"));
        }
    }
    if words.is_empty() && style.fg.is_none() && style.bg.is_none() {
        return None;
    }
    // rich has no public per-attribute setter, so the attributes go through
    // its parser. The words are all valid, so parsing cannot fail. Callers
    // converting many cells cache the result (see `buffer::StyleCache`).
    let mut out = Style::parse(&words.join(" ")).unwrap_or_default();
    if let Some(fg) = style.fg {
        out = out.with_color(to_rich_color(fg));
    }
    if let Some(bg) = style.bg {
        out = out.with_bgcolor(to_rich_color(bg));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::ColorSystem;

    /// Two rich colours are the same colour (names aside).
    fn same_color(a: &Color, b: &Color) -> bool {
        let kind = |c: &Color| match c.kind {
            ColorType::Windows => ColorType::Standard,
            k => k,
        };
        kind(a) == kind(b) && a.number == b.number && a.triplet == b.triplet
    }

    #[test]
    fn colours_round_trip() {
        let mut colours = vec![RColor::Reset];
        colours.extend(NAMED);
        colours.extend((0..=255).map(RColor::Indexed));
        colours.extend([RColor::Rgb(0, 0, 0), RColor::Rgb(255, 128, 1)]);
        for colour in colours {
            assert_eq!(to_ratatui_color(&to_rich_color(colour)), colour);
            let style = RStyle::default().fg(colour).bg(colour);
            assert_eq!(to_ratatui_style(&to_rich_style(style).unwrap()), style);
        }
        // rich → ratatui → rich, from rich's own parser.
        for spec in [
            "default",
            "red",
            "bright_cyan",
            "grey0",
            "color(200)",
            "#ff8001",
        ] {
            let colour = Color::parse(spec).unwrap();
            let back = to_rich_color(to_ratatui_color(&colour));
            assert!(same_color(&colour, &back), "{spec}: {colour:?} vs {back:?}");
        }
        // The named colours carry rich's names.
        assert_eq!(to_rich_color(RColor::Gray).name, "white");
        assert_eq!(to_rich_color(RColor::White).name, "bright_white");
    }

    #[test]
    fn modifiers_round_trip() {
        for (index, name, modifier) in ATTRIBUTES {
            let on = Style::parse(name).unwrap();
            assert_eq!(on.attr(index), Some(true));
            let ratatui = to_ratatui_style(&on);
            assert_eq!(ratatui, RStyle::default().add_modifier(modifier));
            assert_eq!(to_rich_style(ratatui), Some(on));

            let off = Style::parse(&format!("not {name}")).unwrap();
            let ratatui = to_ratatui_style(&off);
            assert_eq!(ratatui, RStyle::default().remove_modifier(modifier));
            assert_eq!(to_rich_style(ratatui), Some(off));
        }
        // All at once, with colours.
        let all = Style::parse(
            "bold dim italic underline blink blink2 reverse conceal strike red on #102030",
        )
        .unwrap();
        let back = to_rich_style(to_ratatui_style(&all)).unwrap();
        assert_eq!(to_ratatui_style(&back), to_ratatui_style(&all));
        assert_eq!(
            back.ansi_codes(ColorSystem::Truecolor),
            all.ansi_codes(ColorSystem::Truecolor)
        );
        // Nothing set is no style.
        assert_eq!(to_rich_style(RStyle::default()), None);
        // The lossy ones: dropped, not mistranslated.
        let lossy = Style::parse("underline2 frame encircle overline").unwrap();
        assert_eq!(to_ratatui_style(&lossy), RStyle::default());
    }
}
