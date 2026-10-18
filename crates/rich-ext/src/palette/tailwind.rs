//! Tailwind CSS's colour palette: 22 palettes of eleven shades each, from
//! `c50` (the lightest) to `c950` (the darkest).
//!
//! The values are Tailwind CSS 3.4's (`src/public/colors.js` at v3.4.17),
//! MIT licence, Copyright (c) Tailwind Labs, Inc. Each shade is a `#rrggbb`
//! string, so it works wherever a colour does: in markup
//! (`format!("[on {}]", SLATE.c900)`), in [`Style::parse`](rich::Style::parse),
//! in [`Color::parse`](rich::Color::parse), and in a theme.
//!
//! ```
//! use rich::Style;
//! use rich_ext::palette::tailwind::{BLUE, SLATE};
//!
//! assert_eq!(SLATE.c950, "#020617");
//! let style = Style::parse(&format!("{} on {}", BLUE.c400, SLATE.c900)).unwrap();
//! assert_eq!(style.color().unwrap().get_truecolor().unwrap().hex(), "#60a5fa");
//! ```

/// One of Tailwind's palettes: eleven shades of a hue, as `#rrggbb`.
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
    pub c950: &'static str,
}

impl Palette {
    /// The shades, lightest first.
    pub fn shades(&self) -> [&'static str; 11] {
        [
            self.c50, self.c100, self.c200, self.c300, self.c400, self.c500, self.c600, self.c700,
            self.c800, self.c900, self.c950,
        ]
    }
}

/// The shades' names, in the order [`Palette::shades`] gives them.
pub const SHADES: [u16; 11] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];

/// Black, as Tailwind writes it.
pub const BLACK: &str = "#000000";
/// White, as Tailwind writes it.
pub const WHITE: &str = "#ffffff";

pub const SLATE: Palette = Palette {
    c50: "#f8fafc",
    c100: "#f1f5f9",
    c200: "#e2e8f0",
    c300: "#cbd5e1",
    c400: "#94a3b8",
    c500: "#64748b",
    c600: "#475569",
    c700: "#334155",
    c800: "#1e293b",
    c900: "#0f172a",
    c950: "#020617",
};
pub const GRAY: Palette = Palette {
    c50: "#f9fafb",
    c100: "#f3f4f6",
    c200: "#e5e7eb",
    c300: "#d1d5db",
    c400: "#9ca3af",
    c500: "#6b7280",
    c600: "#4b5563",
    c700: "#374151",
    c800: "#1f2937",
    c900: "#111827",
    c950: "#030712",
};
pub const ZINC: Palette = Palette {
    c50: "#fafafa",
    c100: "#f4f4f5",
    c200: "#e4e4e7",
    c300: "#d4d4d8",
    c400: "#a1a1aa",
    c500: "#71717a",
    c600: "#52525b",
    c700: "#3f3f46",
    c800: "#27272a",
    c900: "#18181b",
    c950: "#09090b",
};
pub const NEUTRAL: Palette = Palette {
    c50: "#fafafa",
    c100: "#f5f5f5",
    c200: "#e5e5e5",
    c300: "#d4d4d4",
    c400: "#a3a3a3",
    c500: "#737373",
    c600: "#525252",
    c700: "#404040",
    c800: "#262626",
    c900: "#171717",
    c950: "#0a0a0a",
};
pub const STONE: Palette = Palette {
    c50: "#fafaf9",
    c100: "#f5f5f4",
    c200: "#e7e5e4",
    c300: "#d6d3d1",
    c400: "#a8a29e",
    c500: "#78716c",
    c600: "#57534e",
    c700: "#44403c",
    c800: "#292524",
    c900: "#1c1917",
    c950: "#0c0a09",
};
pub const RED: Palette = Palette {
    c50: "#fef2f2",
    c100: "#fee2e2",
    c200: "#fecaca",
    c300: "#fca5a5",
    c400: "#f87171",
    c500: "#ef4444",
    c600: "#dc2626",
    c700: "#b91c1c",
    c800: "#991b1b",
    c900: "#7f1d1d",
    c950: "#450a0a",
};
pub const ORANGE: Palette = Palette {
    c50: "#fff7ed",
    c100: "#ffedd5",
    c200: "#fed7aa",
    c300: "#fdba74",
    c400: "#fb923c",
    c500: "#f97316",
    c600: "#ea580c",
    c700: "#c2410c",
    c800: "#9a3412",
    c900: "#7c2d12",
    c950: "#431407",
};
pub const AMBER: Palette = Palette {
    c50: "#fffbeb",
    c100: "#fef3c7",
    c200: "#fde68a",
    c300: "#fcd34d",
    c400: "#fbbf24",
    c500: "#f59e0b",
    c600: "#d97706",
    c700: "#b45309",
    c800: "#92400e",
    c900: "#78350f",
    c950: "#451a03",
};
pub const YELLOW: Palette = Palette {
    c50: "#fefce8",
    c100: "#fef9c3",
    c200: "#fef08a",
    c300: "#fde047",
    c400: "#facc15",
    c500: "#eab308",
    c600: "#ca8a04",
    c700: "#a16207",
    c800: "#854d0e",
    c900: "#713f12",
    c950: "#422006",
};
pub const LIME: Palette = Palette {
    c50: "#f7fee7",
    c100: "#ecfccb",
    c200: "#d9f99d",
    c300: "#bef264",
    c400: "#a3e635",
    c500: "#84cc16",
    c600: "#65a30d",
    c700: "#4d7c0f",
    c800: "#3f6212",
    c900: "#365314",
    c950: "#1a2e05",
};
pub const GREEN: Palette = Palette {
    c50: "#f0fdf4",
    c100: "#dcfce7",
    c200: "#bbf7d0",
    c300: "#86efac",
    c400: "#4ade80",
    c500: "#22c55e",
    c600: "#16a34a",
    c700: "#15803d",
    c800: "#166534",
    c900: "#14532d",
    c950: "#052e16",
};
pub const EMERALD: Palette = Palette {
    c50: "#ecfdf5",
    c100: "#d1fae5",
    c200: "#a7f3d0",
    c300: "#6ee7b7",
    c400: "#34d399",
    c500: "#10b981",
    c600: "#059669",
    c700: "#047857",
    c800: "#065f46",
    c900: "#064e3b",
    c950: "#022c22",
};
pub const TEAL: Palette = Palette {
    c50: "#f0fdfa",
    c100: "#ccfbf1",
    c200: "#99f6e4",
    c300: "#5eead4",
    c400: "#2dd4bf",
    c500: "#14b8a6",
    c600: "#0d9488",
    c700: "#0f766e",
    c800: "#115e59",
    c900: "#134e4a",
    c950: "#042f2e",
};
pub const CYAN: Palette = Palette {
    c50: "#ecfeff",
    c100: "#cffafe",
    c200: "#a5f3fc",
    c300: "#67e8f9",
    c400: "#22d3ee",
    c500: "#06b6d4",
    c600: "#0891b2",
    c700: "#0e7490",
    c800: "#155e75",
    c900: "#164e63",
    c950: "#083344",
};
pub const SKY: Palette = Palette {
    c50: "#f0f9ff",
    c100: "#e0f2fe",
    c200: "#bae6fd",
    c300: "#7dd3fc",
    c400: "#38bdf8",
    c500: "#0ea5e9",
    c600: "#0284c7",
    c700: "#0369a1",
    c800: "#075985",
    c900: "#0c4a6e",
    c950: "#082f49",
};
pub const BLUE: Palette = Palette {
    c50: "#eff6ff",
    c100: "#dbeafe",
    c200: "#bfdbfe",
    c300: "#93c5fd",
    c400: "#60a5fa",
    c500: "#3b82f6",
    c600: "#2563eb",
    c700: "#1d4ed8",
    c800: "#1e40af",
    c900: "#1e3a8a",
    c950: "#172554",
};
pub const INDIGO: Palette = Palette {
    c50: "#eef2ff",
    c100: "#e0e7ff",
    c200: "#c7d2fe",
    c300: "#a5b4fc",
    c400: "#818cf8",
    c500: "#6366f1",
    c600: "#4f46e5",
    c700: "#4338ca",
    c800: "#3730a3",
    c900: "#312e81",
    c950: "#1e1b4b",
};
pub const VIOLET: Palette = Palette {
    c50: "#f5f3ff",
    c100: "#ede9fe",
    c200: "#ddd6fe",
    c300: "#c4b5fd",
    c400: "#a78bfa",
    c500: "#8b5cf6",
    c600: "#7c3aed",
    c700: "#6d28d9",
    c800: "#5b21b6",
    c900: "#4c1d95",
    c950: "#2e1065",
};
pub const PURPLE: Palette = Palette {
    c50: "#faf5ff",
    c100: "#f3e8ff",
    c200: "#e9d5ff",
    c300: "#d8b4fe",
    c400: "#c084fc",
    c500: "#a855f7",
    c600: "#9333ea",
    c700: "#7e22ce",
    c800: "#6b21a8",
    c900: "#581c87",
    c950: "#3b0764",
};
pub const FUCHSIA: Palette = Palette {
    c50: "#fdf4ff",
    c100: "#fae8ff",
    c200: "#f5d0fe",
    c300: "#f0abfc",
    c400: "#e879f9",
    c500: "#d946ef",
    c600: "#c026d3",
    c700: "#a21caf",
    c800: "#86198f",
    c900: "#701a75",
    c950: "#4a044e",
};
pub const PINK: Palette = Palette {
    c50: "#fdf2f8",
    c100: "#fce7f3",
    c200: "#fbcfe8",
    c300: "#f9a8d4",
    c400: "#f472b6",
    c500: "#ec4899",
    c600: "#db2777",
    c700: "#be185d",
    c800: "#9d174d",
    c900: "#831843",
    c950: "#500724",
};
pub const ROSE: Palette = Palette {
    c50: "#fff1f2",
    c100: "#ffe4e6",
    c200: "#fecdd3",
    c300: "#fda4af",
    c400: "#fb7185",
    c500: "#f43f5e",
    c600: "#e11d48",
    c700: "#be123c",
    c800: "#9f1239",
    c900: "#881337",
    c950: "#4c0519",
};

/// Every palette with its name, in Tailwind's order: the five greys, then
/// the hues round the wheel from red to rose.
pub const ALL: [(&str, Palette); 22] = [
    ("slate", SLATE),
    ("gray", GRAY),
    ("zinc", ZINC),
    ("neutral", NEUTRAL),
    ("stone", STONE),
    ("red", RED),
    ("orange", ORANGE),
    ("amber", AMBER),
    ("yellow", YELLOW),
    ("lime", LIME),
    ("green", GREEN),
    ("emerald", EMERALD),
    ("teal", TEAL),
    ("cyan", CYAN),
    ("sky", SKY),
    ("blue", BLUE),
    ("indigo", INDIGO),
    ("violet", VIOLET),
    ("purple", PURPLE),
    ("fuchsia", FUCHSIA),
    ("pink", PINK),
    ("rose", ROSE),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shade_is_a_colour() {
        assert_eq!(SLATE.c950, "#020617");
        assert_eq!(ROSE.c50, "#fff1f2");
        for (name, palette) in ALL {
            for shade in palette.shades() {
                let colour = rich::Color::parse(shade).unwrap();
                assert_eq!(colour.get_truecolor().unwrap().hex(), shade, "{name}");
            }
        }
    }
}
