//! Colour emoji from the embedded Twemoji font.
//!
//! Twemoji Mozilla is a COLRv0 font: each emoji is a stack of plain glyphs,
//! each filled with one palette colour, and ZWJ sequences, skin tones and
//! keycaps are GSUB ligatures. ttf-parser reads the layers, ligatures and
//! outlines, and a small coverage rasterizer (the font-rs algorithm) fills
//! them: fontdue only loads glyphs that have a character, and layer glyphs
//! have none.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use ttf_parser::colr::{ClipBox, CompositeMode, Paint, Painter};
use ttf_parser::gsub::SubstitutionSubtable;
use ttf_parser::{Face, GlyphId, OutlineBuilder, RgbaColor, Transform};

const TWEMOJI: &[u8] = include_bytes!("../../fonts/TwemojiMozilla.ttf");
/// The em square in font units: every emoji is drawn in the square from
/// `DESCENT` below the baseline to `UNITS - DESCENT` above it.
const UNITS: f32 = 512.0;
const DESCENT: f32 = 64.0;

/// An emoji drawn into a square, straight (not premultiplied) RGBA.
pub struct Bitmap {
    pub side: usize,
    pub rgba: Vec<[u8; 4]>,
}

/// Drawn emoji by cluster and size; `None` for what the font lacks.
type Cache = HashMap<(String, u32), Option<Arc<Bitmap>>>;

/// The emoji face, parsed on first use.
pub struct Emoji {
    face: OnceLock<Option<Face<'static>>>,
    cache: Mutex<Cache>,
}

impl Default for Emoji {
    fn default() -> Self {
        Emoji {
            face: OnceLock::new(),
            cache: Mutex::new(HashMap::new()),
        }
    }
}

/// Records each COLRv0 layer: a glyph and its colour.
#[derive(Default)]
struct Layers {
    outline: Option<GlyphId>,
    layers: Vec<(GlyphId, RgbaColor)>,
}

impl<'a> Painter<'a> for Layers {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.outline = Some(glyph_id);
    }
    fn paint(&mut self, paint: Paint<'a>) {
        if let (Some(glyph), Paint::Solid(colour)) = (self.outline, paint) {
            self.layers.push((glyph, colour));
        }
    }
    fn push_clip(&mut self) {}
    fn push_clip_box(&mut self, _: ClipBox) {}
    fn pop_clip(&mut self) {}
    fn push_layer(&mut self, _: CompositeMode) {}
    fn pop_layer(&mut self) {}
    fn push_transform(&mut self, _: Transform) {}
    fn pop_transform(&mut self) {}
}

impl Emoji {
    fn face(&self) -> Option<&Face<'static>> {
        self.face
            .get_or_init(|| Face::parse(TWEMOJI, 0).ok())
            .as_ref()
    }

    /// Whether the font has `c`.
    pub fn has(&self, c: char) -> bool {
        self.face()
            .is_some_and(|face| face.glyph_index(c).is_some())
    }

    /// The glyph for a whole cluster: its ligature when the font has one,
    /// retried without variation selectors, else its first character.
    fn glyph(face: &Face<'_>, text: &str) -> Option<GlyphId> {
        let glyphs: Option<Vec<GlyphId>> = text.chars().map(|c| face.glyph_index(c)).collect();
        if let Some(glyphs) = glyphs.filter(|g| g.len() > 1) {
            if let Some(ligature) = Self::ligature(face, &glyphs) {
                return Some(ligature);
            }
        }
        let bare: String = text.chars().filter(|c| *c != '\u{fe0f}').collect();
        if bare != text {
            let glyphs: Option<Vec<GlyphId>> = bare.chars().map(|c| face.glyph_index(c)).collect();
            if let Some(glyphs) = glyphs {
                if glyphs.len() == 1 {
                    return Some(glyphs[0]);
                }
                if let Some(ligature) = Self::ligature(face, &glyphs) {
                    return Some(ligature);
                }
            }
        }
        face.glyph_index(text.chars().next()?)
    }

    fn ligature(face: &Face<'_>, glyphs: &[GlyphId]) -> Option<GlyphId> {
        let gsub = face.tables().gsub?;
        for lookup in gsub.lookups {
            for subtable in lookup.subtables.into_iter::<SubstitutionSubtable>() {
                let SubstitutionSubtable::Ligature(table) = subtable else {
                    continue;
                };
                let Some(index) = table.coverage.get(glyphs[0]) else {
                    continue;
                };
                let Some(set) = table.ligature_sets.get(index) else {
                    continue;
                };
                for ligature in set {
                    if ligature.components.len() as usize == glyphs.len() - 1
                        && ligature
                            .components
                            .into_iter()
                            .eq(glyphs[1..].iter().copied())
                    {
                        return Some(ligature.glyph);
                    }
                }
            }
        }
        None
    }

    /// `text` drawn into a square `side` pixels wide, or `None` when the
    /// font does not have it.
    pub fn render(&self, text: &str, side: usize) -> Option<Arc<Bitmap>> {
        let key = (text.to_string(), side as u32);
        if let Some(hit) = self.cache.lock().expect("emoji cache").get(&key) {
            return hit.clone();
        }
        let bitmap = self.draw(text, side).map(Arc::new);
        self.cache
            .lock()
            .expect("emoji cache")
            .insert(key, bitmap.clone());
        bitmap
    }

    fn draw(&self, text: &str, side: usize) -> Option<Bitmap> {
        let face = self.face()?;
        let glyph = Self::glyph(face, text)?;
        let mut layers = Layers::default();
        let black = RgbaColor::new(0, 0, 0, 255);
        if face
            .paint_color_glyph(glyph, 0, black, &mut layers)
            .is_none()
            || layers.layers.is_empty()
        {
            layers.layers = vec![(glyph, black)];
        }
        // Straight-alpha "over" compositing, layer by layer, in floats.
        let mut pixels = vec![[0f32; 4]; side * side];
        for (glyph, colour) in layers.layers {
            let mut raster = Raster::new(side, side as f32 / UNITS);
            if face.outline_glyph(glyph, &mut raster).is_none() {
                continue;
            }
            let source = [
                colour.red as f32 / 255.0,
                colour.green as f32 / 255.0,
                colour.blue as f32 / 255.0,
            ];
            for (pixel, coverage) in pixels.iter_mut().zip(raster.coverage()) {
                let a = coverage * (colour.alpha as f32 / 255.0);
                if a <= 0.0 {
                    continue;
                }
                let out = a + pixel[3] * (1.0 - a);
                for channel in 0..3 {
                    pixel[channel] =
                        (source[channel] * a + pixel[channel] * pixel[3] * (1.0 - a)) / out;
                }
                pixel[3] = out;
            }
        }
        let rgba = pixels
            .into_iter()
            .map(|p| p.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
            .collect();
        Some(Bitmap { side, rgba })
    }
}

/// Antialiased coverage of one glyph outline in a square, by signed-area
/// accumulation (as in font-rs): each edge adds the area it sweeps to the
/// cells it crosses, and a running sum along the buffer gives the coverage.
/// Overlapping contours are clamped, which matches the non-zero rule for the
/// same-direction overlaps emoji fonts use.
struct Raster {
    side: usize,
    scale: f32,
    area: Vec<f32>,
    start: (f32, f32),
    last: (f32, f32),
}

impl Raster {
    fn new(side: usize, scale: f32) -> Raster {
        Raster {
            side,
            scale,
            area: vec![0.0; side * side + 3],
            start: (0.0, 0.0),
            last: (0.0, 0.0),
        }
    }

    /// Font units to pixels: y down, the baseline `DESCENT` above the bottom.
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.scale, (UNITS - DESCENT - y) * self.scale)
    }

    fn coverage(&self) -> impl Iterator<Item = f32> + '_ {
        let mut sum = 0.0f32;
        self.area[..self.side * self.side].iter().map(move |a| {
            sum += a;
            sum.abs().min(1.0)
        })
    }

    fn line(&mut self, p0: (f32, f32), p1: (f32, f32)) {
        // Keep x inside the buffer; outside the square it only shifts where
        // a row's area lands, which the running sum cancels.
        let clamp = |p: (f32, f32)| (p.0.clamp(0.0, self.side as f32 - 0.001), p.1);
        let (p0, p1) = (clamp(p0), clamp(p1));
        if (p0.1 - p1.1).abs() <= f32::EPSILON {
            return;
        }
        let (dir, p0, p1) = if p0.1 < p1.1 {
            (1.0, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let mut x = p0.0;
        if p0.1 < 0.0 {
            x -= p0.1 * dxdy;
        }
        let first = p0.1.max(0.0) as usize;
        let last = (p1.1.ceil().max(0.0) as usize).min(self.side);
        for y in first..last {
            let row = y * self.side;
            let dy = ((y + 1) as f32).min(p1.1) - (y as f32).max(p0.1);
            let next = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < next { (x, next) } else { (next, x) };
            let x0floor = x0.floor();
            let x0i = x0floor as usize;
            let x1ceil = x1.ceil();
            let x1i = x1ceil as usize;
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + next) - x0floor;
                self.area[row + x0i] += d - d * xmf;
                self.area[row + x0i + 1] += d * xmf;
            } else {
                let s = (x1 - x0).recip();
                let x0f = x0 - x0floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                self.area[row + x0i] += d * a0;
                if x1i == x0i + 2 {
                    self.area[row + x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    self.area[row + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        self.area[row + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    self.area[row + x1i - 1] += d * (1.0 - a2 - am);
                }
                self.area[row + x1i] += d * am;
            }
            x = next;
        }
    }
}

impl OutlineBuilder for Raster {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.point(x, y);
        self.last = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.line(self.last, p);
        self.last = p;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, c, p) = (self.last, self.point(x1, y1), self.point(x, y));
        const STEPS: usize = 8;
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let u = 1.0 - t;
            let q = (
                u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p.0,
                u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p.1,
            );
            self.line(self.last, q);
            self.last = q;
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, c1, c2, p) = (
            self.last,
            self.point(x1, y1),
            self.point(x2, y2),
            self.point(x, y),
        );
        const STEPS: usize = 12;
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            let q = (
                a * p0.0 + b * c1.0 + c * c2.0 + d * p.0,
                a * p0.1 + b * c1.1 + c * c2.1 + d * p.1,
            );
            self.line(self.last, q);
            self.last = q;
        }
    }

    fn close(&mut self) {
        if self.last != self.start {
            self.line(self.last, self.start);
        }
        self.last = self.start;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colours(bitmap: &Bitmap) -> usize {
        let mut seen: Vec<[u8; 3]> = bitmap
            .rgba
            .iter()
            .filter(|p| p[3] == 255)
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        seen.sort();
        seen.dedup();
        seen.len()
    }

    #[test]
    fn draws_emoji_in_colour() {
        let emoji = Emoji::default();
        let rocket = emoji.render("🚀", 32).expect("rocket");
        assert_eq!(rocket.rgba.len(), 32 * 32);
        assert!(colours(&rocket) >= 4, "a rocket has several colours");
        // Transparent corners: the emoji is drawn over the cell background.
        assert_eq!(rocket.rgba[0][3], 0);
        assert!(!emoji.has('A') && emoji.render("A", 32).is_none());
    }

    #[test]
    fn sequences_use_their_ligature() {
        let emoji = Emoji::default();
        let face = emoji.face().unwrap();
        let woman = face.glyph_index('👩').unwrap();
        let family = Emoji::glyph(face, "👩\u{200d}👧").unwrap();
        assert_ne!(family, woman);
        let thumbs = face.glyph_index('👍').unwrap();
        assert_ne!(Emoji::glyph(face, "👍🏽").unwrap(), thumbs);
        // A sequence the font lacks falls back to its first emoji, and a
        // heart with VS16 is the plain heart.
        assert_eq!(Emoji::glyph(face, "👩\u{200d}🚀\u{200d}🚀").unwrap(), woman);
        assert_eq!(
            Emoji::glyph(face, "❤\u{fe0f}").unwrap(),
            face.glyph_index('❤').unwrap()
        );
    }
}
