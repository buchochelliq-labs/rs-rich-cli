//! Native plugins: the example `cdylib` is built with cargo, loaded through
//! the C ABI and used through the registry; a future ABI is refused.
#![cfg(feature = "dylib-plugins")]

use std::path::PathBuf;
use std::sync::OnceLock;

use rich::{Console, Text};
use rich_ext::plugin::abi::{AbiError, CapabilityKind};
use rich_ext::plugin_loading::{load, load_native, LoadError, LoadOptions, RuntimeKind};
use rich_ext::ExtensionRegistry;

include!("../../rich-plugin-api/examples/dylib-plugin/build_fixture.rs");

fn tmp() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dylib-plugins")
}

/// The example plugin, and the same crate built for a future ABI.
fn libraries() -> &'static (PathBuf, PathBuf) {
    static BUILT: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    BUILT.get_or_init(|| {
        let good = build_example_dylib(&tmp(), &[]);
        let future = build_example_dylib(&tmp(), &["future-abi"]);
        (good, future)
    })
}

#[test]
fn the_example_dylib_loads_and_its_capabilities_work() {
    let plugin = load(&libraries().0, &LoadOptions::default()).unwrap();
    assert_eq!(plugin.kind(), RuntimeKind::Native);
    let abi = plugin.abi();
    assert_eq!(abi.name, "example-dylib");
    assert_eq!(abi.version, "0.1.0");
    let kinds: Vec<_> = abi.capabilities.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            CapabilityKind::Transform,
            CapabilityKind::Highlighter,
            CapabilityKind::FenceMarkup,
            CapabilityKind::FenceAnsi
        ]
    );

    let mut registry = ExtensionRegistry::with_defaults();
    registry.add_plugin(&plugin).unwrap();
    // The registry keeps the library loaded after the plugin value is gone.
    drop(plugin);
    let reverse = registry.transform("reverse").unwrap();
    assert_eq!(
        reverse.transform(Text::new("abc\nxyz")).unwrap().plain(),
        "cba\nzyx"
    );

    let mut console = Console::builder()
        .width(12)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .force_terminal(true)
        .build();
    registry.install(&mut console);
    let text = console.render_str("route 66", Some(true));
    let magenta = rich::Style::parse("bold magenta").unwrap();
    assert!(
        text.spans()
            .iter()
            .any(|span| (span.start, span.end) == (6, 7) && span.style == magenta.clone().into()),
        "{:?}",
        text.spans()
    );

    let fences = registry.fences().unwrap();
    let options = console.options();
    let plain = |language: &str, code: &str| {
        fences
            .render_fence(language, code, &console, &options)
            .map(|segments| segments.iter().map(|s| s.text.as_str()).collect::<String>())
    };
    let banner = plain("banner", "hi").unwrap();
    assert_eq!(
        banner.lines().collect::<Vec<_>>(),
        ["=".repeat(12).as_str(), "hi", &"=".repeat(12)]
    );
    // The plugin's error leaves the fence to render as code.
    assert_eq!(plain("banner", " "), None);
    // ANSI output keeps its colour but not the window-title escape.
    let red = plain("red", "stop").unwrap();
    assert_eq!(red.trim_end(), "stop");
    let segments = fences
        .render_fence("red", "stop", &console, &options)
        .unwrap();
    assert!(segments
        .iter()
        .any(|s| s.text == "stop" && s.style.as_ref().is_some_and(|st| st.color().is_some())));
}

#[test]
fn a_dylib_built_for_another_abi_major_is_refused() {
    let error = load_native(&libraries().1).unwrap_err();
    assert!(
        matches!(
            error,
            LoadError::Abi {
                error: AbiError::Incompatible { major: 2, minor: 0 },
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("rebuild"), "{error}");
}

#[test]
fn a_missing_library_is_an_error_not_a_panic() {
    let error = load_native(&tmp().join("missing.so")).unwrap_err();
    assert!(matches!(error, LoadError::Io { .. }), "{error}");
    assert!(error.to_string().contains("missing.so"));
    // A file that is not a library at all.
    std::fs::create_dir_all(tmp()).unwrap();
    let fake = tmp().join("fake.so");
    std::fs::write(&fake, b"not a library").unwrap();
    // It exists, so it is not an I/O error but not a plugin, with the system
    // loader's reason rather than a bare "dlopen failed".
    let error = load_native(&fake).unwrap_err();
    assert!(matches!(error, LoadError::NotAPlugin { .. }), "{error}");
    if cfg!(target_os = "linux") {
        assert!(error.to_string().contains("file too short"), "{error}");
    }
}
