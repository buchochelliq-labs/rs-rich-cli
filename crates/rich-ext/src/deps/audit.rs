//! Security advisories (#330, #418): a generic advisory model, read from
//! `cargo audit --json`, and a report grouped by severity.
//!
//! [`Advisory`] is one finding about one package: a vulnerability, or a
//! warning that it is unmaintained, unsound or yanked. It says nothing of
//! where it came from, so a plugin reading another scanner's output builds
//! the same values and reuses [`AdvisoryReport`]; [`AdvisoryReport::from_cargo_audit`]
//! reads `cargo audit --json` (cargo-audit 0.14 and later write warnings as
//! a table by kind; the older list is read too).
//!
//! `cargo audit` gives a CVSS vector rather than a severity. A CVSS 3.x
//! vector is scored here with the specification's base-score formula
//! ([`cvss3_score`]) and banded as CVSS does (0.1–3.9 low, 4.0–6.9 medium,
//! 7.0–8.9 high, 9.0 and up critical); a vulnerability with no vector, or a
//! CVSS 4 one, is [`Severity::Unknown`], and a warning without one is
//! [`Severity::Informational`].
//!
//! Nothing here runs `cargo audit` or reaches the network: the report reads
//! the file it was given.
//!
//! ```
//! use rich_ext::deps::audit::{AdvisoryKind, AdvisoryReport, Severity};
//!
//! let json = r#"{
//!   "lockfile": {"dependency-count": 42},
//!   "vulnerabilities": {"found": true, "count": 1, "list": [{
//!     "advisory": {"id": "RUSTSEC-2020-0071", "package": "time",
//!       "title": "Potential segfault in the time crate",
//!       "cvss": "CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:N/I:N/A:H"},
//!     "versions": {"patched": [">=0.2.23"], "unaffected": ["=0.2.0"]},
//!     "package": {"name": "time", "version": "0.1.45"}
//!   }]},
//!   "warnings": {}
//! }"#;
//! let report = AdvisoryReport::from_cargo_audit(json).unwrap();
//! let advisory = &report.advisories()[0];
//! assert_eq!(advisory.kind, AdvisoryKind::Vulnerability);
//! assert_eq!(advisory.score, Some(5.9));
//! assert_eq!(advisory.severity, Severity::Medium);
//! assert_eq!(report.vulnerabilities(), 1);
//! ```

use std::collections::BTreeMap;
use std::fmt;

use rich::cells::cell_len;
use rich::table::Table;
use rich::{ColumnOptions, Console, ConsoleOptions, Renderable, Segment, Style, Text};
use serde_json::Value;

use super::{check_count, check_size, clean, stack, stack_measure, theme_style, DepsError};

/// What an advisory is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdvisoryKind {
    /// A security vulnerability.
    Vulnerability,
    /// Code that can cause undefined behaviour through a safe API.
    Unsound,
    /// No longer maintained.
    Unmaintained,
    /// The version in use was yanked from its registry.
    Yanked,
    /// Any other informational advisory (`notice`, …).
    Notice,
}

impl AdvisoryKind {
    /// The word for it: `vulnerability`, `unsound`, ….
    pub fn name(self) -> &'static str {
        match self {
            AdvisoryKind::Vulnerability => "vulnerability",
            AdvisoryKind::Unsound => "unsound",
            AdvisoryKind::Unmaintained => "unmaintained",
            AdvisoryKind::Yanked => "yanked",
            AdvisoryKind::Notice => "notice",
        }
    }

    fn parse(name: &str) -> Self {
        match name {
            "vulnerability" => AdvisoryKind::Vulnerability,
            "unsound" => AdvisoryKind::Unsound,
            "unmaintained" => AdvisoryKind::Unmaintained,
            "yanked" => AdvisoryKind::Yanked,
            _ => AdvisoryKind::Notice,
        }
    }
}

impl fmt::Display for AdvisoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How severe an advisory is, most severe first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    /// A vulnerability whose severity is not known.
    Unknown,
    /// A warning with no score, or a score of zero.
    Informational,
}

impl Severity {
    /// Every severity, most severe first.
    pub const ALL: [Severity; 6] = [
        Severity::Critical,
        Severity::High,
        Severity::Medium,
        Severity::Low,
        Severity::Unknown,
        Severity::Informational,
    ];

    /// CVSS's band for a base score.
    pub fn from_score(score: f64) -> Self {
        match score {
            s if s >= 9.0 => Severity::Critical,
            s if s >= 7.0 => Severity::High,
            s if s >= 4.0 => Severity::Medium,
            s if s > 0.0 => Severity::Low,
            _ => Severity::Informational,
        }
    }

    /// A severity's name (`critical`, `high`, …, as most scanners write
    /// them; `moderate` reads as medium, `none` and `info` as
    /// informational).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" | "moderate" => Severity::Medium,
            "low" => Severity::Low,
            "unknown" => Severity::Unknown,
            "informational" | "info" | "none" => Severity::Informational,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::High => "high",
            Severity::Medium => "medium",
            Severity::Low => "low",
            Severity::Unknown => "unknown",
            Severity::Informational => "informational",
        }
    }

    fn style(self, console: &Console) -> Style {
        let key = match self {
            Severity::Critical => "deps.critical",
            Severity::High => "deps.high",
            Severity::Medium => "deps.medium",
            Severity::Low => "deps.low",
            Severity::Unknown => "deps.unknown",
            Severity::Informational => "deps.info",
        };
        theme_style(console, key)
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One advisory about one package. See the [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct Advisory {
    /// The advisory's id (`RUSTSEC-2020-0071`, `GHSA-…`); empty for a
    /// yanked version, which has none.
    pub id: String,
    pub kind: AdvisoryKind,
    /// The affected package's name.
    pub package: String,
    /// The version in use, which the advisory affects.
    pub version: String,
    /// Version requirements that fix it (`>=0.2.23`); empty when no fix
    /// exists.
    pub patched: Vec<String>,
    /// Version requirements it never affected.
    pub unaffected: Vec<String>,
    pub severity: Severity,
    /// The CVSS base score, when there is one.
    pub score: Option<f64>,
    pub title: String,
    pub url: Option<String>,
    /// Other ids for the same issue (`CVE-…`).
    pub aliases: Vec<String>,
    /// When it was published (`YYYY-MM-DD`).
    pub date: Option<String>,
}

impl Advisory {
    /// An advisory with no versions, score, link or aliases yet.
    pub fn new(
        id: impl Into<String>,
        kind: AdvisoryKind,
        package: impl Into<String>,
        version: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Advisory {
            id: id.into(),
            kind,
            package: package.into(),
            version: version.into(),
            patched: Vec::new(),
            unaffected: Vec::new(),
            severity: if kind == AdvisoryKind::Vulnerability {
                Severity::Unknown
            } else {
                Severity::Informational
            },
            score: None,
            title: title.into(),
            url: None,
            aliases: Vec::new(),
            date: None,
        }
    }
}

/// The CVSS 3.0 or 3.1 base score of a vector
/// (`CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H` scores 9.8), from the
/// specification's formula; `None` for another version or a vector missing
/// a base metric.
pub fn cvss3_score(vector: &str) -> Option<f64> {
    let mut parts = vector.trim().split('/');
    let version = parts.next()?;
    if version != "CVSS:3.0" && version != "CVSS:3.1" {
        return None;
    }
    let metrics: BTreeMap<&str, &str> = parts.filter_map(|part| part.split_once(':')).collect();
    let get = |key: &str| metrics.get(key).copied();
    let changed = match get("S")? {
        "U" => false,
        "C" => true,
        _ => return None,
    };
    let av = match get("AV")? {
        "N" => 0.85,
        "A" => 0.62,
        "L" => 0.55,
        "P" => 0.2,
        _ => return None,
    };
    let ac = match get("AC")? {
        "L" => 0.77,
        "H" => 0.44,
        _ => return None,
    };
    let pr = match (get("PR")?, changed) {
        ("N", _) => 0.85,
        ("L", false) => 0.62,
        ("L", true) => 0.68,
        ("H", false) => 0.27,
        ("H", true) => 0.5,
        _ => return None,
    };
    let ui = match get("UI")? {
        "N" => 0.85,
        "R" => 0.62,
        _ => return None,
    };
    let cia = |key: &str| match get(key)? {
        "H" => Some(0.56),
        "L" => Some(0.22),
        "N" => Some(0.0),
        _ => None,
    };
    let iss = 1.0 - (1.0 - cia("C")?) * (1.0 - cia("I")?) * (1.0 - cia("A")?);
    let impact = if changed {
        7.52 * (iss - 0.029) - 3.25 * (iss - 0.02f64).powi(15)
    } else {
        6.42 * iss
    };
    if impact <= 0.0 {
        return Some(0.0);
    }
    let exploitability = 8.22 * av * ac * pr * ui;
    let base = if changed {
        1.08 * (impact + exploitability)
    } else {
        impact + exploitability
    };
    Some(round_up(base.min(10.0)))
}

/// CVSS 3.1's `Roundup`: the smallest one-decimal number at or above
/// `value`, computed on integers so floating point cannot push it up.
fn round_up(value: f64) -> f64 {
    let scaled = (value * 100_000.0).round() as i64;
    if scaled % 10_000 == 0 {
        scaled as f64 / 100_000.0
    } else {
        ((scaled / 10_000) + 1) as f64 / 10.0
    }
}

/// What the report's first column says: a vulnerability's severity (and
/// score), or a warning's kind (and its severity, when it has one).
fn severity_label(advisory: &Advisory) -> String {
    let mut label = if advisory.kind == AdvisoryKind::Vulnerability {
        advisory.severity.name().to_string()
    } else if advisory.severity == Severity::Informational {
        advisory.kind.name().to_string()
    } else {
        format!("{} {}", advisory.kind.name(), advisory.severity.name())
    };
    if let Some(score) = advisory.score {
        label.push_str(&format!(" {score:.1}"));
    }
    label
}

/// Advisories, with the number of dependencies they were checked against.
/// See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AdvisoryReport {
    advisories: Vec<Advisory>,
    dependency_count: Option<u64>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// One `cargo audit` entry: a vulnerability or a warning.
fn entry(value: &Value, kind: Option<AdvisoryKind>) -> Result<Advisory, DepsError> {
    let package = value
        .get("package")
        .ok_or_else(|| DepsError::new("a cargo audit entry has no package"))?;
    let (Some(name), Some(version)) = (
        package.get("name").and_then(Value::as_str),
        package.get("version").and_then(Value::as_str),
    ) else {
        return Err(DepsError::new(
            "a cargo audit entry's package has no name or version",
        ));
    };
    let advisory = value.get("advisory").filter(|a| !a.is_null());
    let text = |key: &str| {
        advisory
            .and_then(|a| a.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let kind = kind.unwrap_or_else(|| {
        value
            .get("kind")
            .and_then(Value::as_str)
            .or_else(|| {
                advisory
                    .and_then(|a| a.get("informational"))
                    .and_then(Value::as_str)
            })
            .map_or(AdvisoryKind::Notice, AdvisoryKind::parse)
    });
    let id = text("id").unwrap_or_default();
    let title = text("title").unwrap_or_else(|| match kind {
        AdvisoryKind::Yanked => "this version was yanked from its registry".into(),
        _ => String::new(),
    });
    let mut found = Advisory::new(id, kind, name, version, title);
    found.url = text("url").or_else(|| {
        found
            .id
            .starts_with("RUSTSEC-")
            .then(|| format!("https://rustsec.org/advisories/{}", found.id))
    });
    found.date = text("date");
    found.aliases = strings(advisory.and_then(|a| a.get("aliases")));
    let versions = value.get("versions").filter(|v| !v.is_null());
    found.patched = strings(versions.and_then(|v| v.get("patched")));
    found.unaffected = strings(versions.and_then(|v| v.get("unaffected")));
    if let Some(severity) = text("severity").as_deref().and_then(Severity::parse) {
        found.severity = severity;
    } else if let Some(score) = text("cvss").as_deref().and_then(cvss3_score) {
        found.score = Some(score);
        found.severity = Severity::from_score(score);
    }
    Ok(found)
}

impl AdvisoryReport {
    /// A report of these advisories.
    pub fn new(advisories: Vec<Advisory>) -> Self {
        AdvisoryReport {
            advisories,
            dependency_count: None,
        }
    }

    /// Say how many dependencies were checked.
    pub fn dependency_count(mut self, count: u64) -> Self {
        self.dependency_count = Some(count);
        self
    }

    /// Read `cargo audit --json` output.
    pub fn from_cargo_audit(json: &str) -> Result<Self, DepsError> {
        check_size(json, "the cargo audit report")?;
        let value: Value = serde_json::from_str(json)
            .map_err(|e| DepsError::new(format!("not cargo audit JSON: {e}")))?;
        Self::from_cargo_audit_value(&value)
    }

    /// [`AdvisoryReport::from_cargo_audit`] for parsed JSON.
    pub fn from_cargo_audit_value(value: &Value) -> Result<Self, DepsError> {
        let vulnerabilities = value
            .get("vulnerabilities")
            .filter(|v| v.is_object())
            .ok_or_else(|| DepsError::new("not cargo audit JSON: no `vulnerabilities`"))?;
        let mut advisories = Vec::new();
        let list = vulnerabilities
            .get("list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        check_count(list.len(), "advisories")?;
        for item in list {
            advisories.push(entry(item, Some(AdvisoryKind::Vulnerability))?);
        }
        match value.get("warnings") {
            Some(Value::Object(by_kind)) => {
                for (kind, list) in by_kind {
                    let list = list.as_array().ok_or_else(|| {
                        DepsError::new(format!("cargo audit warnings {kind:?} are not a list"))
                    })?;
                    check_count(advisories.len() + list.len(), "advisories")?;
                    for item in list {
                        advisories.push(entry(item, Some(AdvisoryKind::parse(kind)))?);
                    }
                }
            }
            Some(Value::Array(list)) => {
                check_count(advisories.len() + list.len(), "advisories")?;
                for item in list {
                    advisories.push(entry(item, None)?);
                }
            }
            Some(Value::Null) | None => {}
            Some(_) => return Err(DepsError::new("cargo audit `warnings` is not a table")),
        }
        let mut report = AdvisoryReport::new(advisories);
        report.dependency_count = value
            .pointer("/lockfile/dependency-count")
            .and_then(Value::as_u64);
        Ok(report)
    }

    /// The advisories, in the order they were read.
    pub fn advisories(&self) -> &[Advisory] {
        &self.advisories
    }

    /// How many are vulnerabilities (the rest are warnings).
    pub fn vulnerabilities(&self) -> usize {
        self.advisories
            .iter()
            .filter(|a| a.kind == AdvisoryKind::Vulnerability)
            .count()
    }

    /// How many there are of each severity (only those with any).
    pub fn counts(&self) -> BTreeMap<Severity, usize> {
        let mut counts = BTreeMap::new();
        for advisory in &self.advisories {
            *counts.entry(advisory.severity).or_insert(0) += 1;
        }
        counts
    }

    /// The advisories by severity (most severe first), then kind, package
    /// and id.
    pub fn sorted(&self) -> Vec<&Advisory> {
        let mut sorted: Vec<&Advisory> = self.advisories.iter().collect();
        sorted.sort_by(|a, b| {
            (a.severity, a.kind, &a.package, &a.version, &a.id)
                .cmp(&(b.severity, b.kind, &b.package, &b.version, &b.id))
        });
        sorted
    }

    fn heading(&self, console: &Console) -> Text {
        let vulnerabilities = self.vulnerabilities();
        let warnings = self.advisories.len() - vulnerabilities;
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let mut line = format!(
            "{}, {}",
            plural(vulnerabilities, "vulnerability", "vulnerabilities"),
            plural(warnings, "warning", "warnings")
        );
        if let Some(count) = self.dependency_count {
            line.push_str(&format!(" in {count} dependencies"));
        }
        let style = if vulnerabilities > 0 {
            theme_style(console, "deps.critical")
        } else {
            theme_style(console, "deps.root")
        };
        let mut text = Text::styled(line, style);
        let counts = self.counts();
        if !counts.is_empty() {
            text.append("\n", None);
            for (index, (severity, count)) in counts.iter().enumerate() {
                if index > 0 {
                    text.append(" · ", Some(theme_style(console, "deps.section").into()));
                }
                text.append(
                    &format!("{count} {severity}"),
                    Some(severity.style(console).into()),
                );
            }
        }
        text
    }

    fn table(&self, console: &Console) -> Option<Table> {
        if self.advisories.is_empty() {
            return None;
        }
        let sorted = self.sorted();
        let widest = |cell: &dyn Fn(&Advisory) -> usize| sorted.iter().map(|a| cell(a)).max();
        let fixed = |width: Option<usize>| ColumnOptions {
            no_wrap: true,
            min_width: width,
            ..ColumnOptions::default()
        };
        let mut table = Table::new();
        table.add_column_with(
            Text::new("Severity"),
            fixed(widest(&|a| severity_label(a).len())),
        );
        table.add_column_with(Text::new("ID"), fixed(widest(&|a| cell_len(&clean(&a.id)))));
        table.add_column("Crate");
        table.add_column("Patched");
        table.add_column("Title");
        let mut previous = None;
        for advisory in sorted {
            if previous.is_some_and(|p| p != advisory.severity) {
                table.add_section();
            }
            previous = Some(advisory.severity);
            let severity = Text::styled(severity_label(advisory), advisory.severity.style(console));
            // A link with a control in it could end the link early and
            // write the rest to the terminal: none is better.
            let id_style = match &advisory.url {
                Some(url) if !url.contains(char::is_control) => {
                    Style::default().with_link(url.clone())
                }
                _ => Style::default(),
            };
            let id = if advisory.id.is_empty() {
                Text::styled("–", theme_style(console, "deps.off"))
            } else {
                Text::styled(clean(&advisory.id).into_owned(), id_style)
            };
            let patched = if advisory.patched.is_empty() {
                Text::styled("no fix", theme_style(console, "deps.unknown"))
            } else {
                Text::new(clean(&advisory.patched.join(", ")).into_owned())
            };
            let mut krate = Text::new(clean(&advisory.package).into_owned());
            krate.append(
                &format!(" v{}", clean(&advisory.version)),
                Some(theme_style(console, "deps.version").into()),
            );
            table.add_row_text(vec![
                severity,
                id,
                krate,
                patched,
                Text::new(clean(&advisory.title).into_owned()),
            ]);
        }
        Some(table)
    }
}

impl Renderable for AdvisoryReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let heading = self.heading(console);
        match self.table(console) {
            Some(table) => stack(&[&heading, &table], console, options),
            None => heading.rich_render(console, options),
        }
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let heading = self.heading(console);
        match self.table(console) {
            Some(table) => stack_measure(&[&heading, &table], console, options),
            None => heading.measure(console, options),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cvss3_scores_match_the_specification() {
        let score = |v: &str| cvss3_score(v).unwrap();
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H"), 9.8);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:C/C:H/I:H/A:H"), 10.0);
        assert_eq!(score("CVSS:3.0/AV:L/AC:L/PR:L/UI:N/S:U/C:H/I:N/A:N"), 5.5);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:R/S:C/C:L/I:L/A:N"), 6.1);
        assert_eq!(score("CVSS:3.1/AV:N/AC:H/PR:N/UI:N/S:U/C:N/I:N/A:H"), 5.9);
        assert_eq!(score("CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:N/I:N/A:N"), 0.0);
        assert_eq!(cvss3_score("CVSS:4.0/AV:N/AC:L/AT:N/PR:N/UI:N"), None);
        assert_eq!(cvss3_score("CVSS:3.1/AV:N"), None);
        assert_eq!(cvss3_score(""), None);
    }

    #[test]
    fn severities_band_as_cvss_does() {
        assert_eq!(Severity::from_score(9.0), Severity::Critical);
        assert_eq!(Severity::from_score(8.9), Severity::High);
        assert_eq!(Severity::from_score(4.0), Severity::Medium);
        assert_eq!(Severity::from_score(0.1), Severity::Low);
        assert_eq!(Severity::from_score(0.0), Severity::Informational);
        assert_eq!(Severity::parse("Moderate"), Some(Severity::Medium));
    }

    #[test]
    fn the_old_warning_list_is_read() {
        let json = r#"{"vulnerabilities": {"found": false, "count": 0, "list": []},
            "warnings": [{"kind": "unmaintained", "package": {"name": "a", "version": "1.0.0"},
              "advisory": {"id": "RUSTSEC-2021-0001", "title": "a is unmaintained"}}]}"#;
        let report = AdvisoryReport::from_cargo_audit(json).unwrap();
        assert_eq!(report.vulnerabilities(), 0);
        let a = &report.advisories()[0];
        assert_eq!(a.kind, AdvisoryKind::Unmaintained);
        assert_eq!(a.severity, Severity::Informational);
        assert_eq!(
            a.url.as_deref(),
            Some("https://rustsec.org/advisories/RUSTSEC-2021-0001")
        );
    }

    #[test]
    fn malformed_reports_are_errors() {
        for bad in [
            "",
            "[]",
            "{}",
            r#"{"vulnerabilities": {"list": [{}]}}"#,
            r#"{"vulnerabilities": {"list": [{"package": {"name": "a"}}]}}"#,
            r#"{"vulnerabilities": {}, "warnings": {"yanked": 3}}"#,
            r#"{"vulnerabilities": {}, "warnings": 3}"#,
        ] {
            assert!(AdvisoryReport::from_cargo_audit(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_empty_report_is_one_line() {
        let report = AdvisoryReport::new(Vec::new()).dependency_count(12);
        let console = Console::builder().width(60).color_system(None).build();
        assert_eq!(
            console.render_to_string(&report),
            "0 vulnerabilities, 0 warnings in 12 dependencies"
        );
    }
}
