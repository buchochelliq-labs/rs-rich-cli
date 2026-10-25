//! `about:noctiluca`: what this browser is and what it can play.
//!
//! The page is ordinary HTML that Noctiluca makes and opens as a `data:`
//! address, so it is read and drawn like any other page. Its codec table
//! holds the browser's own answers to `canPlayType` for each media type
//! ([`CODECS`]), asked when the page is opened: a system Chrome plays
//! H.264 and AAC, an open-source Chromium usually does not.

/// The address that opens the about page.
pub const ADDRESS: &str = "about:noctiluca";

/// The about page's title, which the header shows as [`ADDRESS`].
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

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The about page, with `answers` to [`CODECS`] in order (`canPlayType`'s
/// raw answers) and the browser's `product` (its name and version).
pub fn page(answers: &[String], product: &str) -> String {
    let mut rows = String::new();
    for ((name, kind), answer) in CODECS.iter().zip(answers) {
        rows.push_str(&format!(
            "<tr><td>{}</td><td><code>{}</code></td><td>{}</td></tr>\n",
            escape(name),
            escape(kind),
            verdict(answer)
        ));
    }
    format!(
        "<!doctype html><meta charset=utf-8><title>{TITLE}</title>\n\
         <h1>Noctiluca {version}</h1>\n\
         <p>A terminal web browser: page text as terminal text, media as media.</p>\n\
         <p>Browser: {product}</p>\n\
         <h2>Codecs</h2>\n\
         <p>What this browser says it can play (<code>canPlayType</code>). \
         DRM playback is never tried.</p>\n\
         <table><thead><tr><th>Codec</th><th>Type</th><th>Plays</th></tr></thead>\n\
         <tbody>\n{rows}</tbody></table>\n",
        version = env!("CARGO_PKG_VERSION"),
        product = escape(product),
    )
}

/// `html` as a `data:` address.
pub fn data_url(html: &str) -> String {
    let mut out = String::from("data:text/html;charset=utf-8,");
    for byte in html.bytes() {
        if byte.is_ascii_alphanumeric() || b" -_.~=/;:()!*,".contains(&byte) {
            if byte == b' ' {
                out.push_str("%20");
            } else {
                out.push(byte as char);
            }
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Whether `url` and `title` are the about page's, so the header shows
/// [`ADDRESS`] rather than the `data:` address.
pub fn is_about(url: &str, title: &str) -> bool {
    url.starts_with("data:text/html;charset=utf-8,") && title == TITLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_each_answer() {
        let answers: Vec<String> = CODECS
            .iter()
            .enumerate()
            .map(|(i, _)| ["probably", "maybe", ""][i % 3].to_string())
            .collect();
        let html = page(&answers, "Chrome/1 <test>");
        assert!(html.contains("<td>H.264 (MP4)</td><td><code>video/mp4; codecs=&quot;avc1.42E01E&quot;</code></td><td>yes (probably)</td>"));
        assert!(html.contains("<td>H.265 (MP4)</td>") && html.contains("<td>maybe</td>"));
        assert!(html.contains("Chrome/1 &lt;test&gt;"));
        let url = data_url(&html);
        assert!(url.starts_with("data:text/html;charset=utf-8,%3C!doctype"));
        assert!(!url.contains('"') && !url.contains('#') && !url.contains('\n'));
        assert!(is_about(&url, TITLE));
        assert!(!is_about("https://example.com", TITLE));
    }
}
