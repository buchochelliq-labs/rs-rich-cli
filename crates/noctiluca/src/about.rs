//! `about:noctiluca`: what this browser is, how it adapts pages to the
//! terminal, and what it can play.
//!
//! One page, opened by `?` (`Alt+I`) or by going to [`ADDRESS`]: the
//! version, the browser, the terminal adaptations and which are on, and a
//! codec table with the browser's own answers to `canPlayType` for each
//! media type ([`CODECS`]), asked when the page opens: a system Chrome
//! plays H.264 and AAC, an open-source Chromium usually does not. The app
//! draws it ([`crate::app`]); the tab asks the browser
//! ([`crate::tab::Update::About`]).

/// The address that opens the about page.
pub const ADDRESS: &str = "about:noctiluca";

/// The about page's title.
pub const TITLE: &str = "About Noctiluca";

/// The media types the codec table asks about: a name for people, and the
/// type as `canPlayType` takes it.
pub const CODECS: &[(&str, &str)] = &[
    ("H.264 (MP4)", "video/mp4; codecs=\"avc1.42E01E\""),
    ("H.265 (MP4)", "video/mp4; codecs=\"hev1.1.6.L93.B0\""),
    ("AV1 (MP4)", "video/mp4; codecs=\"av01.0.05M.08\""),
    ("VP8 (WebM)", "video/webm; codecs=\"vp8\""),
    ("VP9 (WebM)", "video/webm; codecs=\"vp9\""),
    ("AV1 (WebM)", "video/webm; codecs=\"av01.0.05M.08\""),
    ("Theora (Ogg)", "video/ogg; codecs=\"theora\""),
    ("AAC (MP4)", "audio/mp4; codecs=\"mp4a.40.2\""),
    ("MP3", "audio/mpeg"),
    ("Opus (WebM)", "audio/webm; codecs=\"opus\""),
    ("Opus (Ogg)", "audio/ogg; codecs=\"opus\""),
    ("Vorbis (Ogg)", "audio/ogg; codecs=\"vorbis\""),
    ("FLAC", "audio/flac"),
    ("WAV", "audio/wav"),
    ("HLS", "application/vnd.apple.mpegurl"),
];

/// The media types of [`CODECS`], in order.
pub fn types() -> Vec<&'static str> {
    CODECS.iter().map(|(_, t)| *t).collect()
}

/// `canPlayType`'s answer as the table says it.
pub fn verdict(answer: &str) -> &'static str {
    match answer {
        "probably" => "yes (probably)",
        "maybe" => "maybe",
        _ => "no",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_say_what_can_play_type_said() {
        assert_eq!(verdict("probably"), "yes (probably)");
        assert_eq!(verdict("maybe"), "maybe");
        assert_eq!(verdict(""), "no");
        assert_eq!(types().len(), CODECS.len());
    }
}
