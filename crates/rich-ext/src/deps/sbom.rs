//! Software bills of materials (#704): the packages and licences of an SPDX
//! or CycloneDX JSON document, as a [`LicenseReport`], so the licence
//! report and the [policy](super::policy) work beyond Cargo.
//!
//! [`Sbom::parse`] tells the two apart by content:
//!
//! - **SPDX** 2.2 and 2.3 JSON (`spdxVersion`): each of `packages[]` with
//!   its `name` and `versionInfo`, and `licenseConcluded`, else
//!   `licenseDeclared` (`NOASSERTION` and `NONE` read as no licence). The
//!   package the document `DESCRIBES` is the root.
//! - **CycloneDX** 1.4 to 1.6 JSON (`bomFormat: "CycloneDX"`): each of
//!   `components[]` (nested ones too) with its `name` and `version`, and
//!   `licenses[]`: an `expression`, or a `license` with an SPDX `id` or a
//!   `name`. Several entries are read as all applying (`AND`), the
//!   stricter reading. `metadata.component` is the root.
//!
//! SPDX tag-value, SPDX 3 and XML documents are refused with an error that
//! names them.
//!
//! ```
//! use rich_ext::deps::sbom::{Sbom, SbomFormat};
//!
//! let bom = r#"{"bomFormat": "CycloneDX", "specVersion": "1.5",
//!   "metadata": {"component": {"name": "app", "version": "1.0.0",
//!     "licenses": [{"license": {"id": "MIT"}}]}},
//!   "components": [{"name": "serde", "version": "1.0.210",
//!     "licenses": [{"expression": "MIT OR Apache-2.0"}]}]}"#;
//! let sbom = Sbom::parse(bom).unwrap();
//! assert_eq!(sbom.format(), SbomFormat::CycloneDx);
//! assert_eq!(sbom.root().unwrap().name, "app");
//! assert_eq!(sbom.report().packages().len(), 2);
//! assert_eq!(
//!     sbom.report().packages()[1].license.as_deref(),
//!     Some("MIT OR Apache-2.0")
//! );
//! ```

use serde_json::Value;

use super::licenses::{LicenseReport, LicensedPackage};
use super::{check_count, check_size, clean, DepsError};

/// The deepest CycloneDX components nest before the reader stops.
pub const MAX_NESTING: usize = 32;

/// Which standard a document follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbomFormat {
    Spdx,
    CycloneDx,
}

impl SbomFormat {
    pub fn name(self) -> &'static str {
        match self {
            SbomFormat::Spdx => "SPDX",
            SbomFormat::CycloneDx => "CycloneDX",
        }
    }
}

/// A bill of materials read. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sbom {
    format: SbomFormat,
    version: String,
    root: Option<LicensedPackage>,
    report: LicenseReport,
}

impl Sbom {
    /// An SPDX or CycloneDX JSON document.
    pub fn parse(json: &str) -> Result<Self, DepsError> {
        check_size(json, "the SBOM")?;
        let trimmed = json.trim_start_matches('\u{feff}').trim_start();
        if trimmed.starts_with('<') {
            return Err(DepsError::new(
                "an XML document: give the SBOM as SPDX or CycloneDX JSON",
            ));
        }
        if trimmed.starts_with("SPDXVersion:") {
            return Err(DepsError::new(
                "an SPDX tag-value document: give the SBOM as SPDX JSON",
            ));
        }
        let value: Value = serde_json::from_str(trimmed)
            .map_err(|e| DepsError::new(format!("not SBOM JSON: {e}")))?;
        if value.get("spdxVersion").is_some() {
            return Self::from_spdx(&value);
        }
        let cyclonedx = value.get("bomFormat").and_then(Value::as_str) == Some("CycloneDX")
            || (value.get("specVersion").is_some() && value.get("components").is_some());
        if cyclonedx {
            return Self::from_cyclonedx(&value);
        }
        if value.get("@context").is_some() {
            return Err(DepsError::new(
                "an SPDX 3 (JSON-LD) document: give the SBOM as SPDX 2.2 or 2.3 JSON",
            ));
        }
        Err(DepsError::new(
            "not an SPDX or CycloneDX JSON document (no `spdxVersion` or `bomFormat`)",
        ))
    }

    /// An SPDX 2.2 or 2.3 JSON document, parsed.
    pub fn from_spdx(value: &Value) -> Result<Self, DepsError> {
        let version = value
            .get("spdxVersion")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !version.starts_with("SPDX-2.") {
            return Err(DepsError::new(format!(
                "SPDX version {:?}: this reads SPDX 2.2 and 2.3 JSON",
                clean(&version)
            )));
        }
        let packages = match value.get("packages") {
            None => &[][..],
            Some(Value::Array(list)) => list.as_slice(),
            Some(_) => return Err(DepsError::new("SPDX: `packages` is not a list")),
        };
        check_count(packages.len(), "packages")?;
        let mut read = Vec::with_capacity(packages.len());
        for (index, package) in packages.iter().enumerate() {
            let text = |key: &str| package.get(key).and_then(Value::as_str);
            let Some(name) = text("name").map(str::trim).filter(|n| !n.is_empty()) else {
                return Err(DepsError::new(format!(
                    "SPDX: packages[{index}] has no name"
                )));
            };
            let version = text("versionInfo").unwrap_or_default().trim().to_string();
            let id = text("SPDXID")
                .map(str::to_string)
                .unwrap_or_else(|| format!("{name}@{version}"));
            let license = [text("licenseConcluded"), text("licenseDeclared")]
                .into_iter()
                .flatten()
                .map(str::trim)
                .find(|l| !l.is_empty() && *l != "NOASSERTION" && *l != "NONE")
                .map(str::to_string);
            read.push(LicensedPackage {
                id,
                name: name.to_string(),
                version,
                license,
                license_file: None,
            });
        }
        // The root: what the document describes.
        let described: Option<String> = value
            .get("documentDescribes")
            .and_then(Value::as_array)
            .and_then(|ids| ids.first())
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                let document = value.get("SPDXID").and_then(Value::as_str)?;
                value
                    .get("relationships")
                    .and_then(Value::as_array)?
                    .iter()
                    .find(|r| {
                        r.get("spdxElementId").and_then(Value::as_str) == Some(document)
                            && r.get("relationshipType").and_then(Value::as_str)
                                == Some("DESCRIBES")
                    })
                    .and_then(|r| r.get("relatedSpdxElement").and_then(Value::as_str))
                    .map(str::to_string)
            });
        let root = described.and_then(|id| read.iter().find(|p| p.id == id).cloned());
        Ok(Sbom {
            format: SbomFormat::Spdx,
            version: version.trim_start_matches("SPDX-").to_string(),
            root,
            report: LicenseReport::new(read),
        })
    }

    /// A CycloneDX 1.4 to 1.6 JSON document, parsed.
    pub fn from_cyclonedx(value: &Value) -> Result<Self, DepsError> {
        let version = value
            .get("specVersion")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut read = Vec::new();
        let root = match value.pointer("/metadata/component") {
            Some(component) if component.is_object() => {
                let root = component_package(component, "metadata.component")?;
                read.push(root.clone());
                Some(root)
            }
            _ => None,
        };
        let mut stack: Vec<(&Value, String, usize)> = Vec::new();
        match value.get("components") {
            None => {}
            Some(Value::Array(list)) => {
                for (index, component) in list.iter().enumerate().rev() {
                    stack.push((component, format!("components[{index}]"), 0));
                }
            }
            Some(_) => return Err(DepsError::new("CycloneDX: `components` is not a list")),
        }
        while let Some((component, at, depth)) = stack.pop() {
            check_count(read.len() + 1, "components")?;
            read.push(component_package(component, &at)?);
            if let Some(nested) = component.get("components").and_then(Value::as_array) {
                if depth + 1 >= MAX_NESTING {
                    return Err(DepsError::new(format!(
                        "CycloneDX: components nest deeper than {MAX_NESTING} levels"
                    )));
                }
                for (index, child) in nested.iter().enumerate().rev() {
                    stack.push((child, format!("{at}.components[{index}]"), depth + 1));
                }
            }
        }
        Ok(Sbom {
            format: SbomFormat::CycloneDx,
            version,
            root,
            report: LicenseReport::new(read),
        })
    }

    pub fn format(&self) -> SbomFormat {
        self.format
    }

    /// The specification version the document gives: `2.3`, `1.5`.
    pub fn spec_version(&self) -> &str {
        &self.version
    }

    /// The package the document describes, when it says.
    pub fn root(&self) -> Option<&LicensedPackage> {
        self.root.as_ref()
    }

    /// Every package, the root among them.
    pub fn report(&self) -> &LicenseReport {
        &self.report
    }

    pub fn into_report(self) -> LicenseReport {
        self.report
    }
}

/// A CycloneDX component as a package.
fn component_package(component: &Value, at: &str) -> Result<LicensedPackage, DepsError> {
    let text = |key: &str| component.get(key).and_then(Value::as_str);
    let Some(name) = text("name").map(str::trim).filter(|n| !n.is_empty()) else {
        return Err(DepsError::new(format!("CycloneDX: {at} has no name")));
    };
    let version = text("version").unwrap_or_default().trim().to_string();
    let id = text("bom-ref")
        .or_else(|| text("purl"))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{name}@{version}"));
    let mut parts: Vec<String> = Vec::new();
    if let Some(licenses) = component.get("licenses").and_then(Value::as_array) {
        for entry in licenses {
            let part = entry
                .get("expression")
                .and_then(Value::as_str)
                .or_else(|| entry.pointer("/license/id").and_then(Value::as_str))
                .or_else(|| entry.pointer("/license/name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|part| !part.is_empty());
            if let Some(part) = part {
                parts.push(part.to_string());
            }
        }
    }
    let license = match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(
            parts
                .iter()
                .map(|part| {
                    if part.contains(' ') {
                        format!("({part})")
                    } else {
                        part.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
    };
    Ok(LicensedPackage {
        id,
        name: name.to_string(),
        version,
        license,
        license_file: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spdx_reads_concluded_then_declared() {
        let json = r#"{"spdxVersion": "SPDX-2.3", "SPDXID": "SPDXRef-DOCUMENT",
          "packages": [
            {"SPDXID": "SPDXRef-app", "name": "app", "versionInfo": "0.1.0",
             "licenseConcluded": "NOASSERTION", "licenseDeclared": "MIT"},
            {"SPDXID": "SPDXRef-a", "name": "a", "versionInfo": "1.0.0",
             "licenseConcluded": "Apache-2.0", "licenseDeclared": "MIT"},
            {"SPDXID": "SPDXRef-b", "name": "b", "licenseConcluded": "NONE"}],
          "relationships": [{"spdxElementId": "SPDXRef-DOCUMENT",
            "relationshipType": "DESCRIBES", "relatedSpdxElement": "SPDXRef-app"}]}"#;
        let sbom = Sbom::parse(json).unwrap();
        assert_eq!(sbom.format(), SbomFormat::Spdx);
        assert_eq!(sbom.spec_version(), "2.3");
        let licenses: Vec<Option<&str>> = sbom
            .report()
            .packages()
            .iter()
            .map(|p| p.license.as_deref())
            .collect();
        assert_eq!(licenses, [Some("MIT"), Some("Apache-2.0"), None]);
        assert_eq!(sbom.root().map(|r| r.name.as_str()), Some("app"));
    }

    #[test]
    fn cyclonedx_nests_and_joins_licences() {
        let json = r#"{"bomFormat": "CycloneDX", "specVersion": "1.6", "components": [
            {"name": "a", "version": "1", "licenses": [
                {"license": {"id": "MIT"}}, {"license": {"id": "Apache-2.0"}}],
             "components": [{"name": "a-inner", "version": "1",
                "licenses": [{"license": {"name": "Custom Licence"}}]}]},
            {"name": "b", "version": "2"}]}"#;
        let sbom = Sbom::parse(json).unwrap();
        let packages = sbom.report().packages();
        let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "a-inner", "b"]);
        assert_eq!(packages[0].license.as_deref(), Some("MIT AND Apache-2.0"));
        assert_eq!(packages[1].license.as_deref(), Some("Custom Licence"));
        assert_eq!(packages[2].license, None);
        assert!(sbom.root().is_none());
    }

    #[test]
    fn other_documents_are_refused_with_a_reason() {
        for (input, expected) in [
            ("<bom/>", "XML"),
            ("SPDXVersion: SPDX-2.3\nDataLicense: CC0-1.0", "tag-value"),
            (
                r#"{"@context": "https://spdx.org/rdf/3.0.0/spdx-context.jsonld"}"#,
                "SPDX 3",
            ),
            (r#"{"spdxVersion": "SPDX-1.2"}"#, "SPDX 2.2 and 2.3"),
            (r#"{"packages": []}"#, "not an SPDX or CycloneDX"),
            (
                r#"{"spdxVersion": "SPDX-2.3", "packages": [{}]}"#,
                "has no name",
            ),
            (
                r#"{"bomFormat": "CycloneDX", "components": {}}"#,
                "not a list",
            ),
            (
                "{\"bomFormat\": \"CycloneDX\", \"components\": [",
                "not SBOM JSON",
            ),
        ] {
            let err = Sbom::parse(input).unwrap_err().to_string();
            assert!(err.contains(expected), "{input:?}: {err}");
        }
        let mut deep = String::from(r#"{"bomFormat": "CycloneDX", "components": "#);
        for _ in 0..40 {
            deep.push_str(r#"[{"name": "n", "components": "#);
        }
        deep.push_str("[]");
        for _ in 0..40 {
            deep.push_str("}]");
        }
        deep.push('}');
        assert!(Sbom::parse(&deep).is_err());
    }
}
