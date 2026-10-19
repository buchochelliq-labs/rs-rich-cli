//! Putting the framebuffer on screen. In cells, through rs-rich-art's
//! `ImageArt` (half blocks, quadrants or Braille) rendered on the widget's
//! canvas; in real pixels, through the app's graphics side channel
//! (`App::graphics`, a `PlacementSource`) as a Kitty or iTerm2 image
//! placed over blank cells. The widget is also where key releases arrive:
//! intuiTUIve gives `KeyUp` to widgets only, never to bindings.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use intuituive::interact::Key;
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::widget::{Canvas, DrawCx, EventCx, Used, WidgetEvent};
use intuituive::RectHandle;
use rich_art::image::{DynamicImage, RgbImage, RgbaImage};
use rich_art::{ImageArt, ImageMode};
use rich_ext::frame::{Frame as CellFrame, Graphic, Placement, PlacementSource};

use super::render::Frame;

/// How the view reaches the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Half blocks: two pixels a cell, in 24-bit colour.
    Blocks,
    /// Quadrant blocks: four pixels a cell, two colours each.
    Quadrants,
    /// Braille: eight dots a cell, one colour each.
    Braille,
    /// Kitty's graphics protocol: real pixels.
    Kitty,
    /// iTerm2's (and WezTerm's) inline images: real pixels.
    Iterm,
}

impl Mode {
    pub const NAMES: &'static str = "auto, blocks, quadrants, braille, kitty, iterm";

    /// `auto` or a mode's name.
    pub fn parse(name: &str) -> Result<Option<Mode>, String> {
        Ok(Some(match name {
            "auto" => return Ok(None),
            "blocks" | "half" => Mode::Blocks,
            "quadrants" => Mode::Quadrants,
            "braille" => Mode::Braille,
            "kitty" => Mode::Kitty,
            "iterm" | "iterm2" => Mode::Iterm,
            other => return Err(format!("unknown mode {other} ({})", Mode::NAMES)),
        }))
    }

    /// The best the terminal has, by rs-rich-ext's capability detection
    /// (environment only, no query). Sixel is detected but cannot be drawn
    /// here (see the report), so it falls back to half blocks.
    pub fn detect() -> Mode {
        use rich_ext::capabilities::{Graphics, SystemEnvironment};
        let env = rich_ext::graphics::GraphicsEnvironment::detect(&SystemEnvironment);
        match env.graphics.value {
            Graphics::Kitty => Mode::Kitty,
            Graphics::Iterm => Mode::Iterm,
            Graphics::Sixel | Graphics::None => Mode::Blocks,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Blocks => "half blocks",
            Mode::Quadrants => "quadrants",
            Mode::Braille => "braille",
            Mode::Kitty => "kitty",
            Mode::Iterm => "iterm2",
        }
    }

    pub fn is_pixels(self) -> bool {
        matches!(self, Mode::Kitty | Mode::Iterm)
    }

    /// The framebuffer's pixels a cell, across and down, and a pixel's
    /// height over its width on screen (a cell is about twice as tall as
    /// wide). Covered by an overlay, a pixel mode draws half blocks.
    pub fn pixels(self, covered: bool) -> (usize, usize, f64) {
        match self {
            Mode::Kitty | Mode::Iterm if !covered => (4, 8, 1.0),
            Mode::Blocks | Mode::Kitty | Mode::Iterm => (1, 2, 1.0),
            Mode::Quadrants => (2, 2, 2.0),
            Mode::Braille => (2, 4, 1.0),
        }
    }

    fn cells(self) -> ImageMode {
        match self {
            Mode::Quadrants => ImageMode::Quadrants,
            Mode::Braille => ImageMode::Braille,
            _ => ImageMode::Blocks,
        }
    }
}

/// The picture the graphics source places, and where.
#[derive(Default)]
pub struct Placed {
    rect: Option<Rect>,
    picture: Option<Arc<Picture>>,
    mode: Option<Mode>,
}

/// One frame, encoded for the terminal.
#[derive(Debug)]
pub struct Picture {
    key: u64,
    escape: String,
}

impl Graphic for Picture {
    fn key(&self) -> u64 {
        self.key
    }

    fn draw(&self, _frame: usize) -> String {
        self.escape.clone()
    }
}

/// The graphics source: the view's latest frame over its rectangle.
pub struct Pixels(pub Arc<Mutex<Placed>>);

/// Kitty's image id for the view.
const KITTY_ID: u32 = 7_701;

impl PlacementSource for Pixels {
    fn placements(&self, _frame: &CellFrame) -> Vec<Placement> {
        let placed = self.0.lock().expect("the view's picture");
        match (placed.rect, &placed.picture) {
            (Some(rect), Some(picture)) => vec![Placement {
                row: rect.y as usize,
                column: rect.x as usize,
                cols: rect.width as usize,
                rows: rect.height as usize,
                graphic: picture.clone(),
                frame: 0,
            }],
            _ => Vec::new(),
        }
    }

    fn release(&self, _retained: &[Placement]) -> String {
        match self.0.lock().expect("the view's picture").mode {
            Some(Mode::Kitty) => format!("\x1b_Ga=d,d=I,i={KITTY_ID},q=2\x1b\\"),
            _ => String::new(),
        }
    }
}

/// `frame` as the escape that draws it over `cols` × `rows` cells.
fn encode(mode: Mode, frame: &Frame, cols: u16, rows: u16) -> Option<String> {
    let mut rgba = RgbaImage::new(frame.width as u32, frame.height as u32);
    for (i, px) in rgba.pixels_mut().enumerate() {
        px.0 = [frame.rgb[i * 3], frame.rgb[i * 3 + 1], frame.rgb[i * 3 + 2], 255];
    }
    let png = rich_art::graphics::encode_png(&rgba)?;
    let data = rich_art::graphics::base64(&png);
    Some(match mode {
        Mode::Kitty => {
            // Replace the image and its one placement, at the cursor, the
            // cursor left where it is (C=1), under every cell that has a
            // background colour (z below -2^30), so overlays cover it.
            let chunks: Vec<&str> = (0..data.len())
                .step_by(4096)
                .map(|s| &data[s..(s + 4096).min(data.len())])
                .collect();
            let mut out = String::with_capacity(data.len() + chunks.len() * 16 + 96);
            for (i, chunk) in chunks.iter().enumerate() {
                let more = u8::from(i + 1 < chunks.len());
                if i == 0 {
                    out.push_str(&format!(
                        "\x1b_Ga=T,f=100,t=d,i={KITTY_ID},p=1,c={cols},r={rows},C=1,z=-1073741825,q=2,m={more};{chunk}\x1b\\"
                    ));
                } else {
                    out.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
                }
            }
            out
        }
        _ => format!(
            "\x1b7\x1b]1337;File=inline=1;size={};width={cols};height={rows};preserveAspectRatio=0:{data}\x07\x1b8",
            png.len()
        ),
    })
}

/// The frame in black and white, brightened and dithered (a 4x4 Bayer
/// matrix): rs-rich-art's Braille sets a dot where a pixel is brighter
/// than half, with no colour, which leaves a dim scene blank.
fn dither(frame: &Frame) -> Vec<u8> {
    const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let mut out = vec![0u8; frame.rgb.len()];
    for (i, px) in frame.rgb.chunks_exact(3).enumerate() {
        let (x, y) = (i % frame.width, i / frame.width);
        let luma = (0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64) / 255.0;
        let level = luma.powf(0.6) * 16.0;
        if level > BAYER[y % 4][x % 4] as f64 + 0.5 {
            out[i * 3..i * 3 + 3].fill(255);
        }
    }
    out
}

/// The view: the framebuffer in cells or pixels, and the keys let go.
pub struct View {
    pub frame: Signal<Rc<Frame>>,
    pub mode: Mode,
    /// An overlay (a menu, the map, a message box) is over the view: a
    /// pixel mode draws cells meanwhile, which the overlay can cover.
    pub covered: Memo<bool>,
    /// The view's size in cells, for the next frame.
    pub size: Rc<Cell<(u16, u16)>>,
    pub rect: RectHandle,
    pub placed: Option<Arc<Mutex<Placed>>>,
    pub released: Box<dyn FnMut(&Key)>,
}

impl Widget for View {
    fn name(&self) -> &'static str {
        "view"
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (w, h) = (canvas.width(), canvas.height());
        self.size.set((w, h));
        let frame = self.frame.get();
        let covered = self.covered.get();
        if self.mode.is_pixels() && !covered {
            // Blank cells; the picture goes over them after the frame.
            if let (Some(placed), Some(rect)) = (&self.placed, self.rect.get_untracked()) {
                let escape = encode(self.mode, &frame, rect.width, rect.height);
                let mut placed = placed.lock().expect("the view's picture");
                placed.mode = Some(self.mode);
                placed.rect = Some(rect);
                placed.picture = escape.map(|escape| {
                    Arc::new(Picture {
                        key: frame.number,
                        escape,
                    })
                });
            }
            return;
        }
        if let Some(placed) = &self.placed {
            placed.lock().expect("the view's picture").picture = None;
        }
        let rgb = if self.mode == Mode::Braille {
            dither(&frame)
        } else {
            frame.rgb.clone()
        };
        let Some(image) = RgbImage::from_raw(frame.width as u32, frame.height as u32, rgb) else {
            return;
        };
        let art = ImageArt::new(DynamicImage::ImageRgb8(image))
            .mode(self.mode.cells())
            .width(usize::from(w))
            .height(usize::from(h));
        canvas.render(cx.console(), 0, 0, w, h, &art);
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::KeyUp(key) => {
                (self.released)(key);
                Used::Yes
            }
            WidgetEvent::Resize { width, height } => {
                self.size.set((*width, *height));
                Used::No
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn access_text(&self) -> Option<String> {
        Some("3D view".into())
    }
}
