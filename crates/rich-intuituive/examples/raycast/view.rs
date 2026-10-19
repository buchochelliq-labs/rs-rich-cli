//! Putting the framebuffer on screen. In cells, through intuiTUIve's
//! `pixels` node (half blocks, quadrants or Braille); in real pixels, through the app's graphics side channel
//! (`App::graphics`, a `PlacementSource`) as a Kitty or iTerm2 image
//! placed over blank cells, which keep the terminal's own background so
//! the image (under cells with a background) shows.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use intuituive::pixels::{pixels_fit, PixelMode};
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::widget::{Canvas, DrawCx, EventCx, Used, WidgetEvent};
use rich_art::image::RgbaImage;
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
        px.0 = [
            frame.rgb[i * 3],
            frame.rgb[i * 3 + 1],
            frame.rgb[i * 3 + 2],
            255,
        ];
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

/// The view in cells: the framebuffer copied into a `pixels` node, in half
/// blocks, quadrants or Braille. `size` keeps the view's size in cells for
/// the next frame.
pub fn cells(frame: Signal<Rc<Frame>>, mode: Mode, size: Rc<Cell<(u16, u16)>>) -> Node {
    let pixel_mode = match mode {
        Mode::Quadrants => PixelMode::Quadrants,
        Mode::Braille => PixelMode::Braille,
        _ => PixelMode::HalfBlocks,
    };
    let (pw, ph) = pixel_mode.cell();
    pixels_fit(move |buf| {
        size.set((buf.width() / pw, buf.height() / ph));
        let frame = frame.get();
        let width = usize::from(buf.width());
        for (y, row) in buf.pixels_mut().chunks_mut(width.max(1)).enumerate() {
            for (x, px) in row.iter_mut().enumerate() {
                *px = match (
                    x < frame.width && y < frame.height,
                    (y * frame.width + x) * 3,
                ) {
                    (true, i) => [frame.rgb[i], frame.rgb[i + 1], frame.rgb[i + 2], 255],
                    (false, _) => [0, 0, 0, 255],
                };
            }
        }
    })
    .pixel_mode(pixel_mode)
}

/// The view in real pixels: blank cells for the picture; gone (covered),
/// no picture.
pub fn pixels(
    frame: Signal<Rc<Frame>>,
    mode: Mode,
    size: Rc<Cell<(u16, u16)>>,
    placed: Arc<Mutex<Placed>>,
) -> Node {
    let gone = placed.clone();
    widget(View {
        frame,
        mode,
        size,
        placed,
    })
    .on_unmount(move || gone.lock().expect("the view's picture").picture = None)
}

/// The view in real pixels: blank cells, with the terminal's own
/// background, that the picture goes over after the frame.
pub struct View {
    pub frame: Signal<Rc<Frame>>,
    pub mode: Mode,
    /// The view's size in cells, for the next frame.
    pub size: Rc<Cell<(u16, u16)>>,
    pub placed: Arc<Mutex<Placed>>,
}

impl Widget for View {
    fn name(&self) -> &'static str {
        "view"
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let rect = cx.rect();
        self.size.set((canvas.width(), canvas.height()));
        let frame = self.frame.get();
        let escape = encode(self.mode, &frame, rect.width, rect.height);
        let mut placed = self.placed.lock().expect("the view's picture");
        placed.mode = Some(self.mode);
        placed.rect = Some(rect);
        placed.picture = escape.map(|escape| {
            Arc::new(Picture {
                key: frame.number,
                escape,
            })
        });
    }

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if let WidgetEvent::Resize { width, height } = event {
            self.size.set((*width, *height));
        }
        Used::No
    }

    fn access_text(&self) -> Option<String> {
        Some("3D view".into())
    }
}
