//! Licences (#331): every package's licence from `cargo metadata`, grouped
//! by expression, with unknown, missing and copyleft licences marked.
//!
//! A package states its licence as an SPDX expression (`license = "MIT OR
//! Apache-2.0"`) or points at a file (`license-file`). [`classify`] reads an
//! expression (`AND`, `OR`, `WITH` and parentheses; the old `/` separator
//! reads as `OR`) and decides what it asks of a user: an `OR` is as
//! permissive as its most permissive choice, an `AND` as strict as its
//! strictest part. The identifiers it knows are the common permissive ones
//! (MIT, Apache-2.0, the BSDs, ISC, Zlib, …), the weak copyleft ones (LGPL,
//! MPL, EPL, CDDL, …) and the strong ones (GPL, AGPL, EUPL, OSL, SSPL,
//! CC-BY-SA); anything else is [`LicenseClass::Unrecognised`]. This is a
//! reading aid, not legal advice.
//!
//! [`LicenseReport`] groups the packages by expression (operands of an `OR`
//! or `AND` sorted, so `Apache-2.0 OR MIT` and `MIT OR Apache-2.0` are one
//! group) and draws a table, most-used licence first.
//!
//! ```
//! use rich_ext::deps::licenses::{classify, LicenseClass};
//!
//! assert_eq!(classify("MIT OR Apache-2.0"), LicenseClass::Permissive);
//! assert_eq!(classify("MIT/Apache-2.0"), LicenseClass::Permissive);
//! assert_eq!(classify("GPL-3.0-or-later OR MIT"), LicenseClass::Permissive);
//! assert_eq!(classify("MIT AND MPL-2.0"), LicenseClass::WeakCopyleft);
//! assert_eq!(classify("GPL-2.0 WITH Classpath-exception-2.0"), LicenseClass::Copyleft);
//! assert_eq!(classify("LicenseRef-Proprietary"), LicenseClass::Unrecognised);
//! assert_eq!(classify("MIT AND ("), LicenseClass::Invalid);
//! ```

use std::collections::BTreeMap;
use std::fmt;

use rich::table::Table;
use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Text};
use serde_json::Value;

use super::{check_count, clean, compare_versions, stack, stack_measure, theme_style, DepsError};

/// The longest licence expression read, in bytes; longer is
/// [`LicenseClass::Invalid`].
pub const MAX_EXPRESSION: usize = 1024;
/// The deepest parentheses nest in an expression before it is
/// [`LicenseClass::Invalid`].
pub const MAX_NESTING: usize = 32;
/// The most crate names listed beside one licence; the rest are counted.
pub const MAX_LISTED: usize = 6;

/// What a licence expression asks of its user, least restrictive first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LicenseClass {
    /// Permissive: MIT, Apache-2.0, BSD, ….
    Permissive,
    /// File-level copyleft: LGPL, MPL, EPL, CDDL, ….
    WeakCopyleft,
    /// Strong copyleft: GPL, AGPL, EUPL, OSL, SSPL, CC-BY-SA.
    Copyleft,
    /// An identifier this module does not know (a `LicenseRef-`, a typo).
    Unrecognised,
    /// Not an expression it can read.
    Invalid,
    /// Only a `license-file`: read the file.
    File,
    /// Neither a licence nor a licence file.
    Missing,
}

impl LicenseClass {
    /// The note the report writes beside it; nothing for permissive.
    pub fn note(self) -> &'static str {
        match self {
            LicenseClass::Permissive => "",
            LicenseClass::WeakCopyleft => "weak copyleft",
            LicenseClass::Copyleft => "copyleft",
            LicenseClass::Unrecognised => "unknown licence",
            LicenseClass::Invalid => "not an SPDX expression",
            LicenseClass::File => "licence file only",
            LicenseClass::Missing => "no licence",
        }
    }

    /// Whether the report marks it as needing a look: anything but
    /// permissive.
    pub fn is_flagged(self) -> bool {
        self != LicenseClass::Permissive
    }

    fn style_key(self) -> &'static str {
        match self {
            LicenseClass::Permissive => "deps.name",
            LicenseClass::WeakCopyleft | LicenseClass::Copyleft => "deps.copyleft",
            _ => "deps.unknown",
        }
    }
}

impl fmt::Display for LicenseClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LicenseClass::Permissive => "permissive",
            other => other.note(),
        })
    }
}

const PERMISSIVE: &[&str] = &[
    "0BSD",
    "Apache-1.1",
    "Apache-2.0",
    "Artistic-2.0",
    "BlueOak-1.0.0",
    "BSD-1-Clause",
    "BSD-2-Clause",
    "BSD-2-Clause-Patent",
    "BSD-3-Clause",
    "BSL-1.0",
    "bzip2-1.0.6",
    "CC-BY-3.0",
    "CC-BY-4.0",
    "CC0-1.0",
    "CDLA-Permissive-2.0",
    "curl",
    "ICU",
    "ISC",
    "MIT",
    "MIT-0",
    "MIT-Modern-Variant",
    "NCSA",
    "OpenSSL",
    "PSF-2.0",
    "Python-2.0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Unlicense",
    "UPL-1.0",
    "W3C",
    "WTFPL",
    "X11",
    "Zlib",
    "zlib-acknowledgement",
];

/// Identifier prefixes of file-level (weak) copyleft licences.
const WEAK_COPYLEFT: &[&str] = &["LGPL-", "MPL-", "EPL-", "CDDL-", "CPL-", "MS-RL", "Ms-RL"];
/// Identifier prefixes of strong copyleft licences.
const COPYLEFT: &[&str] = &[
    "GPL-",
    "AGPL-",
    "EUPL-",
    "OSL-",
    "SSPL-",
    "CC-BY-SA-",
    "CECILL-",
];

/// Whether `id` starts with `prefix`, ignoring ASCII case.
fn starts_with_ignore_case(id: &str, prefix: &str) -> bool {
    id.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// SPDX matches identifiers without regard to case, so `gpl-3.0` is GPL.
fn identifier(id: &str) -> LicenseClass {
    let id = id.trim_end_matches('+');
    if PERMISSIVE.iter().any(|p| p.eq_ignore_ascii_case(id)) {
        LicenseClass::Permissive
    } else if WEAK_COPYLEFT.iter().any(|p| starts_with_ignore_case(id, p)) {
        LicenseClass::WeakCopyleft
    } else if COPYLEFT.iter().any(|p| starts_with_ignore_case(id, p)) {
        LicenseClass::Copyleft
    } else {
        LicenseClass::Unrecognised
    }
}

/// A parsed expression.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Expr {
    Id(String),
    With(String, String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

impl Expr {
    fn class(&self) -> LicenseClass {
        match self {
            Expr::Id(id) | Expr::With(id, _) => identifier(id),
            Expr::And(parts) => parts
                .iter()
                .map(Expr::class)
                .max()
                .unwrap_or(LicenseClass::Invalid),
            Expr::Or(parts) => parts
                .iter()
                .map(Expr::class)
                .min()
                .unwrap_or(LicenseClass::Invalid),
        }
    }

    /// The expression written back, the operands of each `AND` and `OR`
    /// sorted.
    fn canonical(&self, nested: bool) -> String {
        let join = |parts: &[Expr], op: &str| {
            let mut parts: Vec<String> = parts.iter().map(|p| p.canonical(true)).collect();
            parts.sort();
            parts.dedup();
            let joined = parts.join(op);
            if nested && parts.len() > 1 {
                format!("({joined})")
            } else {
                joined
            }
        };
        match self {
            Expr::Id(id) => id.clone(),
            Expr::With(id, exception) => format!("{id} WITH {exception}"),
            Expr::And(parts) => join(parts, " AND "),
            Expr::Or(parts) => join(parts, " OR "),
        }
    }
}

struct Parser<'a> {
    tokens: Vec<&'a str>,
    at: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn peek_op(&self, op: &str) -> bool {
        self.tokens
            .get(self.at)
            .is_some_and(|t| t.eq_ignore_ascii_case(op))
    }

    fn or(&mut self) -> Option<Expr> {
        let mut parts = vec![self.and()?];
        while self.peek_op("OR") {
            self.at += 1;
            parts.push(self.and()?);
        }
        Some(if parts.len() == 1 {
            parts.pop()?
        } else {
            Expr::Or(parts)
        })
    }

    fn and(&mut self) -> Option<Expr> {
        let mut parts = vec![self.with()?];
        while self.peek_op("AND") {
            self.at += 1;
            parts.push(self.with()?);
        }
        Some(if parts.len() == 1 {
            parts.pop()?
        } else {
            Expr::And(parts)
        })
    }

    fn with(&mut self) -> Option<Expr> {
        let atom = self.atom()?;
        if self.peek_op("WITH") {
            self.at += 1;
            let Expr::Id(id) = atom else {
                return None;
            };
            let exception = self.ident()?;
            return Some(Expr::With(id, exception.to_string()));
        }
        Some(atom)
    }

    fn ident(&mut self) -> Option<&'a str> {
        let token = *self.tokens.get(self.at)?;
        let operator = ["AND", "OR", "WITH"]
            .iter()
            .any(|op| token.eq_ignore_ascii_case(op));
        if token == "(" || token == ")" || operator {
            return None;
        }
        if !token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '+' | ':'))
        {
            return None;
        }
        self.at += 1;
        Some(token)
    }

    fn atom(&mut self) -> Option<Expr> {
        if self.tokens.get(self.at) == Some(&"(") {
            self.at += 1;
            self.depth += 1;
            if self.depth > MAX_NESTING {
                return None;
            }
            let inner = self.or()?;
            if self.tokens.get(self.at) != Some(&")") {
                return None;
            }
            self.at += 1;
            self.depth -= 1;
            return Some(inner);
        }
        self.ident().map(|id| Expr::Id(id.to_string()))
    }
}

fn parse(expression: &str) -> Option<Expr> {
    if expression.len() > MAX_EXPRESSION {
        return None;
    }
    // The old `MIT/Apache-2.0` form means `OR`.
    let spaced = expression
        .replace('/', " OR ")
        .replace('(', " ( ")
        .replace(')', " ) ");
    let mut parser = Parser {
        tokens: spaced.split_whitespace().collect(),
        at: 0,
        depth: 0,
    };
    let expr = parser.or()?;
    (parser.at == parser.tokens.len()).then_some(expr)
}

/// What a licence expression asks of its user. See the [module docs](self).
pub fn classify(expression: &str) -> LicenseClass {
    parse(expression).map_or(LicenseClass::Invalid, |expr| expr.class())
}

/// A package and what it says about its licence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LicensedPackage {
    /// Cargo's package id.
    pub id: String,
    pub name: String,
    pub version: String,
    /// The SPDX expression in `license`.
    pub license: Option<String>,
    /// The path in `license-file`.
    pub license_file: Option<String>,
}

impl LicensedPackage {
    /// Its group's name and class: the canonical expression, or a note for
    /// a file or nothing.
    pub fn license_class(&self) -> (String, LicenseClass) {
        match (&self.license, &self.license_file) {
            (Some(expression), _) => match parse(expression) {
                Some(expr) => (expr.canonical(false), expr.class()),
                None => (expression.trim().to_string(), LicenseClass::Invalid),
            },
            (None, Some(_)) => ("(licence file)".into(), LicenseClass::File),
            (None, None) => ("(none)".into(), LicenseClass::Missing),
        }
    }

    /// `name v1.2.3`.
    pub fn display(&self) -> String {
        format!("{} v{}", clean(&self.name), clean(&self.version))
    }
}

/// The packages under one licence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LicenseGroup {
    /// The expression, canonical (`(none)` and `(licence file)` for the
    /// packages without one).
    pub license: String,
    pub class: LicenseClass,
    /// Indexes into [`LicenseReport::packages`], by name and version.
    pub packages: Vec<usize>,
}

/// Every package's licence, grouped. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LicenseReport {
    packages: Vec<LicensedPackage>,
}

impl LicenseReport {
    /// A report of these packages.
    pub fn new(packages: Vec<LicensedPackage>) -> Self {
        LicenseReport { packages }
    }

    /// Every package in `cargo metadata --format-version 1` output.
    pub fn from_json(json: &str) -> Result<Self, DepsError> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| DepsError::new(format!("not cargo metadata JSON: {e}")))?;
        Self::from_value(&value)
    }

    /// [`LicenseReport::from_json`] for parsed JSON.
    pub fn from_value(value: &Value) -> Result<Self, DepsError> {
        let packages = value
            .get("packages")
            .and_then(Value::as_array)
            .ok_or_else(|| DepsError::new("not cargo metadata: no `packages`"))?;
        check_count(packages.len(), "packages")?;
        let mut read = Vec::with_capacity(packages.len());
        for package in packages {
            let field = |key: &str| {
                package
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let (Some(id), Some(name), Some(version)) =
                (field("id"), field("name"), field("version"))
            else {
                return Err(DepsError::new(
                    "not cargo metadata: a package has no id, name or version",
                ));
            };
            read.push(LicensedPackage {
                id,
                name,
                version,
                license: field("license"),
                license_file: field("license_file"),
            });
        }
        Ok(LicenseReport { packages: read })
    }

    /// Keep only the packages `keep` accepts (say, those a
    /// [`DepGraph`](super::DepGraph) reaches).
    pub fn retain(mut self, keep: impl FnMut(&LicensedPackage) -> bool) -> Self {
        self.packages.retain(keep);
        self
    }

    pub fn packages(&self) -> &[LicensedPackage] {
        &self.packages
    }

    /// The groups, most packages first, then by licence.
    pub fn groups(&self) -> Vec<LicenseGroup> {
        let mut groups: BTreeMap<String, LicenseGroup> = BTreeMap::new();
        for (index, package) in self.packages.iter().enumerate() {
            let (license, class) = package.license_class();
            groups
                .entry(license.clone())
                .or_insert_with(|| LicenseGroup {
                    license,
                    class,
                    packages: Vec::new(),
                })
                .packages
                .push(index);
        }
        let mut groups: Vec<LicenseGroup> = groups.into_values().collect();
        for group in &mut groups {
            group.packages.sort_by(|&a, &b| {
                let (a, b) = (&self.packages[a], &self.packages[b]);
                a.name
                    .cmp(&b.name)
                    .then_with(|| compare_versions(&a.version, &b.version))
            });
        }
        groups.sort_by(|a, b| {
            b.packages
                .len()
                .cmp(&a.packages.len())
                .then_with(|| a.license.cmp(&b.license))
        });
        groups
    }

    /// How many packages fall in each class.
    pub fn counts(&self) -> BTreeMap<LicenseClass, usize> {
        let mut counts = BTreeMap::new();
        for package in &self.packages {
            *counts.entry(package.license_class().1).or_insert(0) += 1;
        }
        counts
    }

    fn heading(&self, groups: &[LicenseGroup], console: &Console) -> Text {
        let count = self.packages.len();
        let mut text = Text::styled(
            format!(
                "{count} crate{} under {} licence{}",
                if count == 1 { "" } else { "s" },
                groups.len(),
                if groups.len() == 1 { "" } else { "s" }
            ),
            theme_style(console, "deps.root"),
        );
        let flagged: Vec<(LicenseClass, usize)> = self
            .counts()
            .into_iter()
            .filter(|(class, _)| class.is_flagged())
            .collect();
        for (index, (class, n)) in flagged.iter().enumerate() {
            text.append(if index == 0 { ": " } else { ", " }, None);
            text.append(
                &format!("{n} {}", class.note()),
                Some(theme_style(console, class.style_key()).into()),
            );
        }
        text
    }

    fn table(&self, groups: &[LicenseGroup], console: &Console) -> Table {
        let mut table = Table::new();
        table.add_column("Licence");
        table.add_column_justify("Crates", Justify::Right);
        table.add_column("Which");
        table.add_column("Note");
        for group in groups {
            let mut names: Vec<String> = group
                .packages
                .iter()
                .take(MAX_LISTED)
                .map(|&p| self.packages[p].display())
                .collect();
            let more = group.packages.len().saturating_sub(MAX_LISTED);
            if more > 0 {
                names.push(format!("and {more} more"));
            }
            let style = theme_style(console, group.class.style_key());
            table.add_row_text(vec![
                Text::styled(clean(&group.license).into_owned(), style.clone()),
                Text::new(group.packages.len().to_string()),
                Text::new(names.join(", ")),
                Text::styled(group.class.note(), style),
            ]);
        }
        table
    }
}

impl Renderable for LicenseReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let groups = self.groups();
        let heading = self.heading(&groups, console);
        if groups.is_empty() {
            return heading.rich_render(console, options);
        }
        stack(&[&heading, &self.table(&groups, console)], console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let groups = self.groups();
        let heading = self.heading(&groups, console);
        if groups.is_empty() {
            return heading.measure(console, options);
        }
        stack_measure(&[&heading, &self.table(&groups, console)], console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, license: Option<&str>, file: Option<&str>) -> LicensedPackage {
        LicensedPackage {
            id: format!("{name} 1.0.0"),
            name: name.into(),
            version: "1.0.0".into(),
            license: license.map(str::to_string),
            license_file: file.map(str::to_string),
        }
    }

    #[test]
    fn expressions_classify_by_their_operators() {
        assert_eq!(classify("MIT"), LicenseClass::Permissive);
        assert_eq!(classify("mit or apache-2.0"), LicenseClass::Permissive);
        assert_eq!(
            classify("(MIT OR Apache-2.0) AND Unicode-DFS-2016"),
            LicenseClass::Permissive
        );
        assert_eq!(classify("LGPL-2.1-or-later"), LicenseClass::WeakCopyleft);
        assert_eq!(classify("GPL-2.0+"), LicenseClass::Copyleft);
        assert_eq!(classify("AGPL-3.0-only AND MIT"), LicenseClass::Copyleft);
        assert_eq!(classify("MPL-2.0 OR GPL-3.0"), LicenseClass::WeakCopyleft);
        // Identifiers match without case, the copyleft ones too.
        assert_eq!(classify("gpl-3.0-only"), LicenseClass::Copyleft);
        assert_eq!(classify("MIT AND lgpl-2.1"), LicenseClass::WeakCopyleft);
        assert_eq!(classify("Agpl-3.0 OR Foo"), LicenseClass::Copyleft);
        assert_eq!(classify("Foo"), LicenseClass::Unrecognised);
        assert_eq!(classify("MIT OR Foo"), LicenseClass::Permissive);
        for invalid in [
            "",
            "MIT OR",
            "(MIT",
            "MIT)",
            "MIT AND AND Apache-2.0",
            "MIT WITH",
            "MIT; GPL",
        ] {
            assert_eq!(classify(invalid), LicenseClass::Invalid, "{invalid:?}");
        }
        let deep = format!("{}MIT{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(classify(&deep), LicenseClass::Invalid);
        assert_eq!(classify(&"MIT OR ".repeat(500)), LicenseClass::Invalid);
    }

    #[test]
    fn groups_merge_reordered_expressions() {
        let report = LicenseReport::new(vec![
            package("a", Some("MIT OR Apache-2.0"), None),
            package("b", Some("Apache-2.0 OR MIT"), None),
            package("c", Some("MIT/Apache-2.0"), None),
            package("d", Some("GPL-3.0"), None),
            package("e", None, Some("LICENSE")),
            package("f", None, None),
        ]);
        let groups = report.groups();
        assert_eq!(groups[0].license, "Apache-2.0 OR MIT");
        assert_eq!(groups[0].packages.len(), 3);
        assert_eq!(groups.len(), 4);
        let console = Console::builder().width(80).color_system(None).build();
        let out = console.render_to_string(&report);
        assert!(
            out.starts_with(
                "6 crates under 4 licences: 1 copyleft, 1 licence file only, 1 no licence"
            ),
            "{out}"
        );
    }

    #[test]
    fn metadata_is_read_and_filtered() {
        let json = r#"{"packages": [
            {"id": "a 1.0.0", "name": "a", "version": "1.0.0", "license": "MIT"},
            {"id": "b 1.0.0", "name": "b", "version": "1.0.0", "license": null,
             "license_file": "COPYING"}]}"#;
        let report = LicenseReport::from_json(json).unwrap();
        assert_eq!(report.packages().len(), 2);
        let report = report.retain(|p| p.name == "a");
        assert_eq!(report.groups().len(), 1);
        assert!(LicenseReport::from_json("{}").is_err());
        assert!(LicenseReport::from_json(r#"{"packages": [{}]}"#).is_err());
    }
}
