// Builds the example native plugin for a test and returns the library's path.
// Included (`include!`) by the rs-rich-ext and rs-rich-cli dylib tests.
//
// The crate is copied into `tmp` with the workspace's Cargo.lock and built
// offline in its own target directory, so the versions match the workspace's
// and the build never waits on the cargo lock of the test that runs it.

/// Build the example with `features`, and return a copy of the library named
/// after them (so two builds can coexist).
#[allow(dead_code)]
fn build_example_dylib(tmp: &std::path::Path, features: &[&str]) -> std::path::PathBuf {
    use std::path::Path;
    let api = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rich-plugin-api")
        .canonicalize()
        .expect("the rs-rich-plugin-api crate");
    let source = api.join("examples/dylib-plugin");
    let crate_dir = tmp.join("example-dylib");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    let manifest = std::fs::read_to_string(source.join("Cargo.toml"))
        .unwrap()
        .replace(
            r#"path = "../..""#,
            &format!("path = {:?}", api.display().to_string()),
        );
    std::fs::write(crate_dir.join("Cargo.toml"), manifest).unwrap();
    std::fs::copy(source.join("src/lib.rs"), crate_dir.join("src/lib.rs")).unwrap();
    std::fs::copy(api.join("../../Cargo.lock"), crate_dir.join("Cargo.lock")).unwrap();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command
        .current_dir(&crate_dir)
        .args(["build", "--offline", "--lib", "--quiet"])
        .env("CARGO_TARGET_DIR", crate_dir.join("target"))
        // No debug info: a much smaller build, on machines short of disk.
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    if !features.is_empty() {
        command.args(["--features", &features.join(",")]);
    }
    let status = command.status().expect("run cargo");
    assert!(status.success(), "building the example plugin failed");
    let name = format!(
        "{}rich_plugin_example_dylib{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let built = crate_dir.join("target/debug").join(&name);
    let copy = tmp.join(format!(
        "{}{}{}",
        name.trim_end_matches(std::env::consts::DLL_SUFFIX),
        features.iter().map(|f| format!("-{f}")).collect::<String>(),
        std::env::consts::DLL_SUFFIX
    ));
    std::fs::copy(built, &copy).unwrap();
    copy
}
