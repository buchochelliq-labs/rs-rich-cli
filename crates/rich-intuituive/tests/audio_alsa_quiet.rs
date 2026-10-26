//! ALSA's messages are kept off stderr (the screen) for an output's
//! whole life, not only while it is opened. A child process opens an
//! output on ALSA's `file` plugin, writing to a path it cannot: the
//! plugin complains when the stream starts, on cpal's stream thread.

#![cfg(target_os = "linux")]

use std::process::Command;
use std::time::Duration;

use intuituive::audio::{messages, output, OutputOptions};

const CHILD: &str = "INTUITUIVE_ALSA_QUIET_CHILD";

#[test]
fn an_error_after_opening_is_kept_not_written() {
    if std::env::var_os(CHILD).is_some() {
        // The child: open, play a little, report what was kept.
        match output(OutputOptions::new().rate(8000).channels(1)) {
            Ok(out) => {
                out.write(&[0.1; 4000]);
                std::thread::sleep(Duration::from_millis(600));
                println!("opened; failed: {:?}", out.failed());
            }
            Err(e) => println!("not opened: {e}"),
        }
        for line in messages() {
            println!("kept: {line}");
        }
        return;
    }
    let home = std::env::temp_dir().join(format!("intuituive-alsa-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join(".asoundrc"),
        "pcm.!default {\n  type file\n  slave.pcm \"null\"\n  file \"/nonexistent-intuituive/out.raw\"\n  format \"raw\"\n}\n",
    )
    .unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "an_error_after_opening_is_kept_not_written",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env("HOME", &home)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&home).ok();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr),
    );
    println!("child stdout:\n{stdout}\nchild stderr:\n{stderr}");
    assert!(child.status.success());
    assert!(!stderr.contains("ALSA lib"), "{stderr}");
    assert!(stdout.contains("kept: ALSA lib"), "{stdout}");
}
