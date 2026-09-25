//! CLI-only TOML configuration and read-only inspection.
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use toml::Value;

type Settings = BTreeMap<String, Value>;
type ThemeStyles = BTreeMap<String, String>;

#[derive(Default)]
struct Configuration {
    /// The `[defaults]` table with the selected profile applied.
    settings: Settings,
    themes: BTreeMap<String, ThemeStyles>,
    /// The `[defaults]` table alone, for `config explain`.
    base: Settings,
    /// The applied profile's name and table, for `config explain`.
    profile: Option<(String, Settings)>,
    /// Set when a working-directory `rich.toml` asked for `no_color = false`
    /// while `NO_COLOR` is set, and was ignored (see `load`).
    ignored_color: bool,
    /// Set when a working-directory `rich.toml` named a `theme_file`, which
    /// was ignored (see `load`).
    ignored_theme_file: bool,
    /// Set when a working-directory `rich.toml` asked for `sanitize = false`,
    /// which was ignored (see `load`).
    ignored_sanitize: bool,
    /// Set when a working-directory `rich.toml` asked for
    /// `mermaid_backend = "mmdc"`, which was ignored (see `load`).
    ignored_mmdc: bool,
    /// The `export_*` keys a working-directory `rich.toml` set, which were
    /// ignored (see `load`).
    ignored_export: Vec<&'static str>,
}

pub(crate) fn validate_theme_name(name: &str) -> Result<(), String> {
    if name
        .bytes()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
    {
        Ok(())
    } else {
        Err(format!("invalid theme or style name {name:?}: expected letters, digits, underscores, dots or hyphens"))
    }
}

pub(crate) fn validate_theme_binding(name: &str, style: &str) -> Result<(), String> {
    validate_theme_name(name)?;
    rich::Style::parse(style)
        .map(|_| ())
        .map_err(|e| format!("invalid style for {name:?}: {e}"))
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigRoots {
    pub home: Option<PathBuf>,
    pub cwd: PathBuf,
    /// Whether `NO_COLOR` is set and non-empty.
    pub no_color_env: bool,
}

impl Default for ConfigRoots {
    fn default() -> Self {
        Self {
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            no_color_env: std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()),
        }
    }
}

#[derive(Default)]
struct Arguments {
    cleaned: Vec<String>,
    path: Option<PathBuf>,
    profile: Option<String>,
    theme: Option<String>,
    disabled: bool,
}

fn takes_value(arg: &str) -> bool {
    super::VALUE_OPTIONS.contains(&arg)
        || matches!(
            arg,
            "--image-anchor"
                | "--interval"
                | "--theme-style"
                | "--image-color"
                | "--image-dither"
                | "--image-color-distance"
        )
}

fn arguments(args: &[String]) -> Result<Arguments, String> {
    let mut result = Arguments::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--" => {
                result.cleaned.push(arg.clone());
                result.cleaned.extend(iter.cloned());
                break;
            }
            "--no-config" => result.disabled = true,
            "--config" | "--profile" | "--theme" => {
                let value = iter
                    .next()
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                if arg == "--config" {
                    result.path = Some(PathBuf::from(value));
                } else if arg == "--theme" {
                    validate_theme_name(value)?;
                    result.theme = Some(value.clone());
                } else {
                    result.profile = Some(value.clone());
                }
            }
            _ => {
                result.cleaned.push(arg.clone());
                if takes_value(arg) {
                    let value = iter
                        .next()
                        .ok_or_else(|| format!("{arg} requires a value"))?;
                    result.cleaned.push(value.clone());
                }
            }
        }
    }
    Ok(result)
}

fn boolean_flags(key: &str) -> Option<(&'static str, &'static str)> {
    Some(match key {
        "pager" => ("--pager", "--no-pager"),
        "auto_pager" => ("--auto-pager", "--no-auto-pager"),
        "no_color" => ("--no-color", "--color"),
        "batch" => ("--batch", "--no-batch"),
        "image_flip_horizontal" => ("--image-flip-horizontal", "--no-image-flip-horizontal"),
        "image_flip_vertical" => ("--image-flip-vertical", "--no-image-flip-vertical"),
        "image_grayscale" => ("--image-grayscale", "--no-image-grayscale"),
        "batch_preserve_dirs" => ("--batch-preserve-dirs", "--no-batch-preserve-dirs"),
        "continue_on_error" => ("--continue-on-error", "--no-continue-on-error"),
        "overwrite" => ("--overwrite", "--no-overwrite"),
        "watch" => ("--watch", "--no-watch"),
        "watch_cache" => ("--watch-cache", "--no-watch-cache"),
        "watch_poll" => ("--watch-poll", "--no-watch-poll"),
        "watch_exit_on_error" => ("--watch-exit-on-error", "--no-watch-exit-on-error"),
        "sanitize" => ("--sanitize", "--no-sanitize"),
        "progress" => ("--progress", "--no-progress"),
        _ => return None,
    })
}

const BOOLEAN_KEYS: &[&str] = &[
    "image_flip_horizontal",
    "image_flip_vertical",
    "image_grayscale",
    "batch_preserve_dirs",
    "pager",
    "auto_pager",
    "no_color",
    "batch",
    "continue_on_error",
    "overwrite",
    "watch",
    "watch_cache",
    "watch_poll",
    "watch_exit_on_error",
    "sanitize",
    "progress",
];
const VALUE_KEYS: &[&str] = &[
    "image_rotate",
    "batch_input_root",
    "batch_name_template",
    "width",
    "height",
    "jobs",
    "watch_interval",
    "watch_debounce",
    "export_html",
    "export_svg",
    "panel",
    "padding",
    "collision",
    "image_fit",
    "image_anchor",
    "image_background",
    "image_color",
    "image_dither",
    "image_color_distance",
    "image_max_width",
    "image_max_height",
    "image_brightness",
    "image_contrast",
    "image_gamma",
    "log_presentation",
    "format",
    "theme_file",
    "mermaid_backend",
    "highlighter",
    "code_theme",
];

pub(crate) fn validate_value(key: &str, value: &Value) -> Result<(), String> {
    let valid = if boolean_flags(key).is_some() {
        value.is_bool()
    } else {
        match key {
            "image_rotate" => value
                .as_integer()
                .is_some_and(|v| matches!(v, 0 | 90 | 180 | 270)),
            "width" | "height" | "jobs" | "image_max_width" | "image_max_height" => value
                .as_integer()
                .is_some_and(|v| v > 0 && usize::try_from(v).is_ok()),
            "watch_interval" => value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
                .is_some_and(|v| v.is_finite() && v > 0.0),
            "watch_debounce" => value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
                .is_some_and(|v| v.is_finite() && (0.0..=3600.0).contains(&v)),
            "mode" => value.as_str().is_some_and(|v| {
                matches!(
                    v,
                    "print"
                        | "markdown"
                        | "json"
                        | "syntax"
                        | "csv"
                        | "ipynb"
                        | "jsonl"
                        | "log"
                        | "rule"
                        | "image"
                        | "gif"
                        | "diff"
                        | "inspect"
                        | "ansi"
                )
            }),
            "collision" => value
                .as_str()
                .is_some_and(|v| matches!(v, "error" | "overwrite" | "suffix")),
            "theme" => value
                .as_str()
                .is_some_and(|v| validate_theme_name(v).is_ok()),
            "log_presentation" => value
                .as_str()
                .is_some_and(|v| matches!(v, "plain" | "rich")),
            "mermaid_backend" => value
                .as_str()
                .is_some_and(|v| matches!(v, "text" | "mmdc" | "off")),
            // A choice among the engines compiled in, so a project's
            // `rich.toml` may set it; the name is checked when rich runs, with
            // the choices in the error.
            "highlighter" | "code_theme" => value.as_str().is_some_and(|v| !v.is_empty()),
            "format" => value
                .as_str()
                .is_some_and(|v| crate::inspect::InputFormat::parse(v).is_ok()),
            "image_color" => value
                .as_str()
                .is_some_and(|v| matches!(v, "truecolor" | "ansi256" | "ansi16" | "grayscale")),
            "image_brightness" | "image_contrast" => value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
                .is_some_and(|v| v.is_finite() && v >= 0.0),
            "image_gamma" => value
                .as_float()
                .or_else(|| value.as_integer().map(|v| v as f64))
                .is_some_and(|v| v.is_finite() && v > 0.0),
            "image_dither" => value
                .as_str()
                .is_some_and(|v| matches!(v, "none" | "floyd-steinberg" | "bayer4x4" | "atkinson")),
            "image_color_distance" => value.as_str().is_some_and(|v| matches!(v, "rgb" | "oklab")),
            "image_fit" => value
                .as_str()
                .is_some_and(|v| matches!(v, "contain" | "cover" | "stretch")),
            "image_anchor" => value.as_str().is_some_and(|v| {
                matches!(
                    v,
                    "top-left"
                        | "top"
                        | "top-right"
                        | "left"
                        | "center"
                        | "right"
                        | "bottom-left"
                        | "bottom"
                        | "bottom-right"
                )
            }),
            "image_background" => value
                .as_str()
                .is_some_and(|v| super::ImageBackdrop::parse(v).is_some()),
            "panel" => value.as_str().is_some_and(|v| {
                matches!(
                    v.to_ascii_lowercase().as_str(),
                    "none" | "ascii" | "ascii2" | "square" | "rounded" | "heavy" | "double"
                )
            }),
            "padding" => value.as_str().is_some_and(|v| {
                let parts: Vec<_> = v.split(',').collect();
                matches!(parts.len(), 1 | 2 | 4)
                    && parts.iter().all(|p| p.trim().parse::<usize>().is_ok())
            }),
            "theme_file" => value.as_str().is_some_and(|v| !v.is_empty()),
            "export_html" | "export_svg" | "batch_input_root" | "batch_name_template" => {
                value.is_str()
            }
            _ => return Err(format!("unknown key {key:?}")),
        }
    };
    if valid {
        Ok(())
    } else {
        Err(format!("invalid type or value for {key:?}: {value}"))
    }
}

fn section(value: &Value, name: &str) -> Result<Settings, String> {
    let table = value
        .as_table()
        .ok_or_else(|| format!("{name} must be a table"))?;
    for (key, value) in table {
        validate_value(key, value).map_err(|e| format!("{name}.{e}"))?;
    }
    let mut settings: Settings = table.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    // A TOML table has no flag order. Prefer the specific automatic paging and
    // collision policy keys when both forms occur in the same layer.
    if settings.get("auto_pager").and_then(Value::as_bool) == Some(true) {
        settings.insert("pager".into(), Value::Boolean(false));
    } else if settings.contains_key("pager") {
        settings.insert("auto_pager".into(), Value::Boolean(false));
    }
    if let Some(policy) = settings.get("collision").and_then(Value::as_str) {
        settings.insert("overwrite".into(), Value::Boolean(policy == "overwrite"));
    } else if let Some(overwrite) = settings.get("overwrite").and_then(Value::as_bool) {
        settings.insert(
            "collision".into(),
            Value::String(if overwrite { "overwrite" } else { "error" }.into()),
        );
    }
    Ok(settings)
}

fn decode_configuration(text: &str, selected: Option<&str>) -> Result<Configuration, String> {
    let document: Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let table = document.as_table().ok_or("configuration must be a table")?;
    let mut defaults = Settings::new();
    let mut profiles = BTreeMap::new();
    let mut themes = BTreeMap::new();
    for (key, value) in table {
        match key.as_str() {
            "version" if value.as_integer() == Some(1) => {}
            "version" => return Err("version must be integer 1".into()),
            "defaults" => defaults = section(value, "defaults")?,
            "themes" => {
                for (name, bindings) in value.as_table().ok_or("themes must be a table")? {
                    validate_theme_name(name)?;
                    let mut styles = ThemeStyles::new();
                    for (style_name, style) in bindings
                        .as_table()
                        .ok_or_else(|| format!("themes.{name} must be a table"))?
                    {
                        let style = style.as_str().ok_or_else(|| {
                            format!("themes.{name}.{style_name} must be a style string")
                        })?;
                        validate_theme_binding(style_name, style)
                            .map_err(|e| format!("themes.{name}: {e}"))?;
                        styles.insert(style_name.clone(), style.into());
                    }
                    themes.insert(name.clone(), styles);
                }
            }
            "profile" | "profiles" => {
                for (name, settings) in value
                    .as_table()
                    .ok_or_else(|| format!("{key} must be a table"))?
                {
                    let settings = section(settings, &format!("{key}.{name}"))?;
                    if profiles.insert(name.clone(), settings).is_some() {
                        return Err(format!(
                            "duplicate profile {name:?} in profile and profiles"
                        ));
                    }
                }
            }
            _ => return Err(format!("unknown key {key:?}")),
        }
    }
    // Validate references in every layer, including profiles not selected now.
    for settings in std::iter::once(&defaults).chain(profiles.values()) {
        if let Some(name) = settings.get("theme").and_then(Value::as_str) {
            if !themes.contains_key(name) {
                return Err(format!("unknown theme {name:?}"));
            }
        }
    }
    let base = defaults.clone();
    let name = selected.unwrap_or("default");
    let mut applied = None;
    if let Some(profile) = profiles.remove(name) {
        defaults.extend(profile.clone());
        applied = Some((name.to_string(), profile));
    } else if selected.is_some() {
        return Err(format!("unknown profile {name:?}"));
    }
    Ok(Configuration {
        settings: defaults,
        themes,
        base,
        profile: applied,
        ignored_color: false,
        ignored_theme_file: false,
        ignored_sanitize: false,
        ignored_mmdc: false,
        ignored_export: Vec::new(),
    })
}

#[cfg(test)]
fn decode(text: &str, selected: Option<&str>) -> Result<Settings, String> {
    decode_configuration(text, selected).map(|configuration| configuration.settings)
}

fn load(args: &Arguments, roots: &ConfigRoots) -> Result<(Configuration, Option<PathBuf>), String> {
    if args.disabled {
        return Ok((Configuration::default(), None));
    }
    let path = args
        .path
        .as_ref()
        .map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                roots.cwd.join(p)
            }
        })
        .or_else(|| {
            std::iter::once(roots.cwd.join("rich.toml"))
                .chain(
                    roots
                        .home
                        .as_ref()
                        .map(|h| h.join(".config/rich/config.toml")),
                )
                .find(|p| p.is_file())
        });
    let Some(path) = path else {
        if let Some(profile) = &args.profile {
            return Err(format!("unknown profile {profile:?}: no config found"));
        }
        return Ok((Configuration::default(), None));
    };
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("config {}: {e}", path.display()))?;
    let mut settings = decode_configuration(&text, args.profile.as_deref())
        .map_err(|e| format!("config {}: {e}", path.display()))?;
    // A `rich.toml` found in the working directory belongs to whatever project
    // the user is in, not to the user, so it may turn colour off but not back
    // on against `NO_COLOR`. The user's own config (`~/.config/rich`, or a file
    // named with `--config`) still can, as the NO_COLOR convention allows, and
    // so can `--color`.
    let untrusted = args.path.is_none() && path == roots.cwd.join("rich.toml");
    if untrusted && roots.no_color_env {
        let enables =
            |settings: &Settings| settings.get("no_color") == Some(&Value::Boolean(false));
        let mut ignored = false;
        for table in std::iter::once(&mut settings.settings)
            .chain(std::iter::once(&mut settings.base))
            .chain(settings.profile.as_mut().map(|(_, table)| table))
        {
            if enables(table) {
                table.remove("no_color");
                ignored = true;
            }
        }
        settings.ignored_color = ignored;
    }
    // For the same reason it may not name a file for every command to read
    // (`theme_file`: a FIFO hangs every run) or to write (`export_html`,
    // `export_svg`: any path the user can write), nor turn off the sanitizing
    // `rich view` and `rich diff` do by default, nor start a browser for every
    // Markdown document (`mermaid_backend = "mmdc"`).
    if untrusted {
        let (mut theme_file, mut sanitize, mut mmdc) = (false, false, false);
        let mut export = Vec::new();
        for table in std::iter::once(&mut settings.settings)
            .chain(std::iter::once(&mut settings.base))
            .chain(settings.profile.as_mut().map(|(_, table)| table))
        {
            theme_file |= table.remove("theme_file").is_some();
            if table.get("sanitize") == Some(&Value::Boolean(false)) {
                table.remove("sanitize");
                sanitize = true;
            }
            if table.get("mermaid_backend").and_then(Value::as_str) == Some("mmdc") {
                table.remove("mermaid_backend");
                mmdc = true;
            }
            for key in EXPORT_KEYS {
                if table.remove(*key).is_some() && !export.contains(key) {
                    export.push(*key);
                }
            }
        }
        export.sort_unstable();
        settings.ignored_theme_file = theme_file;
        settings.ignored_sanitize = sanitize;
        settings.ignored_mmdc = mmdc;
        settings.ignored_export = export;
    }
    // A theme file named in a config file is relative to that file, so the
    // config works from any directory.
    if let Some(dir) = path.parent() {
        for table in std::iter::once(&mut settings.settings)
            .chain(std::iter::once(&mut settings.base))
            .chain(settings.profile.as_mut().map(|(_, table)| table))
        {
            if let Some(Value::String(file)) = table.get_mut("theme_file") {
                if PathBuf::from(&*file).is_relative() {
                    *file = dir.join(&*file).to_string_lossy().into_owned();
                }
            }
        }
    }
    Ok((settings, Some(path)))
}

/// Return normalized explicit CLI settings while respecting consumed values and --.
fn overrides(args: &[String]) -> Settings {
    let mut result = Settings::new();
    let mut iter = args.iter();
    let mut positional = false;
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        let mut matched = false;
        for key in BOOLEAN_KEYS {
            let (yes, no) = boolean_flags(key).unwrap();
            if arg == yes || arg == no {
                result.insert((*key).into(), Value::Boolean(arg == yes));
                matched = true;
                break;
            }
        }
        if matched {
            match arg.as_str() {
                "--pager" | "--no-pager" => {
                    result.insert("auto_pager".into(), Value::Boolean(false));
                }
                "--auto-pager" => {
                    result.insert("pager".into(), Value::Boolean(false));
                }
                "--overwrite" | "--no-overwrite" => {
                    result.insert(
                        "collision".into(),
                        Value::String(
                            if arg == "--overwrite" {
                                "overwrite"
                            } else {
                                "error"
                            }
                            .into(),
                        ),
                    );
                }
                _ => {}
            }
            continue;
        }
        if let Some(flag) = super::mode_flag_alias(arg) {
            result.insert("mode".into(), Value::String(flag[2..].into()));
            continue;
        }
        if takes_value(arg) {
            if let Some(value) = iter.next() {
                let canonical = match arg.as_str() {
                    "-w" => "--width",
                    "-o" => "--export-html",
                    "--interval" => "--watch-interval",
                    _ => arg,
                };
                if let Some(key) = VALUE_KEYS
                    .iter()
                    .find(|k| format!("--{}", k.replace('_', "-")) == canonical)
                {
                    let value = match *key {
                        "image_rotate" | "width" | "height" | "jobs" => value
                            .parse::<i64>()
                            .map(Value::Integer)
                            .unwrap_or_else(|_| Value::String(value.clone())),
                        "watch_interval" | "watch_debounce" => value
                            .parse::<f64>()
                            .map(Value::Float)
                            .unwrap_or_else(|_| Value::String(value.clone())),
                        _ => Value::String(value.clone()),
                    };
                    if *key == "collision" {
                        result.insert(
                            "overwrite".into(),
                            Value::Boolean(value.as_str() == Some("overwrite")),
                        );
                    }
                    result.insert((*key).into(), value);
                }
            }
        } else if !arg.starts_with('-') && !positional {
            positional = true;
            if super::selects_mode_explicitly(std::slice::from_ref(arg)) {
                let mode = match arg.as_str() {
                    "md" => "markdown",
                    "code" => "syntax",
                    "ndjson" => "jsonl",
                    _ => arg,
                };
                result.insert("mode".into(), Value::String(mode.into()));
            }
        }
    }
    result
}

/// Disabling watch drops inherited tuning, but explicit contradictory CLI
/// requests keep their usage error. Run after all layers so --watch can still
/// enable the tuning inherited through a profile that normally disables watch.
fn normalize_watch(settings: &mut Settings, explicit: &Settings) -> Result<(), String> {
    let watch = explicit
        .get("watch")
        .or_else(|| settings.get("watch"))
        .and_then(Value::as_bool);
    if watch == Some(false) {
        for key in [
            "watch_interval",
            "watch_cache",
            "watch_debounce",
            "watch_poll",
            "watch_exit_on_error",
        ] {
            settings.remove(key);
        }
    }
    if watch != Some(true) {
        for (key, flag) in [
            ("watch_cache", "--watch-cache"),
            ("watch_poll", "--watch-poll"),
            ("watch_exit_on_error", "--watch-exit-on-error"),
        ] {
            if explicit.get(key).and_then(Value::as_bool) == Some(true) {
                return Err(format!("{flag} requires --watch"));
            }
        }
        if explicit.contains_key("watch_debounce") {
            return Err("--watch-debounce requires --watch".into());
        }
        if explicit
            .get("watch_interval")
            .and_then(|value| {
                value
                    .as_float()
                    .or_else(|| value.as_integer().map(|v| v as f64))
            })
            .is_some_and(|interval| interval != 1.0)
        {
            return Err("--watch-interval requires --watch".into());
        }
    }
    Ok(())
}

fn selected_theme(
    configuration: &mut Configuration,
    args: &Arguments,
) -> Result<ThemeStyles, String> {
    if let Some(name) = &args.theme {
        configuration
            .settings
            .insert("theme".into(), Value::String(name.clone()));
    }
    let Some(name) = configuration.settings.get("theme").and_then(Value::as_str) else {
        return Ok(ThemeStyles::new());
    };
    configuration
        .themes
        .get(name)
        .cloned()
        .ok_or_else(|| format!("unknown theme {name:?}"))
}

fn explicit_theme_styles(args: &[String]) -> Result<ThemeStyles, String> {
    let mut result = ThemeStyles::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if arg == "--theme-style" {
            let binding = iter.next().ok_or("--theme-style requires NAME=STYLE")?;
            let (name, style) = binding
                .split_once('=')
                .ok_or("--theme-style requires NAME=STYLE")?;
            validate_theme_binding(name, style)?;
            result.insert(name.into(), style.into());
        } else if takes_value(arg) {
            iter.next();
        }
    }
    Ok(result)
}

pub(crate) fn config_args(args: &[String], roots: &ConfigRoots) -> Result<Vec<String>, String> {
    let json_report = super::wants_json_report(args);
    let args = arguments(args)?;
    let (mut configuration, source) = load(&args, roots)?;
    let theme_styles = selected_theme(&mut configuration, &args)?;
    explicit_theme_styles(&args.cleaned)?;
    let mut settings = configuration.settings;
    let overrides = overrides(&args.cleaned);
    normalize_watch(&mut settings, &overrides)?;
    let explicit: BTreeSet<_> = overrides.into_keys().collect();
    // Moot when the command line names its own theme file.
    if configuration.ignored_theme_file && !json_report && !explicit.contains("theme_file") {
        eprintln!("rich: warning: {UNTRUSTED_THEME_FILE}");
    }
    for key in &configuration.ignored_export {
        if !json_report && !explicit.contains(*key) {
            eprintln!("rich: warning: {}", untrusted_export(key));
        }
    }
    if configuration.ignored_mmdc && !json_report && !explicit.contains("mermaid_backend") {
        eprintln!("rich: warning: {UNTRUSTED_MMDC}");
    }
    let mut result = Vec::new();
    for (name, style) in theme_styles {
        result.extend(["--theme-style".into(), format!("{name}={style}")]);
    }
    settings.remove("theme");
    let mut settings: Vec<_> = settings.into_iter().collect();
    // Inverse flags reset related state: emit the final policy after its reset.
    settings.sort_by_key(|(key, _)| match key.as_str() {
        "pager" | "overwrite" => 0,
        "auto_pager" | "collision" => 2,
        _ => 1,
    });
    for (key, value) in settings {
        // A configured `format` is for automatic and inspect modes; it must not
        // turn `rich --json file` into a usage error.
        let explicit_mode = super::explicit_mode(&args.cleaned);
        if explicit.contains(&key)
            || (key == "mode" && explicit_mode.is_some())
            || (key == "format" && explicit_mode.is_some_and(|mode| mode != super::Mode::Inspect))
        {
            continue;
        }
        if let (Some(file), Some(config)) =
            (value.as_str().filter(|_| key == "theme_file"), &source)
        {
            // Name the setting in errors: the command line has no --theme-file.
            let label = format!("config {}: theme_file {file}", config.display());
            super::read_theme_file(file, &label)?;
        }
        if let Some((yes, no)) = boolean_flags(&key) {
            result.push(if value.as_bool().unwrap() { yes } else { no }.to_string());
        } else if key == "mode" {
            result.push(format!("--{}", value.as_str().unwrap()));
        } else {
            result.push(format!("--{}", key.replace('_', "-")));
            result.push(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            );
        }
    }
    result.extend(args.cleaned);
    Ok(result)
}

pub(crate) fn inspect(args: &[String], roots: &ConfigRoots) -> Result<Option<String>, String> {
    let args = arguments(args)?;
    let mut positions = Vec::new();
    let mut iter = args.cleaned.iter().enumerate();
    while let Some((index, arg)) = iter.next() {
        if arg == "--" {
            break;
        }
        if takes_value(arg) {
            iter.next();
        } else if !arg.starts_with('-') {
            positions.push(index);
        }
    }
    if positions.first().map(|i| args.cleaned[*i].as_str()) != Some("config") {
        return Ok(None);
    }
    let command = positions.get(1).map(|i| args.cleaned[*i].as_str());
    let no_color = super::cli_spec::no_color_requested(&args.cleaned);
    if args
        .cleaned
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h")
    {
        let mut path = vec!["config"];
        path.extend(
            command.filter(|c| matches!(*c, "show" | "validate" | "explain" | "reference")),
        );
        return Ok(super::cli_spec::subcommand_help(&path, no_color));
    }
    let key = match (command, positions.len()) {
        (Some("show" | "validate" | "explain" | "reference"), 2) => None,
        (Some("explain"), 3) => Some(args.cleaned[positions[2]].as_str()),
        _ => return Err("config requires show, validate, explain [KEY] or reference".into()),
    };
    let mut iter = args.cleaned.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            if iter.next().is_some() {
                return Err("config inspection does not accept trailing operands".into());
            }
            break;
        }
        if !arg.starts_with('-') {
            continue;
        }
        let boolean = BOOLEAN_KEYS.iter().any(|key| {
            let (yes, no) = boolean_flags(key).unwrap();
            arg == yes || arg == no
        });
        let value_option = VALUE_KEYS
            .iter()
            .any(|key| format!("--{}", key.replace('_', "-")) == *arg)
            || matches!(arg.as_str(), "-w" | "-o" | "--interval" | "--theme-style");
        if value_option {
            iter.next();
        } else if arg == "--report" {
            if iter.next().map(String::as_str) != Some("json") {
                return Err("--report requires json".into());
            }
        } else if !boolean && super::mode_flag_alias(arg).is_none() && arg != "--machine-json" {
            return Err(format!("unknown config inspection option {arg:?}"));
        }
    }
    if command == Some("reference") {
        let reference = super::cli_spec::config_reference();
        return Ok(Some(super::cli_spec::render(&reference, no_color)));
    }
    if let Some(key) = key {
        let known = super::cli_spec::known_config_keys();
        if !known.iter().any(|k| k == key) {
            let close = rich_ext::cli_doc::suggest(key, &known);
            let hint = if close.is_empty() {
                format!("; keys are {}", known.join(", "))
            } else {
                format!("; did you mean {}?", close.join(" or "))
            };
            return Err(format!("unknown config key {key:?}{hint}"));
        }
    }
    let (mut configuration, source) = load(&args, roots)?;
    let mut theme_styles = selected_theme(&mut configuration, &args)?;
    theme_styles.extend(explicit_theme_styles(&args.cleaned)?);
    let mut settings = configuration.settings;
    let overrides = overrides(&args.cleaned);
    for (key, value) in &overrides {
        validate_value(key, value)?;
    }
    normalize_watch(&mut settings, &overrides)?;
    if command == Some("explain") {
        let layers = Layers {
            base: &configuration.base,
            profile: configuration.profile.as_ref(),
            source: source.as_deref().map(|path| shown_path(path, roots)),
            theme: args.theme.as_deref(),
            overrides: &overrides,
            no_color_env: roots.no_color_env,
            ignored_color: configuration.ignored_color,
            ignored_theme_file: configuration.ignored_theme_file,
            ignored_sanitize: configuration.ignored_sanitize,
            ignored_mmdc: configuration.ignored_mmdc,
            ignored_export: &configuration.ignored_export,
        };
        return Ok(Some(explain(&layers, key)));
    }
    settings.extend(overrides);
    let output = serde_json::json!({
        "valid": true,
        "source": source.map(|p| p.to_string_lossy().into_owned()),
        "profile": if args.disabled { None } else { Some(args.profile.as_deref().unwrap_or("default")) },
        "disabled": args.disabled,
        "theme": settings.get("theme").and_then(Value::as_str),
        "theme_styles": theme_styles,
        "settings": settings,
    });
    serde_json::to_string_pretty(&output)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// What `config explain` layers, lowest precedence first after the defaults.
struct Layers<'a> {
    base: &'a Settings,
    profile: Option<&'a (String, Settings)>,
    /// The config file, as `shown_path` displays it.
    source: Option<String>,
    theme: Option<&'a str>,
    overrides: &'a Settings,
    no_color_env: bool,
    /// The working-directory config's `no_color = false` was ignored.
    ignored_color: bool,
    /// The working-directory config's `theme_file` was ignored.
    ignored_theme_file: bool,
    /// The working-directory config's `sanitize = false` was ignored.
    ignored_sanitize: bool,
    /// The working-directory config's `mermaid_backend = "mmdc"` was ignored.
    ignored_mmdc: bool,
    /// The working-directory config's `export_*` keys that were ignored.
    ignored_export: &'a [&'static str],
}

/// A config path relative to the working directory, or under `~`, when it is.
fn shown_path(path: &std::path::Path, roots: &ConfigRoots) -> String {
    if let Ok(relative) = path.strip_prefix(&roots.cwd) {
        return relative.display().to_string();
    }
    if let Some(relative) = roots.home.as_ref().and_then(|h| path.strip_prefix(h).ok()) {
        return format!("~/{}", relative.display());
    }
    path.display().to_string()
}

fn shown(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

/// The layers in the order the binary applies them: `NO_COLOR` seeds the
/// parser's state before any flag, and config values arrive as flags ahead
/// of the command line's, so a config `no_color = false` beats `NO_COLOR`
/// (except from a working-directory `rich.toml`, which `load` has dropped).
fn precedence(layers: &Layers) -> rich_ext::cli_doc::Precedence {
    use rich_ext::cli_doc::{Layer, Precedence};
    let table = |mut layer: Layer, settings: &Settings| {
        for (key, value) in settings {
            layer = layer.value(key.clone(), shown(value));
        }
        layer
    };
    let mut precedence = Precedence::new().layer(super::cli_spec::default_layer());
    if layers.no_color_env {
        precedence = precedence.layer(
            Layer::new("environment")
                .origin("NO_COLOR")
                .value("no_color", "true"),
        );
    }
    if let Some(path) = &layers.source {
        let file = Layer::new("config file").origin(path.clone());
        precedence = precedence.layer(table(file, layers.base));
    }
    if let Some((name, settings)) = layers.profile {
        let profile = Layer::new("profile").origin(format!("profile {name}"));
        precedence = precedence.layer(table(profile, settings));
    }
    let mut command_line = table(Layer::new("command line"), layers.overrides);
    if let Some(theme) = layers.theme {
        command_line = command_line.value("theme", theme);
    }
    precedence.layer(command_line)
}

/// `config explain [KEY]`, rendered. Colour follows the effective `no_color`.
fn explain(layers: &Layers, key: Option<&str>) -> String {
    let precedence = precedence(layers);
    let no_color = precedence
        .resolve()
        .iter()
        .any(|r| r.key == "no_color" && r.value == "true");
    let render = |renderable: &dyn rich::Renderable| super::cli_spec::render(renderable, no_color);
    let mut output = match key {
        None => render(&precedence.view()),
        Some(key) => match precedence.explain(key) {
            Some(explanation) => render(&explanation),
            None => format!("{key} is not set by any layer"),
        },
    };
    if layers.ignored_color && key.is_none_or(|key| key == "no_color") {
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(
            "note: no_color = false in ./rich.toml is ignored while NO_COLOR is set; \
             pass --color, or set it in ~/.config/rich/config.toml, to override NO_COLOR\n",
        );
    }
    for (ignored, name, note) in [
        (
            layers.ignored_theme_file,
            "theme_file",
            UNTRUSTED_THEME_FILE,
        ),
        (layers.ignored_sanitize, "sanitize", UNTRUSTED_SANITIZE),
        (layers.ignored_mmdc, "mermaid_backend", UNTRUSTED_MMDC),
    ] {
        if ignored && key.is_none_or(|key| key == name) {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&format!("note: {note}\n"));
        }
    }
    for name in layers.ignored_export {
        if key.is_none_or(|key| key == *name) {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&format!("note: {}\n", untrusted_export(name)));
        }
    }
    output
}

/// Why a working-directory `rich.toml`'s `theme_file` has no effect.
pub(crate) const UNTRUSTED_THEME_FILE: &str =
    "theme_file in ./rich.toml is ignored: a project's config may not name a file for every \
     command to read; pass --theme-file, or set it in ~/.config/rich/config.toml or a file \
     given with --config";

/// Settings that name a file `rich` writes.
const EXPORT_KEYS: &[&str] = &["export_html", "export_svg"];

/// Why a working-directory `rich.toml`'s `export_html` or `export_svg` has no
/// effect.
fn untrusted_export(key: &str) -> String {
    format!(
        "{key} in ./rich.toml is ignored: a project's config may not choose files for rich to \
         write; pass --{}, or set it in ~/.config/rich/config.toml or a file given with --config",
        key.replace('_', "-")
    )
}

/// Why a working-directory `rich.toml`'s `sanitize = false` has no effect.
const UNTRUSTED_SANITIZE: &str =
    "sanitize = false in ./rich.toml is ignored: a project's config may not let input control \
     the terminal; pass --no-sanitize, or set it in ~/.config/rich/config.toml";

/// Why a working-directory `rich.toml`'s `mermaid_backend = "mmdc"` has no
/// effect.
const UNTRUSTED_MMDC: &str =
    "mermaid_backend = \"mmdc\" in ./rich.toml is ignored: a project's config may not start a \
     browser; pass --mermaid-backend mmdc, or set it in ~/.config/rich/config.toml or a file \
     given with --config";

/// Every key `validate_value` accepts.
#[cfg(test)]
pub(crate) fn all_keys() -> Vec<&'static str> {
    let mut keys = vec!["mode", "theme"];
    keys.extend(BOOLEAN_KEYS);
    keys.extend(VALUE_KEYS);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).into()).collect()
    }

    #[test]
    fn themes_expand_to_a_self_contained_worker_snapshot() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("rich.toml"),
            "[defaults]\ntheme = 'day'\n[themes.day]\nalert = 'red'\n",
        )
        .unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        let expanded = config_args(
            &strings(&[
                "--theme-style",
                "alert=green",
                "--print",
                "[alert]hello[/alert]",
            ]),
            &roots,
        )
        .unwrap();
        assert_eq!(
            expanded,
            strings(&[
                "--theme-style",
                "alert=red",
                "--theme-style",
                "alert=green",
                "--print",
                "[alert]hello[/alert]"
            ])
        );
        std::fs::remove_file(root.path().join("rich.toml")).unwrap();
        let mut worker = strings(&["--no-config"]);
        worker.extend(expanded.clone());
        assert_eq!(config_args(&worker, &roots).unwrap(), expanded);
    }

    #[test]
    fn full_toml_and_profile_precedence() {
        let settings = decode("[profiles.ci]\npager = false\nexport_html = \"line\\t#quoted.html\"\n[defaults]\npager = true\nwidth = 1_000\n", Some("ci")).unwrap();
        assert_eq!(settings["pager"].as_bool(), Some(false));
        assert_eq!(settings["width"].as_integer(), Some(1000));
        assert_eq!(settings["export_html"].as_str(), Some("line\t#quoted.html"));
    }

    #[test]
    fn unused_profiles_and_schema_are_validated() {
        for text in [
            "[profiles.unused]\nwat = true",
            "[profiles.unused]\npager = 'false'",
            "[profiles.unused]\nimage_fit = 'squash'",
            "version = 2",
            "[defaults]\nwidth = -1",
            "[defaults]\nwatch_interval = nan",
        ] {
            assert!(decode(text, None).is_err(), "accepted {text}");
        }
        assert!(decode("[defaults]\nwidth = 2", Some("missing"))
            .unwrap_err()
            .contains("unknown profile"));
    }

    #[test]
    fn controls_respect_values_and_terminator() {
        let raw = strings(&[
            "--title",
            "--no-config",
            "--config",
            "yes.toml",
            "--",
            "--profile",
            "literal",
        ]);
        let parsed = arguments(&raw).unwrap();
        assert!(!parsed.disabled);
        assert_eq!(parsed.path, Some(PathBuf::from("yes.toml")));
        assert_eq!(
            parsed.cleaned,
            strings(&["--title", "--no-config", "--", "--profile", "literal"])
        );
        let parsed = arguments(&strings(&["--no-config", "--title", "--config"])).unwrap();
        assert!(parsed.disabled);
        assert_eq!(parsed.cleaned, strings(&["--title", "--config"]));
    }

    #[test]
    fn merged_flags_and_inspection_share_the_effective_configuration() {
        let root = std::env::temp_dir().join(format!("rich-v8-config-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("rich.toml"),
            "[profiles.ci]\npager = false\nwidth = 40\n[defaults]\npager = true\nwidth = 80\n",
        )
        .unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.clone(),
            no_color_env: false,
        };
        let merged = config_args(
            &strings(&["--profile", "ci", "--width", "60", "input"]),
            &roots,
        )
        .unwrap();
        assert_eq!(
            merged,
            strings(&["--no-pager", "--no-auto-pager", "--width", "60", "input"])
        );
        let output = inspect(
            &strings(&["config", "show", "--profile", "ci", "--pager"]),
            &roots,
        )
        .unwrap()
        .unwrap();
        let output: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(output["settings"]["pager"], true);
        assert_eq!(output["settings"]["width"], 40);
        assert_eq!(output["profile"], "ci");
        assert!(inspect(&strings(&["--title", "config", "input"]), &roots)
            .unwrap()
            .is_none());
        assert!(inspect(&strings(&["--", "config", "show"]), &roots)
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inspection_rejects_unknown_flags_and_accepts_image_modes() {
        let root = tempfile::tempdir().unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        for flag in ["--bogus", "--title"] {
            let mut args = strings(&["config", "validate", flag]);
            if flag == "--title" {
                args.push("value".into());
            }
            assert!(inspect(&args, &roots)
                .unwrap_err()
                .contains("unknown config inspection option"));
        }
        assert!(inspect(&strings(&["config", "show", "--image"]), &roots)
            .unwrap()
            .unwrap()
            .contains("image"));
        for mode in ["image", "gif", "diff"] {
            assert!(decode(&format!("[defaults]\nmode = '{mode}'"), None).is_ok());
        }
        for (alias, canonical) in [("md", "markdown"), ("code", "syntax"), ("ndjson", "jsonl")] {
            assert_eq!(
                overrides(&strings(&[alias]))["mode"].as_str(),
                Some(canonical)
            );
        }
    }

    #[test]
    fn related_settings_match_runtime_and_cli_order() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("rich.toml"),
            "[defaults]\npager = true\nauto_pager = true\noverwrite = true\n",
        )
        .unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        for flags in [
            vec![],
            vec!["--no-pager"],
            vec!["--pager"],
            vec!["--collision", "suffix"],
            vec!["--no-overwrite"],
        ] {
            let mut args = strings(&flags);
            args.push("input.txt".into());
            let merged = config_args(&args, &roots).unwrap();
            let cli = super::super::parse_inner(&merged).unwrap().unwrap();
            let mut inspection = strings(&["config", "show"]);
            inspection.extend(strings(&flags));
            let output: serde_json::Value =
                serde_json::from_str(&inspect(&inspection, &roots).unwrap().unwrap()).unwrap();
            assert_eq!(output["settings"]["pager"], cli.pager, "{flags:?}");
            assert_eq!(
                output["settings"]["auto_pager"], cli.auto_pager,
                "{flags:?}"
            );
            assert_eq!(output["settings"]["overwrite"], cli.overwrite, "{flags:?}");
        }
        let settings = decode("[defaults]\npager = true\noverwrite = true\n[profiles.ci]\nauto_pager = true\ncollision = 'suffix'", Some("ci")).unwrap();
        assert_eq!(settings["pager"].as_bool(), Some(false));
        assert_eq!(settings["auto_pager"].as_bool(), Some(true));
        assert_eq!(settings["overwrite"].as_bool(), Some(false));
    }

    #[test]
    fn disabling_watch_suppresses_inherited_watch_dependencies() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("rich.toml"), "[defaults]\nwatch = true\nwatch_interval = 0.2\nwatch_cache = true\n[profiles.once]\nwatch = false\n").unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        for flags in [vec!["--profile", "once"], vec!["--no-watch"]] {
            let mut args = strings(&flags);
            args.push("input.txt".into());
            let merged = config_args(&args, &roots).unwrap();
            assert!(
                !merged
                    .iter()
                    .any(|arg| arg == "--watch-interval" || arg == "--watch-cache"),
                "{merged:?}"
            );
            let cli = super::super::parse_inner(&merged).unwrap().unwrap();
            assert!(!cli.watch);
            assert!(!cli.watch_cache);
            assert_eq!(cli.watch_interval, 1.0);
            let mut args = strings(&["config", "show"]);
            args.extend(strings(&flags));
            let output: serde_json::Value =
                serde_json::from_str(&inspect(&args, &roots).unwrap().unwrap()).unwrap();
            assert_eq!(output["settings"]["watch"], false);
            assert!(output["settings"].get("watch_interval").is_none());
            assert!(output["settings"].get("watch_cache").is_none());
        }
    }

    #[test]
    fn watch_debounce_poll_and_exit_on_error_are_configurable() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("rich.toml"),
            "[defaults]\nwatch = true\nwatch_debounce = 0.25\nwatch_poll = true\nwatch_exit_on_error = true\n[profiles.once]\nwatch = false\n",
        )
        .unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        let merged = config_args(&strings(&["a.md", "b.md"]), &roots).unwrap();
        let cli = super::super::parse_inner(&merged).unwrap().unwrap();
        assert!(cli.watch && cli.watch_poll && cli.watch_exit_on_error);
        assert_eq!(cli.watch_debounce, 0.25);
        assert_eq!(cli.resources, ["a.md", "b.md"]);

        // Explicit flags win over configured values.
        let merged = config_args(
            &strings(&["--watch-debounce", "0", "--no-watch-poll", "a.md"]),
            &roots,
        )
        .unwrap();
        let cli = super::super::parse_inner(&merged).unwrap().unwrap();
        assert_eq!(cli.watch_debounce, 0.0);
        assert!(!cli.watch_poll);

        // Disabling watch drops the inherited tuning.
        for flags in [vec!["--profile", "once"], vec!["--no-watch"]] {
            let mut args = strings(&flags);
            args.push("input.txt".into());
            let merged = config_args(&args, &roots).unwrap();
            let cli = super::super::parse_inner(&merged).unwrap().unwrap();
            assert!(!cli.watch && !cli.watch_poll && !cli.watch_exit_on_error);
        }

        // Explicit requests without watch remain usage errors.
        for flags in [
            vec!["--no-watch", "--watch-debounce", "0.5"],
            vec!["--no-watch", "--watch-poll"],
            vec!["--no-watch", "--watch-exit-on-error"],
        ] {
            let result = config_args(&strings(&flags), &roots)
                .and_then(|merged| super::super::parse_inner(&merged).map(|_| ()));
            assert!(
                result.unwrap_err().contains("requires --watch"),
                "{flags:?}"
            );
        }

        for bad in ["-1", "nan", "\"fast\"", "3601"] {
            assert!(
                decode(&format!("[defaults]\nwatch_debounce = {bad}"), None).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn explicitly_requested_watch_dependencies_still_require_watch() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("rich.toml"),
            "[defaults]\nwatch = true\nwatch_interval = 0.2\nwatch_cache = true\n",
        )
        .unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        for flags in [
            vec!["--no-watch", "--watch-cache"],
            vec!["--no-watch", "--watch-interval", "0.5"],
        ] {
            let mut args = strings(&["config", "validate"]);
            args.extend(strings(&flags));
            assert!(inspect(&args, &roots)
                .unwrap_err()
                .contains("requires --watch"));
            let result = config_args(&strings(&flags), &roots)
                .and_then(|merged| super::super::parse_inner(&merged).map(|_| ()));
            assert!(result.unwrap_err().contains("requires --watch"));
        }
    }

    #[test]
    fn explicit_boolean_and_value_overrides_are_normalized() {
        let values = overrides(&strings(&[
            "--no-pager",
            "--color",
            "--no-watch",
            "--title",
            "--batch",
            "-w",
            "80",
            "--",
            "--overwrite",
        ]));
        assert_eq!(values["pager"].as_bool(), Some(false));
        assert_eq!(values["no_color"].as_bool(), Some(false));
        assert_eq!(values["watch"].as_bool(), Some(false));
        assert_eq!(values["width"].as_integer(), Some(80));
        assert!(!values.contains_key("batch"));
        assert!(!values.contains_key("overwrite"));
    }

    fn explain_fixture() -> (tempfile::TempDir, ConfigRoots) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("rich.toml"),
            "version = 1\n[defaults]\nwidth = 100\npager = true\n[profile.ci]\nwidth = 60\n",
        )
        .unwrap();
        let roots = ConfigRoots {
            home: None,
            cwd: root.path().into(),
            no_color_env: false,
        };
        (root, roots)
    }

    #[test]
    fn decoding_keeps_the_base_table_and_the_profile_apart() {
        let text = "[defaults]\nwidth = 100\npager = true\n[profile.ci]\nwidth = 60\n";
        let configuration = decode_configuration(text, Some("ci")).unwrap();
        assert_eq!(configuration.base["width"].as_integer(), Some(100));
        let (name, profile) = configuration.profile.as_ref().unwrap();
        assert_eq!(name, "ci");
        assert_eq!(profile["width"].as_integer(), Some(60));
        assert!(!profile.contains_key("pager"));
        // The merged settings `config show` reads are unchanged.
        assert_eq!(configuration.settings["width"].as_integer(), Some(60));
        assert_eq!(configuration.settings["pager"].as_bool(), Some(true));
        assert!(decode_configuration(text, None).unwrap().profile.is_none());
    }

    #[test]
    fn explain_traces_one_key_through_every_layer() {
        let (_root, roots) = explain_fixture();
        let output = inspect(
            &strings(&[
                "config",
                "explain",
                "width",
                "--profile",
                "ci",
                "--width",
                "40",
            ]),
            &roots,
        )
        .unwrap()
        .unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "width = 40", "{output}");
        assert!(lines[1].contains("command line") && lines[1].contains("effective"));
        assert!(lines[2].contains("profile (profile ci)") && lines[2].contains("60"));
        assert!(lines[2].contains("overridden by command line"));
        assert!(lines[3].contains("config file (rich.toml)") && lines[3].contains("100"));
        assert!(lines[3].contains("overridden by profile"));

        // Built-in defaults sit below the file.
        let output = inspect(&strings(&["config", "explain", "pager"]), &roots)
            .unwrap()
            .unwrap();
        assert!(output.starts_with("pager = true"), "{output}");
        assert!(output.contains("defaults") && output.contains("overridden by config file"));
    }

    #[test]
    fn explain_without_a_key_tables_every_layer() {
        let (_root, roots) = explain_fixture();
        let output = inspect(
            &strings(&["config", "explain", "--no-config", "--jobs", "4"]),
            &roots,
        )
        .unwrap()
        .unwrap();
        for column in ["Key", "defaults", "command line", "Effective"] {
            assert!(output.contains(column), "{column}:\n{output}");
        }
        assert!(!output.contains("config file"), "{output}");
        let jobs = output.lines().find(|l| l.contains("jobs")).unwrap();
        assert!(jobs.contains('1') && jobs.contains('4'), "{jobs}");
    }

    #[test]
    fn explain_rejects_unknown_keys_and_reports_unset_ones() {
        let (_root, roots) = explain_fixture();
        let error = inspect(&strings(&["config", "explain", "widht"]), &roots).unwrap_err();
        assert!(
            error.contains("unknown config key") && error.contains("width"),
            "{error}"
        );
        let output = inspect(&strings(&["config", "explain", "export_html"]), &roots)
            .unwrap()
            .unwrap();
        assert_eq!(output, "export_html is not set by any layer");
        // The same flags `config show` accepts, and the same rejections.
        assert!(
            inspect(&strings(&["config", "explain", "--title", "x"]), &roots)
                .unwrap_err()
                .contains("unknown config inspection option")
        );
        assert!(inspect(&strings(&["config", "explain", "a", "b"]), &roots).is_err());
    }

    #[test]
    fn a_working_directory_config_cannot_undo_no_color() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("project");
        let home = root.path().join("home");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(home.join(".config/rich")).unwrap();
        let color = "[defaults]\nno_color = false\n[profile.p]\nno_color = false\n";
        let roots = |no_color_env| ConfigRoots {
            home: Some(home.clone()),
            cwd: cwd.clone(),
            no_color_env,
        };
        let merged = |args: &[&str], no_color_env| {
            config_args(&strings(args), &roots(no_color_env)).unwrap()
        };

        // The user's own config may override NO_COLOR, as the convention allows.
        std::fs::write(home.join(".config/rich/config.toml"), color).unwrap();
        assert!(merged(&["x.md"], true).contains(&"--color".to_string()));

        // A project's ./rich.toml may not, from [defaults] or a profile.
        std::fs::write(cwd.join("rich.toml"), color).unwrap();
        for args in [&["x.md"][..], &["--profile", "p", "x.md"]] {
            let args = merged(args, true);
            assert!(!args.contains(&"--color".to_string()), "{args:?}");
        }
        // Without NO_COLOR it still applies, and --color still wins.
        assert!(merged(&["x.md"], false).contains(&"--color".to_string()));
        assert!(merged(&["--color", "x.md"], true).contains(&"--color".to_string()));
        // A file named with --config is the user's choice, so it keeps its say.
        assert!(merged(&["--config", "rich.toml", "x.md"], true).contains(&"--color".to_string()));
        // Turning colour off is always allowed.
        std::fs::write(cwd.join("rich.toml"), "[defaults]\nno_color = true\n").unwrap();
        assert!(merged(&["x.md"], true).contains(&"--no-color".to_string()));

        // `config show` and `config explain` report the effective setting.
        std::fs::write(cwd.join("rich.toml"), color).unwrap();
        let show = inspect(&strings(&["config", "show"]), &roots(true))
            .unwrap()
            .unwrap();
        let show: serde_json::Value = serde_json::from_str(&show).unwrap();
        assert!(show["settings"].get("no_color").is_none(), "{show}");
        let explained = inspect(&strings(&["config", "explain", "no_color"]), &roots(true))
            .unwrap()
            .unwrap();
        assert!(explained.contains("NO_COLOR"), "{explained}");
        assert!(
            explained.contains("is ignored while NO_COLOR is set"),
            "{explained}"
        );
        let explained = inspect(&strings(&["config", "explain", "no_color"]), &roots(false))
            .unwrap()
            .unwrap();
        assert!(!explained.contains("ignored"), "{explained}");
    }

    #[test]
    fn reference_and_help_render_from_the_spec() {
        let (_root, roots) = explain_fixture();
        let output = inspect(&strings(&["config", "reference"]), &roots)
            .unwrap()
            .unwrap();
        for text in [
            "rich configuration",
            "config file",
            "./rich.toml",
            "NO_COLOR",
            "mode",
        ] {
            assert!(output.contains(text), "{text}:\n{output}");
        }
        let help = inspect(&strings(&["config", "explain", "--help"]), &roots)
            .unwrap()
            .unwrap();
        assert!(
            help.starts_with("Usage: rich config explain [OPTIONS] [KEY]"),
            "{help}"
        );
        let help = inspect(&strings(&["config", "--help"]), &roots)
            .unwrap()
            .unwrap();
        assert!(help.contains("reference"), "{help}");
        assert!(inspect(&strings(&["config"]), &roots)
            .unwrap_err()
            .contains("explain"));
    }
}
