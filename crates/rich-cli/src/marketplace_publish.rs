//! `rich plugin publish FILE --url URL`: check a package locally and write
//! its listing, to be submitted as a pull request to the marketplace
//! repository. No network: the artifact is read from FILE, and URL is where
//! its author will upload it.
//!
//! The package is checked as an install would check it: a WASM module is
//! loaded in the sandbox and its manifest gives the id, version, ABI and
//! capabilities; a micro pack is read with micro's default limits and its
//! `pack.json` gives the id, version and assets; a theme must parse. The
//! listing written is read back with rs-rich-marketplace's own parser, so it
//! passes the rules the marketplace's CI applies.
use super::*;

use rich_marketplace::semver::Version;
use rich_marketplace::{Kind, Listing};
use sha2::{Digest, Sha256};

/// What the package says about itself.
struct Described {
    id: String,
    version: String,
    description: Option<String>,
    requires: Value,
    capabilities: Vec<(String, String)>,
    permissions: Vec<String>,
}

fn option<'a>(options: &'a Options, name: &str) -> Option<&'a str> {
    options
        .publish
        .get(name)
        .and_then(|values| values.last())
        .map(String::as_str)
}

fn options_of<'a>(options: &'a Options, name: &str) -> &'a [String] {
    options.publish.get(name).map_or(&[], Vec::as_slice)
}

pub(super) fn run(options: &Options) -> Outcome<Done> {
    let file = match options.positionals.as_slice() {
        [file] => fs_path(file),
        [] => return Err(usage("rich plugin publish needs a package FILE")),
        _ => return Err(usage("rich plugin publish takes one package FILE")),
    };
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let kind =
        match extension.as_str() {
            "wasm" => Kind::WasmPlugin,
            "zip" | "richmicro" => Kind::MicroPack,
            "ini" | "theme" => Kind::Theme,
            "crate" => return Err(usage(
                "a crate is listed from crates.io, not published from a file; write its listing \
                 by hand (docs/design/marketplace.md shows one)",
            )),
            _ => {
                return Err(usage(format!(
                "{}: expected a WASM plugin (.wasm), a micro pack (.zip, .richmicro) or a theme \
                 (.ini, .theme)",
                file.display()
            )))
            }
        };
    let limit = kind.max_artifact_bytes().unwrap_or(u64::MAX);
    let bytes = read_bounded(&file, limit)
        .map_err(|error| match error {
            FetchError::TooLarge { limit, .. } => data(format!(
                "{} is larger than the {limit} bytes a {kind} may be",
                file.display()
            )),
            other => input(other.to_string()),
        })?
        .ok_or_else(|| input(format!("{}: no such file", file.display())))?;
    let url = option(options, "url").ok_or_else(|| {
        usage("rich plugin publish needs --url URL, where the file will be downloaded from")
    })?;
    rich_marketplace::require_https(url).map_err(|e| usage(e.to_string()))?;
    let path_part = url.split(['?', '#']).next().unwrap_or(url);
    if !path_part
        .to_ascii_lowercase()
        .ends_with(&format!(".{extension}"))
    {
        return Err(usage(format!(
            "--url must name the file itself, ending in .{extension}"
        )));
    }

    let described = describe(kind, &file, &bytes, options)?;
    for (flag, value) in [("id", &described.id), ("version", &described.version)] {
        if let Some(given) = option(options, flag) {
            if given != value {
                return Err(data(format!(
                    "--{flag} {given:?}, but the package says {value:?}"
                )));
            }
        }
    }
    let number = Version::parse(&described.version).map_err(|e| {
        data(format!(
            "the package's version {:?} is not SemVer: {e}",
            described.version
        ))
    })?;
    let sha256: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    // The listing: an existing one gains the version, or a new one is made.
    let existing = option(options, "listing").map(fs_path);
    let mut listing = match &existing {
        Some(path) => {
            let text =
                std::fs::read(path).map_err(|e| input(format!("{}: {e}", path.display())))?;
            Listing::parse(&text).map_err(|e| data(format!("{}: {e}", path.display())))?;
            let value: Value = serde_json::from_slice(&text)
                .map_err(|e| data(format!("{}: {e}", path.display())))?;
            if value["id"] != json!(described.id) || value["kind"] != json!(kind.as_str()) {
                return Err(data(format!(
                    "{} lists {} {}, not the {} {}",
                    path.display(),
                    value["kind"].as_str().unwrap_or("?"),
                    value["id"].as_str().unwrap_or("?"),
                    kind,
                    described.id
                )));
            }
            value
        }
        None => {
            let need = |name: &str| {
                option(options, name).map(str::to_string).ok_or_else(|| {
                    usage(format!(
                        "a new listing needs --{name}; or pass --listing FILE to add a version to \
                         an existing one"
                    ))
                })
            };
            let description = match option(options, "description") {
                Some(text) => text.to_string(),
                None => described
                    .description
                    .clone()
                    .filter(|d| !d.is_empty())
                    .ok_or_else(|| usage("a new listing needs --description"))?,
            };
            json!({
                "schema_version": rich_marketplace::LISTING_SCHEMA_VERSION,
                "id": described.id,
                "kind": kind.as_str(),
                "name": option(options, "name").unwrap_or(&described.id),
                "description": description,
                "tier": option(options, "tier").unwrap_or("community"),
                "license": need("license")?,
                "repository": need("repository")?,
                "versions": [],
            })
        }
    };
    // Metadata given on the command line replaces what was there.
    for key in [
        "name",
        "description",
        "tier",
        "license",
        "repository",
        "homepage",
    ] {
        if let Some(value) = option(options, key) {
            listing[key] = json!(value);
        }
    }
    for (key, flag) in [("authors", "author"), ("keywords", "keyword")] {
        let values = options_of(options, flag);
        if !values.is_empty() {
            listing[key] = json!(values);
        }
    }

    let mut version = json!({
        "version": described.version,
        "published": option(options, "published").map_or_else(today, str::to_string),
        "requires": described.requires,
        "artifact": {"url": url, "sha256": sha256, "size": bytes.len()},
    });
    match option(options, "source-revision") {
        Some(revision) => version["source_revision"] = json!(revision),
        None if kind == Kind::WasmPlugin => {
            return Err(usage(
                "a WASM plugin's listing needs --source-revision COMMIT, the full Git commit it \
                 was built from",
            ))
        }
        None => {}
    }
    version["capabilities"] = json!(described
        .capabilities
        .iter()
        .map(|(kind, name)| json!({"kind": kind, "name": name}))
        .collect::<Vec<_>>());
    version["permissions"] = json!(described.permissions);

    let versions = listing["versions"]
        .as_array_mut()
        .ok_or_else(|| data("the listing's versions is not an array"))?;
    if versions
        .iter()
        .any(|v| v["version"].as_str() == Some(described.version.as_str()))
    {
        return Err(data(format!(
            "the listing already has version {}; a listed version never changes, so publish a \
             new one",
            described.version
        )));
    }
    versions.push(version);
    // Newest first, by SemVer precedence.
    versions.sort_by(|a, b| {
        let parse = |v: &Value| v["version"].as_str().and_then(|s| Version::parse(s).ok());
        parse(b).cmp(&parse(a))
    });

    let mut text = serde_json::to_string_pretty(&listing).unwrap_or_default();
    text.push('\n');
    Listing::parse(text.as_bytes()).map_err(|e| data(format!("the listing is not valid: {e}")))?;
    let output = option(options, "output")
        .map(fs_path)
        .or(existing)
        .unwrap_or_else(|| PathBuf::from("packages").join(format!("{}.json", described.id)));
    write_atomically(&output, text.as_bytes())
        .map_err(|e| input(format!("{}: {e}", output.display())))?;
    let summary = format!(
        "Wrote {}: {} {} ({kind}), {} bytes, SHA-256 {sha256}.\n\
         Upload {} to {url}, then open a pull request with the listing to the marketplace \
         repository.\n",
        output.display(),
        described.id,
        number,
        bytes.len(),
        file.display(),
    );
    Ok(Done::new(
        summary,
        json!({
            "listing": output.display().to_string(),
            "id": described.id,
            "kind": kind.as_str(),
            "version": described.version,
            "sha256": sha256,
            "size": bytes.len(),
            "url": url,
        }),
    ))
}

fn describe(kind: Kind, file: &Path, bytes: &[u8], options: &Options) -> Outcome<Described> {
    match kind {
        Kind::WasmPlugin => describe_wasm(file, bytes),
        Kind::MicroPack => describe_pack(file, bytes),
        _ => {
            rich_marketplace::check_theme(bytes)
                .map_err(|e| data(format!("{}: {e}", file.display())))?;
            let id = option(options, "id")
                .map(str::to_string)
                .or_else(|| {
                    file.file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_ascii_lowercase)
                })
                .unwrap_or_default();
            if !rich_ext::plugin::is_valid_name(&id) {
                return Err(usage(format!(
                    "{id:?} is not a valid package id; pass --id (lowercase letters, digits, -, _ \
                     and .)"
                )));
            }
            let version = option(options, "version")
                .ok_or_else(|| usage("a theme has no version of its own; pass --version"))?
                .to_string();
            Ok(Described {
                capabilities: vec![("theme".into(), id.clone())],
                id,
                version,
                description: None,
                requires: json!({"theme_format": rich_marketplace::THEME_FORMAT}),
                permissions: Vec::new(),
            })
        }
    }
}

#[cfg(feature = "wasm-plugins")]
fn describe_wasm(file: &Path, bytes: &[u8]) -> Outcome<Described> {
    let plugin = rich_ext::plugin_loading::load_wasm_bytes(
        bytes,
        file,
        &rich_ext::plugin_loading::WasmLimits::default(),
    )
    .map_err(|e| data(e.to_string()))?;
    let abi = plugin.abi();
    let capabilities: Vec<(String, String)> = abi
        .capabilities
        .iter()
        .map(|c| (c.kind.as_str().to_string(), c.name.clone()))
        .collect();
    let permissions = if capabilities.iter().any(|(kind, _)| kind == "fence-ansi") {
        vec!["ansi-output".to_string()]
    } else {
        Vec::new()
    };
    Ok(Described {
        id: abi.name.clone(),
        version: abi.version.clone(),
        description: Some(abi.description.clone()),
        requires: json!({"abi": format!("{}.{}", abi.abi_major, abi.abi_minor)}),
        capabilities,
        permissions,
    })
}

#[cfg(not(feature = "wasm-plugins"))]
fn describe_wasm(_: &Path, _: &[u8]) -> Outcome<Described> {
    Err(usage(
        "publishing a WASM plugin loads it in the sandbox, which needs a build with the \
         wasm-plugins feature",
    ))
}

#[cfg(feature = "art")]
fn describe_pack(file: &Path, bytes: &[u8]) -> Outcome<Described> {
    use rich_micro::model::Layer;
    use rich_micro::package::{load_pack, Limits};
    let pack = load_pack(file, Layer::User, &Limits::default())
        .map_err(|e| data(format!("{}: {e}", file.display())))?;
    if let Some((package, why)) = pack.rejected.first() {
        return Err(data(format!(
            "{}: package {package} is refused: {why}",
            file.display()
        )));
    }
    let version = pack
        .version
        .clone()
        .ok_or_else(|| data(format!("{}: pack.json has no version", file.display())))?;
    rich_marketplace::check_pack_json(bytes, &pack.name, &version)
        .map_err(|e| data(format!("{}: {e}", file.display())))?;
    let mut capabilities: Vec<(String, String)> = pack
        .assets
        .iter()
        .map(|asset| ("asset".to_string(), asset.name().to_string()))
        .collect();
    capabilities.sort();
    capabilities.dedup();
    if capabilities.is_empty() {
        return Err(data(format!("{}: the pack has no assets", file.display())));
    }
    Ok(Described {
        id: pack.name,
        version,
        description: None,
        requires: json!({"micro_schema": rich_marketplace::MICRO_SCHEMA_VERSION}),
        capabilities,
        permissions: Vec::new(),
    })
}

#[cfg(not(feature = "art"))]
fn describe_pack(_: &Path, _: &[u8]) -> Outcome<Described> {
    Err(usage(
        "publishing a micro pack reads it with rs-rich-micro, which needs a build with the art \
         feature",
    ))
}

/// Today's date in UTC, `YYYY-MM-DD`.
fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    civil_date(seconds.div_euclid(86_400))
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(20_746), "2026-10-20");
        assert_eq!(civil_date(11_016), "2000-02-29");
    }
}
