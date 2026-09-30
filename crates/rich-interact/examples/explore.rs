//! Explore a structured document interactively (#465):
//!
//! ```bash
//! cargo run -p rs-rich-interact --features data --example explore -- config.yaml
//! ```
//!
//! Left and Right fold, typing searches, Ctrl+Y copies the focused node's
//! path and Alt+Y its value (where the terminal takes OSC 52), Enter prints
//! the path. Without a file it explores a small sample.

use rich_ext::data::{parse, Format};
use rich_interact::components::json_path;
use rich_interact::{run, DataExplorer, Outcome, RunOptions};

const SAMPLE: &str = r#"{
  "name": "demo",
  "server": {"host": "localhost", "port": 8080, "tls": false},
  "users": [
    {"name": "ada", "roles": ["admin", "dev"]},
    {"name": "grace", "roles": ["dev"]}
  ]
}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (name, text) = match std::env::args().nth(1) {
        Some(path) => (path.clone(), std::fs::read_to_string(&path)?),
        None => ("sample.json".to_string(), SAMPLE.to_string()),
    };
    let format = Format::detect(&text, Some(&name)).ok_or("cannot tell the file's format")?;
    let document = parse(format, &text).map_err(|error| error.message)?;
    let explorer = DataExplorer::new(name, document);
    match run(explorer, &RunOptions::default())? {
        Outcome::Done(path) => println!("{}", json_path(&path)),
        Outcome::Cancelled | Outcome::Interrupted => std::process::exit(1),
    }
    Ok(())
}
