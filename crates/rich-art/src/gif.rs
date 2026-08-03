//! Animated GIFs played in the terminal as ASCII/ANSI art.
//!
//! Each frame is converted with [`AsciiArt`](crate::AsciiArt) and drawn in
//! place through `rich`'s [`Live`] display, honouring the GIF's own per-frame
//! delays. Frame disposal (background / previous) is handled by the `image`
//! decoder, so every frame arrives as a full canvas.
//!
//! Requires the non-default `gif` feature.
//!
//! ```no_run
//! use rich::Console;
//! use rich_art::gif::AnimatedArt;
//!
//! let art = AnimatedArt::from_path("spin.gif")?.color(true).max_fps(20.0);
//! art.play_stdout(Console::builder().build())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::io::Write;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::{AnimationDecoder, DynamicImage, ImageError};

use rich::protocol::Renderable;
use rich::{Console, Control, Live};

use crate::ascii::AsciiArt;

/// A GIF's `0` delay means "as fast as possible"; browsers substitute 100ms and
/// so do we, otherwise such GIFs spin at whatever speed the terminal allows.
const ZERO_DELAY_SUBSTITUTE: Duration = Duration::from_millis(100);

/// How many times playback repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    /// Play through once.
    #[default]
    Once,
    /// Play a fixed number of times.
    Times(usize),
    /// Loop until the process is interrupted.
    Forever,
}

/// One decoded frame: a full canvas plus how long it should be shown.
#[derive(Clone)]
struct Frame {
    image: DynamicImage,
    delay: Duration,
}

/// An animated GIF, renderable as terminal art.
#[derive(Clone)]
pub struct AnimatedArt {
    frames: Vec<Frame>,
    width: Option<usize>,
    height: Option<usize>,
    ramp: Option<String>,
    invert: bool,
    color: bool,
    repeat: Repeat,
    /// Lower bound on a frame's on-screen time, i.e. an upper bound on frame
    /// rate. Colour art is byte-heavy, so an uncapped fast GIF can outrun a
    /// slow terminal and tear.
    min_delay: Option<Duration>,
}

impl AnimatedArt {
    /// Decode an animated GIF from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ImageError> {
        let decoder = GifDecoder::new(std::io::Cursor::new(bytes))?;
        let decoded = decoder.into_frames().collect_frames()?;
        let frames = decoded
            .into_iter()
            .map(|frame| {
                let (numer, denom) = frame.delay().numer_denom_ms();
                let millis = if denom == 0 {
                    0
                } else {
                    u64::from(numer) / u64::from(denom)
                };
                let delay = if millis == 0 {
                    ZERO_DELAY_SUBSTITUTE
                } else {
                    Duration::from_millis(millis)
                };
                Frame {
                    image: DynamicImage::ImageRgba8(frame.into_buffer()),
                    delay,
                }
            })
            .collect();
        Ok(AnimatedArt {
            frames,
            width: None,
            height: None,
            ramp: None,
            invert: false,
            color: false,
            repeat: Repeat::Once,
            min_delay: None,
        })
    }

    /// Decode an animated GIF from a file.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, ImageError> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// Render this many columns (default: the console width).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Render this many rows (default: from the GIF's aspect ratio).
    pub fn height(mut self, height: usize) -> Self {
        self.height = Some(height);
        self
    }

    /// Use a custom density ramp, darkest character first.
    pub fn ramp(mut self, ramp: impl Into<String>) -> Self {
        self.ramp = Some(ramp.into());
        self
    }

    /// Swap dark and light.
    pub fn invert(mut self, invert: bool) -> Self {
        self.invert = invert;
        self
    }

    /// Colour each cell with its sampled pixel (ANSI art).
    pub fn color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// How many times to play through.
    pub fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Cap the frame rate. Frames that would display for less than `1/fps`
    /// seconds are held for that long instead.
    pub fn max_fps(mut self, fps: f64) -> Self {
        if fps > 0.0 {
            self.min_delay = Some(Duration::from_secs_f64(1.0 / fps));
        }
        self
    }

    /// Number of decoded frames.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Total run time of one pass, after any frame-rate cap.
    pub fn duration(&self) -> Duration {
        self.frames.iter().map(|f| self.delay_of(f)).sum()
    }

    fn delay_of(&self, frame: &Frame) -> Duration {
        match self.min_delay {
            Some(min) => frame.delay.max(min),
            None => frame.delay,
        }
    }

    /// How long frame `index` is displayed, after any frame-rate cap. Used by
    /// [`Stage`](crate::Stage) to drive each animation on its own clock.
    pub fn frame_delay(&self, index: usize) -> Option<Duration> {
        self.frames.get(index).map(|frame| self.delay_of(frame))
    }

    /// Frame `index` as a still renderable, carrying this animation's options.
    pub fn frame(&self, index: usize) -> Option<AsciiArt> {
        let frame = self.frames.get(index)?;
        let mut art = AsciiArt::new(frame.image.clone())
            .invert(self.invert)
            .color(self.color);
        if let Some(width) = self.width {
            art = art.width(width);
        }
        if let Some(height) = self.height {
            art = art.height(height);
        }
        if let Some(ramp) = &self.ramp {
            art = art.ramp(ramp.clone());
        }
        Some(art)
    }

    /// How many passes to make, or `None` for endless.
    pub(crate) fn passes(&self) -> Option<usize> {
        match self.repeat {
            Repeat::Once => Some(1),
            Repeat::Times(n) => Some(n),
            Repeat::Forever => None,
        }
    }

    /// Play the animation, drawing each frame in place on `writer`.
    ///
    /// Blocks for the animation's duration. The cursor is hidden during
    /// playback and restored on return.
    ///
    /// **Note:** an interrupt (Ctrl-C) terminates the process without unwinding,
    /// so the cursor can be left hidden. A caller that traps signals should emit
    /// [`Control::show_cursor(true)`] on the way out.
    pub fn play<W: Write>(&self, console: Console, mut writer: W) -> std::io::Result<()> {
        if self.frames.is_empty() {
            return Ok(());
        }
        let Some(first) = self.frame(0) else {
            return Ok(());
        };

        let mut live = Live::new(Box::new(first), console, &mut writer);
        live.start();

        let mut pass = 0usize;
        loop {
            for index in 0..self.frames.len() {
                // The first frame of the first pass is already on screen from
                // `start()`; sleep through its delay, then move on.
                if !(pass == 0 && index == 0) {
                    if let Some(art) = self.frame(index) {
                        live.update(Box::new(art));
                    }
                }
                std::thread::sleep(self.delay_of(&self.frames[index]));
            }
            pass += 1;
            if let Some(total) = self.passes() {
                if pass >= total {
                    break;
                }
            }
        }

        live.stop();
        writer.flush()
    }

    /// Play to stdout.
    pub fn play_stdout(&self, console: Console) -> std::io::Result<()> {
        let stdout = std::io::stdout();
        let handle = stdout.lock();
        self.play(console, handle)
    }
}

/// The control sequence that re-shows the cursor — useful for a signal handler
/// that has to clean up after an interrupted [`AnimatedArt::play`].
pub fn show_cursor_sequence() -> String {
    Control::show_cursor(true).as_str().to_string()
}

/// A still frame is a renderable in its own right; the animation renders as its
/// first frame so it can be `print`ed like any other art.
impl Renderable for AnimatedArt {
    fn rich_render(
        &self,
        console: &Console,
        options: &rich::console::ConsoleOptions,
    ) -> Vec<rich::segment::Segment> {
        match self.frame(0) {
            Some(art) => art.rich_render(console, options),
            None => Vec::new(),
        }
    }
}

/// Helpers shared by this module's tests and the [`stage`](crate::stage) ones.
#[cfg(test)]
pub(crate) mod tests_support {
    use image::codecs::gif::GifEncoder;
    use image::{Delay, Frame as ImageFrame, Rgba, RgbaImage};

    /// Encode a small animated GIF in memory: one solid-colour frame per entry
    /// in `colors`, each shown for `delay_ms`.
    pub(crate) fn make_gif(colors: &[[u8; 3]], delay_ms: u32) -> Vec<u8> {
        let mut buffer = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut buffer);
            for rgb in colors {
                let image = RgbaImage::from_pixel(8, 8, Rgba([rgb[0], rgb[1], rgb[2], 255]));
                let frame =
                    ImageFrame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(delay_ms, 1));
                encoder.encode_frame(frame).expect("encodes");
            }
        }
        buffer
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::make_gif;
    use super::*;

    #[test]
    fn decodes_frames_and_delays() {
        let bytes = make_gif(&[[0, 0, 0], [255, 255, 255], [128, 128, 128]], 50);
        let art = AnimatedArt::from_bytes(&bytes).expect("decodes");
        assert_eq!(art.frame_count(), 3);
        // GIF stores delays in centiseconds, so 50ms round-trips exactly.
        assert_eq!(art.duration(), Duration::from_millis(150));
    }

    #[test]
    fn zero_delay_is_substituted() {
        let bytes = make_gif(&[[0, 0, 0], [255, 255, 255]], 0);
        let art = AnimatedArt::from_bytes(&bytes).expect("decodes");
        assert_eq!(art.duration(), ZERO_DELAY_SUBSTITUTE * 2);
    }

    #[test]
    fn max_fps_raises_short_delays() {
        let bytes = make_gif(&[[0, 0, 0], [255, 255, 255]], 10);
        let art = AnimatedArt::from_bytes(&bytes)
            .expect("decodes")
            .max_fps(20.0);
        // 10ms frames held to 50ms each by the 20fps cap.
        assert_eq!(art.duration(), Duration::from_millis(100));
    }

    #[test]
    fn frames_carry_the_render_options() {
        let ramp: Vec<char> = crate::ascii::DEFAULT_RAMP.chars().collect();
        let bytes = make_gif(&[[0, 0, 0]], 50);
        let art = AnimatedArt::from_bytes(&bytes)
            .expect("decodes")
            .width(4)
            .height(1);
        let frame = art.frame(0).expect("a frame");
        assert_eq!(frame.to_text(4), format!("{0}{0}{0}{0}", ramp[0]));
        assert!(art.frame(99).is_none());
    }

    #[test]
    fn playback_writes_frames_and_restores_the_cursor() {
        let bytes = make_gif(&[[0, 0, 0], [255, 255, 255]], 0);
        let art = AnimatedArt::from_bytes(&bytes)
            .expect("decodes")
            .width(4)
            .height(1)
            .max_fps(1000.0); // keep the test fast
        let console = Console::builder().width(20).build();
        let mut out = Vec::new();
        art.play(console, &mut out).expect("plays");
        let text = String::from_utf8(out).expect("utf-8");
        // Hides the cursor to start and shows it again at the end.
        assert!(text.starts_with("\x1b[?25l"), "got {text:?}");
        assert!(text.ends_with("\x1b[?25h"), "got {text:?}");
    }

    #[test]
    fn repeat_multiplies_the_duration() {
        let bytes = make_gif(&[[0, 0, 0], [255, 255, 255]], 50);
        let art = AnimatedArt::from_bytes(&bytes).expect("decodes");
        assert_eq!(art.passes(), Some(1));
        assert_eq!(art.clone_repeat(Repeat::Times(3)).passes(), Some(3));
        assert_eq!(art.clone_repeat(Repeat::Forever).passes(), None);
    }

    impl AnimatedArt {
        /// Test helper: the builder consumes `self`, but these assertions want
        /// to vary only the repeat mode.
        fn clone_repeat(&self, repeat: Repeat) -> AnimatedArt {
            AnimatedArt {
                frames: Vec::new(),
                width: self.width,
                height: self.height,
                ramp: self.ramp.clone(),
                invert: self.invert,
                color: self.color,
                repeat,
                min_delay: self.min_delay,
            }
        }
    }
}
