//! An output opened under `rich record --audio`: what it plays goes to the
//! recorder's directory (`RICH_RECORD_AUDIO`) as well as to the device.
//! With no sound card that will play 8 kHz mono (CI, a container) it falls
//! back to a null output on the real clock, whose sound goes to the
//! recording alone: what is checked exactly here. The tee to a device is
//! tested in `audio::output`'s unit tests. One test alone in its binary,
//! as it sets the environment.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intuituive::audio::{output, OutputOptions, RECORD_AUDIO_ENV};

#[test]
fn an_output_under_the_recorder_writes_its_sound_there_timed() {
    let dir = std::env::temp_dir().join(format!("intuituive-record-audio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(RECORD_AUDIO_ENV, &dir);
    let before = SystemTime::now();
    // A device output; under the recorder it opens even with no device.
    let speaker = output(OutputOptions::new().rate(8000).channels(1)).unwrap();
    // With no device, the recorder's own null output.
    let fallback = speaker.device() == "rich record";
    assert_eq!((speaker.rate(), speaker.channels()), (8000, 1));
    std::thread::sleep(Duration::from_millis(100));
    assert!(speaker.write(&[0.5; 800]));
    std::thread::sleep(Duration::from_millis(300));
    let after = SystemTime::now();
    drop(speaker);
    std::env::remove_var(RECORD_AUDIO_ENV);

    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].extension().unwrap(), "pcm");
    let bytes = std::fs::read(&files[0]).unwrap();
    assert_eq!(&bytes[..8], b"RRAUDIO1");
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 8000);
    assert_eq!(u16::from_le_bytes(bytes[12..14].try_into().unwrap()), 1);
    let nanos = |t: SystemTime| t.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
    let started = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    assert!(nanos(before) <= started && started <= nanos(after));
    let samples: Vec<f32> = bytes[24..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    // Silence for the first tenth of a second, then the sound written.
    let first = samples.iter().position(|s| *s != 0.0).expect("the sound");
    if fallback {
        assert!(
            (600..=1400).contains(&first),
            "first sound at sample {first}"
        );
    }
    assert!(samples[first..first + 800].iter().all(|s| *s == 0.5));
    // About as many samples as the time it ran.
    let ran = (nanos(after) - started) as f64 / 1e9 * 8000.0;
    assert!(
        (samples.len() as f64) <= ran + 100.0,
        "{} in {ran}",
        samples.len()
    );
    assert!(samples.len() >= first + 800);
    let _ = std::fs::remove_dir_all(&dir);
}
