//! A mixer of voices: many short sounds at once on numbered channels,
//! each at its own rate, gain and pan, changed or stopped while playing,
//! and streams (music) under them. Sound effects for a game, UI sounds, a
//! sampler.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use super::{Clip, Source};

/// Frames the mixer's thread mixes at a time.
#[cfg(feature = "audio")]
const BLOCK: usize = 256;

/// One sound playing on a channel.
struct Playing {
    clip: Arc<Clip>,
    /// Its number, to tell it from a later sound on the same channel.
    serial: u64,
    /// Where it is, in the clip's frames.
    at: f64,
    /// Clip frames a mixed frame.
    step: f64,
    gain: f32,
    pan: f32,
}

/// A stream playing under the voices.
struct Stream {
    source: Box<dyn Source>,
    /// Its samples read and not yet mixed, a row a channel.
    pending: Vec<VecDeque<f32>>,
    /// Where it is between the first pending frame and the next.
    at: f64,
    step: f64,
    ended: bool,
}

struct Core {
    rate: u32,
    channels: u16,
    voices: Vec<Option<Playing>>,
    streams: Vec<Stream>,
    serial: u64,
}

fn lock(core: &Mutex<Core>) -> MutexGuard<'_, Core> {
    core.lock().unwrap_or_else(|e| e.into_inner())
}

/// The left and right gains of `gain` at `pan`, -1 (left) to 1 (right):
/// a balance, so the middle is the sound as it is and the sides take the
/// other side away.
fn sides(gain: f32, pan: f32) -> (f32, f32) {
    let pan = if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    (gain * (1.0 - pan).min(1.0), gain * (1.0 + pan).min(1.0))
}

/// The sample of `row` at `at`, between frames linearly; 0 past the end.
fn sample_at(row: &[f64], at: f64) -> f32 {
    let i = at as usize;
    let t = (at - i as f64) as f32;
    let a = row.get(i).copied().unwrap_or(0.0) as f32;
    if t == 0.0 {
        return a;
    }
    let b = row.get(i + 1).copied().unwrap_or(0.0) as f32;
    a + (b - a) * t
}

impl Core {
    /// Add the next frames to `out`, interleaved in the mixer's channels.
    fn mix(&mut self, out: &mut [f32]) {
        let channels = usize::from(self.channels.max(1));
        let frames = out.len() / channels;
        for slot in self.voices.iter_mut() {
            let Some(voice) = slot else { continue };
            let clip = &voice.clip;
            let len = clip.len() as f64;
            let rows = &clip.samples;
            let (left, right) = sides(voice.gain, voice.pan);
            for frame in out.chunks_exact_mut(channels).take(frames) {
                if voice.at >= len {
                    break;
                }
                let l = rows.first().map_or(0.0, |r| sample_at(r, voice.at));
                let r = rows.get(1).map_or(l, |r| sample_at(r, voice.at));
                add(frame, l, r, left, right);
                voice.at += voice.step;
            }
            if voice.at >= len {
                *slot = None;
            }
        }
        for stream in &mut self.streams {
            for frame in out.chunks_exact_mut(channels).take(frames) {
                let Some((l, r)) = stream.next() else { break };
                add(frame, l, r, 1.0, 1.0);
            }
        }
        self.streams.retain(|s| !s.ended);
    }
}

/// Add a frame of `l` and `r` at the side gains to `frame`.
fn add(frame: &mut [f32], l: f32, r: f32, left: f32, right: f32) {
    match frame {
        [mono] => *mono += (l * left + r * right) / 2.0,
        [a, b, ..] => {
            *a += l * left;
            *b += r * right;
        }
        [] => {}
    }
}

impl Stream {
    /// The next mixed frame, left and right; `None` at the end.
    fn next(&mut self) -> Option<(f32, f32)> {
        // Two frames pending to blend between, unless the source ended.
        while self.pending.first().map_or(0, VecDeque::len) < self.at as usize + 2 && !self.ended {
            match self.source.next_buffer() {
                Some(Ok(buffer)) => {
                    if self.pending.len() < buffer.len() {
                        self.pending.resize(buffer.len(), VecDeque::new());
                    }
                    for (row, samples) in self.pending.iter_mut().zip(&buffer) {
                        row.extend(samples.iter().map(|s| *s as f32));
                    }
                }
                _ => self.ended = true,
            }
        }
        let first = self.pending.first()?;
        let i = self.at as usize;
        if i >= first.len() {
            self.ended = true;
            return None;
        }
        let t = (self.at - i as f64) as f32;
        let at = |row: &VecDeque<f32>| {
            let a = row.get(i).copied().unwrap_or(0.0);
            let b = row.get(i + 1).copied().unwrap_or(0.0);
            a + (b - a) * t
        };
        let l = at(first);
        let r = self.pending.get(1).map_or(l, at);
        let pending = first.len();
        self.at += self.step;
        // Drain only what is pending: an advance past the buffer's end
        // stays in `at`, to be taken from the next buffer.
        let done = (self.at as usize).min(pending);
        for row in &mut self.pending {
            row.drain(..done.min(row.len()));
        }
        self.at -= done as f64;
        Some((l, r))
    }
}

/// A mixer of voices on numbered channels and of streams, at one rate in
/// one layout: see [`Mixer::play`] and [`Mixer::source`]. Clips at any
/// rate are resampled linearly. With the `audio` feature,
/// [`Mixer::new`] mixes into an output on a thread of its own; a mixer
/// made with [`Mixer::detached`] is mixed by calling [`Mixer::mix`] (from
/// a renderer, [`output_with`](super::output_with), or a test). Clones
/// are the same mixer.
///
/// ```
/// use intuituive::audio::{clip, Mixer};
/// use std::sync::Arc;
///
/// let mixer = Mixer::detached(8000, 2);
/// let beep = Arc::new(clip(4000, vec![vec![0.5; 100]]));
/// let voice = mixer.play(0, beep, 1.0, -1.0);
/// let mut out = vec![0.0f32; 2 * 256];
/// mixer.mix(&mut out);
/// // Left only, twice as long at twice the rate.
/// assert_eq!((out[0], out[1]), (0.5, 0.0));
/// assert_eq!((out[2 * 198], out[2 * 200]), (0.5, 0.0));
/// assert!(!voice.is_playing());
/// ```
#[derive(Clone)]
pub struct Mixer {
    core: Arc<Mutex<Core>>,
}

impl std::fmt::Debug for Mixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let core = lock(&self.core);
        f.debug_struct("Mixer")
            .field("rate", &core.rate)
            .field("channels", &core.channels)
            .field("voices", &core.voices.iter().flatten().count())
            .field("streams", &core.streams.len())
            .finish()
    }
}

impl Mixer {
    /// A mixer at `rate` in `channels` that nothing mixes yet: call
    /// [`mix`](Self::mix) for each block.
    pub fn detached(rate: u32, channels: u16) -> Mixer {
        Mixer {
            core: Arc::new(Mutex::new(Core {
                rate: rate.max(1),
                channels: channels.max(1),
                voices: Vec::new(),
                streams: Vec::new(),
                serial: 0,
            })),
        }
    }

    /// A mixer that mixes into `output` on a thread of its own, 256
    /// frames at a time, until the output stops or the mixer's last
    /// clone is dropped.
    #[cfg(feature = "audio")]
    pub fn new(output: &super::Output) -> Mixer {
        let mixer = Mixer::detached(output.rate(), output.channels());
        let core = Arc::downgrade(&mixer.core);
        let output = output.clone();
        let channels = usize::from(output.channels());
        let _ = std::thread::Builder::new()
            .name("intuituive-mixer".into())
            .spawn(move || {
                let mut block = vec![0.0f32; BLOCK * channels];
                while let Some(core) = core.upgrade() {
                    block.fill(0.0);
                    lock(&core).mix(&mut block);
                    drop(core);
                    if !output.write(&block) {
                        break;
                    }
                }
            });
        mixer
    }

    /// Samples a second.
    pub fn rate(&self) -> u32 {
        lock(&self.core).rate
    }

    /// Channels a frame has.
    pub fn channels(&self) -> u16 {
        lock(&self.core).channels
    }

    /// Play `clip` on `channel` (any number; a sound already on it stops)
    /// at `gain` (1 as it is) and `pan` (-1 left only, 0 both, 1 right
    /// only): a balance, so a sound in the middle plays at its gain on
    /// both sides. A clip of one channel plays on both sides, of two its
    /// left and right. In one channel out, the sides are averaged.
    pub fn play(&self, channel: usize, clip: Arc<Clip>, gain: f32, pan: f32) -> Voice {
        let mut core = lock(&self.core);
        core.serial += 1;
        let serial = core.serial;
        let step = f64::from(clip.rate) / f64::from(core.rate);
        if core.voices.len() <= channel {
            core.voices.resize_with(channel + 1, || None);
        }
        core.voices[channel] = Some(Playing {
            clip,
            serial,
            at: 0.0,
            step,
            gain,
            pan,
        });
        Voice {
            core: self.core.clone(),
            channel,
            serial,
        }
    }

    /// Mix `source` (music, a stream) in at its own gain until it ends,
    /// resampled to the mixer's rate.
    pub fn source(&self, source: Box<dyn Source>) {
        let mut core = lock(&self.core);
        let step = f64::from(source.rate().max(1)) / f64::from(core.rate);
        core.streams.push(Stream {
            source,
            pending: Vec::new(),
            at: 0.0,
            step,
            ended: false,
        });
    }

    /// Stop every voice and stream.
    pub fn stop_all(&self) {
        let mut core = lock(&self.core);
        core.voices.iter_mut().for_each(|v| *v = None);
        core.streams.clear();
    }

    /// Mix the next `out.len() / channels` frames into `out`, interleaved,
    /// added to what is there: for a mixer made with
    /// [`detached`](Self::detached).
    pub fn mix(&self, out: &mut [f32]) {
        lock(&self.core).mix(out);
    }
}

/// A sound playing on a [`Mixer`]'s channel: change it, stop it, ask
/// whether it still plays. Once it ends, or another sound takes its
/// channel, it does nothing.
#[derive(Clone)]
pub struct Voice {
    core: Arc<Mutex<Core>>,
    channel: usize,
    serial: u64,
}

impl std::fmt::Debug for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voice")
            .field("channel", &self.channel)
            .field("playing", &self.is_playing())
            .finish()
    }
}

impl Voice {
    fn with(&self, f: impl FnOnce(&mut Option<Playing>)) {
        let mut core = lock(&self.core);
        if let Some(slot) = core.voices.get_mut(self.channel) {
            if slot.as_ref().is_some_and(|v| v.serial == self.serial) {
                f(slot);
            }
        }
    }

    /// Its channel.
    pub fn channel(&self) -> usize {
        self.channel
    }

    /// Change its gain and pan, from the next block mixed.
    pub fn set(&self, gain: f32, pan: f32) {
        self.with(|slot| {
            if let Some(voice) = slot {
                voice.gain = gain;
                voice.pan = pan;
            }
        });
    }

    /// Stop it, from the next block mixed.
    pub fn stop(&self) {
        self.with(|slot| *slot = None);
    }

    /// Whether it is still playing: false once its clip has been mixed to
    /// the end, it was stopped, or another sound took its channel.
    pub fn is_playing(&self) -> bool {
        let mut playing = false;
        self.with(|slot| playing = slot.is_some());
        playing
    }
}
