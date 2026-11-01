//! The start page: what Noctiluca shows when it starts with no address
//! (and no `home`), while Chrome starts behind it.
//!
//! The NOCTILUCA banner, its glow travelling through the letters, a wave
//! under it and specks of light rising (rs-rich-art's
//! [`AnimatedBanner`], played by intuiTUIve's
//! [`banner`](rich_intuituive::banner::banner) once, about two seconds,
//! then still); the lanternfish, Noctiluca's mascot, swimming in from the
//! right to settle beside the title, its lure pulsing with the glow; the
//! tagline and version; a line of key hints read from the keymap; and the
//! bookmarks, a list to pick one from with the keyboard. Any key ends the
//! animation at once (and then does what it does); with reduced motion,
//! no animation or no colour it starts still.
//!
//! The fish is a button: Enter on it, or a click, opens
//! [`about:noctiluca`](crate::about::ADDRESS) by the same path as the
//! `about` action. It is drawn beside the title when the terminal is wide
//! enough, above it when it is tall enough instead, and left out
//! otherwise: it never squeezes the title or the bookmarks ([`fish_place`]).
//!
//! Opening a bookmark or an address leaves the page; the app hides it on
//! the first page it opens ([`leaves`]). The about page opens in a box
//! over it, and Esc comes back to it.
//!
//! What is generic lives in the libraries (the frames, the effects, the
//! wave and specks in rs-rich-art; the play clock and the banner node in
//! intuiTUIve; the half-block sprite and its stage already there); the
//! title, the fish, its action and this layout are Noctiluca's.

use std::rc::Rc;
use std::time::Duration;

use rich_art::FigletFont;
use rich_art::animate::{AnimatedBanner, mix};
use rich_intuituive::banner::{Play, banner};
use rich_intuituive::interact::keymap::Keymap;
use rich_intuituive::pixels::{PixelSprite, Rgba};
use rich_intuituive::prelude::*;
use rich_intuituive::sprite::{Placed, Sprite, StageOptions, stage_with};

use crate::actions::keys_label;
use crate::bookmarks::Bookmarks;

/// What the start page says Noctiluca is.
pub const TAGLINE: &str = "a web browser for the terminal";

/// The title in the banner.
pub const TITLE: &str = "NOCTILUCA";

/// The lanternfish's accessible name: what activating it does.
pub const FISH_NAME: &str = "About Noctiluca";

/// The lanternfish, 32x32 pixels (`.` transparent), a palette in its
/// header: a copy of `docs/assets/noctiluca/lanternfish-pixel.txt`.
pub const LANTERNFISH: &str = include_str!("assets/lanternfish-pixel.txt");

/// The fish's size in cells: two pixels a cell, one above the other.
pub const FISH_CELLS: (u16, u16) = (32, 16);

/// Columns between the title and the fish beside it.
const FISH_GAP: u16 = 2;

/// Rows under the title block: a blank, the hints, a blank, the
/// bookmarks' heading, and at least three bookmarks.
const BELOW_TITLE: u16 = 4 + MIN_LIST;

/// The fewest bookmark rows the page keeps room for.
const MIN_LIST: u16 = 3;

/// The tagline and version under the banner.
const TAG_ROWS: u16 = 1;

/// Whether the app sending `command` leaves the start page: opening an
/// address or moving in the history does. The about page is a box over
/// it, so opening that does not: Esc comes back to it.
pub fn leaves(command: &crate::tab::Command) -> bool {
    use crate::tab::Command;
    match command {
        Command::Open(url) => url != crate::about::ADDRESS,
        Command::History(_) => true,
        _ => false,
    }
}

/// What the start page needs from the app.
#[derive(Clone)]
pub struct StartPage {
    /// The keymap, for the hints line.
    pub keymap: Keymap,
    pub bookmarks: Signal<Bookmarks>,
    /// Open an address.
    pub open: Rc<dyn Fn(String, &mut Ctx)>,
    /// Open the about page (the `about` action).
    pub about: Rc<dyn Fn(&mut Ctx)>,
    /// Rows the browser's own bars take, under the terminal's.
    pub chrome_rows: u16,
}

/// Where the fish goes, for a page of `width` x `height` cells and a
/// title block (banner, tagline) of `title` cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FishPlace {
    /// To the right of the title.
    Beside,
    /// Above the title.
    Above,
    /// Not drawn: no room.
    None,
}

/// Where the fish fits without squeezing the title or the bookmarks:
/// beside the title when the width has room for both and the height for
/// the fish and what is under the title; else above it when the height
/// has room for both; else nowhere.
pub fn fish_place(width: u16, height: u16, title: (u16, u16)) -> FishPlace {
    let (fish_w, fish_h) = FISH_CELLS;
    let (title_w, title_h) = title;
    if width >= title_w + FISH_GAP + fish_w && height >= title_h.max(fish_h) + BELOW_TITLE {
        FishPlace::Beside
    } else if width >= fish_w && height >= fish_h + title_h + BELOW_TITLE {
        FishPlace::Above
    } else {
        FishPlace::None
    }
}

/// The lanternfish's palette from its header (`# B #2a7fd8`).
fn palette() -> Vec<(char, [u8; 3])> {
    LANTERNFISH
        .lines()
        .filter_map(|line| line.strip_prefix("# "))
        .filter_map(|rest| {
            let (key, hex) = rest.split_once(' ')?;
            let key = key.chars().next().filter(|_| key.len() == 1)?;
            let hex = hex.trim().strip_prefix('#')?;
            if hex.len() != 6 {
                return None;
            }
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some((key, [byte(0)?, byte(2)?, byte(4)?]))
        })
        .collect()
}

/// The fish's pixel rows.
fn rows() -> Vec<&'static str> {
    LANTERNFISH
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect()
}

/// The lanternfish with its lure (the `G` and `W` bulb) at brightness
/// `lure`, from 0 (dark) to 1 (its own colours); the rest of the fish as
/// drawn.
pub fn lanternfish(lure: f64) -> PixelSprite {
    let colours = palette();
    let colour = |c: char| colours.iter().find(|(k, _)| *k == c).map(|(_, rgb)| *rgb);
    let dark = colour('K').unwrap_or([0, 0, 0]);
    let rgba = |[r, g, b]: [u8; 3]| -> Rgba { [r, g, b, 255] };
    let palette: Vec<(char, Rgba)> = colours.iter().map(|(k, rgb)| (*k, rgba(*rgb))).collect();
    let rows = rows();
    // Only the bulb in the top-left corner pulses: its frame of `G` and
    // the `W` light inside. The `G` marks along the belly stay as drawn.
    let drawn = PixelSprite::from_art(&rows, &palette);
    let width = drawn.width() as usize;
    let mut pixels = drawn.frame(0).to_vec();
    for (y, row) in rows.iter().enumerate().take(10) {
        for (x, c) in row.chars().enumerate().take(10) {
            if let Some(rgb) = colour(c).filter(|_| c == 'G' || c == 'W') {
                pixels[y * width + x] = rgba(mix(dark, rgb, 0.2 + 0.8 * lure));
            }
        }
    }
    PixelSprite::new(drawn.width(), drawn.height(), pixels)
}

/// The start page.
pub fn page(start: StartPage) -> Node {
    let font = FigletFont::standard();
    let art = AnimatedBanner::new(TITLE, &font)
        .wave(true)
        .specks(true)
        .seed(7);
    let (art_w, art_h) = art.size();
    let title = (art_w as u16, art_h as u16 + TAG_ROWS);
    let play = Play::new(art.duration_of());
    let port = viewport();
    let chrome = start.chrome_rows;
    let fish_play = play.clone();
    let fish_art = art.clone();
    let about = start.about.clone();
    let place = move || {
        let (w, h) = port.get();
        fish_place(w, h.saturating_sub(chrome), title)
    };
    let banner_play = play.clone();
    let title_rows = title.1;
    let top = keyed(place, move |at| {
        let title = title_block(art.clone(), banner_play.clone());
        let fish = || fish(fish_art.clone(), fish_play.clone(), about.clone(), at);
        match at {
            FishPlace::Beside => row([title, fish().flex(1)])
                .gap(FISH_GAP)
                .fixed(FISH_CELLS.1.max(title_rows)),
            FishPlace::Above => column([fish().fixed(FISH_CELLS.1), title]).auto(),
            FishPlace::None => title,
        }
    })
    .auto();
    let page = column([
        top,
        hints(&start.keymap).fixed(1),
        bookmarks(start.clone()).flex(1),
    ])
    .gap(1)
    .padding(1, 2);
    play.skip_on_key(page)
}

/// The banner (or, too narrow for it, the title in plain letters) and
/// the tagline.
fn title_block(art: AnimatedBanner, play: Play) -> Node {
    let (art_w, _) = art.size();
    let port = viewport();
    let tagline = label(format!(
        "[b]{TAGLINE}[/] [dim]· Noctiluca {}[/]",
        env!("CARGO_PKG_VERSION")
    ))
    .fixed(1);
    let big = keyed(
        move || usize::from(port.get().0) >= art_w + 4,
        move |fits| {
            if fits {
                banner(art.clone(), play.clone()).label(TITLE)
            } else {
                label(format!("[b]{TITLE}[/]")).fixed(1)
            }
        },
    )
    .auto();
    column([big, tagline]).auto()
}

/// The lanternfish: swimming in from the right while `play` runs, then
/// still, its lure pulsing with the banner's glow. Enter or a click runs
/// `about`.
fn fish(art: AnimatedBanner, play: Play, about: Rc<dyn Fn(&mut Ctx)>, at: FishPlace) -> Node {
    let port = viewport();
    let fish_w = FISH_CELLS.0;
    let sprites = move || {
        let t = play.elapsed();
        let p = art.progress(t);
        let lure = art.pulse(t);
        // Beside the title the stage runs to the right edge; above it, the
        // fish settles in the middle.
        let width = f64::from(port.get().0);
        let rest = match at {
            FishPlace::Above => ((width - f64::from(fish_w)) / 2.0).max(0.0) - 2.0,
            _ => 0.0,
        };
        let eased = 1.0 - (1.0 - p).powi(3);
        let x = width + (rest - width) * eased;
        vec![Placed::new(0u8, Sprite::from_pixels(&lanternfish(lure))).at(x.max(rest), 0.0)]
    };
    let click = about.clone();
    stage_with(
        sprites,
        StageOptions::new().on_sprite_click(move |_, cx| click(cx)),
    )
    .label(FISH_NAME)
    .role(rich_intuituive::a11y::Role::Button)
    .focusable()
    .on_key("enter", move |cx| about(cx))
}

/// The key hints: the keys this keymap binds to opening an address, the
/// bookmarks, the actions list, the about page and quitting.
fn hints(keymap: &Keymap) -> Node {
    let parts: Vec<String> = [
        ("open", "open an address"),
        ("bookmarks", "bookmarks"),
        ("actions", "all actions"),
        ("about", "about"),
        ("quit", "quit"),
    ]
    .iter()
    .filter_map(|(id, what)| {
        let keys = keys_label(keymap, id);
        (!keys.is_empty()).then(|| format!("[b]{}[/] {what}", crate::clean::markup(&keys)))
    })
    .collect();
    label(format!(
        "{}  [dim]· Tab: the lanternfish[/]",
        parts.join(" · ")
    ))
}

/// The bookmarks heading and list: Enter opens the one selected.
fn bookmarks(start: StartPage) -> Node {
    let marks = start.bookmarks;
    let selected = signal(0usize);
    let items = move || {
        marks.with(|m| {
            m.items
                .iter()
                .map(|bm| {
                    if bm.title.is_empty() {
                        bm.url.clone()
                    } else {
                        format!("{}  {}", bm.title, bm.url)
                    }
                })
                .collect::<Vec<_>>()
        })
    };
    let open = start.open.clone();
    let list = list(items, selected)
        .symbol("› ")
        .label("Bookmarks")
        .autofocus()
        .on_key("enter", move |cx| {
            let url = marks
                .with_untracked(|m| m.items.get(selected.get_untracked()).map(|b| b.url.clone()));
            if let Some(url) = url {
                open(url, cx);
            }
        })
        .flex(1);
    let add = keys_label(&start.keymap, "bookmark");
    let empty = label(format!(
        "[dim]No bookmarks yet: on a page, {} keeps it.[/]",
        crate::clean::markup(&add)
    ))
    .fixed(1);
    let has = move || marks.with(|m| !m.items.is_empty());
    column([
        label("[b]Bookmarks[/]").fixed(1),
        list.shown_when(has),
        empty.hidden_when(has),
    ])
}

/// How long the start page's animation runs.
pub fn duration() -> Duration {
    rich_art::animate::DEFAULT_DURATION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fish_is_32_by_16_cells_with_its_palette() {
        let fish = lanternfish(1.0);
        assert_eq!((fish.width(), fish.height()), (32, 32));
        assert_eq!(palette().len(), 6);
        // The lure's light, lit, is its own white; dark, it is dim.
        let w = |s: &PixelSprite| s.frame(0)[5 * 32 + 4];
        assert_eq!(w(&fish), [0xea, 0xff, 0xfd, 255]);
        assert!(w(&lanternfish(0.0))[0] < 0x60);
        // The marks on the belly do not pulse.
        let belly = |s: &PixelSprite| s.frame(0)[26 * 32 + 8];
        assert_eq!(belly(&lanternfish(0.0)), belly(&fish));
    }

    #[test]
    fn opening_a_page_leaves_it_and_the_about_box_does_not() {
        use crate::engine::HistoryStep;
        use crate::tab::Command;
        assert!(leaves(&Command::Open("https://example.org/".into())));
        assert!(leaves(&Command::History(HistoryStep::Back)));
        assert!(!leaves(&Command::Open(crate::about::ADDRESS.into())));
        assert!(!leaves(&Command::Resize(80, 24)));
    }

    #[test]
    fn the_fish_never_squeezes_the_title_or_the_list() {
        let title = (64, 9);
        assert_eq!(fish_place(120, 40, title), FishPlace::Beside);
        // Too narrow beside it, tall enough above.
        assert_eq!(fish_place(80, 40, title), FishPlace::Above);
        // Neither.
        assert_eq!(fish_place(80, 24, title), FishPlace::None);
        assert_eq!(fish_place(30, 60, title), FishPlace::None);
        // Wide, but too short for the fish and the bookmarks under the title.
        assert_eq!(fish_place(120, 20, title), FishPlace::None);
    }
}
