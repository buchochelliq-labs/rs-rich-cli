//! Turning snapshots and timelines into files.

pub mod cast;
pub mod emoji;
pub mod html;
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
}

impl<'a> Look<'a> {
    /// A window titled `title`, without a caption.
    pub fn window(title: &'a str) -> Look<'a> {
        Look {
            title,
            window: true,
            caption: None,
        }
    }
}
