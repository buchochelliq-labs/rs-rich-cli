//! Sound in recordings (`Set Audio on`): a program run by the tape writes
//! a second of a 440 Hz tone to `RICH_RECORD_AUDIO`, as intuiTUIve's
//! outputs do, and the recording carries it: a WAV beside the cast, an AAC
//! track in the MP4. The program is a Python script, so these are skipped
//! where `python3` is missing.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use rich_record::record::{self, Formats, Options};
use rich_record::render::raster::Fonts;
use rich_record::session::Event;
use rich_record::tape::{self, Format};

/// Says whether it was asked for sound, then (when it was) plays a 1 s
/// tone, saying `TONE` at the moment it starts.
const SCRIPT: &str = r#"
import math, os, struct, time
d = os.environ.get("RICH_RECORD_AUDIO")
print("AUDIO", "set" if d else "unset", flush=True)
if d:
    time.sleep(0.4)
    rate = 8000
    samples = b"".join(struct.pack("<f", 0.5 * math.sin(2 * math.pi * 440 * i / rate)) for i in range(rate))
    start = time.time_ns()
    print("TONE", flush=True)
    with open(os.path.join(d, "tone.pcm"), "wb") as f:
        f.write(b"RRAUDIO1" + struct.pack("<IHHQ", rate, 1, 0, start) + samples)
print("DONE", flush=True)
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-record-audio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn has(program: &str) -> bool {
    Command::new(program)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
        || Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
}

/// The tape, with sound or without, running the script in `dir`.
fn tape(dir: &Path, audio: bool) -> tape::Tape {
    let script = dir.join("tone.py");
    std::fs::write(&script, SCRIPT).unwrap();
    let source = format!(
        "Set Size 40x6\n{}Type \"python3 {}\"\nEnter\nWait \"DONE\" 10s\nSleep 1500ms\nScreenshot end\n",
        if audio { "Set Audio on\n" } else { "" },
        script.display()
    );
    tape::parse(&source).unwrap()
}

/// When `text` was first shown, on the video's clock.
fn shown(recording: &record::Recording, text: &str) -> f64 {
    let zero = recording.timeline.frames[0].0;
    let at = recording
        .timeline
        .events
        .iter()
        .find_map(|(t, event)| match event {
            Event::Output(out) if out.contains(text) => Some(*t),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text} was never shown"));
    at - zero
}

/// A 16-bit PCM WAV's rate, channels and samples.
fn read_wav(bytes: &[u8]) -> (u32, u16, Vec<f32>) {
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1, "PCM");
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    assert_eq!(&bytes[36..40], b"data");
    let samples = bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)) / 32767.0)
        .collect();
    (rate, channels, samples)
}

#[test]
fn a_program_that_plays_sound_gives_a_wav_and_an_mp4_with_its_track() {
    if !has("python3") {
        eprintln!("skipped: python3 is not installed");
        return;
    }
    let dir = scratch("on");
    let recording = record::record(&tape(&dir, true), "audio", &Options::default()).unwrap();
    assert!(recording
        .text_grid(&recording.shots[0].1)
        .contains("AUDIO set"));
    let audio = recording.audio.clone().expect("the tone was recorded");
    assert_eq!((audio.rate, audio.channels), (8000, 1));
    // The tone starts with the frame that says so, within a frame (1/12 s).
    let frame = 1.0 / recording.frame_rate;
    let tone = shown(&recording, "TONE");
    assert!(
        (audio.offset - tone).abs() <= frame,
        "the sound starts at {:.3} s, the frame at {tone:.3} s",
        audio.offset
    );
    assert!(
        (audio.duration() - 1.0).abs() <= frame,
        "{}",
        audio.duration()
    );

    // The cast gets a WAV beside it, from the video's start.
    let out = dir.join("cast");
    let mut cast = Formats::NONE;
    cast.set(Format::Cast, true);
    let written = record::write(
        &recording,
        &out,
        "audio",
        cast,
        &Fonts::embedded(),
        &Default::default(),
        None,
    )
    .unwrap();
    let wav = out.join("audio.wav");
    assert!(written.contains(&wav), "{written:?}");
    let (rate, channels, samples) = read_wav(&std::fs::read(&wav).unwrap());
    assert_eq!((rate, channels), (8000, 1));
    let loud: Vec<usize> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.abs() > 0.1)
        .map(|(i, _)| i)
        .collect();
    let (first, last) = (
        loud[0] as f64 / 8000.0,
        *loud.last().unwrap() as f64 / 8000.0,
    );
    assert!(
        (first - tone).abs() <= frame,
        "the WAV's tone at {first:.3} s, the frame at {tone:.3} s"
    );
    assert!(
        ((last - first) - 1.0).abs() <= frame,
        "lasts {:.3} s",
        last - first
    );
    // 440 Hz: 880 zero crossings in its second.
    let crossings = samples[loud[0]..*loud.last().unwrap()]
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    assert!((860..=900).contains(&crossings), "{crossings} crossings");

    // An MP4 carries it as AAC, from the same moment, and needs no WAV.
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("skipped the MP4: ffmpeg or ffprobe is not installed");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let out = dir.join("mp4");
    let mut mp4 = Formats::NONE;
    mp4.set(Format::Mp4, true);
    let written = record::write_selected(
        &recording,
        &out,
        "audio",
        mp4,
        &Fonts::embedded(),
        &Default::default(),
        None,
    )
    .unwrap();
    assert!(written.is_complete());
    assert!(!out.join("audio.wav").exists());
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,codec_name,start_time",
        ])
        .args(["-of", "csv=p=0"])
        .arg(out.join("audio.mp4"))
        .output()
        .unwrap();
    let streams = String::from_utf8_lossy(&probe.stdout);
    let sound = streams
        .lines()
        .find(|line| line.contains("audio"))
        .unwrap_or_else(|| panic!("no audio stream in {streams:?}"));
    assert!(sound.starts_with("aac,audio,"), "{sound}");
    // Decoded as a player plays it, from the video's start (the AAC
    // encoder's priming skipped, silence before the track): the tone
    // starts with its frame.
    let decoded = out.join("decoded.raw");
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(out.join("audio.mp4"))
        .args(["-map", "0:a", "-af", "aresample=async=1:first_pts=0"])
        .args(["-ac", "1", "-ar", "8000", "-f", "s16le"])
        .arg(&decoded)
        .status()
        .unwrap();
    assert!(status.success());
    let samples: Vec<f32> = std::fs::read(&decoded)
        .unwrap()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)) / 32767.0)
        .collect();
    let onset = samples
        .iter()
        .position(|s| s.abs() > 0.1)
        .expect("the tone") as f64
        / 8000.0;
    assert!(
        (onset - tone).abs() <= frame,
        "the MP4's tone at {onset:.3} s, the frame at {tone:.3} s"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_audio_the_program_is_not_asked_and_nothing_changes() {
    if !has("python3") {
        eprintln!("skipped: python3 is not installed");
        return;
    }
    let dir = scratch("off");
    let recording = record::record(&tape(&dir, false), "quiet", &Options::default()).unwrap();
    assert!(recording
        .text_grid(&recording.shots[0].1)
        .contains("AUDIO unset"));
    assert!(recording.audio.is_none());
    let out = dir.join("out");
    let written = record::write(
        &recording,
        &out,
        "quiet",
        Formats::DEFAULT,
        &Fonts::embedded(),
        &Default::default(),
        None,
    )
    .unwrap();
    assert!(written
        .iter()
        .all(|p| p.extension().is_none_or(|e| e != "wav")));
    // `Options::audio` turns it on for a tape that does not.
    let options = Options {
        audio: true,
        ..Options::default()
    };
    let asked = record::record(&tape(&dir, false), "asked", &options).unwrap();
    assert!(asked.text_grid(&asked.shots[0].1).contains("AUDIO set"));
    assert!(asked.audio.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
