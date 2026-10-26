//! `rich plugin`: find, install, update and check packages from the plugin
//! marketplace, on rs-rich-marketplace (docs/design/marketplace.md).
//!
//! A binary-boundary convenience (docs/PORTING.md): the crate verifies,
//! resolves and installs; this module prompts, prints and decides. Nothing
//! here changes how a render behaves, apart from one rule: an installed,
//! enabled WASM plugin loads on every run without `--plugin`, after its file
//! is checked against the lockfile ([`installed_plugins`]).
//!
//! - **The install directory** is `--plugin-dir DIR`, else `RICH_PLUGIN_DIR`,
//!   else `$XDG_DATA_HOME/rich/plugins` (`~/.local/share/rich/plugins`;
//!   `%LOCALAPPDATA%\rich\plugins` on Windows). The crate's lockfile
//!   (`marketplace.lock.json`) is there, and this module's own state
//!   (`marketplace.state.json`): the disabled packages, the last index each
//!   marketplace served (to refuse a rollback), the newest key generation
//!   verified, and the yank reasons last seen for what is installed.
//! - **The marketplace** is `--marketplace LOCATION`, else
//!   `RICH_MARKETPLACE`, else [`DEFAULT_MARKETPLACE`]: an `https://` base
//!   URL, or a local directory (a path or `file://` URL) laid out as the
//!   marketplace repository is. A local marketplace serves artifacts from
//!   its `artifacts/HOST/PATH` mirror only, never from the network, which
//!   is how the tests and the CI compatibility job run offline.
//! - **The trust root** is `--marketplace-keys FILE` (a `keys-N.json`), else
//!   `RICH_MARKETPLACE_KEYS`, else the keys compiled into
//!   rs-rich-marketplace. Later generations are fetched from the
//!   marketplace's `keys/` and accepted one at a time.
use super::*;

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::time::SystemTime;

use rich_ext::plugin_loading::{RuntimeKind, RuntimePlugin};
use rich_marketplace::{
    resolve, verify_index, AcceptedIndex, ArtifactChecker, CheckError, FetchError, Host,
    IndexError, InstallError, Installer, KeyChainError, KeySet, Kind, ListedVersion, Listing,
    ListingError, LockEntry, Lockfile, LockfileError, Request, Requires, ResolveError, Rotation,
    RotationKind, Tier, Transport, VerifiedIndex, WasmExpectation, LOCKFILE_NAME,
};
use serde_json::{json, Value};

#[path = "marketplace_publish.rs"]
mod publish;

#[cfg(feature = "interact")]
#[path = "marketplace_browse.rs"]
mod browse;

/// The marketplace a build uses when nothing else is named.
pub(crate) const DEFAULT_MARKETPLACE: &str =
    "https://raw.githubusercontent.com/buchochelliq-labs/rs-rich-marketplace/main/";

/// This module's state file, beside the lockfile.
const STATE_NAME: &str = "marketplace.state.json";
const STATE_SCHEMA_VERSION: u64 = 1;

/// The most read of each marketplace file.
const MAX_INDEX_BYTES: u64 = 8 << 20;
const MAX_SIGNATURE_BYTES: u64 = 8 << 10;
const MAX_LISTING_BYTES: u64 = 1 << 20;
const MAX_KEYS_BYTES: u64 = 64 << 10;
/// Key generations walked in one refresh, so a hostile marketplace cannot
/// keep a client fetching.
const MAX_ROTATIONS: u64 = 64;

/// The exit code of an install or update the user declined.
const EXIT_DECLINED: u8 = 1;

/// The subcommands, in the order help lists them.
pub(super) const COMMANDS: &[&str] = &[
    "search", "info", "install", "update", "remove", "list", "enable", "disable", "verify",
    "browse", "publish",
];

/// Whether the command line is `rich plugin …` (not `rich -p plugin`).
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("plugin")
}

/// `rich plugin`'s help, registered with the root spec in `cli_spec`.
pub(crate) fn spec() -> rich_ext::cli_doc::CommandSpec {
    use rich_ext::cli_doc::{ArgSpec, CommandSpec};
    let common = || {
        vec![
            ArgSpec::option("marketplace").value_name("LOCATION").help(
                "The marketplace: an https:// base URL, or a directory or file:// URL laid out as \
                 the marketplace repository is (artifacts from its artifacts/HOST/PATH mirror, \
                 never the network); else RICH_MARKETPLACE, else the official one",
            ),
            ArgSpec::option("marketplace-keys").value_name("FILE").help(
                "Trust this keys-N.json instead of the keys compiled in; else \
                 RICH_MARKETPLACE_KEYS",
            ),
            ArgSpec::option("plugin-dir").value_name("DIR").help(
                "Install into DIR; else RICH_PLUGIN_DIR, else $XDG_DATA_HOME/rich/plugins \
                 (~/.local/share/rich/plugins)",
            ),
            ArgSpec::option("report")
                .value_name("FORMAT")
                .help("json: the result or error envelope on stderr, nothing on stdout"),
        ]
    };
    let with_common = |mut command: CommandSpec| {
        for arg in common() {
            command = command.arg(arg);
        }
        command
    };
    let id = || ArgSpec::positional("id").help("The package's id, as `search` shows it");
    let ids = || ArgSpec::positional("id").multiple(true).help("Package ids");
    let yes = || {
        ArgSpec::flag("yes")
            .short('y')
            .help("Install without asking (for scripts); the summary is still shown")
    };
    let publish_option =
        |name: &str, value: &str, help: &str| ArgSpec::option(name).value_name(value).help(help);
    CommandSpec::new("plugin")
        .about(
            "Find, install, update and check plugins and asset packs from the plugin marketplace: \
             a signed index, every file checked by SHA-256; an installed, enabled WASM plugin \
             loads on every run without --plugin, after its file is checked against the lockfile",
        )
        .usage(
            "plugin COMMAND [ARGS] [--marketplace LOCATION] [--marketplace-keys FILE] \
             [--plugin-dir DIR] [--report json]",
        )
        .subcommand(with_common(
            CommandSpec::new("search")
                .about("List the marketplace's packages, or those whose id, name, description or keywords contain QUERY")
                .usage("search [QUERY]")
                .arg(ArgSpec::positional("query").help("Text to look for, in any case")),
        ))
        .subcommand(with_common(
            CommandSpec::new("info")
                .about("One package: its tier, source, versions, what loads here, and for a crate the `cargo add` line")
                .usage("info ID")
                .arg(id()),
        ))
        .subcommand(with_common(
            CommandSpec::new("install")
                .about(
                    "Show the tier, permissions and capabilities, ask, then download, check \
                     (size, SHA-256, the WASM manifest in the sandbox) and install; a crate is \
                     refused with its `cargo add` line",
                )
                .usage("install ID[@VERSION]... [--yes]")
                .arg(ArgSpec::positional("id").multiple(true).help(
                    "Package ids, each optionally @VERSION (the only way to get a pre-release)",
                ))
                .arg(yes())
                .example("rich plugin install shout", "The newest version this host loads")
                .example("rich plugin install shout@0.2.0 --yes", "Exactly 0.2.0, without asking"),
        ))
        .subcommand(with_common(
            CommandSpec::new("update")
                .about("Move installed packages (or those named) to the newest version this host loads, asking first; a yanked version is replaced")
                .usage("update [ID...] [--yes]")
                .arg(ids())
                .arg(yes()),
        ))
        .subcommand(with_common(
            CommandSpec::new("remove")
                .about("Uninstall packages: the lockfile entry, then the file")
                .usage("remove ID...")
                .arg(ids()),
        ))
        .subcommand(with_common(
            CommandSpec::new("list")
                .about("What is installed: version, kind, tier, enabled, whether the file still matches the lockfile, and yank reasons last seen; no network")
                .usage("list"),
        ))
        .subcommand(with_common(
            CommandSpec::new("enable")
                .about("Load an installed package again on every run")
                .usage("enable ID...")
                .arg(ids()),
        ))
        .subcommand(with_common(
            CommandSpec::new("disable")
                .about("Keep a package installed but do not load it")
                .usage("disable ID...")
                .arg(ids()),
        ))
        .subcommand(with_common(
            CommandSpec::new("verify")
                .about(
                    "Check every installed file against the lockfile, and every installed \
                     version against the current index; yanked versions are reported with their \
                     reason; exits 4 when a file or listing does not match",
                )
                .usage("verify"),
        ))
        .subcommand(with_common(
            CommandSpec::new("browse")
                .about("The marketplace in a selector, with each package's provenance (tier, source, hash, permissions) beside the list; Enter installs, updates, removes, enables or disables")
                .usage("browse"),
        ))
        .subcommand(
            CommandSpec::new("publish")
                .about(
                    "Check a local package (a WASM plugin, a micro pack or a theme), compute its \
                     SHA-256 and size, and write its listing JSON for a pull request to the \
                     marketplace repository; no network",
                )
                .usage(
                    "publish FILE --url URL [--listing FILE | --license SPDX --repository URL \
                     [--description TEXT]] [--source-revision COMMIT] [--output FILE]",
                )
                .arg(ArgSpec::positional("file").help("The .wasm, .zip/.richmicro or .ini/.theme file"))
                .arg(publish_option("url", "URL", "Where the file will be downloaded from (https://, ending in its extension)"))
                .arg(publish_option("listing", "FILE", "An existing listing to add this version to (it is rewritten unless --output is given)"))
                .arg(publish_option("output", "FILE", "Where to write the listing (default packages/ID.json)"))
                .arg(publish_option("source-revision", "COMMIT", "The full Git commit the artifact was built from (required for a WASM plugin)"))
                .arg(publish_option("id", "ID", "A theme's id (default: the file name); for a plugin or pack, checked against its manifest"))
                .arg(publish_option("version", "VERSION", "A theme's version; for a plugin or pack, checked against its manifest"))
                .arg(publish_option("name", "TEXT", "The human-readable name (default: the id)"))
                .arg(publish_option("description", "TEXT", "One line on what it adds (default: a WASM plugin's own)"))
                .arg(publish_option("tier", "TIER", "community (the default), reviewed or official; raising it needs a maintainer"))
                .arg(publish_option("license", "SPDX", "The SPDX license expression"))
                .arg(publish_option("repository", "URL", "The source repository, https://"))
                .arg(publish_option("homepage", "URL", "A homepage, https://"))
                .arg(publish_option("author", "NAME", "An author; repeatable"))
                .arg(publish_option("keyword", "WORD", "A search keyword; repeatable, at most 8"))
                .arg(publish_option("published", "DATE", "The listing date, YYYY-MM-DD (default: today, UTC)"))
                .example(
                    "rich plugin publish shout.wasm --url https://github.com/me/shout/releases/download/v0.2.0/shout.wasm --source-revision $(git rev-parse HEAD) --license MIT --repository https://github.com/me/shout",
                    "A new listing, packages/shout.json",
                )
                .example(
                    "rich plugin publish shout.wasm --url URL --source-revision COMMIT --listing packages/shout.json",
                    "Add a version to an existing listing",
                ),
        )
        .section(
            "Environment",
            "- RICH_MARKETPLACE: the marketplace, as --marketplace\n\
             - RICH_MARKETPLACE_KEYS: a keys-N.json to trust, as --marketplace-keys\n\
             - RICH_PLUGIN_DIR: the install directory, as --plugin-dir; also where every run \
             looks for installed plugins",
        )
}

// ---------------------------------------------------------------------------
// Failures and their exit classes.
// ---------------------------------------------------------------------------

/// A failure, with the exit class it reports.
#[derive(Debug)]
pub(crate) struct Failure {
    pub class: ExitClass,
    pub message: String,
}

pub(crate) type Outcome<T> = Result<T, Failure>;

pub(crate) fn usage(message: impl Into<String>) -> Failure {
    Failure {
        class: ExitClass::Usage,
        message: message.into(),
    }
}

pub(crate) fn input(message: impl Into<String>) -> Failure {
    Failure {
        class: ExitClass::Input,
        message: message.into(),
    }
}

pub(crate) fn data(message: impl Into<String>) -> Failure {
    Failure {
        class: ExitClass::Data,
        message: message.into(),
    }
}

fn index_failure(error: IndexError) -> Failure {
    data(format!("marketplace index refused: {error}"))
}

fn keys_failure(error: KeyChainError) -> Failure {
    match error {
        KeyChainError::NoTrustRoot => usage(
            "this build has no marketplace keys compiled in yet; pass --marketplace-keys FILE \
             (or set RICH_MARKETPLACE_KEYS) to a keys-N.json you trust",
        ),
        other => data(format!("marketplace keys refused: {other}")),
    }
}

fn listing_failure(error: ListingError) -> Failure {
    match error {
        ListingError::NotInIndex(id) => usage(format!(
            "no package {:?} in the marketplace",
            controls::shown(&id)
        )),
        other => data(other.to_string()),
    }
}

fn resolve_failure(error: ResolveError) -> Failure {
    match error {
        ResolveError::NoSuchVersion { .. } => usage(error.to_string()),
        other => data(other.to_string()),
    }
}

fn fetch_failure(error: FetchError) -> Failure {
    match error {
        FetchError::TooLarge { .. } | FetchError::NotHttps { .. } => data(error.to_string()),
        other => input(other.to_string()),
    }
}

fn lockfile_failure(error: LockfileError) -> Failure {
    match error {
        LockfileError::Io { .. } => input(error.to_string()),
        LockfileError::NotInstalled(_) => usage(error.to_string()),
        other => data(other.to_string()),
    }
}

fn install_failure(error: InstallError) -> Failure {
    match error {
        InstallError::NotInstallable { .. } => usage(error.to_string()),
        InstallError::Index(e) => index_failure(e),
        InstallError::Listing(e) => listing_failure(e),
        InstallError::Resolve(e) => resolve_failure(e),
        InstallError::Fetch(e) => fetch_failure(e),
        InstallError::Lockfile(e) => lockfile_failure(e),
        other => data(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// The command line.
// ---------------------------------------------------------------------------

/// What `rich plugin …` was asked.
#[derive(Debug, Default)]
pub(crate) struct Options {
    pub command: String,
    pub positionals: Vec<String>,
    pub json: bool,
    pub no_color: bool,
    pub yes: bool,
    pub marketplace: Option<String>,
    pub plugin_dir: Option<PathBuf>,
    pub keys: Option<PathBuf>,
    /// `publish`'s options, by name without `--`; repeatable ones keep
    /// every value.
    pub publish: BTreeMap<&'static str, Vec<String>>,
}

/// `publish`'s options that take a value.
const PUBLISH_OPTIONS: &[&str] = &[
    "--url",
    "--listing",
    "--output",
    "--id",
    "--name",
    "--description",
    "--tier",
    "--license",
    "--repository",
    "--homepage",
    "--source-revision",
    "--version",
    "--published",
    "--author",
    "--keyword",
];

fn parse(args: &[String]) -> Outcome<Options> {
    let mut options = Options::default();
    let mut words = Vec::new();
    let mut iter = args.iter();
    let mut no_color = None;
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| usage(format!("{name} requires a value")))
        };
        match arg.as_str() {
            "--marketplace" => options.marketplace = Some(value(arg)?),
            "--plugin-dir" => options.plugin_dir = Some(fs_path(&value(arg)?)),
            "--marketplace-keys" => options.keys = Some(fs_path(&value(arg)?)),
            "--report" => {
                options.json = match value(arg)?.as_str() {
                    "json" => true,
                    "human" => false,
                    other => {
                        return Err(usage(format!(
                            "unknown report format {other:?} (human, json)"
                        )))
                    }
                }
            }
            "--machine-json" => options.json = true,
            "--yes" | "-y" => options.yes = true,
            "--no-color" => no_color = Some(true),
            "--color" => no_color = Some(false),
            "--" => words.extend(iter.by_ref().cloned()),
            other if other.starts_with("--") => {
                match PUBLISH_OPTIONS.iter().find(|known| **known == other) {
                    Some(known) => {
                        let v = value(other)?;
                        options.publish.entry(&known[2..]).or_default().push(v);
                    }
                    None => {
                        return Err(usage(format!(
                            "unexpected argument {:?}; see `rich plugin --help`",
                            controls::shown(other)
                        )))
                    }
                }
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(usage(format!(
                    "unexpected argument {:?}; see `rich plugin --help`",
                    controls::shown(other)
                )))
            }
            _ => words.push(arg.clone()),
        }
    }
    // The first word is `plugin` itself.
    let mut words = words.into_iter().skip(1);
    options.command = words.next().unwrap_or_default();
    options.positionals = words.collect();
    if options.command.is_empty() {
        return Err(usage(format!(
            "rich plugin needs a command: {}",
            COMMANDS.join(", ")
        )));
    }
    if !COMMANDS.contains(&options.command.as_str()) {
        return Err(usage(format!(
            "unknown command {:?}; commands: {} (`rich plugins` lists what this run has loaded)",
            controls::shown(&options.command),
            COMMANDS.join(", ")
        )));
    }
    if options.command != "publish" && !options.publish.is_empty() {
        let names: Vec<String> = options.publish.keys().map(|k| format!("--{k}")).collect();
        return Err(usage(format!(
            "{} only apply to `rich plugin publish`",
            names.join(", ")
        )));
    }
    let env_no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    options.no_color = no_color.unwrap_or(env_no_color);
    Ok(options)
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h")
    {
        let no_color = cli_spec::no_color_requested(args);
        let sub = args
            .iter()
            .find(|arg| COMMANDS.contains(&arg.as_str()))
            .map(String::as_str);
        let mut path = vec!["plugin"];
        path.extend(sub);
        if let Some(help) = cli_spec::subcommand_help(&path, no_color) {
            authoring::out(&format!("{help}\n"));
        }
        return ExitCode::SUCCESS;
    }
    let options = match parse(args) {
        Ok(options) => options,
        Err(failure) => return emit_error(json, failure.class, &failure.message),
    };
    let result = match options.command.as_str() {
        "search" => search(&options),
        "info" => info(&options),
        "install" => install(&options),
        "update" => update(&options),
        "remove" => remove(&options),
        "list" => list(&options),
        "enable" => set_enabled(&options, true),
        "disable" => set_enabled(&options, false),
        "verify" => verify(&options),
        "publish" => publish::run(&options),
        #[cfg(feature = "interact")]
        "browse" => browse::run(&options),
        #[cfg(not(feature = "interact"))]
        "browse" => Err(usage(
            "`rich plugin browse` needs a build with the interact feature; use `rich plugin \
             search` and `rich plugin info`",
        )),
        _ => unreachable!("checked by parse"),
    };
    finish(&options, result)
}

/// What a command did: the text for a person, and the `result` object of
/// the JSON envelope.
pub(crate) struct Done {
    pub text: String,
    pub result: Value,
    /// Set when the command ran but found problems (`verify`): reported
    /// with this class, `ok: false`, and the result still included.
    pub problem: Option<Failure>,
    /// The user declined: exit 1.
    pub declined: bool,
}

impl Done {
    pub(crate) fn new(text: String, result: Value) -> Self {
        Done {
            text,
            result,
            problem: None,
            declined: false,
        }
    }
}

fn finish(options: &Options, result: Outcome<Done>) -> ExitCode {
    match result {
        Err(failure) => emit_error(options.json, failure.class, &failure.message),
        Ok(done) => {
            if !options.json {
                authoring::out(&done.text);
            }
            if done.declined {
                let message = "cancelled; nothing was installed";
                if options.json {
                    eprintln!(
                        "{}",
                        json!({
                            "ok": false, "code": "cancelled", "exit_code": EXIT_DECLINED,
                            "message": message, "error": {"message": message},
                            "result": done.result,
                        })
                    );
                } else {
                    eprintln!("rich: {message}");
                }
                return ExitCode::from(EXIT_DECLINED);
            }
            match done.problem {
                Some(failure) => {
                    let message = controls::shown(&failure.message);
                    if options.json {
                        eprintln!(
                            "{}",
                            json!({
                                "ok": false, "code": failure.class.name(),
                                "exit_code": failure.class.code(), "message": message,
                                "error": {"message": message}, "result": done.result,
                            })
                        );
                    } else {
                        eprintln!("rich: {message}");
                    }
                    failure.class.exit_code()
                }
                None => {
                    if options.json {
                        eprintln!(
                            "{}",
                            json!({
                                "ok": true, "code": ExitClass::Success.name(),
                                "exit_code": ExitClass::Success.code(), "result": done.result,
                            })
                        );
                    }
                    ExitCode::SUCCESS
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The install directory and this module's state.
// ---------------------------------------------------------------------------

/// The install directory: `explicit`, else `RICH_PLUGIN_DIR`, else the
/// platform's data directory.
pub(crate) fn plugin_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = explicit {
        return Some(dir.to_path_buf());
    }
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = env("RICH_PLUGIN_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        if let Some(local) = env("LOCALAPPDATA") {
            return Some(PathBuf::from(local).join("rich").join("plugins"));
        }
    }
    if let Some(data) = env("XDG_DATA_HOME").map(PathBuf::from) {
        if data.is_absolute() {
            return Some(data.join("rich").join("plugins"));
        }
    }
    env("HOME")
        .or_else(|| env("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".local/share/rich/plugins"))
}

fn require_dir(options: &Options) -> Outcome<PathBuf> {
    plugin_dir(options.plugin_dir.as_deref()).ok_or_else(|| {
        usage("no install directory: set HOME, RICH_PLUGIN_DIR or pass --plugin-dir DIR")
    })
}

/// What one marketplace served last.
#[derive(Clone, Debug, Default, PartialEq)]
struct MarketState {
    accepted: Option<AcceptedIndex>,
    /// The newest key generation verified, as its `keys-N.json` value, and
    /// the main key id of the trust root it was walked from.
    keys: Option<(Value, String)>,
}

/// `marketplace.state.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct State {
    disabled: Vec<String>,
    markets: BTreeMap<String, MarketState>,
    /// The yank reason last seen for an installed version: `(version, reason)`.
    yanked: BTreeMap<String, (String, String)>,
}

impl State {
    fn path(dir: &Path) -> PathBuf {
        dir.join(STATE_NAME)
    }

    /// Read the state; a missing file is the empty state.
    pub(crate) fn load(dir: &Path) -> Outcome<State> {
        let path = State::path(dir);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
            Err(e) => return Err(input(format!("{}: {e}", path.display()))),
        };
        let bad = |why: &str| data(format!("{}: {why}", path.display()));
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| bad(&e.to_string()))?;
        if value.get("schema_version").and_then(Value::as_u64) != Some(STATE_SCHEMA_VERSION) {
            return Err(bad("unsupported or missing schema_version"));
        }
        let strings = |v: Option<&Value>| -> Vec<String> {
            v.and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut state = State {
            disabled: strings(value.get("disabled")),
            ..State::default()
        };
        if let Some(markets) = value.get("marketplaces").and_then(Value::as_object) {
            for (location, market) in markets {
                let accepted = match (
                    market.get("serial").and_then(Value::as_u64),
                    market.get("sha256").and_then(Value::as_str),
                ) {
                    (Some(serial), Some(sha256)) => Some(AcceptedIndex {
                        serial,
                        sha256: sha256.to_string(),
                    }),
                    _ => None,
                };
                let keys = match (
                    market.get("keys"),
                    market.get("root").and_then(Value::as_str),
                ) {
                    (Some(keys), Some(root)) if keys.is_object() => {
                        Some((keys.clone(), root.to_string()))
                    }
                    _ => None,
                };
                state
                    .markets
                    .insert(location.clone(), MarketState { accepted, keys });
            }
        }
        if let Some(yanked) = value.get("yanked").and_then(Value::as_object) {
            for (id, entry) in yanked {
                if let (Some(version), Some(reason)) = (
                    entry.get("version").and_then(Value::as_str),
                    entry.get("reason").and_then(Value::as_str),
                ) {
                    state
                        .yanked
                        .insert(id.clone(), (version.to_string(), reason.to_string()));
                }
            }
        }
        Ok(state)
    }

    fn to_json(&self) -> String {
        let mut markets = serde_json::Map::new();
        for (location, market) in &self.markets {
            let mut entry = serde_json::Map::new();
            if let Some(accepted) = &market.accepted {
                entry.insert("serial".into(), json!(accepted.serial));
                entry.insert("sha256".into(), json!(accepted.sha256));
            }
            if let Some((keys, root)) = &market.keys {
                entry.insert("keys".into(), keys.clone());
                entry.insert("root".into(), json!(root));
            }
            markets.insert(location.clone(), Value::Object(entry));
        }
        let yanked: serde_json::Map<String, Value> = self
            .yanked
            .iter()
            .map(|(id, (version, reason))| {
                (id.clone(), json!({"version": version, "reason": reason}))
            })
            .collect();
        let value = json!({
            "schema_version": STATE_SCHEMA_VERSION,
            "disabled": self.disabled,
            "marketplaces": markets,
            "yanked": yanked,
        });
        serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
    }

    pub(crate) fn save(&self, dir: &Path) -> Outcome<()> {
        write_atomically(&State::path(dir), self.to_json().as_bytes())
            .map_err(|e| input(format!("{}: {e}", State::path(dir).display())))
    }

    pub(crate) fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.iter().any(|d| d == id)
    }

    fn set_enabled(&mut self, id: &str, enabled: bool) {
        self.disabled.retain(|d| d != id);
        if !enabled {
            self.disabled.push(id.to_string());
            self.disabled.sort();
        }
    }

    /// Remember whether the installed `version` of `id` is yanked.
    fn note_yank(&mut self, id: &str, version: &str, listed: Option<&ListedVersion>) {
        match listed.and_then(|v| v.yanked.as_ref()) {
            Some(reason) => {
                self.yanked
                    .insert(id.to_string(), (version.to_string(), reason.clone()));
            }
            None => {
                self.yanked.remove(id);
            }
        }
    }

    fn yank_reason(&self, entry: &LockEntry) -> Option<&str> {
        self.yanked
            .get(&entry.id)
            .filter(|(version, _)| *version == entry.version)
            .map(|(_, reason)| reason.as_str())
    }
}

/// Write `bytes` to `path` through a temporary file beside it.
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = dir.join(format!(".{name}.{}.partial", std::process::id()));
    let result = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

// ---------------------------------------------------------------------------
// The marketplace: where it is, its keys, its index and its listings.
// ---------------------------------------------------------------------------

/// Where the marketplace is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Location {
    /// A directory laid out as the marketplace repository is.
    Local(PathBuf),
    /// An `https://` base URL, ending in `/`.
    Remote(String),
}

impl Location {
    fn parse(text: &str) -> Outcome<Location> {
        if let Some(rest) = text.strip_prefix("file://") {
            // `file:///abs/path`, or `file://localhost/abs/path`.
            let path = rest.strip_prefix("localhost").unwrap_or(rest);
            if path.is_empty() {
                return Err(usage("--marketplace file:// needs a path"));
            }
            return Ok(Location::Local(PathBuf::from(percent_decode(path))));
        }
        if text.starts_with("https://") {
            let mut base = text.to_string();
            if !base.ends_with('/') {
                base.push('/');
            }
            return Ok(Location::Remote(base));
        }
        if text.contains("://") {
            return Err(usage(format!(
                "marketplace {:?}: only https:// URLs, file:// URLs and directories are allowed",
                controls::shown(text)
            )));
        }
        Ok(Location::Local(fs_path(text)))
    }

    /// The key it is remembered under.
    fn key(&self) -> String {
        match self {
            Location::Local(path) => {
                let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                format!("file://{}", path.display())
            }
            Location::Remote(url) => url.clone(),
        }
    }

    fn shown(&self) -> String {
        match self {
            Location::Local(path) => path.display().to_string(),
            Location::Remote(url) => url.clone(),
        }
    }

    /// Read `relative` (`index.json`, `packages/ID.json`, …), at most
    /// `max` bytes. `None` when it is not there.
    fn fetch(&self, relative: &str, max: u64) -> Outcome<Option<Vec<u8>>> {
        match self {
            Location::Local(root) => {
                let path = relative
                    .split('/')
                    .fold(root.clone(), |path, part| path.join(part));
                read_bounded(&path, max).map_err(fetch_failure)
            }
            Location::Remote(base) => {
                #[cfg(feature = "fetch")]
                {
                    let url = format!("{base}{relative}");
                    match rich_marketplace::HttpsTransport::new().get(&url, max) {
                        Ok(bytes) => Ok(Some(bytes)),
                        Err(FetchError::Status { status: 404, .. }) => Ok(None),
                        Err(error) => Err(fetch_failure(error)),
                    }
                }
                #[cfg(not(feature = "fetch"))]
                {
                    let _ = (base, relative, max);
                    Err(usage(
                        "this build has no network access (the fetch feature); pass \
                         --marketplace DIR for a local marketplace",
                    ))
                }
            }
        }
    }

    /// Where artifacts come from: the network for a remote marketplace, the
    /// `artifacts/` mirror for a local one.
    fn transport(&self) -> Outcome<ArtifactTransport> {
        match self {
            Location::Local(root) => Ok(ArtifactTransport::Mirror(root.join("artifacts"))),
            #[cfg(feature = "fetch")]
            Location::Remote(_) => Ok(ArtifactTransport::Https(
                rich_marketplace::HttpsTransport::new(),
            )),
            #[cfg(not(feature = "fetch"))]
            Location::Remote(_) => Err(usage(
                "this build has no network access (the fetch feature); pass --marketplace DIR \
                 for a local marketplace",
            )),
        }
    }
}

/// `%XX` escapes in a `file://` URL's path.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read a local file of at most `max` bytes; `None` when it is missing.
fn read_bounded(path: &Path, max: u64) -> Result<Option<Vec<u8>>, FetchError> {
    use std::io::Read;
    let url = path.display().to_string();
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(FetchError::Transport {
                url,
                message: e.to_string(),
            })
        }
    };
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| FetchError::Transport {
            url: url.clone(),
            message: e.to_string(),
        })?;
    if bytes.len() as u64 > max {
        return Err(FetchError::TooLarge { url, limit: max });
    }
    Ok(Some(bytes))
}

/// Downloads artifacts.
pub(crate) enum ArtifactTransport {
    /// `https://HOST/PATH` read from `DIR/HOST/PATH`: a local marketplace's
    /// mirror. Nothing outside it, and no network.
    Mirror(PathBuf),
    #[cfg(feature = "fetch")]
    Https(rich_marketplace::HttpsTransport),
}

impl Transport for ArtifactTransport {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, FetchError> {
        rich_marketplace::require_https(url)?;
        match self {
            ArtifactTransport::Mirror(root) => {
                let not_found = || FetchError::NotFound {
                    url: url.to_string(),
                };
                let rest = &url["https://".len()..];
                let rest = rest.split(['?', '#']).next().unwrap_or(rest);
                let mut path = root.clone();
                for part in rest.split('/') {
                    if part.is_empty()
                        || part == "."
                        || part == ".."
                        || part.contains(['\\', ':', '\0'])
                    {
                        return Err(not_found());
                    }
                    path.push(part);
                }
                read_bounded(&path, max_bytes)
                    .map_err(|error| match error {
                        FetchError::TooLarge { limit, .. } => FetchError::TooLarge {
                            url: url.to_string(),
                            limit,
                        },
                        other => other,
                    })?
                    .ok_or_else(not_found)
            }
            #[cfg(feature = "fetch")]
            ArtifactTransport::Https(transport) => transport.get(url, max_bytes),
        }
    }
}

/// The marketplace named by `options`.
fn location(options: &Options) -> Outcome<Location> {
    let text = options
        .marketplace
        .clone()
        .or_else(|| {
            std::env::var("RICH_MARKETPLACE")
                .ok()
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_MARKETPLACE.to_string());
    Location::parse(&text)
}

/// The trust root: `--marketplace-keys`, `RICH_MARKETPLACE_KEYS`, or the
/// compiled-in keys.
fn trust_root(options: &Options) -> Outcome<KeySet> {
    let file = options.keys.clone().or_else(|| {
        std::env::var_os("RICH_MARKETPLACE_KEYS")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    });
    match file {
        Some(path) => {
            let bytes = std::fs::read(&path)
                .map_err(|e| input(format!("marketplace keys {}: {e}", path.display())))?;
            KeySet::from_json(&bytes)
                .map_err(|e| data(format!("marketplace keys {}: {e}", path.display())))
        }
        None => KeySet::official().map_err(keys_failure),
    }
}

/// An opened marketplace: its location and its verified index.
pub(crate) struct Market {
    pub location: Location,
    pub index: VerifiedIndex,
}

/// Open the marketplace: walk its key chain from the trust root (or the
/// newest generation already verified), then verify `index.json` against
/// the last index accepted from it, and remember both.
pub(crate) fn open(options: &Options, dir: &Path, state: &mut State) -> Outcome<Market> {
    let location = location(options)?;
    let key = location.key();
    let root = trust_root(options)?;
    let root_id = root.main().id().to_string();
    let market = state.markets.get(&key).cloned().unwrap_or_default();
    // The newest generation verified from this same root, if any.
    let mut keys = match &market.keys {
        Some((value, from)) if *from == root_id => serde_json::to_vec(value)
            .ok()
            .and_then(|bytes| KeySet::from_json(&bytes).ok())
            .filter(|cached| cached.generation() >= root.generation())
            .unwrap_or_else(|| root.clone()),
        _ => root.clone(),
    };
    let mut notices = Vec::new();
    for _ in 0..MAX_ROTATIONS {
        let next = keys.generation() + 1;
        let name = format!("keys/keys-{next}.json");
        let Some(keys_json) = location.fetch(&name, MAX_KEYS_BYTES)? else {
            break;
        };
        let backup = location
            .fetch(&format!("{name}.backup.minisig"), MAX_SIGNATURE_BYTES)?
            .ok_or_else(|| data(format!("{name} has no backup signature")))?;
        let main = location.fetch(&format!("{name}.main.minisig"), MAX_SIGNATURE_BYTES)?;
        let backup = String::from_utf8_lossy(&backup).into_owned();
        let main = main.map(|m| String::from_utf8_lossy(&m).into_owned());
        let (set, kind) = keys
            .rotate(Rotation {
                keys_json: &keys_json,
                main_signature: main.as_deref(),
                backup_signature: &backup,
            })
            .map_err(keys_failure)?;
        if let RotationKind::MainKeyReplaced { revoked } = kind {
            notices.push(format!(
                "the marketplace's main key {revoked} was revoked and replaced (keys generation \
                 {next})"
            ));
        }
        keys = set;
    }

    let index = location
        .fetch("index.json", MAX_INDEX_BYTES)?
        .ok_or_else(|| {
            input(format!(
                "no index.json in the marketplace at {}",
                location.shown()
            ))
        })?;
    let signature = location
        .fetch("index.json.minisig", MAX_SIGNATURE_BYTES)?
        .ok_or_else(|| {
            data(format!(
                "the marketplace at {} has no index.json.minisig",
                location.shown()
            ))
        })?;
    let signature = String::from_utf8_lossy(&signature);
    let verified = verify_index(
        &keys,
        &index,
        &signature,
        market.accepted.as_ref(),
        SystemTime::now(),
    )
    .map_err(index_failure)?;
    let keys_value: Value = serde_json::from_str(&keys.to_json()).unwrap_or(Value::Null);
    state.markets.insert(
        key,
        MarketState {
            accepted: Some(verified.accepted()),
            keys: Some((keys_value, root_id)),
        },
    );
    // Without this, a rollback could not be noticed next time; a directory
    // that cannot be written is still searched.
    if let Err(failure) = state.save(dir) {
        eprintln!("rich: warning: {}", failure.message);
    }
    for notice in &notices {
        eprintln!("rich: warning: {notice}");
    }
    Ok(Market {
        location,
        index: verified,
    })
}

impl Market {
    /// Read and check listing `id`.
    pub(crate) fn listing(&self, id: &str) -> Outcome<Listing> {
        let entry = self.index.entry(id).ok_or_else(|| {
            usage(format!(
                "no package {:?} in the marketplace at {}",
                controls::shown(id),
                self.location.shown()
            ))
        })?;
        let bytes = self
            .location
            .fetch(&entry.path, MAX_LISTING_BYTES)?
            .ok_or_else(|| data(format!("{} is missing from the marketplace", entry.path)))?;
        self.index.read_listing(id, &bytes).map_err(listing_failure)
    }

    /// Every listing, by id.
    pub(crate) fn listings(&self) -> Outcome<Vec<Listing>> {
        self.index
            .listings()
            .keys()
            .map(|id| self.listing(id))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Checking a WASM module in the sandbox.
// ---------------------------------------------------------------------------

/// The CLI's [`ArtifactChecker`]: with `wasm-plugins`, a module is loaded in
/// the sandbox (`load_wasm_bytes`, the loader `--plugin` uses) and its
/// manifest compared with the listing; the other kinds use the built-in
/// checks.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SandboxChecker;

impl ArtifactChecker for SandboxChecker {
    fn check_wasm(&self, bytes: &[u8], expected: &WasmExpectation<'_>) -> Result<(), CheckError> {
        rich_marketplace::check_wasm_header(bytes)?;
        #[cfg(feature = "wasm-plugins")]
        {
            let label = PathBuf::from(format!("{}-{}.wasm", expected.id, expected.version));
            let plugin = rich_ext::plugin_loading::load_wasm_bytes(
                bytes,
                &label,
                &rich_ext::plugin_loading::WasmLimits::default(),
            )
            .map_err(|e| CheckError::Host(format!("the module does not load: {e}")))?;
            rich_marketplace::compare_wasm_manifest(plugin.abi(), expected)
        }
        #[cfg(not(feature = "wasm-plugins"))]
        {
            let _ = expected;
            Err(CheckError::Host(
                "this build cannot load WASM plugins; it needs the wasm-plugins feature".into(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Loading what is installed, on every run.
// ---------------------------------------------------------------------------

/// Whether anything may be installed: the lockfile exists. One stat, so a
/// run with nothing installed does no more.
pub(crate) fn has_installed() -> bool {
    plugin_dir(None).is_some_and(|dir| dir.join(LOCKFILE_NAME).is_file())
}

/// The installed, enabled WASM plugins, each read once and checked against
/// the lockfile, then loaded from the bytes checked. `skip` names plugins
/// already loaded (`--plugin PATH` wins over an installed copy). Nothing
/// installed costs one missing-file check. A plugin that fails is not
/// loaded and is named in a warning: a changed file is refused.
pub(crate) fn installed_plugins(skip: &[String]) -> (Vec<RuntimePlugin>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut plugins = Vec::new();
    let Some(dir) = plugin_dir(None) else {
        return (plugins, warnings);
    };
    let lock_path = dir.join(LOCKFILE_NAME);
    if !lock_path.is_file() {
        return (plugins, warnings);
    }
    let lockfile = match Lockfile::load(&lock_path) {
        Ok(lockfile) => lockfile,
        Err(error) => {
            warnings.push(format!("installed plugins are not loaded: {error}"));
            return (plugins, warnings);
        }
    };
    let state = match State::load(&dir) {
        Ok(state) => state,
        Err(failure) => {
            warnings.push(format!(
                "installed plugins are not loaded: {}",
                failure.message
            ));
            return (plugins, warnings);
        }
    };
    let wanted: Vec<&LockEntry> = lockfile
        .entries()
        .filter(|e| e.kind == Kind::WasmPlugin && state.is_enabled(&e.id))
        .filter(|e| !skip.contains(&e.id))
        .collect();
    if wanted.is_empty() {
        return (plugins, warnings);
    }
    if !RuntimeKind::Wasm.supported() {
        let ids: Vec<&str> = wanted.iter().map(|e| e.id.as_str()).collect();
        warnings.push(format!(
            "installed WASM plugins ({}) are not loaded: this build lacks the wasm-plugins \
             feature",
            ids.join(", ")
        ));
        return (plugins, warnings);
    }
    for entry in wanted {
        match load_entry(&dir, entry) {
            Ok(plugin) => plugins.push(plugin),
            Err(message) => warnings.push(format!(
                "plugin {} is not loaded: {message}; `rich plugin verify` checks every installed \
                 file",
                entry.id
            )),
        }
    }
    (plugins, warnings)
}

#[cfg(feature = "wasm-plugins")]
fn load_entry(dir: &Path, entry: &LockEntry) -> Result<RuntimePlugin, String> {
    let bytes = entry.verify(dir).map_err(|e| e.to_string())?;
    let plugin = rich_ext::plugin_loading::load_wasm_bytes(
        &bytes,
        &entry.file(dir),
        &rich_ext::plugin_loading::WasmLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    if plugin.abi().name != entry.id {
        return Err(format!(
            "its manifest names {:?}, but it is installed as {}",
            plugin.abi().name,
            entry.id
        ));
    }
    Ok(plugin)
}

#[cfg(not(feature = "wasm-plugins"))]
fn load_entry(_: &Path, _: &LockEntry) -> Result<RuntimePlugin, String> {
    Err("this build lacks the wasm-plugins feature".into())
}

// ---------------------------------------------------------------------------
// Showing packages.
// ---------------------------------------------------------------------------

/// One line of listing text, safe to print.
pub(crate) fn line(text: &str) -> String {
    rich_ext::sanitize::sanitize_single_line(text)
}

fn escape(text: &str) -> String {
    rich::markup::escape(&line(text))
}

pub(crate) fn tier_meaning(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "rs-rich's own, built from its repository",
        Tier::Reviewed => "a maintainer read the source and rebuilt the same artifact",
        _ => "checked by the marketplace's CI only, not reviewed",
    }
}

pub(crate) fn requires_text(requires: Requires) -> String {
    match requires {
        Requires::Abi { major, minor } => format!("plugin ABI {major}.{minor}"),
        Requires::PluginApi(n) => format!("plugin API {n}"),
        Requires::MicroSchema(n) => format!("micro pack schema {n}"),
        Requires::ThemeFormat(n) => format!("theme format {n}"),
        _ => "unknown".into(),
    }
}

fn requires_json(requires: Requires) -> Value {
    match requires {
        Requires::Abi { major, minor } => json!({"abi": format!("{major}.{minor}")}),
        Requires::PluginApi(n) => json!({"plugin_api": n}),
        Requires::MicroSchema(n) => json!({"micro_schema": n}),
        Requires::ThemeFormat(n) => json!({"theme_format": n}),
        _ => Value::Null,
    }
}

pub(crate) fn capabilities_text(version: &ListedVersion) -> String {
    version
        .capabilities
        .iter()
        .map(|c| format!("{} {}", c.kind, c.name))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn permissions_text(kind: Kind, version: &ListedVersion) -> String {
    if version.permissions.is_empty() {
        return match kind {
            Kind::WasmPlugin => {
                "none (sandboxed: no files, network, clock or processes)".to_string()
            }
            Kind::Crate => "none declared".to_string(),
            _ => "none (data only)".to_string(),
        };
    }
    let mut text = version.permissions.join(", ");
    if kind == Kind::Crate {
        text.push_str(" (declared, not enforced: native code runs with your rights)");
    }
    text
}

fn version_json(listing: &Listing, version: &ListedVersion, host: &Host) -> Value {
    json!({
        "version": version.version.to_string(),
        "published": version.published,
        "requires": requires_json(version.requires),
        "compatible": host.check(listing.kind, version).is_ok(),
        "artifact": {
            "url": version.artifact.url,
            "sha256": version.artifact.sha256,
            "size": version.artifact.size,
        },
        "source_revision": version.source_revision,
        "capabilities": version.capabilities.iter()
            .map(|c| json!({"kind": c.kind, "name": c.name})).collect::<Vec<_>>(),
        "permissions": version.permissions,
        "yanked": version.yanked.is_some(),
        "yank_reason": version.yanked,
    })
}

fn listing_json(listing: &Listing, host: &Host, installed: Option<&LockEntry>) -> Value {
    let candidate = resolve(listing, host, &Request::Newest).ok();
    json!({
        "id": listing.id,
        "kind": listing.kind.as_str(),
        "name": listing.name,
        "description": listing.description,
        "tier": listing.tier.as_str(),
        "license": listing.license,
        "authors": listing.authors,
        "repository": listing.repository,
        "homepage": listing.homepage,
        "keywords": listing.keywords,
        "crate": listing.crate_info.as_ref().map(|c| json!({"name": c.name, "features": c.features})),
        "installable": listing.kind.is_installable(),
        "candidate": candidate.map(|v| v.version.to_string()),
        "installed": installed.map(|e| e.version.clone()),
        "versions": listing.versions.iter().map(|v| version_json(listing, v, host)).collect::<Vec<_>>(),
    })
}

/// `cargo add` for a crate listing.
pub(crate) fn cargo_add(listing: &Listing, version: &ListedVersion) -> String {
    let name = listing
        .crate_info
        .as_ref()
        .map_or(listing.id.as_str(), |c| c.name.as_str());
    let mut command = format!("cargo add {name}@={}", version.version);
    if let Some(features) = listing.crate_info.as_ref().map(|c| &c.features) {
        if !features.is_empty() {
            command.push_str(&format!(" --features {}", features.join(",")));
        }
    }
    command
}

fn crate_refusal(listing: &Listing) -> Failure {
    let version = resolve(listing, &Host::current(), &Request::Newest)
        .ok()
        .or_else(|| listing.versions.first());
    let name = listing
        .crate_info
        .as_ref()
        .map_or(listing.id.as_str(), |c| c.name.as_str());
    match version {
        Some(version) => usage(format!(
            "{} is a Cargo crate ({name} {}); add it to your program:\n    {}",
            listing.id,
            version.version,
            cargo_add(listing, version)
        )),
        None => usage(format!(
            "{} is a Cargo crate ({name}), which is listed only and never installed",
            listing.id
        )),
    }
}

/// A key/value grid.
fn grid(rows: &[(&str, String)], no_color: bool) -> String {
    let mut table = rich::Table::grid().padding(0, 2, 0, 0);
    table.add_column("");
    table.add_column_with(
        Text::default(),
        rich::ColumnOptions {
            overflow: rich::Overflow::Fold,
            ..rich::ColumnOptions::default()
        },
    );
    for (key, value) in rows {
        let value: Vec<String> = value.lines().map(escape).collect();
        table.add_row(&[&format!("[bold]{key}[/]"), &value.join("\n")]);
    }
    cli_spec::render(&table, no_color) + "\n"
}

fn table(columns: &[&str], rows: &[Vec<String>], no_color: bool) -> String {
    let mut table = rich::Table::new();
    for column in columns {
        table.add_column(*column);
    }
    for row in rows {
        let cells: Vec<String> = row.iter().map(|c| escape(c)).collect();
        let cells: Vec<&str> = cells.iter().map(String::as_str).collect();
        table.add_row(&cells);
    }
    cli_spec::render(&table, no_color) + "\n"
}

/// What an install shows before it asks.
pub(crate) fn provenance(
    listing: &Listing,
    version: &ListedVersion,
) -> Vec<(&'static str, String)> {
    let mut rows = vec![
        (
            "Package",
            format!("{} {} ({})", listing.id, version.version, listing.kind),
        ),
        ("Name", listing.name.clone()),
        (
            "Tier",
            format!("{}: {}", listing.tier, tier_meaning(listing.tier)),
        ),
        ("Source", listing.repository.clone()),
    ];
    if let Some(revision) = &version.source_revision {
        rows.push(("Revision", revision.clone()));
    }
    rows.push(("Artifact", version.artifact.url.clone()));
    rows.push(("SHA-256", version.artifact.sha256.clone()));
    rows.push(("Size", format!("{} bytes", version.artifact.size)));
    rows.push(("Requires", requires_text(version.requires)));
    rows.push(("Capabilities", capabilities_text(version)));
    rows.push(("Permissions", permissions_text(listing.kind, version)));
    rows.push(("License", listing.license.clone()));
    if let Some(reason) = &version.yanked {
        rows.push(("Yanked", reason.clone()));
    }
    rows
}

/// Ask `question` on the terminal; `--yes` answers yes. Without a terminal
/// and without `--yes`, nothing is asked and nothing is done.
pub(crate) fn confirm(question: &str, options: &Options) -> Outcome<bool> {
    if options.yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err(input(format!(
            "{question} needs an answer, and there is no terminal to ask on; pass --yes"
        )));
    }
    eprint!("{question} [y/N] ");
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| input(format!("cannot read the answer: {e}")))?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

// ---------------------------------------------------------------------------
// The commands.
// ---------------------------------------------------------------------------

fn lockfile(dir: &Path) -> Outcome<Lockfile> {
    Lockfile::load(&dir.join(LOCKFILE_NAME)).map_err(lockfile_failure)
}

/// `search [QUERY]`: every listing whose id, name, description or keywords
/// contain QUERY (any case).
fn search(options: &Options) -> Outcome<Done> {
    let dir = require_dir(options)?;
    let mut state = State::load(&dir)?;
    let market = open(options, &dir, &mut state)?;
    let lock = lockfile(&dir)?;
    let query = options.positionals.join(" ").to_lowercase();
    let host = Host::current();
    let mut rows = Vec::new();
    let mut found = Vec::new();
    for listing in market.listings()? {
        let haystack = [
            listing.id.as_str(),
            listing.name.as_str(),
            listing.description.as_str(),
        ]
        .into_iter()
        .chain(listing.keywords.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
        if !query.is_empty() && !haystack.contains(&query) {
            continue;
        }
        let installed = lock.get(&listing.id);
        let candidate = resolve(&listing, &host, &Request::Newest)
            .map(|v| v.version.to_string())
            .unwrap_or_else(|_| "none for this host".into());
        rows.push(vec![
            listing.id.clone(),
            listing.kind.to_string(),
            listing.tier.to_string(),
            candidate,
            installed.map_or_else(String::new, |e| e.version.clone()),
            listing.description.clone(),
        ]);
        found.push(listing_json(&listing, &host, installed));
    }
    let text = if rows.is_empty() {
        "No packages match.\n".to_string()
    } else {
        table(
            &["Id", "Kind", "Tier", "Version", "Installed", "Description"],
            &rows,
            options.no_color,
        )
    };
    Ok(Done::new(
        text,
        json!({
            "marketplace": market.location.shown(),
            "index_serial": market.index.serial(),
            "packages": found,
        }),
    ))
}

fn one_id(options: &Options, what: &str) -> Outcome<String> {
    match options.positionals.as_slice() {
        [id] => Ok(id.clone()),
        [] => Err(usage(format!("rich plugin {what} needs a package ID"))),
        _ => Err(usage(format!("rich plugin {what} takes one package ID"))),
    }
}

/// `info ID`: a listing, its versions and what this host would install.
fn info(options: &Options) -> Outcome<Done> {
    let id = one_id(options, "info")?;
    let dir = require_dir(options)?;
    let mut state = State::load(&dir)?;
    let market = open(options, &dir, &mut state)?;
    let listing = market.listing(&id)?;
    let lock = lockfile(&dir)?;
    let installed = lock.get(&id);
    if let Some(entry) = installed {
        let listed = listing
            .versions
            .iter()
            .find(|v| v.version.to_string() == entry.version);
        state.note_yank(&id, &entry.version, listed);
        let _ = state.save(&dir);
    }
    let host = Host::current();
    let candidate = resolve(&listing, &host, &Request::Newest);
    let mut rows = vec![
        ("Name", listing.name.clone()),
        ("Kind", listing.kind.to_string()),
        (
            "Tier",
            format!("{}: {}", listing.tier, tier_meaning(listing.tier)),
        ),
        ("Description", listing.description.clone()),
        ("License", listing.license.clone()),
        ("Repository", listing.repository.clone()),
    ];
    if let Some(homepage) = &listing.homepage {
        rows.push(("Homepage", homepage.clone()));
    }
    if !listing.authors.is_empty() {
        rows.push(("Authors", listing.authors.join(", ")));
    }
    match &candidate {
        Ok(version) => {
            rows.push(("Newest", format!("{} (suits this host)", version.version)));
            rows.push(("Capabilities", capabilities_text(version)));
            rows.push(("Permissions", permissions_text(listing.kind, version)));
            if listing.kind == Kind::Crate {
                rows.push(("Add with", cargo_add(&listing, version)));
            }
        }
        Err(error) => rows.push(("Newest", error.to_string())),
    }
    if let Some(entry) = installed {
        rows.push(("Installed", entry.version.clone()));
    }
    let mut text = format!("{}\n\n", line(&listing.id));
    text.push_str(&grid(&rows, options.no_color));
    let versions: Vec<Vec<String>> = listing
        .versions
        .iter()
        .map(|v| {
            vec![
                v.version.to_string(),
                v.published.clone(),
                requires_text(v.requires),
                match host.check(listing.kind, v) {
                    Ok(()) => "yes".into(),
                    Err(reason) => reason.to_string(),
                },
                v.yanked.clone().unwrap_or_default(),
            ]
        })
        .collect();
    text.push('\n');
    text.push_str(&table(
        &["Version", "Published", "Requires", "Loads here", "Yanked"],
        &versions,
        options.no_color,
    ));
    Ok(Done::new(text, listing_json(&listing, &host, installed)))
}

/// `ID` or `ID@VERSION`.
fn requested_version(arg: &str) -> Outcome<(String, Request)> {
    match arg.split_once('@') {
        None => Ok((arg.to_string(), Request::Newest)),
        Some((id, version)) => {
            let version = rich_marketplace::semver::Version::parse(version).map_err(|e| {
                usage(format!(
                    "{:?}: not a version: {e}",
                    controls::shown(version)
                ))
            })?;
            Ok((id.to_string(), Request::Exact(version)))
        }
    }
}

fn entry_json(entry: &LockEntry) -> Value {
    json!({
        "id": entry.id,
        "kind": entry.kind.as_str(),
        "version": entry.version,
        "tier": entry.tier.as_str(),
        "url": entry.url,
        "sha256": entry.sha256,
        "size": entry.size,
        "index_serial": entry.index_serial,
        "path": entry.path,
    })
}

/// Show what is about to be installed, ask, and install it. `None` when the
/// user declined.
fn install_version(
    options: &Options,
    dir: &Path,
    market: &Market,
    listing: &Listing,
    version: &ListedVersion,
    verb: &str,
) -> Outcome<Option<LockEntry>> {
    if listing.kind == Kind::WasmPlugin && !RuntimeKind::Wasm.supported() {
        return Err(usage(format!(
            "{} is a WASM plugin, and this build cannot load WASM plugins; install rs-rich-cli \
             with --features wasm-plugins",
            listing.id
        )));
    }
    if !options.json || !options.yes {
        eprint!("{}", grid(&provenance(listing, version), options.no_color));
    }
    let question = format!(
        "{verb} {} {} into {}?",
        listing.id,
        version.version,
        dir.display()
    );
    if !confirm(&question, options)? {
        return Ok(None);
    }
    let installer = Installer::new(dir, market.location.transport()?).with_checker(SandboxChecker);
    let entry = installer
        .install(&market.index, listing, version, SystemTime::now())
        .map_err(install_failure)?;
    Ok(Some(entry))
}

/// `install ID[@VERSION]...`.
fn install(options: &Options) -> Outcome<Done> {
    if options.positionals.is_empty() {
        return Err(usage("rich plugin install needs a package ID"));
    }
    let requests = options
        .positionals
        .iter()
        .map(|arg| requested_version(arg))
        .collect::<Outcome<Vec<_>>>()?;
    let dir = require_dir(options)?;
    let mut state = State::load(&dir)?;
    let market = open(options, &dir, &mut state)?;
    let host = Host::current();
    let mut text = String::new();
    let mut installed = Vec::new();
    for (id, request) in requests {
        let listing = market.listing(&id)?;
        if !listing.kind.is_installable() {
            return Err(crate_refusal(&listing));
        }
        let version = resolve(&listing, &host, &request).map_err(resolve_failure)?;
        let current = lockfile(&dir)?;
        if let Some(entry) = current.get(&id) {
            if entry.version == version.version.to_string()
                && entry.sha256 == version.artifact.sha256
                && entry.verify(&dir).is_ok()
            {
                text.push_str(&format!("{id} {} is already installed\n", entry.version));
                installed.push(entry_json(entry));
                continue;
            }
        }
        match install_version(options, &dir, &market, &listing, version, "Install")? {
            Some(entry) => {
                state = State::load(&dir)?;
                state.note_yank(&id, &entry.version, None);
                state.save(&dir)?;
                text.push_str(&format!(
                    "Installed {} {} ({}, {})\n",
                    entry.id, entry.version, entry.kind, entry.tier
                ));
                installed.push(entry_json(&entry));
            }
            None => {
                let mut done = Done::new(text, json!({"installed": installed}));
                done.declined = true;
                return Ok(done);
            }
        }
    }
    Ok(Done::new(text, json!({"installed": installed})))
}

/// `update [ID...]`: each installed package (or those named) to its newest
/// version for this host, asking first.
fn update(options: &Options) -> Outcome<Done> {
    let dir = require_dir(options)?;
    let lock = lockfile(&dir)?;
    let ids: Vec<String> = if options.positionals.is_empty() {
        lock.entries().map(|e| e.id.clone()).collect()
    } else {
        for id in &options.positionals {
            if lock.get(id).is_none() {
                return Err(usage(format!(
                    "{} is not installed; `rich plugin install {}` installs it",
                    controls::shown(id),
                    controls::shown(id)
                )));
            }
        }
        options.positionals.clone()
    };
    let mut state = State::load(&dir)?;
    let market = open(options, &dir, &mut state)?;
    let host = Host::current();
    let mut text = String::new();
    let (mut updated, mut current, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    for id in ids {
        let Some(entry) = lock.get(&id) else { continue };
        let listing = match market.listing(&id) {
            Ok(listing) => listing,
            Err(failure) if failure.class == ExitClass::Usage => {
                text.push_str(&format!(
                    "{id}: no longer listed; kept at {}\n",
                    entry.version
                ));
                skipped.push(json!({"id": id, "reason": "not listed"}));
                continue;
            }
            Err(failure) => return Err(failure),
        };
        let listed = listing
            .versions
            .iter()
            .find(|v| v.version.to_string() == entry.version);
        let yanked = listed.and_then(|v| v.yanked.clone());
        state.note_yank(&id, &entry.version, listed);
        state.save(&dir)?;
        let candidate = match resolve(&listing, &host, &Request::Newest) {
            Ok(candidate) => candidate,
            Err(error) => {
                text.push_str(&format!("{id}: {error}; kept at {}\n", entry.version));
                skipped.push(json!({"id": id, "reason": error.to_string()}));
                continue;
            }
        };
        let installed = rich_marketplace::semver::Version::parse(&entry.version).ok();
        let newer = installed.as_ref().is_none_or(|v| candidate.version > *v);
        let replace_yanked = yanked.is_some() && candidate.version.to_string() != entry.version;
        if !newer && !replace_yanked {
            text.push_str(&format!("{id} {} is up to date\n", entry.version));
            current.push(json!({"id": id, "version": entry.version}));
            continue;
        }
        match install_version(options, &dir, &market, &listing, candidate, "Update to")? {
            Some(new) => {
                state = State::load(&dir)?;
                state.note_yank(&id, &new.version, None);
                state.save(&dir)?;
                text.push_str(&format!(
                    "Updated {id} {} -> {}\n",
                    entry.version, new.version
                ));
                updated.push(json!({"id": id, "from": entry.version, "to": new.version}));
            }
            None => {
                let mut done = Done::new(
                    text,
                    json!({"updated": updated, "current": current, "skipped": skipped}),
                );
                done.declined = true;
                return Ok(done);
            }
        }
    }
    if text.is_empty() {
        text.push_str("Nothing is installed.\n");
    }
    Ok(Done::new(
        text,
        json!({"updated": updated, "current": current, "skipped": skipped}),
    ))
}

/// `remove ID...`.
fn remove(options: &Options) -> Outcome<Done> {
    if options.positionals.is_empty() {
        return Err(usage("rich plugin remove needs a package ID"));
    }
    let dir = require_dir(options)?;
    let installer = Installer::new(&dir, rich_marketplace::MemoryTransport::new());
    let mut state = State::load(&dir)?;
    let mut text = String::new();
    let mut removed = Vec::new();
    for id in &options.positionals {
        let entry = installer.remove(id).map_err(lockfile_failure)?;
        state.set_enabled(id, true);
        state.yanked.remove(id);
        text.push_str(&format!("Removed {} {}\n", entry.id, entry.version));
        removed.push(entry_json(&entry));
    }
    state.save(&dir)?;
    Ok(Done::new(text, json!({"removed": removed})))
}

/// The state of one installed file, with no network.
fn file_status(dir: &Path, entry: &LockEntry) -> (&'static str, Option<String>) {
    match entry.verify(dir) {
        Ok(_) => ("ok", None),
        Err(LockfileError::Missing { .. }) => ("missing", Some("the file is missing".into())),
        Err(LockfileError::Mismatch { .. }) => (
            "changed",
            Some("the file changed on disk; it is not loaded".into()),
        ),
        Err(other) => ("error", Some(other.to_string())),
    }
}

/// `list`: what is installed, with no network.
fn list(options: &Options) -> Outcome<Done> {
    let dir = require_dir(options)?;
    let lock = lockfile(&dir)?;
    let state = State::load(&dir)?;
    let mut rows = Vec::new();
    let mut packages = Vec::new();
    for entry in lock.entries() {
        let enabled = state.is_enabled(&entry.id);
        let (status, problem) = file_status(&dir, entry);
        let yanked = state.yank_reason(entry);
        let mut note = problem.clone().unwrap_or_default();
        if let Some(reason) = yanked {
            if !note.is_empty() {
                note.push_str("; ");
            }
            note.push_str(&format!("yanked: {reason}"));
        }
        rows.push(vec![
            entry.id.clone(),
            entry.version.clone(),
            entry.kind.to_string(),
            entry.tier.to_string(),
            if enabled { "yes" } else { "no" }.into(),
            status.into(),
            note,
        ]);
        let mut value = entry_json(entry);
        value["enabled"] = json!(enabled);
        value["status"] = json!(status);
        value["yank_reason"] = json!(yanked);
        packages.push(value);
    }
    let text = if rows.is_empty() {
        format!("Nothing is installed in {}.\n", dir.display())
    } else {
        table(
            &["Id", "Version", "Kind", "Tier", "Enabled", "File", "Notes"],
            &rows,
            options.no_color,
        )
    };
    Ok(Done::new(
        text,
        json!({"plugin_dir": dir.display().to_string(), "packages": packages}),
    ))
}

/// `enable ID...` and `disable ID...`.
fn set_enabled(options: &Options, enabled: bool) -> Outcome<Done> {
    let verb = if enabled { "enable" } else { "disable" };
    if options.positionals.is_empty() {
        return Err(usage(format!("rich plugin {verb} needs a package ID")));
    }
    let dir = require_dir(options)?;
    let lock = lockfile(&dir)?;
    let mut state = State::load(&dir)?;
    let mut text = String::new();
    let mut changed = Vec::new();
    for id in &options.positionals {
        if lock.get(id).is_none() {
            return Err(usage(format!("{} is not installed", controls::shown(id))));
        }
        state.set_enabled(id, enabled);
        text.push_str(&format!(
            "{} {id}\n",
            if enabled { "Enabled" } else { "Disabled" }
        ));
        changed.push(json!({"id": id, "enabled": enabled}));
    }
    state.save(&dir)?;
    Ok(Done::new(text, json!({"packages": changed})))
}

/// `verify`: every installed file against the lockfile, then every
/// installed version against the current index (still listed, the same
/// hash, and whether it is yanked since).
fn verify(options: &Options) -> Outcome<Done> {
    let dir = require_dir(options)?;
    let lock = lockfile(&dir)?;
    let mut problems = Vec::new();
    let mut yanked = Vec::new();
    let mut checked = 0;
    for entry in lock.entries() {
        checked += 1;
        if let (_, Some(problem)) = file_status(&dir, entry) {
            problems.push((entry.id.clone(), problem));
        }
    }
    let mut serial = None;
    let mut index_problem = None;
    if lock.entries().next().is_some() {
        let mut state = State::load(&dir)?;
        match open(options, &dir, &mut state) {
            Ok(market) => {
                serial = Some(market.index.serial());
                for entry in lock.entries() {
                    let listing = match market.listing(&entry.id) {
                        Ok(listing) => listing,
                        Err(failure) => {
                            problems.push((entry.id.clone(), failure.message));
                            continue;
                        }
                    };
                    let listed = listing
                        .versions
                        .iter()
                        .find(|v| v.version.to_string() == entry.version);
                    state.note_yank(&entry.id, &entry.version, listed);
                    match listed {
                        None => problems.push((
                            entry.id.clone(),
                            format!("version {} is no longer listed", entry.version),
                        )),
                        Some(v) if v.artifact.sha256 != entry.sha256 => problems.push((
                            entry.id.clone(),
                            format!(
                                "the listing's SHA-256 for {} is {}, the lockfile records {}",
                                entry.version, v.artifact.sha256, entry.sha256
                            ),
                        )),
                        Some(v) => {
                            if let Some(reason) = &v.yanked {
                                yanked.push((
                                    entry.id.clone(),
                                    entry.version.clone(),
                                    reason.clone(),
                                ));
                            }
                        }
                    }
                }
                state.save(&dir)?;
            }
            Err(failure) => index_problem = Some(failure),
        }
    }
    let mut text = String::new();
    if checked == 0 {
        text.push_str("Nothing is installed.\n");
    } else {
        text.push_str(&format!(
            "Checked {checked} installed package{}{}.\n",
            if checked == 1 { "" } else { "s" },
            serial.map_or_else(String::new, |s| format!(" against index serial {s}"))
        ));
    }
    for (id, version, reason) in &yanked {
        text.push_str(&format!(
            "{id} {version} is yanked: {}; `rich plugin update {id}` moves to the newest version\n",
            line(reason)
        ));
    }
    for (id, problem) in &problems {
        text.push_str(&format!("{id}: {}\n", line(problem)));
    }
    if let Some(failure) = &index_problem {
        text.push_str(&format!(
            "The marketplace index was not checked: {}\n",
            line(&failure.message)
        ));
    }
    let result = json!({
        "checked": checked,
        "index_serial": serial,
        "problems": problems.iter().map(|(id, p)| json!({"id": id, "problem": p})).collect::<Vec<_>>(),
        "yanked": yanked.iter().map(|(id, v, r)| json!({"id": id, "version": v, "reason": r})).collect::<Vec<_>>(),
        "index_error": index_problem.as_ref().map(|f| f.message.clone()),
    });
    let mut done = Done::new(text, result);
    if !problems.is_empty() {
        done.problem = Some(data(format!(
            "{} installed package{} failed verification",
            problems.len(),
            if problems.len() == 1 { "" } else { "s" }
        )));
    } else if let Some(failure) = index_problem {
        done.problem = Some(failure);
    }
    Ok(done)
}

// ---------------------------------------------------------------------------
// Shared with `browse`.
// ---------------------------------------------------------------------------

/// The commands `browse` runs on one package, with `id` as the only
/// positional.
#[cfg(feature = "interact")]
pub(crate) fn run_on(options: &Options, command: &str, id: &str) -> Outcome<Done> {
    let options = Options {
        command: command.to_string(),
        positionals: vec![id.to_string()],
        json: false,
        no_color: options.no_color,
        yes: options.yes,
        marketplace: options.marketplace.clone(),
        plugin_dir: options.plugin_dir.clone(),
        keys: options.keys.clone(),
        publish: BTreeMap::new(),
    };
    match command {
        "install" => install(&options),
        "update" => update(&options),
        "remove" => remove(&options),
        "enable" => set_enabled(&options, true),
        "disable" => set_enabled(&options, false),
        _ => info(&options),
    }
}

#[cfg(feature = "interact")]
pub(crate) fn open_for_browse(options: &Options) -> Outcome<(PathBuf, Market, Lockfile, State)> {
    let dir = require_dir(options)?;
    let mut state = State::load(&dir)?;
    let market = open(options, &dir, &mut state)?;
    let lock = lockfile(&dir)?;
    Ok((dir, market, lock, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn locations() {
        assert_eq!(
            Location::parse("https://example.test/m").unwrap(),
            Location::Remote("https://example.test/m/".into())
        );
        assert_eq!(
            Location::parse("file:///tmp/a%20b").unwrap(),
            Location::Local(PathBuf::from("/tmp/a b"))
        );
        assert!(Location::parse("http://example.test/").is_err());
        assert!(matches!(
            Location::parse("some/dir").unwrap(),
            Location::Local(_)
        ));
    }

    #[test]
    fn the_mirror_stays_inside_its_directory() {
        let mirror = ArtifactTransport::Mirror(PathBuf::from("/nonexistent-mirror"));
        for url in [
            "https://example.test/../etc/passwd",
            "https://example.test//x",
            "https://example.test/a\\b",
        ] {
            assert!(
                matches!(mirror.get(url, 10), Err(FetchError::NotFound { .. })),
                "{url}"
            );
        }
        assert!(matches!(
            mirror.get("http://example.test/x", 10),
            Err(FetchError::NotHttps { .. })
        ));
    }

    #[test]
    fn options_parse() {
        let options = parse(&args(&[
            "plugin",
            "install",
            "shout@0.2.0",
            "--yes",
            "--marketplace",
            "m",
        ]))
        .unwrap();
        assert_eq!(options.command, "install");
        assert_eq!(options.positionals, ["shout@0.2.0"]);
        assert!(options.yes);
        assert!(parse(&args(&["plugin", "frob"])).is_err());
        assert!(parse(&args(&["plugin"])).is_err());
        assert!(parse(&args(&["plugin", "list", "--url", "x"])).is_err());
        let (id, request) = requested_version("shout@0.2.0").unwrap();
        assert_eq!(id, "shout");
        assert!(matches!(request, Request::Exact(_)));
        assert!(requested_version("shout@x").is_err());
    }

    #[test]
    fn state_round_trips() {
        let mut state = State::default();
        state.set_enabled("shout", false);
        state.markets.insert(
            "file:///m".into(),
            MarketState {
                accepted: Some(AcceptedIndex {
                    serial: 3,
                    sha256: "0".repeat(64),
                }),
                keys: None,
            },
        );
        state
            .yanked
            .insert("shout".into(), ("0.2.1".into(), "bad".into()));
        let dir = tempfile::tempdir().unwrap();
        state.save(dir.path()).unwrap();
        assert_eq!(State::load(dir.path()).unwrap(), state);
        assert!(!state.is_enabled("shout"));
    }
}
