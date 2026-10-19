//! The game's art, all generated here: wall, floor and ceiling textures,
//! the sprites (barrels, columns, lamps, pickups, the enemy's frames), the
//! pistol and its flash, and the status face. No image files.

use super::game::{Kind, Mode, Wall};

/// A texture's side, in pixels.
pub const TEX: usize = 64;

/// An RGBA picture; alpha 0 is transparent (sprites only).
#[derive(Clone)]
pub struct Texture {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

impl Texture {
    fn new(w: usize, h: usize) -> Texture {
        Texture {
            w,
            h,
            px: vec![[0; 4]; w * h],
        }
    }

    fn from_fn(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 3]) -> Texture {
        let mut t = Texture::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = f(x, y);
                t.px[y * w + x] = [r, g, b, 255];
            }
        }
        t
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> [u8; 4] {
        self.px[(y % self.h) * self.w + (x % self.w)]
    }

    fn put(&mut self, x: i32, y: i32, rgb: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = [rgb[0], rgb[1], rgb[2], 255];
        }
    }

    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, f: impl Fn(i32, i32) -> [u8; 3]) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.put(x, y, f(x, y));
            }
        }
    }

    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, f: impl Fn(f64, f64) -> [u8; 3]) {
        for y in (cy - ry).floor() as i32..=(cy + ry).ceil() as i32 {
            for x in (cx - rx).floor() as i32..=(cx + rx).ceil() as i32 {
                let (nx, ny) = ((x as f64 + 0.5 - cx) / rx, (y as f64 + 0.5 - cy) / ry);
                if nx * nx + ny * ny <= 1.0 {
                    self.put(x, y, f(nx, ny));
                }
            }
        }
    }
}

/// A hash of a point: the noise every texture is grained with.
fn hash(x: i64, y: i64, seed: u64) -> u64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^ (h >> 32)
}

/// Noise from -1 to 1.
fn noise(x: i64, y: i64, seed: u64) -> f64 {
    (hash(x, y, seed) % 2001) as f64 / 1000.0 - 1.0
}

/// Smooth noise from -1 to 1, `scale` pixels a cell.
fn smooth(x: f64, y: f64, scale: f64, seed: u64) -> f64 {
    let (fx, fy) = (x / scale, y / scale);
    let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = noise(x0, y0, seed) * (1.0 - tx) + noise(x0 + 1, y0, seed) * tx;
    let b = noise(x0, y0 + 1, seed) * (1.0 - tx) + noise(x0 + 1, y0 + 1, seed) * tx;
    a * (1.0 - ty) + b * ty
}

/// `rgb` lightened or darkened by `by` (-1 to 1, a share).
fn shade(rgb: [u8; 3], by: f64) -> [u8; 3] {
    rgb.map(|c| (c as f64 * (1.0 + by)).clamp(0.0, 255.0) as u8)
}

fn brick() -> Texture {
    Texture::from_fn(TEX, TEX, |x, y| {
        let row = y / 8;
        let x_off = (x + if row % 2 == 1 { 8 } else { 0 }) % TEX;
        let brick = x_off / 16;
        if y % 8 == 7 || x_off.is_multiple_of(16) {
            return shade([86, 80, 74], noise(x as i64, y as i64, 1) * 0.12);
        }
        let base = shade(
            [150, 62, 44],
            noise(brick as i64, row as i64, 2) * 0.18 + smooth(x as f64, y as f64, 6.0, 3) * 0.08,
        );
        let edge = if y % 8 == 6 {
            -0.25
        } else if y % 8 == 0 {
            0.12
        } else {
            0.0
        };
        shade(base, edge + noise(x as i64, y as i64, 4) * 0.08)
    })
}

fn stone() -> Texture {
    Texture::from_fn(TEX, TEX, |x, y| {
        let row = y / 16;
        let x_off = (x + row * 11) % TEX;
        let block = x_off / 21;
        let seam = y % 16 == 15 || x_off.is_multiple_of(21) || (x_off == 63 && block == 3);
        let base = shade(
            [118, 116, 108],
            noise(block as i64, row as i64, 5) * 0.12 + smooth(x as f64, y as f64, 9.0, 6) * 0.15,
        );
        if seam {
            return shade(base, -0.45);
        }
        let crack = smooth(x as f64, y as f64, 5.0, 7).abs() < 0.03;
        shade(
            base,
            noise(x as i64, y as i64, 8) * 0.07 + if crack { -0.3 } else { 0.0 },
        )
    })
}

fn wood() -> Texture {
    Texture::from_fn(TEX, TEX, |x, y| {
        let plank = x / 16;
        if x % 16 == 0 {
            return [40, 26, 14];
        }
        let grain = ((x as f64 * 0.9 + smooth(x as f64, y as f64, 12.0, 9 + plank as u64) * 9.0)
            .sin()
            * 0.5
            + 0.5)
            * 0.18;
        let nail = (y % 32 == 4 || y % 32 == 27) && (x % 16 == 3 || x % 16 == 13);
        if nail {
            return [170, 170, 160];
        }
        shade(
            [128, 84, 46],
            grain - 0.09 + noise(plank as i64, 0, 10) * 0.12 + noise(x as i64, y as i64, 11) * 0.04,
        )
    })
}

/// A 3x5 font for the signs.
fn glyph(c: char) -> [u8; 5] {
    match c {
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        _ => [0; 5],
    }
}

fn tech_at(x: usize, y: usize) -> [u8; 3] {
    let (px, py) = (x % 32, y % 32);
    let base = shade([72, 82, 98], smooth(x as f64, y as f64, 10.0, 12) * 0.08);
    if px == 0 || py == 0 {
        return shade(base, 0.35);
    }
    if px == 31 || py == 31 {
        return shade(base, -0.45);
    }
    if (px == 3 || px == 28) && (py == 3 || py == 28) {
        return [190, 196, 205];
    }
    let panel = (x / 32 + y / 32) % 2;
    if panel == 0 && (12..15).contains(&py) && (5..27).contains(&px) {
        return if py == 13 {
            [140, 240, 255]
        } else {
            [60, 170, 200]
        };
    }
    if panel == 1 && (8..24).contains(&py) && (8..24).contains(&px) && py % 3 == 0 {
        return shade(base, -0.5);
    }
    shade(base, noise(x as i64, y as i64, 13) * 0.05)
}

fn tech() -> Texture {
    Texture::from_fn(TEX, TEX, tech_at)
}

fn exit() -> Texture {
    let mut t = Texture::from_fn(TEX, TEX, tech_at);
    // The sign: a dark box with EXIT in red.
    t.rect(14, 8, 50, 20, |_, _| [24, 20, 22]);
    for (i, c) in "EXIT".chars().enumerate() {
        let rows = glyph(c);
        for (gy, bits) in rows.iter().enumerate() {
            for gx in 0..3 {
                if bits & (0b100 >> gx) != 0 {
                    let (x, y) = (17 + i as i32 * 8 + gx * 2, 10 + gy as i32 * 2);
                    t.rect(x, y, x + 2, y + 2, |_, _| [255, 48, 40]);
                }
            }
        }
    }
    // The switch: a plate and a lever.
    t.rect(24, 32, 40, 54, |x, y| {
        if x == 24 || y == 32 {
            [150, 150, 150]
        } else {
            [96, 96, 100]
        }
    });
    t.rect(30, 36, 34, 46, |_, _| [40, 220, 60]);
    t.rect(29, 44, 35, 50, |_, _| [200, 200, 205]);
    t
}

fn door() -> Texture {
    Texture::from_fn(TEX, TEX, |x, y| {
        if (28..36).contains(&y) {
            // A hazard stripe across the middle.
            return if (x + y) / 4 % 2 == 0 {
                [230, 190, 30]
            } else {
                [30, 28, 26]
            };
        }
        let base = [104, 106, 116];
        let rib = match y % 8 {
            0 => 0.25,
            7 => -0.35,
            _ => 0.0,
        };
        let edge = if !(2..=61).contains(&x) { -0.4 } else { 0.0 };
        shade(base, rib + edge + noise(x as i64, y as i64, 14) * 0.05)
    })
}

fn frame() -> Texture {
    Texture::from_fn(TEX, TEX, |x, y| {
        let stripe = (24..40).contains(&x);
        let base = if stripe { [180, 150, 40] } else { [60, 60, 66] };
        shade(
            base,
            noise(x as i64, y as i64, 15) * 0.06 - if x % 16 == 0 { 0.3 } else { 0.0 },
        )
    })
}

fn floor() -> Texture {
    Texture::from_fn(TEX, TEX, |x, y| {
        let tile = (x / 16, y / 16);
        if x % 16 == 0 || y % 16 == 0 {
            return [48, 44, 40];
        }
        shade(
            [92, 86, 76],
            noise(tile.0 as i64, tile.1 as i64, 16) * 0.1
                + smooth(x as f64, y as f64, 7.0, 17) * 0.1
                + noise(x as i64, y as i64, 18) * 0.05,
        )
    })
}

fn ceiling() -> Texture {
    Texture::from_fn(TEX, TEX, |x, y| {
        let (px, py) = (x % 32, y % 32);
        let light = (x / 32 + y / 32) % 2 == 0 && (10..22).contains(&px) && (10..22).contains(&py);
        if light {
            return [225, 222, 200];
        }
        if px == 0 || py == 0 {
            return [28, 28, 32];
        }
        shade([58, 58, 64], noise(x as i64, y as i64, 19) * 0.06)
    })
}

/// The wall textures, by material.
pub struct Textures {
    pub walls: Vec<(Wall, Texture)>,
    pub floor: Texture,
    pub ceiling: Texture,
    pub sprites: Vec<(Kind, Texture)>,
    /// The enemy: standing, two walking frames, firing, hurt, and three of
    /// falling down.
    pub enemy: Vec<Texture>,
    pub pistol: Texture,
    pub flash: Texture,
}

impl Textures {
    pub fn generate() -> Textures {
        Textures {
            walls: vec![
                (Wall::Brick, brick()),
                (Wall::Stone, stone()),
                (Wall::Wood, wood()),
                (Wall::Tech, tech()),
                (Wall::Exit, exit()),
                (Wall::Door, door()),
                (Wall::Frame, frame()),
            ],
            floor: floor(),
            ceiling: ceiling(),
            sprites: vec![
                (Kind::Barrel, barrel()),
                (Kind::Column, column()),
                (Kind::Lamp, lamp()),
                (Kind::Ammo, ammo()),
                (Kind::Health, medikit()),
                (Kind::Armour, armour()),
            ],
            enemy: (0..8).map(enemy).collect(),
            pistol: pistol(),
            flash: flash(),
        }
    }

    pub fn wall(&self, wall: Wall) -> &Texture {
        &self
            .walls
            .iter()
            .find(|(w, _)| *w == wall)
            .expect("every wall has a texture")
            .1
    }

    /// A thing's picture now.
    pub fn sprite(&self, kind: Kind, mode: Mode, steps: u32) -> &Texture {
        if kind == Kind::Enemy {
            let frame = match mode {
                Mode::Idle => 0,
                Mode::Chase => 1 + (steps / 8 % 2) as usize,
                Mode::Attack(_) => 3,
                Mode::Pain(_) => 4,
                Mode::Dead(n) => 5 + (n / 6).min(2) as usize,
            };
            return &self.enemy[frame];
        }
        &self
            .sprites
            .iter()
            .find(|(k, _)| *k == kind)
            .expect("every thing has a sprite")
            .1
    }
}

fn barrel() -> Texture {
    let mut t = Texture::new(TEX, TEX);
    t.rect(18, 24, 46, 64, |x, y| {
        let across = (x as f64 - 31.5) / 14.0;
        let light = (1.0 - across * across).max(0.0).sqrt() - 0.25 * across;
        let band = matches!(y, 30 | 31 | 46 | 47 | 61 | 62);
        let base = if band { [90, 70, 50] } else { [168, 96, 38] };
        shade(
            base,
            light * 0.5 - 0.45 + noise(i64::from(x), i64::from(y), 20) * 0.08,
        )
    });
    t.ellipse(32.0, 24.5, 14.0, 3.0, |nx, _| {
        shade([120, 82, 40], -nx * 0.2)
    });
    t.ellipse(32.0, 24.5, 9.0, 1.8, |_, _| [70, 200, 60]);
    t
}

fn column() -> Texture {
    let mut t = Texture::new(TEX, TEX);
    let stone = |x: i32, y: i32, half: f64| {
        let across = (x as f64 - 31.5) / half;
        let light = (1.0 - across * across).max(0.0).sqrt() - 0.3 * across;
        shade(
            [150, 146, 136],
            light * 0.45 - 0.4 + noise(i64::from(x), i64::from(y), 21) * 0.07,
        )
    };
    t.rect(22, 0, 42, 64, |x, y| stone(x, y, 10.0));
    t.rect(17, 0, 47, 6, |x, y| stone(x, y, 15.0));
    t.rect(17, 58, 47, 64, |x, y| stone(x, y, 15.0));
    for x in [26, 31, 36] {
        t.rect(x, 6, x + 1, 58, |x, y| shade(stone(x, y, 10.0), -0.3));
    }
    t
}

fn lamp() -> Texture {
    let mut t = Texture::new(TEX, TEX);
    t.rect(31, 0, 33, 14, |_, _| [40, 40, 40]);
    for y in 14..22 {
        let half = 3 + (y - 14);
        t.rect(32 - half, y, 32 + half, y + 1, |x, _| {
            shade([40, 90, 60], -(x as f64 - 32.0).abs() / 30.0)
        });
    }
    t.ellipse(32.0, 23.0, 7.0, 2.5, |_, ny| {
        shade([255, 244, 190], -ny * 0.1)
    });
    t
}

fn ammo() -> Texture {
    let mut t = Texture::new(TEX, TEX);
    t.rect(22, 54, 42, 64, |x, y| {
        let edge = if y == 54 || x == 22 { 0.25 } else { 0.0 };
        shade(
            [92, 100, 52],
            edge + noise(i64::from(x), i64::from(y), 22) * 0.06,
        )
    });
    for i in 0..4 {
        let x = 24 + i * 5;
        t.rect(x, 48, x + 3, 54, |_, y| {
            if y < 50 {
                [200, 140, 60]
            } else {
                [220, 180, 70]
            }
        });
    }
    t
}

fn medikit() -> Texture {
    let mut t = Texture::new(TEX, TEX);
    t.rect(20, 48, 44, 64, |x, y| {
        let edge = if y == 48 || x == 20 { 0.0 } else { -0.08 };
        shade(
            [232, 232, 228],
            edge + noise(i64::from(x), i64::from(y), 23) * 0.04,
        )
    });
    t.rect(30, 50, 34, 62, |_, _| [210, 30, 30]);
    t.rect(26, 54, 38, 58, |_, _| [210, 30, 30]);
    t
}

fn armour() -> Texture {
    let mut t = Texture::new(TEX, TEX);
    for y in 40..64 {
        let half = if y < 46 { 6 + (y - 40) } else { 12 };
        t.rect(32 - half, y, 32 + half, y + 1, |x, y| {
            let neck = y < 48 && (x - 32).abs() < 4;
            if neck {
                [0, 0, 0]
            } else {
                shade(
                    [60, 150, 70],
                    -(x as f64 - 32.0).abs() / 40.0 + noise(i64::from(x), i64::from(y), 24) * 0.08,
                )
            }
        });
    }
    // The neck hole is cut out (transparent).
    for y in 40..48 {
        for x in 29..35 {
            t.px[y * TEX + x] = [0; 4];
        }
    }
    t
}

/// The enemy's frame `n`: 0 standing, 1–2 walking, 3 firing, 4 hurt, 5–7
/// falling down. A hooded sentry with glowing eyes.
fn enemy(n: usize) -> Texture {
    let mut t = Texture::new(TEX, TEX);
    let robe = |x: i32, y: i32| {
        shade(
            [92, 50, 120],
            -(x as f64 - 32.0).abs() / 28.0 + noise(i64::from(x), i64::from(y), 25) * 0.06,
        )
    };
    if n >= 5 {
        // Falling: lower and wider, then a heap in a puddle.
        let drop = (n - 5) as i32;
        t.ellipse(32.0, 61.0, 10.0 + 4.0 * drop as f64, 2.5, |_, _| {
            [120, 10, 10]
        });
        let top = 30 + drop * 11;
        t.rect(22 - drop * 3, top, 42 + drop * 3, 61, robe);
        if drop < 2 {
            t.ellipse(32.0, top as f64 - 4.0, 6.0, 5.0, |_, _| [150, 160, 130]);
        }
        return t;
    }
    // Legs: apart in the walking frames.
    let stride = match n {
        1 => 3,
        2 => -3,
        _ => 0,
    };
    t.rect(25 + stride, 44, 30 + stride, 63, |_, _| [50, 40, 46]);
    t.rect(34 - stride, 44, 39 - stride, 63, |_, _| [50, 40, 46]);
    t.rect(24 + stride, 62, 31 + stride, 64, |_, _| [30, 24, 20]);
    t.rect(33 - stride, 62, 40 - stride, 64, |_, _| [30, 24, 20]);
    // The robe and its belt.
    for y in 20..46 {
        let half = 8 + (y - 20) / 4;
        t.rect(32 - half, y, 32 + half, y + 1, robe);
    }
    t.rect(22, 36, 42, 38, |_, _| [150, 120, 50]);
    // Arms; firing, the right one points a weapon out with a flash.
    if n == 3 {
        t.rect(36, 24, 52, 28, robe);
        t.rect(50, 23, 56, 27, |_, _| [60, 60, 66]);
        t.ellipse(59.0, 25.0, 4.5, 4.5, |nx, ny| {
            let d = (nx * nx + ny * ny).sqrt();
            shade([255, 220, 90], -d * 0.4)
        });
    } else {
        t.rect(18, 22, 23, 40, robe);
        t.rect(41, 22, 46, 40, robe);
    }
    t.rect(20, 40, 25, 43, |_, _| [150, 160, 130]);
    if n != 3 {
        t.rect(39, 40, 44, 43, |_, _| [150, 160, 130]);
    }
    // The hood, the face in its shadow, the eyes.
    t.ellipse(32.0, 13.0, 8.0, 8.5, |nx, ny| {
        shade([80, 42, 104], -nx * 0.15 - ny * 0.1)
    });
    t.ellipse(32.0, 14.5, 5.0, 5.0, |_, _| [26, 20, 30]);
    t.rect(29, 13, 31, 15, |_, _| [255, 70, 40]);
    t.rect(33, 13, 35, 15, |_, _| [255, 70, 40]);
    if n == 4 {
        // Hurt: washed out and red.
        for px in &mut t.px {
            if px[3] > 0 {
                *px = [255, px[1] / 2 + 60, px[2] / 2 + 60, 255];
            }
        }
    }
    t
}

/// The pistol from behind, held in a gloved hand: 48 by 40.
fn pistol() -> Texture {
    let mut t = Texture::new(48, 40);
    // The hand and cuff.
    t.rect(14, 22, 34, 40, |x, y| {
        shade(
            [196, 150, 112],
            -(x as f64 - 24.0).abs() / 30.0 + noise(i64::from(x), i64::from(y), 26) * 0.05,
        )
    });
    t.rect(12, 34, 36, 40, |_, _| [60, 64, 70]);
    // The slide and barrel.
    t.rect(18, 4, 30, 26, |x, y| {
        let light = if x < 21 {
            0.25
        } else if x > 27 {
            -0.3
        } else {
            0.0
        };
        shade(
            [82, 84, 92],
            light + noise(i64::from(x), i64::from(y), 27) * 0.04,
        )
    });
    t.rect(22, 1, 26, 5, |_, _| [40, 40, 44]);
    t.rect(23, 2, 25, 4, |_, _| [10, 10, 10]);
    t.rect(20, 10, 28, 11, |_, _| [50, 52, 58]);
    t
}

/// The muzzle flash: a star of white and yellow, 32 by 24.
fn flash() -> Texture {
    let mut t = Texture::new(32, 24);
    for y in 0..24 {
        for x in 0..32 {
            let (dx, dy) = ((x as f64 - 15.5) / 16.0, (y as f64 - 13.0) / 12.0);
            let r = (dx * dx + dy * dy).sqrt();
            let a = dy.atan2(dx);
            let spike = 0.55 + 0.4 * (a * 5.0).cos().abs();
            if r < spike {
                let heat = 1.0 - r / spike;
                let rgb = [
                    255,
                    (170.0 + 85.0 * heat) as u8,
                    (40.0 + 200.0 * heat * heat) as u8,
                ];
                t.put(x, y, rgb);
            }
        }
    }
    t
}

/// The status face, `w` by `h` pixels (RGB): how the player is doing.
pub fn face(health: i32, hurt: bool, grin: bool, look: i32) -> (usize, usize, Vec<u8>) {
    const W: usize = 10;
    const H: usize = 6;
    let skin = match health {
        h if h <= 0 => [150, 120, 110],
        h if h < 25 => [200, 120, 100],
        h if h < 60 => [214, 150, 115],
        _ => [224, 170, 130],
    };
    let mut px = vec![0u8; W * H * 3];
    let mut put = |x: usize, y: usize, rgb: [u8; 3]| {
        let i = (y * W + x) * 3;
        px[i..i + 3].copy_from_slice(&rgb);
    };
    for y in 0..H {
        for x in 0..W {
            let edge = x == 0 || x == W - 1;
            let rgb = if y == 0 || (edge && y < 3) {
                [90, 60, 30]
            } else if edge {
                [20, 20, 24]
            } else {
                skin
            };
            put(x, y, rgb);
        }
    }
    let eye = if hurt { [255, 255, 255] } else { [30, 40, 90] };
    if health <= 0 {
        put(3, 2, [60, 0, 0]);
        put(6, 2, [60, 0, 0]);
    } else {
        let look = look.clamp(-1, 1);
        put((3 + look) as usize, 2, eye);
        put((6 + look) as usize, 2, eye);
    }
    let mouth = if health <= 0 {
        [80, 10, 10]
    } else if grin {
        [250, 250, 250]
    } else {
        [120, 40, 40]
    };
    if grin {
        for x in 3..7 {
            put(x, 4, mouth);
        }
        put(2, 3, mouth);
        put(7, 3, mouth);
    } else if hurt {
        put(4, 4, mouth);
        put(5, 4, mouth);
        put(4, 3, mouth);
        put(5, 3, mouth);
    } else {
        for x in 4..6 {
            put(x, 4, mouth);
        }
    }
    if health < 50 {
        put(2, 1, [170, 20, 20]);
        put(7, 4, [170, 20, 20]);
    }
    if health < 25 {
        put(8, 2, [150, 10, 10]);
        put(1, 3, [150, 10, 10]);
    }
    (W, H, px)
}
