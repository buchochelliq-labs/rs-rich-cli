//! Screenshots as SVG: sharp at any size, with selectable text.

use std::fmt::Write;

use crate::screen::{Rgb, Snapshot, Theme};

const CELL_WIDTH: f64 = 9.6;
const LINE_HEIGHT: f64 = 20.8;
const FONT_SIZE: u32 = 16;
const PAD: f64 = 19.0;
const BAR: f64 = 35.0;
const CHROME: Rgb = (30, 30, 30);

fn hex((r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Draw `snapshot` in a window frame titled `title`. Each run of text is
/// stretched to its cells (`textLength`), so columns line up whatever
/// monospace font the viewer has.
pub fn svg(snapshot: &Snapshot, theme: &Theme, title: &str) -> String {
    let columns = snapshot.columns() as f64;
    let lines = snapshot.rows.len() as f64;
    let width = columns * CELL_WIDTH + 2.0 * PAD;
    let height = lines * LINE_HEIGHT + 2.0 * PAD + BAR;
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.1} {height:.1}" font-family="Fira Code, DejaVu Sans Mono, Menlo, monospace" font-size="{FONT_SIZE}">"#
    );
    let _ = writeln!(
        out,
        r##"<rect width="100%" height="100%" rx="9" fill="{}" stroke="#464646"/>"##,
        hex(theme.background)
    );
    let _ = writeln!(
        out,
        r#"<path d="M0 9a9 9 0 0 1 9-9h{:.1}a9 9 0 0 1 9 9v{}h-{width:.1}z" fill="{}"/>"#,
        width - 18.0,
        BAR - 9.0,
        hex(CHROME)
    );
    for (index, dot) in ["#ff5f56", "#ffbd2e", "#27c93f"].iter().enumerate() {
        let _ = writeln!(
            out,
            r#"<circle cx="{}" cy="{}" r="6" fill="{dot}"/>"#,
            PAD + index as f64 * 22.0,
            BAR / 2.0
        );
    }
    if !title.is_empty() {
        let _ = writeln!(
            out,
            r##"<text x="{:.1}" y="{}" fill="#969696" font-family="DejaVu Sans, sans-serif" font-size="13" text-anchor="middle">{}</text>"##,
            width / 2.0,
            BAR / 2.0 + 5.0,
            escape(title)
        );
    }
    let top = BAR + PAD;
    for (y, row) in snapshot.rows.iter().enumerate() {
        let row_top = top + y as f64 * LINE_HEIGHT;
        let baseline = row_top + LINE_HEIGHT * 0.75;
        let mut x = 0;
        while x < row.len() {
            let cell = &row[x];
            let mut end = x + 1;
            while end < row.len() && (row[end].is_continuation() || row[end].same_style(cell)) {
                end += 1;
            }
            let left = PAD + x as f64 * CELL_WIDTH;
            let span = (end - x) as f64 * CELL_WIDTH;
            if cell.bg != theme.background {
                let _ = writeln!(
                    out,
                    r#"<rect x="{left:.1}" y="{row_top:.1}" width="{span:.1}" height="{LINE_HEIGHT}" fill="{}"/>"#,
                    hex(cell.bg)
                );
            }
            let text: String = row[x..end].iter().map(|c| c.text.as_str()).collect();
            if !text.trim().is_empty() {
                let mut attrs = format!(r#"fill="{}""#, hex(cell.fg));
                if cell.bold {
                    attrs.push_str(r#" font-weight="bold""#);
                }
                if cell.italic {
                    attrs.push_str(r#" font-style="italic""#);
                }
                if cell.underline {
                    attrs.push_str(r#" text-decoration="underline""#);
                }
                let _ = writeln!(
                    out,
                    r#"<text x="{left:.1}" y="{baseline:.1}" {attrs} textLength="{span:.1}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{}</text>"#,
                    escape(&text)
                );
            }
            x = end;
        }
    }
    if let Some((x, y)) = snapshot.cursor {
        let _ = writeln!(
            out,
            r#"<rect x="{:.1}" y="{:.1}" width="{CELL_WIDTH}" height="{LINE_HEIGHT}" fill="{}" opacity="0.7"/>"#,
            PAD + x as f64 * CELL_WIDTH,
            top + y as f64 * LINE_HEIGHT,
            hex(theme.foreground)
        );
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_backgrounds_and_escaping() {
        let mut parser = vt100::Parser::new(2, 10, 0);
        parser.process(b"\x1b[41m<a>\x1b[0m & b");
        let theme = Theme::default();
        let shot = Snapshot::from_screen(parser.screen(), &theme);
        let svg = svg(&shot, &theme, "T & t");
        assert!(svg.contains("&lt;a&gt;"));
        assert!(svg.contains("T &amp; t"));
        assert!(svg.contains(&format!(r#"fill="{}""#, hex(theme.ansi[1]))));
        assert!(svg.ends_with("</svg>\n"));
    }
}
