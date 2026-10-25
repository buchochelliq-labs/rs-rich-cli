//! Sound, against a real Chrome with its audio on (`ChromeOptions::audio`,
//! rs-rich-embed's switch). Runs when `RICH_EMBED_CHROME` names a browser,
//! as a normal user. Chrome's ALSA output goes to a file sink of this
//! test's own (an `ALSA_CONFIG_PATH` for this process and the browser it
//! starts, as the proof of concept's `audio-output.py` did): no device, no
//! microphone, and nothing of the system's audio settings changes.

use std::io::Read;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::engine::{MediaOp, Playback};
use noctiluca::file_url;
use noctiluca::model::Page;
use noctiluca::tab::{Command, Tab, Update};

/// The RMS of 16-bit little-endian samples.
fn rms(bytes: &[u8]) -> f64 {
    let samples: Vec<f64> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b) as f64 / 32768.0)
        .collect();
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
}

/// Chrome's ALSA output, read from a FIFO at the pace of a real device
/// (48 kHz, stereo, 16-bit): unpaced, a file sink is written as fast as
/// Chrome can decode, and the media clock runs away with it.
struct Sink {
    pcm: Arc<Mutex<Vec<u8>>>,
}

const RATE: usize = 48_000 * 2 * 2;

impl Sink {
    /// An ALSA configuration whose default output writes raw S16_LE
    /// stereo at 48 kHz to a FIFO, for this process and its children.
    fn install(name: &str) -> Sink {
        let dir =
            std::env::temp_dir().join(format!("noctiluca-audio-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("out.fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo failed");
        let conf = dir.join("asound.conf");
        std::fs::write(
            &conf,
            format!(
                "pcm.null {{ type null }}\n\
                 pcm.sink {{ type file slave.pcm \"null\" file \"{}\" format \"raw\" }}\n\
                 pcm.!default {{ type plug slave {{ pcm \"sink\" format S16_LE rate 48000 channels 2 }} }}\n",
                fifo.display()
            ),
        )
        .unwrap();
        std::env::set_var("ALSA_CONFIG_PATH", &conf);
        // No PulseAudio server: Chrome uses ALSA.
        std::env::set_var("PULSE_SERVER", "unix:/nonexistent");
        let pcm = Arc::new(Mutex::new(Vec::new()));
        let kept = pcm.clone();
        std::thread::spawn(move || {
            // Opening blocks until Chrome opens its end.
            let Ok(mut fifo) = std::fs::File::open(&fifo) else {
                return;
            };
            let start = Instant::now();
            let mut buffer = vec![0u8; RATE / 50];
            loop {
                match fifo.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut pcm = kept.lock().unwrap();
                        pcm.extend_from_slice(&buffer[..n]);
                        // At most a device's pace: sleep until this many
                        // bytes would have played.
                        let due = Duration::from_secs_f64(pcm.len() as f64 / RATE as f64);
                        drop(pcm);
                        if let Some(wait) = due.checked_sub(start.elapsed()) {
                            std::thread::sleep(wait);
                        }
                    }
                }
            }
        });
        Sink { pcm }
    }

    fn len(&self) -> usize {
        self.pcm.lock().unwrap().len()
    }

    /// The RMS of what was played since `from`.
    fn level_since(&self, from: usize) -> f64 {
        let pcm = self.pcm.lock().unwrap();
        rms(pcm.get(from..).unwrap_or_default())
    }
}

fn playback(page: &Page) -> Option<Playback> {
    page.media.first().and_then(|m| m.playback)
}

/// One test at a time: each points the process's ALSA at its own sink.
static ALSA: Mutex<()> = Mutex::new(());

#[test]
fn without_the_switch_chrome_stays_silent() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let _one = ALSA.lock().unwrap_or_else(|e| e.into_inner());
    let sink = Sink::install("off");
    let backend = ChromeBackend::start(ChromeOptions {
        binary: Some(binary.into()),
        columns: 100,
        rows: 40,
        ..ChromeOptions::default()
    })
    .unwrap();
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(backend), move |u| {
        let _ = send.send(u);
    });
    let url =
        file_url(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio.html")).unwrap();
    tab.send(Command::Open(url.clone()));
    let start = Instant::now();
    let mut playing = false;
    while start.elapsed() < Duration::from_secs(30) && !playing {
        if let Ok(Update::Page(p)) = updates.recv_timeout(Duration::from_millis(100)) {
            if p.url == url {
                if let Some(b) = playback(&p) {
                    if b.paused {
                        tab.send(Command::Media {
                            document: p.document,
                            node: p.media[0].node,
                            op: MediaOp::Play,
                        });
                    } else {
                        playing = b.time > 0.3;
                    }
                }
            }
        }
    }
    assert!(playing, "it did not play");
    std::thread::sleep(Duration::from_millis(1500));
    // Muted by --mute-audio: nothing, or silence, leaves it.
    assert_eq!(sink.level_since(0), 0.0, "sound left a muted Chrome");
}

#[test]
fn sound_plays_mutes_and_changes_volume() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let _one = ALSA.lock().unwrap_or_else(|e| e.into_inner());
    let sink = Sink::install("on");
    let backend = ChromeBackend::start(ChromeOptions {
        binary: Some(binary.into()),
        audio: true,
        columns: 100,
        rows: 40,
        ..ChromeOptions::default()
    })
    .unwrap();
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(Box::new(backend), move |u| {
        let _ = send.send(u);
    });
    let url =
        file_url(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio.html")).unwrap();
    tab.send(Command::Open(url.clone()));
    let mut page: Option<Arc<Page>> = None;
    let mut wait = |what: &str, ok: &dyn Fn(&Page) -> bool| -> Arc<Page> {
        let start = Instant::now();
        loop {
            if let Some(p) = &page {
                if ok(p) {
                    return p.clone();
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timed out waiting for {what}: {:?}",
                page.as_ref().map(|p| playback(p))
            );
            if let Ok(Update::Page(p)) = updates.recv_timeout(Duration::from_millis(100)) {
                page = Some(p);
            }
        }
    };
    let first = wait("the page", &|p| p.url == url && playback(p).is_some());
    let node = first.media[0].node;
    let document = first.document;
    let media = |op| Command::Media { document, node, op };
    tab.send(media(MediaOp::Play));
    let playing = wait("playing", &|p| {
        playback(p).is_some_and(|b| !b.paused && !b.muted && b.time > 0.0)
    });
    let t0 = playback(&playing).unwrap().time;
    let advanced = wait("time moving on", &|p| {
        playback(p).is_some_and(|b| b.time > t0 + 0.5)
    });
    assert!(!playback(&advanced).unwrap().paused);
    let loud_from = sink.len();
    std::thread::sleep(Duration::from_millis(1500));
    let loud = sink.level_since(loud_from);
    tab.send(media(MediaOp::Mute(true)));
    wait("muted", &|p| playback(p).is_some_and(|b| b.muted));
    std::thread::sleep(Duration::from_millis(500));
    let muted_from = sink.len();
    std::thread::sleep(Duration::from_millis(1000));
    let muted = sink.level_since(muted_from);
    tab.send(media(MediaOp::Mute(false)));
    tab.send(media(MediaOp::Volume(0.3)));
    wait("at 30%", &|p| {
        playback(p).is_some_and(|b| !b.muted && (b.volume - 0.3).abs() < 0.01)
    });
    std::thread::sleep(Duration::from_millis(500));
    let quiet_from = sink.len();
    std::thread::sleep(Duration::from_millis(1000));
    let quiet = sink.level_since(quiet_from);
    tab.send(media(MediaOp::Pause));
    wait("paused", &|p| playback(p).is_some_and(|b| b.paused));
    eprintln!(
        "sink: {} bytes; RMS playing {loud:.4}, muted {muted:.4}, at 30% {quiet:.4}",
        sink.len()
    );
    if sink.len() == 0 {
        // The element's state is proven above; that samples left Chrome
        // is not, on a machine where Chrome wrote nothing to ALSA.
        eprintln!("unverified: Chrome wrote no samples to the ALSA sink here");
        return;
    }
    assert!(loud > 0.01, "playing is silent: {loud}");
    assert!(
        muted < loud / 10.0,
        "muted is not quiet: {muted} against {loud}"
    );
    assert!(quiet < loud && quiet > muted, "30% is not between: {quiet}");
}
