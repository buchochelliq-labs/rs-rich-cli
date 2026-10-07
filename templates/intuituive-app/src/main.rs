//! An intuiTUIve app, from the rs-rich template.
//!
//!     cargo run                         # run it
//!     INTUITUIVE_INSPECT=1 cargo run    # with the inspector (F12 toggles it)
//!     cargo test                        # its tests, headless
//!
//! Edit theme.ini while it runs: the app redraws in the new styles.

use intuituive::interact::Input;
use intuituive::prelude::*;
use intuituive::rich::markup::escape;

/// The app: a list you add to.
fn app() -> App {
    App::new(|| {
        let items = signal(vec!["Read the intuiTUIve tutorial".to_string()]);
        let count = memo(move || items.with(Vec::len));
        column([
            text!("[title]{}[/] · {count} items", env!("CARGO_PKG_NAME")).auto(),
            repeating(
                || Input::new("Add"),
                move |item: String, _| {
                    if !item.trim().is_empty() {
                        items.update(|v| v.push(item));
                    }
                },
            )
            .panel("New")
            .fixed(3),
            each(
                move || (0..items.with(Vec::len)).collect(),
                move |i| {
                    text(move || {
                        let item = items.with(|v| v.get(i).cloned().unwrap_or_default());
                        format!("[accent]•[/] {}", escape(&item))
                    })
                },
            )
            .panel("Items"),
            label("[muted]enter adds · ctrl+q quits").auto(),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    })
}

fn main() -> std::io::Result<()> {
    app().theme_file("theme.ini").run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_adds_an_item() {
        let keys = ["m", "i", "l", "k", "enter", "ctrl+q"];
        let screen = app().render_with(&keys, 40, 10).unwrap();
        assert!(screen[0].contains("2 items"), "{screen:?}");
        assert!(
            screen.iter().any(|line| line.contains("• milk")),
            "{screen:?}"
        );
    }
}
