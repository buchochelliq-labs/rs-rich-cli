//! Screenshots as SVG: sharp at any size, with selectable text.
//!
//! Drawn by rich-ext's frame exporter ([`rich_ext::frame::Frame::to_svg`]),
//! so a recording's screenshots look like the rest of rich's SVG output: the
//! same template, fonts and window. The screen becomes a frame first, with
//! the theme's default colours left unset.

use rich_ext::frame::SvgOptions;

pub use crate::render::Look;
use crate::screen::{Snapshot, Theme};

/// Draw `snapshot` as `look` says. Each run of text is stretched to its
/// cells (`textLength`), so columns line up whatever monospace font the
/// viewer has.
pub fn svg(snapshot: &Snapshot, theme: &Theme, look: &Look<'_>) -> String {
    let frame = snapshot.export_frame(theme);
    let terminal = theme.terminal_theme();
    frame.to_svg(&SvgOptions {
        theme: &terminal,
        title: look.title,
        unique_id: "rich-record",
        width: Some(snapshot.columns()),
        window: look.window,
        cursor: snapshot
            .cursor
            .map(|(column, row)| (column as usize, row as usize)),
        caption: look.caption,
        ..SvgOptions::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(bytes: &[u8]) -> Snapshot {
        let mut parser = vt100::Parser::new(2, 10, 0);
        parser.process(bytes);
        Snapshot::from_screen(parser.screen(), &Theme::default())
    }

    #[test]
    fn runs_backgrounds_and_escaping() {
        let theme = Theme::default();
        let svg = svg(
            &shot(b"\x1b[41m<a>\x1b[0m & b"),
            &theme,
            &Look::window("T & t"),
        );
        assert!(svg.contains("&lt;a&gt;"));
        assert!(svg.contains("T&#160;&amp;&#160;t"));
        let (r, g, b) = theme.ansi[1];
        assert!(svg.contains(&format!(r##"<rect fill="#{r:02x}{g:02x}{b:02x}""##)));
        // The theme's own background draws no rectangle.
        assert_eq!(svg.matches("shape-rendering=\"crispEdges\"").count(), 1);
        assert!(svg.ends_with("</svg>\n"));
        assert!(svg.contains("#ff5f57"), "the window's buttons");
    }

    #[test]
    fn without_a_window_with_a_caption_and_cursor() {
        let theme = Theme::default();
        let look = Look {
            title: "t",
            window: false,
            caption: Some("A caption"),
        };
        let svg = svg(&shot(b"hi"), &theme, &look);
        assert!(!svg.contains("#ff5f57"));
        assert!(svg.contains("A&#160;caption"));
        assert!(svg.contains(r#"opacity="0.7""#), "the cursor");
    }

    #[test]
    fn characters_xml_forbids_do_not_reach_the_svg() {
        let theme = Theme::default();
        let svg = svg(&shot(b"a"), &theme, &Look::window("bell\u{7}"));
        assert!(!svg.chars().any(|c| c == '\u{7}'), "{svg}");
    }
}
