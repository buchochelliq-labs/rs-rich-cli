//! The voice mixer: clips on channels at their own rates, gains and pans,
//! mixed block by block, and into a null output on its own thread.

use std::sync::Arc;
use std::time::{Duration, Instant};

use intuituive::audio::{clip, output, Clip, Mixer, OutputOptions};

/// `frames` frames of a mixer's output, in blocks of 256.
fn mixed(mixer: &Mixer, frames: usize) -> Vec<f32> {
    let channels = usize::from(mixer.channels());
    let mut out = vec![0.0f32; frames * channels];
    for block in out.chunks_mut(256 * channels) {
        mixer.mix(block);
    }
    out
}

fn noise(rate: u32, frames: usize, seed: u32) -> Arc<Clip> {
    let mut n = seed;
    let samples = (0..frames)
        .map(|_| {
            n = n.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            f64::from(n >> 16) / 65_536.0 - 0.5
        })
        .collect();
    Arc::new(clip(rate, vec![samples]))
}

#[test]
fn two_clips_started_together_sum_sample_for_sample() {
    let (a, b) = (noise(44_100, 3000, 1), noise(44_100, 2000, 2));
    let alone = |clip: &Arc<Clip>, channel| {
        let mixer = Mixer::detached(44_100, 2);
        mixer.play(channel, clip.clone(), 0.7, 0.3);
        mixed(&mixer, 4096)
    };
    let both = Mixer::detached(44_100, 2);
    both.play(0, a.clone(), 0.7, 0.3);
    both.play(1, b.clone(), 0.7, 0.3);
    let sum: Vec<f32> = alone(&a, 0)
        .iter()
        .zip(alone(&b, 1))
        .map(|(x, y)| x + y)
        .collect();
    assert_eq!(mixed(&both, 4096), sum);
}

#[test]
fn a_clip_at_a_quarter_of_the_rate_lasts_four_times_as_long() {
    let mixer = Mixer::detached(44_100, 1);
    let voice = mixer.play(3, Arc::new(clip(11_025, vec![vec![0.5; 1000]])), 1.0, 0.0);
    let out = mixed(&mixer, 5000);
    let last = out.iter().rposition(|s| *s != 0.0).unwrap();
    assert_eq!(last, 3999);
    assert!(!voice.is_playing());
}

#[test]
fn pan_left_is_left_only_and_stop_silences_the_next_block() {
    let mixer = Mixer::detached(8000, 2);
    let voice = mixer.play(0, Arc::new(clip(8000, vec![vec![0.5; 8000]])), 1.0, -1.0);
    let first = mixed(&mixer, 256);
    assert!(first.chunks(2).all(|f| f == [0.5, 0.0]));
    voice.set(0.5, 1.0);
    assert!(mixed(&mixer, 256).chunks(2).all(|f| f == [0.0, 0.25]));
    assert!(voice.is_playing());
    voice.stop();
    assert!(!voice.is_playing());
    assert!(mixed(&mixer, 256).iter().all(|s| *s == 0.0));
    // A new sound on the channel; the old voice no longer reaches it.
    let next = mixer.play(0, Arc::new(clip(8000, vec![vec![0.1; 10]])), 1.0, 0.0);
    voice.stop();
    assert!(next.is_playing());
}

#[test]
fn is_playing_ends_within_a_block_of_the_clips_end() {
    let mixer = Mixer::detached(8000, 2);
    let voice = mixer.play(0, Arc::new(clip(8000, vec![vec![0.5; 300]])), 1.0, 0.0);
    mixed(&mixer, 256);
    assert!(voice.is_playing());
    mixed(&mixer, 256);
    assert!(!voice.is_playing());
}

#[test]
fn a_stream_plays_under_the_voices() {
    let mixer = Mixer::detached(8000, 2);
    mixer.source(Box::new(clip(
        4000,
        vec![vec![0.2; 2500], vec![-0.2; 2500]],
    )));
    mixer.play(0, Arc::new(clip(8000, vec![vec![0.1; 100]])), 1.0, 0.0);
    let out = mixed(&mixer, 6000);
    assert!(
        (out[0] - 0.3).abs() < 1e-6 && (out[1] + 0.1).abs() < 1e-6,
        "{:?}",
        &out[..2]
    );
    assert!((out[2 * 1000] - 0.2).abs() < 1e-6);
    // 2500 frames at half the rate: 5000 frames.
    assert!(out[2 * 4998].abs() > 0.0);
    assert_eq!(out[2 * 5000], 0.0);
}

#[test]
fn a_mixer_plays_into_an_output_on_its_own_thread() {
    let null = output(OutputOptions::null().rate(8000).channels(2)).unwrap();
    let tap = null.tap(Duration::from_secs(2));
    let mixer = Mixer::new(&null);
    let voice = mixer.play(0, Arc::new(clip(8000, vec![vec![0.5; 800]])), 1.0, 1.0);
    let started = Instant::now();
    while voice.is_playing() && started.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!voice.is_playing());
    std::thread::sleep(Duration::from_millis(400));
    let samples = tap.samples();
    let right = samples.chunks(2).filter(|f| *f == [0.0, 0.5]).count();
    assert_eq!(right, 800);
}

#[test]
fn a_downsampled_stream_keeps_its_place_across_source_buffers() {
    // A ramp whose value is its frame number, at six times the mixer's
    // rate, in the clip's 1024-frame buffers: output frame n is source
    // frame 6n, also just after a buffer runs out.
    let ramp = (0..12_000).map(|k| f64::from(k) / 100_000.0).collect();
    let mixer = Mixer::detached(8000, 1);
    mixer.source(Box::new(clip(48_000, vec![ramp])));
    let out = mixed(&mixer, 1990);
    for (n, sample) in out.iter().enumerate() {
        let want = (6 * n) as f32 / 100_000.0;
        assert!(
            (sample - want).abs() < 1e-5,
            "frame {n}: {sample} is not source frame {}",
            6 * n
        );
    }
}
