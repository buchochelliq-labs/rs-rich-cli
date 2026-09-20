//! Real process and PTY tests for interactive progress and interruption.
#[cfg(target_os = "linux")]
#[test]
fn batch_cancellation_reaps_workers_and_preserves_progress_channels() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/test_batch_v9.py");
    let output = std::process::Command::new("python3")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_rich"))
        .output()
        .expect("Python 3 is required for the Linux process/PTY regression tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
