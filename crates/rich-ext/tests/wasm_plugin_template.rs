//! The WASM plugin template (templates/wasm-plugin) expands, passes its own
//! unit tests, builds for wasm32-unknown-unknown, and the module passes the
//! checks `--plugin` and `rich plugin install` make: the sandbox loads it
//! (no imports, the ABI, the manifest), its manifest is the package's name
//! and version, and every capability registers and works.
//!
//! It needs the wasm32-unknown-unknown target. Without it the test says so
//! and passes, unless `RICH_WASM_TEMPLATE_TEST=require` is set, as CI's
//! "WASM plugins (release)" job sets it.
#![cfg(feature = "wasm-plugins")]

use std::path::{Path, PathBuf};
use std::process::Command;

use rich::{Console, Text};
use rich_ext::ExtensionRegistry;
use rich_ext::plugin::abi::{ABI_MAJOR, ABI_MINOR, CapabilityKind, is_supported};
use rich_ext::plugin_loading::{LoadOptions, RuntimeKind, load};

const TARGET: &str = "wasm32-unknown-unknown";
const NAME: &str = "my-plugin";

fn template() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../templates/wasm-plugin")
}

/// Copy the template into `to` as cargo-generate would for `NAME`: the
/// `.liquid` suffix dropped and `{{project-name}}` filled in, except in the
/// files its `cargo-generate.toml` excludes from Liquid.
fn expand(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        if path.is_dir() {
            if name != "target" {
                expand(&path, &to.join(&name));
            }
            continue;
        }
        if name == "cargo-generate.toml" {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let (name, bytes) = match name.strip_suffix(".liquid") {
            Some(stem) => {
                let text = String::from_utf8(bytes).unwrap();
                (
                    stem.to_string(),
                    text.replace("{{project-name}}", NAME).into_bytes(),
                )
            }
            None => (name, bytes),
        };
        std::fs::write(to.join(name), bytes).unwrap();
    }
}

/// The cargo running this test.
fn cargo_path() -> PathBuf {
    std::env::var_os("CARGO").map_or_else(|| "cargo".into(), PathBuf::from)
}

/// The rustc beside that cargo, so the template builds with the same
/// toolchain; else the one on `PATH`.
fn rustc_path() -> PathBuf {
    let rustc = cargo_path().with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    if rustc.is_file() {
        rustc
    } else {
        "rustc".into()
    }
}

fn cargo(dir: &Path, target_dir: &Path) -> Command {
    let mut command = Command::new(cargo_path());
    command
        .current_dir(dir)
        .env("RUSTC", rustc_path())
        .env("CARGO_TARGET_DIR", target_dir)
        // A coverage or sanitizer job's flags are for the host, not wasm.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET");
    command
}

/// Whether the toolchain has the standard library for `TARGET`.
fn has_target() -> bool {
    let Ok(out) = Command::new(rustc_path())
        .args(["--print", "target-libdir", "--target", TARGET])
        .output()
    else {
        return false;
    };
    let libdir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    out.status.success()
        && std::fs::read_dir(&libdir).is_ok_and(|mut entries| {
            entries.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("libstd-")))
        })
}

fn run(mut command: Command, what: &str) {
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "{what} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_template_builds_a_plugin_the_host_loads() {
    // Outside this workspace, so cargo does not take the template for a
    // member of it.
    let work = std::env::temp_dir().join(format!("rich-wasm-template-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let project = work.join(NAME);
    if !has_target() {
        let message = format!(
            "the {TARGET} target is not installed (rustup target add {TARGET}); \
             the WASM plugin template was not built"
        );
        assert!(
            std::env::var("RICH_WASM_TEMPLATE_TEST").as_deref() != Ok("require"),
            "{message}"
        );
        eprintln!("skipped: {message}");
        return;
    }
    expand(&template(), &project);
    let target_dir = work.join("target");

    let mut test = cargo(&project, &target_dir);
    test.args(["test", "--quiet"]);
    run(test, "the template's unit tests");

    // The template's own alias, `cargo wasm`.
    let mut build = cargo(&project, &target_dir);
    build.arg("wasm");
    run(build, "cargo wasm");
    let module = target_dir
        .join(TARGET)
        .join("release")
        .join(format!("{}.wasm", NAME.replace('-', "_")));

    // What `--plugin` and `rich plugin install` do: load it in the sandbox.
    let plugin = load(&module, &LoadOptions::default()).unwrap();
    assert_eq!(plugin.kind(), RuntimeKind::Wasm);
    let abi = plugin.abi();
    assert_eq!((abi.name.as_str(), abi.version.as_str()), (NAME, "0.1.0"));
    assert_eq!((abi.abi_major, abi.abi_minor), (ABI_MAJOR, ABI_MINOR));
    assert!(is_supported(abi.abi_major, abi.abi_minor));
    let kinds: Vec<_> = abi
        .capabilities
        .iter()
        .map(|c| (c.kind, c.name.as_str()))
        .collect();
    assert_eq!(
        kinds,
        [
            (CapabilityKind::Transform, "title-case"),
            (CapabilityKind::Highlighter, "todo"),
            (CapabilityKind::FenceMarkup, "callout"),
        ]
    );

    // Every capability registers beside the built-ins and works.
    let mut registry = ExtensionRegistry::with_defaults();
    registry.add_plugin(&plugin).unwrap();
    let title = registry.transform("title-case").unwrap();
    assert_eq!(
        title.transform(Text::new("hello wörld")).unwrap().plain(),
        "Hello Wörld"
    );

    let console = Console::builder().width(12).color_system(None).build();
    let fences = registry.fences().unwrap();
    let segments = fences
        .render_fence("callout", "a [b] c", &console, &console.options())
        .unwrap();
    let rendered: String = segments.iter().map(|s| s.text.as_str()).collect();
    let lines: Vec<&str> = rendered.lines().map(str::trim_end).collect();
    assert_eq!(lines, ["┃ Note", "┃ a [b] c", "────────────"]);

    // The highlighter, as `rich -p` runs it: through the console.
    let mut console = Console::builder()
        .width(40)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .force_terminal(true)
        .build();
    registry.install(&mut console);
    let text = console.render_str("x TODO y", Some(true));
    let span = text.spans().iter().find(|s| s.start == 2).unwrap();
    assert_eq!(span.end, 6);
    let rich::StyleType::Style(style) = span.style.clone() else {
        panic!("{:?}", span.style)
    };
    assert_eq!(style, rich::Style::parse("bold black on yellow").unwrap());

    let _ = std::fs::remove_dir_all(&work);
}
