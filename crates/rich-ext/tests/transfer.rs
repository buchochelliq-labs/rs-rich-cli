//! Download and upload progress helpers (#390).
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rich::progress::TaskUpdate;
use rich::{ColorSystem, Console, Progress};
use rich_ext::a11y::SymbolSet;
use rich_ext::cancel::CancelToken;
use rich_ext::transfer::{
    is_cancelled, transfer_columns, Clock, Direction, Transfer, TransferReader, TransferState,
    TransferWriter, Transfers,
};

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn plain(width: usize, renderable: &dyn rich::Renderable) -> String {
    Console::builder()
        .width(width)
        .build()
        .render_to_string(renderable)
}

fn color(width: usize, renderable: &dyn rich::Renderable) -> String {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build()
        .render_to_string(renderable)
}

/// 45.6 MB, 12.3 MB done at a steady 2.4 MB/s.
fn midway() -> Transfer {
    let mut t = Transfer::download("release.tar.gz").total(45_600_000);
    t.set_completed(12_300_000 - 2_400_000 * 2, secs(8));
    t.set_completed(12_300_000 - 2_400_000, secs(9));
    t.set_completed(12_300_000, secs(10));
    t
}

#[test]
fn rate_is_smoothed_over_the_window_and_eta_follows() {
    let mut t = Transfer::download("x").total(1_000).rate_window(secs(2));
    assert_eq!((t.rate(), t.eta()), (None, None));
    t.advance(100, secs(0));
    t.advance(100, secs(1));
    t.advance(100, secs(2));
    assert_eq!(t.rate(), Some(100.0));
    // A burst lifts the rate, then drops out of the window.
    t.advance(400, secs(3));
    assert_eq!(t.rate(), Some(250.0));
    t.advance(0, secs(5));
    t.advance(0, secs(6));
    assert_eq!(t.rate(), Some(0.0));
    assert_eq!(t.eta(), None, "no ETA at a standstill");
    t.advance(100, secs(7));
    assert_eq!(t.rate(), Some(50.0));
    assert_eq!(t.eta(), Some(secs(4)), "200 left at 50/s");
}

#[test]
fn plain_line_spells_everything_out() {
    assert_eq!(
        plain(80, &midway()),
        "↓ release.tar.gz  [#####.................]  12.3/45.6 MB  2.4 MB/s  ETA 0:00:14"
    );
}

#[test]
fn colour_line_uses_the_core_bar_and_transfer_styles() {
    let expected = concat!(
        "\u{1b}[36m↓\u{1b}[0m \u{1b}[1mrelease.tar.gz\u{1b}[0m  ",
        "\u{1b}[38;2;249;38;114m━━━━━\u{1b}[0m\u{1b}[38;2;249;38;114m╸\u{1b}[0m",
        "\u{1b}[38;5;237m━━━━━━━━━━━━━━━━\u{1b}[0m  ",
        "\u{1b}[32m12.3/45.6 MB\u{1b}[0m  \u{1b}[31m2.4 MB/s\u{1b}[0m  ",
        "\u{1b}[36mETA 0:00:14\u{1b}[0m",
    );
    assert_eq!(color(80, &midway().bar_width(22)), expected);
}

#[test]
fn bar_shrinks_then_disappears_when_narrow() {
    assert_eq!(
        plain(64, &midway()),
        "↓ release.tar.gz  [#......]  12.3/45.6 MB  2.4 MB/s  ETA 0:00:14"
    );
    assert_eq!(
        plain(60, &midway()),
        "↓ release.tar.gz  12.3/45.6 MB  2.4 MB/s  ETA 0:00:14"
    );
    assert_eq!(
        plain(50, &midway()),
        "↓ release.tar.gz  12.3/45.6 MB  2.4 MB/s  ETA 0:00"
    );
}

#[test]
fn unknown_total_shows_bytes_and_an_indeterminate_bar() {
    let mut t = Transfer::upload("stream").bar_width(8);
    t.advance(1_500, secs(0));
    t.advance(1_500, secs(1));
    assert_eq!(t.fraction(), None);
    assert_eq!(
        plain(60, &t),
        "↑ stream  [------]  3.0 kB  1.5 kB/s  ETA -:--:--"
    );
    // With colour the bar is the core's pulse, positioned by the last update.
    let out = color(60, &t);
    assert!(out.contains("━"), "{out:?}");
}

#[test]
fn states_are_marked_in_words_and_symbols() {
    let mut retrying = Transfer::download("a").total(100).max_attempts(3);
    retrying.advance(40, secs(1));
    assert!(retrying.retry("connection reset"));
    let mut resumed = retrying.clone();
    resumed.advance(10, secs(2));
    let mut paused = Transfer::download("b").total(100);
    paused.pause(secs(1));
    let mut done = Transfer::download("c").total(100);
    done.finish(secs(3));
    let mut failed = Transfer::download("d").total(100);
    failed.fail("404 not found");
    let mut cancelled = Transfer::download("e").total(100);
    cancelled.cancel();

    let line = |t: &Transfer, set| plain(60, &t.clone().symbols(set).bar_width(0));
    assert_eq!(
        line(&retrying, SymbolSet::Unicode),
        "↓ a  40/100 bytes  -  ↻ retrying 2/3"
    );
    assert_eq!(
        line(&resumed, SymbolSet::Unicode),
        "↓ a  50/100 bytes  -  ETA -:--:-- (attempt 2/3)"
    );
    assert_eq!(
        line(&paused, SymbolSet::Ascii),
        "v b  0/100 bytes  -  [PAUSED]"
    );
    assert_eq!(
        line(&done, SymbolSet::Words),
        "down c  100/100 bytes  -  done"
    );
    assert_eq!(
        line(&failed, SymbolSet::Unicode),
        "↓ d  0/100 bytes  -  ✖ failed: 404 not found"
    );
    assert_eq!(
        line(&cancelled, SymbolSet::Ascii),
        "v e  0/100 bytes  -  [CANCELLED]"
    );
    assert_eq!(Direction::Upload.symbol(SymbolSet::Words), "up");
    assert_eq!(
        TransferState::Retrying.status(),
        rich_ext::a11y::Status::Warning
    );
}

#[test]
fn ascii_console_downgrades_symbols() {
    let console = Console::builder().width(60).ascii_only(true).build();
    let mut t = Transfer::upload("u").total(10).bar_width(0);
    t.finish(secs(1));
    assert_eq!(console.render_to_string(&t), "^ u  10/10 bytes  -  [DONE]");
}

#[test]
fn group_aligns_columns_and_sums_totals() {
    let mut group = Transfers::new().summary(true);
    let a = group.push(Transfer::download("small.txt").total(2_000));
    assert_eq!(
        plain(48, &Transfer::download("n").total(1_234)).trim_end(),
        "↓ n  [.............]  0.0/1.2 kB  -  ETA -:--:--"
    );
    assert_eq!(
        plain(40, &Transfer::download("n").total(999).bar_width(0)),
        "↓ n  0/999 bytes  -  ETA -:--:--"
    );
    let b = group.push(Transfer::upload("backup.tar").total(3_000_000));
    let c = group.push(Transfer::download("gone").total(1_000).max_attempts(2));
    group[a].advance(1_000, secs(0));
    group[a].advance(1_000, secs(1));
    group[a].finish(secs(1));
    group[b].advance(0, secs(0));
    group[b].advance(1_000_000, secs(1));
    group[c].retry("timeout");
    assert!(!group.finished());
    assert_eq!(group.rate(), Some(1_000_000.0));
    assert_eq!(
        plain(72, &group),
        concat!(
            "↓ small.txt   [##################]  2.0/2.0 kB         -  ✔ done\n",
            "↑ backup.tar  [######............]  1.0/3.0 MB  1.0 MB/s  ETA 0:00:02\n",
            "↓ gone        [..................]  0.0/1.0 kB         -  ↻ retrying 2/2\n",
            "3 transfers, 1 done, 1 retrying  1.0/3.0 MB  1.0 MB/s",
        )
    );
}

#[test]
fn group_summary_saturates_huge_byte_counts() {
    let mut group = Transfers::new().summary(true);
    for name in ["a", "b"] {
        let i = group.push(Transfer::download(name).total(u64::MAX));
        group[i].advance(u64::MAX, secs(0));
    }
    let out = plain(72, &group);
    let summary = out.lines().last().unwrap();
    assert_eq!(summary, "2 transfers  18.4/18.4 EB", "{out}");
}

#[test]
fn task_update_drives_core_progress() {
    let mut progress = Progress::new().columns(transfer_columns()).clock(|| 0.0);
    let mut t = Transfer::download("data.bin")
        .total(2_000_000)
        .max_attempts(3);
    let task = progress.add_task("data.bin", 0.0, 0.0);
    t.advance(500_000, secs(1));
    t.retry("reset");
    progress.update(task, t.task_update());
    let view = progress.task(task).unwrap();
    assert_eq!(view.total(), Some(2_000_000.0));
    assert_eq!(view.completed(), 500_000.0);
    assert_eq!(view.description(), "data.bin (retry 2/3)");
    // Names are escaped: they are shown as console markup.
    let update: TaskUpdate = Transfer::download("[x]").task_update();
    assert_eq!(update.description.as_deref(), Some("\\[x]"));
    assert_eq!(transfer_columns().len(), 5);
}

fn manual_clock() -> (Clock, Arc<AtomicU64>) {
    let millis = Arc::new(AtomicU64::new(0));
    let reader = millis.clone();
    let clock: Clock = Arc::new(move || Duration::from_millis(reader.load(Ordering::SeqCst)));
    (clock, millis)
}

#[test]
fn reader_counts_finishes_and_cancels() {
    let (clock, millis) = manual_clock();
    let shared = Arc::new(Mutex::new(Transfer::download("blob").total(10)));
    let token = CancelToken::new();
    let mut reader = TransferReader::new(&b"0123456789"[..], shared.clone())
        .cancel(token.clone())
        .clock(clock);
    let mut buf = [0u8; 4];
    assert_eq!(reader.read(&mut buf).unwrap(), 4);
    millis.store(1000, Ordering::SeqCst);
    assert_eq!(reader.read(&mut buf).unwrap(), 4);
    {
        let t = shared.lock().unwrap();
        assert_eq!(t.completed(), 8);
        assert_eq!(t.rate(), Some(4.0));
    }
    token.cancel();
    let error = reader.read(&mut buf).unwrap_err();
    assert!(is_cancelled(&error));
    assert_eq!(error.to_string(), "transfer cancelled");
    assert_eq!(shared.lock().unwrap().state(), TransferState::Cancelled);
    assert_eq!(shared.lock().unwrap().completed(), 8);
}

#[test]
fn reader_error_marks_the_transfer_failed() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("connection reset"))
        }
    }
    let shared = Arc::new(Mutex::new(Transfer::download("x")));
    let mut reader = TransferReader::new(Broken, shared.clone());
    assert!(!is_cancelled(&reader.read(&mut [0; 1]).unwrap_err()));
    let t = shared.lock().unwrap();
    assert_eq!(t.state(), TransferState::Failed);
    assert_eq!(t.error(), Some("connection reset"));
}

#[test]
fn writer_counts_and_io_copy_stops_on_cancel() {
    let (clock, _) = manual_clock();
    let shared = Arc::new(Mutex::new(Transfer::upload("up").total(6)));
    let mut writer = TransferWriter::new(Vec::new(), shared.clone()).clock(clock.clone());
    writer.write_all(b"abc").unwrap();
    writer.flush().unwrap();
    assert_eq!(shared.lock().unwrap().completed(), 3);
    assert_eq!(writer.into_inner(), b"abc");

    // io::copy does not spin on the cancellation error.
    let token = CancelToken::new();
    token.cancel();
    let mut writer = TransferWriter::new(Vec::new(), shared.clone())
        .cancel(token)
        .clock(clock);
    let error = std::io::copy(&mut &b"xyz"[..], &mut writer).unwrap_err();
    assert!(is_cancelled(&error));
    assert_eq!(shared.lock().unwrap().state(), TransferState::Cancelled);
}
