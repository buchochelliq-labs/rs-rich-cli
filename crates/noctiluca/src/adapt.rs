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
    /// An image with an empty `alt`, `aria-hidden`, or smaller than
    /// [`DECORATIVE_PX`] a side is not a media region.
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
        what: "drop decorative images: empty alt, aria-hidden, spacers",
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
/// The most rows a picture takes when pictures are capped.
pub const MAX_PICTURE_ROWS: usize = 14;
/// Below this many columns a terminal is narrow: navigation collapses.
pub const NARROW_COLUMNS: u16 = 100;
/// A grid's column is at least this many cells wide when grids snap.
pub const MIN_GRID_CELLS: usize = 18;
/// A picture of text is a banner at least this many times wider than
/// tall.
pub const BANNER_RATIO: f64 = 4.0;

/// Whether `media` is decorative: an image with an empty `alt`, hidden
/// from assistive technology, or a spacer. Video, audio, canvases and
/// frames never are.
pub fn decorative(media: &Media) -> bool {
    media.kind == crate::model::MediaKind::Image
        && (media.empty_alt
            || media.aria_hidden
            || media.rect.width < DECORATIVE_PX
            || media.rect.height < DECORATIVE_PX)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_meaning() {
        let adapt = Adapt::default();
        assert!(adapt.on(Rule::DecorativeImages) && adapt.on(Rule::LiveTail));
        assert!(!adapt.on(Rule::TextOverPictures));
        assert!(Adapt::none().is_none());
        assert_eq!(RULES.len(), 10);
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
        let adapt = adapt.apply("+text-over-pictures decorative-images").unwrap();
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
    fn capped_pictures_keep_their_shape() {
        let rect = Rect::new(0.0, 0.0, 400.0, 28.0 * 18.0);
        assert_eq!(
            picture_cells(rect, 50, 18.0, Adapt::default(), 200),
            (MAX_PICTURE_ROWS, 25)
        );
        assert_eq!(picture_cells(rect, 50, 18.0, Adapt::none(), 200), (28, 50));
    }
}
