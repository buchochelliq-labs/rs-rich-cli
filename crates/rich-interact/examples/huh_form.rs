//! A Huh-style form and a paginator (0.0.21):
//! `cargo run -p rs-rich-interact --example huh_form [-- pages]`.
//!
//! - no argument: a two-page form with a note, descriptions, a
//!   multi-select, a file field and a region that depends on the cloud;
//! - `pages`: the paginator on its own, over this crate's components.
//!
//! Each prints its answer. Piped or under CI, each asks line by line.

use rich_interact::{run, Answers, FilePicker, Form, Input, Outcome, Paginator, RunOptions, Value};

fn regions(answers: &Answers) -> Vec<String> {
    let names: &[&str] = match answers.text("cloud") {
        Some("gcp") => &["us-central1", "europe-west1", "asia-east1"],
        Some("azure") => &["eastus", "westeurope"],
        _ => &["us-east-1", "eu-west-1", "ap-south-1"],
    };
    names.iter().map(|name| name.to_string()).collect()
}

fn form() -> Form {
    Form::new("Deploy a service")
        .group("The service")
        .note(
            "Two pages",
            "Enter on a page's last field checks the page and moves on; Page Up goes back.",
        )
        .input(
            "name",
            Input::new("Name").validate(|text| {
                if text.is_empty() {
                    Err("a name is required".into())
                } else {
                    Ok(())
                }
            }),
        )
        .describe("lower case, like api")
        .multi(
            "features",
            "Features",
            ["tls", "metrics", "logs", "tracing"],
        )
        .chosen(["tls"])
        .limit(3)
        .describe("space chooses, at most three")
        .file(
            "config",
            "Config",
            FilePicker::new("Config", ".").default("Cargo.toml"),
        )
        .group("Where it runs")
        .choice("cloud", "Cloud", ["aws", "gcp", "azure"])
        .dynamic_choice("region", "Region", regions)
        .describe("the regions follow the cloud")
        .toggle("public", "Public", false)
        .with_mouse(true)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = RunOptions::default();
    match std::env::args().nth(1).as_deref() {
        Some("pages") => {
            let names = [
                "Select",
                "MultiSelect",
                "Input",
                "Confirm",
                "Form",
                "Pager",
                "Paginator",
                "TextArea",
                "FilePicker",
                "ColorPicker",
                "AssetPicker",
                "DatePicker",
                "Reorder",
            ];
            let paginator = Paginator::over("Components", names, 4).with_mouse(true);
            if let Outcome::Done(page) = run(paginator, &options)? {
                println!("page {}", page + 1);
            }
        }
        _ => {
            if let Outcome::Done(answers) = run(form(), &options)? {
                for (name, value) in &answers.0 {
                    match value {
                        Value::Text(text) => println!("{name}: {text}"),
                        Value::Flag(on) => println!("{name}: {on}"),
                        Value::List(list) => println!("{name}: {}", list.join(", ")),
                        Value::Path(path) => println!("{name}: {}", path.display()),
                    }
                }
            }
        }
    }
    Ok(())
}
