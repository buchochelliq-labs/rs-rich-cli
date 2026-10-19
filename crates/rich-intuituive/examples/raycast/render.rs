//! The 3D view, drawn into an RGB framebuffer: textured walls with
//! distance and side shading, a cast (or flat, fogged) floor and ceiling,
//! sprites depth-sorted against a z-buffer, the pistol and its flash, and
//! the hurt and pickup tints. After Lode Vandevenne's raycasting tutorial
//! (<https://lodev.org/cgtutor/raycasting.html>, parts 1 to 3): the same
//! camera plane, DDA, floor casting and sprite projection.

use super::art::{Textures, TEX};
use super::game::{Game, Hit, Kind, Wall, PLANE};

/// A rendered frame: `width` × `height` pixels, three bytes each.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
    /// What the centre column's ray hit (for tests, and the crosshair's
    /// sense of what is ahead).
    pub centre: Option<(Wall, f64)>,
    /// Which frame this is: a new number for each one drawn.
    pub number: u64,
}

impl Frame {
    pub fn blank(width: usize, height: usize) -> Frame {
        Frame {
            width,
            height,
            rgb: vec![0; width * height * 3],
            centre: None,
            number: 0,
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]]
    }

    #[inline]
    fn put(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        let i = (y * self.width + x) * 3;
        self.rgb[i..i + 3].copy_from_slice(&rgb);
    }
}

/// How to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Textured floor and ceiling (Lode's floor casting), or flat colours
    /// fading into the dark.
    pub floor_cast: bool,
    /// A pixel's height over its width on screen: 1 for half blocks and
    /// Braille, 2 for quadrants.
    pub aspect: f64,
}

/// Light at distance `d`: full near, fading into the dark.
#[inline]
fn fog(d: f64) -> f64 {
    (1.3 / (1.0 + d * 0.22)).clamp(0.08, 1.0)
}

#[inline]
fn lit(rgb: [u8; 4], light: f64) -> [u8; 3] {
    [
        (rgb[0] as f64 * light) as u8,
        (rgb[1] as f64 * light) as u8,
        (rgb[2] as f64 * light) as u8,
    ]
}

/// Draw `game` from the player's eyes into a `width` × `height` frame.
pub fn render(game: &Game, tex: &Textures, width: usize, height: usize, look: Look) -> Frame {
    let (w, h) = (width.max(2), height.max(2));
    let mut frame = Frame::blank(w, h);
    let p = &game.player;
    let (dx, dy) = p.dir();
    // Wider screens see more round the sides, not less above and below.
    let ratio = w as f64 / (h as f64 * look.aspect);
    let half = (PLANE * ratio / 1.6).clamp(0.5, 1.0);
    let (px_, py_) = p.plane(half);
    let fx = w as f64 / 2.0 / half;
    let fy = fx / look.aspect;
    let horizon = h as f64 / 2.0;

    // Floor and ceiling.
    if look.floor_cast {
        let (r0x, r0y) = (dx - px_, dy - py_);
        let (r1x, r1y) = (dx + px_, dy + py_);
        for y in (horizon as usize)..h {
            let row = y as f64 + 0.5 - horizon;
            let dist = 0.5 * fy / row;
            let light = fog(dist);
            let (sx, sy) = (dist * (r1x - r0x) / w as f64, dist * (r1y - r0y) / w as f64);
            let (mut fxw, mut fyw) = (p.x + dist * r0x, p.y + dist * r0y);
            let mirror = (2.0 * horizon) as usize - 1 - y;
            for x in 0..w {
                let tx = ((fxw - fxw.floor()) * TEX as f64) as usize;
                let ty = ((fyw - fyw.floor()) * TEX as f64) as usize;
                frame.put(x, y, lit(tex.floor.get(tx, ty), light));
                if mirror < h {
                    frame.put(x, mirror, lit(tex.ceiling.get(tx, ty), light));
                }
                fxw += sx;
                fyw += sy;
            }
        }
    } else {
        for y in 0..h {
            let t = (y as f64 + 0.5 - horizon).abs() / horizon;
            let rgb = if (y as f64) < horizon {
                [(14.0 + 46.0 * t) as u8, (14.0 + 46.0 * t) as u8, (16.0 + 50.0 * t) as u8]
            } else {
                [(12.0 + 78.0 * t) as u8, (10.0 + 70.0 * t) as u8, (8.0 + 58.0 * t) as u8]
            };
            for x in 0..w {
                frame.put(x, y, rgb);
            }
        }
    }

    // Walls, one column a ray.
    let mut zbuf = vec![f64::MAX; w];
    for (x, depth) in zbuf.iter_mut().enumerate() {
        let cam = 2.0 * (x as f64 + 0.5) / w as f64 - 1.0;
        let (rx, ry) = (dx + px_ * cam, dy + py_ * cam);
        let Some(Hit {
            dist,
            side,
            wall,
            u,
            ..
        }) = game.cast(p.x, p.y, rx, ry)
        else {
            continue;
        };
        let dist = dist.max(0.01);
        *depth = dist;
        if x == w / 2 {
            frame.centre = Some((wall, dist));
        }
        let line = fy / dist;
        let top = horizon - line / 2.0;
        let texture = tex.wall(wall);
        let tx = ((u * TEX as f64) as usize).min(TEX - 1);
        let light = fog(dist) * if side { 0.72 } else { 1.0 };
        let (y0, y1) = (top.max(0.0) as usize, (top + line).min(h as f64) as usize);
        for y in y0..y1 {
            let ty = (((y as f64 + 0.5 - top) / line) * TEX as f64) as usize;
            frame.put(x, y, lit(texture.get(tx, ty.min(TEX - 1)), light));
        }
    }

    // Sprites, farthest first, each column behind the walls left out.
    let inv = 1.0 / (px_ * dy - dx * py_);
    let mut seen: Vec<(f64, f64, usize)> = game
        .things
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.gone)
        .filter_map(|(i, t)| {
            let (sx, sy) = (t.x - p.x, t.y - p.y);
            let tx = inv * (dy * sx - dx * sy);
            let ty = inv * (-py_ * sx + px_ * sy);
            (ty > 0.1).then_some((ty, tx, i))
        })
        .collect();
    seen.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (ty, tx, i) in seen {
        let thing = &game.things[i];
        let texture = tex.sprite(thing.kind, thing.mode, thing.steps);
        let screen_x = (w as f64 / 2.0) * (1.0 + tx / ty);
        let size_y = fy / ty;
        let size_x = fx / ty;
        let top = horizon - size_y / 2.0;
        let left = screen_x - size_x / 2.0;
        let light = if thing.kind == Kind::Lamp {
            1.0
        } else {
            fog(ty)
        };
        let (x0, x1) = (left.max(0.0) as usize, (left + size_x).min(w as f64) as usize);
        let (y0, y1) = (top.max(0.0) as usize, (top + size_y).min(h as f64) as usize);
        for x in x0..x1 {
            if ty >= zbuf[x] {
                continue;
            }
            let u = (((x as f64 + 0.5 - left) / size_x) * texture.w as f64) as usize;
            for y in y0..y1 {
                let v = (((y as f64 + 0.5 - top) / size_y) * texture.h as f64) as usize;
                let rgba = texture.get(u.min(texture.w - 1), v.min(texture.h - 1));
                if rgba[3] > 0 {
                    frame.put(x, y, lit(rgba, light));
                }
            }
        }
    }

    // The pistol, bobbing as the player walks, and its flash.
    let bob = game.walked * 4.0;
    let gh = h as f64 * 0.42;
    let gw = gh * look.aspect * tex.pistol.w as f64 / tex.pistol.h as f64;
    let gx = w as f64 / 2.0 - gw / 2.0 + bob.sin() * w as f64 * 0.02;
    let gy = h as f64 - gh + bob.cos().abs() * h as f64 * 0.03 + 1.0;
    if game.flash > 0 {
        let fh = gh * 0.6;
        let fw = fh * look.aspect * tex.flash.w as f64 / tex.flash.h as f64;
        blit(&mut frame, &tex.flash, gx + gw / 2.0 - fw / 2.0, gy - fh * 0.8, fw, fh);
    }
    blit(&mut frame, &tex.pistol, gx, gy, gw, gh);

    // Tints: the flash lights the room, a hit reddens it, a pickup gilds it.
    let tint = if game.hurt > 0 {
        Some(([255.0, 0.0, 0.0], 0.06 * game.hurt as f64))
    } else if game.bonus > 0 {
        Some(([255.0, 210.0, 60.0], 0.05 * game.bonus as f64))
    } else if game.flash > 3 {
        Some(([255.0, 230.0, 160.0], 0.12))
    } else {
        None
    };
    if let Some((rgb, amount)) = tint {
        for px in frame.rgb.chunks_exact_mut(3) {
            for c in 0..3 {
                px[c] = (px[c] as f64 * (1.0 - amount) + rgb[c] * amount) as u8;
            }
        }
    }
    frame
}

/// Draw `texture` scaled into the rectangle at `(x, y)`, `w` × `h`.
fn blit(frame: &mut Frame, texture: &super::art::Texture, x: f64, y: f64, w: f64, h: f64) {
    let (x0, x1) = (x.max(0.0) as usize, (x + w).min(frame.width as f64) as usize);
    let (y0, y1) = (y.max(0.0) as usize, (y + h).min(frame.height as f64) as usize);
    for py in y0..y1 {
        let v = (((py as f64 + 0.5 - y) / h) * texture.h as f64) as usize;
        for px in x0..x1 {
            let u = (((px as f64 + 0.5 - x) / w) * texture.w as f64) as usize;
            let rgba = texture.get(u.min(texture.w - 1), v.min(texture.h - 1));
            if rgba[3] > 0 {
                frame.put(px, py, [rgba[0], rgba[1], rgba[2]]);
            }
        }
    }
}
