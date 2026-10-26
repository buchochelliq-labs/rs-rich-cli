//! Sound in and out: a sound card captured into a signal, raw PCM and WAV
//! files read a buffer at a time, and what is shown recorded to a WAV. A
//! plugin, not core: the module is there only with the off-by-default
//! `pcm` feature, which `audio` and `wav` turn on.
//!
//! Every source gives the same thing, a [`Matrix`]: one row a channel,
//! each sample from -1 to 1. What the `pcm` feature brings:
//!
//! - [`pcm`] reads interleaved PCM (`s16le`, `f32le`, …, as `parec` and
//!   `arecord` write it) from a file, a pipe or stdin, a buffer at a time,
//!   and [`play`] plays its buffers onto a signal at the sample rate, then
//!   silence (on the core's [`latest_iter`](crate::latest_iter)).
//! - [`Frame`] is a buffer with its number, the shape a capture's signal
//!   holds.
//! - [`Mixer`] mixes clips on numbered channels (each at its own rate,
//!   gain and pan, changed or stopped while playing) and streams; with
//!   `audio` it mixes into an `Output` on a thread of its own.
//! - [`Capture::simulated`] is a capture with no sound card: buffers a
//!   period apart, which a test can [`stall`](Capture::stall) and
//!   [`fail`](Capture::fail), so what an app does on a stall or a failure
//!   is tested in any build.
//!
//! Behind the off-by-default `audio` feature, on
//! [cpal](https://docs.rs/cpal) (ALSA on Linux, CoreAudio on macOS, WASAPI
//! on Windows; building it on Linux needs ALSA's headers,
//! `libasound2-dev` or `alsa-lib-devel`):
//!
//! - [`input`] opens a device and returns a `Signal<Option<Frame>>`: `None`
//!   until the first buffer, then the newest, numbered, so two equal
//!   buffers (digital silence) are still two frames. It has the shape of
//!   [`latest`](crate::latest)'s signal, so a sound card and a reader
//!   thread are one kind of source. The app always gets the newest buffer:
//!   one that arrives while the last is still waiting for the app's thread
//!   replaces it, so a slow frame drops buffers instead of falling behind.
//! - [`open`] does the same before an app exists, so a missing device
//!   fails before the terminal is taken, and gives a [`Capture`] with a
//!   [`status`](Capture::status) signal (the core's
//!   [`FeedState`](crate::FeedState): running, stalled since a time on the
//!   app's clock, failed, or ended once stopped) to end the app on.
//! - [`devices`] lists the inputs by [`Device`] (an id to open, a name to
//!   show), [`Device::configs`] what each can do, and [`picker`] is a list
//!   to choose one in the app.
//! - ALSA's complaints on stderr (`ALSA lib conf.c: …`) are kept off the
//!   screen while devices are listed and opened; [`messages`] has them,
//!   and [`Capture::log`] sends them to a [`Log`](crate::Log).
//!
//! The `pulse` feature adds PulseAudio (and PipeWire's PulseAudio server)
//! as [`Host::Pulse`], through cpal's own client, which needs no system
//! library: every source is listed, a sink's monitor among them, and
//! [`AudioOptions::latency`] sets the server's buffer. The `wav` feature
//! adds [`wav`] (WAV files in) and [`wav_writer`] and [`record`] (out),
//! through hound.
//!
//! ```no_run
//! # #[cfg(feature = "audio")] {
//! use intuituive::audio;
//! use intuituive::prelude::*;
//!
//! for device in audio::devices() {
//!     println!("{} ({}){}", device.name, device.id, if device.default { " default" } else { "" });
//! }
//! App::new(|| {
//!     let samples = audio::input(None, 48_000, 2, 1024).expect("a microphone");
//!     text(move || {
//!         let peak = samples.with(|f| match f {
//!             Some(frame) => frame.samples.iter().flatten().fold(0.0f64, |a, s| a.max(s.abs())),
//!             None => 0.0,
//!         });
//!         format!("peak {peak:.2}")
//!     })
//!     .on_key("q", |cx| cx.quit())
//! })
//! .run()
//! # .unwrap();
//! # }
//! ```

use std::fmt;

#[cfg(all(feature = "audio", target_os = "linux"))]
mod alsa;
#[cfg(feature = "audio")]
mod capture;
mod live;
mod mixer;
#[cfg(feature = "audio")]
mod output;
mod pcm;
#[cfg(feature = "audio")]
mod picker;
mod player;
mod source;
#[cfg(feature = "wav")]
mod wav;

#[cfg(feature = "audio")]
pub use capture::{
    devices, devices_on, input, messages, open, AudioOptions, Device, DeviceConfig, Host,
};
pub use live::Capture;
pub use mixer::{Mixer, Voice};
#[cfg(feature = "audio")]
pub use output::{output, output_devices, Output, OutputOptions, Tap};
pub use pcm::{deinterleave, pcm, play, Format, Pcm};
#[cfg(feature = "audio")]
pub use picker::picker;
pub use player::{player, Player, PlayerOptions};
pub use source::{clip, open_source, Clip, Source};
#[cfg(feature = "wav")]
pub use wav::{record, wav, wav_writer, Recording, Wav, WavWriter};

/// One buffer of samples: one row a channel, each sample from -1 to 1.
pub type Matrix = Vec<Vec<f64>>;

/// A buffer as a capture delivers it: numbered from 1 in the order it was
/// captured, so a watch on the number sees every buffer, two that hold
/// the same samples included. Numbers skip the buffers the app had no
/// time for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// Its place among the buffers captured, from 1.
    pub number: u64,
    /// The samples.
    pub samples: Matrix,
}

/// Why sound could not be opened, read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioError(String);

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AudioError {}

#[cfg(any(feature = "audio", feature = "wav"))]
fn error(what: impl fmt::Display) -> AudioError {
    AudioError(what.to_string())
}
