//! Registry conformance (#567, #583, #586): precedence, trust, aliases,
//! collisions and `explain`.

mod common;

use std::path::Path;

use common::*;
use rich::Console;
use rich_micro::{Layer, Limits, MicroAsset, MicroError, MicroRegistry, MicroRoots};
use serde_json::json;

fn with_alt(dir: &Path, name: &str, alt: &str) {
    let mut m = manifest(name);
    m["alt"] = json!(alt);
    package_dir(dir, &m, &[("static.png", png(16, 16))]);
}

struct Tree {
    _dir: tempfile::TempDir,
    roots: MicroRoots,
}

/// `ok` in every file-backed layer with a different alt text; `user-only` in
/// the user layer; `project-only` in the project layer.
fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let project = dir.path().join("project");
    let builtin = dir.path().join("builtin");
    let user = MicroRoots::user_dir(&home);
    let project_dir = MicroRoots::project_dir(&project);
    with_alt(&builtin.join("ok"), "ok", "built-in ok");
    with_alt(&user.join("ok.richmicro"), "ok", "user ok");
    with_alt(&user.join("user-only"), "user-only", "user only");
    with_alt(&project_dir.join("ok"), "ok", "project ok");
    with_alt(&project_dir.join("project-only"), "project-only", "project only");
    Tree {
        roots: MicroRoots {
            builtin: Some(builtin),
            user: Some(user),
            project: Some(project_dir),
            project_trusted: false,
        },
        _dir: dir,
    }
}

#[test]
fn layers_resolve_in_order() {
    let tree = tree();
    let limits = Limits::default();

    // Untrusted: the project layer is not loaded, and the report says so.
    let (registry, report) = MicroRegistry::load(&tree.roots, &limits);
    assert_eq!(registry.resolve("ok").unwrap().alt(), "user ok");
    assert!(registry.resolve("project-only").is_none());
    assert_eq!(report.untrusted_project, tree.roots.project);
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);

    // Trusted: the project wins over the user.
    let trusted = tree.roots.clone().trust_project(true);
    let (mut registry, report) = MicroRegistry::load(&trusted, &limits);
    assert!(report.untrusted_project.is_none());
    let ok = registry.resolve("ok").unwrap();
    assert_eq!((ok.alt(), ok.origin().layer), ("project ok", Layer::Project));
    assert_eq!(registry.resolve("project-only").unwrap().alt(), "project only");
    let shadowed: Vec<&str> = registry.shadowed("ok").iter().map(|a| a.alt()).collect();
    assert_eq!(shadowed, ["user ok", "built-in ok"]);

    // Inline wins over everything.
    registry
        .add(Layer::Inline, MicroAsset::new("ok", "inline ok").unwrap())
        .unwrap();
    assert_eq!(registry.resolve("ok").unwrap().alt(), "inline ok");
    assert_eq!(registry.get(Layer::BuiltIn, "ok").unwrap().alt(), "built-in ok");
    assert_eq!(registry.names(), ["ok", "project-only", "user-only"]);

    // Removing the inline one uncovers the project's.
    registry.remove(Layer::Inline, "ok").unwrap();
    assert_eq!(registry.resolve("ok").unwrap().alt(), "project ok");
    assert!(matches!(
        registry.require("nope"),
        Err(MicroError::UnknownAsset(_))
    ));
    assert!(matches!(
        registry.require("No"),
        Err(MicroError::InvalidName(_))
    ));
}

#[test]
fn a_missing_layer_is_empty_and_the_builtin_layer_exists() {
    let roots = MicroRoots {
        user: Some("/nonexistent/rich/micro".into()),
        ..MicroRoots::default()
    };
    let (registry, report) = MicroRegistry::load(&roots, &Limits::default());
    assert!(registry.names().is_empty());
    assert_eq!(report, Default::default());
    assert_eq!(registry.layer(Layer::BuiltIn).count(), 0);
    // `explain` knows all four layers even when empty.
    assert_eq!(registry.precedence().layers.len(), 4);
}

#[test]
fn user_dir_matches_the_cli_config_home() {
    assert_eq!(
        MicroRoots::user_dir(Path::new("/home/me")),
        Path::new("/home/me/.config/rich/micro")
    );
    assert_eq!(
        MicroRoots::project_dir(Path::new("/src/app")),
        Path::new("/src/app/.rich/micro")
    );
    let roots = MicroRoots::from_env(Some(Path::new("/src/app")));
    assert!(!roots.project_trusted);
}

#[test]
fn collisions_are_deterministic_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    // Two packages claim `ok`: file-name order decides, whatever order the
    // directory lists them in.
    with_alt(&dir.path().join("b-second"), "ok", "second");
    with_alt(&dir.path().join("a-first"), "ok", "first");
    // A broken package is rejected; the rest load.
    let mut bad = manifest("broken");
    bad["schema_version"] = json!(0);
    package_dir(&dir.path().join("c-broken"), &bad, &[("static.png", png(1, 1))]);
    // Hidden entries and stray files are skipped.
    with_alt(&dir.path().join(".hidden"), "hidden", "hidden");
    std::fs::write(dir.path().join("README.txt"), "notes").unwrap();

    for _ in 0..3 {
        let mut registry = MicroRegistry::new();
        let mut report = Default::default();
        registry.load_dir(Layer::User, dir.path(), &Limits::default(), &mut report);
        assert_eq!(registry.resolve("ok").unwrap().alt(), "first");
        assert!(registry.resolve("hidden").is_none());
        assert_eq!(report.collisions.len(), 1);
        let collision = &report.collisions[0];
        assert_eq!((collision.layer, collision.name.as_str()), (Layer::User, "ok"));
        assert!(collision.kept.contains("a-first"), "{collision:?}");
        assert!(collision.dropped.contains("b-second"), "{collision:?}");
        assert_eq!(report.rejected.len(), 1);
        assert!(report.rejected[0].path.ends_with("c-broken"));
    }
}

#[test]
fn aliases_resolve_within_their_layer() {
    let mut registry = MicroRegistry::new();
    let asset = MicroAsset::new("status/check", "check mark")
        .unwrap()
        .with_alias("check")
        .unwrap();
    assert!(registry.add(Layer::User, asset).unwrap().is_empty());
    assert_eq!(registry.resolve("check").unwrap().name(), "status/check");

    // A higher layer's asset named like the alias wins over it.
    registry
        .add(Layer::Project, MicroAsset::new("check", "project check").unwrap())
        .unwrap();
    assert_eq!(registry.resolve("check").unwrap().alt(), "project check");
    // The project's own `status/check` does not exist, so that still comes
    // from the user layer.
    assert_eq!(registry.resolve("status/check").unwrap().alt(), "check mark");

    // An alias that clashes with a name in the same layer loses, and says so.
    let clash = MicroAsset::new("other", "other")
        .unwrap()
        .with_alias("status/check")
        .unwrap();
    let collisions = registry.add(Layer::User, clash).unwrap();
    assert_eq!(collisions.len(), 1);
    assert_eq!(collisions[0].name, "status/check");
    assert_eq!(registry.resolve("status/check").unwrap().alt(), "check mark");
    assert_eq!(registry.resolve("other").unwrap().alt(), "other");

    // Removing an asset removes its aliases.
    registry.remove(Layer::User, "status/check");
    assert!(registry.get(Layer::User, "check").is_none());
}

#[test]
fn explain_shows_the_chain() {
    let tree = tree();
    let (registry, _) = MicroRegistry::load(&tree.roots.clone().trust_project(true), &Limits::default());
    let explanation = registry.explain("ok").unwrap();
    let resolved = explanation.resolved();
    assert_eq!(resolved.winner, Layer::Project as usize);
    assert_eq!(resolved.shadowed.len(), 2);
    let console = Console::builder().width(100).force_terminal(false).build();
    let out = console.render_to_string(&explanation);
    assert!(out.contains("project"), "{out}");
    assert!(out.contains("overridden by project"), "{out}");
    assert!(registry.explain("nope").is_none());
    // Aliases are keys too.
    let mut registry = registry;
    registry
        .add(
            Layer::Inline,
            MicroAsset::new("x", "x").unwrap().with_alias("y").unwrap(),
        )
        .unwrap();
    assert_eq!(registry.explain("y").unwrap().resolved().value, "alias of x");
}
