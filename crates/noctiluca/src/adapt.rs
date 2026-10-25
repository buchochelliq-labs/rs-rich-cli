//! Terminal adaptations: named, generic rules about constructs and cells,
//! applied to every page alike (`docs/design/browser.md`, "Terminal
//! adaptations"). None of them knows a site: each reads only what the
//! [model](crate::model) has from the snapshot and the accessibility tree.
//!
//! Each rule can be switched off on its own, in the config's `[adapt]`
//! table and on the command line (`--adapt=-decorative-images,-collapse-nav`);
//! `--no-adapt` switches all of them off, and a key toggles between the
//! adapted and the faithful projection while browsing. A rule that changes
//! what the page means is off until asked for; the others are on.
//!
//! Some rules are applied here and in the [layout](crate::layout); the
//! picture rules are applied where pictures are drawn, and the bandwidth
//! rule by the media scheduler. They all read the one [`Adapt`] set.

use crate::engine::Rect;
use crate::model::Media;

/// A terminal adaptation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    /// A spacer or tracking pixel (under [`DECORATIVE_PX`] a side), and an
    /// icon-sized picture marked `alt=""`, `aria-hidden` or
    /// `role=presentation`, is not a media region.
    DecorativeImages,
    /// An image at most two rows tall in a line of text stays in its line.
    InlinePictures,
    /// A picture is at most [`MAX_PICTURE_ROWS`] rows tall; what is under
    /// it in its columns moves up by the rows saved.
    CapPictures,
    /// A picture with alt text and no caption gets an `Image: …` line.
    Captions,
    /// A button reads as its accessible name in brackets, an icon-only
    /// one too.
    IconButtons,
    /// A grid's or a wrapping flex row's items are snapped to equal cell
    /// widths that tile it, with fewer columns when a column would be
    /// narrower than [`MIN_GRID_CELLS`].
    SnapGrids,
    /// On a terminal narrower than [`NARROW_COLUMNS`], a navigation or
    /// complementary landmark (with no field and no live log in it)
    /// collapses to one row of its links.
    CollapseNav,
    /// A picture of text (in a heading, a link or a button, or a wide,
    /// short banner) with alt text is drawn as that text.
    TextOverPictures,
    /// A live log (`role=log`, `aria-live`, or a scroll box the page
    /// appends to) shows its newest entries and holds still while the
    /// user has scrolled it up.
    LiveTail,
    /// Each moving region's frame rate and tier step down when the
    /// terminal is short of bandwidth.
    Bandwidth,
    /// The page's text colours, for the terminal: saturated colours stay
    /// (a red "LIVE", an author's name), greys and colours near the
    /// foreground follow the terminal's own, and every kept colour is
    /// made readable on the terminal's background. Off, text has no page
    /// colour at all.
    PageColors,
}

/// What a rule is, for the config, the command line and the about page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleInfo {
    pub rule: Rule,
    /// Its name in the config and on the command line.
    pub name: &'static str,
    /// What it does, in a line.
    pub what: &'static str,
    /// Whether it is on by default: off only when it changes what the
    /// page means.
    pub default: bool,
}

/// Every rule, in the about page's order.
pub const RULES: &[RuleInfo] = &[
    RuleInfo {
        rule: Rule::DecorativeImages,
        name: "decorative-images",
        what: "drop spacers, tracking pixels and icons marked decorative",
        default: true,
    },
    RuleInfo {
        rule: Rule::InlinePictures,
        name: "inline-pictures",
        what: "keep small pictures (emotes, badges, icons) in their line",
        default: true,
    },
    RuleInfo {
        rule: Rule::CapPictures,
        name: "cap-pictures",
        what: "cap picture heights; what is under them moves up",
        default: true,
    },
    RuleInfo {
        rule: Rule::Captions,
        name: "captions",
        what: "an Image: line under a picture with alt text and no caption",
        default: true,
    },
    RuleInfo {
        rule: Rule::IconButtons,
        name: "icon-buttons",
        what: "show buttons, icon-only ones too, as [ label ] from their name",
        default: true,
    },
    RuleInfo {
        rule: Rule::SnapGrids,
        name: "snap-grids",
        what: "snap grids to the width, columns from the cells there are",
        default: true,
    },
    RuleInfo {
        rule: Rule::CollapseNav,
        name: "collapse-nav",
        what: "collapse navigation and sidebars to a row on narrow terminals",
        default: true,
    },
    RuleInfo {
        rule: Rule::TextOverPictures,
        name: "text-over-pictures",
        what: "draw a picture of text as its text (changes meaning: off)",
        default: false,
    },
    RuleInfo {
        rule: Rule::LiveTail,
        name: "live-tail",
        what: "live logs show their newest entries unless scrolled up",
        default: true,
    },
    RuleInfo {
        rule: Rule::PageColors,
        name: "page-colors",
        what: "keep saturated text colours, readable; greys follow the terminal",
        default: true,
    },
    RuleInfo {
        rule: Rule::Bandwidth,
        name: "bandwidth",
        what: "lower frame rate and tier when the terminal is short of bandwidth",
        default: true,
    },
];

impl Rule {
    pub fn info(self) -> &'static RuleInfo {
        RULES
            .iter()
            .find(|r| r.rule == self)
            .expect("every rule is listed")
    }

    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// The rule with this name.
    pub fn parse(name: &str) -> Option<Rule> {
        RULES
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(name.trim()))
            .map(|r| r.rule)
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Which rules are on.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Adapt(u16);

impl Default for Adapt {
    /// The defaults: every rule that keeps the page's meaning.
    fn default() -> Self {
        let mut adapt = Adapt::none();
        for rule in RULES.iter().filter(|r| r.default) {
            adapt.set(rule.rule, true);
        }
        adapt
    }
}

impl std::fmt::Debug for Adapt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set()
            .entries(self.active().map(Rule::name))
            .finish()
    }
}

impl Adapt {
    /// No rule: the faithful projection.
    pub const fn none() -> Adapt {
        Adapt(0)
    }

    pub fn on(self, rule: Rule) -> bool {
        self.0 & rule.bit() != 0
    }

    pub fn set(&mut self, rule: Rule, on: bool) {
        if on {
            self.0 |= rule.bit();
        } else {
            self.0 &= !rule.bit();
        }
    }

    #[must_use]
    pub fn with(mut self, rule: Rule, on: bool) -> Adapt {
        self.set(rule, on);
        self
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// The rules that are on, in [`RULES`]' order.
    pub fn active(self) -> impl Iterator<Item = Rule> {
        RULES.iter().map(|r| r.rule).filter(move |r| self.on(*r))
    }

    /// `self` changed by a list as the command line takes it:
    /// `-name` switches a rule off, `+name` or `name` on, `none` all off,
    /// `default` back to the defaults; commas or spaces between.
    pub fn apply(mut self, list: &str) -> Result<Adapt, String> {
        for item in list
            .split([',', ' '])
            .map(str::trim)
            .filter(|i| !i.is_empty())
        {
            let (on, name) = match item.strip_prefix('-') {
                Some(name) => (false, name),
                None => (true, item.strip_prefix('+').unwrap_or(item)),
            };
            match name {
                "none" if on => self = Adapt::none(),
                "default" if on => self = Adapt::default(),
                "all" => {
                    for rule in RULES {
                        self.set(rule.rule, on);
                    }
                }
                _ => match Rule::parse(name) {
                    Some(rule) => self.set(rule, on),
                    None => {
                        return Err(format!(
                            "{name:?} is not an adaptation (they are {})",
                            RULES.iter().map(|r| r.name).collect::<Vec<_>>().join(", ")
                        ))
                    }
                },
            }
        }
        Ok(self)
    }
}

/// An image smaller than this many CSS pixels a side is a spacer or a
/// tracking pixel.
pub const DECORATIVE_PX: f64 = 8.0;
/// A marked picture at most this many CSS pixels tall (under a terminal
/// row) is an icon; an 18 px comment avatar is not.
pub const ICON_PX: f64 = 16.0;
/// The most rows a picture takes when pictures are capped.
pub const MAX_PICTURE_ROWS: usize = 14;
/// Below this many columns a terminal is narrow: navigation collapses.
pub const NARROW_COLUMNS: u16 = 100;
/// A grid's column is at least this many cells wide when grids snap.
pub const MIN_GRID_CELLS: usize = 16;
/// A picture of text is a banner at least this many times wider than
/// tall.
pub const BANNER_RATIO: f64 = 4.0;

/// Whether `media` is decorative: a spacer or a tracking pixel (under
/// [`DECORATIVE_PX`] a side), or an icon-sized picture (at most
/// [`ICON_PX`] tall) marked `alt=""`, `aria-hidden` or
/// `role=presentation`. An empty-alt picture of real size (a card's
/// thumbnail, a comment's avatar, titled by the text beside it) is kept.
/// Video, audio, canvases and frames never are.
pub fn decorative(media: &Media) -> bool {
    media.kind == crate::model::MediaKind::Image
        && is_decorative(
            media.rect.width,
            media.rect.height,
            media.empty_alt || media.aria_hidden,
        )
}

/// [`decorative`] from a picture's size and whether it is marked
/// decorative (`alt=""`, `aria-hidden`, `role=presentation`).
pub fn is_decorative(width: f64, height: f64, marked: bool) -> bool {
    width < DECORATIVE_PX || height < DECORATIVE_PX || (marked && height <= ICON_PX)
}

/// Whether `media` is a picture of text: an image with alt text that
/// stands for a heading, a link or a button, or a wide, short banner.
pub fn picture_of_text(media: &Media, cell_height: f64) -> bool {
    media.kind == crate::model::MediaKind::Image
        && !media.alt.trim().is_empty()
        && (media.in_label
            || (media.rect.width >= BANNER_RATIO * media.rect.height
                && media.rect.height <= 3.0 * cell_height))
}

/// How many rows a picture of `rect` takes, `cell_height` CSS pixels a
/// row, and how many columns of its `columns`: capped, it keeps its
/// aspect ratio, narrower.
pub fn picture_cells(
    rect: Rect,
    columns: usize,
    cell_height: f64,
    adapt: Adapt,
    max_rows: usize,
) -> (usize, usize) {
    let rows = ((rect.height / cell_height.max(1.0)).round() as usize).clamp(1, max_rows);
    if !adapt.on(Rule::CapPictures) || rows <= MAX_PICTURE_ROWS {
        return (rows, columns.max(1));
    }
    let width = (columns as f64 * MAX_PICTURE_ROWS as f64 / rows as f64).round() as usize;
    (MAX_PICTURE_ROWS, width.clamp(1, columns.max(1)))
}

/// The terminal's background, light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    Dark,
    Light,
}

/// The terminal's background as `COLORFGBG` says it (`15;0`: light text
/// on black), a convention many terminals set; dark when unknown.
pub fn background() -> Background {
    static FOUND: std::sync::OnceLock<Background> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| {
        let bg = std::env::var("COLORFGBG")
            .ok()
            .and_then(|v| v.rsplit(';').next()?.trim().parse::<u8>().ok());
        match bg {
            Some(7 | 9..=15) => Background::Light,
            _ => Background::Dark,
        }
    })
}

/// How far from grey a colour must be (its chroma, 0 to 1) to keep it.
pub const MIN_CHROMA: f64 = 0.25;

/// A page's CSS colour (`rgb(255, 0, 0)`, `rgba(…)`) for the terminal,
/// by the [page colours](Rule::PageColors) rule: `None` for a grey, a
/// colour near black or white, or a transparent one (the terminal's own
/// foreground then); a saturated one as a rich colour, lifted or darkened
/// until it reads on `background`, keeping its hue.
pub fn page_color(css: &str, background: Background) -> Option<String> {
    let css = css.trim();
    let inner = css
        .strip_prefix("rgba(")
        .or_else(|| css.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<f64> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if parts.len() < 3 || parts.get(3).is_some_and(|a| *a < 0.1) {
        return None;
    }
    let (mut r, mut g, mut b) = (
        parts[0].clamp(0.0, 255.0),
        parts[1].clamp(0.0, 255.0),
        parts[2].clamp(0.0, 255.0),
    );
    let chroma = (r.max(g).max(b) - r.min(g).min(b)) / 255.0;
    if chroma < MIN_CHROMA {
        return None;
    }
    let luma = |r: f64, g: f64, b: f64| (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
    // Mix toward white (on dark) or black (on light) until it reads.
    for _ in 0..8 {
        let l = luma(r, g, b);
        match background {
            Background::Dark if l < 0.45 => {
                r += (255.0 - r) * 0.2;
                g += (255.0 - g) * 0.2;
                b += (255.0 - b) * 0.2;
            }
            Background::Light if l > 0.55 => {
                r *= 0.8;
                g *= 0.8;
                b *= 0.8;
            }
            _ => break,
        }
    }
    Some(format!(
        "rgb({},{},{})",
        r.round() as u8,
        g.round() as u8,
        b.round() as u8
    ))
}

/// The parts of a CSS colour (`rgb(…)`, `rgba(…)`): red, green and blue,
/// 0 to 255; `None` for another form or a transparent one.
fn rgb_of(css: &str) -> Option<(f64, f64, f64)> {
    let css = css.trim();
    let inner = css
        .strip_prefix("rgba(")
        .or_else(|| css.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<f64> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if parts.len() < 3 || parts.get(3).is_some_and(|a| *a < 0.1) {
        return None;
    }
    Some((
        parts[0].clamp(0.0, 255.0),
        parts[1].clamp(0.0, 255.0),
        parts[2].clamp(0.0, 255.0),
    ))
}

/// A page's CSS colour as it is, with the [page colours](Rule::PageColors)
/// rule off: as written on a light terminal (on the page's white
/// [backdrop](backdrop)); on a dark one, with its lightness inverted in
/// OKLab (`L` to `1 - L`, hue and chroma kept), so near-black text becomes
/// near-white and a red stays red (inverting RGB would make it cyan).
/// `None` for a transparent colour or another form.
pub fn raw_color(css: &str, background: Background) -> Option<String> {
    let (r, g, b) = rgb_of(css)?;
    let (r, g, b) = match background {
        Background::Light => (r, g, b),
        Background::Dark => {
            let (l, a, bb) = oklab(r, g, b);
            from_oklab(1.0 - l, a, bb)
        }
    };
    Some(format!(
        "rgb({},{},{})",
        r.round() as u8,
        g.round() as u8,
        b.round() as u8
    ))
}

/// The background drawn behind the projected page with the page colours
/// rule off: white on a light terminal, so the page reads as designed;
/// black on a dark one, under its inverted colours. A rich style.
pub fn backdrop(background: Background) -> &'static str {
    match background {
        Background::Light => "on rgb(255,255,255)",
        Background::Dark => "on rgb(0,0,0)",
    }
}

/// sRGB (0 to 255) to OKLab.
fn oklab(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let linear = |c: f64| {
        let c = c / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(r), linear(g), linear(b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    (
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    )
}

/// OKLab to sRGB (0 to 255), clamped into the gamut.
fn from_oklab(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    let r = 4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_;
    let g = -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_;
    let b = -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701_0 * s_;
    let gamma = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        let c = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        c * 255.0
    };
    (gamma(r), gamma(g), gamma(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(rich: &str) -> (f64, f64, f64) {
        rgb_of(rich.replace(',', ", ").as_str()).unwrap()
    }

    /// A colour's hue in degrees, from OKLab.
    fn hue(rgb: (f64, f64, f64)) -> f64 {
        let (_, a, b) = oklab(rgb.0, rgb.1, rgb.2);
        b.atan2(a).to_degrees()
    }

    #[test]
    fn raw_colours_invert_lightness_on_a_dark_terminal_and_keep_their_hue() {
        // Near-black page text comes out near-white on a dark terminal...
        let text = parts(&raw_color("rgb(17, 17, 17)", Background::Dark).unwrap());
        assert!(
            text.0 > 185.0 && text.1 > 185.0 && text.2 > 185.0,
            "{text:?}"
        );
        // ...and stays as written on a light one.
        assert_eq!(
            raw_color("rgb(17, 17, 17)", Background::Light).as_deref(),
            Some("rgb(17,17,17)")
        );
        // A saturated red keeps its hue in both modes (RGB inversion would
        // make it cyan).
        let red = (220.0, 20.0, 20.0);
        for background in [Background::Dark, Background::Light] {
            let out = parts(&raw_color("rgb(220, 20, 20)", background).unwrap());
            assert!(
                out.0 > out.1 + 60.0 && out.0 > out.2 + 60.0,
                "{background:?} {out:?}"
            );
            assert!((hue(out) - hue(red)).abs() < 12.0, "{background:?} {out:?}");
        }
        assert_eq!(raw_color("rgba(0, 0, 0, 0)", Background::Dark), None);
        // White behind the page on a light terminal, black on a dark one.
        assert_eq!(backdrop(Background::Light), "on rgb(255,255,255)");
        assert_eq!(backdrop(Background::Dark), "on rgb(0,0,0)");
        // OKLab round-trips.
        let (l, a, b) = oklab(12.0, 200.0, 99.0);
        let back = from_oklab(l, a, b);
        assert!((back.0 - 12.0).abs() < 0.5 && (back.1 - 200.0).abs() < 0.5);
    }

    #[test]
    fn defaults_keep_meaning() {
        let adapt = Adapt::default();
        assert!(adapt.on(Rule::DecorativeImages) && adapt.on(Rule::LiveTail));
        assert!(!adapt.on(Rule::TextOverPictures));
        assert!(Adapt::none().is_none());
        assert_eq!(RULES.len(), 11);
        for info in RULES {
            assert_eq!(Rule::parse(info.name), Some(info.rule));
        }
    }

    #[test]
    fn lists_switch_rules_off_and_on() {
        let adapt = Adapt::default()
            .apply("-decorative-images,-collapse-nav")
            .unwrap();
        assert!(!adapt.on(Rule::DecorativeImages) && !adapt.on(Rule::CollapseNav));
        assert!(adapt.on(Rule::IconButtons));
        let adapt = adapt
            .apply("+text-over-pictures decorative-images")
            .unwrap();
        assert!(adapt.on(Rule::TextOverPictures) && adapt.on(Rule::DecorativeImages));
        assert!(Adapt::default().apply("none").unwrap().is_none());
        assert!(Adapt::default().apply("-all").unwrap().is_none());
        assert_eq!(Adapt::none().apply("default").unwrap(), Adapt::default());
        assert!(Adapt::default()
            .apply("-nope")
            .unwrap_err()
            .contains("decorative-images"));
    }

    #[test]
    fn saturated_colours_stay_readable_and_greys_follow_the_terminal() {
        // A red "LIVE" and a coral author's name stay; light enough already.
        assert_eq!(
            page_color("rgb(255, 127, 80)", Background::Dark).as_deref(),
            Some("rgb(255,127,80)")
        );
        // Greys and near-black or near-white text take the terminal's own.
        for grey in [
            "rgb(17, 17, 17)",
            "rgb(102, 102, 102)",
            "rgb(240, 240, 245)",
        ] {
            assert_eq!(page_color(grey, Background::Dark), None, "{grey}");
        }
        assert_eq!(page_color("rgba(255, 0, 0, 0)", Background::Dark), None);
        // A dark blue is lifted on a dark terminal, darkened stays on light.
        let lifted = page_color("rgb(20, 40, 160)", Background::Dark).unwrap();
        assert_ne!(lifted, "rgb(20,40,160)");
        assert_eq!(
            page_color("rgb(20, 40, 160)", Background::Light).as_deref(),
            Some("rgb(20,40,160)")
        );
        // A yellow is darkened on a light terminal.
        assert_ne!(
            page_color("rgb(255, 215, 0)", Background::Light).as_deref(),
            Some("rgb(255,215,0)")
        );
    }

    #[test]
    fn capped_pictures_keep_their_shape() {
        let rect = Rect::new(0.0, 0.0, 400.0, 28.0 * 18.0);
        assert_eq!(
            picture_cells(rect, 50, 18.0, Adapt::default(), 200),
            (MAX_PICTURE_ROWS, 25)
        );
        assert_eq!(picture_cells(rect, 50, 18.0, Adapt::none(), 200), (28, 50));
    }
}
