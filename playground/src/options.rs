//! A block's options: the `data-*` attributes of a `.rich-playground` block
//! and the controls the page shows, passed in as one JSON object.

use rich::r#box::{self, Box as BoxSet};
use rich::{HorizontalAlign, Justify, Overflow};
use serde::Deserialize;

/// The widest and narrowest console a block may ask for.
pub const MIN_WIDTH: usize = 20;
pub const MAX_WIDTH: usize = 200;
/// The width when a block names none.
pub const DEFAULT_WIDTH: usize = 80;
/// The longest input rendered, in bytes; the page caps it lower.
pub const MAX_INPUT: usize = 64 * 1024;

/// Every option a kind may read. Unknown keys are refused, so a typo in a
/// page's `data-*` attribute fails the docs check instead of being ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Options {
    pub width: Option<usize>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub caption: Option<String>,
    #[serde(rename = "box")]
    pub box_style: Option<String>,
    pub border_style: Option<String>,
    pub justify: Option<String>,
    pub overflow: Option<String>,
    pub align: Option<String>,
    pub style: Option<String>,
    pub characters: Option<String>,
    pub expand: Option<bool>,
    pub show_lines: Option<bool>,
    pub show_header: Option<bool>,
    pub show_edge: Option<bool>,
    pub padding: Option<Padding>,
    pub guide_style: Option<String>,
    pub indent: Option<usize>,
    pub sort_keys: Option<bool>,
    pub lang: Option<String>,
    pub theme: Option<String>,
    pub line_numbers: Option<bool>,
    pub word_wrap: Option<bool>,
    pub tab_size: Option<usize>,
    pub hyperlinks: Option<bool>,
    pub code_theme: Option<String>,
    pub inline_code_lexer: Option<String>,
    pub orientation: Option<String>,
    pub bar_width: Option<usize>,
    pub bins: Option<usize>,
    pub min_max: Option<bool>,
    pub threshold: Option<f64>,
    pub charset: Option<String>,
    pub layout: Option<String>,
    pub context: Option<usize>,
    pub old_title: Option<String>,
    pub new_title: Option<String>,
    pub format: Option<String>,
    pub view: Option<String>,
    pub root_label: Option<String>,
    pub show_types: Option<bool>,
}

/// Padding as upstream's `Padding.unpack` takes it: one number for every
/// side, or "top/bottom,left/right", or "top,right,bottom,left".
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Padding {
    Uniform(usize),
    Sides(String),
}

impl Padding {
    pub fn unpack(&self) -> Result<(usize, usize, usize, usize), String> {
        let parts: Vec<usize> = match self {
            Padding::Uniform(n) => vec![*n],
            Padding::Sides(text) => text
                .split(',')
                .map(|part| part.trim().parse::<usize>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("padding {text:?}: numbers, comma-separated"))?,
        };
        let clamp = |n: usize| n.min(16);
        match parts[..] {
            [n] => Ok((clamp(n), clamp(n), clamp(n), clamp(n))),
            [v, h] => Ok((clamp(v), clamp(h), clamp(v), clamp(h))),
            [t, r, b, l] => Ok((clamp(t), clamp(r), clamp(b), clamp(l))),
            _ => Err("padding takes one, two or four numbers".into()),
        }
    }
}

impl Options {
    /// Parse the page's JSON; an empty string is no options.
    pub fn parse(json: &str) -> Result<Options, String> {
        if json.trim().is_empty() {
            return Ok(Options::default());
        }
        serde_json::from_str(json).map_err(|e| format!("options: {e}"))
    }

    /// The console width, clamped to what the page can show.
    pub fn width(&self) -> usize {
        self.width
            .unwrap_or(DEFAULT_WIDTH)
            .clamp(MIN_WIDTH, MAX_WIDTH)
    }

    pub fn justify(&self) -> Result<Option<Justify>, String> {
        self.justify.as_deref().map(justify).transpose()
    }

    pub fn overflow(&self) -> Result<Option<Overflow>, String> {
        self.overflow.as_deref().map(overflow).transpose()
    }

    pub fn align(&self) -> Result<Option<HorizontalAlign>, String> {
        self.align.as_deref().map(align).transpose()
    }

    pub fn box_set(&self) -> Result<Option<BoxSet>, String> {
        self.box_style.as_deref().map(box_set).transpose()
    }
}

pub fn justify(name: &str) -> Result<Justify, String> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "default" => Justify::Default,
        "left" => Justify::Left,
        "center" | "centre" => Justify::Center,
        "right" => Justify::Right,
        "full" => Justify::Full,
        _ => return Err(format!("unknown justify {name:?}")),
    })
}

pub fn overflow(name: &str) -> Result<Overflow, String> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "fold" => Overflow::Fold,
        "crop" => Overflow::Crop,
        "ellipsis" => Overflow::Ellipsis,
        "ignore" => Overflow::Ignore,
        _ => return Err(format!("unknown overflow {name:?}")),
    })
}

pub fn align(name: &str) -> Result<HorizontalAlign, String> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "left" => HorizontalAlign::Left,
        "center" | "centre" => HorizontalAlign::Center,
        "right" => HorizontalAlign::Right,
        _ => return Err(format!("unknown align {name:?}")),
    })
}

/// The box styles by their upstream names, as `rich --box` takes them.
pub const BOX_NAMES: [&str; 20] = [
    "ASCII",
    "ASCII2",
    "ASCII_DOUBLE_HEAD",
    "SQUARE",
    "SQUARE_DOUBLE_HEAD",
    "MINIMAL",
    "MINIMAL_HEAVY_HEAD",
    "MINIMAL_DOUBLE_HEAD",
    "SIMPLE",
    "SIMPLE_HEAD",
    "SIMPLE_HEAVY",
    "HORIZONTALS",
    "ROUNDED",
    "HEAVY",
    "HEAVY_EDGE",
    "HEAVY_HEAD",
    "DOUBLE",
    "DOUBLE_EDGE",
    "MARKDOWN",
    "NONE",
];

pub fn box_set(name: &str) -> Result<BoxSet, String> {
    Ok(match name.to_ascii_uppercase().as_str() {
        "ASCII" => r#box::ASCII,
        "ASCII2" => r#box::ASCII2,
        "ASCII_DOUBLE_HEAD" => r#box::ASCII_DOUBLE_HEAD,
        "SQUARE" => r#box::SQUARE,
        "SQUARE_DOUBLE_HEAD" => r#box::SQUARE_DOUBLE_HEAD,
        "MINIMAL" => r#box::MINIMAL,
        "MINIMAL_HEAVY_HEAD" => r#box::MINIMAL_HEAVY_HEAD,
        "MINIMAL_DOUBLE_HEAD" => r#box::MINIMAL_DOUBLE_HEAD,
        "SIMPLE" => r#box::SIMPLE,
        "SIMPLE_HEAD" => r#box::SIMPLE_HEAD,
        "SIMPLE_HEAVY" => r#box::SIMPLE_HEAVY,
        "HORIZONTALS" => r#box::HORIZONTALS,
        "ROUNDED" => r#box::ROUNDED,
        "HEAVY" => r#box::HEAVY,
        "HEAVY_EDGE" => r#box::HEAVY_EDGE,
        "HEAVY_HEAD" => r#box::HEAVY_HEAD,
        "DOUBLE" => r#box::DOUBLE,
        "DOUBLE_EDGE" => r#box::DOUBLE_EDGE,
        "MARKDOWN" => r#box::MARKDOWN,
        "NONE" => r#box::NONE,
        _ => return Err(format!("unknown box {name:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_box_name_resolves() {
        for name in BOX_NAMES {
            assert!(box_set(name).is_ok(), "{name}");
        }
        assert!(box_set("nope").is_err());
    }

    #[test]
    fn unknown_options_are_refused() {
        assert!(Options::parse(r#"{"widht": 40}"#).is_err());
        assert_eq!(
            Options::parse(r#"{"width": 5}"#).unwrap().width(),
            MIN_WIDTH
        );
        assert_eq!(Options::parse("").unwrap().width(), DEFAULT_WIDTH);
    }
}
