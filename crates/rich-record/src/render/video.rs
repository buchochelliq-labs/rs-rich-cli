//! Video frames from a timeline, and MP4 through FFmpeg.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::render::raster::{self, Canvas, Fonts};
use crate::screen::{Snapshot, Theme};
use crate::session::Timeline;

/// How long the last frame holds, in seconds.
const HOLD: f64 = 2.5;
/// How long a key stays in the overlay after it is pressed, in seconds.
const KEY_SHOWN: f64 = 0.8;

/// One distinct video frame: the screen, the cursor as drawn (it blinks), the
/// key shown in the overlay, and how long the frame lasts.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrame {
    pub snapshot: Snapshot,
    pub key: Option<String>,
    pub seconds: f64,
}

/// Sample `timeline` at `fps`, merging equal neighbours.
pub fn sample(timeline: &Timeline, fps: f64) -> Vec<VideoFrame> {
    let frames = &timeline.frames;
    let Some(first) = frames.first() else {
        return Vec::new();
    };
    let step = 1.0 / fps;
    let end = frames.last().expect("non-empty").0 + HOLD;
    let mut out: Vec<VideoFrame> = Vec::new();
    let mut index = 0;
    let mut t = first.0;
    while t <= end {
        while index + 1 < frames.len() && frames[index + 1].0 <= t {
            index += 1;
        }
        let key = timeline
            .keys
            .iter()
            .rev()
            .find(|(when, _)| *when <= t && t - *when <= KEY_SHOWN)
            .map(|(_, label)| label.clone());
        let mut snapshot = frames[index].1.clone();
        // Blink the cursor at 2 Hz, as a terminal does.
        if (t * 2.0) as u64 % 2 == 1 {
            snapshot.cursor = None;
        }
        match out.last_mut() {
            Some(last) if last.snapshot == snapshot && last.key == key => last.seconds += step,
            _ => out.push(VideoFrame {
                snapshot,
                key,
                seconds: step,
            }),
        }
        t += step;
    }
    out
}

/// Draw sampled frames.
pub fn render(
    frames: &[VideoFrame],
    theme: &Theme,
    fonts: &Fonts,
    title: &str,
) -> Vec<(Canvas, f64)> {
    frames
        .iter()
        .map(|frame| {
            let options = raster::Frame {
                title,
                key: frame.key.as_deref(),
                size: 16.0,
                window: true,
            };
            (
                raster::render(&frame.snapshot, theme, fonts, &options),
                frame.seconds,
            )
        })
        .collect()
}

/// Whether FFmpeg can be run.
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Encode frames as an H.264 MP4 at 12 fps through FFmpeg.
pub fn mp4(frames: &[(Canvas, f64)], path: &std::path::Path) -> std::io::Result<()> {
    let width = frames.iter().map(|(c, _)| c.width).max().unwrap_or(2);
    let height = frames.iter().map(|(c, _)| c.height).max().unwrap_or(2);
    // H.264 needs even dimensions.
    let (width, height) = (width + width % 2, height + height % 2);
    let fps = 12.0;
    let mut child = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
        ])
        .args(["-s", &format!("{width}x{height}"), "-r", "12", "-i", "-"])
        .args(["-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        for (canvas, seconds) in frames {
            let mut padded = Canvas::new(width, height, (30, 30, 30));
            padded.paste(
                canvas,
                (width - canvas.width) / 2,
                (height - canvas.height) / 2,
            );
            let repeats = (seconds * fps).round().max(1.0) as usize;
            for _ in 0..repeats {
                stdin.write_all(&padded.pixels)?;
            }
        }
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("ffmpeg failed: {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(text: &[u8]) -> Snapshot {
        let mut parser = vt100::Parser::new(2, 8, 0);
        parser.process(text);
        Snapshot::from_screen(parser.screen(), &Theme::default())
    }

    #[test]
    fn samples_hold_blink_and_keys() {
        let timeline = Timeline {
            frames: vec![(0.0, snapshot(b"a")), (1.0, snapshot(b"ab"))],
            keys: vec![(1.0, "⏎".into())],
            ..Timeline::default()
        };
        let frames = sample(&timeline, 10.0);
        let total: f64 = frames.iter().map(|f| f.seconds).sum();
        assert!((total - 3.6).abs() < 0.11, "{total}");
        assert!(frames.iter().any(|f| f.key.as_deref() == Some("⏎")));
        assert!(frames.iter().any(|f| f.snapshot.cursor.is_none()));
        assert!(frames.iter().any(|f| f.snapshot.cursor.is_some()));
    }
}
