//! Cargo dependency graphs (#248), read from `cargo metadata`.
//!
//! [`DepGraph`] is the resolved graph from `cargo metadata --format-version
//! 1`: every package, which ones are the workspace's, and which depend on
//! which (normal, build and dev). It finds the crates resolved at more than
//! one version ([`DepGraph::duplicates`]) and the paths that pull a crate in
//! ([`DepGraph::paths_to`]).
//!
//! Two renderables draw it as a [`Tree`]:
//!
//! - [`DepTree`]: each root (the package `cargo metadata` ran in, else every
//!   workspace member) and what it depends on, as `cargo tree` prints it: a
//!   package already shown is marked `(*)` instead of repeated, build and
//!   dev dependencies sit under `[build-dependencies]` and
//!   `[dev-dependencies]`, and a crate resolved at several versions is
//!   marked `(duplicate)` (and coloured), so it reads without colour too.
//! - [`WhyTree`]: one crate and what depends on it, up to the roots: the
//!   inverted tree `cargo tree -i` prints, which is every path that pulls it
//!   in.
//!
//! Five supply-chain reports sit beside them, each reading what Cargo or
//! its tools already wrote (0.0.16 workstream 6):
//!
//! - [`duplicates`]: a consolidation summary for the crates resolved at
//!   several versions: who pulls each version, and which one most of them
//!   already use ([`DepTree::consolidation`] adds it under the tree).
//! - [`features`]: the features `cargo metadata` resolved for each crate,
//!   what each turns on, and which dependents asked for them.
//! - [`timings`]: `cargo build --timings` reports, as bars and a table.
//! - [`audit`]: a generic advisory model, read from `cargo audit --json`.
//! - [`licenses`]: licences from `cargo metadata`, grouped, with unknown and
//!   copyleft licences marked.
//!
//! Running `cargo` (or `cargo audit`) is the caller's business; this module
//! only reads its output, and reaches no network.
//!
//! ```
//! use rich::Console;
//! use rich_ext::deps::{DepGraph, DepTree};
//!
//! let metadata = r#"{
//!   "packages": [
//!     {"id": "app 0.1.0", "name": "app", "version": "0.1.0", "source": null},
//!     {"id": "log 0.4.0", "name": "log", "version": "0.4.0",
//!      "source": "registry+https://github.com/rust-lang/crates.io-index"}
//!   ],
//!   "workspace_members": ["app 0.1.0"],
//!   "resolve": {"root": "app 0.1.0", "nodes": [
//!     {"id": "app 0.1.0", "deps": [{"name": "log", "pkg": "log 0.4.0",
//!       "dep_kinds": [{"kind": null, "target": null}]}]},
//!     {"id": "log 0.4.0", "deps": []}
//!   ]}
//! }"#;
//! let graph = DepGraph::from_json(metadata).unwrap();
//! let console = Console::builder().width(40).color_system(None).build();
//! assert_eq!(
//!     console.render_to_string(&DepTree::new(graph)),
//!     "app v0.1.0\n└── log v0.4.0"
//! );
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text, Tree};
use serde_json::Value;

pub mod audit;
pub mod duplicates;
pub mod features;
pub mod licenses;
pub mod timings;

/// The largest input the supply-chain readers ([`timings`], [`audit`])
/// accept, in bytes: far past any real report, and a bound on what a
/// hostile file can make them hold.
pub const MAX_INPUT: usize = 64 * 1024 * 1024;

/// The most records (packages, timing units, advisories) a supply-chain
/// reader takes from one input; more is refused rather than truncated.
/// JSON nesting is bounded separately, by `serde_json`'s recursion limit
/// (128 levels).
pub const MAX_RECORDS: usize = 100_000;

/// Theme keys for dependency trees, with the styles used when a theme lacks
/// them.
pub const STYLES: &[(&str, &str)] = &[
    ("deps.root", "bold"),
    ("deps.name", "none"),
    ("deps.version", "cyan"),
    ("deps.duplicate", "bold yellow"),
    ("deps.source", "dim"),
    ("deps.repeat", "dim"),
    ("deps.section", "dim italic"),
    ("deps.feature", "green"),
    ("deps.off", "dim"),
    ("deps.copyleft", "bold yellow"),
    ("deps.unknown", "bold red"),
    ("deps.critical", "bold red"),
    ("deps.high", "red"),
    ("deps.medium", "yellow"),
    ("deps.low", "cyan"),
    ("deps.info", "dim"),
];

pub(crate) fn theme_style(console: &Console, key: &str) -> Style {
    if let Some(style) = console.theme().get(key) {
        return style.clone();
    }
    STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, spec)| Style::parse(spec).ok())
        .unwrap_or_default()
}

/// Why metadata could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepsError(String);

impl DepsError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        DepsError(message.into())
    }
}

/// `text` with terminal controls made visible
/// ([`sanitize_terminal_controls`](crate::sanitize_terminal_controls)).
/// The reports draw text read from files (an advisory's title, a licence
/// expression, a crate's name in `--metadata FILE`), which must not retitle
/// the terminal, clear it, or end a link early.
pub(crate) fn clean(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(|c: char| c.is_control() && c != '\n' && c != '\t') {
        std::borrow::Cow::Owned(crate::sanitize_terminal_controls(text))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Refuse an input larger than [`MAX_INPUT`].
pub(crate) fn check_size(input: &str, what: &str) -> Result<(), DepsError> {
    if input.len() > MAX_INPUT {
        return Err(DepsError(format!(
            "{what} is {} bytes, more than the {MAX_INPUT} read",
            input.len()
        )));
    }
    Ok(())
}

/// Refuse more than [`MAX_RECORDS`] records.
pub(crate) fn check_count(count: usize, what: &str) -> Result<(), DepsError> {
    if count > MAX_RECORDS {
        return Err(DepsError(format!(
            "{count} {what}, more than the {MAX_RECORDS} read"
        )));
    }
    Ok(())
}

/// Render `parts` one under the other, each starting on a new line, with a
/// blank line between them.
pub(crate) fn stack(
    parts: &[&dyn Renderable],
    console: &Console,
    options: &ConsoleOptions,
) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            if segments
                .iter()
                .rev()
                .find(|segment| !segment.text.is_empty())
                .is_some_and(|segment| !segment.text.ends_with('\n'))
            {
                segments.push(Segment::line());
            }
            segments.push(Segment::line());
        }
        segments.extend(part.rich_render(console, options));
    }
    segments
}

/// The measurement of [`stack`]ed parts: the widest of them.
pub(crate) fn stack_measure(
    parts: &[&dyn Renderable],
    console: &Console,
    options: &ConsoleOptions,
) -> rich::measure::Measurement {
    let mut minimum = 0;
    let mut maximum = 0;
    for part in parts {
        let measured = part.measure(console, options);
        minimum = minimum.max(measured.minimum);
        maximum = maximum.max(measured.maximum);
    }
    rich::measure::Measurement::new(minimum, maximum)
}

impl fmt::Display for DepsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DepsError {}

/// A package in the graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    /// Cargo's package id.
    pub id: String,
    pub name: String,
    pub version: String,
    /// Where it comes from: `None` for a path (local) package, else Cargo's
    /// source string (`registry+…`, `git+…`).
    pub source: Option<String>,
}

impl Package {
    /// Where it comes from, when that is worth saying: `(path)` for a local
    /// package outside the workspace, `(git)` for a git one, the registry's
    /// URL for one that is not crates.io; `None` for crates.io.
    fn origin(&self) -> Option<&'static str> {
        match self.source.as_deref() {
            None => Some("(path)"),
            Some(source) if source.starts_with("git+") => Some("(git)"),
            Some(source)
                if source.contains("crates.io-index")
                    || source.starts_with("sparse+https://index.crates.io") =>
            {
                None
            }
            Some(_) => Some("(registry)"),
        }
    }
}

/// What a dependency is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DepKind {
    Normal,
    Build,
    Dev,
}

impl DepKind {
    fn section(self) -> Option<&'static str> {
        match self {
            DepKind::Normal => None,
            DepKind::Build => Some("[build-dependencies]"),
            DepKind::Dev => Some("[dev-dependencies]"),
        }
    }
}

/// An edge: a dependency on [`package`](Dep::package), by index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    pub package: usize,
    /// The kinds it is used as, sorted; never empty.
    pub kinds: Vec<DepKind>,
}

/// The resolved dependency graph of a workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepGraph {
    packages: Vec<Package>,
    deps: Vec<Vec<Dep>>,
    roots: Vec<usize>,
    members: Vec<usize>,
    /// Per package, whether it is one of `roots` / `members`: the walks
    /// ask for every package, and a scan of the lists each time is
    /// quadratic in a large workspace.
    is_root: Vec<bool>,
    is_member: Vec<bool>,
}

/// `indices` as one flag per package.
fn flags(count: usize, indices: &[usize]) -> Vec<bool> {
    let mut flags = vec![false; count];
    for &index in indices {
        if let Some(flag) = flags.get_mut(index) {
            *flag = true;
        }
    }
    flags
}

impl DepGraph {
    /// Read `cargo metadata --format-version 1` output.
    pub fn from_json(json: &str) -> Result<Self, DepsError> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| DepsError(format!("not cargo metadata JSON: {e}")))?;
        Self::from_value(&value)
    }

    /// [`DepGraph::from_json`] for parsed JSON.
    pub fn from_value(value: &Value) -> Result<Self, DepsError> {
        let missing = |what: &str| DepsError(format!("not cargo metadata: no `{what}`"));
        let packages_json = value
            .get("packages")
            .and_then(Value::as_array)
            .ok_or_else(|| missing("packages"))?;
        let mut packages = Vec::with_capacity(packages_json.len());
        let mut index = HashMap::new();
        for package in packages_json {
            let field = |key: &str| package.get(key).and_then(Value::as_str);
            let (Some(id), Some(name), Some(version)) =
                (field("id"), field("name"), field("version"))
            else {
                return Err(DepsError(
                    "not cargo metadata: a package has no id, name or version".into(),
                ));
            };
            index.insert(id.to_string(), packages.len());
            packages.push(Package {
                id: id.to_string(),
                name: name.to_string(),
                version: version.to_string(),
                source: field("source").map(str::to_string),
            });
        }
        let lookup = |id: &str| {
            index
                .get(id)
                .copied()
                .ok_or_else(|| DepsError(format!("cargo metadata names an unknown package {id:?}")))
        };
        let members = value
            .get("workspace_members")
            .and_then(Value::as_array)
            .ok_or_else(|| missing("workspace_members"))?
            .iter()
            .filter_map(Value::as_str)
            .map(lookup)
            .collect::<Result<Vec<_>, _>>()?;
        let resolve = value
            .get("resolve")
            .filter(|r| !r.is_null())
            .ok_or_else(|| {
                DepsError("cargo metadata has no `resolve`: it was run with --no-deps".into())
            })?;
        let mut deps = vec![Vec::new(); packages.len()];
        for node in resolve
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| missing("resolve.nodes"))?
        {
            let from = lookup(node.get("id").and_then(Value::as_str).unwrap_or_default())?;
            let mut edges: Vec<Dep> = Vec::new();
            if let Some(list) = node.get("deps").and_then(Value::as_array) {
                for dep in list {
                    let to = lookup(dep.get("pkg").and_then(Value::as_str).unwrap_or_default())?;
                    let mut kinds: Vec<DepKind> = dep
                        .get("dep_kinds")
                        .and_then(Value::as_array)
                        .map(|kinds| {
                            kinds
                                .iter()
                                .map(|kind| match kind.get("kind").and_then(Value::as_str) {
                                    Some("build") => DepKind::Build,
                                    Some("dev") => DepKind::Dev,
                                    _ => DepKind::Normal,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    if kinds.is_empty() {
                        kinds.push(DepKind::Normal);
                    }
                    kinds.sort();
                    kinds.dedup();
                    edges.push(Dep { package: to, kinds });
                }
            } else if let Some(list) = node.get("dependencies").and_then(Value::as_array) {
                // Before Cargo 1.30 nodes listed ids only.
                for id in list.iter().filter_map(Value::as_str) {
                    edges.push(Dep {
                        package: lookup(id)?,
                        kinds: vec![DepKind::Normal],
                    });
                }
            }
            edges.sort_by(|a, b| {
                let (a, b) = (&packages[a.package], &packages[b.package]);
                (&a.name, &a.version).cmp(&(&b.name, &b.version))
            });
            deps[from] = edges;
        }
        let roots = match resolve.get("root").and_then(Value::as_str) {
            Some(root) => vec![lookup(root)?],
            None => {
                let mut roots = members.clone();
                roots.sort_by(|&a, &b| packages[a].name.cmp(&packages[b].name));
                roots
            }
        };
        Ok(DepGraph {
            is_root: flags(packages.len(), &roots),
            is_member: flags(packages.len(), &members),
            packages,
            deps,
            roots,
            members,
        })
    }

    /// Every package, in `cargo metadata`'s order.
    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// What `package` depends on, sorted by name and version.
    pub fn deps(&self, package: usize) -> &[Dep] {
        &self.deps[package]
    }

    /// The trees' roots: the package `cargo metadata` ran in, else every
    /// workspace member, by name.
    pub fn roots(&self) -> &[usize] {
        &self.roots
    }

    /// Whether `package` is a workspace member.
    pub fn is_member(&self, package: usize) -> bool {
        self.is_member.get(package).copied().unwrap_or(false)
    }

    /// Whether `package` is one of the [`roots`](Self::roots).
    pub fn is_root(&self, package: usize) -> bool {
        self.is_root.get(package).copied().unwrap_or(false)
    }

    /// Show these packages as the roots instead.
    pub fn set_roots(&mut self, roots: Vec<usize>) {
        self.is_root = flags(self.packages.len(), &roots);
        self.roots = roots;
    }

    /// The packages reachable from the roots, through the kinds in `kinds`
    /// (dev dependencies only from the roots themselves, as Cargo resolves
    /// them). A root keeps its dev dependencies however the walk reaches it,
    /// through another root included.
    pub fn reachable(&self, kinds: &[DepKind]) -> Vec<bool> {
        let mut seen = vec![false; self.packages.len()];
        let mut stack: Vec<usize> = self.roots.clone();
        while let Some(package) = stack.pop() {
            if std::mem::replace(&mut seen[package], true) {
                continue;
            }
            let root = self.is_root(package);
            for dep in &self.deps[package] {
                if self.follows(dep, root, kinds) {
                    stack.push(dep.package);
                }
            }
        }
        seen
    }

    /// Whether a walk through `kinds` follows `dep` from a package (a root,
    /// or not): dev dependencies count only from the roots.
    fn follows(&self, dep: &Dep, root: bool, kinds: &[DepKind]) -> bool {
        dep.kinds
            .iter()
            .any(|k| kinds.contains(k) && (root || *k != DepKind::Dev))
    }

    /// Crates resolved at more than one version (among the packages
    /// reachable from the roots), by name, each with its versions in order.
    pub fn duplicates(&self) -> BTreeMap<String, Vec<String>> {
        self.duplicates_in(&[DepKind::Normal, DepKind::Build, DepKind::Dev])
    }

    /// [`DepGraph::duplicates`] among the packages reachable through the
    /// kinds in `kinds` (as [`DepGraph::reachable`] follows them).
    pub fn duplicates_in(&self, kinds: &[DepKind]) -> BTreeMap<String, Vec<String>> {
        let reachable = self.reachable(kinds);
        let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (package, _) in self.packages.iter().zip(&reachable).filter(|(_, r)| **r) {
            versions
                .entry(package.name.clone())
                .or_default()
                .push(package.version.clone());
        }
        versions.retain(|_, list| {
            list.sort_by(|a, b| compare_versions(a, b));
            list.dedup();
            list.len() > 1
        });
        versions
    }

    /// The packages `spec` names: `name`, or `name@version`.
    pub fn find(&self, spec: &str) -> Vec<usize> {
        let (name, version) = match spec.split_once('@') {
            Some((name, version)) => (name, Some(version.trim_start_matches('v'))),
            None => (spec, None),
        };
        let mut found: Vec<usize> = (0..self.packages.len())
            .filter(|&i| {
                let package = &self.packages[i];
                package.name == name && version.is_none_or(|v| package.version == v)
            })
            .collect();
        found.sort_by(|&a, &b| {
            compare_versions(&self.packages[a].version, &self.packages[b].version)
        });
        found
    }

    /// What depends on each package, the inverse of [`DepGraph::deps`]:
    /// only the edges [`DepGraph::reachable`] follows through `kinds` (dev
    /// dependencies only from the roots), from packages it reaches, each
    /// with the kinds it is used as among those.
    pub fn dependents(&self, kinds: &[DepKind]) -> Vec<Vec<(usize, Vec<DepKind>)>> {
        let reachable = self.reachable(kinds);
        let mut dependents = vec![Vec::new(); self.packages.len()];
        for (from, deps) in self.deps.iter().enumerate() {
            if !reachable[from] {
                continue;
            }
            let root = self.is_root(from);
            for dep in deps {
                let used: Vec<DepKind> = dep
                    .kinds
                    .iter()
                    .copied()
                    .filter(|k| kinds.contains(k) && (root || *k != DepKind::Dev))
                    .collect();
                if !used.is_empty() {
                    dependents[dep.package].push((from, used));
                }
            }
        }
        for list in &mut dependents {
            list.sort_by(|a, b| self.packages[a.0].name.cmp(&self.packages[b.0].name));
        }
        dependents
    }

    /// Up to `limit` paths from a root to `target` through the kinds in
    /// `kinds` (as [`DepGraph::dependents`] follows them), each root first,
    /// shortest first. Every path is simple (no package twice).
    pub fn paths_to(&self, target: usize, limit: usize, kinds: &[DepKind]) -> Vec<Vec<usize>> {
        let dependents = self.dependents(kinds);
        // Breadth-first from the target up, so shorter paths come first.
        let mut paths = Vec::new();
        let mut queue = std::collections::VecDeque::from([vec![target]]);
        while let Some(path) = queue.pop_front() {
            if paths.len() >= limit || queue.len() > 100_000 {
                break;
            }
            let top = *path.last().expect("paths are never empty");
            if self.is_root(top) {
                let mut found = path.clone();
                found.reverse();
                paths.push(found);
                continue;
            }
            for (parent, _) in &dependents[top] {
                if !path.contains(parent) {
                    let mut longer = path.clone();
                    longer.push(*parent);
                    queue.push_back(longer);
                }
            }
        }
        paths
    }

    /// `name v1.2.3`.
    pub fn display(&self, package: usize) -> String {
        let package = &self.packages[package];
        format!("{} v{}", clean(&package.name), clean(&package.version))
    }
}

/// Compare versions by SemVer precedence: the dotted core numerically where
/// its parts are numbers, then a release above any of its pre-releases, then
/// pre-release identifiers (numeric ones numerically and below alphanumeric
/// ones). Build metadata takes no part, except to break a tie, so the order
/// stays total.
pub(crate) fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let split = |v: &str| -> (Vec<(u64, String)>, Option<String>) {
        let v = v.split_once('+').map_or(v, |(v, _)| v);
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (v, None),
        };
        let core = core
            .split('.')
            .map(|part| (part.parse().unwrap_or(u64::MAX), part.to_string()))
            .collect();
        (core, pre)
    };
    let identifier = |x: &str, y: &str| match (x.parse::<u64>(), y.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => x.cmp(y),
    };
    let ((a_core, a_pre), (b_core, b_pre)) = (split(a), split(b));
    a_core
        .cmp(&b_core)
        .then_with(|| match (&a_pre, &b_pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => {
                let (x, y): (Vec<&str>, Vec<&str>) =
                    (x.split('.').collect(), y.split('.').collect());
                x.iter()
                    .zip(&y)
                    .map(|(x, y)| identifier(x, y))
                    .find(|o| o.is_ne())
                    .unwrap_or_else(|| x.len().cmp(&y.len()))
            }
        })
        .then_with(|| a.cmp(b))
}

/// The deepest a [`DepTree`] or [`WhyTree`] is drawn: below it, a branch
/// ends in a note. Real graphs are far shallower; this keeps a pathological
/// one from exhausting the stack.
pub const MAX_TREE_DEPTH: usize = 128;

/// The note that ends a branch cut at [`MAX_TREE_DEPTH`].
fn cut(console: &Console) -> Text {
    Text::styled(
        format!("… (deeper levels not shown: the tree stops at {MAX_TREE_DEPTH} levels)"),
        theme_style(console, "deps.section"),
    )
}

/// The label of one package in a tree.
fn label(
    graph: &DepGraph,
    package: usize,
    duplicates: &HashSet<String>,
    repeated: bool,
    console: &Console,
) -> Text {
    let info = &graph.packages[package];
    let mut text = Text::new("");
    let duplicate = duplicates.contains(&info.name);
    let name_style = if duplicate {
        theme_style(console, "deps.duplicate")
    } else if graph.is_member(package) {
        theme_style(console, "deps.root")
    } else {
        theme_style(console, "deps.name")
    };
    text.append(&clean(&info.name), Some(name_style.into()));
    text.append(" ", None);
    let version_style = if duplicate {
        theme_style(console, "deps.duplicate")
    } else {
        theme_style(console, "deps.version")
    };
    text.append(
        &format!("v{}", clean(&info.version)),
        Some(version_style.into()),
    );
    if let Some(origin) = info.origin() {
        if !(origin == "(path)" && graph.is_member(package)) {
            text.append(" ", None);
            text.append(origin, Some(theme_style(console, "deps.source").into()));
        }
    }
    if duplicate {
        text.append(
            " (duplicate)",
            Some(theme_style(console, "deps.duplicate").into()),
        );
    }
    if repeated {
        text.append(" (*)", Some(theme_style(console, "deps.repeat").into()));
    }
    text
}

fn section(name: &str, console: &Console) -> Tree {
    Tree::new(Text::styled(name, theme_style(console, "deps.section")))
}

/// A dependency tree: each root and what it depends on. See the
/// [module docs](self).
#[derive(Clone, Debug)]
pub struct DepTree {
    graph: DepGraph,
    max_depth: Option<usize>,
    kinds: Vec<DepKind>,
    duplicates_only: bool,
    summary: bool,
    consolidation: bool,
}

impl DepTree {
    pub fn new(graph: DepGraph) -> Self {
        DepTree {
            graph,
            max_depth: None,
            kinds: vec![DepKind::Normal, DepKind::Build, DepKind::Dev],
            duplicates_only: false,
            summary: false,
            consolidation: false,
        }
    }

    /// Under the tree (and the [`summary`](Self::summary)), after a blank
    /// line, the [`duplicates::Consolidation`] of the crates resolved at
    /// several versions, through the same kinds. Nothing is added when
    /// there are none.
    pub fn consolidation(mut self, consolidation: bool) -> Self {
        self.consolidation = consolidation;
        self
    }

    /// The consolidation summary, when asked for and there is something to
    /// consolidate.
    fn consolidation_view(&self) -> Option<duplicates::Consolidation> {
        if !self.consolidation {
            return None;
        }
        let view = duplicates::Consolidation::new(&self.graph).kinds(&self.kinds);
        (!view.duplicates().is_empty()).then_some(view)
    }

    /// Under the tree, list each crate resolved at several versions
    /// (`duplicates: syn v1.0.109, v2.0.79`), or say there are none.
    pub fn summary(mut self, summary: bool) -> Self {
        self.summary = summary;
        self
    }

    /// The summary lines [`DepTree::summary`] adds.
    fn summary_text(&self, console: &Console) -> Text {
        let duplicates = self.graph.duplicates_in(&self.kinds);
        let mut text = Text::new("");
        if duplicates.is_empty() {
            text.append(
                "no crate is resolved at more than one version",
                Some(theme_style(console, "deps.section").into()),
            );
            return text;
        }
        let style = theme_style(console, "deps.duplicate");
        for (index, (name, versions)) in duplicates.iter().enumerate() {
            if index > 0 {
                text.append("\n", None);
            }
            let versions: Vec<String> = versions.iter().map(|v| format!("v{}", clean(v))).collect();
            text.append(
                "duplicate: ",
                Some(theme_style(console, "deps.section").into()),
            );
            text.append(&clean(name), Some(style.clone().into()));
            text.append(&format!(" {}", versions.join(", ")), None);
        }
        text
    }

    /// Show at most this many levels below each root (`cargo tree --depth`).
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Which kinds of dependency to follow (all by default).
    pub fn kinds(mut self, kinds: &[DepKind]) -> Self {
        self.kinds = kinds.to_vec();
        self
    }

    /// Keep only the branches that lead to a duplicated crate.
    pub fn duplicates_only(mut self, only: bool) -> Self {
        self.duplicates_only = only;
        self
    }

    pub fn graph(&self) -> &DepGraph {
        &self.graph
    }

    /// Build the [`Tree`] this renders as.
    pub fn tree(&self, console: &Console) -> Tree {
        let duplicates: HashSet<String> =
            self.graph.duplicates_in(&self.kinds).into_keys().collect();
        // Which packages lead to a duplicate, for `duplicates_only`.
        let leads = if self.duplicates_only {
            Some(self.leads_to_duplicate(&duplicates))
        } else {
            None
        };
        let mut shown = HashSet::new();
        let mut trees: Vec<Tree> = Vec::new();
        for &root in &self.graph.roots {
            if leads.as_ref().is_some_and(|l| l[root] != Some(true)) {
                continue;
            }
            trees.push(self.subtree(
                root,
                0,
                true,
                &duplicates,
                leads.as_deref(),
                &mut shown,
                console,
            ));
        }
        if trees.len() == 1 {
            return trees.pop().expect("one tree");
        }
        let mut tree = Tree::new("").hide_root(true);
        for subtree in trees {
            tree.add_tree(subtree);
        }
        tree
    }

    /// For each package, whether it is a duplicate or leads to one through
    /// the edges the tree follows: a walk back from the duplicates, without
    /// recursion, so a deep graph cannot exhaust the stack.
    fn leads_to_duplicate(&self, duplicates: &HashSet<String>) -> Vec<Option<bool>> {
        let count = self.graph.packages.len();
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (from, deps) in self.graph.deps.iter().enumerate() {
            let root = self.graph.is_root(from);
            for dep in deps {
                if self.graph.follows(dep, root, &self.kinds) {
                    dependents[dep.package].push(from);
                }
            }
        }
        let mut leads = vec![Some(false); count];
        let mut stack: Vec<usize> = (0..count)
            .filter(|&p| duplicates.contains(&self.graph.packages[p].name))
            .collect();
        while let Some(package) = stack.pop() {
            if leads[package] == Some(true) {
                continue;
            }
            leads[package] = Some(true);
            stack.extend(&dependents[package]);
        }
        leads
    }

    #[allow(clippy::too_many_arguments)]
    fn subtree(
        &self,
        package: usize,
        depth: usize,
        root: bool,
        duplicates: &HashSet<String>,
        leads: Option<&[Option<bool>]>,
        shown: &mut HashSet<(usize, bool)>,
        console: &Console,
    ) -> Tree {
        let deps: Vec<&Dep> = self.graph.deps[package]
            .iter()
            .filter(|dep| leads.is_none_or(|l| l[dep.package] == Some(true)))
            .collect();
        let expandable = !deps.is_empty() && self.max_depth.is_none_or(|max| depth < max);
        // A package is expanded once as a root (with its dev dependencies)
        // and once elsewhere; expanded as a root covers both.
        let repeated = expandable && !shown.insert((package, root));
        if root {
            shown.insert((package, false));
        }
        let mut tree = Tree::new(label(&self.graph, package, duplicates, repeated, console));
        if !expandable || repeated {
            return tree;
        }
        if depth >= MAX_TREE_DEPTH {
            tree.add(cut(console));
            return tree;
        }
        for kind in [DepKind::Normal, DepKind::Build, DepKind::Dev] {
            if !self.kinds.contains(&kind) || (kind == DepKind::Dev && !root) {
                continue;
            }
            let children: Vec<Tree> = deps
                .iter()
                .filter(|dep| dep.kinds.contains(&kind))
                .map(|dep| {
                    self.subtree(
                        dep.package,
                        depth + 1,
                        false,
                        duplicates,
                        leads,
                        shown,
                        console,
                    )
                })
                .collect();
            if children.is_empty() {
                continue;
            }
            match kind.section() {
                None => {
                    for child in children {
                        tree.add_tree(child);
                    }
                }
                Some(name) => {
                    let mut group = section(name, console);
                    for child in children {
                        group.add_tree(child);
                    }
                    tree.add_tree(group);
                }
            }
        }
        tree
    }
}

impl Renderable for DepTree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = self.tree(console).rich_render(console, options);
        if self.summary {
            if segments
                .iter()
                .rev()
                .find(|segment| !segment.text.is_empty())
                .is_some_and(|segment| !segment.text.ends_with('\n'))
            {
                segments.push(Segment::line());
            }
            segments.extend(self.summary_text(console).rich_render(console, options));
        }
        if let Some(view) = self.consolidation_view() {
            segments = stack(&[&Prerendered(segments), &view], console, options);
        }
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let mut measured = self.tree(console).measure(console, options);
        if self.summary {
            let summary = self.summary_text(console).measure(console, options);
            measured = rich::measure::Measurement::new(
                measured.minimum.max(summary.minimum),
                measured.maximum.max(summary.maximum),
            );
        }
        if let Some(view) = self.consolidation_view() {
            let view = view.measure(console, options);
            measured = rich::measure::Measurement::new(
                measured.minimum.max(view.minimum),
                measured.maximum.max(view.maximum),
            );
        }
        measured
    }
}

/// Segments already rendered, for [`stack`].
struct Prerendered(Vec<Segment>);

impl Renderable for Prerendered {
    fn rich_render(&self, _console: &Console, _options: &ConsoleOptions) -> Vec<Segment> {
        self.0.clone()
    }
}

/// One crate and everything that depends on it, up to the roots (`cargo
/// tree -i`). See the [module docs](self).
#[derive(Clone, Debug)]
pub struct WhyTree {
    graph: DepGraph,
    targets: Vec<usize>,
    kinds: Vec<DepKind>,
}

impl WhyTree {
    /// The crates `spec` names (`name` or `name@version`); an error when the
    /// graph has none.
    pub fn new(graph: DepGraph, spec: &str) -> Result<Self, DepsError> {
        let targets = graph.find(spec);
        if targets.is_empty() {
            return Err(DepsError(format!(
                "no package {spec:?} in the dependency graph"
            )));
        }
        Ok(WhyTree {
            graph,
            targets,
            kinds: vec![DepKind::Normal, DepKind::Build, DepKind::Dev],
        })
    }

    /// Which kinds of dependency to follow up (all by default); dev
    /// dependencies count only from the roots, as in [`DepTree`]. A target
    /// those kinds never reach is left out, as `cargo tree -i -e` does.
    pub fn kinds(mut self, kinds: &[DepKind]) -> Self {
        self.kinds = kinds.to_vec();
        let reachable = self.graph.reachable(&self.kinds);
        self.targets.retain(|&target| reachable[target]);
        self
    }

    /// The packages asked about.
    pub fn targets(&self) -> &[usize] {
        &self.targets
    }

    pub fn graph(&self) -> &DepGraph {
        &self.graph
    }

    /// Build the [`Tree`] this renders as.
    pub fn tree(&self, console: &Console) -> Tree {
        let duplicates: HashSet<String> =
            self.graph.duplicates_in(&self.kinds).into_keys().collect();
        let dependents = self.graph.dependents(&self.kinds);
        let mut shown = HashSet::new();
        let mut trees: Vec<Tree> = self
            .targets
            .iter()
            .map(|&target| {
                self.up(
                    target,
                    None,
                    0,
                    &dependents,
                    &duplicates,
                    &mut shown,
                    console,
                )
            })
            .collect();
        if trees.len() == 1 {
            return trees.pop().expect("one tree");
        }
        let mut tree = Tree::new("").hide_root(true);
        for subtree in trees {
            tree.add_tree(subtree);
        }
        tree
    }

    #[allow(clippy::too_many_arguments)]
    fn up(
        &self,
        package: usize,
        via: Option<&[DepKind]>,
        depth: usize,
        dependents: &[Vec<(usize, Vec<DepKind>)>],
        duplicates: &HashSet<String>,
        shown: &mut HashSet<usize>,
        console: &Console,
    ) -> Tree {
        let parents = &dependents[package];
        let root = self.graph.is_root(package);
        let repeated = !parents.is_empty() && !root && !shown.insert(package);
        let mut text = label(&self.graph, package, duplicates, repeated, console);
        if let Some(kinds) = via.filter(|kinds| !kinds.contains(&DepKind::Normal)) {
            let names: Vec<&str> = kinds
                .iter()
                .map(|kind| match kind {
                    DepKind::Build => "build",
                    DepKind::Dev => "dev",
                    DepKind::Normal => "normal",
                })
                .collect();
            text.append(
                &format!(" [{}]", names.join(", ")),
                Some(theme_style(console, "deps.section").into()),
            );
        }
        let mut tree = Tree::new(text);
        if repeated || root {
            return tree;
        }
        if depth >= MAX_TREE_DEPTH {
            tree.add(cut(console));
            return tree;
        }
        for (parent, kinds) in parents {
            tree.add_tree(self.up(
                *parent,
                Some(kinds),
                depth + 1,
                dependents,
                duplicates,
                shown,
                console,
            ));
        }
        tree
    }
}

impl Renderable for WhyTree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.tree(console).rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.tree(console).measure(console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_by_semver_precedence() {
        use std::cmp::Ordering::*;
        assert_eq!(compare_versions("1.0.0-alpha", "1.0.0"), Less);
        assert_eq!(compare_versions("1.0.0-alpha", "1.0.0-alpha.1"), Less);
        assert_eq!(compare_versions("1.0.0-alpha.1", "1.0.0-alpha.beta"), Less);
        assert_eq!(compare_versions("1.0.0-beta.2", "1.0.0-beta.11"), Less);
        assert_eq!(compare_versions("1.0.0-rc.1", "1.0.0"), Less);
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Greater);
        assert_eq!(compare_versions("1.0.0", "1.0.0"), Equal);
        assert_ne!(compare_versions("1.0.0+a", "1.0.0+b"), Equal);
    }

    fn metadata() -> String {
        let pkg = |name: &str, version: &str, source: Option<&str>| {
            serde_json::json!({
                "id": format!("{name} {version}"),
                "name": name,
                "version": version,
                "source": source,
            })
        };
        let crates_io = Some("registry+https://github.com/rust-lang/crates.io-index");
        let dep = |id: &str, kind: Option<&str>| {
            serde_json::json!({"name": id.split(' ').next(), "pkg": id,
                "dep_kinds": [{"kind": kind, "target": null}]})
        };
        serde_json::json!({
            "packages": [
                pkg("app", "0.1.0", None),
                pkg("syn", "1.0.109", crates_io),
                pkg("syn", "2.0.0", crates_io),
                pkg("serde", "1.0.0", crates_io),
                pkg("cc", "1.0.0", crates_io),
                pkg("insta", "1.0.0", crates_io),
            ],
            "workspace_members": ["app 0.1.0"],
            "resolve": {"root": null, "nodes": [
                {"id": "app 0.1.0", "deps": [
                    dep("serde 1.0.0", None), dep("syn 2.0.0", None),
                    dep("cc 1.0.0", Some("build")), dep("insta 1.0.0", Some("dev"))]},
                {"id": "serde 1.0.0", "deps": [dep("syn 1.0.109", None)]},
                {"id": "syn 1.0.109", "deps": []},
                {"id": "syn 2.0.0", "deps": []},
                {"id": "cc 1.0.0", "deps": []},
                {"id": "insta 1.0.0", "deps": [dep("serde 1.0.0", None)]},
            ]}
        })
        .to_string()
    }

    fn render(r: &dyn Renderable) -> String {
        Console::builder()
            .width(60)
            .color_system(None)
            .build()
            .render_to_string(r)
    }

    #[test]
    fn tree_sections_duplicates_and_repeats() {
        let graph = DepGraph::from_json(&metadata()).unwrap();
        assert_eq!(
            graph.duplicates(),
            BTreeMap::from([("syn".to_string(), vec!["1.0.109".into(), "2.0.0".into()])])
        );
        assert_eq!(
            render(&DepTree::new(graph)),
            "app v0.1.0\n\
             ├── serde v1.0.0\n\
             │   └── syn v1.0.109 (duplicate)\n\
             ├── syn v2.0.0 (duplicate)\n\
             ├── [build-dependencies]\n\
             │   └── cc v1.0.0\n\
             └── [dev-dependencies]\n    \
                 └── insta v1.0.0\n        \
                     └── serde v1.0.0 (*)"
        );
    }

    #[test]
    fn why_inverts_the_tree() {
        let graph = DepGraph::from_json(&metadata()).unwrap();
        let serde = graph.find("serde")[0];
        assert_eq!(
            graph
                .paths_to(serde, 10, &[DepKind::Normal, DepKind::Dev])
                .len(),
            2
        );
        let why = WhyTree::new(graph, "syn@1.0.109").unwrap();
        assert_eq!(
            render(&why),
            "syn v1.0.109 (duplicate)\n\
             └── serde v1.0.0\n    \
                 ├── app v0.1.0\n    \
                 └── insta v1.0.0\n        \
                     └── app v0.1.0 [dev]"
        );
    }

    #[test]
    fn duplicates_only_keeps_their_branches() {
        let graph = DepGraph::from_json(&metadata()).unwrap();
        let out = render(&DepTree::new(graph).duplicates_only(true));
        assert!(!out.contains("cc v1.0.0"), "{out}");
        assert!(out.contains("syn v2.0.0"), "{out}");
    }

    #[test]
    fn why_follows_only_the_selected_kinds() {
        let graph = DepGraph::from_json(&metadata()).unwrap();
        let serde = graph.find("serde")[0];
        let no_dev = [DepKind::Normal, DepKind::Build];
        assert_eq!(graph.paths_to(serde, 10, &no_dev).len(), 1);
        let why = WhyTree::new(graph, "serde").unwrap().kinds(&no_dev);
        assert_eq!(render(&why), "serde v1.0.0\n└── app v0.1.0");
    }

    /// [`metadata`] plus `log` at two versions: 0.4.0 a normal dependency of
    /// `app`, 0.3.0 only through the dev dependency `insta`.
    fn metadata_with_dev_only_duplicate() -> String {
        let mut value: Value = serde_json::from_str(&metadata()).unwrap();
        let crates_io = "registry+https://github.com/rust-lang/crates.io-index";
        let packages = value["packages"].as_array_mut().unwrap();
        for version in ["0.3.0", "0.4.0"] {
            packages.push(
                serde_json::json!({"id": format!("log {version}"), "name": "log",
                "version": version, "source": crates_io}),
            );
        }
        let dep = |id: &str| {
            serde_json::json!({"name": "log", "pkg": id,
                "dep_kinds": [{"kind": null, "target": null}]})
        };
        let nodes = value["resolve"]["nodes"].as_array_mut().unwrap();
        nodes[0]["deps"]
            .as_array_mut()
            .unwrap()
            .push(dep("log 0.4.0"));
        nodes[5]["deps"]
            .as_array_mut()
            .unwrap()
            .push(dep("log 0.3.0"));
        nodes.push(serde_json::json!({"id": "log 0.3.0", "deps": []}));
        nodes.push(serde_json::json!({"id": "log 0.4.0", "deps": []}));
        value.to_string()
    }

    #[test]
    fn why_leaves_out_targets_the_kinds_never_reach() {
        let graph = DepGraph::from_json(&metadata_with_dev_only_duplicate()).unwrap();
        let no_dev = [DepKind::Normal, DepKind::Build];
        // log 0.3.0 comes only through the dev dependency `insta`.
        let why = WhyTree::new(graph.clone(), "log@0.3.0")
            .unwrap()
            .kinds(&no_dev);
        assert!(why.targets().is_empty());
        let why = WhyTree::new(graph, "log").unwrap().kinds(&no_dev);
        assert_eq!(why.targets().len(), 1);
        assert!(render(&why).starts_with("log v0.4.0"), "{}", render(&why));
    }

    #[test]
    fn duplicates_follow_the_selected_kinds() {
        let graph = DepGraph::from_json(&metadata_with_dev_only_duplicate()).unwrap();
        assert!(graph.duplicates().contains_key("log"));
        let no_dev = [DepKind::Normal, DepKind::Build];
        assert!(!graph.duplicates_in(&no_dev).contains_key("log"));
        let out = render(&DepTree::new(graph.clone()).kinds(&no_dev));
        assert!(out.contains("log v0.4.0\n"), "{out}");
        assert!(!out.contains("log v0.4.0 (duplicate)"), "{out}");
        let out = render(&DepTree::new(graph).kinds(&no_dev).duplicates_only(true));
        assert!(!out.contains("log v"), "{out}");
    }

    #[test]
    fn bad_input_says_why() {
        assert!(DepGraph::from_json("[]")
            .unwrap_err()
            .to_string()
            .contains("no `packages`"));
        let no_deps = r#"{"packages": [], "workspace_members": [], "resolve": null}"#;
        assert!(DepGraph::from_json(no_deps)
            .unwrap_err()
            .to_string()
            .contains("--no-deps"));
        let graph = DepGraph::from_json(&metadata()).unwrap();
        assert!(WhyTree::new(graph, "nope").is_err());
    }
}
