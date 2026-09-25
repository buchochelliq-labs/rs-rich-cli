//! The plugin contract must stay small: a plugin depends on this crate and core
//! `rich`, never on `rs-rich-ext`. This fails if anything else becomes a normal
//! dependency.

#[test]
fn the_only_dependency_is_core() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("read manifest");
    let mut in_dependencies = false;
    let mut found = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_dependencies = line == "[dependencies]";
            continue;
        }
        if in_dependencies && !line.is_empty() && !line.starts_with('#') {
            found.push(line.split(['=', '.']).next().unwrap().trim().to_string());
        }
    }
    assert_eq!(found, ["rich"], "unexpected dependencies: {found:?}");
}
