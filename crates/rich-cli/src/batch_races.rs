//! Deterministic swaps at the boundary between planning and export publication.
use super::*;

fn race(output: &Path, overwrite: bool, mutate: impl FnOnce()) -> std::io::Result<()> {
    let root = batch_output::OutputRoot::capture(output.parent().unwrap()).unwrap();
    let destination = root.destination(output, false).unwrap();
    mutate();
    destination.publish(&mut &b"rendered export"[..], overwrite)
}

#[test]
fn late_hard_link_is_never_truncated_by_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let victim = temp.path().join("victim.txt");
    let output = temp.path().join("out.html");
    std::fs::write(&victim, "protected").unwrap();
    let result = race(&output, true, || {
        std::fs::hard_link(&victim, &output).unwrap()
    });
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "protected",
        "{result:?}"
    );
}

#[test]
fn late_collision_is_not_overwritten_without_permission() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out.html");
    let result = race(&output, false, || {
        std::fs::write(&output, "other writer").unwrap()
    });
    assert!(result.is_err(), "{result:?}");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "other writer");
}

#[cfg(unix)]
#[test]
fn late_leaf_symlink_cannot_redirect_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let victim = temp.path().join("victim.txt");
    let output = temp.path().join("out.html");
    std::fs::write(&victim, "protected").unwrap();
    let result = race(&output, true, || {
        std::os::unix::fs::symlink(&victim, &output).unwrap()
    });
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "protected",
        "{result:?}"
    );
}

#[cfg(unix)]
#[test]
fn late_parent_symlink_cannot_redirect_export() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    let moved = temp.path().join("moved");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&out).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let victim = outside.join("out.html");
    std::fs::write(&victim, "protected").unwrap();
    let result = race(&out.join("out.html"), true, || {
        std::fs::rename(&out, &moved).unwrap();
        std::os::unix::fs::symlink(&outside, &out).unwrap();
    });
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "protected",
        "{result:?}"
    );
    result.unwrap();
    assert_eq!(
        std::fs::read_to_string(moved.join("out.html")).unwrap(),
        "rendered export"
    );
}

#[cfg(unix)]
#[test]
fn replaced_subdirectory_is_rejected_before_missing_parents_are_created() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(out.join("nested")).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let root = batch_output::OutputRoot::capture(&out).unwrap();
    std::fs::remove_dir(out.join("nested")).unwrap();
    std::os::unix::fs::symlink(&outside, out.join("nested")).unwrap();
    assert!(root
        .destination(&out.join("nested/new/export.html"), true)
        .is_err());
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn missing_root_uses_retained_ancestor_after_path_swap() {
    let temp = tempfile::tempdir().unwrap();
    let ancestor = temp.path().join("ancestor");
    let moved = temp.path().join("moved");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&ancestor).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let root = batch_output::OutputRoot::capture(&ancestor.join("new")).unwrap();
    assert!(!ancestor.join("new").exists());
    std::fs::rename(&ancestor, &moved).unwrap();
    std::os::unix::fs::symlink(&outside, &ancestor).unwrap();
    root.destination(&ancestor.join("new/nested/export.html"), true)
        .unwrap()
        .publish(&mut &b"export"[..], false)
        .unwrap();
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
    assert_eq!(
        std::fs::read(moved.join("new/nested/export.html")).unwrap(),
        b"export"
    );
}

#[cfg(unix)]
#[test]
fn dangling_leaf_is_a_collision_and_its_target_is_not_created() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("missing");
    let output = temp.path().join("out.html");
    let result = race(&output, false, || {
        std::os::unix::fs::symlink(&target, &output).unwrap()
    });
    assert!(result.is_err());
    assert!(!target.exists());
    assert_eq!(std::fs::read_link(output).unwrap(), target);
}

#[test]
fn failed_overwrite_copy_preserves_existing_export() {
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("source read failed"))
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out.html");
    std::fs::write(&output, "original").unwrap();
    let root = batch_output::OutputRoot::capture(temp.path()).unwrap();
    let destination = root.destination(&output, false).unwrap();
    assert!(destination.publish(&mut Broken, true).is_err());
    assert_eq!(std::fs::read_to_string(output).unwrap(), "original");
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn successful_overwrite_replaces_entry_and_leaves_other_hard_links_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let victim = temp.path().join("victim");
    let output = temp.path().join("out.html");
    std::fs::write(&victim, "protected").unwrap();
    let result = race(&output, true, || {
        std::fs::hard_link(&victim, &output).unwrap()
    });
    result.unwrap();
    assert_eq!(std::fs::read_to_string(victim).unwrap(), "protected");
    assert_eq!(std::fs::read_to_string(output).unwrap(), "rendered export");
}

#[cfg(windows)]
#[test]
fn junction_below_acquired_root_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("out");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&out).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let root = batch_output::OutputRoot::capture(&out).unwrap();
    let result = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(out.join("nested"))
        .arg(&outside)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    assert!(root
        .destination(&out.join("nested/new/export.html"), true)
        .is_err());
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn overwrite_does_not_make_private_export_world_readable() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("private.html");
    std::fs::write(&output, "private").unwrap();
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o600)).unwrap();
    race(&output, true, || {}).unwrap();
    assert_eq!(
        std::fs::metadata(output).unwrap().permissions().mode() & 0o077,
        0
    );
}

#[test]
fn interruption_during_copy_does_not_commit_overwrite() {
    let cancelled = std::cell::Cell::new(false);
    struct Interrupting<'a>(&'a std::cell::Cell<bool>);
    impl std::io::Read for Interrupting<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.0.replace(true) {
                return Ok(0);
            }
            buffer[0] = b'x';
            Ok(1)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("out.html");
    std::fs::write(&output, "original").unwrap();
    let root = batch_output::OutputRoot::capture(temp.path()).unwrap();
    let destination = root.destination(&output, false).unwrap();
    let result = destination.publish_checked(&mut Interrupting(&cancelled), true, || {
        if cancelled.get() {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "cancelled",
            ))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(output).unwrap(), "original");
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn replacement_has_protected_owner_only_windows_acl() {
    let temp = tempfile::tempdir().unwrap();
    let output = temp.path().join("private.html");
    std::fs::write(&output, "old").unwrap();
    race(&output, true, || {}).unwrap();
    let result = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-Acl -LiteralPath $env:RICH_TEST_PATH).Sddl",
        ])
        .env("RICH_TEST_PATH", &output)
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    let descriptor = String::from_utf8(result.stdout).unwrap();
    assert!(descriptor.contains("D:P"), "{descriptor}");
    let dacl = descriptor.split("D:").nth(1).unwrap();
    assert_eq!(dacl.matches('(').count(), 1, "{descriptor}");
    assert!(dacl.contains("(A;;FA;;;OW)"), "{descriptor}");
}

#[cfg(unix)]
#[test]
fn temporary_is_private_before_source_bytes_are_read() {
    use std::os::unix::fs::PermissionsExt;
    struct Inspect<'a>(&'a Path);
    impl std::io::Read for Inspect<'_> {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            let entries: Vec<_> = std::fs::read_dir(self.0)?.collect::<Result<_, _>>()?;
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].metadata()?.permissions().mode() & 0o077, 0);
            Ok(0)
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = batch_output::OutputRoot::capture(temp.path()).unwrap();
    root.destination(&temp.path().join("out.html"), false)
        .unwrap()
        .publish(&mut Inspect(temp.path()), true)
        .unwrap();
}
