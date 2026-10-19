//! Terminal graphics: the images a program draws with the Kitty graphics
//! protocol, Sixel or iTerm2's inline images, kept on the cell grid.
//!
//! [`Terminal`](crate::terminal::Terminal) hands the escape sequences that
//! carry images to [`Graphics`], and a [`Snapshot`](crate::screen::Snapshot)
//! of the screen lists what shows as [`Tile`]s: the cells an image covers
//! and the part of it they show. PNG, GIF and video frames draw them there;
//! text grids and SVGs show the cells under them (blank, as the program
//! left them).
//!
//! - **Kitty**: images transmitted directly (`t=d`) as PNG (`f=100`) or raw
//!   RGB or RGBA (`f=24`, `f=32`), zlib-compressed or not (`o=z`), in one
//!   escape or in chunks (`m=1`); shown by a placement at the cursor (`a=T`,
//!   `a=p`: `c`, `r`, the source rectangle `x`, `y`, `w`, `h`, `z`, `C=1`)
//!   or through Unicode placeholder cells (`U=1`: U+10EEEE with row and
//!   column diacritics, the image id in the foreground colour); deleted
//!   with `a=d` (`d=a`, `i`, `c`, `p`, `z`, upper case freeing the data).
//!   Animation frames are not played: the image shows its first frame.
//! - **Sixel** (`DCS … q … ST`): colour registers in RGB or HLS, repeats,
//!   raster attributes; drawn at the cursor, [`CELL_PIXELS`] a cell.
//! - **iTerm2** (`OSC 1337 ; File = … : data`): PNG and GIF images with
//!   `inline=1`, sized in cells, pixels, percent or `auto`, with or without
//!   `preserveAspectRatio`.
//!
//! A Kitty placement stays until it is deleted; Sixel and iTerm2 images are
//! part of the cells they cover, so each shows only through the cells that
//! are as they were when it was drawn: text written over one hides it
//! there, as on the terminals that have them. Clearing the screen
//! (`ED 2`), switching screens and a reset remove every placement. Images
//! do not scroll with the text.
//!
//! Nothing is answered: a program that asks whether the terminal has a
//! protocol hears nothing, as before, and draws in text unless told to use
//! one.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::sync::{Arc, OnceLock};

/// A cell's size in pixels as the emulator counts it: what a Sixel image,
/// or an image placed without a size in cells, is measured in.
pub const CELL_PIXELS: (u32, u32) = (8, 16);
/// The most pixels one image may have; a larger one is not kept.
pub const MAX_IMAGE_PIXELS: u64 = 4096 * 4096;
/// Bytes of image data kept for the Kitty images a program transmitted;
/// past it the oldest are forgotten, as a terminal's quota does.
const MAX_STORED: usize = 64 << 20;
/// The most images placed at the cursor kept at once: past it the oldest
/// go.
const MAX_OVERLAYS: usize = 256;
/// Kitty's placeholder character (Supplementary Private Use Area-B).
pub const PLACEHOLDER: char = '\u{10EEEE}';
/// Below this z index a Kitty image is drawn under cells that have a
/// background colour, not only under their text.
const UNDER_BACKGROUNDS: i64 = -1_073_741_824;

/// Kitty's row and column diacritics, in order: the *n*th encodes *n*. From
/// kitty's `gen/rowcolumn-diacritics.txt`, as rs-rich-art has them.
#[rustfmt::skip]
const DIACRITICS: [char; 297] = [
    '\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}', '\u{033F}',
    '\u{0346}', '\u{034A}', '\u{034B}', '\u{034C}', '\u{0350}', '\u{0351}', '\u{0352}', '\u{0357}',
    '\u{035B}', '\u{0363}', '\u{0364}', '\u{0365}', '\u{0366}', '\u{0367}', '\u{0368}', '\u{0369}',
    '\u{036A}', '\u{036B}', '\u{036C}', '\u{036D}', '\u{036E}', '\u{036F}', '\u{0483}', '\u{0484}',
    '\u{0485}', '\u{0486}', '\u{0487}', '\u{0592}', '\u{0593}', '\u{0594}', '\u{0595}', '\u{0597}',
    '\u{0598}', '\u{0599}', '\u{059C}', '\u{059D}', '\u{059E}', '\u{059F}', '\u{05A0}', '\u{05A1}',
    '\u{05A8}', '\u{05A9}', '\u{05AB}', '\u{05AC}', '\u{05AF}', '\u{05C4}', '\u{0610}', '\u{0611}',
    '\u{0612}', '\u{0613}', '\u{0614}', '\u{0615}', '\u{0616}', '\u{0617}', '\u{0657}', '\u{0658}',
    '\u{0659}', '\u{065A}', '\u{065B}', '\u{065D}', '\u{065E}', '\u{06D6}', '\u{06D7}', '\u{06D8}',
    '\u{06D9}', '\u{06DA}', '\u{06DB}', '\u{06DC}', '\u{06DF}', '\u{06E0}', '\u{06E1}', '\u{06E2}',
    '\u{06E4}', '\u{06E7}', '\u{06E8}', '\u{06EB}', '\u{06EC}', '\u{0730}', '\u{0732}', '\u{0733}',
    '\u{0735}', '\u{0736}', '\u{073A}', '\u{073D}', '\u{073F}', '\u{0740}', '\u{0741}', '\u{0743}',
    '\u{0745}', '\u{0747}', '\u{0749}', '\u{074A}', '\u{07EB}', '\u{07EC}', '\u{07ED}', '\u{07EE}',
    '\u{07EF}', '\u{07F0}', '\u{07F1}', '\u{07F3}', '\u{0816}', '\u{0817}', '\u{0818}', '\u{0819}',
    '\u{081B}', '\u{081C}', '\u{081D}', '\u{081E}', '\u{081F}', '\u{0820}', '\u{0821}', '\u{0822}',
    '\u{0823}', '\u{0825}', '\u{0826}', '\u{0827}', '\u{0829}', '\u{082A}', '\u{082B}', '\u{082C}',
    '\u{082D}', '\u{0951}', '\u{0953}', '\u{0954}', '\u{0F82}', '\u{0F83}', '\u{0F86}', '\u{0F87}',
    '\u{135D}', '\u{135E}', '\u{135F}', '\u{17DD}', '\u{193A}', '\u{1A17}', '\u{1A75}', '\u{1A76}',
    '\u{1A77}', '\u{1A78}', '\u{1A79}', '\u{1A7A}', '\u{1A7B}', '\u{1A7C}', '\u{1B6B}', '\u{1B6D}',
    '\u{1B6E}', '\u{1B6F}', '\u{1B70}', '\u{1B71}', '\u{1B72}', '\u{1B73}', '\u{1CD0}', '\u{1CD1}',
    '\u{1CD2}', '\u{1CDA}', '\u{1CDB}', '\u{1CE0}', '\u{1DC0}', '\u{1DC1}', '\u{1DC3}', '\u{1DC4}',
    '\u{1DC5}', '\u{1DC6}', '\u{1DC7}', '\u{1DC8}', '\u{1DC9}', '\u{1DCB}', '\u{1DCC}', '\u{1DD1}',
    '\u{1DD2}', '\u{1DD3}', '\u{1DD4}', '\u{1DD5}', '\u{1DD6}', '\u{1DD7}', '\u{1DD8}', '\u{1DD9}',
    '\u{1DDA}', '\u{1DDB}', '\u{1DDC}', '\u{1DDD}', '\u{1DDE}', '\u{1DDF}', '\u{1DE0}', '\u{1DE1}',
    '\u{1DE2}', '\u{1DE3}', '\u{1DE4}', '\u{1DE5}', '\u{1DE6}', '\u{1DFE}', '\u{20D0}', '\u{20D1}',
    '\u{20D4}', '\u{20D5}', '\u{20D6}', '\u{20D7}', '\u{20DB}', '\u{20DC}', '\u{20E1}', '\u{20E7}',
    '\u{20E9}', '\u{20F0}', '\u{2CEF}', '\u{2CF0}', '\u{2CF1}', '\u{2DE0}', '\u{2DE1}', '\u{2DE2}',
    '\u{2DE3}', '\u{2DE4}', '\u{2DE5}', '\u{2DE6}', '\u{2DE7}', '\u{2DE8}', '\u{2DE9}', '\u{2DEA}',
    '\u{2DEB}', '\u{2DEC}', '\u{2DED}', '\u{2DEE}', '\u{2DEF}', '\u{2DF0}', '\u{2DF1}', '\u{2DF2}',
    '\u{2DF3}', '\u{2DF4}', '\u{2DF5}', '\u{2DF6}', '\u{2DF7}', '\u{2DF8}', '\u{2DF9}', '\u{2DFA}',
    '\u{2DFB}', '\u{2DFC}', '\u{2DFD}', '\u{2DFE}', '\u{2DFF}', '\u{A66F}', '\u{A67C}', '\u{A67D}',
    '\u{A6F0}', '\u{A6F1}', '\u{A8E0}', '\u{A8E1}', '\u{A8E2}', '\u{A8E3}', '\u{A8E4}', '\u{A8E5}',
    '\u{A8E6}', '\u{A8E7}', '\u{A8E8}', '\u{A8E9}', '\u{A8EA}', '\u{A8EB}', '\u{A8EC}', '\u{A8ED}',
    '\u{A8EE}', '\u{A8EF}', '\u{A8F0}', '\u{A8F1}', '\u{AAB0}', '\u{AAB2}', '\u{AAB3}', '\u{AAB7}',
    '\u{AAB8}', '\u{AABE}', '\u{AABF}', '\u{AAC1}', '\u{FE20}', '\u{FE21}', '\u{FE22}', '\u{FE23}',
    '\u{FE24}', '\u{FE25}', '\u{FE26}', '\u{10A0F}', '\u{10A38}', '\u{1D185}', '\u{1D186}', '\u{1D187}',
    '\u{1D188}', '\u{1D189}', '\u{1D1AA}', '\u{1D1AB}', '\u{1D1AC}', '\u{1D1AD}', '\u{1D242}', '\u{1D243}',
    '\u{1D244}',
];

/// The number a placeholder diacritic encodes.
fn diacritic(c: char) -> Option<u32> {
    static INDEX: OnceLock<HashMap<char, u32>> = OnceLock::new();
    INDEX
        .get_or_init(|| (0..).zip(DIACRITICS).map(|(i, c)| (c, i)).collect())
        .get(&c)
        .copied()
}

/// How an image's pixels are kept: as they came when that is compressed,
/// else deflated, so a recording that shows a new image every frame keeps
/// a few kilobytes a frame rather than every pixel.
#[derive(Debug)]
enum Data {
    Png(Vec<u8>),
    Gif(Vec<u8>),
    /// RGBA, zlib-compressed.
    Deflated(Vec<u8>),
}

/// An image a program drew: its size in pixels and its pixels, kept
/// compressed until a frame is drawn. Two images are equal when their
/// sizes and data are.
#[derive(Debug)]
pub struct Image {
    width: u32,
    height: u32,
    data: Data,
    hash: u64,
}

/// FNV-1a, 64-bit.
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in *part {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

fn allowed(width: u32, height: u32) -> bool {
    width > 0 && height > 0 && u64::from(width) * u64::from(height) <= MAX_IMAGE_PIXELS
}

impl Image {
    fn new(width: u32, height: u32, data: Data) -> Image {
        let bytes = match &data {
            Data::Png(b) | Data::Gif(b) | Data::Deflated(b) => b.as_slice(),
        };
        let hash = fnv(&[&width.to_le_bytes(), &height.to_le_bytes(), bytes]);
        Image {
            width,
            height,
            data,
            hash,
        }
    }

    /// A PNG file's image. Only its header is read here; `None` when that
    /// is not a PNG's, or the image is larger than [`MAX_IMAGE_PIXELS`].
    pub fn png(bytes: Vec<u8>) -> Option<Image> {
        let reader = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .ok()?;
        let (width, height) = (reader.info().width, reader.info().height);
        drop(reader);
        allowed(width, height).then(|| Image::new(width, height, Data::Png(bytes)))
    }

    /// A GIF file's image (its first frame).
    pub fn gif(bytes: Vec<u8>) -> Option<Image> {
        let decoder = gif::DecodeOptions::new()
            .read_info(std::io::Cursor::new(&bytes))
            .ok()?;
        let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
        drop(decoder);
        allowed(width, height).then(|| Image::new(width, height, Data::Gif(bytes)))
    }

    /// An image of `width` x `height` RGBA pixels.
    pub fn rgba(width: u32, height: u32, pixels: &[u8]) -> Option<Image> {
        if !allowed(width, height) || pixels.len() != width as usize * height as usize * 4 {
            return None;
        }
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(pixels).ok()?;
        Some(Image::new(
            width,
            height,
            Data::Deflated(encoder.finish().ok()?),
        ))
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The bytes kept for it.
    fn kept(&self) -> usize {
        match &self.data {
            Data::Png(b) | Data::Gif(b) | Data::Deflated(b) => b.len(),
        }
    }

    /// Its pixels, RGBA, row by row; `None` when its data does not decode.
    pub fn pixels(&self) -> Option<Vec<u8>> {
        let size = self.width as usize * self.height as usize * 4;
        let pixels = match &self.data {
            Data::Deflated(bytes) => {
                let mut out = Vec::with_capacity(size);
                flate2::read::ZlibDecoder::new(bytes.as_slice())
                    .take(size as u64)
                    .read_to_end(&mut out)
                    .ok()?;
                out
            }
            Data::Png(bytes) => decode_png(bytes)?,
            Data::Gif(bytes) => decode_gif(bytes)?,
        };
        (pixels.len() == size).then_some(pixels)
    }
}

impl PartialEq for Image {
    fn eq(&self, other: &Image) -> bool {
        (self.width, self.height, self.hash) == (other.width, other.height, other.hash)
    }
}

impl Eq for Image {}

/// A PNG's pixels as RGBA.
fn decode_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());
    Some(match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|[r, g, b]| [*r, *g, *b, 255])
            .collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|[v, a]| [*v, *v, *v, *a])
            .collect(),
        png::ColorType::Indexed => return None,
    })
}

/// A GIF's first frame as RGBA, on its logical screen.
fn decode_gif(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).ok()?;
    let (width, height) = (usize::from(decoder.width()), usize::from(decoder.height()));
    let frame = decoder.read_next_frame().ok()??;
    let mut out = vec![0u8; width * height * 4];
    let (left, top) = (usize::from(frame.left), usize::from(frame.top));
    let frame_width = usize::from(frame.width);
    for (y, row) in frame.buffer.chunks(frame_width * 4).enumerate() {
        let y = top + y;
        if y >= height || left >= width {
            continue;
        }
        let w = frame_width.min(width - left);
        let at = (y * width + left) * 4;
        out[at..at + w * 4].copy_from_slice(&row[..w * 4]);
    }
    Some(out)
}

/// Part of an image on the cell grid: the cells it covers and the part of
/// the image they show, stretched over them.
#[derive(Clone, Debug)]
pub struct Tile {
    pub image: Arc<Image>,
    /// The first cell it covers, from 0 at the top left.
    pub column: u16,
    pub row: u16,
    /// How many cells across and down it covers.
    pub columns: u16,
    pub rows: u16,
    /// The part of the image shown: x, y, width and height, in the image's
    /// pixels. It may reach past the image's edges, which are transparent
    /// there.
    pub source: [f32; 4],
    /// Drawn under the cells' text (over their backgrounds), not over it.
    pub below_text: bool,
}

impl Tile {
    fn key(&self) -> (u16, u16, u16, u16, [u32; 4], bool) {
        (
            self.column,
            self.row,
            self.columns,
            self.rows,
            self.source.map(f32::to_bits),
            self.below_text,
        )
    }
}

impl PartialEq for Tile {
    fn eq(&self, other: &Tile) -> bool {
        self.key() == other.key() && self.image == other.image
    }
}

impl Eq for Tile {}

impl Hash for Tile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key().hash(state);
        self.image.hash.hash(state);
    }
}

/// Standard base64, padding optional; whitespace is passed over. `None` on
/// any other character.
pub fn base64_decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let (mut bits, mut count) = (0u32, 0);
    for &byte in text {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        bits = bits << 6 | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Some(out)
}

/// The keys of a Kitty graphics command: `a=T,f=100,i=7`.
#[derive(Clone, Debug, Default)]
struct Keys(Vec<(u8, String)>);

impl Keys {
    fn parse(text: &[u8]) -> Keys {
        let text = String::from_utf8_lossy(text);
        Keys(
            text.split(',')
                .filter_map(|pair| {
                    let (key, value) = pair.split_once('=')?;
                    let key = *key.as_bytes().first()?;
                    Some((key, value.to_string()))
                })
                .collect(),
        )
    }

    fn get(&self, key: u8) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    fn char(&self, key: u8) -> Option<char> {
        self.get(key).and_then(|v| v.chars().next())
    }

    fn number(&self, key: u8) -> Option<i64> {
        self.get(key).and_then(|v| v.parse().ok())
    }

    fn unsigned(&self, key: u8) -> u32 {
        self.number(key)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    }
}

/// What the emulator does after an image is placed: move the cursor down
/// and right this many cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CursorMove {
    pub down: u16,
    pub right: u16,
}

/// A cell as it was when an image was drawn over it.
#[derive(Clone, Debug, PartialEq)]
struct CellState {
    text: String,
    fg: vt100::Color,
    bg: vt100::Color,
    inverse: bool,
}

impl CellState {
    fn of(cell: &vt100::Cell) -> CellState {
        CellState {
            text: cell.contents().to_string(),
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            inverse: cell.inverse(),
        }
    }
}

/// An image placed at the cursor.
#[derive(Debug)]
struct Overlay {
    /// A Kitty placement's image id and placement id.
    kitty: Option<(u32, u32)>,
    image: Arc<Image>,
    column: u16,
    row: u16,
    columns: u16,
    rows: u16,
    source: [f32; 4],
    z: i64,
    /// Sixel and iTerm2: the cells under it as they were, row by row; it
    /// shows through those that still are.
    under: Option<Vec<Option<CellState>>>,
}

impl Overlay {
    fn covers(&self, column: u16, row: u16) -> bool {
        (self.column..self.column.saturating_add(self.columns)).contains(&column)
            && (self.row..self.row.saturating_add(self.rows)).contains(&row)
    }

    /// Whether the cell at (`column`, `row`), inside it, shows it.
    fn shows(&self, screen: &vt100::Screen, column: u16, row: u16) -> bool {
        let Some(cell) = screen.cell(row, column) else {
            return false;
        };
        if self.z < UNDER_BACKGROUNDS && (cell.bgcolor() != vt100::Color::Default || cell.inverse())
        {
            return false;
        }
        match &self.under {
            None => true,
            Some(under) => {
                let index = usize::from(row - self.row) * usize::from(self.columns)
                    + usize::from(column - self.column);
                under
                    .get(index)
                    .and_then(Option::as_ref)
                    .is_some_and(|state| *state == CellState::of(cell))
            }
        }
    }
}

/// The images a program has drawn, and where.
#[derive(Debug, Default)]
pub(crate) struct Graphics {
    /// Kitty images by id, and the order they were stored in.
    images: HashMap<u32, Arc<Image>>,
    stored: VecDeque<u32>,
    stored_bytes: usize,
    /// A chunked Kitty transmission still arriving: its first chunk's keys
    /// and the payload so far.
    pending: Option<(Keys, Vec<u8>)>,
    /// Kitty virtual placements (`U=1`): the cells each image's
    /// placeholders count in.
    virtuals: HashMap<u32, (u16, u16)>,
    overlays: Vec<Overlay>,
    /// Ids for Kitty images sent with only a number (`I`).
    numbers: HashMap<u32, u32>,
}

/// The cells `width` x `height` pixels take, at least one each way.
fn cells_for(width: f64, height: f64) -> (u16, u16) {
    let (cw, ch) = (f64::from(CELL_PIXELS.0), f64::from(CELL_PIXELS.1));
    let fit = |v: f64| v.ceil().clamp(1.0, f64::from(u16::MAX)) as u16;
    (fit(width / cw), fit(height / ch))
}

impl Graphics {
    /// Remove every placement: the screen was cleared, switched or reset.
    pub fn clear(&mut self) {
        self.overlays.clear();
        self.pending = None;
    }

    /// Forget the images too: a reset.
    pub fn reset(&mut self) {
        *self = Graphics::default();
    }

    /// Handle an APC string (after `ESC _`): a Kitty graphics command when
    /// it starts with `G`. Returns whether it was one, and where the
    /// cursor moves.
    pub fn apc(&mut self, body: &[u8], screen: &vt100::Screen) -> Option<Option<CursorMove>> {
        let rest = body.strip_prefix(b"G")?;
        Some(self.kitty(rest, screen))
    }

    /// Handle a DCS string (after `ESC P`): Sixel when its parameters end
    /// in `q`.
    pub fn dcs(&mut self, body: &[u8], screen: &vt100::Screen) -> Option<Option<CursorMove>> {
        let start = body
            .iter()
            .position(|b| !(b.is_ascii_digit() || *b == b';'))?;
        if body[start] != b'q' {
            return None;
        }
        let Some(image) = decode_sixel(&body[start + 1..]) else {
            return Some(None);
        };
        let (columns, rows) = cells_for(f64::from(image.width), f64::from(image.height));
        let source = [
            0.0,
            0.0,
            f32::from(columns) * CELL_PIXELS.0 as f32,
            f32::from(rows) * CELL_PIXELS.1 as f32,
        ];
        let (row, column) = screen.cursor_position();
        self.overlay(
            screen,
            None,
            Arc::new(image),
            (column, row),
            (columns, rows),
            source,
            0,
        );
        Some(Some(CursorMove {
            down: rows,
            right: 0,
        }))
    }

    /// Handle an OSC string (after `ESC ]`): an iTerm2 inline image when
    /// it starts `1337;File=`.
    pub fn osc(&mut self, body: &[u8], screen: &vt100::Screen) -> Option<Option<CursorMove>> {
        let rest = body.strip_prefix(b"1337;File=")?;
        Some(self.iterm(rest, screen))
    }

    fn iterm(&mut self, rest: &[u8], screen: &vt100::Screen) -> Option<CursorMove> {
        let colon = rest.iter().position(|b| *b == b':')?;
        let args = String::from_utf8_lossy(&rest[..colon]);
        let arg = |name: &str| {
            args.split(';')
                .filter_map(|pair| pair.split_once('='))
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        };
        if arg("inline").as_deref() != Some("1") {
            return None;
        }
        let bytes = base64_decode(&rest[colon + 1..])?;
        let image = if bytes.starts_with(b"\x89PNG") {
            Image::png(bytes)?
        } else if bytes.starts_with(b"GIF8") {
            Image::gif(bytes)?
        } else {
            return None;
        };
        let (screen_rows, screen_columns) = screen.size();
        let (cw, ch) = (f64::from(CELL_PIXELS.0), f64::from(CELL_PIXELS.1));
        // A size in cells, or None for `auto`.
        let length = |value: Option<String>, cell: f64, whole: u16| -> Option<f64> {
            let value = value?;
            if value == "auto" {
                None
            } else if let Some(px) = value.strip_suffix("px") {
                px.parse::<f64>().ok().map(|px| px / cell)
            } else if let Some(percent) = value.strip_suffix('%') {
                percent
                    .parse::<f64>()
                    .ok()
                    .map(|p| f64::from(whole) * p / 100.0)
            } else {
                value.parse::<f64>().ok()
            }
        };
        let (w, h) = (f64::from(image.width), f64::from(image.height));
        let wanted_w = length(arg("width"), cw, screen_columns).filter(|v| *v > 0.0);
        let wanted_h = length(arg("height"), ch, screen_rows).filter(|v| *v > 0.0);
        let preserve = arg("preserveAspectRatio").as_deref() != Some("0");
        let (box_w, box_h) = match (wanted_w, wanted_h) {
            (Some(bw), Some(bh)) => (bw * cw, bh * ch),
            (Some(bw), None) => (bw * cw, bw * cw * h / w),
            (None, Some(bh)) => (bh * ch * w / h, bh * ch),
            (None, None) => (w, h),
        };
        let (columns, rows) = cells_for(box_w, box_h);
        let (box_w, box_h) = (f64::from(columns) * cw, f64::from(rows) * ch);
        let source = if preserve {
            // Fitted in the box, at its top left.
            let scale = (box_w / w).min(box_h / h);
            [0.0, 0.0, (box_w / scale) as f32, (box_h / scale) as f32]
        } else {
            [0.0, 0.0, w as f32, h as f32]
        };
        let (row, column) = screen.cursor_position();
        self.overlay(
            screen,
            None,
            Arc::new(image),
            (column, row),
            (columns, rows),
            source,
            0,
        );
        Some(CursorMove {
            down: rows - 1,
            right: columns,
        })
    }

    /// Place `image` at (`column`, `row`) over `columns` x `rows` cells.
    /// Without a Kitty key it is part of the cells (Sixel, iTerm2) and
    /// replaces the earlier such images it covers whole.
    #[allow(clippy::too_many_arguments)]
    fn overlay(
        &mut self,
        screen: &vt100::Screen,
        kitty: Option<(u32, u32)>,
        image: Arc<Image>,
        (column, row): (u16, u16),
        (columns, rows): (u16, u16),
        source: [f32; 4],
        z: i64,
    ) {
        let under = kitty.is_none().then(|| {
            let mut cells = Vec::with_capacity(usize::from(columns) * usize::from(rows));
            for y in row..row.saturating_add(rows) {
                for x in column..column.saturating_add(columns) {
                    cells.push(screen.cell(y, x).map(CellState::of));
                }
            }
            cells
        });
        let placed = Overlay {
            kitty,
            image,
            column,
            row,
            columns,
            rows,
            source,
            z,
            under,
        };
        let inside = |old: &Overlay| {
            old.column >= placed.column
                && old.row >= placed.row
                && old.column + old.columns <= placed.column.saturating_add(placed.columns)
                && old.row + old.rows <= placed.row.saturating_add(placed.rows)
        };
        self.overlays.retain(|old| match (kitty, old.kitty) {
            (Some(key), Some(old_key)) => key != old_key,
            (None, None) => !inside(old),
            _ => true,
        });
        // Images whose cells have all changed show nowhere: forget them.
        self.overlays.retain(|old| {
            old.under.is_none()
                || (old.row..old.row.saturating_add(old.rows)).any(|y| {
                    (old.column..old.column.saturating_add(old.columns))
                        .any(|x| old.shows(screen, x, y))
                })
        });
        if self.overlays.len() >= MAX_OVERLAYS {
            self.overlays.remove(0);
        }
        self.overlays.push(placed);
    }

    fn store(&mut self, id: u32, image: Arc<Image>) {
        self.stored_bytes += image.kept();
        if let Some(old) = self.images.insert(id, image) {
            self.stored_bytes -= old.kept();
            self.stored.retain(|stored| *stored != id);
        }
        self.stored.push_back(id);
        while self.stored_bytes > MAX_STORED && self.stored.len() > 1 {
            let oldest = self.stored.pop_front().expect("more than one image");
            if let Some(old) = self.images.remove(&oldest) {
                self.stored_bytes -= old.kept();
            }
        }
    }

    fn kitty(&mut self, rest: &[u8], screen: &vt100::Screen) -> Option<CursorMove> {
        let (keys, payload) = match rest.iter().position(|b| *b == b';') {
            Some(at) => (Keys::parse(&rest[..at]), &rest[at + 1..]),
            None => (Keys::parse(rest), &[][..]),
        };
        let more = keys.number(b'm') == Some(1);
        let (keys, payload) = match self.pending.take() {
            // A later chunk: its keys are only `m` (and `q`).
            Some((first, mut so_far)) => {
                so_far.extend_from_slice(payload);
                if more {
                    if so_far.len() > MAX_STORED {
                        return None;
                    }
                    self.pending = Some((first, so_far));
                    return None;
                }
                (first, so_far)
            }
            None if more => {
                self.pending = Some((keys, payload.to_vec()));
                return None;
            }
            None => (keys, payload.to_vec()),
        };
        match keys.char(b'a').unwrap_or('t') {
            't' => {
                self.transmit(&keys, &payload);
                None
            }
            'T' => {
                let id = self.transmit(&keys, &payload)?;
                self.place(&keys, id, screen)
            }
            'p' => {
                let id = self.id(&keys)?;
                self.place(&keys, id, screen)
            }
            'd' => {
                self.delete(&keys, screen);
                None
            }
            // Queries, animation frames and composition: nothing to show.
            _ => None,
        }
    }

    /// The image id a command names: `i`, or the id given to its number
    /// `I`.
    fn id(&self, keys: &Keys) -> Option<u32> {
        match keys.unsigned(b'i') {
            0 => self.numbers.get(&keys.unsigned(b'I')).copied(),
            id => Some(id),
        }
    }

    /// Decode and keep a transmitted image; returns its id.
    fn transmit(&mut self, keys: &Keys, payload: &[u8]) -> Option<u32> {
        // Only data in the escape itself: files and shared memory are the
        // program's machine's, not the recording's.
        if keys.char(b't').is_some_and(|t| t != 'd') {
            return None;
        }
        let mut bytes = base64_decode(payload)?;
        if keys.char(b'o') == Some('z') {
            let mut inflated = Vec::new();
            flate2::read::ZlibDecoder::new(bytes.as_slice())
                .take(MAX_IMAGE_PIXELS * 4 + 1)
                .read_to_end(&mut inflated)
                .ok()?;
            bytes = inflated;
        }
        let image = match keys.number(b'f').unwrap_or(32) {
            100 => Image::png(bytes)?,
            format @ (24 | 32) => {
                let (width, height) = (keys.unsigned(b's'), keys.unsigned(b'v'));
                let size = width as usize * height as usize;
                let rgba: Vec<u8> = if format == 24 {
                    if bytes.len() != size * 3 {
                        return None;
                    }
                    bytes
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .flat_map(|[r, g, b]| [*r, *g, *b, 255])
                        .collect()
                } else {
                    bytes
                };
                Image::rgba(width, height, &rgba)?
            }
            _ => return None,
        };
        let id = match self.id(keys) {
            Some(id) => id,
            None => {
                let number = keys.unsigned(b'I');
                // A fresh id, above the ones programs pick.
                let id = u32::MAX - self.numbers.len() as u32;
                if number != 0 {
                    self.numbers.insert(number, id);
                }
                id
            }
        };
        self.store(id, Arc::new(image));
        Some(id)
    }

    /// Place image `id` as `keys` say: at the cursor, or as a virtual
    /// placement for placeholder cells (`U=1`).
    fn place(&mut self, keys: &Keys, id: u32, screen: &vt100::Screen) -> Option<CursorMove> {
        let image = self.images.get(&id)?.clone();
        let (w, h) = (image.width as f64, image.height as f64);
        let (x, y) = (keys.unsigned(b'x') as f64, keys.unsigned(b'y') as f64);
        let crop = |given: u32, from: f64, whole: f64| {
            if given == 0 {
                (whole - from).max(1.0)
            } else {
                f64::from(given)
            }
        };
        let (crop_w, crop_h) = (
            crop(keys.unsigned(b'w'), x, w),
            crop(keys.unsigned(b'h'), y, h),
        );
        let (cw, ch) = (f64::from(CELL_PIXELS.0), f64::from(CELL_PIXELS.1));
        let given = |key: u8| match keys.unsigned(key) {
            0 => None,
            n => Some(n.min(u32::from(u16::MAX)) as u16),
        };
        let native = cells_for(crop_w, crop_h);
        let (columns, rows) = match (given(b'c'), given(b'r')) {
            (Some(c), Some(r)) => (c, r),
            (Some(c), None) => (c, cells_for(0.0, f64::from(c) * cw * crop_h / crop_w).1),
            (None, Some(r)) => (cells_for(f64::from(r) * ch * crop_w / crop_h, 0.0).0, r),
            (None, None) => native,
        };
        if keys.number(b'U') == Some(1) {
            self.virtuals.insert(id, (columns, rows));
            return None;
        }
        // Stretched over the cells it was given; at its own size otherwise.
        let source = if given(b'c').is_some() || given(b'r').is_some() {
            [x as f32, y as f32, crop_w as f32, crop_h as f32]
        } else {
            [
                x as f32,
                y as f32,
                (f64::from(columns) * cw) as f32,
                (f64::from(rows) * ch) as f32,
            ]
        };
        let (row, column) = screen.cursor_position();
        let placement = keys.unsigned(b'p');
        self.overlay(
            screen,
            Some((id, placement)),
            image,
            (column, row),
            (columns, rows),
            source,
            keys.number(b'z').unwrap_or(0),
        );
        (keys.number(b'C') != Some(1)).then_some(CursorMove {
            down: rows - 1,
            right: columns,
        })
    }

    fn delete(&mut self, keys: &Keys, screen: &vt100::Screen) {
        let what = keys.char(b'd').unwrap_or('a');
        let free = what.is_ascii_uppercase();
        let kitty = |overlay: &Overlay| overlay.kitty.is_some();
        match what.to_ascii_lowercase() {
            'a' => {
                self.overlays.retain(|o| !kitty(o));
                self.virtuals.clear();
                if free {
                    self.images.clear();
                    self.stored.clear();
                    self.stored_bytes = 0;
                }
            }
            'i' | 'n' => {
                let Some(id) = self.id(keys) else {
                    return;
                };
                let placement = keys.unsigned(b'p');
                self.overlays.retain(|o| match o.kitty {
                    Some((image, p)) => image != id || (placement != 0 && p != placement),
                    None => true,
                });
                if placement == 0 {
                    self.virtuals.remove(&id);
                    if free {
                        if let Some(old) = self.images.remove(&id) {
                            self.stored_bytes -= old.kept();
                            self.stored.retain(|stored| *stored != id);
                        }
                    }
                }
            }
            'c' | 'p' => {
                let (column, row) = if what.eq_ignore_ascii_case(&'c') {
                    let (row, column) = screen.cursor_position();
                    (column, row)
                } else {
                    let at = |key| keys.unsigned(key).saturating_sub(1) as u16;
                    (at(b'x'), at(b'y'))
                };
                self.overlays
                    .retain(|o| !(kitty(o) && o.covers(column, row)));
            }
            'z' => {
                let z = keys.number(b'z').unwrap_or(0);
                self.overlays.retain(|o| !(kitty(o) && o.z == z));
            }
            _ => {}
        }
    }

    /// What shows on `screen` now: placeholder cells' images first, then
    /// the placed images in the order they were drawn (Kitty's by z
    /// index). Also returns the placeholder cells, (column, row), which a
    /// snapshot shows blank.
    pub fn tiles(&self, screen: &vt100::Screen) -> (Vec<Tile>, Vec<(u16, u16)>) {
        let mut tiles = Vec::new();
        let mut cells = Vec::new();
        if screen.scrollback() > 0 {
            return (tiles, cells);
        }
        let (rows, columns) = screen.size();
        for y in 0..rows {
            self.placeholder_row(screen, y, columns, &mut tiles, &mut cells);
        }
        let mut overlays: Vec<&Overlay> = self.overlays.iter().collect();
        overlays.sort_by_key(|o| if o.kitty.is_some() { o.z } else { 0 });
        for overlay in overlays {
            for y in overlay.row..overlay.row.saturating_add(overlay.rows).min(rows) {
                let mut x = overlay.column;
                let end = overlay.column.saturating_add(overlay.columns).min(columns);
                while x < end {
                    if !overlay.shows(screen, x, y) {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < end && overlay.shows(screen, x, y) {
                        x += 1;
                    }
                    let [sx, sy, sw, sh] = overlay.source;
                    let across = sw / f32::from(overlay.columns);
                    let down = sh / f32::from(overlay.rows);
                    tiles.push(Tile {
                        image: overlay.image.clone(),
                        column: start,
                        row: y,
                        columns: x - start,
                        rows: 1,
                        source: [
                            sx + f32::from(start - overlay.column) * across,
                            sy + f32::from(y - overlay.row) * down,
                            f32::from(x - start) * across,
                            down,
                        ],
                        below_text: overlay.kitty.is_some() && overlay.z < 0,
                    });
                }
            }
        }
        (tiles, cells)
    }

    /// The tiles of row `y`'s placeholder cells: runs of cells of one
    /// image's row, in consecutive columns.
    fn placeholder_row(
        &self,
        screen: &vt100::Screen,
        y: u16,
        columns: u16,
        tiles: &mut Vec<Tile>,
        cells: &mut Vec<(u16, u16)>,
    ) {
        // The cell before: its image id, image row and column.
        let mut before: Option<(u32, u32, u32)> = None;
        // The run being gathered: its first column, id, image row, first
        // image column and length.
        let mut run: Option<(u16, u32, u32, u32, u16)> = None;
        let flush = |run: &mut Option<(u16, u32, u32, u32, u16)>, tiles: &mut Vec<Tile>| {
            if let Some((x, id, image_row, image_column, length)) = run.take() {
                if let Some(tile) =
                    self.placeholder_tile(id, (x, y), image_row, image_column, length)
                {
                    tiles.push(tile);
                }
            }
        };
        for x in 0..columns {
            let cell = screen.cell(y, x);
            let text = cell.map_or("", vt100::Cell::contents);
            let mut chars = text.chars();
            if chars.next() != Some(PLACEHOLDER) {
                before = None;
                flush(&mut run, tiles);
                continue;
            }
            cells.push((x, y));
            let marks: Vec<u32> = chars.filter_map(diacritic).collect();
            let low = match cell.map(vt100::Cell::fgcolor) {
                Some(vt100::Color::Rgb(r, g, b)) => {
                    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
                }
                Some(vt100::Color::Idx(i)) => u32::from(i),
                _ => 0,
            };
            // Diacritics left out continue the cell before.
            let (image_row, image_column, high) = match (marks.as_slice(), before) {
                ([row, column, high, ..], _) => (*row, *column, *high),
                ([row, column], Some((id, r, _))) if *row == r => (*row, *column, id >> 24),
                ([row, column], _) => (*row, *column, 0),
                ([row], Some((id, r, c))) if *row == r => (*row, c + 1, id >> 24),
                ([row], _) => (*row, 0, 0),
                ([], Some((id, r, c))) => (r, c + 1, id >> 24),
                ([], None) => (0, 0, 0),
            };
            let id = high << 24 | low;
            before = Some((id, image_row, image_column));
            match &mut run {
                Some((_, run_id, run_row, first, length))
                    if *run_id == id
                        && *run_row == image_row
                        && *first + u32::from(*length) == image_column =>
                {
                    *length += 1;
                }
                _ => {
                    flush(&mut run, tiles);
                    run = Some((x, id, image_row, image_column, 1));
                }
            }
        }
        flush(&mut run, tiles);
    }

    /// The tile for `length` placeholder cells from (`x`, `y`) showing
    /// image `id`'s row `image_row` from column `image_column`. The image
    /// is fitted in its virtual placement's cells, centred, its aspect
    /// kept, as Kitty draws it.
    fn placeholder_tile(
        &self,
        id: u32,
        (x, y): (u16, u16),
        image_row: u32,
        image_column: u32,
        length: u16,
    ) -> Option<Tile> {
        let image = self.images.get(&id)?;
        let &(columns, rows) = self.virtuals.get(&id)?;
        if image_row >= u32::from(rows) || image_column >= u32::from(columns) {
            return None;
        }
        let (cw, ch) = (CELL_PIXELS.0 as f32, CELL_PIXELS.1 as f32);
        let (box_w, box_h) = (f32::from(columns) * cw, f32::from(rows) * ch);
        let (w, h) = (image.width as f32, image.height as f32);
        let scale = (box_w / w).min(box_h / h);
        let (left, top) = ((box_w - w * scale) / 2.0, (box_h - h * scale) / 2.0);
        Some(Tile {
            image: image.clone(),
            column: x,
            row: y,
            columns: length,
            rows: 1,
            source: [
                (image_column as f32 * cw - left) / scale,
                (image_row as f32 * ch - top) / scale,
                f32::from(length) * cw / scale,
                ch / scale,
            ],
            below_text: true,
        })
    }
}

/// The VT340's default colour registers, in percent.
const SIXEL_PALETTE: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (20, 20, 80),
    (80, 13, 13),
    (20, 80, 20),
    (80, 20, 80),
    (20, 80, 80),
    (80, 80, 20),
    (53, 53, 53),
    (26, 26, 26),
    (33, 33, 60),
    (60, 26, 26),
    (33, 60, 33),
    (60, 33, 60),
    (33, 60, 60),
    (60, 60, 33),
    (80, 80, 80),
];

/// DEC's HLS (hue 0 blue, 120 red, 240 green; lightness and saturation in
/// percent) as RGB.
fn hls(h: u32, l: u32, s: u32) -> [u8; 3] {
    let hue = ((h + 240) % 360) as f64;
    let (l, s) = (f64::from(l.min(100)) / 100.0, f64::from(s.min(100)) / 100.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// A Sixel image from the data after its `q`. Pixels no sixel sets are
/// transparent. `None` when it draws nothing, or is larger than
/// [`MAX_IMAGE_PIXELS`] allows.
pub fn decode_sixel(data: &[u8]) -> Option<Image> {
    let percent = |v: u32| (v.min(100) * 255 / 100) as u8;
    let mut palette: Vec<[u8; 3]> = vec![[0, 0, 0]; 256];
    for (slot, (r, g, b)) in palette.iter_mut().zip(SIXEL_PALETTE) {
        *slot = [percent(r.into()), percent(g.into()), percent(b.into())];
    }
    let limit = 4096usize;
    let (mut width, mut height) = (0usize, 0usize);
    // The size the raster attributes give: the image's, its last band cut
    // to it.
    let mut declared: Option<(usize, usize)> = None;
    let mut pixels: Vec<u8> = Vec::new();
    let mut stride = 0usize;
    let (mut x, mut y) = (0usize, 0usize);
    let mut colour = [0u8, 0, 0];
    let mut i = 0;
    // Numbers after `"`, `#` or `!`.
    let numbers = |i: &mut usize| -> Vec<u32> {
        let mut out = vec![0u32];
        while *i < data.len() {
            match data[*i] {
                d @ b'0'..=b'9' => {
                    let last = out.last_mut().expect("a number");
                    *last = last.saturating_mul(10).saturating_add(u32::from(d - b'0'));
                }
                b';' => out.push(0),
                _ => break,
            }
            *i += 1;
        }
        out
    };
    // Grow the canvas to hold (w, h); false past the limit.
    let mut grow = |pixels: &mut Vec<u8>, stride: &mut usize, w: usize, h: usize| -> bool {
        if w > limit || h > limit {
            return false;
        }
        let new_stride = (*stride).max(w);
        let rows = (pixels.len() / 4).checked_div(*stride).unwrap_or(0);
        if new_stride != *stride {
            let mut wider = vec![0u8; new_stride * rows.max(h) * 4];
            for row in 0..rows {
                let from = row * *stride * 4;
                wider[row * new_stride * 4..row * new_stride * 4 + *stride * 4]
                    .copy_from_slice(&pixels[from..from + *stride * 4]);
            }
            *pixels = wider;
            *stride = new_stride;
        } else if h > rows {
            pixels.resize(new_stride * h * 4, 0);
        }
        true
    };
    while i < data.len() {
        let byte = data[i];
        i += 1;
        match byte {
            b'"' => {
                let n = numbers(&mut i);
                if let (Some(&w), Some(&h)) = (n.get(2), n.get(3)) {
                    let (w, h) = (w as usize, h as usize);
                    if !grow(&mut pixels, &mut stride, w, h) {
                        return None;
                    }
                    if w > 0 && h > 0 {
                        declared = Some((w, h));
                    }
                }
            }
            b'#' => {
                let n = numbers(&mut i);
                let register = (n[0] as usize).min(255);
                if let [_, kind, a, b, c] = n[..] {
                    palette[register] = if kind == 1 {
                        hls(a, b, c)
                    } else {
                        [percent(a), percent(b), percent(c)]
                    };
                }
                colour = palette[register];
            }
            b'!' => {
                let count = numbers(&mut i)[0].max(1) as usize;
                if let Some(&sixel @ b'?'..=b'~') = data.get(i) {
                    i += 1;
                    if !draw(
                        &mut pixels,
                        &mut stride,
                        &mut grow,
                        (x, y),
                        count,
                        sixel - b'?',
                        colour,
                    ) {
                        return None;
                    }
                    x += count;
                    width = width.max(x);
                    height = height.max(y + 6);
                }
            }
            b'$' => x = 0,
            b'-' => {
                x = 0;
                y += 6;
            }
            b'?'..=b'~' => {
                if !draw(
                    &mut pixels,
                    &mut stride,
                    &mut grow,
                    (x, y),
                    1,
                    byte - b'?',
                    colour,
                ) {
                    return None;
                }
                x += 1;
                width = width.max(x);
                height = height.max(y + 6);
            }
            _ => {}
        }
    }
    if let Some(size) = declared {
        (width, height) = size;
    }
    if width == 0 || height == 0 {
        return None;
    }
    // Cropped to the image's size (the last band may be taller), with the
    // stride squeezed out.
    let rows = (pixels.len() / 4).checked_div(stride).unwrap_or(0);
    let mut out = vec![0u8; width * height * 4];
    for row in 0..height.min(rows) {
        let w = width.min(stride);
        out[row * width * 4..row * width * 4 + w * 4]
            .copy_from_slice(&pixels[row * stride * 4..row * stride * 4 + w * 4]);
    }
    Image::rgba(width as u32, height as u32, &out)
}

/// Draw one sixel (`bits`, the low bit on top) `count` times from (x, y).
#[allow(clippy::too_many_arguments)]
fn draw(
    pixels: &mut Vec<u8>,
    stride: &mut usize,
    grow: &mut impl FnMut(&mut Vec<u8>, &mut usize, usize, usize) -> bool,
    (x, y): (usize, usize),
    count: usize,
    bits: u8,
    colour: [u8; 3],
) -> bool {
    if !grow(pixels, stride, x + count, y + 6) {
        return false;
    }
    for bit in 0..6 {
        if bits & (1 << bit) == 0 {
            continue;
        }
        let row = (y + bit) * *stride;
        for px in x..x + count {
            let at = (row + px) * 4;
            pixels[at..at + 4].copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of `width` x `height` pixels, each `colour`.
    pub(crate) fn png(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let pixels: Vec<u8> = (0..width * height).flat_map(|_| colour).collect();
            writer.write_image_data(&pixels).unwrap();
        }
        out
    }

    #[test]
    fn base64_round_trips_and_refuses_junk() {
        assert_eq!(base64_decode(b"aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode(b"aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64_decode(b"aGVs\nbG8=").unwrap(), b"hello");
        assert!(base64_decode(b"a*b").is_none());
    }

    #[test]
    fn images_keep_their_pixels_compressed() {
        let pixels: Vec<u8> = (0..64u32 * 32).flat_map(|i| [i as u8, 0, 9, 255]).collect();
        let image = Image::rgba(64, 32, &pixels).unwrap();
        assert!(image.kept() < pixels.len());
        assert_eq!(image.pixels().unwrap(), pixels);
        let png = Image::png(png(3, 2, [1, 2, 3, 255])).unwrap();
        assert_eq!((png.width(), png.height()), (3, 2));
        assert_eq!(&png.pixels().unwrap()[..4], &[1, 2, 3, 255]);
        // Equal by content.
        assert_eq!(
            Image::png(super::tests::png(3, 2, [1, 2, 3, 255])).unwrap(),
            png
        );
        assert!(Image::rgba(5000, 5000, &[]).is_none());
        assert!(Image::png(b"not a png".to_vec()).is_none());
    }

    #[test]
    fn sixel_draws_bands_repeats_and_colours() {
        // Register 1 is red, 2 is HLS pure green (hue 240, lightness 50,
        // saturation 100); two bands; a repeat.
        let image = decode_sixel(b"\"1;1;4;10#1;2;100;0;0#1!4~-#2;1;240;50;100#2A").unwrap();
        // The raster attributes' size, the second band cut to it.
        assert_eq!((image.width(), image.height()), (4, 10));
        let px = image.pixels().unwrap();
        let at = |x: usize, y: usize| &px[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(at(3, 5), [255, 0, 0, 255]);
        // `A` (0b10) sets the second pixel of the second band.
        assert_eq!(at(0, 7), [0, 255, 0, 255]);
        assert_eq!(at(0, 6)[3], 0, "unset pixels are transparent");
        assert_eq!(at(1, 7)[3], 0);
        // Without them, as far as it draws.
        let image = decode_sixel(b"#1!3~-~").unwrap();
        assert_eq!((image.width(), image.height()), (3, 12));
        assert!(decode_sixel(b"").is_none());
        assert!(decode_sixel(b"!99999~").is_none());
    }

    #[test]
    fn placeholder_diacritics_count_up() {
        assert_eq!(diacritic('\u{0305}'), Some(0));
        assert_eq!(diacritic('\u{030D}'), Some(1));
        assert_eq!(diacritic('\u{1D244}'), Some(296));
        assert_eq!(diacritic('a'), None);
    }
}
