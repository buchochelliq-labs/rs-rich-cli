//! Named theme configuration, precedence, and frozen worker bindings.
use std::process::{Command, Output};

const CONFIG: &str = "[defaults]\ntheme = 'day'\n[profiles.night]\ntheme = 'night'\n[themes.day]\nalert = 'red'\n[themes.night]\nalert = 'blue'\n";

fn run(config: &str, args: &[&str]) -> Output {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("rich.toml"), config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(root.path())
        .env("HOME", root.path())
        .env_remove("NO_COLOR")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn defaults_profiles_and_explicit_theme_selection_are_inspectable() {
    for (flags, name, style) in [
        (vec![], "day", "red"),
        (vec!["--profile", "night"], "night", "blue"),
        (vec!["--profile", "night", "--theme", "day"], "day", "red"),
    ] {
        let mut args = vec!["config", "show"];
        args.extend(flags);
        let out = run(CONFIG, &args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["settings"]["theme"], name);
        assert_eq!(json["theme_styles"]["alert"], style);
    }
}

#[test]
fn validates_inactive_themes_and_profile_references() {
    for (config, message) in [
        ("[themes.unused]\nalert = 'not-a-real-color'", "style"),
        ("[themes.unused]\nalert = 42", "string"),
        ("[themes.'bad name']\nalert = 'red'", "name"),
        ("[themes.unused]\n'bad name' = 'red'", "name"),
        ("[profiles.unused]\ntheme = 'missing'", "unknown theme"),
        ("[defaults]\ntheme = 'missing'", "unknown theme"),
    ] {
        let out = run(config, &["config", "validate"]);
        assert_eq!(out.status.code(), Some(2), "{config}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(message),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn no_config_cannot_resolve_a_named_theme() {
    let out = run(
        CONFIG,
        &["--no-config", "--theme", "day", "--print", "hello"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown theme"));
}

#[test]
fn explicit_bindings_override_config_and_validate_during_inspection() {
    let out = run(CONFIG, &["config", "show", "--theme-style", "alert=green"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["theme_styles"]["alert"], "green");
    for binding in ["missing-equals", "bad name=red", "alert=not-a-real-color"] {
        assert_eq!(
            run(CONFIG, &["config", "validate", "--theme-style", binding])
                .status
                .code(),
            Some(2)
        );
    }
}

#[test]
fn new_scalar_settings_are_validated_even_in_inactive_profiles() {
    let out = run(
        "[defaults]\nimage_color = 'ansi256'\nimage_dither = 'floyd-steinberg'\nprogress = false",
        &["config", "show"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for setting in [
        "image_color = 'wrong'",
        "image_dither = 'wrong'",
        "progress = 'yes'",
    ] {
        assert_eq!(
            run(
                &format!("[profiles.unused]\n{setting}"),
                &["config", "validate"]
            )
            .status
            .code(),
            Some(2)
        );
    }
}

#[test]
fn option_looking_operands_do_not_select_a_theme() {
    let out = run(CONFIG, &["config", "show", "--export-html", "--theme"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["settings"]["theme"], "day");
    assert_eq!(json["settings"]["export_html"], "--theme");
}

#[test]
fn selected_theme_and_explicit_binding_reach_exports() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("rich.toml");
    std::fs::write(&config, CONFIG).unwrap();
    for (flags, rgb) in [
        (vec![], "#800000"),
        (vec!["--profile", "night"], "#000080"),
        (
            vec!["--theme", "night", "--theme-style", "alert=green"],
            "#008000",
        ),
    ] {
        let html = root.path().join("theme.html");
        let out = Command::new(env!("CARGO_BIN_EXE_rich"))
            .current_dir(root.path())
            .env_remove("NO_COLOR")
            .args([
                "--config",
                config.to_str().unwrap(),
                "--print",
                "[alert]hello[/alert]",
                "--export-html",
                html.to_str().unwrap(),
            ])
            .args(flags)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let output = std::fs::read_to_string(html).unwrap();
        assert!(output.contains(rgb), "expected {rgb} in {output}");
    }
}

#[test]
fn named_theme_reaches_svg_and_no_color_stays_plain() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("rich.toml"),
        CONFIG.replace("alert = 'red'", "alert = '#12ab34'"),
    )
    .unwrap();
    let svg = root.path().join("theme.svg");
    let out = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(root.path())
        .env_remove("NO_COLOR")
        .args([
            "--print",
            "[alert]hello[/alert]",
            "--export-svg",
            svg.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(std::fs::read_to_string(svg).unwrap().contains("#12ab34"));
    let plain = run(CONFIG, &["--print", "[alert]hello[/alert]", "--no-color"]);
    assert!(plain.status.success());
    assert_eq!(plain.stdout, b"hello\n");
}

#[test]
fn parallel_workers_receive_resolved_bindings() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("rich.toml"), CONFIG).unwrap();
    for file in ["a.txt", "b.txt"] {
        std::fs::write(root.path().join(file), "content").unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(root.path())
        .env_remove("NO_COLOR")
        .args([
            "--batch",
            "--jobs",
            "2",
            "--theme",
            "night",
            "--theme-style",
            "alert=green",
            "--panel",
            "rounded",
            "--title",
            "[alert]Theme[/alert]",
            "--export-html",
            "out.html",
            "a.txt",
            "b.txt",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exports: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "html"))
        .collect();
    assert_eq!(exports.len(), 2);
    for path in exports {
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.contains("#008000"), "{html}");
    }
}
