//! Third-party plugins in the binary: `rich plugins list` and `rich plugins
//! info NAME`, and the runtime plugins `--plugin PATH` (or a trusted config's
//! `plugins`) loads for a render.
//!
//! A binary-boundary convenience (docs/PORTING.md): it composes rs-rich-ext's
//! registry and loaders. Nothing loads unless the user asks: a runtime plugin
//! needs a build with the `dylib-plugins` or `wasm-plugins` feature, and a
//! path on the command line or in the user's own config. A project's
//! `./rich.toml` may not list plugins (see `config::UNTRUSTED_PLUGINS`).
use super::*;
use rich_ext::plugin::abi::ABI_MAJOR;
use rich_ext::plugin::PLUGIN_API_VERSION;
use rich_ext::plugin_loading::{self, LoadError, LoadOptions, RuntimeKind, RuntimePlugin};
use rich_ext::ExtensionRegistry;

/// The runtime plugins loaded for this run, added to every registry the
/// binary builds ([`add_to`]).
static RUNTIME: std::sync::Mutex<Vec<RuntimePlugin>> = std::sync::Mutex::new(Vec::new());

/// The exit class for a plugin that failed to load: a file that cannot be
/// read is an input error, anything else a usage or configuration error.
fn class(error: &LoadError) -> ExitClass {
    match error {
        LoadError::Io { .. } => ExitClass::Input,
        _ => ExitClass::Usage,
    }
}

/// Load the plugins at `paths` (each once), and check they register cleanly
/// alongside the built-ins and every linked plugin: a name another plugin
/// already provides is refused here, naming the file.
pub(crate) fn load(paths: &[String]) -> Result<Vec<RuntimePlugin>, (ExitClass, String)> {
    let mut seen = Vec::new();
    let mut loaded: Vec<RuntimePlugin> = Vec::new();
    for arg in paths {
        let path = fs_path(arg);
        let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let plugin = plugin_loading::load(&path, &LoadOptions::default())
            .map_err(|error| (class(&error), error.to_string()))?;
        loaded.push(plugin);
    }
    let mut registry = base_registry(MermaidBackend::Text);
    let _ = registry.add_linked_plugins();
    for plugin in &loaded {
        registry.add_plugin(plugin).map_err(|error| {
            (
                ExitClass::Usage,
                format!("plugin {}: {error}", plugin.path().display()),
            )
        })?;
    }
    Ok(loaded)
}

/// Make `plugins` part of every registry this run builds.
pub(crate) fn install(plugins: Vec<RuntimePlugin>) {
    if let Ok(mut slot) = RUNTIME.lock() {
        *slot = plugins;
    }
}

/// Linked plugins with a duplicate id (a build problem, not the user's) are
/// reported once, up front, instead of silently missing.
pub(crate) fn check_linked() -> Result<(), String> {
    ExtensionRegistry::new()
        .add_linked_plugins()
        .map(drop)
        .map_err(|error| format!("linked plugins: {error}"))
}

/// Add the linked plugins and this run's runtime plugins to `registry`,
/// after the built-ins. `load` and `check_linked` already reported any
/// refusal, so none is expected here.
pub(crate) fn add_to(registry: &mut ExtensionRegistry) {
    let _ = registry.add_linked_plugins();
    if let Ok(runtime) = RUNTIME.lock() {
        for plugin in runtime.iter() {
            let _ = registry.add_plugin(plugin);
        }
    }
}

/// Install the highlighters of the linked and runtime plugins onto a console
/// that already has the built-in ones.
pub(crate) fn install_highlighters(console: &mut Console) {
    let mut registry = ExtensionRegistry::new();
    add_to(&mut registry);
    registry.install(console);
}

// ---------------------------------------------------------------------------
// `rich plugins list` and `rich plugins info NAME`.
// ---------------------------------------------------------------------------

/// Whether the command line is `rich plugins …` (not `rich -p plugins`).
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("plugins")
}

/// What the command line asked `rich plugins` for.
struct Request {
    info: Option<String>,
    json: bool,
}

const USAGE: &str = "plugins [list | info NAME] [--plugin PATH]... [--report json] \
                     [--config PATH] [--profile NAME] [--no-config] [--no-color]";

fn request(args: &[String]) -> Result<Request, String> {
    let mut positionals = Vec::new();
    let mut json = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--plugin" | "--config" | "--profile" => {
                if iter.next().is_none() {
                    return Err(format!("{arg} requires a value"));
                }
            }
            "--report" => {
                if iter.next().map(String::as_str) != Some("json") {
                    return Err("--report requires json".into());
                }
                json = true;
            }
            "--no-config" | "--no-color" | "--color" => {}
            "--" => positionals.extend(iter.by_ref().cloned()),
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unexpected argument {other:?}; use {USAGE}"));
            }
            _ => positionals.push(arg.clone()),
        }
    }
    let info = match positionals
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["plugins"] | ["plugins", "list"] => None,
        ["plugins", "info", name] => Some(name.to_string()),
        ["plugins", "info"] => return Err("plugins info requires a plugin NAME".into()),
        _ => return Err(format!("expected {USAGE}")),
    };
    Ok(Request { info, json })
}

/// The `--plugin` paths the merged command line (config first) names.
fn plugin_paths(merged: &[String]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut iter = merged.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if arg == "--plugin" {
            paths.extend(iter.next().cloned());
        } else if VALUE_OPTIONS.contains(&arg.as_str()) || config::takes_value_option(arg) {
            iter.next();
        }
    }
    paths
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help")
    {
        // Config's `no_color` counts here too; a config error is reported
        // by the command, not by its help.
        let no_color = match config_args(args, &ConfigRoots::default()) {
            Ok(merged) => cli_spec::no_color_requested(&merged),
            Err(_) => cli_spec::no_color_requested(args),
        };
        let sub = args
            .iter()
            .find(|arg| matches!(arg.as_str(), "list" | "info"))
            .map(String::as_str);
        let mut path = vec!["plugins"];
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
    let merged = match config_args(args, &ConfigRoots::default()) {
        Ok(merged) => merged,
        Err(message) => return emit_error(json, ExitClass::Usage, &message),
    };
    // Colour as the rest of the binary decides it: from the merged command
    // line, so config's `no_color` counts, and a trusted config's
    // `no_color = false` beats NO_COLOR.
    let no_color = cli_spec::no_color_requested(&merged);
    if let Err(message) = check_linked() {
        return emit_error(json, ExitClass::Usage, &message);
    }
    match load(&plugin_paths(&merged)) {
        Ok(plugins) => install(plugins),
        Err((class, message)) => return emit_error(json, class, &controls::shown(&message)),
    }
    let entries = entries();
    let shown: Vec<&Entry> = match &request.info {
        None => entries.iter().collect(),
        Some(name) => match entries.iter().find(|entry| entry.id == *name) {
            Some(entry) => vec![entry],
            None => {
                let names: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
                return emit_error(
                    json,
                    ExitClass::Usage,
                    &format!(
                        "unknown plugin {:?}; plugins: {}",
                        controls::shown(name),
                        names.join(", ")
                    ),
                );
            }
        },
    };
    let text = if request.json {
        let value: Vec<serde_json::Value> = shown.iter().map(|entry| entry.json()).collect();
        let value = if request.info.is_some() {
            value.into_iter().next().unwrap_or_default()
        } else {
            serde_json::json!({
                "plugin_api": PLUGIN_API_VERSION,
                "abi": format!("{ABI_MAJOR}.{}", rich_ext::plugin::abi::ABI_MINOR),
                "runtime_loading": {
                    "native": RuntimeKind::Native.supported(),
                    "wasm": RuntimeKind::Wasm.supported(),
                },
                "plugins": value,
            })
        };
        serde_json::to_string_pretty(&value).expect("plugin JSON") + "\n"
    } else if request.info.is_some() {
        shown[0].info(no_color)
    } else {
        list(&shown, no_color)
    };
    authoring::out(&text);
    ExitCode::SUCCESS
}

/// One plugin as `rich plugins` shows it.
struct Entry {
    id: String,
    name: String,
    version: String,
    description: Option<String>,
    /// `built-in`, `linked`, `native` or `wasm`.
    source: &'static str,
    path: Option<PathBuf>,
    /// `plugin API 1`, or for a runtime plugin its ABI version.
    abi: String,
    capabilities: Vec<String>,
}

impl Entry {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "version": self.version,
            "description": self.description,
            "source": self.source,
            "path": self.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "abi": self.abi,
            "capabilities": self.capabilities,
        })
    }

    fn info(&self, no_color: bool) -> String {
        let mut table = rich::Table::grid().padding(0, 2, 0, 0);
        table.add_column("");
        table.add_column("");
        let mut rows = vec![
            ("Name", self.name.clone()),
            ("Version", self.version.clone()),
            ("Source", self.source.to_string()),
        ];
        if let Some(path) = &self.path {
            rows.push(("Path", path.display().to_string()));
        }
        rows.push(("ABI", self.abi.clone()));
        rows.push(("Capabilities", self.capabilities.join("\n")));
        if let Some(description) = &self.description {
            rows.push(("Description", description.clone()));
        }
        for (key, value) in rows {
            table.add_row(&[
                &format!("[bold]{key}[/]"),
                &rich::markup::escape(&rich_ext::sanitize::sanitize_terminal_controls(&value)),
            ]);
        }
        let title = format!("[bold]{}[/]\n", rich::markup::escape(&self.id));
        cli_spec::render(&Text::from_markup(&title).unwrap_or_default(), no_color)
            + "\n"
            + &cli_spec::render(&table, no_color)
            + "\n"
    }
}

fn list(entries: &[&Entry], no_color: bool) -> String {
    let mut table = rich::Table::new();
    for column in ["Name", "Version", "Source", "ABI", "Capabilities"] {
        table.add_column(column);
    }
    for entry in entries {
        let cells: Vec<String> = [
            entry.id.clone(),
            entry.version.clone(),
            entry.source.to_string(),
            entry.abi.clone(),
            entry.capabilities.join(", "),
        ]
        .iter()
        .map(|cell| rich::markup::escape(&rich_ext::sanitize::sanitize_single_line(cell)))
        .collect();
        let cells: Vec<&str> = cells.iter().map(String::as_str).collect();
        table.add_row(&cells);
    }
    let mut out = cli_spec::render(&table, no_color);
    out.push('\n');
    if !RuntimeKind::Native.supported() || !RuntimeKind::Wasm.supported() {
        let missing: Vec<&str> = [RuntimeKind::Native, RuntimeKind::Wasm]
            .into_iter()
            .filter(|kind| !kind.supported())
            .map(RuntimeKind::feature)
            .collect();
        out.push_str(&format!(
            "Runtime plugins load with --plugin PATH; this build lacks {}.\n",
            missing.join(" and ")
        ));
    }
    out
}

/// The built-ins: the registry every render starts from, before linked and
/// runtime plugins.
fn base_registry(mermaid: MermaidBackend) -> ExtensionRegistry {
    super::builtin_registry(mermaid)
}

/// Every plugin this run has, built-ins first, then linked plugins (sorted by
/// id), then runtime plugins in the order given.
fn entries() -> Vec<Entry> {
    let builtin: Vec<String> = base_registry(MermaidBackend::Text)
        .plugins()
        .iter()
        .map(|plugin| plugin.metadata.id.clone())
        .collect();
    let runtime = RUNTIME.lock().map(|r| r.clone()).unwrap_or_default();
    let registry = super::plugin_registry(MermaidBackend::Text);
    registry
        .plugins()
        .iter()
        .map(|plugin| {
            let metadata = &plugin.metadata;
            let loaded = runtime.iter().find(|r| r.abi().name == metadata.id);
            let (source, path, abi, capabilities) = match loaded {
                Some(runtime) => (
                    runtime.kind().as_str(),
                    Some(runtime.path().to_path_buf()),
                    format!(
                        "{} ABI {}.{}",
                        match runtime.kind() {
                            RuntimeKind::Native => "C",
                            RuntimeKind::Wasm => "WASM",
                        },
                        runtime.abi().abi_major,
                        runtime.abi().abi_minor
                    ),
                    runtime
                        .abi()
                        .capabilities
                        .iter()
                        .map(|c| format!("{} {:?}", c.kind, c.name))
                        .collect(),
                ),
                None => (
                    if builtin.contains(&metadata.id) {
                        "built-in"
                    } else {
                        "linked"
                    },
                    None,
                    format!("plugin API {}", metadata.api_version),
                    plugin
                        .capabilities
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                ),
            };
            Entry {
                id: metadata.id.clone(),
                name: metadata.name.clone(),
                version: metadata.version.clone(),
                description: metadata.description.clone(),
                source,
                path,
                abi,
                capabilities,
            }
        })
        .collect()
}
