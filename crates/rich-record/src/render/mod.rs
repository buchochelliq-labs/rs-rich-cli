//! Turning snapshots and timelines into files, and rendered output into
//! PNG ([`png`]).

pub mod cast;
pub mod emoji;
pub mod html;
pub mod png;
pub mod raster;
pub mod svg;
pub mod video;

/// How stills and video frames are presented: the window frame and its
/// title, and a caption under the terminal.
#[derive(Clone, Copy, Debug)]
pub struct Look<'a> {
    pub title: &'a str,
    /// Draw the window frame (title bar and buttons).
    pub window: bool,
    pub caption: Option<&'a str>,
    /// The text size of video frames, in pixels.
    pub text_size: f32,
}

impl<'a> Look<'a> {
    /// A window titled `title`, without a caption, in 16-pixel text.
    pub fn window(title: &'a str) -> Look<'a> {
        Look {
            title,
            window: true,
            caption: None,
            text_size: f32::from(crate::record::FONT_SIZE),
        }
    }
}
