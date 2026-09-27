//! Every component, one subcommand each:
//! `cargo run -p rs-rich-interact --example components -- <name> [ARG]`.
//!
//! - `select [DIR]`: pick a file, with a highlighted preview;
//! - `multi`: mark several crates;
//! - `input`: a crate name, with suggestions and validation;
//! - `confirm`: a change to apply, with a diff, a warning and four choices;
//! - `form`: a new service's settings;
//! - `pager [FILE]`: page a file, `/` to search.
//!
//! Each prints its answer. Piped or under CI, each asks line by line.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rich::syntax::Syntax;
use rich_interact::{
    run, Choice, Confirm, Form, Input, Item, MultiSelect, Outcome, Pager, Preview, RunOptions,
    Select, Suggestion,
};

const CRATES: [(&str, &str); 8] = [
    ("rs-rich", "the faithful port of rich"),
    ("rs-rich-ext", "extensions: frames, live, workflows"),
    ("rs-rich-cli", "the rich command"),
    ("rs-rich-art", "images, GIFs and banners"),
    ("rs-rich-record", "tapes: record terminal sessions"),
    ("rs-rich-interact", "interactive components"),
    ("rs-rich-mermaid", "Mermaid diagrams as text"),
    ("rs-rich-lumis", "tree-sitter highlighting"),
];

fn files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') || entry.file_name() == "target" {
            continue;
        }
        if path.is_dir() {
            files(root, &path, out);
        } else if out.len() < 2000 {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
}

fn syntax(path: &Path, lines: usize) -> Syntax {
    let source = std::fs::read_to_string(path).unwrap_or_default();
    let head: String = source.lines().take(lines).collect::<Vec<_>>().join("\n");
    let language = path.extension().and_then(|e| e.to_str()).unwrap_or("text");
    Syntax::new(head, language).line_numbers(true)
}

fn answer<T: std::fmt::Debug>(outcome: Outcome<T>) {
    match outcome {
        Outcome::Done(value) => println!("{value:?}"),
        Outcome::Cancelled => eprintln!("cancelled"),
        Outcome::Interrupted => eprintln!("interrupted"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = args.first().map_or("select", String::as_str);
    let arg = args.get(1).map(String::as_str);
    let options = RunOptions::default();
    match name {
        "select" => {
            let root = PathBuf::from(arg.unwrap_or("."));
            let mut paths = Vec::new();
            files(&root, &root, &mut paths);
            let items = paths.into_iter().map(|path| {
                let size = std::fs::metadata(root.join(&path)).map_or(0, |m| m.len());
                let preview = Preview::Renderable(Arc::new(syntax(&root.join(&path), 60)));
                Item::new(path.clone(), path.display().to_string())
                    .description(format!("{size} B"))
                    .preview(preview)
            });
            answer(run(Select::new("Open", items), &options)?);
        }
        "multi" => {
            let items = CRATES
                .iter()
                .map(|(name, about)| Item::new(*name, *name).description(*about));
            answer(run(MultiSelect::new("Publish", items).height(8), &options)?);
        }
        "input" => {
            let input = Input::new("Crate")
                .placeholder("a crate name")
                .suggestions(
                    CRATES
                        .iter()
                        .map(|(name, about)| Suggestion::new(*name).description(*about)),
                )
                .validate(|text| {
                    if CRATES.iter().any(|(name, _)| *name == text) {
                        Ok(())
                    } else {
                        Err(format!("{text:?} is not one of the workspace's crates"))
                    }
                });
            answer(run(input, &options)?);
        }
        "confirm" => {
            let diff = rich::Text::from_markup(
                "[dim]deployment.yaml[/]\n  [red]- replicas: 2[/]\n  [green]+ replicas: 5[/]\n  [red]- image: api:1.4[/]\n  [green]+ image: api:1.5[/]",
            )?;
            let sheet = Confirm::new("Apply this change to production?")
                .body(diff)
                .warning("5 pods will restart, one at a time")
                .choices([
                    Choice::new("apply", "Apply", 'a'),
                    Choice::new("dry-run", "Dry run", 'd'),
                    Choice::new("edit", "Edit", 'e'),
                    Choice::new("cancel", "Cancel", 'c'),
                ])
                .default("dry-run");
            answer(run(sheet, &options)?);
        }
        "form" => {
            let form = Form::new("New service")
                .input(
                    "name",
                    Input::new("Name")
                        .placeholder("lowercase, like api")
                        .validate(|text| {
                            if !text.is_empty()
                                && text.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                            {
                                Ok(())
                            } else {
                                Err("lowercase letters and dashes".into())
                            }
                        }),
                )
                .input("port", Input::new("Port").default("8080"))
                .choice("env", "Environment", ["dev", "staging", "prod"])
                .toggle("tls", "TLS", true)
                .password("token", "Deploy token");
            if let Outcome::Done(answers) = run(form, &options)? {
                // Everything but the token, which is only said to be set.
                for (name, value) in &answers.0 {
                    match (name.as_str(), value) {
                        ("token", _) => println!("token: (set)"),
                        (_, rich_interact::Value::Text(text)) => println!("{name}: {text}"),
                        (_, rich_interact::Value::Flag(on)) => println!("{name}: {on}"),
                    }
                }
            }
        }
        "pager" => {
            let path = PathBuf::from(arg.unwrap_or("README.md"));
            answer(run(Pager::new(syntax(&path, usize::MAX)), &options)?);
        }
        other => return Err(format!("unknown component {other:?}").into()),
    }
    Ok(())
}
