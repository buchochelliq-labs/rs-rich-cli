//! Video frames from a timeline, and MP4 through FFmpeg.
//!
//! Frames are sampled as references into the timeline and drawn a few at a
//! time, so a long recording costs the memory of a handful of images, not of
//! every frame.

use std::io::Write;
use std::process::{Child, Command, Stdio};

use crate::render::raster::{self, Canvas, Fonts};
use crate::screen::{Snapshot, Theme};
use crate::session::{Timeline, FRAME_RATE};

/// How long the last frame holds, in seconds.
const HOLD: f64 = 2.5;
/// How long a key stays in the overlay after it is pressed, in seconds.
const KEY_SHOWN: f64 = 0.8;
/// About how many bytes of drawn frames are held at once.
const BATCH_BYTES: usize = 256 * 1024 * 1024;
/// Text size of video frames, in pixels.
const TEXT_SIZE: f32 = 16.0;

/// One distinct video frame: which of the timeline's frames it shows, whether
/// the cursor, if there is one, is drawn (it blinks), the key shown in the overlay, and how
/// long the frame lasts.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrame {
    /// An index into [`Timeline::frames`].
    pub frame: usize,
    pub cursor: bool,
    pub key: Option<String>,
    pub seconds: f64,
}

impl VideoFrame {
    /// The screen this frame shows.
    pub fn snapshot(&self, timeline: &Timeline) -> Snapshot {
        let mut snapshot = timeline.frames[self.frame].1.clone();
        if !self.cursor {
            snapshot.cursor = None;
        }
        snapshot
    }
}

/// Sample `timeline` at `fps`, merging equal neighbours.
pub fn sample(timeline: &Timeline, fps: f64) -> Vec<VideoFrame> {
    let frames = &timeline.frames;
    let (Some(first), Some(last)) = (frames.first(), frames.last()) else {
        return Vec::new();
    };
    let step = 1.0 / fps;
    let end = last.0 + HOLD;
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
        // Blink the cursor at 2 Hz, as a terminal does.
        let cursor = ((t * 2.0) as u64).is_multiple_of(2);
        let drawn = |frame: usize, cursor: bool| frames[frame].1.cursor.filter(|_| cursor);
        match out.last_mut() {
            Some(last)
                if last.key == key
                    && drawn(last.frame, last.cursor) == drawn(index, cursor)
                    && (last.frame == index
                        || frames[last.frame].1.rows == frames[index].1.rows) =>
            {
                last.seconds += step;
            }
            _ => out.push(VideoFrame {
                frame: index,
                cursor,
                key,
                seconds: step,
            }),
        }
        t += step;
    }
    out
}

/// How video frames are drawn.
fn options<'a>(title: &'a str, key: Option<&'a str>) -> raster::Frame<'a> {
    raster::Frame {
        title,
        key,
        size: TEXT_SIZE,
        window: true,
    }
}

/// The width and height, in pixels, that holds every sampled frame.
pub fn size(timeline: &Timeline, frames: &[VideoFrame], fonts: &Fonts) -> (usize, usize) {
    frames
        .iter()
        .map(|frame| {
            let snapshot = &timeline.frames[frame.frame].1;
            raster::size(
                snapshot.columns(),
                snapshot.rows.len(),
                fonts,
                &options("", None),
            )
        })
        .fold((0, 0), |(w, h), (fw, fh)| (w.max(fw), h.max(fh)))
}

/// Draw the sampled frames in order and hand each, with how long it lasts,
/// to `sink`. Frames are drawn in batches spread over the machine's cores,
/// each batch dropped before the next is drawn.
pub fn render_each(
    timeline: &Timeline,
    frames: &[VideoFrame],
    theme: &Theme,
    fonts: &Fonts,
    title: &str,
    sink: &mut raster::Sink<'_>,
) -> std::io::Result<()> {
    let draw = |frame: &VideoFrame| {
        raster::render(
            &frame.snapshot(timeline),
            theme,
            fonts,
            &options(title, frame.key.as_deref()),
        )
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let (width, height) = size(timeline, frames, fonts);
    let batch = (BATCH_BYTES / (width * height * 3).max(1)).clamp(1, threads);
    for part in frames.chunks(batch) {
        let drawn: Vec<Canvas> = if part.len() == 1 {
            vec![draw(&part[0])]
        } else {
            std::thread::scope(|scope| {
                let workers: Vec<_> = part
                    .iter()
                    .map(|frame| scope.spawn(move || draw(frame)))
                    .collect();
                workers
                    .into_iter()
                    .map(|worker| worker.join().expect("frame renderer"))
                    .collect()
            })
        };
        for (canvas, frame) in drawn.into_iter().zip(part) {
            sink(canvas, frame.seconds)?;
        }
    }
    Ok(())
}

/// Draw the sampled frames all at once. [`render_each`] holds fewer.
pub fn render(
    timeline: &Timeline,
    frames: &[VideoFrame],
    theme: &Theme,
    fonts: &Fonts,
    title: &str,
) -> Vec<(Canvas, f64)> {
    let mut out = Vec::with_capacity(frames.len());
    render_each(
        timeline,
        frames,
        theme,
        fonts,
        title,
        &mut |canvas, seconds| {
            out.push((canvas, seconds));
            Ok(())
        },
    )
    .expect("collecting frames cannot fail");
    out
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

/// An H.264 MP4 being encoded by FFmpeg at [`FRAME_RATE`], frame by frame.
/// Dropped unfinished, it stops FFmpeg and waits for it.
pub struct Mp4 {
    child: Child,
    width: usize,
    height: usize,
    /// Frames written, and the time they should cover, so rounding each
    /// frame's length does not add up to drift.
    written: u64,
    elapsed: f64,
}

impl Mp4 {
    /// Start FFmpeg writing `path`, `width` x `height` pixels (rounded up to
    /// even, as H.264 needs).
    pub fn start(path: &std::path::Path, width: usize, height: usize) -> std::io::Result<Mp4> {
        let (width, height) = (width.max(2), height.max(2));
        let (width, height) = (width + width % 2, height + height % 2);
        let child = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
            ])
            .args(["-s", &format!("{width}x{height}")])
            .args(["-r", &FRAME_RATE.to_string(), "-i", "-"])
            .args(["-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()?;
        Ok(Mp4 {
            child,
            width,
            height,
            written: 0,
            elapsed: 0.0,
        })
    }

    /// Add `canvas`, centred, for `seconds`.
    pub fn write(&mut self, canvas: &Canvas, seconds: f64) -> std::io::Result<()> {
        let result = self.write_frame(canvas, seconds);
        if result.is_err() {
            self.stop();
        }
        result
    }

    fn write_frame(&mut self, canvas: &Canvas, seconds: f64) -> std::io::Result<()> {
        let mut padded = Canvas::new(self.width, self.height, (30, 30, 30));
        padded.paste(
            canvas,
            self.width.saturating_sub(canvas.width) / 2,
            self.height.saturating_sub(canvas.height) / 2,
        );
        self.elapsed += seconds;
        let due = ((self.elapsed * FRAME_RATE).round() as u64).max(self.written + 1);
        let stdin = self
            .child
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::other("ffmpeg has stopped"))?;
        for _ in self.written..due {
            stdin.write_all(&padded.pixels)?;
        }
        self.written = due;
        Ok(())
    }

    /// Kill FFmpeg and wait for it.
    fn stop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Close the input and wait for FFmpeg to write the file.
    pub fn finish(mut self) -> std::io::Result<()> {
        drop(self.child.stdin.take());
        let status = self.child.wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("ffmpeg failed: {status}")))
        }
    }
}

impl Drop for Mp4 {
    fn drop(&mut self) {
        // After `finish` the child is already reaped; kill fails harmlessly.
        if self.child.stdin.is_some() {
            self.stop();
        }
    }
}

/// Encode frames already drawn as an MP4 through FFmpeg.
pub fn mp4(frames: &[(Canvas, f64)], path: &std::path::Path) -> std::io::Result<()> {
    let width = frames.iter().map(|(c, _)| c.width).max().unwrap_or(2);
    let height = frames.iter().map(|(c, _)| c.height).max().unwrap_or(2);
    let mut video = Mp4::start(path, width, height)?;
    for (canvas, seconds) in frames {
        video.write(canvas, *seconds)?;
    }
    video.finish()
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
        let shown = |f: &VideoFrame| f.snapshot(&timeline).cursor;
        assert!(frames.iter().any(|f| shown(f).is_none()));
        assert!(frames.iter().any(|f| shown(f).is_some()));
    }

    #[test]
    fn samples_refer_to_the_timeline_and_merge_equal_screens() {
        // Many identical frames, as a flood of output that changes nothing
        // visible leaves: sampled into a few references, not copies.
        let frames: Vec<_> = (0..600)
            .map(|i| (i as f64 / 12.0, snapshot(b"x")))
            .collect();
        let timeline = Timeline {
            frames,
            ..Timeline::default()
        };
        let sampled = sample(&timeline, FRAME_RATE);
        // One frame per half second of blink, over 50s plus the hold.
        assert!(sampled.len() <= 2 * 53, "{}", sampled.len());
        assert!(sampled.iter().all(|f| f.frame < 600));
    }

    #[test]
    fn frames_are_drawn_in_order_in_batches() {
        let timeline = Timeline {
            frames: (0..20)
                .map(|i| (i as f64, snapshot(format!("{i}").as_bytes())))
                .collect(),
            ..Timeline::default()
        };
        let sampled = sample(&timeline, 2.0);
        let fonts = Fonts::embedded();
        let theme = Theme::default();
        let all = render(&timeline, &sampled, &theme, &fonts, "t");
        let mut streamed = Vec::new();
        render_each(
            &timeline,
            &sampled,
            &theme,
            &fonts,
            "t",
            &mut |canvas, seconds| {
                streamed.push((canvas, seconds));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(all, streamed);
        let (width, height) = size(&timeline, &sampled, &fonts);
        assert!(all
            .iter()
            .all(|(c, _)| (c.width, c.height) == (width, height)));
    }

    #[test]
    fn a_failing_sink_stops_rendering() {
        let timeline = Timeline {
            frames: vec![(0.0, snapshot(b"a")), (1.0, snapshot(b"b"))],
            ..Timeline::default()
        };
        let sampled = sample(&timeline, 2.0);
        let mut calls = 0;
        let result = render_each(
            &timeline,
            &sampled,
            &Theme::default(),
            &Fonts::embedded(),
            "",
            &mut |_, _| {
                calls += 1;
                Err(std::io::Error::other("full"))
            },
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}
