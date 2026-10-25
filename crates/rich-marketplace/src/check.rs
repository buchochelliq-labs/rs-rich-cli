//! Checking a downloaded artifact by kind, after its size and SHA-256.
//!
//! The built-in checks need nothing heavy: a WASM module's header, a micro
//! pack's `pack.json` read from its archive, and a theme parsed with core's
//! `Theme::from_file`. Loading a module in the sandbox needs rs-rich-ext's
//! `wasm-plugins` feature (the wasmi interpreter), so a host that has it
//! supplies an [`ArtifactChecker`] whose
//! [`check_wasm`](ArtifactChecker::check_wasm) runs `load_wasm` and compares
//! the manifest with [`compare_wasm_manifest`].

use std::io::{Cursor, Read};

use rich_plugin_api::abi::PluginAbi;
use serde_json::Value;

use crate::error::CheckError;
use crate::listing::Capability;
use crate::util::parse_json;

/// What the listing says a WASM module is.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct WasmExpectation<'a> {
    /// The listing's id: the manifest's `name`.
    pub id: &'a str,
    /// The version: the manifest's `version`.
    pub version: &'a str,
    /// `requires.abi`.
    pub abi: (u32, u32),
    /// The version's capabilities.
    pub capabilities: &'a [Capability],
}

/// Checks an artifact by kind before it is moved into place. Every method
/// has the built-in check as its default; a host overrides what it can check
/// better.
pub trait ArtifactChecker {
    /// A WASM plugin. Built in: the module header only.
    fn check_wasm(&self, bytes: &[u8], expected: &WasmExpectation<'_>) -> Result<(), CheckError> {
        let _ = expected;
        check_wasm_header(bytes)
    }

    /// A micro pack: its `pack.json` names the id and version.
    fn check_micro_pack(&self, bytes: &[u8], id: &str, version: &str) -> Result<(), CheckError> {
        check_pack_json(bytes, id, version)
    }

    /// A theme file parses.
    fn check_theme(&self, bytes: &[u8]) -> Result<(), CheckError> {
        check_theme(bytes)
    }
}

/// The built-in checks.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuiltinChecks;

impl ArtifactChecker for BuiltinChecks {}

impl<C: ArtifactChecker + ?Sized> ArtifactChecker for &C {
    fn check_wasm(&self, bytes: &[u8], expected: &WasmExpectation<'_>) -> Result<(), CheckError> {
        (**self).check_wasm(bytes, expected)
    }

    fn check_micro_pack(&self, bytes: &[u8], id: &str, version: &str) -> Result<(), CheckError> {
        (**self).check_micro_pack(bytes, id, version)
    }

    fn check_theme(&self, bytes: &[u8]) -> Result<(), CheckError> {
        (**self).check_theme(bytes)
    }
}

/// A WebAssembly binary module, version 1: `\0asm` then `01 00 00 00`.
pub fn check_wasm_header(bytes: &[u8]) -> Result<(), CheckError> {
    if bytes.len() >= 8 && bytes[..8] == *b"\0asm\x01\x00\x00\x00" {
        Ok(())
    } else {
        Err(CheckError::NotWasm)
    }
}

/// Compare a loaded module's manifest with the listing: name, version, ABI
/// and capabilities (as sets) must be equal. For a host's
/// [`ArtifactChecker::check_wasm`].
pub fn compare_wasm_manifest(
    manifest: &PluginAbi,
    expected: &WasmExpectation<'_>,
) -> Result<(), CheckError> {
    let mismatch = |field, expected: String, found: String| {
        Err(CheckError::ManifestMismatch {
            field,
            expected,
            found,
        })
    };
    if manifest.name != expected.id {
        return mismatch("name", expected.id.into(), manifest.name.clone());
    }
    if manifest.version != expected.version {
        return mismatch("version", expected.version.into(), manifest.version.clone());
    }
    if (manifest.abi_major, manifest.abi_minor) != expected.abi {
        return mismatch(
            "ABI",
            format!("{}.{}", expected.abi.0, expected.abi.1),
            format!("{}.{}", manifest.abi_major, manifest.abi_minor),
        );
    }
    let mut listed: Vec<(String, String)> = expected
        .capabilities
        .iter()
        .map(|c| (c.kind.clone(), c.name.clone()))
        .collect();
    let mut found: Vec<(String, String)> = manifest
        .capabilities
        .iter()
        .map(|c| (c.kind.as_str().to_string(), c.name.clone()))
        .collect();
    listed.sort();
    found.sort();
    if listed != found {
        let show = |list: &[(String, String)]| {
            list.iter()
                .map(|(k, n)| format!("{k} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        return mismatch("capabilities", show(&listed), show(&found));
    }
    Ok(())
}

/// The largest `pack.json` read (micro's `Limits::default().max_manifest_bytes`).
const MAX_PACK_JSON: u64 = 64 << 10;

/// A micro pack archive whose root `pack.json` has `schema_version` 1 and
/// names `id` and `version`.
pub fn check_pack_json(bytes: &[u8], id: &str, version: &str) -> Result<(), CheckError> {
    let pack = |message: String| CheckError::Pack(message);
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| pack(format!("not a zip archive: {e}")))?;
    let mut entry = archive
        .by_name("pack.json")
        .map_err(|_| pack("no pack.json at the archive's root".into()))?;
    if entry.size() > MAX_PACK_JSON {
        return Err(pack(format!(
            "pack.json is larger than {MAX_PACK_JSON} bytes"
        )));
    }
    let mut text = Vec::new();
    (&mut entry)
        .take(MAX_PACK_JSON + 1)
        .read_to_end(&mut text)
        .map_err(|e| pack(format!("cannot read pack.json: {e}")))?;
    if text.len() as u64 > MAX_PACK_JSON {
        return Err(pack(format!(
            "pack.json is larger than {MAX_PACK_JSON} bytes"
        )));
    }
    let value = parse_json(&text).map_err(|e| pack(format!("pack.json: {e}")))?;
    let field = |key: &str| value.get(key);
    if field("schema_version").and_then(Value::as_u64) != Some(crate::resolve::MICRO_SCHEMA_VERSION)
    {
        return Err(pack(
            "pack.json: unsupported or missing schema_version".into(),
        ));
    }
    let name = field("name").and_then(Value::as_str).unwrap_or_default();
    if name != id {
        return Err(CheckError::ManifestMismatch {
            field: "pack name",
            expected: id.into(),
            found: name.into(),
        });
    }
    let found = field("version").and_then(Value::as_str).unwrap_or_default();
    if found != version {
        return Err(CheckError::ManifestMismatch {
            field: "pack version",
            expected: version.into(),
            found: found.into(),
        });
    }
    Ok(())
}

/// A UTF-8 `[styles]` theme file that core's `Theme::from_file` reads.
pub fn check_theme(bytes: &[u8]) -> Result<(), CheckError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CheckError::Theme("not UTF-8".into()))?;
    rich::theme::Theme::from_file(text, true)
        .map(|_| ())
        .map_err(|e| CheckError::Theme(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich_plugin_api::abi::{AbiCapability, CapabilityKind};

    #[test]
    fn wasm_header() {
        assert!(check_wasm_header(b"\0asm\x01\0\0\0").is_ok());
        assert_eq!(
            check_wasm_header(b"\0asm\x02\0\0\0"),
            Err(CheckError::NotWasm)
        );
        assert_eq!(check_wasm_header(b"MZ"), Err(CheckError::NotWasm));
    }

    #[test]
    fn manifest_comparison() {
        let mut abi = PluginAbi::new("shout", "0.2.0");
        abi.capabilities.push(AbiCapability {
            kind: CapabilityKind::Transform,
            name: "upper".into(),
        });
        let capabilities = [Capability {
            kind: "transform".into(),
            name: "upper".into(),
        }];
        let expected = WasmExpectation {
            id: "shout",
            version: "0.2.0",
            abi: (abi.abi_major, abi.abi_minor),
            capabilities: &capabilities,
        };
        assert!(compare_wasm_manifest(&abi, &expected).is_ok());
        let other = WasmExpectation {
            version: "0.2.1",
            ..expected
        };
        assert!(matches!(
            compare_wasm_manifest(&abi, &other),
            Err(CheckError::ManifestMismatch {
                field: "version",
                ..
            })
        ));
        let none = WasmExpectation {
            capabilities: &[],
            ..expected
        };
        assert!(matches!(
            compare_wasm_manifest(&abi, &none),
            Err(CheckError::ManifestMismatch {
                field: "capabilities",
                ..
            })
        ));
    }

    #[test]
    fn themes() {
        assert!(check_theme(b"[styles]\nwarning = bold red\n").is_ok());
        assert!(check_theme(b"no section here\n").is_err());
        assert!(check_theme(&[0xff, 0xfe]).is_err());
    }
}
