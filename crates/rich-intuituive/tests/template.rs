//! The app template (templates/intuituive-app) compiles against this crate
//! and passes its own tests, so `cargo generate` hands out working code.

#[path = "../../../templates/intuituive-app/src/main.rs"]
#[allow(dead_code)]
mod template;

#[test]
fn the_template_names_this_crate_as_its_dependency() {
    let manifest = include_str!("../../../templates/intuituive-app/Cargo.toml");
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        manifest.contains(&format!("rs-rich-intuituive = \"{version}\"")),
        "templates/intuituive-app/Cargo.toml should depend on rs-rich-intuituive {version}"
    );
}

#[test]
fn the_template_theme_file_parses() {
    let theme = include_str!("../../../templates/intuituive-app/theme.ini");
    intuituive::Theme::dark().with_config(theme).unwrap();
}
