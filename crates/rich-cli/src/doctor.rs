//! Read-only CLI diagnostics. Capability heuristics never probe the terminal.
use super::*;
use rich_ext::capabilities::{
    Capabilities, CapabilityReport, ColorDepth, Overrides, Report, SystemEnvironment,
};

pub(super) fn requested(args: &[String]) -> bool {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            return false;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
            continue;
        }
        if !arg.starts_with('-') || arg == "-" {
            return arg == "doctor";
        }
    }
    false
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h")
    {
        let no_color = cli_spec::no_color_requested(args);
        if let Some(help) = cli_spec::subcommand_help(&["doctor"], no_color) {
            authoring::out(&format!("{help}\n"));
        }
        return ExitCode::SUCCESS;
    }
    // Consume values before interpreting report flags, even option-looking values.
    let mut json = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            let value = iter.next();
            if arg == "--report" {
                json = value.map(String::as_str) == Some("json");
            }
        }
    }
    match report(args) {
        Ok((report, capabilities, no_color)) => {
            // Written as one piece, and a closed pipe (`rich doctor | head`)
            // is not a panic.
            let mut text = String::new();
            if json {
                text.push_str(&serde_json::to_string_pretty(&report).expect("diagnostic JSON"));
                text.push('\n');
            } else {
                let lines = [
                    format!(
                        "Rich doctor — {} {}",
                        env!("CARGO_PKG_NAME"),
                        env!("CARGO_PKG_VERSION")
                    ),
                    format!("Build features: {}", report["features"]),
                    format!(
                        "Plugins (API {}): {}",
                        report["plugins"]["api_version"],
                        report["plugins"]["registered"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|plugin| format!(
                                "{} {} ({})",
                                plugin["id"].as_str().unwrap_or_default(),
                                plugin["version"].as_str().unwrap_or_default(),
                                plugin["capabilities"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|c| c.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ),
                    format!(
                        "Terminal: stdout TTY={}, {}×{} cells, colour={} ({}); NO_COLOR={}",
                        report["terminal"]["stdout_tty"],
                        report["terminal"]["width"],
                        report["terminal"]["height"],
                        report["terminal"]["color"].as_str().unwrap(),
                        report["terminal"]["provenance"]["color_system"]
                            .as_str()
                            .unwrap(),
                        report["terminal"]["no_color"]
                    ),
                    format!(
                        "Image backend: {}; Sixel support={} (inferred, no probe)",
                        report["image"]["selected_mode"].as_str().unwrap(),
                        report["image"]["sixel_inferred"]
                    ),
                    format!(
                        "Configuration: source={}, profile={}, disabled={}",
                        report["config"]["source"],
                        report["config"]["profile"],
                        report["config"]["disabled"]
                    ),
                    format!(
                        "Pager: {} from {}; availability not checked; never launched",
                        report["pager"]["program"].as_str().unwrap(),
                        report["pager"]["source"].as_str().unwrap()
                    ),
                    String::new(),
                ];
                for line in lines {
                    text.push_str(&line);
                    text.push('\n');
                }
                let console = Console::builder().no_color(no_color).build();
                text.push_str(&console.render_to_string(&CapabilityReport::new(&capabilities)));
                text.push('\n');
            }
            authoring::out(&text);
            ExitCode::SUCCESS
        }
        Err(message) => emit_error(json, ExitClass::Usage, &format!("doctor: {message}")),
    }
}

/// The doctor report, the shared capability detection behind it, and whether
/// colour is off.
fn report(args: &[String]) -> Result<(serde_json::Value, Report, bool), String> {
    let mut inspect_args = vec!["config".into(), "show".into()];
    let mut command_seen = false;
    let mut image_mode = "auto".to_owned();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "doctor" if !command_seen => command_seen = true,
            "--config" | "--profile" | "--width" | "-w" | "--height" => {
                inspect_args.push(arg.clone());
                inspect_args.push(iter.next().ok_or_else(|| format!("{arg} requires a value"))?.clone());
            }
            "--image-mode" => {
                let value = iter.next().ok_or("--image-mode requires a value")?;
                let parsed = value.parse::<ImageMode>()?;
                image_mode = format!("{parsed:?}").to_lowercase();
            }
            "--report" => {
                if iter.next().map(String::as_str) != Some("json") { return Err("--report requires json".into()); }
            }
            "--no-config" | "--no-color" | "--color" | "--pager" | "--no-pager" | "--auto-pager" | "--no-auto-pager" => inspect_args.push(arg.clone()),
            _ => return Err(format!("unexpected argument {arg:?}; use doctor [--report json] [--config PATH] [--profile NAME] [--no-config] [--no-color]")),
        }
    }
    let config = config::inspect(&inspect_args, &ConfigRoots::default())?
        .ok_or("could not inspect configuration")?;
    let config: serde_json::Value = serde_json::from_str(&config).map_err(|e| e.to_string())?;
    let settings = &config["settings"];
    let no_color = settings["no_color"]
        .as_bool()
        .unwrap_or_else(|| std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()));
    let mut builder = Console::builder().no_color(no_color);
    if let Some(width) = settings["width"].as_u64().filter(|value| *value > 0) {
        builder = builder.width(width as usize);
    }
    if let Some(height) = settings["height"].as_u64().filter(|value| *value > 0) {
        builder = builder.height(height as usize);
    }
    let console = builder.build();
    let resolved = render_target::observe(
        &console,
        rich_ext::target::TargetOverrides {
            width: settings["width"].as_u64().map(|v| v as usize),
            height: settings["height"].as_u64().map(|v| v as usize),
            color_system: (settings["no_color"].is_boolean()
                || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()))
            .then_some(console.color_system().filter(|_| !console.no_color())),
            ..Default::default()
        },
    );
    let provenance: serde_json::Map<String, serde_json::Value> = resolved
        .origins
        .into_iter()
        .map(|(key, origin)| {
            (
                key,
                serde_json::Value::String(format!("{origin:?}").to_lowercase()),
            )
        })
        .collect();
    let color = if no_color {
        "none".into()
    } else {
        console
            .color_system()
            .map(|value| format!("{value:?}").to_lowercase())
            .unwrap_or_else(|| "none".into())
    };
    // The shared detection (rich_ext::capabilities), the same API library
    // users call; config width, height and no_color are its overrides.
    let capabilities = Capabilities::detect_with(
        &SystemEnvironment,
        &Overrides {
            width: settings["width"].as_u64().map(|v| v as usize),
            height: settings["height"].as_u64().map(|v| v as usize),
            color: no_color.then_some(ColorDepth::None),
            ..Default::default()
        },
    );
    let mut capabilities = capabilities;
    if no_color {
        capabilities.color.reason = "--no-color, a non-empty NO_COLOR or config no_color".into();
    }
    let sixel = cfg!(feature = "art") && capabilities.sixel.value;
    let requested_mode = image_mode.as_str();
    let selected_mode = if !cfg!(feature = "art") {
        "unavailable"
    } else if requested_mode != "auto" {
        requested_mode
    } else if color == "none" {
        "ascii"
    } else if console.is_terminal() && sixel {
        "sixel"
    } else {
        "blocks"
    };
    // Match SystemPager's selection order, omitting its arguments: these can
    // carry private data and are unnecessary to identify the chosen program.
    let (pager_source, pager_program) = ["MANPAGER", "PAGER"]
        .iter()
        .find_map(|name| {
            std::env::var(name).ok().and_then(|value| {
                value
                    .split_whitespace()
                    .next()
                    .map(|program| ((*name).to_owned(), program.to_owned()))
            })
        })
        .unwrap_or_else(|| {
            (
                "platform default".into(),
                if cfg!(windows) { "more.com" } else { "less" }.into(),
            )
        });
    let pager_eligible = std::io::stdin().is_terminal()
        && console.is_terminal()
        && !matches!(
            std::env::var("TERM").unwrap_or_default().as_str(),
            "dumb" | "emacs"
        );
    let capabilities_json = serde_json::to_value(&capabilities).map_err(|e| e.to_string())?;
    let registry = rich_ext::ExtensionRegistry::with_defaults();
    let plugins: Vec<serde_json::Value> = registry
        .plugins()
        .iter()
        .map(|plugin| {
            serde_json::json!({
                "id": plugin.metadata.id,
                "name": plugin.metadata.name,
                "version": plugin.metadata.version,
                "api_version": plugin.metadata.api_version,
                "capabilities": plugin
                    .capabilities
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    let json = serde_json::json!({
        "package": {"name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION")},
        "features": {"art": cfg!(feature="art"), "fetch": cfg!(feature="fetch"), "syntax-cache": cfg!(feature="syntax-cache"), "onig": cfg!(feature="onig"), "json-escape-safe": cfg!(feature="json-escape-safe")},
        "terminal": {"stdout_tty": console.is_terminal(), "width": console.width(), "height": console.height(), "color": color, "no_color": no_color, "detection": "local terminal and environment; no probe", "provenance": provenance},
        "image": {"requested_mode": requested_mode, "selected_mode": selected_mode, "sixel_inferred": sixel, "detection": "inferred from environment; no probe"},
        "config": {"source": config["source"], "profile": config["profile"], "disabled": config["disabled"]},
        "pager": {"source": pager_source, "program": pager_program, "availability": "not checked", "terminal_eligible": pager_eligible, "explicit": settings["pager"].as_bool().unwrap_or(false), "automatic": settings["auto_pager"].as_bool().unwrap_or(false)},
        "plugins": {"api_version": rich_ext::plugin::PLUGIN_API_VERSION, "registered": plugins},
        "capabilities": capabilities_json
    });
    Ok((json, capabilities, no_color))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_obeys_option_values_and_first_operand() {
        for (args, expected) in [
            (vec!["doctor"], true),
            (vec!["--config", "settings.toml", "doctor"], true),
            (vec!["--title", "doctor", "hello"], false),
            (vec!["hello", "doctor"], false),
            (vec!["--", "doctor"], false),
            (vec!["--demo-section", "doctor"], false),
            (vec!["--title", "--", "doctor"], true),
        ] {
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(requested(&args), expected, "{args:?}");
        }
    }
}
