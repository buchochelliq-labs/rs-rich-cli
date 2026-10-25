//! `ChromeEngine::end_all`: the browsers of engines that are never
//! dropped (an app's state that lasts until the process exits) end, and
//! their temporary profiles go; a kept profile stays. Against a stand-in
//! browser, a script, so no Chrome is needed. A test file of its own,
//! since `end_all` ends every browser of the process.

#![cfg(all(feature = "chrome", unix))]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rich_embed::{ChromeEngine, WebEngine};

/// A stand-in browser that writes down its profile and its pid, then idles.
fn stand_in(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("chrome");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             for a in \"$@\"; do case \"$a\" in --user-data-dir=*) \
             printf '%s' \"${{a#--user-data-dir=}}\" > '{dir}/profile.tmp' && \
             mv '{dir}/profile.tmp' '{dir}/profile';; esac; done\n\
             echo $$ > '{dir}/pid.tmp' && mv '{dir}/pid.tmp' '{dir}/pid'\n\
             exec sleep 30\n",
            dir = dir.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn wait_for(path: &Path) -> String {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{path:?} never came"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::read_to_string(path).unwrap().trim().to_string()
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn end_all_ends_the_browsers_and_removes_their_temporary_profiles() {
    let dir = std::env::temp_dir().join(format!("rich-embed-end-all-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (first, second) = (dir.join("first"), dir.join("second"));
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let kept = dir.join("kept");

    let mut temporary = ChromeEngine::new().binary(stand_in(&first));
    temporary.resize(80, 24).unwrap();
    let mut keeping = ChromeEngine::new().binary(stand_in(&second)).profile(&kept);
    keeping.resize(80, 24).unwrap();
    let profile = PathBuf::from(wait_for(&first.join("profile")));
    let pids = [wait_for(&first.join("pid")), wait_for(&second.join("pid"))];
    assert!(profile.is_dir());
    assert!(pids.iter().all(|pid| alive(pid)));

    // Both engines live on, as in an app's state.
    ChromeEngine::end_all();
    assert!(!profile.exists(), "the temporary profile is still there");
    assert!(kept.is_dir(), "a kept profile stays");
    for pid in &pids {
        let start = Instant::now();
        while alive(pid) {
            assert!(start.elapsed() < Duration::from_secs(5), "{pid} still runs");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // Dropping them afterwards is harmless.
    drop(temporary);
    drop(keeping);
    std::fs::remove_dir_all(&dir).unwrap();
}
