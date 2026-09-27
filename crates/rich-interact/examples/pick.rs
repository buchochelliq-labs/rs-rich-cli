//! Pick a file with a fuzzy filter and a highlighted preview:
//! `cargo run -p rs-rich-interact --example pick -- [DIR]`.
//!
//! Type to filter, arrows to move, Enter to pick, Tab to mark several with
//! `--multi`. Without a terminal it lists the files and reads a number.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rich::syntax::Syntax;
use rich_interact::{run, Item, MultiSelect, Outcome, Preview, RunOptions, Select};

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out);
        } else if out.len() < 2000 {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
}

fn item(root: &Path, path: PathBuf) -> Item<PathBuf> {
    let label = path.display().to_string();
    let source = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
    let head: String = source.lines().take(60).collect::<Vec<_>>().join("\n");
    let language = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("text")
        .to_string();
    let size = std::fs::metadata(root.join(&path)).map_or(0, |m| m.len());
    Item::new(path, label)
        .description(format!("{size} B"))
        .preview(Preview::Renderable(Arc::new(
            Syntax::new(head, language).line_numbers(true),
        )))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let multi = args.iter().any(|a| a == "--multi");
    let root = PathBuf::from(
        args.iter()
            .find(|a| !a.starts_with("--"))
            .map_or(".", String::as_str),
    );
    let mut paths = Vec::new();
    walk(&root, &root, &mut paths);
    let items: Vec<Item<PathBuf>> = paths.into_iter().map(|path| item(&root, path)).collect();
    let options = RunOptions::default();
    if multi {
        if let Outcome::Done(paths) = run(MultiSelect::new("Files", items), &options)? {
            for path in paths {
                println!("{}", path.display());
            }
        }
    } else if let Outcome::Done(path) = run(Select::new("Open", items), &options)? {
        println!("{}", path.display());
    }
    Ok(())
}
