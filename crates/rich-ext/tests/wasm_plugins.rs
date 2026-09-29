//! WASM plugins: the example module loads and works, and the sandbox's
//! limits hold (fuel, memory, no imports, the ABI version).
#![cfg(feature = "wasm-plugins")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rich::{Console, Text};
use rich_ext::plugin::abi::{AbiError, CapabilityKind};
use rich_ext::plugin_loading::{load, load_wasm, LoadError, LoadOptions, RuntimeKind, WasmLimits};
use rich_ext::ExtensionRegistry;

const SHOUT: &str = include_str!("../../rich-plugin-api/examples/wasm/shout.wat");

/// Compile `wat` and write it to a file named `name` in this test's
/// directory.
fn module(name: &str, wat: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("wasm-plugins");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, wat::parse_str(wat).unwrap()).unwrap();
    path
}

/// A module with the given manifest text and `rich_plugin_call` body.
fn plugin(manifest: &str, call_body: &str, extra: &str) -> String {
    let escaped = manifest.replace('\n', "\\n");
    format!(
        r#"(module
  {extra}
  (memory (export "memory") 1)
  (data (i32.const 0) "{escaped}")
  (func (export "rich_plugin_alloc") (param i32) (result i32) (i32.const 4096))
  (func (export "rich_plugin_manifest") (result i64)
    (i64.const {len}))
  (func (export "rich_plugin_call") (param i32 i32 i32 i32) (result i64)
    {call_body}))"#,
        len = manifest.len()
    )
}

const LOOPS: &str = "rich-plugin-abi 1.0\nname loops\nversion 1\ncapability transform spin\n";

#[test]
fn the_example_module_loads_and_its_capabilities_work() {
    let path = module("shout.wasm", SHOUT);
    let plugin = load(&path, &LoadOptions::default()).unwrap();
    assert_eq!(plugin.kind(), RuntimeKind::Wasm);
    let abi = plugin.abi();
    assert_eq!(
        (abi.name.as_str(), abi.version.as_str()),
        ("shout", "0.1.0")
    );
    assert_eq!(abi.capabilities[0].kind, CapabilityKind::Transform);
    assert_eq!(abi.capabilities[1].kind, CapabilityKind::FenceMarkup);

    let mut registry = ExtensionRegistry::with_defaults();
    registry.add_plugin(&plugin).unwrap();
    let upper = registry.transform("upper").unwrap();
    assert_eq!(
        upper.transform(Text::new("hello, wörld")).unwrap().plain(),
        "HELLO, WöRLD"
    );
    // Fresh instance per call: a second call sees no state from the first.
    assert_eq!(upper.transform(Text::new("x")).unwrap().plain(), "X");

    let console = Console::builder().width(30).color_system(None).build();
    let fences = registry.fences().unwrap();
    let segments = fences
        .render_fence("shout", "loud", &console, &console.options())
        .unwrap();
    let rendered: String = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(rendered.trim_end(), "loud");
    assert!(segments
        .iter()
        .any(|s| s.text == "loud" && s.style == Some(rich::Style::parse("bold").unwrap())));

    // An unknown capability number is the plugin's error, not a crash.
    let error = plugin.call(7, "x", 0).unwrap_err();
    assert_eq!(error, "unknown capability");
}

#[test]
fn a_module_that_loops_forever_is_stopped_by_fuel() {
    let spin = plugin(LOOPS, "(loop $l (br $l)) (i64.const 0)", "");
    let path = module("loops.wasm", &spin);
    let limits = WasmLimits {
        fuel: 1_000_000,
        ..WasmLimits::default()
    };
    let plugin = load_wasm(&path, &limits).unwrap();
    let started = Instant::now();
    let error = plugin.call(0, "x", 0).unwrap_err();
    assert!(error.contains("ran out of fuel"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(10));

    // Through the registry, the transform fails with that message.
    let mut registry = ExtensionRegistry::new();
    registry.add_plugin(&plugin).unwrap();
    let error = registry
        .transform("spin")
        .unwrap()
        .transform(Text::new("x"))
        .unwrap_err();
    assert!(error.to_string().contains("fuel"), "{error}");

    // A manifest that never returns is refused at load time.
    let wat = spin.replace(
        &format!("(i64.const {})", LOOPS.len()),
        "(loop $m (br $m)) (i64.const 0)",
    );
    let path = module("loops-manifest.wasm", &wat);
    let error = load_wasm(&path, &limits).unwrap_err();
    assert!(matches!(error, LoadError::Limit { .. }), "{error}");
    assert!(error.to_string().contains("fuel"), "{error}");
}

#[test]
fn a_module_that_asks_for_too_much_memory_is_refused() {
    let limits = WasmLimits {
        memory_bytes: 4 * 1024 * 1024,
        ..WasmLimits::default()
    };
    // Declared up front: 128 pages is 8 MiB.
    let big = plugin(LOOPS, "(i64.const 0)", "").replace(
        r#"(memory (export "memory") 1)"#,
        r#"(memory (export "memory") 128)"#,
    );
    let path = module("big-memory.wasm", &big);
    let error = load_wasm(&path, &limits).unwrap_err();
    assert!(matches!(error, LoadError::Limit { .. }), "{error}");
    assert!(error.to_string().contains("over the limit"), "{error}");

    // Grown at run time: the grow traps and the call fails.
    let grows = plugin(
        LOOPS,
        "(drop (memory.grow (i32.const 1000))) (i64.const 0)",
        "",
    );
    let path = module("grows.wasm", &grows);
    let plugin = load_wasm(&path, &limits).unwrap();
    let error = plugin.call(0, "x", 0).unwrap_err();
    assert!(error.contains("more memory"), "{error}");
}

#[test]
fn a_module_with_a_huge_table_is_refused() {
    // Table elements live in host memory, outside the linear memory cap: a
    // 400-million-element table cost about 1.6 GB before it was limited.
    let declared = plugin(LOOPS, "(i64.const 0)", "(table 400000000 funcref)");
    let path = module("huge-table.wasm", &declared);
    let started = Instant::now();
    assert!(load_wasm(&path, &WasmLimits::default()).is_err());
    assert!(started.elapsed() < Duration::from_secs(10));

    // Grown at run time: the grow traps and the call fails.
    let grows = plugin(
        LOOPS,
        "(drop (table.grow (ref.null func) (i32.const 400000000))) (i64.const 0)",
        "(table 1 funcref)",
    );
    let path = module("grows-table.wasm", &grows);
    let plugin = load_wasm(&path, &WasmLimits::default()).unwrap();
    let error = plugin.call(0, "x", 0).unwrap_err();
    assert!(error.contains("table elements"), "{error}");
}

#[test]
fn plugin_output_cannot_carry_a_hyperlink() {
    // Both capabilities answer with the text at 1024, whatever they are asked.
    const LINKS: &str = "rich-plugin-abi 1.0\nname links\nversion 1\n\
                         capability fence-markup lnk\ncapability highlighter hl\n";
    let answer = |text: &str| {
        let wat = plugin(
            LINKS,
            &format!("(i64.const {})", (1024u64 << 32) | text.len() as u64),
            &format!(r#"(data (i32.const 1024) "{text}")"#),
        );
        let path = module(&format!("links-{}.wasm", text.len()), &wat);
        let mut registry = ExtensionRegistry::new();
        registry
            .add_plugin(&load_wasm(&path, &WasmLimits::default()).unwrap())
            .unwrap();
        registry
    };
    let mut console = Console::builder()
        .width(40)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .force_terminal(true)
        .build();

    // Markup shows one URL and links to another: the link is dropped, the
    // styling kept.
    let registry = answer("[bold link=https://evil.example/x]https://safe.example[/]");
    let segments = registry
        .fences()
        .unwrap()
        .render_fence("lnk", "x", &console, &console.options())
        .unwrap();
    let styled = segments
        .iter()
        .find(|s| s.text == "https://safe.example")
        .unwrap();
    let style = styled.style.clone().unwrap();
    assert_eq!(style.link(), None, "{style:?}");
    assert_eq!(style, rich::Style::parse("bold").unwrap());

    // A highlighter span links nothing either.
    let registry = answer("0 5 bold link https://evil.example/y");
    registry.install(&mut console);
    let text = console.render_str("hello world", Some(true));
    let span = text.spans().iter().find(|s| s.start == 0).unwrap();
    let rich::StyleType::Style(style) = span.style.clone() else {
        panic!("{:?}", span.style)
    };
    assert_eq!(style.link(), None, "{style:?}");
    assert_eq!(style, rich::Style::parse("bold").unwrap());
}

#[test]
fn a_module_that_imports_anything_is_refused() {
    let wasi = plugin(
        LOOPS,
        "(i64.const 0)",
        r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#,
    );
    let path = module("imports.wasm", &wasi);
    let error = load_wasm(&path, &WasmLimits::default()).unwrap_err();
    assert!(matches!(error, LoadError::NotAPlugin { .. }), "{error}");
    assert!(error.to_string().contains("fd_write"), "{error}");
}

#[test]
fn another_abi_major_and_bad_files_are_refused() {
    let future = plugin(&LOOPS.replace("1.0", "2.0"), "(i64.const 0)", "");
    let path = module("future.wasm", &future);
    let error = load_wasm(&path, &WasmLimits::default()).unwrap_err();
    assert!(
        matches!(
            error,
            LoadError::Abi {
                error: AbiError::Incompatible { major: 2, .. },
                ..
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("ABI 2.0"), "{error}");

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("wasm-plugins");
    std::fs::create_dir_all(&dir).unwrap();
    let garbage = dir.join("garbage.wasm");
    std::fs::write(&garbage, b"not wasm").unwrap();
    assert!(matches!(
        load_wasm(&garbage, &WasmLimits::default()),
        Err(LoadError::NotAPlugin { .. })
    ));
    assert!(matches!(
        load_wasm(&dir.join("missing.wasm"), &WasmLimits::default()),
        Err(LoadError::Io { .. })
    ));
    let limits = WasmLimits {
        module_bytes: 4,
        ..WasmLimits::default()
    };
    assert!(matches!(
        load_wasm(&garbage, &limits),
        Err(LoadError::Limit { .. })
    ));
}
