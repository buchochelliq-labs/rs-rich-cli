//! Licence policy (#704): an allow and deny list checked against every
//! package's SPDX expression, and dependency licences that conflict with
//! the root crate's own.
//!
//! A [`Policy`] is built in code or read from TOML ([`Policy::from_toml`],
//! with the `toml` feature) in either of two shapes, told apart by whether
//! the document has a `[licenses]` table:
//!
//! ```toml
//! # rich's own: the keys at the top level.
//! allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception"]
//! deny = ["GPL-3.0-only"]
//! exceptions = [{ name = "ring", allow = ["OpenSSL"] }]
//! conflicts = "warn"   # or "deny" (they fail the check) or "ignore"
//! ```
//!
//! or cargo-deny's `deny.toml`, whose `[licenses]` section gives `allow`,
//! `deny` (older releases), `exceptions` (`crate` or `name`, an optional
//! `version` requirement, `allow`) and `private.ignore`; its other keys
//! (`confidence-threshold`, `clarify`, …) are noted as not applied. In
//! cargo-deny's shape a licence not allowed is refused, as cargo-deny
//! refuses it; in rich's, a policy with no `allow` list allows whatever it
//! does not deny.
//!
//! Each expression is evaluated, not matched as text: an `OR` is satisfied
//! when any alternative is, an `AND` when every part is, and a `WITH`
//! exception is part of the licence (`Apache-2.0 WITH LLVM-exception` must
//! be allowed as such). Identifiers match without case; `-only`,
//! `-or-later` and `+` are read, so allowing `GPL-3.0-only` satisfies
//! `GPL-2.0-or-later`. A deny entry catches its licence however it is
//! suffixed (and any `WITH`, unless the entry names one), and wins over
//! the allow list; a crate's exception wins over both.
//!
//! **Conflicts** ([`conflicts`]) are a heuristic, not legal advice: each
//! dependency's licence is set against the root crate's on a small table.
//! Strong copyleft (GPL, AGPL, EUPL, …) under a permissive, proprietary or
//! weak-copyleft root is a conflict; weak copyleft (LGPL, MPL, …) under a
//! permissive or proprietary root is worth a check; permissive under
//! anything is fine. Within the GNU family, `GPL-2.0-only` conflicts with
//! Apache-2.0 and with the version 3 licences, EPL-1.0 and CDDL conflict
//! with the GPL, and AGPL under another GPL is worth a check. A linking
//! exception (Classpath, GCC runtime, LLVM) makes a copyleft licence count
//! as weak. An identifier it does not know is worth a check.
//!
//! [`PolicyReport`] draws the violations (crate, version, expression, why),
//! the conflicts and the warnings.
//!
//! ```
//! use rich_ext::deps::licenses::LicensedPackage;
//! use rich_ext::deps::policy::{Finding, Policy};
//!
//! let package = |license: &str| LicensedPackage {
//!     id: "x 1.0.0".into(),
//!     name: "x".into(),
//!     version: "1.0.0".into(),
//!     license: Some(license.into()),
//!     license_file: None,
//! };
//! let policy = Policy::new().allow(["MIT", "Apache-2.0"]).deny(["GPL-3.0-only"]);
//! assert_eq!(policy.check(&package("MIT OR GPL-3.0-only")), Finding::Allowed);
//! assert!(matches!(policy.check(&package("MIT AND GPL-3.0-only")), Finding::Violation(_)));
//! assert!(matches!(policy.check(&package("Zlib")), Finding::Violation(_)));
//! ```

use std::collections::HashSet;

use rich::table::Table;
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};
use serde_json::Value;

use super::licenses::{identifier, parse as parse_expression, Expr, LicenseClass};
use super::licenses::{LicenseReport, LicensedPackage};
use super::{clean, compare_versions, stack, stack_measure, theme_style, DepsError};

/// A licence identifier as the policy matches it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Licensee {
    /// The identifier without `-only`, `-or-later` or `+`, in lower case.
    base: String,
    /// Whether it says "or any later version".
    or_later: bool,
    /// The `WITH` exception, in lower case.
    exception: Option<String>,
}

impl Licensee {
    fn new(id: &str, exception: Option<&str>) -> Self {
        let mut base = id.trim().to_ascii_lowercase();
        let mut or_later = false;
        if let Some(stripped) = base.strip_suffix('+') {
            base = stripped.to_string();
            or_later = true;
        }
        if let Some(stripped) = base.strip_suffix("-or-later") {
            base = stripped.to_string();
            or_later = true;
        } else if let Some(stripped) = base.strip_suffix("-only") {
            base = stripped.to_string();
        }
        Licensee {
            base,
            or_later,
            exception: exception.map(|e| e.trim().to_ascii_lowercase()),
        }
    }

    /// The family and version: `gpl` and `2.0` for `gpl-2.0`.
    fn family(&self) -> (&str, Option<&str>) {
        match self.base.rsplit_once('-') {
            Some((family, version)) if version.starts_with(|c: char| c.is_ascii_digit()) => {
                (family, Some(version))
            }
            _ => (&self.base, None),
        }
    }

    /// Whether `self` is a later version of the family `other` names.
    fn at_least(&self, other: &Licensee) -> bool {
        match (self.family(), other.family()) {
            ((a, Some(av)), (b, Some(bv))) if a == b => {
                compare_versions(av, bv) != std::cmp::Ordering::Less
            }
            _ => false,
        }
    }

    /// Whether this allowed licence lets code under `required` in.
    fn satisfies(&self, required: &Licensee) -> bool {
        self.exception == required.exception
            && (self.base == required.base || (required.or_later && self.at_least(required)))
    }

    /// Whether this denied licence catches `required`.
    fn catches(&self, required: &Licensee) -> bool {
        if self.exception.is_some() && self.exception != required.exception {
            return false;
        }
        self.base == required.base || (self.or_later && required.at_least(self))
    }

    fn is_gnu(&self) -> bool {
        matches!(self.family().0, "gpl" | "lgpl" | "agpl")
    }

    /// GPL-2.0 without "or later".
    fn is_gpl2_only(&self) -> bool {
        self.family() == ("gpl", Some("2.0")) && !self.or_later
    }

    /// A GNU licence of version 3.
    fn is_gnu3(&self) -> bool {
        self.is_gnu() && self.family().1.is_some_and(|v| v.starts_with('3'))
    }
}

/// What the policy does with conflicts between a dependency's licence and
/// the root crate's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictMode {
    /// Show them; they do not fail the check. The default.
    #[default]
    Warn,
    /// Show them, and a conflict fails the check.
    Deny,
    /// Leave them out.
    Ignore,
}

/// Which shape a policy was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyFormat {
    /// rich's own TOML: `allow`, `deny`, `exceptions` at the top level.
    Rich,
    /// cargo-deny's `deny.toml`, its `[licenses]` section.
    CargoDeny,
    /// Built in code.
    Built,
}

/// Licences one crate may use beyond the allow list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyException {
    /// The crate's name.
    pub name: String,
    /// A Cargo version requirement (`1.2`, `=0.16.20`, `>=1, <2`); `None`
    /// for every version.
    pub version: Option<String>,
    /// The licences it may use.
    pub allow: Vec<String>,
}

/// An allow and deny list. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    /// `None`: whatever is not denied.
    allow: Option<Vec<Licensee>>,
    deny: Vec<(String, Licensee)>,
    exceptions: Vec<(PolicyException, Vec<Licensee>)>,
    conflicts: ConflictMode,
    ignore_private: bool,
    private_registries: Vec<String>,
    format: PolicyFormat,
    notes: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self::new()
    }
}

impl Policy {
    /// A policy that allows everything; narrow it with [`allow`](Self::allow)
    /// and [`deny`](Self::deny).
    pub fn new() -> Self {
        Policy {
            allow: None,
            deny: Vec::new(),
            exceptions: Vec::new(),
            conflicts: ConflictMode::Warn,
            ignore_private: false,
            private_registries: Vec::new(),
            format: PolicyFormat::Built,
            notes: Vec::new(),
        }
    }

    /// The licence an entry names, or a note on why it was skipped.
    fn licensee(&mut self, list: &str, entry: &str) -> Option<Licensee> {
        match parse_expression(entry) {
            Some(Expr::Id(id)) => Some(Licensee::new(&id, None)),
            Some(Expr::With(id, exception)) => Some(Licensee::new(&id, Some(exception.as_str()))),
            Some(_) => {
                self.notes.push(format!(
                    "{list}: {:?} is an expression; list each licence on its own (skipped)",
                    clean(entry)
                ));
                None
            }
            None => {
                self.notes.push(format!(
                    "{list}: {:?} is not a licence identifier (skipped)",
                    clean(entry)
                ));
                None
            }
        }
    }

    /// Allow these licences (and only these, with the others allowed so
    /// far): `MIT`, `Apache-2.0 WITH LLVM-exception`.
    pub fn allow<S: AsRef<str>>(mut self, ids: impl IntoIterator<Item = S>) -> Self {
        let mut list = self.allow.take().unwrap_or_default();
        for id in ids {
            if let Some(licensee) = self.licensee("allow", id.as_ref()) {
                list.push(licensee);
            }
        }
        self.allow = Some(list);
        self
    }

    /// Deny these licences, whatever the allow list says.
    pub fn deny<S: AsRef<str>>(mut self, ids: impl IntoIterator<Item = S>) -> Self {
        for id in ids {
            if let Some(licensee) = self.licensee("deny", id.as_ref()) {
                self.deny.push((id.as_ref().trim().to_string(), licensee));
            }
        }
        self
    }

    /// Let one crate use more licences.
    pub fn exception(mut self, exception: PolicyException) -> Self {
        if let Some(requirement) = &exception.version {
            if meets("0.0.0", requirement).is_none() {
                self.notes.push(format!(
                    "exceptions: {:?} for {} is not a version requirement; the exception \
                     applies to no version",
                    clean(requirement),
                    clean(&exception.name)
                ));
            }
        }
        let allowed = exception
            .allow
            .iter()
            .filter_map(|id| self.licensee("exceptions", id))
            .collect();
        self.exceptions.push((exception, allowed));
        self
    }

    /// What to do with [conflicts](conflicts) ([`ConflictMode::Warn`] by
    /// default).
    pub fn conflicts(mut self, mode: ConflictMode) -> Self {
        self.conflicts = mode;
        self
    }

    /// Leave out the workspace's unpublished crates, as cargo-deny's
    /// `private.ignore` does (see [`unpublished`]).
    pub fn ignore_private(mut self, ignore: bool) -> Self {
        self.ignore_private = ignore;
        self
    }

    pub fn conflict_mode(&self) -> ConflictMode {
        self.conflicts
    }

    pub fn ignores_private(&self) -> bool {
        self.ignore_private
    }

    /// The registries cargo-deny's `private.registries` names: publishing
    /// only to them still counts as unpublished.
    pub fn private_registries(&self) -> &[String] {
        &self.private_registries
    }

    pub fn format(&self) -> PolicyFormat {
        self.format
    }

    /// What reading the policy noticed: entries skipped, settings not
    /// applied.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// What the policy makes of `package`'s licence.
    pub fn check(&self, package: &LicensedPackage) -> Finding {
        // Without an allow list, what cannot be read is a warning.
        let refused = |why: &str| {
            if self.allow.is_some() {
                Finding::Violation(why.to_string())
            } else {
                Finding::Warning(why.to_string())
            }
        };
        let Some(expression) = &package.license else {
            return if package.license_file.is_some() {
                refused("a licence file only: no SPDX expression to check")
            } else {
                refused("no licence")
            };
        };
        let Some(expr) = parse_expression(expression) else {
            return refused("not an SPDX expression");
        };
        let extra: Vec<&Licensee> = self
            .exceptions
            .iter()
            .filter(|(exception, _)| {
                exception.name == package.name
                    && exception
                        .version
                        .as_deref()
                        .is_none_or(|req| meets(&package.version, req) == Some(true))
            })
            .flat_map(|(_, allowed)| allowed)
            .collect();
        let mut failures = Vec::new();
        if self.satisfied(&expr, &extra, &mut failures) {
            if self.allow.is_none() && expr.class() == LicenseClass::Unrecognised {
                return Finding::Warning(format!(
                    "{} is not a licence this reader knows",
                    clean(expression.trim())
                ));
            }
            return Finding::Allowed;
        }
        let mut seen = HashSet::new();
        failures.retain(|failure| seen.insert(failure.clone()));
        Finding::Violation(failures.join("; "))
    }

    fn satisfied(&self, expr: &Expr, extra: &[&Licensee], failures: &mut Vec<String>) -> bool {
        match expr {
            Expr::Id(id) => self.leaf(id, None, extra, failures),
            Expr::With(id, exception) => self.leaf(id, Some(exception.as_str()), extra, failures),
            // Every part is checked, so every failing one is named.
            Expr::And(parts) => {
                let mut all = true;
                for part in parts {
                    all &= self.satisfied(part, extra, failures);
                }
                all
            }
            Expr::Or(parts) => {
                let mut tried = Vec::new();
                if parts
                    .iter()
                    .any(|part| self.satisfied(part, extra, &mut tried))
                {
                    return true;
                }
                failures.extend(tried);
                false
            }
        }
    }

    fn leaf(
        &self,
        id: &str,
        exception: Option<&str>,
        extra: &[&Licensee],
        failures: &mut Vec<String>,
    ) -> bool {
        let required = Licensee::new(id, exception);
        let text = match exception {
            Some(exception) => format!("{id} WITH {exception}"),
            None => id.to_string(),
        };
        if extra.iter().any(|allowed| allowed.satisfies(&required)) {
            return true;
        }
        if let Some((entry, _)) = self
            .deny
            .iter()
            .find(|(_, denied)| denied.catches(&required))
        {
            failures.push(if entry.eq_ignore_ascii_case(&text) {
                format!("{text} is denied")
            } else {
                format!("{text} is denied ({})", clean(entry))
            });
            return false;
        }
        match &self.allow {
            None => true,
            Some(list) if list.iter().any(|allowed| allowed.satisfies(&required)) => true,
            Some(_) => {
                failures.push(format!("{text} is not allowed"));
                false
            }
        }
    }
}

#[cfg(feature = "toml")]
impl Policy {
    /// Read a policy: rich's TOML or cargo-deny's `deny.toml`. See the
    /// [module docs](self).
    pub fn from_toml(text: &str) -> Result<Self, DepsError> {
        super::check_size(text, "the policy")?;
        let document: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| DepsError::new(format!("not TOML: {}", e.message())))?;
        let (table, format, prefix) = match document.get("licenses") {
            Some(toml::Value::Table(licenses)) => (licenses, PolicyFormat::CargoDeny, "licenses."),
            Some(_) => return Err(DepsError::new("`licenses` is not a table")),
            None => (&document, PolicyFormat::Rich, ""),
        };
        let strings = |value: &toml::Value, key: &str| -> Result<Vec<String>, DepsError> {
            let not_strings =
                || DepsError::new(format!("`{prefix}{key}` is not a list of strings"));
            let toml::Value::Array(items) = value else {
                return Err(not_strings());
            };
            items
                .iter()
                .map(|item| match item {
                    toml::Value::String(text) => Ok(text.clone()),
                    _ => Err(not_strings()),
                })
                .collect()
        };
        let allow = table
            .get("allow")
            .map(|v| strings(v, "allow"))
            .transpose()?;
        let deny = table.get("deny").map(|v| strings(v, "deny")).transpose()?;
        let exceptions = table.get("exceptions");
        if format == PolicyFormat::Rich && allow.is_none() && deny.is_none() && exceptions.is_none()
        {
            return Err(DepsError::new(
                "no `allow`, `deny` or `exceptions`, and no `[licenses]` section: not a \
                 licence policy",
            ));
        }
        let mut policy = Policy::new();
        policy.format = format;
        match (format, allow) {
            (_, Some(allow)) => policy = policy.allow(allow),
            // cargo-deny refuses whatever it is not told to allow.
            (PolicyFormat::CargoDeny, None) => policy = policy.allow(Vec::<String>::new()),
            _ => {}
        }
        if let Some(deny) = deny {
            policy = policy.deny(deny);
        }
        if let Some(exceptions) = exceptions {
            let toml::Value::Array(items) = exceptions else {
                return Err(DepsError::new(format!(
                    "`{prefix}exceptions` is not a list of tables"
                )));
            };
            for (index, item) in items.iter().enumerate() {
                let at = format!("`{prefix}exceptions[{index}]`");
                let toml::Value::Table(entry) = item else {
                    return Err(DepsError::new(format!("{at} is not a table")));
                };
                let text = |key: &str| match entry.get(key) {
                    None => Ok(None),
                    Some(toml::Value::String(text)) => Ok(Some(text.clone())),
                    Some(_) => Err(DepsError::new(format!("{at}: `{key}` is not a string"))),
                };
                let name = match (text("crate")?, text("name")?) {
                    (Some(name), _) | (None, Some(name)) => name,
                    (None, None) => {
                        return Err(DepsError::new(format!("{at} names no crate")));
                    }
                };
                let allow = match entry.get("allow") {
                    Some(value) => strings(value, &format!("exceptions[{index}].allow"))?,
                    None => return Err(DepsError::new(format!("{at} has no `allow` list"))),
                };
                policy = policy.exception(PolicyException {
                    name,
                    version: text("version")?.filter(|v| !v.trim().is_empty() && v.trim() != "*"),
                    allow,
                });
            }
        }
        match table.get("private") {
            None => {}
            Some(toml::Value::Table(private)) => {
                match private.get("ignore") {
                    None => {}
                    Some(toml::Value::Boolean(ignore)) => policy.ignore_private = *ignore,
                    Some(_) => {
                        return Err(DepsError::new(format!(
                            "`{prefix}private.ignore` is not true or false"
                        )))
                    }
                }
                if let Some(registries) = private.get("registries") {
                    policy.private_registries = strings(registries, "private.registries")?;
                }
            }
            Some(_) => {
                return Err(DepsError::new(format!("`{prefix}private` is not a table")));
            }
        }
        if format == PolicyFormat::Rich {
            policy.conflicts = match table.get("conflicts") {
                None => ConflictMode::Warn,
                Some(toml::Value::String(mode)) => match mode.as_str() {
                    "warn" => ConflictMode::Warn,
                    "deny" => ConflictMode::Deny,
                    "ignore" => ConflictMode::Ignore,
                    _ => {
                        return Err(DepsError::new(format!(
                            "`conflicts` is {:?}: warn, deny or ignore",
                            clean(mode)
                        )))
                    }
                },
                Some(_) => return Err(DepsError::new("`conflicts` is not a string")),
            };
        }
        let known: &[&str] = match format {
            PolicyFormat::Rich => &["allow", "deny", "exceptions", "private", "conflicts"],
            _ => &[
                "allow",
                "deny",
                "exceptions",
                "private",
                "version",
                "unused-allowed-license",
                "include-dev",
            ],
        };
        for key in table.keys() {
            if known.contains(&key.as_str()) {
                continue;
            }
            let note = match (format, key.as_str()) {
                (PolicyFormat::CargoDeny, "confidence-threshold" | "clarify") => {
                    format!("`{prefix}{key}` is not applied: licence files are not read",)
                }
                (PolicyFormat::CargoDeny, _) => format!("`{prefix}{}` is not applied", clean(key)),
                _ => format!("unknown key `{}` (ignored)", clean(key)),
            };
            policy.notes.push(note);
        }
        Ok(policy)
    }
}

/// Whether `version` meets `requirement`, a Cargo version requirement
/// (`1.2`, `^1.2.3`, `~1.2`, `=1.0.0`, `>=1, <2`, `1.*`, `*`); `None` when
/// either cannot be read. Pre-release and build parts are ignored.
pub fn meets(version: &str, requirement: &str) -> Option<bool> {
    fn numbers(text: &str) -> Option<Vec<u64>> {
        let core = text.split(['-', '+']).next()?.trim();
        let mut parts = Vec::new();
        for part in core.split('.') {
            if part == "*" || part.eq_ignore_ascii_case("x") {
                break;
            }
            parts.push(part.parse().ok()?);
        }
        (parts.len() <= 3).then_some(parts)
    }
    let version = numbers(version).filter(|v| !v.is_empty())?;
    let full = |parts: &[u64]| -> [u64; 3] {
        [
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        ]
    };
    let version = full(&version);
    // Past the last component given: `1.2` → `1.3.0`.
    let next = |parts: &[u64]| -> [u64; 3] {
        match parts.len() {
            1 => [parts[0].saturating_add(1), 0, 0],
            2 => [parts[0], parts[1].saturating_add(1), 0],
            _ => [parts[0], parts[1], parts[2].saturating_add(1)],
        }
    };
    let requirement = requirement.trim();
    if requirement.is_empty() {
        return None;
    }
    for comparator in requirement.split(',') {
        let comparator = comparator.trim();
        let (op, rest) = [">=", "<=", ">", "<", "=", "^", "~"]
            .iter()
            .find_map(|op| comparator.strip_prefix(op).map(|rest| (*op, rest.trim())))
            .unwrap_or(("^", comparator));
        let parts = numbers(rest)?;
        if parts.is_empty() {
            // `*`: any version.
            if op == "^" || op == "=" {
                continue;
            }
            return None;
        }
        let low = full(&parts);
        let wildcard = rest.contains('*') || rest.contains(['x', 'X']);
        let (lo, hi): ([u64; 3], Option<[u64; 3]>) = match op {
            "=" => (low, Some(next(&parts))),
            ">" => (next(&parts), None),
            ">=" => (low, None),
            "<" => ([0, 0, 0], Some(low)),
            "<=" => ([0, 0, 0], Some(next(&parts))),
            "~" => (
                low,
                Some(if parts.len() >= 2 {
                    [parts[0], parts[1].saturating_add(1), 0]
                } else {
                    [parts[0].saturating_add(1), 0, 0]
                }),
            ),
            _ if wildcard => (low, Some(next(&parts))),
            _ => {
                let upper = if parts[0] > 0 || parts.len() == 1 {
                    [parts[0].saturating_add(1), 0, 0]
                } else if parts[1] > 0 || parts.len() == 2 {
                    [0, parts[1].saturating_add(1), 0]
                } else {
                    [0, 0, parts[2].saturating_add(1)]
                };
                (low, Some(upper))
            }
        };
        let exact = op == "=" && parts.len() == 3;
        let inside = if exact {
            version == low
        } else {
            version >= lo && hi.is_none_or(|hi| version < hi)
        };
        if !inside {
            return Some(false);
        }
    }
    Some(true)
}

/// What a policy makes of one package's licence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
    Allowed,
    /// Refused, and why: `GPL-3.0-only is denied`.
    Violation(String),
    /// Not refused, but worth a look: no licence under a policy with no
    /// allow list, say.
    Warning(String),
}

/// How serious a [`Conflict`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Nothing to see.
    Compatible,
    /// Worth a check: weak copyleft, an unknown licence.
    Check,
    /// The licences do not combine.
    Conflict,
}

impl Severity {
    pub fn name(self) -> &'static str {
        match self {
            Severity::Compatible => "compatible",
            Severity::Check => "check",
            Severity::Conflict => "conflict",
        }
    }
}

/// A dependency whose licence does not sit well with the root crate's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// The root crate, `name v1.0.0`.
    pub root: String,
    /// Its licence, as written (`(none)` without one).
    pub root_license: String,
    pub name: String,
    pub version: String,
    pub license: String,
    pub severity: Severity,
    pub why: String,
}

/// How a licence's kind reads against another's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Permissive,
    Weak,
    Strong,
    Unknown,
}

/// Exceptions that let code link against a copyleft licence without
/// falling under it.
const LINKING_EXCEPTIONS: &[&str] = &[
    "classpath-exception-2.0",
    "gcc-exception-2.0",
    "gcc-exception-3.1",
    "llvm-exception",
];

fn kind(licensee: &Licensee, id: &str) -> Kind {
    match identifier(id) {
        LicenseClass::Permissive => Kind::Permissive,
        LicenseClass::WeakCopyleft => Kind::Weak,
        LicenseClass::Copyleft
            if licensee
                .exception
                .as_deref()
                .is_some_and(|e| LINKING_EXCEPTIONS.contains(&e)) =>
        {
            Kind::Weak
        }
        LicenseClass::Copyleft => Kind::Strong,
        _ => Kind::Unknown,
    }
}

fn leaf(expr: &Expr) -> Option<(&str, Option<&str>)> {
    match expr {
        Expr::Id(id) => Some((id.as_str(), None)),
        Expr::With(id, exception) => Some((id.as_str(), Some(exception.as_str()))),
        _ => None,
    }
}

/// The compatibility table: one root licence against one dependency's.
fn pair(root: (&str, Option<&str>), dep: (&str, Option<&str>)) -> (Severity, String) {
    let (r, d) = (Licensee::new(root.0, root.1), Licensee::new(dep.0, dep.1));
    let root_kind = kind(&r, root.0);
    let shown = match dep.1 {
        Some(exception) => format!("{} WITH {exception}", dep.0),
        None => dep.0.to_string(),
    };
    let gpl2_only = || r.is_gpl2_only();
    let fine = (Severity::Compatible, String::new());
    match kind(&d, dep.0) {
        Kind::Unknown => (
            Severity::Check,
            format!("{shown}: a licence this reader does not know"),
        ),
        Kind::Permissive => {
            if gpl2_only() && d.base == "apache-2.0" && d.exception.as_deref() != Some("llvm-exception")
            {
                (
                    Severity::Conflict,
                    "Apache-2.0 is incompatible with GPL-2.0-only".into(),
                )
            } else {
                fine
            }
        }
        Kind::Weak => match root_kind {
            Kind::Permissive | Kind::Unknown if d.family().0 == "lgpl" => (
                Severity::Check,
                format!(
                    "weak copyleft: linked statically, {shown} asks that users can relink \
                     against a changed copy"
                ),
            ),
            Kind::Permissive | Kind::Unknown => (
                Severity::Check,
                format!("weak copyleft: changes to its own files stay under {shown}"),
            ),
            Kind::Strong if r.is_gnu() && (d.base == "epl-1.0" || d.base.starts_with("cddl-")) => (
                Severity::Conflict,
                format!("{shown} is incompatible with the GPL"),
            ),
            Kind::Strong if gpl2_only() && d.is_gnu3() => (
                Severity::Conflict,
                "version 3 terms are incompatible with GPL-2.0-only".into(),
            ),
            _ => fine,
        },
        Kind::Strong => match root_kind {
            Kind::Permissive => (
                Severity::Conflict,
                format!("copyleft in a permissive crate: the combined work falls under {shown}"),
            ),
            Kind::Unknown => (
                Severity::Conflict,
                format!(
                    "copyleft in a proprietary or unlicensed crate: the combined work falls \
                     under {shown}"
                ),
            ),
            Kind::Weak => (
                Severity::Conflict,
                format!("strong copyleft in a weak-copyleft crate: the combined work falls under {shown}"),
            ),
            Kind::Strong if r.is_gnu() && d.is_gnu() => {
                if gpl2_only() && d.is_gnu3() {
                    (
                        Severity::Conflict,
                        "version 3 terms are incompatible with GPL-2.0-only".into(),
                    )
                } else if d.family().0 == "agpl" && r.family().0 != "agpl" {
                    (
                        Severity::Check,
                        "the AGPL adds terms for use over a network".into(),
                    )
                } else {
                    fine
                }
            }
            Kind::Strong if r.base == d.base => fine,
            Kind::Strong => (
                Severity::Check,
                format!("two different copyleft licences: check that {shown} combines with the root's"),
            ),
        },
    }
}

/// The best outcome over choices (an `OR`), the worst over parts (an `AND`).
fn compat(root: &Expr, dep: &Expr) -> (Severity, String) {
    let pick = |items: Vec<(Severity, String)>, worst: bool| {
        let chosen = if worst {
            items
                .into_iter()
                .rev()
                .max_by_key(|(severity, _)| *severity)
        } else {
            items.into_iter().min_by_key(|(severity, _)| *severity)
        };
        chosen.unwrap_or((Severity::Compatible, String::new()))
    };
    match root {
        Expr::Or(parts) => pick(parts.iter().map(|p| compat(p, dep)).collect(), false),
        Expr::And(parts) => pick(parts.iter().map(|p| compat(p, dep)).collect(), true),
        _ => match dep {
            Expr::Or(parts) => pick(parts.iter().map(|p| compat(root, p)).collect(), false),
            Expr::And(parts) => pick(parts.iter().map(|p| compat(root, p)).collect(), true),
            _ => match (leaf(root), leaf(dep)) {
                (Some(root), Some(dep)) => pair(root, dep),
                _ => (Severity::Compatible, String::new()),
            },
        },
    }
}

/// The dependencies in `packages` whose licences conflict with `root`'s,
/// or are worth a check. A heuristic, not legal advice: see the
/// [module docs](self). Packages with no readable expression are left to
/// the policy; `root` itself is skipped.
pub fn conflicts(root: &LicensedPackage, packages: &[LicensedPackage]) -> Vec<Conflict> {
    let (root_license, root_expr) = match root.license.as_deref().map(|l| (l, parse_expression(l)))
    {
        Some((text, Some(expr))) => (text.trim().to_string(), expr),
        // No licence (or none readable): proprietary, for this table.
        Some((text, None)) => (text.trim().to_string(), Expr::Id("NOASSERTION".into())),
        None if root.license_file.is_some() => {
            ("(licence file)".into(), Expr::Id("NOASSERTION".into()))
        }
        None => ("(none)".into(), Expr::Id("NOASSERTION".into())),
    };
    let mut found = Vec::new();
    for package in packages {
        if package.id == root.id {
            continue;
        }
        let Some(expr) = package.license.as_deref().and_then(parse_expression) else {
            continue;
        };
        let (severity, why) = compat(&root_expr, &expr);
        if severity > Severity::Compatible {
            found.push(Conflict {
                root: root.display(),
                root_license: clean(&root_license).into_owned(),
                name: package.name.clone(),
                version: package.version.clone(),
                license: package
                    .license
                    .clone()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                severity,
                why,
            });
        }
    }
    found.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| compare_versions(&a.version, &b.version))
    });
    found
}

/// The ids of the packages `cargo metadata` marks unpublished: `publish =
/// false` (an empty list), or publishing only to the `registries` given.
pub fn unpublished(metadata: &str, registries: &[String]) -> Result<HashSet<String>, DepsError> {
    let value: Value = serde_json::from_str(metadata)
        .map_err(|e| DepsError::new(format!("not cargo metadata JSON: {e}")))?;
    let packages = value
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| DepsError::new("not cargo metadata: no `packages`"))?;
    Ok(packages
        .iter()
        .filter(|package| {
            package
                .get("publish")
                .and_then(Value::as_array)
                .is_some_and(|to| {
                    to.iter().all(|r| {
                        r.as_str()
                            .is_some_and(|r| registries.iter().any(|p| p == r))
                    })
                })
        })
        .filter_map(|package| package.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

/// One package the policy refused or warned about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyFinding {
    pub name: String,
    pub version: String,
    /// The expression as written, or `(none)`, `(licence file)`.
    pub license: String,
    pub finding: Finding,
}

/// A policy checked against every package. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyReport {
    findings: Vec<PolicyFinding>,
    packages: Vec<LicensedPackage>,
    conflicts: Vec<Conflict>,
    mode: ConflictMode,
    notes: Vec<String>,
    name: Option<String>,
}

impl PolicyReport {
    /// `policy` checked against each package in `report`.
    pub fn new(report: &LicenseReport, policy: &Policy) -> Self {
        let mut findings: Vec<PolicyFinding> = report
            .packages()
            .iter()
            .filter_map(|package| {
                let finding = policy.check(package);
                (finding != Finding::Allowed).then(|| PolicyFinding {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    license: match (&package.license, &package.license_file) {
                        (Some(license), _) => license.trim().to_string(),
                        (None, Some(_)) => "(licence file)".into(),
                        (None, None) => "(none)".into(),
                    },
                    finding,
                })
            })
            .collect();
        findings.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| compare_versions(&a.version, &b.version))
        });
        PolicyReport {
            findings,
            packages: report.packages().to_vec(),
            conflicts: Vec::new(),
            mode: policy.conflict_mode(),
            notes: policy.notes().to_vec(),
            name: None,
        }
    }

    /// Name the policy in the heading (its file, say).
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Check every package's licence against `root`'s ([`conflicts`]).
    pub fn root(self, root: &LicensedPackage) -> Self {
        let found = conflicts(root, &self.packages);
        self.with_conflicts(found)
    }

    /// Add conflicts found elsewhere (each workspace member against what it
    /// depends on, say).
    pub fn with_conflicts(mut self, conflicts: Vec<Conflict>) -> Self {
        if self.mode == ConflictMode::Ignore {
            return self;
        }
        for conflict in conflicts {
            if !self.conflicts.contains(&conflict) {
                self.conflicts.push(conflict);
            }
        }
        self.conflicts.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| compare_versions(&a.version, &b.version))
                .then_with(|| a.root.cmp(&b.root))
        });
        self
    }

    /// Add a note under the report.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// How many packages were checked.
    pub fn checked(&self) -> usize {
        self.packages.len()
    }

    /// The packages the policy refused.
    pub fn violations(&self) -> Vec<&PolicyFinding> {
        self.findings
            .iter()
            .filter(|f| matches!(f.finding, Finding::Violation(_)))
            .collect()
    }

    /// The packages the policy warned about.
    pub fn warnings(&self) -> Vec<&PolicyFinding> {
        self.findings
            .iter()
            .filter(|f| matches!(f.finding, Finding::Warning(_)))
            .collect()
    }

    /// The conflicts with the root crate's licence, and those worth a check.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Notes on the policy itself.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Whether the check fails: a violation, or a conflict when the policy
    /// denies them.
    pub fn failed(&self) -> bool {
        !self.violations().is_empty()
            || (self.mode == ConflictMode::Deny
                && self
                    .conflicts
                    .iter()
                    .any(|c| c.severity == Severity::Conflict))
    }

    fn heading(&self, console: &Console) -> Text {
        let mut line = String::from("Licence policy");
        if let Some(name) = &self.name {
            line.push_str(&format!(" ({})", clean(name)));
        }
        let checked = self.checked();
        line.push_str(&format!(
            ": {checked} crate{} checked, ",
            if checked == 1 { "" } else { "s" }
        ));
        let mut text = Text::styled(line, theme_style(console, "deps.root"));
        let violations = self.violations().len();
        let conflicts = self
            .conflicts
            .iter()
            .filter(|c| c.severity == Severity::Conflict)
            .count();
        let warnings = self.warnings().len()
            + self.notes.len()
            + self
                .conflicts
                .iter()
                .filter(|c| c.severity == Severity::Check)
                .count();
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        if violations == 0 {
            text.append(
                "no violations",
                Some(theme_style(console, "deps.ok").into()),
            );
        } else {
            text.append(
                &plural(violations, "violation"),
                Some(theme_style(console, "deps.violation").into()),
            );
        }
        if conflicts > 0 {
            text.append(", ", None);
            let key = if self.mode == ConflictMode::Deny {
                "deps.violation"
            } else {
                "deps.warning"
            };
            text.append(
                &plural(conflicts, "conflict"),
                Some(theme_style(console, key).into()),
            );
        }
        if warnings > 0 {
            text.append(", ", None);
            text.append(
                &plural(warnings, "warning"),
                Some(theme_style(console, "deps.warning").into()),
            );
        }
        text
    }

    fn violation_table(&self, console: &Console) -> Option<Table> {
        let violations = self.violations();
        if violations.is_empty() {
            return None;
        }
        let mut table = Table::new();
        for header in ["Crate", "Version", "Licence", "Why"] {
            table.add_column(header);
        }
        let style = theme_style(console, "deps.violation");
        for finding in violations {
            let Finding::Violation(why) = &finding.finding else {
                continue;
            };
            table.add_row_text(vec![
                Text::new(clean(&finding.name).into_owned()),
                Text::styled(
                    format!("v{}", clean(&finding.version)),
                    theme_style(console, "deps.version"),
                ),
                Text::new(clean(&finding.license).into_owned()),
                Text::styled(why.clone(), style.clone()),
            ]);
        }
        Some(table)
    }

    fn conflict_parts(&self, console: &Console) -> Option<(Text, Table)> {
        if self.conflicts.is_empty() {
            return None;
        }
        let roots: HashSet<&str> = self.conflicts.iter().map(|c| c.root.as_str()).collect();
        let mut title = String::from("Licences against the root crate's");
        if let [only] = roots.iter().collect::<Vec<_>>().as_slice() {
            let root = self.conflicts.iter().find(|c| c.root == **only);
            if let Some(root) = root {
                title = format!("Licences against {} ({})", root.root, root.root_license);
            }
        }
        let mut heading = Text::styled(title, theme_style(console, "deps.root"));
        heading.append(
            "\na heuristic, not legal advice",
            Some(theme_style(console, "deps.section").into()),
        );
        let mut table = Table::new();
        let several = roots.len() > 1;
        for header in ["Crate", "Version", "Licence"] {
            table.add_column(header);
        }
        if several {
            table.add_column("Root");
        }
        table.add_column("Verdict");
        table.add_column("Why");
        for conflict in &self.conflicts {
            let key = match (conflict.severity, self.mode) {
                (Severity::Conflict, ConflictMode::Deny) => "deps.violation",
                _ => "deps.warning",
            };
            let style = theme_style(console, key);
            let mut row = vec![
                Text::new(clean(&conflict.name).into_owned()),
                Text::styled(
                    format!("v{}", clean(&conflict.version)),
                    theme_style(console, "deps.version"),
                ),
                Text::new(clean(&conflict.license).into_owned()),
            ];
            if several {
                row.push(Text::new(format!(
                    "{} ({})",
                    conflict.root, conflict.root_license
                )));
            }
            row.push(Text::styled(conflict.severity.name(), style));
            row.push(Text::new(conflict.why.clone()));
            table.add_row_text(row);
        }
        Some((heading, table))
    }

    fn warning_lines(&self, console: &Console) -> Option<Text> {
        let mut lines: Vec<String> = self
            .warnings()
            .into_iter()
            .filter_map(|finding| match &finding.finding {
                // `(none)` and `(licence file)` say nothing the reason does not.
                Finding::Warning(why) if finding.license.starts_with('(') => Some(format!(
                    "warning: {} v{}: {why}",
                    clean(&finding.name),
                    clean(&finding.version)
                )),
                Finding::Warning(why) => Some(format!(
                    "warning: {} v{} ({}): {why}",
                    clean(&finding.name),
                    clean(&finding.version),
                    clean(&finding.license)
                )),
                _ => None,
            })
            .collect();
        lines.extend(
            self.notes
                .iter()
                .map(|note| format!("note: {}", clean(note))),
        );
        if lines.is_empty() {
            return None;
        }
        Some(Text::styled(
            lines.join("\n"),
            theme_style(console, "deps.warning"),
        ))
    }

    fn parts(&self, console: &Console) -> Vec<Box<dyn Renderable>> {
        let mut parts: Vec<Box<dyn Renderable>> = vec![Box::new(self.heading(console))];
        if let Some(table) = self.violation_table(console) {
            parts.push(Box::new(table));
        }
        if let Some((heading, table)) = self.conflict_parts(console) {
            parts.push(Box::new(heading));
            parts.push(Box::new(table));
        }
        if let Some(lines) = self.warning_lines(console) {
            parts.push(Box::new(lines));
        }
        parts
    }
}

impl Renderable for PolicyReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let parts = self.parts(console);
        let refs: Vec<&dyn Renderable> = parts.iter().map(|part| part.as_ref()).collect();
        stack(&refs, console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let parts = self.parts(console);
        let refs: Vec<&dyn Renderable> = parts.iter().map(|part| part.as_ref()).collect();
        stack_measure(&refs, console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, license: Option<&str>) -> LicensedPackage {
        LicensedPackage {
            id: format!("{name} 1.0.0"),
            name: name.into(),
            version: "1.0.0".into(),
            license: license.map(str::to_string),
            license_file: None,
        }
    }

    fn verdict(policy: &Policy, license: &str) -> Finding {
        policy.check(&package("x", Some(license)))
    }

    #[test]
    fn or_needs_one_and_needs_all() {
        let policy = Policy::new().allow(["MIT", "Apache-2.0"]);
        assert_eq!(verdict(&policy, "MIT OR GPL-3.0-only"), Finding::Allowed);
        assert_eq!(
            verdict(&policy, "(MIT OR Zlib) AND Apache-2.0"),
            Finding::Allowed
        );
        assert_eq!(
            verdict(&policy, "MIT AND Zlib"),
            Finding::Violation("Zlib is not allowed".into())
        );
        assert_eq!(
            verdict(&policy, "Zlib OR ISC"),
            Finding::Violation("Zlib is not allowed; ISC is not allowed".into())
        );
        assert_eq!(verdict(&policy, "mit/apache-2.0"), Finding::Allowed);
    }

    #[test]
    fn with_exceptions_are_kept() {
        let policy = Policy::new().allow(["Apache-2.0"]);
        assert_eq!(
            verdict(&policy, "Apache-2.0 WITH LLVM-exception"),
            Finding::Violation("Apache-2.0 WITH LLVM-exception is not allowed".into())
        );
        let policy = policy.allow(["Apache-2.0 WITH LLVM-exception"]);
        assert_eq!(
            verdict(&policy, "apache-2.0 with llvm-exception"),
            Finding::Allowed
        );
        // A deny entry with no exception catches the licence with one.
        let policy = Policy::new().deny(["GPL-2.0-only"]);
        assert!(matches!(
            verdict(&policy, "GPL-2.0-only WITH Classpath-exception-2.0"),
            Finding::Violation(_)
        ));
        let policy = Policy::new().deny(["GPL-2.0-only WITH Classpath-exception-2.0"]);
        assert_eq!(verdict(&policy, "GPL-2.0-only"), Finding::Allowed);
    }

    #[test]
    fn deny_wins_over_allow_and_exceptions_over_both() {
        let policy = Policy::new()
            .allow(["MIT", "GPL-3.0"])
            .deny(["GPL-3.0"])
            .exception(PolicyException {
                name: "special".into(),
                version: Some("<2".into()),
                allow: vec!["GPL-3.0-or-later".into(), "OpenSSL".into()],
            });
        assert_eq!(
            verdict(&policy, "GPL-3.0-or-later"),
            Finding::Violation("GPL-3.0-or-later is denied (GPL-3.0)".into())
        );
        let special = package("special", Some("GPL-3.0-or-later AND OpenSSL"));
        assert_eq!(policy.check(&special), Finding::Allowed);
        let later = LicensedPackage {
            version: "2.0.0".into(),
            ..special
        };
        assert!(matches!(policy.check(&later), Finding::Violation(_)));
    }

    #[test]
    fn or_later_and_suffixes_match() {
        let policy = Policy::new().allow(["GPL-3.0-only", "LGPL-2.1"]);
        assert_eq!(verdict(&policy, "GPL-2.0-or-later"), Finding::Allowed);
        assert_eq!(verdict(&policy, "GPL-2.0+"), Finding::Allowed);
        assert_eq!(verdict(&policy, "LGPL-2.1-only"), Finding::Allowed);
        assert!(matches!(
            verdict(&policy, "GPL-2.0-only"),
            Finding::Violation(_)
        ));
        let policy = Policy::new().deny(["GPL-2.0-or-later"]);
        assert!(matches!(
            verdict(&policy, "GPL-3.0-only"),
            Finding::Violation(_)
        ));
        assert_eq!(verdict(&policy, "LGPL-3.0"), Finding::Allowed);
    }

    #[test]
    fn missing_and_unreadable_licences() {
        let strict = Policy::new().allow(["MIT"]);
        let open = Policy::new().deny(["GPL-3.0"]);
        let none = package("x", None);
        let file = LicensedPackage {
            license_file: Some("LICENSE".into()),
            ..none.clone()
        };
        assert_eq!(strict.check(&none), Finding::Violation("no licence".into()));
        assert_eq!(open.check(&none), Finding::Warning("no licence".into()));
        assert!(matches!(strict.check(&file), Finding::Violation(_)));
        assert!(matches!(
            verdict(&strict, "MIT AND ("),
            Finding::Violation(_)
        ));
        assert!(matches!(verdict(&open, "Foo-Bar"), Finding::Warning(_)));
        assert!(matches!(verdict(&strict, "Foo-Bar"), Finding::Violation(_)));
    }

    #[test]
    fn odd_entries_are_noted_not_fatal() {
        let policy = Policy::new().allow(["MIT OR Apache-2.0", "has spaces", "ISC"]);
        assert_eq!(policy.notes().len(), 2, "{:?}", policy.notes());
        assert_eq!(verdict(&policy, "ISC"), Finding::Allowed);
    }

    #[test]
    fn version_requirements() {
        for (version, requirement, expected) in [
            ("1.2.3", "1.2", Some(true)),
            ("2.0.0", "1.2", Some(false)),
            ("0.2.5", "^0.2.3", Some(true)),
            ("0.3.0", "^0.2.3", Some(false)),
            ("0.0.3", "^0.0.3", Some(true)),
            ("0.0.4", "^0.0.3", Some(false)),
            ("1.2.9", "~1.2.3", Some(true)),
            ("1.3.0", "~1.2.3", Some(false)),
            ("1.0.0", "=1.0.0", Some(true)),
            ("1.0.1", "=1.0.0", Some(false)),
            ("1.5.0", ">=1, <2", Some(true)),
            ("2.0.0", ">=1, <2", Some(false)),
            ("1.9.9", "1.*", Some(true)),
            ("3.0.0", "*", Some(true)),
            ("1.2.3", ">1.2", Some(false)),
            ("1.3.0", ">1.2", Some(true)),
            ("1.2.3", "<=1.2", Some(true)),
            ("1.0.0-alpha", "1", Some(true)),
            ("1.0.0", "banana", None),
            ("x.y", "1", None),
        ] {
            assert_eq!(
                meets(version, requirement),
                expected,
                "{version} {requirement}"
            );
        }
    }

    fn conflict_of(root: &str, dep: &str) -> Severity {
        let root = package("root", Some(root));
        let dep = package("dep", Some(dep));
        conflicts(&root, &[root.clone(), dep])
            .first()
            .map_or(Severity::Compatible, |c| c.severity)
    }

    #[test]
    fn the_conflict_table() {
        use Severity::*;
        for (root, dep, expected) in [
            ("MIT", "Apache-2.0", Compatible),
            ("MIT", "GPL-3.0-only", Conflict),
            ("MIT OR Apache-2.0", "AGPL-3.0", Conflict),
            ("MIT", "MIT OR GPL-3.0", Compatible),
            ("MIT", "MIT AND GPL-3.0", Conflict),
            ("MIT", "LGPL-2.1", Check),
            ("MIT", "MPL-2.0", Check),
            ("MIT", "GPL-2.0 WITH Classpath-exception-2.0", Check),
            ("MIT", "LicenseRef-Custom", Check),
            ("LicenseRef-Proprietary", "GPL-3.0", Conflict),
            ("GPL-3.0-or-later", "MIT", Compatible),
            ("GPL-3.0-or-later", "Apache-2.0", Compatible),
            ("GPL-3.0-or-later", "LGPL-2.1", Compatible),
            ("GPL-2.0-only", "Apache-2.0", Conflict),
            ("GPL-2.0-or-later", "Apache-2.0", Compatible),
            ("GPL-2.0-only", "Apache-2.0 WITH LLVM-exception", Compatible),
            ("GPL-2.0-only", "GPL-3.0-only", Conflict),
            ("GPL-2.0-only", "GPL-2.0-or-later", Compatible),
            ("GPL-3.0-only", "AGPL-3.0-only", Check),
            ("GPL-3.0-only", "EPL-1.0", Conflict),
            ("MPL-2.0", "GPL-3.0", Conflict),
            ("MPL-2.0", "LGPL-3.0", Compatible),
            ("EUPL-1.2", "GPL-3.0", Check),
            ("GPL-3.0", "LicenseRef-X", Check),
        ] {
            assert_eq!(conflict_of(root, dep), expected, "{dep} under {root}");
        }
        // No licence at all reads as proprietary.
        let root = package("root", None);
        let found = conflicts(&root, &[package("dep", Some("GPL-3.0"))]);
        assert_eq!(found[0].severity, Severity::Conflict);
        assert_eq!(found[0].root_license, "(none)");
    }

    #[test]
    fn unpublished_crates() {
        let json = r#"{"packages": [
            {"id": "a", "publish": []},
            {"id": "b", "publish": null},
            {"id": "c", "publish": ["internal"]},
            {"id": "d", "publish": ["internal", "crates-io"]}]}"#;
        let ids = unpublished(json, &["internal".to_string()]).unwrap();
        let mut ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["a", "c"]);
        assert!(unpublished("{", &[]).is_err());
    }
}
