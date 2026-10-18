//! Material Design's 2014 colour palette: 19 palettes of ten shades, from
//! `c50` (the lightest) to `c900` (the darkest), sixteen of them with four
//! accents (`a100`, `a200`, `a400`, `a700`).
//!
//! The values are Material Design Lite's (`src/_color-definitions.scss`,
//! the `mdl-1.x` branch), Apache License 2.0, Copyright 2015 Google Inc.
//! Each shade is a `#rrggbb` string, for markup, styles and themes.
//!
//! ```
//! use rich_ext::palette::material::{BLUE_GREY, RED};
//!
//! assert_eq!(RED.c500, "#f44336");
//! assert_eq!(RED.a200, "#ff5252");
//! assert_eq!(BLUE_GREY.c900, "#263238");
//! ```

/// A palette of ten shades, as `#rrggbb`: brown and the greys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    pub c50: &'static str,
    pub c100: &'static str,
    pub c200: &'static str,
    pub c300: &'static str,
    pub c400: &'static str,
    pub c500: &'static str,
    pub c600: &'static str,
    pub c700: &'static str,
    pub c800: &'static str,
    pub c900: &'static str,
}

/// A palette of ten shades and four brighter accents, as `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AccentPalette {
    pub c50: &'static str,
    pub c100: &'static str,
    pub c200: &'static str,
    pub c300: &'static str,
    pub c400: &'static str,
    pub c500: &'static str,
    pub c600: &'static str,
    pub c700: &'static str,
    pub c800: &'static str,
    pub c900: &'static str,
    pub a100: &'static str,
    pub a200: &'static str,
    pub a400: &'static str,
    pub a700: &'static str,
}

impl Palette {
    /// The shades, lightest first.
    pub fn shades(&self) -> [&'static str; 10] {
        [
            self.c50, self.c100, self.c200, self.c300, self.c400, self.c500, self.c600, self.c700,
            self.c800, self.c900,
        ]
    }
}

impl AccentPalette {
    /// The shades, lightest first.
    pub fn shades(&self) -> [&'static str; 10] {
        [
            self.c50, self.c100, self.c200, self.c300, self.c400, self.c500, self.c600, self.c700,
            self.c800, self.c900,
        ]
    }

    /// The accents: `a100`, `a200`, `a400`, `a700`.
    pub fn accents(&self) -> [&'static str; 4] {
        [self.a100, self.a200, self.a400, self.a700]
    }
}

/// Black.
pub const BLACK: &str = "#000000";
/// White.
pub const WHITE: &str = "#ffffff";

pub const RED: AccentPalette = AccentPalette {
    c50: "#ffebee",
    c100: "#ffcdd2",
    c200: "#ef9a9a",
    c300: "#e57373",
    c400: "#ef5350",
    c500: "#f44336",
    c600: "#e53935",
    c700: "#d32f2f",
    c800: "#c62828",
    c900: "#b71c1c",
    a100: "#ff8a80",
    a200: "#ff5252",
    a400: "#ff1744",
    a700: "#d50000",
};
pub const PINK: AccentPalette = AccentPalette {
    c50: "#fce4ec",
    c100: "#f8bbd0",
    c200: "#f48fb1",
    c300: "#f06292",
    c400: "#ec407a",
    c500: "#e91e63",
    c600: "#d81b60",
    c700: "#c2185b",
    c800: "#ad1457",
    c900: "#880e4f",
    a100: "#ff80ab",
    a200: "#ff4081",
    a400: "#f50057",
    a700: "#c51162",
};
pub const PURPLE: AccentPalette = AccentPalette {
    c50: "#f3e5f5",
    c100: "#e1bee7",
    c200: "#ce93d8",
    c300: "#ba68c8",
    c400: "#ab47bc",
    c500: "#9c27b0",
    c600: "#8e24aa",
    c700: "#7b1fa2",
    c800: "#6a1b9a",
    c900: "#4a148c",
    a100: "#ea80fc",
    a200: "#e040fb",
    a400: "#d500f9",
    a700: "#aa00ff",
};
pub const DEEP_PURPLE: AccentPalette = AccentPalette {
    c50: "#ede7f6",
    c100: "#d1c4e9",
    c200: "#b39ddb",
    c300: "#9575cd",
    c400: "#7e57c2",
    c500: "#673ab7",
    c600: "#5e35b1",
    c700: "#512da8",
    c800: "#4527a0",
    c900: "#311b92",
    a100: "#b388ff",
    a200: "#7c4dff",
    a400: "#651fff",
    a700: "#6200ea",
};
pub const INDIGO: AccentPalette = AccentPalette {
    c50: "#e8eaf6",
    c100: "#c5cae9",
    c200: "#9fa8da",
    c300: "#7986cb",
    c400: "#5c6bc0",
    c500: "#3f51b5",
    c600: "#3949ab",
    c700: "#303f9f",
    c800: "#283593",
    c900: "#1a237e",
    a100: "#8c9eff",
    a200: "#536dfe",
    a400: "#3d5afe",
    a700: "#304ffe",
};
pub const BLUE: AccentPalette = AccentPalette {
    c50: "#e3f2fd",
    c100: "#bbdefb",
    c200: "#90caf9",
    c300: "#64b5f6",
    c400: "#42a5f5",
    c500: "#2196f3",
    c600: "#1e88e5",
    c700: "#1976d2",
    c800: "#1565c0",
    c900: "#0d47a1",
    a100: "#82b1ff",
    a200: "#448aff",
    a400: "#2979ff",
    a700: "#2962ff",
};
pub const LIGHT_BLUE: AccentPalette = AccentPalette {
    c50: "#e1f5fe",
    c100: "#b3e5fc",
    c200: "#81d4fa",
    c300: "#4fc3f7",
    c400: "#29b6f6",
    c500: "#03a9f4",
    c600: "#039be5",
    c700: "#0288d1",
    c800: "#0277bd",
    c900: "#01579b",
    a100: "#80d8ff",
    a200: "#40c4ff",
    a400: "#00b0ff",
    a700: "#0091ea",
};
pub const CYAN: AccentPalette = AccentPalette {
    c50: "#e0f7fa",
    c100: "#b2ebf2",
    c200: "#80deea",
    c300: "#4dd0e1",
    c400: "#26c6da",
    c500: "#00bcd4",
    c600: "#00acc1",
    c700: "#0097a7",
    c800: "#00838f",
    c900: "#006064",
    a100: "#84ffff",
    a200: "#18ffff",
    a400: "#00e5ff",
    a700: "#00b8d4",
};
pub const TEAL: AccentPalette = AccentPalette {
    c50: "#e0f2f1",
    c100: "#b2dfdb",
    c200: "#80cbc4",
    c300: "#4db6ac",
    c400: "#26a69a",
    c500: "#009688",
    c600: "#00897b",
    c700: "#00796b",
    c800: "#00695c",
    c900: "#004d40",
    a100: "#a7ffeb",
    a200: "#64ffda",
    a400: "#1de9b6",
    a700: "#00bfa5",
};
pub const GREEN: AccentPalette = AccentPalette {
    c50: "#e8f5e9",
    c100: "#c8e6c9",
    c200: "#a5d6a7",
    c300: "#81c784",
    c400: "#66bb6a",
    c500: "#4caf50",
    c600: "#43a047",
    c700: "#388e3c",
    c800: "#2e7d32",
    c900: "#1b5e20",
    a100: "#b9f6ca",
    a200: "#69f0ae",
    a400: "#00e676",
    a700: "#00c853",
};
pub const LIGHT_GREEN: AccentPalette = AccentPalette {
    c50: "#f1f8e9",
    c100: "#dcedc8",
    c200: "#c5e1a5",
    c300: "#aed581",
    c400: "#9ccc65",
    c500: "#8bc34a",
    c600: "#7cb342",
    c700: "#689f38",
    c800: "#558b2f",
    c900: "#33691e",
    a100: "#ccff90",
    a200: "#b2ff59",
    a400: "#76ff03",
    a700: "#64dd17",
};
pub const LIME: AccentPalette = AccentPalette {
    c50: "#f9fbe7",
    c100: "#f0f4c3",
    c200: "#e6ee9c",
    c300: "#dce775",
    c400: "#d4e157",
    c500: "#cddc39",
    c600: "#c0ca33",
    c700: "#afb42b",
    c800: "#9e9d24",
    c900: "#827717",
    a100: "#f4ff81",
    a200: "#eeff41",
    a400: "#c6ff00",
    a700: "#aeea00",
};
pub const YELLOW: AccentPalette = AccentPalette {
    c50: "#fffde7",
    c100: "#fff9c4",
    c200: "#fff59d",
    c300: "#fff176",
    c400: "#ffee58",
    c500: "#ffeb3b",
    c600: "#fdd835",
    c700: "#fbc02d",
    c800: "#f9a825",
    c900: "#f57f17",
    a100: "#ffff8d",
    a200: "#ffff00",
    a400: "#ffea00",
    a700: "#ffd600",
};
pub const AMBER: AccentPalette = AccentPalette {
    c50: "#fff8e1",
    c100: "#ffecb3",
    c200: "#ffe082",
    c300: "#ffd54f",
    c400: "#ffca28",
    c500: "#ffc107",
    c600: "#ffb300",
    c700: "#ffa000",
    c800: "#ff8f00",
    c900: "#ff6f00",
    a100: "#ffe57f",
    a200: "#ffd740",
    a400: "#ffc400",
    a700: "#ffab00",
};
pub const ORANGE: AccentPalette = AccentPalette {
    c50: "#fff3e0",
    c100: "#ffe0b2",
    c200: "#ffcc80",
    c300: "#ffb74d",
    c400: "#ffa726",
    c500: "#ff9800",
    c600: "#fb8c00",
    c700: "#f57c00",
    c800: "#ef6c00",
    c900: "#e65100",
    a100: "#ffd180",
    a200: "#ffab40",
    a400: "#ff9100",
    a700: "#ff6d00",
};
pub const DEEP_ORANGE: AccentPalette = AccentPalette {
    c50: "#fbe9e7",
    c100: "#ffccbc",
    c200: "#ffab91",
    c300: "#ff8a65",
    c400: "#ff7043",
    c500: "#ff5722",
    c600: "#f4511e",
    c700: "#e64a19",
    c800: "#d84315",
    c900: "#bf360c",
    a100: "#ff9e80",
    a200: "#ff6e40",
    a400: "#ff3d00",
    a700: "#dd2c00",
};
pub const BROWN: Palette = Palette {
    c50: "#efebe9",
    c100: "#d7ccc8",
    c200: "#bcaaa4",
    c300: "#a1887f",
    c400: "#8d6e63",
    c500: "#795548",
    c600: "#6d4c41",
    c700: "#5d4037",
    c800: "#4e342e",
    c900: "#3e2723",
};
pub const GREY: Palette = Palette {
    c50: "#fafafa",
    c100: "#f5f5f5",
    c200: "#eeeeee",
    c300: "#e0e0e0",
    c400: "#bdbdbd",
    c500: "#9e9e9e",
    c600: "#757575",
    c700: "#616161",
    c800: "#424242",
    c900: "#212121",
};
pub const BLUE_GREY: Palette = Palette {
    c50: "#eceff1",
    c100: "#cfd8dc",
    c200: "#b0bec5",
    c300: "#90a4ae",
    c400: "#78909c",
    c500: "#607d8b",
    c600: "#546e7a",
    c700: "#455a64",
    c800: "#37474f",
    c900: "#263238",
};

/// Every palette with accents, with its name, in Material's order.
pub const ACCENTED: [(&str, AccentPalette); 16] = [
    ("red", RED),
    ("pink", PINK),
    ("purple", PURPLE),
    ("deep purple", DEEP_PURPLE),
    ("indigo", INDIGO),
    ("blue", BLUE),
    ("light blue", LIGHT_BLUE),
    ("cyan", CYAN),
    ("teal", TEAL),
    ("green", GREEN),
    ("light green", LIGHT_GREEN),
    ("lime", LIME),
    ("yellow", YELLOW),
    ("amber", AMBER),
    ("orange", ORANGE),
    ("deep orange", DEEP_ORANGE),
];

/// The palettes without accents, with their names.
pub const PLAIN: [(&str, Palette); 3] =
    [("brown", BROWN), ("grey", GREY), ("blue grey", BLUE_GREY)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shade_is_a_colour() {
        let check = |name: &str, shade: &str| {
            let colour = rich::Color::parse(shade).unwrap();
            assert_eq!(colour.get_truecolor().unwrap().hex(), shade, "{name}");
        };
        for (name, palette) in ACCENTED {
            for shade in palette.shades().into_iter().chain(palette.accents()) {
                check(name, shade);
            }
        }
        for (name, palette) in PLAIN {
            for shade in palette.shades() {
                check(name, shade);
            }
        }
        assert_eq!(DEEP_ORANGE.a700, "#dd2c00");
        assert_eq!(GREY.c500, "#9e9e9e");
    }
}
