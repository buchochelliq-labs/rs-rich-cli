//! The null output and its tap: sound tested with no sound card.

use std::time::{Duration, Instant};

use intuituive::audio::{output, OutputOptions};

#[test]
fn a_null_output_takes_its_rate_and_no_more_than_its_queue_ahead() {
    let null = output(OutputOptions::null().rate(44_100).channels(2)).unwrap();
    assert_eq!(null.device(), "null");
    let tap = null.tap(Duration::from_secs(2));
    let started = Instant::now();
    // A second of sound, written as fast as the queue (250 ms) lets it in.
    let second = vec![0.25f32; 44_100 * 2];
    assert!(null.write(&second));
    let writing = started.elapsed();
    assert!(writing >= Duration::from_millis(650), "{writing:?}");
    std::thread::sleep(Duration::from_millis(500));
    let (frames, elapsed) = (tap.frames(), started.elapsed().as_secs_f64());
    let expected = elapsed * 44_100.0;
    assert!(
        (frames as f64 - expected).abs() <= expected * 0.02 + 441.0,
        "{frames} frames in {elapsed:.3} s"
    );
    // Everything written was taken, in order, then silence.
    let samples = tap.samples();
    let first = samples.iter().position(|s| *s != 0.0).unwrap_or(0);
    assert!(samples[first..first + second.len()].iter().all(|s| *s == 0.25));
    assert!(samples[first + second.len()..].iter().all(|s| *s == 0.0));
}

#[test]
fn at_volume_zero_the_tap_holds_only_zeros() {
    let null = output(OutputOptions::null().rate(8000).channels(1)).unwrap();
    null.set_volume(0.0);
    let tap = null.tap(Duration::from_secs(1));
    assert!(null.write(&[0.9; 2000]));
    std::thread::sleep(Duration::from_millis(400));
    assert!(tap.frames() >= 2000);
    assert!(tap.samples().iter().all(|s| *s == 0.0));
}

#[test]
fn a_tap_keeps_what_it_is_capped_at() {
    let null = output(OutputOptions::null().rate(8000).channels(2)).unwrap();
    let tap = null.tap(Duration::from_secs(1));
    std::thread::sleep(Duration::from_millis(1400));
    assert!(tap.frames() > 8000);
    assert_eq!(tap.samples().len(), 8000 * 2);
    tap.clear();
    assert!(tap.samples().len() < 8000);
}

#[test]
fn a_tap_writes_a_wav_hound_reads_back() {
    let null = output(OutputOptions::null().rate(8000).channels(1)).unwrap();
    let tap = null.tap(Duration::from_millis(500));
    let ramp: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
    assert!(null.write(&ramp));
    std::thread::sleep(Duration::from_millis(300));
    null.stop();
    let path = std::env::temp_dir().join(format!("intuituive-tap-{}.wav", std::process::id()));
    tap.to_wav(&path).unwrap();
    let mut reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.spec().sample_rate, 8000);
    let back: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
    std::fs::remove_file(&path).ok();
    assert_eq!(back, tap.samples());
    let first = back.iter().position(|s| *s != 0.0).unwrap();
    assert_eq!(&back[first..first + 999], &ramp[1..]);
}
