//! An answer streaming in as Markdown (`markdown` over a signal, #797):
//! each press of Space appends its next fragment, as an agent's reply
//! arrives, and only the last block renders again. The answer has a loose
//! list (its items a blank line apart, drawn as one list), a reference
//! link whose definition comes last, and fenced code that draws as code
//! while it is still open.
//!
//!     cargo run -p rs-rich-intuituive --example answer --features syntax
//!
//! Space streams the next fragment, Tab goes to the links, Enter shows the
//! link's address, Ctrl+Q quits.

use intuituive::markdown;
use intuituive::prelude::*;

/// The answer, as it arrives.
const FRAGMENTS: &[&str] = &[
    "# Moving the config\n\nThree steps, in order:\n\n",
    "1. Read the old file from `~/.app.toml`.\n\n",
    "   Keep its comments: the new reader keeps them too.\n\n",
    "2. Write it to the new place.\n\n3. Remove the old one, once the app has started with the new.\n\n",
    "The new place is in [the config guide][guide]. In code:\n\n```rust\nlet old = read(\"~/.app.toml\")?;\n",
    "write(config_dir().join(\"app.toml\"), old)?;\n```\n\n",
    "That is all.\n\n[guide]: https://docs.example/config",
];

fn main() -> std::io::Result<()> {
    App::new(|| {
        let doc = signal(String::new());
        let next = signal(0usize);
        let picked = signal(String::new());
        column([
            scroll(markdown(doc).on_link(move |url, _| picked.set(url.to_string())))
                .follow_end()
                .flex(1),
            text(move || {
                let n = next.get();
                let state = if n < FRAGMENTS.len() {
                    format!(
                        "[dim]{n} of {} fragments · Space for the next[/]",
                        FRAGMENTS.len()
                    )
                } else {
                    "[dim]done · Tab to the links, Enter to follow one[/]".to_string()
                };
                match picked.get() {
                    url if url.is_empty() => state,
                    url => format!("link: [u]{url}[/]"),
                }
            })
            .fixed(1),
        ])
        .on_key("space", move |_| {
            let n = next.get_untracked();
            if let Some(fragment) = FRAGMENTS.get(n) {
                doc.update(|d| d.push_str(fragment));
                next.set(n + 1);
            }
        })
    })
    .run()
}
