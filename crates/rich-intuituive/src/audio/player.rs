//! A player: tracks played one after another on the core's
//! [`transport`](crate::transport), through an [`Output`](super::Output)
//! (with the `audio` feature), with a volume, a speed and gapless
//! playback.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::source::{open_source, Source};
#[cfg(feature = "audio")]
use super::source::{Decoder, Event};
use super::AudioError;
#[cfg(feature = "audio")]
use crate::reactive::Proxy;
use crate::reactive::{signal, Runtime, Signal};
use crate::transport::{transport, Transport};

/// What opens a track: a path to a [`Source`].
type Opener = Arc<dyn Fn(&Path) -> Result<Box<dyn Source>, AudioError> + Send + Sync>;

/// How a [`player`] plays.
#[derive(Clone)]
pub struct PlayerOptions {
    opener: Opener,
    gapless: bool,
    #[cfg(feature = "audio")]
    output: Option<super::Output>,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        PlayerOptions::new()
    }
}

impl std::fmt::Debug for PlayerOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerOptions")
            .field("gapless", &self.gapless)
            .finish()
    }
}

impl PlayerOptions {
    /// Gapless, tracks opened by [`open_source`], and no output: the
    /// transport alone (for a test, or a machine with no sound card),
    /// until one is given ([`output`](Self::output), `audio` feature).
    pub fn new() -> PlayerOptions {
        PlayerOptions {
            opener: Arc::new(open_source),
            gapless: true,
            #[cfg(feature = "audio")]
            output: None,
        }
    }

    /// Open tracks with `opener` (a decoder for another codec), rather
    /// than [`open_source`].
    pub fn opener(
        mut self,
        opener: impl Fn(&Path) -> Result<Box<dyn Source>, AudioError> + Send + Sync + 'static,
    ) -> PlayerOptions {
        self.opener = Arc::new(opener);
        self
    }

    /// Start the next track straight after the last sample of this one
    /// (the default), or after the device has played this one out.
    pub fn gapless(mut self, on: bool) -> PlayerOptions {
        self.gapless = on;
        self
    }

    /// Play through `output` (a sound card opened by
    /// [`output`](super::output)).
    #[cfg(feature = "audio")]
    pub fn output(mut self, output: super::Output) -> PlayerOptions {
        self.output = Some(output);
        self
    }
}

/// What the app tells the decoding thread.
struct Command {
    tracks: Vec<PathBuf>,
    track: Option<usize>,
    /// Bumped on a jump to a track or a seek: the thread opens the track
    /// again at `position`.
    jump: u64,
    position: Duration,
    playing: bool,
    speed: f64,
    gapless: bool,
    stop: bool,
}

/// The command, and a condition the thread waits on.
struct Shared {
    command: Mutex<Command>,
    changed: Condvar,
}

impl Drop for Shared {
    fn drop(&mut self) {
        lock(&self.command).stop = true;
        self.changed.notify_all();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// A music player's model, as termusic's: a list of tracks, the one
/// playing, and its [`transport`](Self::transport) (the core's
/// [`Transport`]: position, play and pause, seek, rate) on the app's
/// clock; a [`volume`](Self::set_volume), a [`speed`](Self::set_speed)
/// and gapless playback. With an output ([`PlayerOptions::output`],
/// `audio` feature) a thread decodes the tracks ([`Source`]s: WAV with
/// the `wav` feature, or an opener of your own) and plays them; without
/// one the transport runs alone, which is what a test steps. When a track
/// ends the next one plays; after the last, it stops. `Copy`; made by
/// [`player`].
#[derive(Clone, Copy)]
pub struct Player {
    transport: Transport,
    tracks: Signal<Vec<PathBuf>>,
    track: Signal<Option<usize>>,
    volume: Signal<f64>,
    speed: Signal<f64>,
    error: Signal<Option<String>>,
    shared: Signal<Arc<Shared>>,
    opener: Signal<Opener>,
    #[cfg(feature = "audio")]
    output: Signal<Option<super::Output>>,
}

/// A [`Player`] with no tracks, as `options` says. Call it while the app
/// is built.
///
/// ```
/// use intuituive::audio::{clip, player, PlayerOptions, Source};
/// use intuituive::prelude::*;
/// use std::time::Duration;
///
/// // Tracks in memory: "one" lasts two seconds, "two" one.
/// let opener = |path: &std::path::Path| -> Result<Box<dyn Source>, intuituive::audio::AudioError> {
///     let seconds = if path.ends_with("one") { 2 } else { 1 };
///     Ok(Box::new(clip(100, vec![vec![0.0; 100 * seconds]])))
/// };
/// let app = App::new(move || {
///     let music = player(PlayerOptions::new().opener(opener));
///     music.queue(vec!["one".into(), "two".into()]);
///     music.play_track(0);
///     let shown = move || {
///         let head = music.transport();
///         format!("{:?} {:.1}/{:.1}", music.track(), head.position().as_secs_f64(), head.length().unwrap_or_default().as_secs_f64())
///     };
///     text(shown).on_key("n", move |_| music.next())
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(30, 1);
/// driver.advance(Duration::from_millis(2500));
/// // The first ended at 2 s; the second is half way.
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(1) 0.5/1.0");
/// driver.advance(Duration::from_secs(1));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(1) 1.0/1.0");
/// ```
///
/// # Panics
/// Outside a running or building app.
pub fn player(options: PlayerOptions) -> Player {
    let runtime = Runtime::current_for("audio::player()");
    let shared = Arc::new(Shared {
        command: Mutex::new(Command {
            tracks: Vec::new(),
            track: None,
            jump: 0,
            position: Duration::ZERO,
            playing: false,
            speed: 1.0,
            gapless: options.gapless,
            stop: false,
        }),
        changed: Condvar::new(),
    });
    let error = signal(None);
    #[cfg(feature = "audio")]
    if let Some(output) = options.output.clone() {
        let (thread_shared, opener, proxy) =
            (shared.clone(), options.opener.clone(), runtime.proxy());
        std::thread::Builder::new()
            .name("intuituive-player".into())
            .spawn(move || decode(&thread_shared, &output, &opener, &proxy, error))
            .ok();
    }
    #[cfg(not(feature = "audio"))]
    let _ = &runtime;
    let music = Player {
        transport: transport(Duration::ZERO),
        tracks: signal(Vec::new()),
        track: signal(None),
        volume: signal(1.0),
        speed: signal(1.0),
        error,
        shared: signal(shared),
        opener: signal(options.opener),
        #[cfg(feature = "audio")]
        output: signal(options.output),
    };
    music.transport.on_end(move |_| music.advance());
    music
}

/// The decoding thread: the command's track at its position, at its
/// speed, into `output`, until the player goes or the app ends.
#[cfg(feature = "audio")]
fn decode(
    shared: &Arc<Shared>,
    output: &super::Output,
    opener: &Opener,
    proxy: &Proxy,
    error: Signal<Option<String>>,
) {
    let mut decoder = Decoder::new(output.rate(), usize::from(output.channels()));
    let mut seen = 0;
    let tell = |why: String| {
        proxy.run(move || error.set(Some(why)));
    };
    loop {
        let command = lock(&shared.command);
        if command.stop || output.stopped() || !proxy.run(|| {}) {
            return;
        }
        if command.jump != seen {
            seen = command.jump;
            output.clear();
            if let Some(path) = command.track.and_then(|i| command.tracks.get(i)).cloned() {
                let index = command.track.unwrap_or(0);
                match opener(&path) {
                    Ok(source) => {
                        decoder.load(index, source);
                        if let Err(why) = decoder.seek(command.position) {
                            tell(why.to_string());
                        }
                    }
                    Err(why) => tell(format!("{}: {why}", path.display())),
                }
            }
        }
        decoder.set_speed(command.speed);
        if !command.playing || decoder.track().is_none() {
            let _ = shared
                .changed
                .wait_timeout(command, Duration::from_millis(100));
            continue;
        }
        let (tracks, gapless) = (command.tracks.clone(), command.gapless);
        drop(command);
        let mut next = |i: usize| {
            if !gapless {
                return None;
            }
            tracks.get(i).and_then(|path| opener(path).ok())
        };
        let (samples, events) = decoder.fill(1024, &mut next);
        for event in events {
            if let Event::Failed(why) = event {
                tell(why);
            }
        }
        if !output.write(&samples) {
            return;
        }
    }
}

impl Player {
    /// The play head: position, length, play and pause, seek, rate, as the
    /// core's [`Transport`]; read it to draw a progress bar.
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// Replace the tracks, stopped, none playing.
    pub fn queue(&self, tracks: Vec<PathBuf>) {
        self.transport.pause();
        self.transport.seek(Duration::ZERO);
        self.tracks.set(tracks.clone());
        self.track.set(None);
        self.tell(|c| {
            c.tracks = tracks;
            c.track = None;
            c.playing = false;
            c.jump += 1;
        });
    }

    /// Add `track` after the others.
    pub fn push(&self, track: PathBuf) {
        self.tracks.update(|t| t.push(track.clone()));
        self.tell(|c| c.tracks.push(track));
    }

    /// The tracks, subscribing.
    pub fn tracks(&self) -> Vec<PathBuf> {
        self.tracks.get()
    }

    /// The track playing (or paused), its index, subscribing.
    pub fn track(&self) -> Option<usize> {
        self.track.get()
    }

    /// Why the last track could not be opened or read, subscribing.
    pub fn error(&self) -> Option<String> {
        self.error.get()
    }

    /// Play track `index` from its start. A track that cannot be opened
    /// is skipped, its error kept ([`error`](Self::error)); an index past
    /// the last stops.
    pub fn play_track(&self, index: usize) {
        self.error.set(None);
        self.start(index, true, false);
    }

    /// Open track `index` and play it from its start, skipping a track
    /// that cannot be opened (to the one before it, going `back`); a
    /// natural advance (`jump` false) leaves a gapless decoder going on to
    /// it by itself.
    fn start(&self, index: usize, jump: bool, back: bool) {
        let Some(path) = self.tracks.with_untracked(|t| t.get(index).cloned()) else {
            self.transport.pause();
            self.set_paused(true);
            self.tell(|c| c.playing = false);
            return;
        };
        let opener = self.opener.with_untracked(Arc::clone);
        let length = match opener(&path) {
            Ok(source) => source.duration(),
            Err(why) => {
                self.error.set(Some(format!("{}: {why}", path.display())));
                return match (back, index) {
                    (true, 0) => self.transport.pause(),
                    (true, _) => self.start(index - 1, true, true),
                    (false, _) => self.start(index + 1, true, false),
                };
            }
        };
        self.track.set(Some(index));
        self.transport.set_length(length);
        self.transport.seek(Duration::ZERO);
        self.transport.play();
        self.set_paused(false);
        self.tell(|c| {
            c.track = Some(index);
            c.position = Duration::ZERO;
            c.playing = true;
            if jump || !c.gapless {
                c.jump += 1;
            }
        });
    }

    /// The track ended: the next one, or a stop after the last.
    fn advance(&self) {
        let Some(at) = self.track.get_untracked() else {
            return;
        };
        if at + 1 < self.tracks.with_untracked(Vec::len) {
            self.start(at + 1, false, false);
        } else {
            self.set_paused(true);
            self.tell(|c| c.playing = false);
        }
    }

    /// Play on (the first track, if none is playing).
    pub fn play(&self) {
        if self.track.get_untracked().is_none() {
            return self.play_track(0);
        }
        self.transport.play();
        let position = self.transport.position_untracked();
        self.set_paused(false);
        self.tell(|c| {
            if !c.playing {
                c.position = position;
                c.playing = true;
            }
        });
    }

    /// Pause where it is.
    pub fn pause(&self) {
        self.transport.pause();
        self.set_paused(true);
        self.tell(|c| c.playing = false);
    }

    /// Play if paused, pause if playing.
    pub fn toggle(&self) {
        if self.transport.is_playing_untracked() {
            self.pause();
        } else {
            self.play();
        }
    }

    /// The next track (nothing after the last).
    pub fn next(&self) {
        if let Some(at) = self.track.get_untracked() {
            if at + 1 < self.tracks.with_untracked(Vec::len) {
                self.play_track(at + 1);
            }
        }
    }

    /// Back to the start of the track, or, within its first three seconds,
    /// the track before (as players do).
    pub fn previous(&self) {
        let Some(at) = self.track.get_untracked() else {
            return;
        };
        if at > 0 && self.transport.position_untracked() < Duration::from_secs(3) {
            self.error.set(None);
            self.start(at - 1, true, true);
        } else {
            self.seek(Duration::ZERO);
        }
    }

    /// Play on from `to` in the track.
    pub fn seek(&self, to: Duration) {
        self.transport.seek(to);
        let position = self.transport.position_untracked();
        self.tell(|c| {
            c.position = position;
            c.jump += 1;
        });
    }

    /// Play on `seconds` from here (back, for less than 0).
    pub fn seek_by(&self, seconds: f64) {
        self.transport.seek_by(seconds);
        let position = self.transport.position_untracked();
        self.tell(|c| {
            c.position = position;
            c.jump += 1;
        });
    }

    /// The volume, from 0 to 1 (and up to 2), subscribing.
    pub fn volume(&self) -> f64 {
        self.volume.get()
    }

    /// Set the volume (kept from 0 to 2), at once.
    pub fn set_volume(&self, volume: f64) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 2.0)
        } else {
            1.0
        };
        self.volume.set(volume);
        #[cfg(feature = "audio")]
        self.output.with_untracked(|o| {
            if let Some(output) = o {
                output.set_volume(volume);
            }
        });
    }

    /// The speed, subscribing: 1 is as recorded.
    pub fn speed(&self) -> f64 {
        self.speed.get()
    }

    /// Play at `speed` (kept from 0.25 to 4; the pitch moves with it, as a
    /// tape's): the transport's rate too.
    pub fn set_speed(&self, speed: f64) {
        if !speed.is_finite() {
            return;
        }
        let speed = speed.clamp(0.25, 4.0);
        self.speed.set(speed);
        self.transport.set_rate(speed);
        self.tell(|c| c.speed = speed);
    }

    /// Gapless playback on or off.
    pub fn set_gapless(&self, on: bool) {
        self.tell(|c| c.gapless = on);
    }

    /// Pause or play the output, at once.
    fn set_paused(&self, paused: bool) {
        #[cfg(feature = "audio")]
        self.output.with_untracked(|o| {
            if let Some(output) = o {
                output.set_paused(paused);
            }
        });
        #[cfg(not(feature = "audio"))]
        let _ = paused;
    }

    /// Change the command, and wake the thread.
    fn tell(&self, change: impl FnOnce(&mut Command)) {
        let shared = self.shared.with_untracked(Arc::clone);
        change(&mut lock(&shared.command));
        shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::super::clip;
    use super::*;

    fn opener(path: &Path) -> Result<Box<dyn Source>, AudioError> {
        let name = path.to_string_lossy();
        if name == "broken" {
            return Err(AudioError("not a sound".into()));
        }
        let seconds: usize = name.parse().unwrap_or(1);
        Ok(Box::new(clip(10, vec![vec![0.0; 10 * seconds]])))
    }

    #[test]
    fn pause_seek_speed_previous_and_a_broken_track_skipped() {
        let app = crate::App::new(|| {
            let music = player(PlayerOptions::new().opener(opener));
            music.queue(vec!["4".into(), "broken".into(), "2".into()]);
            let shown = move || {
                format!(
                    "{:?} {:.1} {} {}",
                    music.track(),
                    music.transport().position().as_secs_f64(),
                    music.transport().is_playing(),
                    music.error().unwrap_or_default()
                )
            };
            crate::text(shown)
                .on_key("space", move |_| music.toggle())
                .on_key("l", move |_| music.seek_by(1.0))
                .on_key("f", move |_| music.set_speed(2.0))
                .on_key("p", move |_| music.previous())
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(40, 1);
        let row = |driver: &crate::Driver| driver.screen().plain()[0].trim_end().to_string();
        driver.press("space");
        driver.advance(Duration::from_millis(500));
        assert_eq!(row(&driver), "Some(0) 0.5 true");
        driver.press("space");
        driver.advance(Duration::from_secs(1));
        assert_eq!(row(&driver), "Some(0) 0.5 false", "paused");
        driver.press("space l f");
        driver.advance(Duration::from_millis(500));
        assert_eq!(row(&driver), "Some(0) 2.5 true");
        // The end of 4 s, at twice the speed; "broken" is skipped.
        driver.advance(Duration::from_millis(750));
        assert_eq!(row(&driver), "Some(2) 0.0 true broken: not a sound");
        // Within three seconds: back past the broken one, to the first.
        driver.press("p");
        assert_eq!(row(&driver), "Some(0) 0.0 true broken: not a sound");
        // Later than that: the start of the track again.
        driver.advance(Duration::from_secs(2));
        driver.press("p");
        assert_eq!(row(&driver), "Some(0) 0.0 true broken: not a sound");
    }

    #[test]
    fn after_the_last_track_it_stops() {
        let app = crate::App::new(|| {
            let music = player(PlayerOptions::new().opener(opener));
            music.queue(vec!["1".into(), "2".into()]);
            music.play_track(1);
            crate::text(move || {
                let head = music.transport();
                format!(
                    "{:?} {:.1} {}",
                    music.track(),
                    head.position().as_secs_f64(),
                    head.is_playing()
                )
            })
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(40, 1);
        driver.advance(Duration::from_secs(3));
        assert_eq!(driver.screen().plain()[0].trim_end(), "Some(1) 2.0 false");
    }
}
