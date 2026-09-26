//! Oracle for crates/rich-py/tests/test_art.py and test_mermaid.py: reads
//! a JSON list of cases on stdin, renders each with the Rust crates, and
//! prints a JSON object of results.

use std::sync::Mutex;

use rich::color::ColorSystem;
use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;
use rich_art::image::{DynamicImage, RgbaImage};
use rich_art::*;
use serde_json::{json, Map, Value};

const ASSETS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../rich-art/examples/assets"
);

fn pixel(pattern: &str, x: u32, y: u32, w: u32, h: u32) -> [u8; 4] {
    let r = if w > 1 { x * 255 / (w - 1) } else { 0 };
    let g = if h > 1 { y * 255 / (h - 1) } else { 0 };
    let b = (x * y * 7 + 13) % 256;
    let mut p = [r as u8, g as u8, b as u8, 255];
    match pattern {
        "gradient" => {}
        "alpha" => {
            p[3] = if w + h > 2 {
                ((x + y) * 255 / (w + h - 2)) as u8
            } else {
                255
            }
        }
        "spot" => {
            if x >= w / 4 && x < w / 2 && y >= h / 4 && y < h / 2 {
                p = [255, 0, 0, 255];
            }
        }
        other => panic!("unknown pattern {other}"),
    }
    p
}

fn image(spec: &Value) -> DynamicImage {
    let w = spec["w"].as_u64().unwrap() as u32;
    let h = spec["h"].as_u64().unwrap() as u32;
    let pattern = spec["pattern"].as_str().unwrap_or("gradient").to_string();
    DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
        rich_art::image::Rgba(pixel(&pattern, x, y, w, h))
    }))
}

fn console(spec: &Value) -> Console {
    let color = spec["color"].as_bool().unwrap_or(false);
    Console::builder()
        .force_terminal(spec["terminal"].as_bool().unwrap_or(color))
        .color_system(if color {
            Some(ColorSystem::Truecolor)
        } else {
            None
        })
        .width(spec["width"].as_u64().unwrap_or(40) as usize)
        .no_color(false)
        .build()
}

fn print(console: &Console, r: &dyn Renderable) -> String {
    console.capture(|c| c.print(r))
}

fn opt<'a>(opts: &'a Value, key: &str) -> Option<&'a Value> {
    opts.get(key).filter(|v| !v.is_null())
}
fn ostr<'a>(opts: &'a Value, key: &str) -> Option<&'a str> {
    opt(opts, key).and_then(Value::as_str)
}
fn ousize(opts: &Value, key: &str) -> Option<usize> {
    opt(opts, key).and_then(Value::as_u64).map(|v| v as usize)
}
fn obool(opts: &Value, key: &str) -> bool {
    opt(opts, key).and_then(Value::as_bool).unwrap_or(false)
}
fn of32(opts: &Value, key: &str, default: f32) -> f32 {
    opt(opts, key)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(default)
}

fn mode(name: &str) -> ImageMode {
    match name {
        "auto" => ImageMode::Auto,
        "ascii" => ImageMode::Ascii,
        "blocks" => ImageMode::Blocks,
        "braille" => ImageMode::Braille,
        "quadrants" => ImageMode::Quadrants,
        "sixel" => ImageMode::Sixel,
        o => panic!("{o}"),
    }
}
fn color_mode(name: Option<&str>) -> ImageColorMode {
    match name.unwrap_or("truecolor") {
        "truecolor" => ImageColorMode::TrueColor,
        "ansi256" => ImageColorMode::Ansi256,
        "ansi16" => ImageColorMode::Ansi16,
        "grayscale" => ImageColorMode::Grayscale,
        o => panic!("{o}"),
    }
}
fn dither(name: Option<&str>) -> Dither {
    match name.unwrap_or("none") {
        "none" => Dither::None,
        "floyd-steinberg" => Dither::FloydSteinberg,
        "bayer4x4" => Dither::Bayer4x4,
        "atkinson" => Dither::Atkinson,
        o => panic!("{o}"),
    }
}
fn distance(name: Option<&str>) -> ColorDistance {
    match name.unwrap_or("rgb") {
        "rgb" => ColorDistance::Rgb,
        "oklab" => ColorDistance::Oklab,
        o => panic!("{o}"),
    }
}

struct Strict {
    art: ImageArt,
    error: Mutex<Option<String>>,
}
impl Renderable for Strict {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        match self.art.render(console, options) {
            Ok(s) => s,
            Err(e) => {
                *self.error.lock().unwrap() = Some(e.to_string());
                Vec::new()
            }
        }
    }
}

fn image_art(case: &Value) -> ImageArt {
    let o = &case["opts"];
    let mut art = ImageArt::new(image(&case["image"]))
        .mode(mode(ostr(o, "mode").unwrap_or("auto")))
        .color(obool(o, "color"))
        .color_mode(color_mode(ostr(o, "color_mode")))
        .dither(dither(ostr(o, "dither")))
        .color_distance(distance(ostr(o, "color_distance")))
        .transforms(ImageTransforms {
            rotation: match ousize(o, "rotate").unwrap_or(0) {
                0 => Rotation::None,
                90 => Rotation::Clockwise90,
                180 => Rotation::Clockwise180,
                270 => Rotation::Clockwise270,
                r => panic!("{r}"),
            },
            flip_horizontal: obool(o, "flip_horizontal"),
            flip_vertical: obool(o, "flip_vertical"),
            grayscale: obool(o, "grayscale"),
            brightness: of32(o, "brightness", 1.0),
            contrast: of32(o, "contrast", 1.0),
            gamma: of32(o, "gamma", 1.0),
        });
    if let Some(w) = ousize(o, "width") {
        art = art.width(w);
    }
    if let Some(h) = ousize(o, "height") {
        art = art.height(h);
    }
    if let Some(fit) = ostr(o, "fit") {
        art = art.fit(match fit {
            "contain" => ImageFit::Contain,
            "cover" => ImageFit::Cover,
            "stretch" => ImageFit::Stretch,
            "native" => ImageFit::Native,
            f => panic!("{f}"),
        });
    }
    if let Some(anchor) = ostr(o, "anchor") {
        art = art.anchor(match anchor {
            "center" => ImageAnchor::Center,
            "top" => ImageAnchor::Top,
            "bottom" => ImageAnchor::Bottom,
            "left" => ImageAnchor::Left,
            "right" => ImageAnchor::Right,
            "top-left" => ImageAnchor::TopLeft,
            "top-right" => ImageAnchor::TopRight,
            "bottom-left" => ImageAnchor::BottomLeft,
            "bottom-right" => ImageAnchor::BottomRight,
            a => panic!("{a}"),
        });
    }
    if let Some(bg) = ostr(o, "background") {
        art = art.background_mode(match bg {
            "default" => ImageBackground::TerminalDefault,
            "checkerboard" => ImageBackground::Checkerboard,
            hex => {
                let h = hex.trim_start_matches('#');
                let c = |i: usize| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap();
                ImageBackground::Color([c(0), c(1), c(2)])
            }
        });
    }
    if let Some(w) = ousize(o, "max_width") {
        art = art.max_width(w);
    }
    if let Some(h) = ousize(o, "max_height") {
        art = art.max_height(h);
    }
    art
}

fn text_result(out: String) -> Value {
    json!({ "out": out })
}

fn gif(o: &Value) -> AnimatedArt {
    let bytes = std::fs::read(format!("{ASSETS}/{}", ostr(o, "asset").unwrap())).unwrap();
    let mut art = AnimatedArt::from_bytes(&bytes)
        .unwrap()
        .invert(obool(o, "invert"))
        .color(obool(o, "color"))
        .blocks(obool(o, "blocks"))
        .color_mode(color_mode(ostr(o, "color_mode")))
        .dither(dither(ostr(o, "dither")))
        .color_distance(distance(ostr(o, "color_distance")));
    if let Some(w) = ousize(o, "width") {
        art = art.width(w);
    }
    if let Some(h) = ousize(o, "height") {
        art = art.height(h);
    }
    if let Some(r) = ostr(o, "ramp") {
        art = art.ramp(r);
    }
    if let Some(fps) = opt(o, "max_fps").and_then(Value::as_f64) {
        art = art.max_fps(fps);
    }
    art
}

fn flowchart_json(chart: &rich_mermaid::Flowchart) -> Value {
    use rich_mermaid::flowchart::{Direction, Head, Stroke};
    let head = |h: Head| match h {
        Head::None => Value::Null,
        Head::Arrow => json!("arrow"),
        Head::Circle => json!("circle"),
        Head::Cross => json!("cross"),
    };
    json!({
        "direction": match chart.direction {
            Direction::TopDown => "TD", Direction::BottomUp => "BT",
            Direction::LeftRight => "LR", Direction::RightLeft => "RL",
        },
        "nodes": chart.nodes.iter().map(|n| json!([n.id, n.label, format!("{:?}", n.shape)])).collect::<Vec<_>>(),
        "edges": chart.edges.iter().map(|e| json!({
            "source": e.from, "target": e.to, "label": e.label,
            "stroke": match e.stroke { Stroke::Solid => "solid", Stroke::Thick => "thick", Stroke::Dotted => "dotted", Stroke::Invisible => "invisible" },
            "start": head(e.start), "end": head(e.end), "length": e.length,
        })).collect::<Vec<_>>(),
        "notes": chart.notes,
    })
}

fn parse_error_json(e: &rich_mermaid::ParseError) -> Value {
    use rich_mermaid::ParseError as P;
    let (kind, line) = match e {
        P::Empty => ("empty", Value::Null),
        P::Unsupported(_) => ("unsupported", Value::Null),
        P::TooLarge(_) => ("too_large", Value::Null),
        P::Syntax { line, .. } => ("syntax", json!(line)),
    };
    json!({"error": e.to_string(), "kind": kind, "line": line})
}

fn run(case: &Value) -> Value {
    let o = &case["opts"];
    let c = console(&case["console"]);
    match case["kind"].as_str().unwrap() {
        "image_art" => {
            let strict = Strict {
                art: image_art(case),
                error: Mutex::new(None),
            };
            let out = print(&c, &strict);
            let error = strict.error.lock().unwrap().take();
            match error {
                Some(e) => json!({ "error": e }),
                None => text_result(out),
            }
        }
        "native_grid" => {
            let art = image_art(case);
            json!({"out": art.native_grid(mode(ostr(o, "grid_mode").unwrap()), ousize(o, "available").unwrap())})
        }
        "ascii" | "ascii_text" => {
            let mut art = AsciiArt::new(image(&case["image"]))
                .invert(obool(o, "invert"))
                .color(obool(o, "color"))
                .normalize(opt(o, "normalize").and_then(Value::as_bool).unwrap_or(true));
            if let Some(w) = ousize(o, "width") {
                art = art.width(w);
            }
            if let Some(h) = ousize(o, "height") {
                art = art.height(h);
            }
            if let Some(r) = ostr(o, "ramp") {
                art = art.ramp(r);
            }
            if case["kind"] == "ascii" {
                text_result(print(&c, &art))
            } else {
                json!({"out": art.to_text(ousize(o, "text_width").unwrap()), "columns": art.columns(ousize(o, "text_width").unwrap())})
            }
        }
        "block" => {
            let mut art = BlockArt::new(image(&case["image"]));
            if let Some(w) = ousize(o, "width") {
                art = art.width(w);
            }
            if let Some(h) = ousize(o, "height") {
                art = art.height(h);
            }
            text_result(print(&c, &art))
        }
        "quadrant" => {
            let mut art = QuadrantArt::new(image(&case["image"]));
            if let Some(w) = ousize(o, "width") {
                art = art.width(w);
            }
            if let Some(h) = ousize(o, "height") {
                art = art.height(h);
            }
            text_result(print(&c, &art))
        }
        "braille" | "braille_text" => {
            let mut art = BrailleArt::new(image(&case["image"]));
            if let Some(w) = ousize(o, "width") {
                art = art.width(w);
            }
            if let Some(h) = ousize(o, "height") {
                art = art.height(h);
            }
            if case["kind"] == "braille" {
                text_result(print(&c, &art))
            } else {
                text_result(art.to_text(ousize(o, "text_width").unwrap()))
            }
        }
        "sixel" | "sixel_encode" => {
            let mut art = SixelArt::new(image(&case["image"]));
            if let Some(w) = ousize(o, "width") {
                art = art.width(w);
            }
            if let Some(h) = ousize(o, "height") {
                art = art.height(h);
            }
            if let Some(cp) = opt(o, "cell_px") {
                art = art.cell_px(
                    cp[0].as_u64().unwrap() as u32,
                    cp[1].as_u64().unwrap() as u32,
                );
            }
            if let Some(m) = ousize(o, "max_colors") {
                art = art.max_colors(m as u16);
            }
            if case["kind"] == "sixel" {
                text_result(print(&c, &art))
            } else {
                json!({"out": art.encode(ousize(o, "available").unwrap())})
            }
        }
        "figlet" | "figlet_text" => {
            let mut banner = Figlet::new(ostr(o, "text").unwrap()).justify(
                match ostr(o, "justify").unwrap_or("left") {
                    "left" => rich_art::Justify::Left,
                    "center" => rich_art::Justify::Center,
                    "right" => rich_art::Justify::Right,
                    j => panic!("{j}"),
                },
            );
            if let Some(s) = ostr(o, "style") {
                banner = banner.style(Style::parse(s).unwrap());
            }
            if let Some(w) = ousize(o, "width") {
                banner = banner.width(w);
            }
            if case["kind"] == "figlet" {
                text_result(print(&c, &banner))
            } else {
                text_result(banner.to_text(ousize(o, "text_width").unwrap()))
            }
        }
        "gif" => {
            let art = gif(o);
            let n = art.frame_count();
            let frames: Vec<String> = (0..n.min(3))
                .map(|i| print(&c, &art.render_frame(i).unwrap()))
                .collect();
            let ascii0 = print(&c, &art.frame(0).unwrap());
            let mut played = Vec::new();
            let plain = Console::builder()
                .force_terminal(false)
                .color_system(None)
                .width(c.width())
                .build();
            art.play(plain, &mut played).unwrap();
            json!({
                "frame_count": n,
                "duration": art.duration().as_secs_f64(),
                "delays": (0..n).map(|i| art.frame_delay(i).unwrap().as_secs_f64()).collect::<Vec<_>>(),
                "frames": frames,
                "ascii0": ascii0,
                "print": print(&c, &art),
                "played": String::from_utf8(played).unwrap(),
            })
        }
        "stage" => {
            let mut stage = Stage::new();
            for art in o["arts"].as_array().unwrap() {
                stage = stage.with(gif(art));
            }
            if let Some(g) = ousize(o, "gap") {
                stage = stage.gap(g);
            }
            let mut played = Vec::new();
            stage.play(c, &mut played).unwrap();
            text_result(String::from_utf8(played).unwrap())
        }
        "diff" => {
            let s = DiffSettings {
                blur: of32(o, "blur", 6.0),
                threshold: of32(o, "threshold", 60.0),
                open_kernel: ousize(o, "open_kernel").unwrap_or(11),
                min_region: ousize(o, "min_region").unwrap_or(400) as u64,
                top: ousize(o, "top").unwrap_or(3),
            };
            let before = image(&case["image"]);
            let after = image(&case["image2"]);
            match diff(&before, &after, &s) {
                Err(e) => json!({"error": e.to_string()}),
                Ok(r) => {
                    let bytes: Vec<u8> = r.delta_e.iter().flat_map(|v| v.to_le_bytes()).collect();
                    json!({
                        "width": r.width, "height": r.height,
                        "changed_fraction": r.changed_fraction as f64,
                        "naive_changed_fraction": r.naive_changed_fraction as f64,
                        "mean_delta_e": r.mean_delta_e as f64,
                        "max_delta_e": r.max_delta_e as f64,
                        "regions": r.regions.iter().map(|g| json!([g.x, g.y, g.width, g.height, g.area_px, g.share_of_change as f64, g.mean_delta_e as f64])).collect::<Vec<_>>(),
                        "delta_e_hex": bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                        "heatmap_hex": r.heatmap().as_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>(),
                        "highlight_hex": r.highlight(&after).as_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>(),
                    })
                }
            }
        }
        "mermaid" => {
            let mut m = rich_mermaid::Mermaid::new(ostr(o, "source").unwrap());
            if ostr(o, "backend") == Some("mmdc") {
                m = m.backend(rich_mermaid::Backend::Mmdc);
            }
            if let Some(a) = opt(o, "ascii").and_then(Value::as_bool) {
                m = m.ascii(a);
            }
            text_result(print(&c, &m))
        }
        "mermaid_parse" => match rich_mermaid::parse(ostr(o, "source").unwrap()) {
            Ok(chart) => json!({"out": flowchart_json(&chart)}),
            Err(e) => parse_error_json(&e),
        },
        "mermaid_draw" => match rich_mermaid::parse(ostr(o, "source").unwrap()) {
            Err(e) => parse_error_json(&e),
            Ok(chart) => match rich_mermaid::draw(&chart, obool(o, "ascii")) {
                Ok(d) => json!({"lines": d.lines, "width": d.width}),
                Err(e) => json!({"error": format!("too large to draw: {e}"), "kind": "too_large"}),
            },
        },
        "clean_label" => text_result(rich_mermaid::flowchart::clean_label(
            ostr(o, "text").unwrap(),
        )),
        other => panic!("unknown kind {other}"),
    }
}

fn main() {
    let cases: Vec<Value> = serde_json::from_reader(std::io::stdin()).unwrap();
    let mut out = Map::new();
    for case in &cases {
        out.insert(case["name"].as_str().unwrap().to_string(), run(case));
    }
    println!("{}", serde_json::to_string(&Value::Object(out)).unwrap());
}
