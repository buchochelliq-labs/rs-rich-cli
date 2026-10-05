//! `rich micro` (#568, #582): list, show, preview, add, remove and create
//! micro assets, and install, uninstall and list packs.
//!
//! A binary-boundary convenience (docs/PORTING.md) over rs-rich-micro: the
//! registry, the package reader and writer, the pipeline and the drawing
//! are the library's; this module only routes arguments, picks layer
//! directories and reports.
//!
//! Layers: the built-in set, the user's `~/.config/rich/micro/`, and the
//! project's `./.rich/micro/`, which loads only when trusted: with
//! `--micro-project`, or `micro_project = true` in the user's own config
//! (`~/.config/rich/config.toml` or `--config FILE`). A project's
//! `./rich.toml` cannot turn it on, the rule that keeps it from loading
//! plugins. `add`, `remove`, `install`, `uninstall` and `create --add` work
//! on the user layer unless `--project` is given.
//!
//! `--report json` (or `--json`) writes data to standard output instead of
//! tables, and errors as the JSON envelope on standard error.
use super::*;

use std::sync::Arc;

use rich_ext::cli_doc::{ArgSpec, CommandSpec};
use rich_micro::create::{package_path, write_package, PackageSpec};
use rich_micro::package::{self as micro_package, Loaded};
use rich_micro::pipeline::{Magnified, Pipeline, Transparency};
use rich_micro::{
    CellSize, FallbackPreference, ImageCache, Layer, Limits, MicroAsset, MicroGraphics,
    MicroRegistry, MicroRoots, PackageLocation,
};

/// Whether the command line is `rich micro …`.
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("micro")
}

const SUBCOMMANDS: &[&str] = &[
    "list",
    "show",
    "preview",
    "add",
    "remove",
    "create",
    "install",
    "uninstall",
    "packs",
];

/// Options of `create` (and `preview`) that take a value.
const CREATE_VALUES: &[&str] = &[
    "--name",
    "--alt",
    "--emoji",
    "--text",
    "--size",
    "--fit",
    "--anchor",
    "--brightness",
    "--contrast",
    "--gamma",
    "--sharpen",
    "--transparency",
    "--colors",
    "--dither",
    "--cell",
    "--license",
    "--author",
    "--version",
    "--output",
];
const CREATE_FLAGS: &[&str] = &["--grayscale", "--archive", "--add"];

/// The help, registered with the root spec.
pub(super) fn command() -> CommandSpec {
    let json = || {
        ArgSpec::option("report")
            .value_name("FORMAT")
            .choices(["human", "json"])
            .help("json: write data to stdout (also --json)")
    };
    let project = || {
        ArgSpec::flag("project").help("Use the project's ./.rich/micro/ instead of the user layer")
    };
    let pipeline_args =
        |command: CommandSpec| {
            command
                .arg(
                    ArgSpec::option("size")
                        .value_name("COLSxROWS")
                        .help("Cells the asset takes: 2x1 (default, an emoji's footprint) or 1x1"),
                )
                .arg(
                    ArgSpec::option("fit")
                        .value_name("FIT")
                        .choices(["contain", "cover", "stretch"])
                        .help("contain (default) keeps the whole image; cover fills and crops"),
                )
                .arg(
                    ArgSpec::option("anchor")
                        .value_name("ANCHOR")
                        .help("What cover keeps: center (default), top, bottom, left, right, …"),
                )
                .arg(
                    ArgSpec::option("brightness")
                        .value_name("F")
                        .help("Multiply brightness (1 = as is)"),
                )
                .arg(
                    ArgSpec::option("contrast")
                        .value_name("F")
                        .help("Scale contrast (1 = as is)"),
                )
                .arg(
                    ArgSpec::option("gamma")
                        .value_name("F")
                        .help("Gamma (above 1 brightens mid-tones)"),
                )
                .arg(ArgSpec::flag("grayscale").help("Gray levels only"))
                .arg(
                    ArgSpec::option("sharpen")
                        .value_name("RADIUS")
                        .help("Unsharp mask of RADIUS pixels after fitting (e.g. 0.8)"),
                )
                .arg(ArgSpec::option("transparency").value_name("MODE").help(
                    "threshold[:ALPHA] (default threshold:128), keep, flatten:#rrggbb, or \
                 key:#rrggbb to make a background colour transparent",
                ))
                .arg(
                    ArgSpec::option("colors")
                        .value_name("PALETTE")
                        .choices(["truecolor", "256", "16", "grayscale"])
                        .help("Reduce to a palette (default truecolor)"),
                )
                .arg(
                    ArgSpec::option("dither")
                        .value_name("DITHER")
                        .choices(["none", "floyd-steinberg", "bayer", "atkinson"])
                        .help("Dithering for --colors (default none)"),
                )
                .arg(ArgSpec::option("cell").value_name("WxH").help(
                    "Pixels per cell to make images for (default 8x16; preview: the terminal's)",
                ))
        };
    let named = |name: &str, about: &str, arg: &str, help: &str| {
        CommandSpec::new(name)
            .about(about)
            .usage(format!(
                "{name} {} [--project] [--report json]",
                arg.to_uppercase()
            ))
            .arg(ArgSpec::positional(arg).help(help))
            .arg(project())
            .arg(json())
    };
    CommandSpec::new("micro")
        .about(
            "Micro assets: emoji-sized images and animations written :micro:name: (with \
             --emoji). List, show, preview, add, remove and create them; install packs",
        )
        .usage("micro [list | show | preview | add | remove | create | install | uninstall | packs] …")
        .subcommand(
            CommandSpec::new("list")
                .about("Every asset that resolves, drawn, with its size, kind, layer and alt text")
                .usage("list [--layer built-in|user|project] [--micro-project] [--report json]")
                .arg(
                    ArgSpec::option("layer")
                        .value_name("LAYER")
                        .choices(["built-in", "user", "project"])
                        .help("Only this layer's assets"),
                )
                .arg(json()),
        )
        .subcommand(
            CommandSpec::new("show")
                .about("One asset: its metadata, fallbacks, files and how its name resolved")
                .usage("show NAME [--report json]")
                .arg(ArgSpec::positional("name").help("The asset's name, e.g. status/success"))
                .arg(json()),
        )
        .subcommand(pipeline_args(
            CommandSpec::new("preview")
                .about(
                    "Draw an asset inline, as this terminal shows it, and magnified at the \
                     terminal's cell size; a package path or an image file (run through the \
                     pipeline) works too",
                )
                .usage("preview NAME|PACKAGE|IMAGE [PIPELINE OPTIONS] [--report json]")
                .arg(ArgSpec::positional("target").help("An asset name, a package, or an image"))
                .arg(json()),
        ))
        .subcommand(named(
            "add",
            "Copy a package (a folder or .richmicro file) into the user layer",
            "package",
            "The package to add",
        ))
        .subcommand(named(
            "remove",
            "Delete an asset's package from the user layer",
            "name",
            "The asset to remove",
        ))
        .subcommand(pipeline_args(
            CommandSpec::new("create")
                .about(
                    "Run an image (PNG, APNG, GIF or JPEG) through the pipeline and write a \
                     package the registry accepts",
                )
                .usage("create IMAGE --name NAME --alt TEXT [--emoji E] [--text T] [OPTIONS]")
                .arg(ArgSpec::positional("image").help("The source image or animation"))
                .arg(ArgSpec::option("name").value_name("NAME").help("The asset's name: [a-z0-9_-] with / namespaces"))
                .arg(ArgSpec::option("alt").value_name("TEXT").help("Alt text (required)"))
                .arg(ArgSpec::option("emoji").value_name("EMOJI").help("The emoji fallback"))
                .arg(ArgSpec::option("text").value_name("TEXT").help("The text fallback"))
                .arg(ArgSpec::option("license").value_name("SPDX").help("The asset's licence"))
                .arg(ArgSpec::option("author").value_name("NAME").help("Its author"))
                .arg(ArgSpec::option("version").value_name("VERSION").help("Its version"))
                .arg(ArgSpec::option("output").value_name("PATH").help(
                    "Where to write it (default ./NAME with / as ., or the layer with --add)",
                ))
                .arg(ArgSpec::flag("archive").help("Write a .richmicro zip, not a folder"))
                .arg(ArgSpec::flag("add").help("Write it into the user (or --project) layer"))
                .arg(project())
                .arg(json())
                .example(
                    "rich micro create logo.png --name team/logo --alt \"our logo\" --text TL --add",
                    "Make an asset from an image and add it to your layer",
                ),
        ))
        .subcommand(named(
            "install",
            "Install a pack (a folder or zip with pack.json) into the user layer",
            "pack",
            "The pack to install",
        ))
        .subcommand(named(
            "uninstall",
            "Remove an installed pack by name from the user layer",
            "name",
            "The pack's name",
        ))
        .subcommand(
            CommandSpec::new("packs")
                .about("The packs in each layer and the assets they hold")
                .usage("packs [--micro-project] [--report json]")
                .arg(json()),
        )
        .example("rich micro list", "Every asset, drawn as this terminal can")
        .example("rich micro preview status/loading", "One asset, inline and magnified")
        .example(
            "rich -p --emoji \"Deploying :micro:status/loading: done :micro:status/success:\"",
            "Micro assets in text",
        )
}

/// What `rich micro` was asked.
#[derive(Default)]
struct Request {
    sub: String,
    positionals: Vec<String>,
    values: std::collections::BTreeMap<String, String>,
    flags: Vec<String>,
    json: bool,
    project: bool,
}

impl Request {
    fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    fn one(&self, what: &str) -> Result<&str, Failure> {
        match self.positionals.as_slice() {
            [one] => Ok(one),
            [] => Err(usage(format!("micro {} requires {what}", self.sub))),
            _ => Err(usage(format!("micro {} takes one {what}", self.sub))),
        }
    }
}

type Failure = (ExitClass, String);

fn usage(message: impl Into<String>) -> Failure {
    (ExitClass::Usage, message.into())
}

fn data(message: impl std::fmt::Display) -> Failure {
    (ExitClass::Data, message.to_string())
}

fn input(message: impl std::fmt::Display) -> Failure {
    (ExitClass::Input, message.to_string())
}

fn request(args: &[String]) -> Result<Request, String> {
    let mut request = Request::default();
    let mut seen_micro = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || -> Result<String, String> {
            inline
                .clone()
                .or_else(|| iter.next().cloned())
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match name {
            "--" => {
                request.positionals.extend(iter.by_ref().cloned());
            }
            "--report" => {
                let format = value()?;
                match format.as_str() {
                    "json" => request.json = true,
                    "human" => request.json = false,
                    other => return Err(format!("unknown report format {other:?} (human, json)")),
                }
            }
            "--json" | "--machine-json" => request.json = true,
            "--config" | "--profile" => {
                value()?;
            }
            "--no-config" | "--no-color" | "--color" | "--micro-project" | "--no-micro-project" => {
            }
            "--project" => request.project = true,
            "--layer" => {
                request.values.insert(name.into(), value()?);
            }
            _ if CREATE_VALUES.contains(&name)
                && matches!(request.sub.as_str(), "create" | "preview") =>
            {
                request.values.insert(name.into(), value()?);
            }
            _ if CREATE_FLAGS.contains(&name)
                && matches!(request.sub.as_str(), "create" | "preview") =>
            {
                request.flags.push(name.into());
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other} for `rich micro`"));
            }
            _ if !seen_micro => seen_micro = true,
            word if request.sub.is_empty() => {
                if !SUBCOMMANDS.contains(&word) {
                    return Err(format!(
                        "unknown micro command {word:?}; use one of: {}",
                        SUBCOMMANDS.join(", ")
                    ));
                }
                request.sub = word.to_string();
            }
            _ => request.positionals.push(text_arg(arg)),
        }
    }
    if request.sub.is_empty() {
        request.sub = "list".into();
    }
    Ok(request)
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args) || args.iter().any(|a| a == "--json");
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help")
    {
        let no_color = cli_spec::no_color_requested(args);
        let sub = args
            .iter()
            .find(|arg| SUBCOMMANDS.contains(&arg.as_str()))
            .map(String::as_str);
        let mut path = vec!["micro"];
        path.extend(sub);
        if let Some(help) = cli_spec::subcommand_help(&path, no_color) {
            authoring::out(&format!("{help}\n"));
        }
        return ExitCode::SUCCESS;
    }
    let request = match request(args) {
        Ok(request) => request,
        Err(message) => {
            return emit_error(json, ExitClass::Usage, &format!("{message} (try --help)"))
        }
    };
    let roots = ConfigRoots::default();
    let merged = match config_args(args, &roots) {
        Ok(merged) => merged,
        Err(message) => return emit_error(json, ExitClass::Usage, &message),
    };
    let no_color = cli_spec::no_color_requested(&merged);
    let context = Context {
        trusted: project_trusted(&merged),
        home: roots.home.clone(),
        cwd: roots.cwd.clone(),
        no_color,
        json: request.json,
    };
    let result = match request.sub.as_str() {
        "list" => list(&context, &request),
        "show" => show(&context, &request),
        "preview" => preview(&context, &request),
        "add" => add(&context, &request),
        "remove" => remove(&context, &request),
        "create" => create(&context, &request),
        "install" => install(&context, &request),
        "uninstall" => uninstall(&context, &request),
        _ => packs(&context),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err((ExitClass::Usage, message)) => emit_error(
            request.json,
            ExitClass::Usage,
            &format!("{message} (try --help)"),
        ),
        Err((class, message)) => emit_error(request.json, class, &message),
    }
}

/// Whether the merged command line (config first, so a trusted config's
/// `micro_project` counts) trusts the project: the last of
/// `--micro-project` and `--no-micro-project` wins.
pub(crate) fn project_trusted(merged: &[String]) -> bool {
    let mut trusted = false;
    for arg in merged.iter().take_while(|arg| *arg != "--") {
        match arg.as_str() {
            "--micro-project" => trusted = true,
            "--no-micro-project" => trusted = false,
            _ => {}
        }
    }
    trusted
}

struct Context {
    trusted: bool,
    home: Option<PathBuf>,
    cwd: PathBuf,
    no_color: bool,
    json: bool,
}

impl Context {
    fn roots(&self) -> MicroRoots {
        MicroRoots {
            builtin_set: true,
            builtin: None,
            user: self.home.as_deref().map(MicroRoots::user_dir),
            project: Some(MicroRoots::project_dir(&self.cwd)),
            project_trusted: self.trusted,
        }
    }

    fn registry(&self) -> (MicroRegistry, rich_micro::LoadReport) {
        let (registry, report) = MicroRegistry::load(&self.roots(), &Limits::default());
        if !self.json {
            warn(&report);
        }
        (registry, report)
    }

    /// The directory `--project` (or not) writes to.
    fn layer_dir(&self, project: bool) -> Result<(Layer, PathBuf), Failure> {
        if project {
            Ok((Layer::Project, MicroRoots::project_dir(&self.cwd)))
        } else {
            let home = self
                .home
                .as_deref()
                .ok_or_else(|| input("no home directory (HOME is not set) for the user layer"))?;
            Ok((Layer::User, MicroRoots::user_dir(home)))
        }
    }

    fn console(&self) -> Console {
        cli_spec::console(self.no_color)
    }

    /// Print `renderable` with its micro assets drawn as this terminal can
    /// (the fallback cells anywhere else).
    fn print(&self, registry: &Arc<MicroRegistry>, renderable: impl Renderable) {
        let console = self.console();
        if console.is_terminal() {
            let graphics = MicroGraphics::detect(Arc::clone(registry));
            console.print(&graphics.view(renderable));
        } else {
            console.print(&renderable);
        }
    }

    fn out_json(&self, value: &serde_json::Value) {
        authoring::out(&(serde_json::to_string_pretty(value).expect("micro JSON") + "\n"));
    }
}

/// A line on standard error. Everything these messages quote (package
/// paths, pack entries, manifest fields, errors) is untrusted, so its
/// terminal controls are shown, never run.
fn note(line: &str) {
    eprintln!("{}", controls::shown(line));
}

/// Untrusted text for a table cell, its controls shown.
fn shown_cell(value: &str) -> Text {
    Text::new(controls::shown(value))
}

/// Loading problems, as warnings on standard error.
fn warn(report: &rich_micro::LoadReport) {
    for rejected in &report.rejected {
        let package = rejected
            .package
            .as_deref()
            .map(|p| format!(" ({p})"))
            .unwrap_or_default();
        note(&format!(
            "rich: warning: micro asset {}{package} not loaded: {}",
            rejected.path.display(),
            rejected.error
        ));
    }
    for collision in &report.collisions {
        note(&format!(
            "rich: warning: micro asset {} in the {} layer: kept {}, ignored {}",
            collision.name, collision.layer, collision.kept, collision.dropped
        ));
    }
}

/// An untrusted project layer, noted on `list` and `packs`.
fn untrusted_note(report: &rich_micro::LoadReport) -> Option<String> {
    report.untrusted_project.as_ref().map(|dir| {
        format!(
            "{} is not loaded: the project is not trusted (pass --micro-project, or set \
             micro_project = true in ~/.config/rich/config.toml)",
            dir.display()
        )
    })
}

fn asset_json(asset: &MicroAsset, registry: &MicroRegistry) -> serde_json::Value {
    let fallback = asset.fallback();
    serde_json::json!({
        "name": asset.name(),
        "size": asset.size().to_string(),
        "kind": asset.kind().as_str(),
        "layer": asset.origin().layer.as_str(),
        "pack": asset.origin().pack,
        "origin": asset.origin().to_string(),
        "alt": asset.alt(),
        "fallback": {"emoji": fallback.emoji, "text": fallback.text},
        "aliases": asset.aliases(),
        "version": asset.version(),
        "license": asset.license(),
        "author": asset.author(),
        "shadows": registry
            .shadowed(asset.name())
            .iter()
            .map(|a| a.origin().layer.as_str())
            .collect::<Vec<_>>(),
    })
}

fn preference(console: &Console) -> FallbackPreference {
    if console.encoding().starts_with("utf") {
        FallbackPreference::Emoji
    } else {
        FallbackPreference::Text
    }
}

fn list(context: &Context, request: &Request) -> Result<(), Failure> {
    if !request.positionals.is_empty() {
        return Err(usage("micro list takes no arguments"));
    }
    let layer = match request.value("--layer") {
        None => None,
        Some("built-in" | "builtin") => Some(Layer::BuiltIn),
        Some("user") => Some(Layer::User),
        Some("project") => Some(Layer::Project),
        Some(other) => {
            return Err(usage(format!(
                "--layer: {other:?} is not built-in, user or project"
            )))
        }
    };
    let (registry, report) = context.registry();
    let assets: Vec<Arc<MicroAsset>> = match layer {
        Some(layer) => registry.layer(layer).cloned().collect(),
        None => registry.assets().into_iter().cloned().collect(),
    };
    if context.json {
        context.out_json(&serde_json::json!({
            "project_trusted": context.trusted,
            "untrusted_project": report.untrusted_project.as_ref().map(|p| p.display().to_string()),
            "assets": assets.iter().map(|a| asset_json(a, &registry)).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    let console = context.console();
    let preference = preference(&console);
    let mut table = rich::Table::new();
    table
        .add_column("")
        .add_column("Name")
        .add_column("Size")
        .add_column("Kind")
        .add_column("Layer")
        .add_column("Alt text");
    for asset in &assets {
        table.add_row_cells(vec![
            rich::table::Cell::Text(rich_micro::placeholder(asset, preference)),
            shown_cell(asset.name()).into(),
            Text::new(asset.size().to_string()).into(),
            Text::new(asset.kind().as_str()).into(),
            Text::new(asset.origin().layer.as_str()).into(),
            shown_cell(asset.alt()).into(),
        ]);
    }
    let registry = Arc::new(registry);
    context.print(&registry, table);
    if let Some(untrusted) = untrusted_note(&report) {
        note(&format!("rich: note: {untrusted}"));
    }
    Ok(())
}

fn show(context: &Context, request: &Request) -> Result<(), Failure> {
    let name = request.one("a NAME")?;
    let (registry, _) = context.registry();
    let asset = Arc::clone(registry.require(name).map_err(|e| usage(e.to_string()))?);
    if context.json {
        let claims: Vec<serde_json::Value> = Layer::ALL
            .iter()
            .rev()
            .filter_map(|layer| registry.get(*layer, name))
            .map(|a| serde_json::json!({"layer": a.origin().layer.as_str(), "origin": a.origin().to_string()}))
            .collect();
        let mut value = asset_json(&asset, &registry);
        let files: Vec<serde_json::Value> = asset
            .variants()
            .iter()
            .map(|image| {
                serde_json::json!({
                    "path": image.path,
                    "format": image.info.format.as_str(),
                    "width": image.info.width,
                    "height": image.info.height,
                    "frames": image.info.frames,
                    "bytes": image.bytes,
                })
            })
            .collect();
        value["files"] = serde_json::Value::Array(files);
        value["claims"] = serde_json::Value::Array(claims);
        context.out_json(&value);
        return Ok(());
    }
    let console = context.console();
    let mut table = rich::Table::grid().padding(0, 2, 0, 0);
    table.add_column("").add_column("");
    let row = |table: &mut rich::Table, key: &str, value: Text| {
        table.add_row_cells(vec![
            Text::styled(key, Style::parse("bold").unwrap_or_default()).into(),
            value.into(),
        ]);
    };
    row(
        &mut table,
        "Asset",
        rich_micro::placeholder(&asset, preference(&console)),
    );
    for (key, value) in details(&asset) {
        row(&mut table, key, shown_cell(&value));
    }
    let registry = Arc::new(registry);
    context.print(&registry, table);
    if let Some(explanation) = registry.explain(name) {
        context.console().print(&explanation);
    }
    Ok(())
}

/// The metadata rows `show` and `preview` print.
fn details(asset: &MicroAsset) -> Vec<(&'static str, String)> {
    let fallback = asset.fallback();
    let mut rows = vec![
        ("Name", asset.name().to_string()),
        ("Size", format!("{} cells", asset.size())),
        ("Kind", asset.kind().as_str().to_string()),
        ("Alt text", asset.alt().to_string()),
        (
            "Fallback",
            format!(
                "emoji {} · text {}",
                fallback.emoji.as_deref().unwrap_or("-"),
                fallback.text.as_deref().unwrap_or("-")
            ),
        ),
        ("Origin", asset.origin().to_string()),
    ];
    if !asset.aliases().is_empty() {
        rows.push(("Aliases", asset.aliases().join(", ")));
    }
    for (key, value) in [
        ("Version", asset.version()),
        ("Licence", asset.license()),
        ("Author", asset.author()),
    ] {
        if let Some(value) = value {
            rows.push((key, value.to_string()));
        }
    }
    let files: Vec<String> = asset
        .variants()
        .iter()
        .map(|image| {
            let frames = if image.info.frames > 1 {
                format!(", {} frames", image.info.frames)
            } else {
                String::new()
            };
            format!(
                "{} ({} {}x{}{frames})",
                image.path,
                image.info.format.as_str(),
                image.info.width,
                image.info.height
            )
        })
        .collect();
    if !files.is_empty() {
        rows.push(("Files", files.join("\n")));
    }
    rows
}

/// The pipeline the options describe.
fn pipeline(request: &Request) -> Result<Pipeline, Failure> {
    let size = match request.value("--size") {
        Some(size) => size
            .parse::<CellSize>()
            .map_err(|e| usage(format!("--size: {e}")))?,
        None => CellSize::default(),
    };
    let mut pipeline = Pipeline::new(size);
    if let Some(cell) = request.value("--cell") {
        pipeline.cell = parse_cell(cell).ok_or_else(|| {
            usage(format!(
                "--cell: {cell:?} is not WxH pixels, like 8x16 (1 to 512 each)"
            ))
        })?;
    }
    if let Some(fit) = request.value("--fit") {
        pipeline.fit = match fit {
            "contain" => rich_art::ImageFit::Contain,
            "cover" => rich_art::ImageFit::Cover,
            "stretch" => rich_art::ImageFit::Stretch,
            other => {
                return Err(usage(format!(
                    "--fit: {other:?} is not contain, cover or stretch"
                )))
            }
        };
    }
    if let Some(anchor) = request.value("--anchor") {
        use rich_art::ImageAnchor as A;
        pipeline.anchor = match anchor.replace('_', "-").as_str() {
            "center" | "centre" => A::Center,
            "top" => A::Top,
            "bottom" => A::Bottom,
            "left" => A::Left,
            "right" => A::Right,
            "top-left" => A::TopLeft,
            "top-right" => A::TopRight,
            "bottom-left" => A::BottomLeft,
            "bottom-right" => A::BottomRight,
            other => return Err(usage(format!("--anchor: {other:?} is not an anchor"))),
        };
    }
    let number = |name: &str| -> Result<Option<f32>, Failure> {
        request
            .value(name)
            .map(|value| {
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| usage(format!("{name}: {value:?} is not a number")))
            })
            .transpose()
    };
    if let Some(v) = number("--brightness")? {
        pipeline.transforms.brightness = v;
    }
    if let Some(v) = number("--contrast")? {
        pipeline.transforms.contrast = v;
    }
    if let Some(v) = number("--gamma")? {
        pipeline.transforms.gamma = v;
    }
    pipeline.transforms.grayscale = request.flag("--grayscale");
    pipeline.sharpen = number("--sharpen")?;
    if let Some(mode) = request.value("--transparency") {
        pipeline.transparency = parse_transparency(mode).ok_or_else(|| {
            usage(format!(
                "--transparency: {mode:?} is not keep, threshold[:0-255], flatten:#rrggbb or \
                 key:#rrggbb"
            ))
        })?;
    }
    if let Some(colors) = request.value("--colors") {
        pipeline.colors = match colors {
            "truecolor" => rich_art::ImageColorMode::TrueColor,
            "256" => rich_art::ImageColorMode::Ansi256,
            "16" => rich_art::ImageColorMode::Ansi16,
            "grayscale" => rich_art::ImageColorMode::Grayscale,
            other => {
                return Err(usage(format!(
                    "--colors: {other:?} is not truecolor, 256, 16 or grayscale"
                )))
            }
        };
    }
    if let Some(dither) = request.value("--dither") {
        pipeline.dither = match dither {
            "none" => rich_art::Dither::None,
            "floyd-steinberg" | "floyd" => rich_art::Dither::FloydSteinberg,
            "bayer" => rich_art::Dither::Bayer4x4,
            "atkinson" => rich_art::Dither::Atkinson,
            other => return Err(usage(format!("--dither: {other:?} is not a dither"))),
        };
    }
    pipeline.validate().map_err(|e| usage(e.to_string()))?;
    Ok(pipeline)
}

fn parse_cell(value: &str) -> Option<rich_art::graphics::CellPixels> {
    let (w, h) = value.split_once(['x', 'X'])?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    ((1..=512).contains(&w) && (1..=512).contains(&h))
        .then(|| rich_art::graphics::CellPixels::new(w, h))
}

fn parse_hex(value: &str) -> Option<[u8; 3]> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn parse_transparency(value: &str) -> Option<Transparency> {
    let (mode, arg) = match value.split_once(':') {
        Some((mode, arg)) => (mode, Some(arg)),
        None => (value, None),
    };
    Some(match (mode, arg) {
        ("keep", None) => Transparency::Keep,
        ("threshold", None) => Transparency::Threshold(128),
        ("threshold", Some(alpha)) => Transparency::Threshold(alpha.parse().ok()?),
        ("flatten", Some(color)) => Transparency::Flatten(parse_hex(color)?),
        ("key", Some(color)) => Transparency::Key(parse_hex(color)?),
        _ => return None,
    })
}

/// Read an image file for the pipeline, within its source limit.
fn read_source(path: &Path) -> Result<Vec<u8>, Failure> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| input(format!("cannot read {}: {e}", path.display())))?;
    if metadata.len() > rich_micro::pipeline::MAX_SOURCE_BYTES as u64 {
        return Err(data(format!(
            "{} is larger than {} bytes",
            path.display(),
            rich_micro::pipeline::MAX_SOURCE_BYTES
        )));
    }
    std::fs::read(path).map_err(|e| input(format!("cannot read {}: {e}", path.display())))
}

/// Whether `path` holds a package or a pack (not a plain image).
fn is_package(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "richmicro" || e == "zip")
}

/// Frames magnified side by side: at most eight, and as many as fit
/// `width`.
fn filmstrip(frames: &[rich_art::graphics::AnimationFrame], width: usize) -> rich::Table {
    let mut table = rich::Table::grid().padding(0, 2, 0, 0);
    // As many as fit whole, one column per pixel and two between.
    let each = frames
        .first()
        .map_or(1, |frame| frame.image.width() as usize)
        + 2;
    let fit = ((width + 2) / each).clamp(1, 8);
    let shown = &frames[..frames.len().min(fit)];
    for _ in shown {
        table.add_column("");
    }
    table.add_row_cells(
        shown
            .iter()
            .map(|frame| rich::table::Cell::Renderable(Arc::new(Magnified(frame.image.clone()))))
            .collect(),
    );
    table.add_row_cells(
        shown
            .iter()
            .map(|frame| Text::new(format!("{} ms", frame.delay.as_millis())).into())
            .collect(),
    );
    table
}

fn preview(context: &Context, request: &Request) -> Result<(), Failure> {
    let target = request.one("a NAME, PACKAGE or IMAGE")?;
    let path = fs_path(target);
    let console = context.console();
    let detected = rich_micro::select(
        &rich_ext::graphics::GraphicsEnvironment::system(),
        &rich_ext::capabilities::SystemEnvironment,
    );
    let (mut registry, _) = context.registry();
    // A plain image: the pipeline's result, magnified.
    if path.is_file() && !is_package(&path) {
        let pipeline = pipeline(request)?;
        let processed = pipeline.process_bytes(&read_source(&path)?).map_err(data)?;
        let (width, height) = pipeline.canvas();
        if context.json {
            context.out_json(&serde_json::json!({
                "source": path.display().to_string(),
                "size": processed.size.to_string(),
                "canvas": format!("{width}x{height}"),
                "frames": processed.frames.len(),
                "animated": processed.animated(),
            }));
            return Ok(());
        }
        console.print(
            &Text::from_markup(&format!(
                "[bold]{}[/] → {} cells, {width}×{height} pixels, {} frame(s)",
                rich::markup::escape(&controls::shown(&path.display().to_string())),
                processed.size,
                processed.frames.len()
            ))
            .unwrap_or_default(),
        );
        console.print(&filmstrip(&processed.frames, console.width()));
        return Ok(());
    }
    let asset = if path.exists() {
        let asset =
            micro_package::load_package(&path, Layer::Inline, &Limits::default()).map_err(data)?;
        registry.add(Layer::Inline, asset.clone()).map_err(data)?;
        Arc::new(asset)
    } else {
        Arc::clone(registry.require(target).map_err(|e| usage(e.to_string()))?)
    };
    let cell = if request.value("--cell").is_some() {
        pipeline(request)?.cell
    } else {
        detected.cell
    };
    let mut cache = ImageCache::new(ImageCache::DEFAULT_BUDGET, None);
    let prepared = cache.prepare(&asset, cell, true, &Limits::default());
    if context.json {
        context.out_json(&serde_json::json!({
            "asset": asset_json(&asset, &registry),
            "mode": detected.mode.name(),
            "reason": detected.reason,
            "cell_pixels": format!("{}x{}", cell.width, cell.height),
            "cell_known": detected.cell_known,
            "frames": prepared.as_ref().map(|p| p.frames.len()),
            "duration_ms": prepared.as_ref().map(|p| p.duration().as_millis() as u64),
        }));
        return Ok(());
    }
    let registry = Arc::new(registry);
    let mut line = Text::from_markup("[bold]Inline:[/] Deploying ").unwrap_or_default();
    line = line.append_text(&rich_micro::placeholder(&asset, preference(&console)));
    line = line.append_text(&Text::new(" done"));
    context.print(&registry, line);
    let mut table = rich::Table::grid().padding(0, 2, 0, 0);
    table.add_column("").add_column("");
    for (key, value) in details(&asset) {
        table.add_row_cells(vec![
            Text::styled(key, Style::parse("bold").unwrap_or_default()).into(),
            shown_cell(&value).into(),
        ]);
    }
    table.add_row_cells(vec![
        Text::styled("Drawn as", Style::parse("bold").unwrap_or_default()).into(),
        Text::new(format!(
            "{} ({}), cells of {}x{} pixels{}",
            detected.mode,
            detected.reason,
            cell.width,
            cell.height,
            if detected.cell_known {
                ""
            } else {
                " (a guess)"
            }
        ))
        .into(),
    ]);
    console.print(&table);
    match prepared {
        Some(prepared) => {
            console.print(
                &Text::from_markup("[bold]Magnified[/] (one pixel per half cell):")
                    .unwrap_or_default(),
            );
            console.print(&filmstrip(&prepared.frames, console.width()));
        }
        None => console.print(&Text::new("(no image: only the fallback shows)")),
    }
    Ok(())
}

/// Copy a package or pack into `dest`: a directory's regular files (no
/// links), or one file.
fn copy_into(source: &Path, dest: &Path) -> Result<(), Failure> {
    let fail = |e: std::io::Error, path: &Path| input(format!("{}: {e}", path.display()));
    if source.is_dir() {
        std::fs::create_dir_all(dest).map_err(|e| fail(e, dest))?;
        for entry in std::fs::read_dir(source).map_err(|e| fail(e, source))? {
            let entry = entry.map_err(|e| fail(e, source))?;
            let kind = entry.file_type().map_err(|e| fail(e, &entry.path()))?;
            if kind.is_symlink() {
                continue;
            }
            let to = dest.join(entry.file_name());
            if kind.is_dir() {
                copy_into(&entry.path(), &to)?;
            } else if kind.is_file() {
                std::fs::copy(entry.path(), &to).map_err(|e| fail(e, &to))?;
            }
        }
        Ok(())
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| fail(e, parent))?;
        }
        std::fs::copy(source, dest)
            .map(drop)
            .map_err(|e| fail(e, dest))
    }
}

fn delete(path: &Path) -> Result<(), Failure> {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|e| input(format!("cannot remove {}: {e}", path.display())))
}

/// The package or pack's file name kept when copied: a folder's name, or an
/// archive's with its extension.
fn archive_extension(path: &Path) -> Option<&str> {
    (!path.is_dir())
        .then(|| path.extension().and_then(|e| e.to_str()))
        .flatten()
}

fn done(context: &Context, value: serde_json::Value, message: String) {
    if context.json {
        context.out_json(&value);
    } else {
        note(&message);
    }
}

fn add(context: &Context, request: &Request) -> Result<(), Failure> {
    let source = fs_path(request.one("a PACKAGE")?);
    let (layer, dir) = context.layer_dir(request.project)?;
    let asset = match micro_package::load(&source, layer, &Limits::default()).map_err(data)? {
        Loaded::Package(asset) => asset,
        Loaded::Pack(pack) => {
            return Err(usage(format!(
                "{} is a pack ({}); use `rich micro install`",
                source.display(),
                pack.name
            )))
        }
    };
    let (registry, _) = MicroRegistry::load(
        &MicroRoots {
            user: (layer == Layer::User).then(|| dir.clone()),
            project: (layer == Layer::Project).then(|| dir.clone()),
            project_trusted: true,
            ..MicroRoots::default()
        },
        &Limits::default(),
    );
    if let Some(existing) = registry.get(layer, asset.name()) {
        return Err(usage(format!(
            "{} is already in the {layer} layer ({}); remove it first",
            asset.name(),
            existing.origin()
        )));
    }
    let dest = match archive_extension(&source) {
        Some(ext) => dir.join(format!("{}.{ext}", asset.name().replace('/', "."))),
        None => package_path(&dir, asset.name(), false),
    };
    // `symlink_metadata`: a dangling link at `dest` is there too.
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Err(usage(format!("{} already exists", dest.display())));
    }
    copy_into(&source, &dest)?;
    // Read back where it landed, so a copy that broke is not left behind.
    if let Err(error) = micro_package::load_package(&dest, layer, &Limits::default()) {
        let _ = delete(&dest);
        return Err(data(error));
    }
    done(
        context,
        serde_json::json!({"name": asset.name(), "layer": layer.as_str(), "path": dest.display().to_string()}),
        format!(
            "added {} to the {layer} layer: {}",
            asset.name(),
            dest.display()
        ),
    );
    Ok(())
}

/// A layer's own assets, loaded from `dir` only.
fn layer_registry(layer: Layer, dir: &Path) -> MicroRegistry {
    let mut registry = MicroRegistry::new();
    registry.load_dir(
        layer,
        dir,
        &Limits::default(),
        &mut rich_micro::LoadReport::default(),
    );
    registry
}

fn remove(context: &Context, request: &Request) -> Result<(), Failure> {
    let name = request.one("a NAME")?;
    let (layer, dir) = context.layer_dir(request.project)?;
    let registry = layer_registry(layer, &dir);
    let asset = registry.get(layer, name).ok_or_else(|| {
        usage(format!(
            "no micro asset {name:?} in the {layer} layer ({})",
            dir.display()
        ))
    })?;
    if let Some(pack) = &asset.origin().pack {
        return Err(usage(format!(
            "{name} comes with pack {pack}; use `rich micro uninstall {pack}`"
        )));
    }
    let path = match &asset.origin().location {
        Some(PackageLocation::Directory(path)) => path.clone(),
        Some(PackageLocation::Archive { path, .. }) => path.clone(),
        _ => return Err(usage(format!("{name} has no package to remove"))),
    };
    delete(&path)?;
    done(
        context,
        serde_json::json!({"name": name, "layer": layer.as_str(), "path": path.display().to_string()}),
        format!("removed {name} from the {layer} layer: {}", path.display()),
    );
    Ok(())
}

fn create(context: &Context, request: &Request) -> Result<(), Failure> {
    let source = fs_path(request.one("an IMAGE")?);
    let name = request
        .value("--name")
        .ok_or_else(|| usage("micro create requires --name NAME"))?;
    let alt = request
        .value("--alt")
        .ok_or_else(|| usage("micro create requires --alt TEXT (alt text is mandatory)"))?;
    let pipeline = pipeline(request)?;
    let mut spec = PackageSpec::new(name, alt);
    spec.emoji = request.value("--emoji").map(str::to_string);
    spec.text = request.value("--text").map(str::to_string);
    spec.license = request.value("--license").map(str::to_string);
    spec.author = request.value("--author").map(str::to_string);
    spec.version = request.value("--version").map(str::to_string);
    let archive = request.flag("--archive");
    let (layer, dest) = match (request.value("--output"), request.flag("--add")) {
        (Some(_), true) => return Err(usage("--output and --add are exclusive")),
        (Some(output), false) => (None, fs_path(output)),
        (None, true) => {
            let (layer, dir) = context.layer_dir(request.project)?;
            (Some(layer), package_path(&dir, name, archive))
        }
        (None, false) => (None, package_path(&context.cwd, name, archive)),
    };
    let processed = pipeline
        .process_bytes(&read_source(&source)?)
        .map_err(data)?;
    let asset = write_package(&dest, &spec, &processed, archive).map_err(data)?;
    if context.json {
        context.out_json(&serde_json::json!({
            "path": dest.display().to_string(),
            "layer": layer.map(Layer::as_str),
            "asset": {
                "name": asset.name(),
                "size": asset.size().to_string(),
                "kind": asset.kind().as_str(),
                "frames": processed.frames.len(),
            },
        }));
        return Ok(());
    }
    note(&format!(
        "created {} ({} {}, {} frame(s)){}: {}",
        asset.name(),
        asset.size(),
        asset.kind().as_str(),
        processed.frames.len(),
        layer
            .map(|l| format!(" in the {l} layer"))
            .unwrap_or_default(),
        dest.display()
    ));
    let console = context.console();
    console.print(&filmstrip(&processed.frames, console.width()));
    Ok(())
}

fn install(context: &Context, request: &Request) -> Result<(), Failure> {
    let source = fs_path(request.one("a PACK")?);
    let (layer, dir) = context.layer_dir(request.project)?;
    let pack = micro_package::load_pack(&source, layer, &Limits::default()).map_err(data)?;
    let dest = match archive_extension(&source) {
        Some(ext) => dir.join(format!("{}.{ext}", pack.name.replace('/', "."))),
        None => dir.join(pack.name.replace('/', ".")),
    };
    // `symlink_metadata`: a dangling link at `dest` is there too.
    if std::fs::symlink_metadata(&dest).is_ok() {
        return Err(usage(format!(
            "pack {} is already installed: {}; uninstall it first",
            pack.name,
            dest.display()
        )));
    }
    copy_into(&source, &dest)?;
    if let Err(error) = micro_package::load_pack(&dest, layer, &Limits::default()) {
        let _ = delete(&dest);
        return Err(data(error));
    }
    for (package, error) in &pack.rejected {
        if !context.json {
            note(&format!(
                "rich: warning: {package} in pack {} not loaded: {error}",
                pack.name
            ));
        }
    }
    done(
        context,
        serde_json::json!({
            "pack": pack.name,
            "version": pack.version,
            "layer": layer.as_str(),
            "path": dest.display().to_string(),
            "assets": pack.assets.iter().map(|a| a.name()).collect::<Vec<_>>(),
            "rejected": pack.rejected.iter().map(|(p, e)| serde_json::json!({"package": p, "error": e.to_string()})).collect::<Vec<_>>(),
        }),
        format!(
            "installed pack {} ({} assets) in the {layer} layer: {}",
            pack.name,
            pack.assets.len(),
            dest.display()
        ),
    );
    Ok(())
}

/// The packs installed in `dir`: `(path, pack)`.
fn installed(layer: Layer, dir: &Path) -> Vec<(PathBuf, rich_micro::Pack)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(
            |path| match micro_package::load(&path, layer, &Limits::default()) {
                Ok(Loaded::Pack(pack)) => Some((path, pack)),
                _ => None,
            },
        )
        .collect()
}

fn uninstall(context: &Context, request: &Request) -> Result<(), Failure> {
    let name = request.one("a pack NAME")?;
    let (layer, dir) = context.layer_dir(request.project)?;
    let (path, _) = installed(layer, &dir)
        .into_iter()
        .find(|(_, pack)| pack.name == name)
        .ok_or_else(|| {
            usage(format!(
                "no pack {name:?} in the {layer} layer ({})",
                dir.display()
            ))
        })?;
    delete(&path)?;
    done(
        context,
        serde_json::json!({"pack": name, "layer": layer.as_str(), "path": path.display().to_string()}),
        format!(
            "uninstalled pack {name} from the {layer} layer: {}",
            path.display()
        ),
    );
    Ok(())
}

/// A pack as `packs` lists it: layer, name, version, where, asset names.
type PackRow = (Layer, String, Option<String>, String, Vec<String>);

fn packs(context: &Context) -> Result<(), Failure> {
    let mut rows: Vec<PackRow> = Vec::new();
    for pack in rich_micro::builtin::packs(Layer::BuiltIn)
        .into_iter()
        .flatten()
    {
        rows.push((
            Layer::BuiltIn,
            pack.name.clone(),
            pack.version.clone(),
            "built in".into(),
            pack.assets.iter().map(|a| a.name().to_string()).collect(),
        ));
    }
    let mut layers = Vec::new();
    if let Ok(user) = context.layer_dir(false) {
        layers.push(user);
    }
    let project = MicroRoots::project_dir(&context.cwd);
    let project_hidden = !context.trusted && project.is_dir();
    if context.trusted {
        layers.push((Layer::Project, project.clone()));
    }
    for (layer, dir) in layers {
        for (path, pack) in installed(layer, &dir) {
            rows.push((
                layer,
                pack.name.clone(),
                pack.version.clone(),
                path.display().to_string(),
                pack.assets.iter().map(|a| a.name().to_string()).collect(),
            ));
        }
    }
    if context.json {
        context.out_json(&serde_json::json!({
            "project_trusted": context.trusted,
            "packs": rows.iter().map(|(layer, name, version, path, assets)| serde_json::json!({
                "layer": layer.as_str(), "name": name, "version": version, "path": path, "assets": assets,
            })).collect::<Vec<_>>(),
        }));
        return Ok(());
    }
    let mut table = rich::Table::new();
    table
        .add_column("Pack")
        .add_column("Version")
        .add_column("Layer")
        .add_column("Assets")
        .add_column("Path");
    for (layer, name, version, path, assets) in &rows {
        table.add_row_cells(vec![
            shown_cell(name).into(),
            shown_cell(version.as_deref().unwrap_or("-")).into(),
            Text::new(layer.as_str()).into(),
            Text::new(assets.len().to_string()).into(),
            shown_cell(path).into(),
        ]);
    }
    context.console().print(&table);
    if project_hidden {
        note(&format!(
            "rich: note: {} is not loaded: the project is not trusted (pass --micro-project)",
            project.display()
        ));
    }
    Ok(())
}

/// The registry the CLI's commands use: the built-in set, the user's
/// layer, and the project's when `trusted`. Loading problems are warnings.
#[cfg(feature = "interact")]
pub(crate) fn registry(trusted: bool) -> MicroRegistry {
    let roots = ConfigRoots::default();
    let context = Context {
        trusted,
        home: roots.home,
        cwd: roots.cwd,
        no_color: false,
        json: false,
    };
    context.registry().0
}

/// Drawing for an interactive command, which paints on standard error:
/// the terminal's mode, chosen as for printing, but for standard error, so
/// `name=$(rich asset --kind micro)` still draws the list's images.
#[cfg(feature = "interact")]
pub(crate) fn picker_graphics(registry: Arc<MicroRegistry>) -> MicroGraphics {
    use rich_ext::capabilities::SystemEnvironment;
    use std::io::IsTerminal;
    let mut environment = rich_ext::graphics::GraphicsEnvironment::system();
    if !environment.interactive && std::io::stderr().is_terminal() {
        environment.interactive = true;
    }
    let selection = rich_micro::select(&environment, &SystemEnvironment);
    MicroGraphics::new(registry, selection).with_disk_cache(rich_micro::cache::user_cache_dir())
}

/// Run `component` with `graphics` drawing its micro assets, then delete
/// the images it transmitted (Kitty's), on standard error where they were
/// drawn.
#[cfg(feature = "interact")]
pub(crate) fn run_drawn<C: rich_interact::Component>(
    component: C,
    options: &rich_interact::RunOptions,
    graphics: &MicroGraphics,
) -> Result<rich_interact::Outcome<C::Output>, rich_interact::Error> {
    let outcome = rich_interact::run_with_graphics(component, options, graphics.source());
    let close = graphics.close();
    if !close.is_empty() {
        use std::io::Write;
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(close.as_bytes());
        let _ = stderr.flush();
    }
    outcome
}

/// `rich explore --icons`: the built-in status assets for true, false and
/// null.
#[cfg(feature = "interact")]
pub(crate) fn value_icon(registry: &MicroRegistry, node: &rich_ext::data::Node) -> Option<Text> {
    use rich_ext::data::Value;
    let name = match node.value {
        Value::Bool(true) => "status/success",
        Value::Bool(false) => "status/error",
        Value::Null => "status/info",
        _ => return None,
    };
    let asset = registry.resolve(name)?;
    Some(rich_micro::placeholder(asset, FallbackPreference::Emoji))
}

/// The registry `:micro:` markup expands against in this run: loaded once,
/// with its warnings, however many places (the text, the title, the
/// caption) hold tokens.
fn markup_registry(trusted: bool) -> Arc<MicroRegistry> {
    static LOADED: std::sync::Mutex<Option<(bool, Arc<MicroRegistry>)>> =
        std::sync::Mutex::new(None);
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((was, registry)) = loaded.as_ref() {
        if *was == trusted {
            return Arc::clone(registry);
        }
    }
    let roots = ConfigRoots::default();
    let context = Context {
        trusted,
        home: roots.home,
        cwd: roots.cwd,
        no_color: false,
        json: true,
    };
    let (registry, report) = MicroRegistry::load(&context.roots(), &Limits::default());
    warn(&report);
    let registry = Arc::new(registry);
    *loaded = Some((trusted, Arc::clone(&registry)));
    registry
}

/// The registry whose placeholders this run's output holds, once something
/// expanded a token: [`take_drawing`] hands it to the last step, which draws
/// them on a terminal.
static DRAWING: std::sync::Mutex<Option<Arc<MicroRegistry>>> = std::sync::Mutex::new(None);

fn draw_later(registry: &Arc<MicroRegistry>) {
    *DRAWING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(registry));
}

/// The registry to draw this run's placeholders with, if any expanded.
pub(crate) fn take_drawing() -> Option<Arc<MicroRegistry>> {
    DRAWING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Whether this run's output holds placeholders to draw.
pub(crate) fn drawing_pending() -> bool {
    DRAWING.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

fn has_tokens(content: &str) -> bool {
    content.contains(rich_micro::markup::TOKEN_PREFIX)
}

/// `:micro:name:` in `--print --emoji` markup: parse `content` with its
/// micro tokens as placeholders. `None` when there are no tokens.
pub(crate) fn print_markup(console: &Console, content: &str, trusted: bool) -> Option<Text> {
    if !has_tokens(content) {
        return None;
    }
    let registry = markup_registry(trusted);
    let prepared = rich_micro::PreparedMarkup::new(content, &registry, preference(console));
    let parsed = console.build_text(prepared.markup());
    draw_later(&registry);
    Some(prepared.finish(&parsed))
}

/// A `--panel`'s `--title` or `--caption` with micro tokens: parsed as the
/// panel parses a label (`Text.from_markup`, so `:emoji:` codes always
/// expand, and micro tokens with them). `None` when there are no tokens.
pub(crate) fn panel_label(console: &Console, label: &str, trusted: bool) -> Option<Text> {
    if !has_tokens(label) {
        return None;
    }
    let registry = markup_registry(trusted);
    let prepared = rich_micro::PreparedMarkup::new(label, &registry, preference(console));
    let markup = rich::emoji::replace(prepared.markup());
    let parsed = Text::from_markup(&markup).unwrap_or_else(|_| Text::new(markup));
    draw_later(&registry);
    Some(prepared.finish(&parsed))
}

/// A CSV table's `--title` or `--caption` with micro tokens: parsed as the
/// table parses one (`console.render_str`, so tokens expand with `--emoji`,
/// where `:emoji:` codes do), in `style`. `None` when there are no tokens or
/// no `--emoji`.
pub(crate) fn table_label(
    console: &Console,
    label: &str,
    style: &str,
    trusted: bool,
) -> Option<Text> {
    if !has_tokens(label) || !console.emoji() {
        return None;
    }
    let registry = markup_registry(trusted);
    let prepared = rich_micro::PreparedMarkup::new(label, &registry, preference(console));
    let mut text = prepared.finish(&console.render_str(prepared.markup(), Some(false)));
    text.set_base_style(rich::style::StyleType::from(style));
    draw_later(&registry);
    Some(text)
}

/// `-m/--markdown` with micro tokens: `content` with its tokens swapped for
/// stand-ins, which [`rich_micro::PreparedMarkdown::view`] turns back into
/// placeholders. `None` when there are no tokens.
pub(crate) fn markdown(
    console: &Console,
    content: &str,
    trusted: bool,
) -> Option<rich_micro::PreparedMarkdown> {
    if !has_tokens(content) {
        return None;
    }
    let registry = markup_registry(trusted);
    let prepared = rich_micro::PreparedMarkdown::new(content, &registry, preference(console));
    if !prepared.has_assets() {
        return None;
    }
    draw_later(&registry);
    Some(prepared)
}

/// `renderable` with its micro assets drawn, on a terminal.
pub(crate) fn drawn(
    console: &Console,
    registry: Arc<MicroRegistry>,
    renderable: Box<dyn Renderable>,
) -> Box<dyn Renderable> {
    /// A boxed renderable as a sized one, for `MicroView`.
    struct Boxed(Box<dyn Renderable>);
    impl Renderable for Boxed {
        fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
            self.0.rich_render(console, options)
        }
        fn measure(
            &self,
            console: &Console,
            options: &ConsoleOptions,
        ) -> rich::measure::Measurement {
            self.0.measure(console, options)
        }
    }
    if console.is_terminal() {
        Box::new(MicroGraphics::detect(registry).view(Boxed(renderable)))
    } else {
        renderable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn routing_and_parsing() {
        assert!(requested(&args(&["micro", "list"])));
        assert!(!requested(&args(&["-p", "micro"])));
        let request = request(&args(&["micro"])).unwrap();
        assert_eq!(request.sub, "list");
        let request = request_of(&[
            "micro",
            "create",
            "a.png",
            "--name",
            "x",
            "--alt=y",
            "--add",
            "--project",
            "--json",
        ]);
        assert_eq!(request.sub, "create");
        assert_eq!(request.positionals, ["a.png"]);
        assert_eq!(request.value("--alt"), Some("y"));
        assert!(request.flag("--add") && request.project && request.json);
        assert!(super::request(&args(&["micro", "frobnicate"])).is_err());
        assert!(super::request(&args(&["micro", "list", "--name", "x"])).is_err());
    }

    fn request_of(words: &[&str]) -> Request {
        request(&args(words)).unwrap()
    }

    #[test]
    fn trust_is_the_last_word() {
        assert!(!project_trusted(&args(&["micro"])));
        assert!(project_trusted(&args(&["--micro-project", "micro"])));
        assert!(!project_trusted(&args(&[
            "--micro-project",
            "--no-micro-project"
        ])));
    }

    #[test]
    fn transparency_and_cells_parse() {
        assert_eq!(parse_transparency("keep"), Some(Transparency::Keep));
        assert_eq!(
            parse_transparency("threshold:10"),
            Some(Transparency::Threshold(10))
        );
        assert_eq!(
            parse_transparency("key:#ffffff"),
            Some(Transparency::Key([255, 255, 255]))
        );
        assert_eq!(parse_transparency("flatten"), None);
        assert_eq!(
            parse_cell("10x20").map(|c| (c.width, c.height)),
            Some((10, 20))
        );
        assert!(parse_cell("0x20").is_none());
    }
}
