//! Feature trees (#419): the features `cargo metadata` resolved for each
//! crate, what each one turns on, and which dependents asked for them.
//!
//! [`FeatureGraph`] reads `cargo metadata --format-version 1`:
//! `resolve.nodes[].features` (the features Cargo enabled, unified across
//! the build), each package's `features` table (what a feature turns on) and
//! its `dependencies` (which dependent asked for which features, with or
//! without the defaults). [`FeatureTree`] draws one crate, or several:
//!
//! ```text
//! serde v1.0.210  4 features enabled
//! ├── default
//! │   └── std
//! ├── derive
//! │   └── serde_derive
//! ├── serde_derive
//! │   └── dep:serde_derive → serde_derive v1.0.210
//! ├── std
//! └── requested by
//!     ├── demo-app v0.3.0: default features, derive
//!     └── demo-core v0.3.0: default features, derive
//!         └── its feature std: std
//! ```
//!
//! Under each enabled feature is its definition, entry by entry: another
//! feature, an optional dependency (`dep:name`) and the package it resolved
//! to, or a dependency's feature (`name/feature`; `name?/feature` only when
//! that dependency is on for another reason). An entry Cargo did not act on
//! is marked `(off)`. "requested by" lists every dependent with the features
//! its `Cargo.toml` asks for, and its own enabled features that turn on more.
//!
//! ```
//! use rich::Console;
//! use rich_ext::deps::features::{FeatureGraph, FeatureTree};
//!
//! let metadata = r#"{
//!   "packages": [
//!     {"id": "app 0.1.0", "name": "app", "version": "0.1.0", "source": null,
//!      "features": {},
//!      "dependencies": [{"name": "log", "features": ["std"],
//!                        "uses_default_features": false, "optional": false}]},
//!     {"id": "log 0.4.0", "name": "log", "version": "0.4.0", "source": null,
//!      "features": {"std": [], "kv": []}, "dependencies": []}
//!   ],
//!   "workspace_members": ["app 0.1.0"],
//!   "resolve": {"root": "app 0.1.0", "nodes": [
//!     {"id": "app 0.1.0", "features": [],
//!      "deps": [{"name": "log", "pkg": "log 0.4.0", "dep_kinds": [{"kind": null}]}]},
//!     {"id": "log 0.4.0", "features": ["std"], "deps": []}
//!   ]}
//! }"#;
//! let graph = FeatureGraph::from_json(metadata).unwrap();
//! let tree = FeatureTree::new(graph, Some("log")).unwrap();
//! let console = Console::builder().width(60).color_system(None).build();
//! assert_eq!(
//!     console.render_to_string(&tree),
//!     "log v0.4.0  1 feature enabled\n\
//!      ├── std\n\
//!      └── requested by\n    \
//!          └── app v0.1.0: std"
//! );
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rich::{Console, ConsoleOptions, Renderable, Segment, Text, Tree};
use serde_json::Value;

use super::{check_count, clean, compare_versions, stack, stack_measure, theme_style, DepsError};

/// A dependency as a package's `Cargo.toml` declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredDep {
    /// The package's name.
    pub name: String,
    /// `package = …` renames: the key in `[dependencies]`.
    pub rename: Option<String>,
    pub optional: bool,
    /// `features = […]`.
    pub features: Vec<String>,
    /// `default-features` (true unless turned off).
    pub default_features: bool,
}

impl DeclaredDep {
    /// The key a feature names it by: the rename, else the name.
    pub fn key(&self) -> &str {
        self.rename.as_deref().unwrap_or(&self.name)
    }

    /// The name the resolve graph gives it (`-` becomes `_`).
    fn extern_name(&self) -> String {
        self.key().replace('-', "_")
    }
}

/// One crate's features.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrateFeatures {
    /// Cargo's package id.
    pub id: String,
    pub name: String,
    pub version: String,
    /// The features Cargo enabled, `default` first, then by name.
    pub enabled: Vec<String>,
    /// The `[features]` table: what each feature turns on.
    pub definitions: BTreeMap<String, Vec<String>>,
    /// The dependencies its manifest declares.
    pub declared: Vec<DeclaredDep>,
    /// The packages it resolved to, by the resolve graph's names for them
    /// (`name`, with `-` as `_`, or the rename): index into
    /// [`FeatureGraph::crates`].
    pub resolved: Vec<(String, usize)>,
}

impl CrateFeatures {
    /// `name v1.2.3`.
    pub fn display(&self) -> String {
        format!("{} v{}", clean(&self.name), clean(&self.version))
    }

    fn is_enabled(&self, feature: &str) -> bool {
        self.enabled.iter().any(|f| f == feature)
    }

    /// The package the dependency `key` resolved to, if it is on.
    fn resolved_dep(&self, key: &str) -> Option<usize> {
        let extern_name = key.replace('-', "_");
        self.resolved
            .iter()
            .find(|(name, _)| *name == extern_name)
            .map(|(_, package)| *package)
    }
}

/// Why a crate has the features it has: one dependent's request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureRequest {
    /// The dependent, by index.
    pub dependent: usize,
    /// Whether it asks for the default features.
    pub default_features: bool,
    /// The features it names in its manifest, sorted.
    pub features: Vec<String>,
    /// Its own enabled features that turn on more of this crate's:
    /// `(its feature, this crate's feature)`.
    pub via: Vec<(String, String)>,
}

/// Every crate's resolved features. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureGraph {
    crates: Vec<CrateFeatures>,
    members: Vec<usize>,
    /// Per crate, whether it is a member: sorting and drawing ask for each.
    is_member: Vec<bool>,
    /// Per crate, the crates that resolved to it, in index order: what
    /// [`requests`](Self::requests) reads instead of scanning every crate.
    dependents: Vec<Vec<usize>>,
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

impl FeatureGraph {
    /// Read `cargo metadata --format-version 1` output.
    pub fn from_json(json: &str) -> Result<Self, DepsError> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| DepsError::new(format!("not cargo metadata JSON: {e}")))?;
        Self::from_value(&value)
    }

    /// [`FeatureGraph::from_json`] for parsed JSON.
    pub fn from_value(value: &Value) -> Result<Self, DepsError> {
        let missing = |what: &str| DepsError::new(format!("not cargo metadata: no `{what}`"));
        let packages = value
            .get("packages")
            .and_then(Value::as_array)
            .ok_or_else(|| missing("packages"))?;
        check_count(packages.len(), "packages")?;
        let mut crates = Vec::with_capacity(packages.len());
        let mut index = HashMap::new();
        for package in packages {
            let field = |key: &str| package.get(key).and_then(Value::as_str);
            let (Some(id), Some(name), Some(version)) =
                (field("id"), field("name"), field("version"))
            else {
                return Err(DepsError::new(
                    "not cargo metadata: a package has no id, name or version",
                ));
            };
            let definitions = package
                .get("features")
                .and_then(Value::as_object)
                .map(|table| {
                    table
                        .iter()
                        .map(|(feature, turns_on)| (feature.clone(), strings(Some(turns_on))))
                        .collect()
                })
                .unwrap_or_default();
            let declared = package
                .get("dependencies")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|dep| {
                            Some(DeclaredDep {
                                name: dep.get("name")?.as_str()?.to_string(),
                                rename: dep
                                    .get("rename")
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                optional: dep
                                    .get("optional")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                                features: strings(dep.get("features")),
                                default_features: dep
                                    .get("uses_default_features")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(true),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            index.insert(id.to_string(), crates.len());
            crates.push(CrateFeatures {
                id: id.to_string(),
                name: name.to_string(),
                version: version.to_string(),
                enabled: Vec::new(),
                definitions,
                declared,
                resolved: Vec::new(),
            });
        }
        let lookup = |id: &str| {
            index.get(id).copied().ok_or_else(|| {
                DepsError::new(format!("cargo metadata names an unknown package {id:?}"))
            })
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
                DepsError::new("cargo metadata has no `resolve`: it was run with --no-deps")
            })?;
        for node in resolve
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| missing("resolve.nodes"))?
        {
            let at = lookup(node.get("id").and_then(Value::as_str).unwrap_or_default())?;
            let mut enabled = strings(node.get("features"));
            enabled.sort_by(|a, b| (a != "default", a).cmp(&(b != "default", b)));
            enabled.dedup();
            let mut resolved = Vec::new();
            for dep in node
                .get("deps")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = dep.get("name").and_then(Value::as_str).unwrap_or_default();
                let package = lookup(dep.get("pkg").and_then(Value::as_str).unwrap_or_default())?;
                resolved.push((name.to_string(), package));
            }
            crates[at].enabled = enabled;
            crates[at].resolved = resolved;
        }
        let mut is_member = vec![false; crates.len()];
        for &member in &members {
            is_member[member] = true;
        }
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); crates.len()];
        for (dependent, krate) in crates.iter().enumerate() {
            for &(_, package) in &krate.resolved {
                // Dependents are visited in order, so one that resolves to
                // `package` twice (renamed) would be the last pushed.
                if dependents[package].last() != Some(&dependent) {
                    dependents[package].push(dependent);
                }
            }
        }
        Ok(FeatureGraph {
            crates,
            members,
            is_member,
            dependents,
        })
    }

    /// Every crate, in `cargo metadata`'s order.
    pub fn crates(&self) -> &[CrateFeatures] {
        &self.crates
    }

    /// The workspace members, by index.
    pub fn members(&self) -> &[usize] {
        &self.members
    }

    /// The crates `spec` names: `name`, or `name@version`; oldest first.
    pub fn find(&self, spec: &str) -> Vec<usize> {
        let (name, version) = match spec.split_once('@') {
            Some((name, version)) => (name, Some(version.trim_start_matches('v'))),
            None => (spec, None),
        };
        let mut found: Vec<usize> = (0..self.crates.len())
            .filter(|&i| {
                let krate = &self.crates[i];
                krate.name == name && version.is_none_or(|v| krate.version == v)
            })
            .collect();
        found.sort_by(|&a, &b| compare_versions(&self.crates[a].version, &self.crates[b].version));
        found
    }

    /// Who asked for `package`'s features: every dependent that resolved to
    /// it, with what its manifest asks for and which of its own enabled
    /// features turn on more. Sorted by the dependent's name.
    pub fn requests(&self, package: usize) -> Vec<FeatureRequest> {
        let target = &self.crates[package];
        let mut requests = Vec::new();
        for &dependent in &self.dependents[package] {
            let krate = &self.crates[dependent];
            let names: BTreeSet<&str> = krate
                .resolved
                .iter()
                .filter(|(_, to)| *to == package)
                .map(|(name, _)| name.as_str())
                .collect();
            if names.is_empty() {
                continue;
            }
            let declared: Vec<&DeclaredDep> = krate
                .declared
                .iter()
                .filter(|dep| dep.name == target.name && names.contains(dep.extern_name().as_str()))
                .collect();
            let mut features: Vec<String> = declared
                .iter()
                .flat_map(|dep| dep.features.iter().cloned())
                .collect();
            features.sort();
            features.dedup();
            let keys: BTreeSet<&str> = declared.iter().map(|dep| dep.key()).collect();
            let mut via = Vec::new();
            for feature in &krate.enabled {
                for entry in krate.definitions.get(feature).into_iter().flatten() {
                    if let Some((key, turns_on)) = entry.split_once('/') {
                        if keys.contains(key.trim_end_matches('?')) {
                            via.push((feature.clone(), turns_on.to_string()));
                        }
                    }
                }
            }
            requests.push(FeatureRequest {
                dependent,
                default_features: declared.is_empty()
                    || declared.iter().any(|dep| dep.default_features),
                features,
                via,
            });
        }
        requests.sort_by(|a, b| {
            let (a, b) = (&self.crates[a.dependent], &self.crates[b.dependent]);
            (&a.name, &a.version).cmp(&(&b.name, &b.version))
        });
        requests
    }
}

/// One crate's features as a tree, or several crates'. See the
/// [module docs](self).
#[derive(Clone, Debug)]
pub struct FeatureTree {
    graph: FeatureGraph,
    targets: Vec<usize>,
    /// Crates left out for having no features enabled (only when showing
    /// every crate).
    featureless: usize,
}

impl FeatureTree {
    /// The crates `spec` names (`name` or `name@version`), an error when
    /// there are none; or, with no `spec`, every crate with a feature
    /// enabled, the workspace members first, then by name.
    pub fn new(graph: FeatureGraph, spec: Option<&str>) -> Result<Self, DepsError> {
        let (targets, featureless) = match spec {
            Some(spec) => {
                let targets = graph.find(spec);
                if targets.is_empty() {
                    return Err(DepsError::new(format!(
                        "no package {spec:?} in the dependency graph"
                    )));
                }
                (targets, 0)
            }
            None => {
                let mut targets: Vec<usize> = (0..graph.crates.len())
                    .filter(|&i| !graph.crates[i].enabled.is_empty())
                    .collect();
                let featureless = graph.crates.len() - targets.len();
                targets.sort_by(|&a, &b| {
                    let (x, y) = (&graph.crates[a], &graph.crates[b]);
                    (!graph.is_member[a], &x.name)
                        .cmp(&(!graph.is_member[b], &y.name))
                        .then_with(|| compare_versions(&x.version, &y.version))
                });
                (targets, featureless)
            }
        };
        Ok(FeatureTree {
            graph,
            targets,
            featureless,
        })
    }

    /// The crates drawn.
    pub fn targets(&self) -> &[usize] {
        &self.targets
    }

    pub fn graph(&self) -> &FeatureGraph {
        &self.graph
    }

    /// The tree of one crate.
    pub fn tree(&self, package: usize, console: &Console) -> Tree {
        let krate = &self.graph.crates[package];
        let mut label = Text::new("");
        let name_style = if self.graph.is_member[package] {
            theme_style(console, "deps.root")
        } else {
            theme_style(console, "deps.name")
        };
        label.append(&clean(&krate.name), Some(name_style.into()));
        label.append(" ", None);
        label.append(
            &format!("v{}", clean(&krate.version)),
            Some(theme_style(console, "deps.version").into()),
        );
        let count = krate.enabled.len();
        let summary = match count {
            0 => "  no features enabled".to_string(),
            1 => "  1 feature enabled".to_string(),
            n => format!("  {n} features enabled"),
        };
        label.append(&summary, Some(theme_style(console, "deps.section").into()));
        let mut tree = Tree::new(label);
        for feature in &krate.enabled {
            let mut node = Tree::new(Text::styled(
                clean(feature).into_owned(),
                theme_style(console, "deps.feature"),
            ));
            match krate.definitions.get(feature) {
                Some(entries) => {
                    for entry in entries {
                        node.add(self.entry(krate, entry, console));
                    }
                }
                None => {
                    // An optional dependency's implicit feature (before
                    // Cargo 1.60 listed them).
                    if let Some(dep) = krate
                        .declared
                        .iter()
                        .find(|d| d.optional && d.key() == feature)
                    {
                        node.add(self.entry(krate, &format!("dep:{}", dep.key()), console));
                    }
                }
            }
            tree.add_tree(node);
        }
        let requests = self.graph.requests(package);
        if !requests.is_empty() {
            let mut section = Tree::new(Text::styled(
                "requested by",
                theme_style(console, "deps.section"),
            ));
            for request in requests {
                section.add_tree(self.request(&request, krate, console));
            }
            tree.add_tree(section);
        }
        tree
    }

    /// One entry of a feature's definition, with what it resolved to.
    fn entry(&self, krate: &CrateFeatures, entry: &str, console: &Console) -> Text {
        let feature_style = theme_style(console, "deps.feature");
        let off = |text: &mut Text| {
            text.append(" (off)", Some(theme_style(console, "deps.off").into()));
        };
        let arrow = |text: &mut Text, package: usize| {
            text.append(" → ", Some(theme_style(console, "deps.section").into()));
            text.append(&self.graph.crates[package].display(), None);
        };
        let mut text = Text::new("");
        if let Some(key) = entry.strip_prefix("dep:") {
            text.append(&clean(entry), None);
            match krate.resolved_dep(key) {
                Some(package) => arrow(&mut text, package),
                None => off(&mut text),
            }
        } else if let Some((key, feature)) = entry.split_once('/') {
            let weak = key.ends_with('?');
            let key = key.trim_end_matches('?');
            text.append(
                &format!("{}{}/", clean(key), if weak { "?" } else { "" }),
                None,
            );
            text.append(&clean(feature), Some(feature_style.into()));
            match krate.resolved_dep(key) {
                Some(package) if self.graph.crates[package].is_enabled(feature) => {
                    arrow(&mut text, package)
                }
                _ => off(&mut text),
            }
        } else if krate.definitions.contains_key(entry) {
            text.append(&clean(entry), Some(feature_style.into()));
            if !krate.is_enabled(entry) {
                off(&mut text);
            }
        } else {
            // A plain name that is no feature: an optional dependency.
            text.append(&clean(entry), None);
            match krate.resolved_dep(entry) {
                Some(package) => arrow(&mut text, package),
                None => off(&mut text),
            }
        }
        text
    }

    fn request(&self, request: &FeatureRequest, krate: &CrateFeatures, console: &Console) -> Tree {
        let mut text = Text::new(self.graph.crates[request.dependent].display());
        let mut asks: Vec<String> = Vec::new();
        if request.default_features && krate.definitions.contains_key("default") {
            asks.push("default features".into());
        }
        asks.extend(request.features.iter().map(|f| clean(f).into_owned()));
        if !asks.is_empty() {
            text.append(": ", None);
            text.append(
                &asks.join(", "),
                Some(theme_style(console, "deps.feature").into()),
            );
        }
        let mut tree = Tree::new(text);
        let mut by_feature: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (theirs, ours) in &request.via {
            by_feature.entry(theirs).or_default().push(ours);
        }
        for (theirs, ours) in by_feature {
            let mut text = Text::styled(
                format!("its feature {}: ", clean(theirs)),
                theme_style(console, "deps.section"),
            );
            text.append(
                &clean(&ours.join(", ")),
                Some(theme_style(console, "deps.feature").into()),
            );
            tree.add(text);
        }
        tree
    }

    fn note(&self, console: &Console) -> Option<Text> {
        (self.featureless > 0).then(|| {
            Text::styled(
                format!(
                    "{} other crate{} no features enabled",
                    self.featureless,
                    if self.featureless == 1 {
                        " has"
                    } else {
                        "s have"
                    }
                ),
                theme_style(console, "deps.section"),
            )
        })
    }

    fn parts(&self, console: &Console) -> Vec<Box<dyn Renderable>> {
        let mut parts: Vec<Box<dyn Renderable>> = self
            .targets
            .iter()
            .map(|&target| Box::new(self.tree(target, console)) as Box<dyn Renderable>)
            .collect();
        if let Some(note) = self.note(console) {
            parts.push(Box::new(note));
        }
        parts
    }
}

impl Renderable for FeatureTree {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let parts = self.parts(console);
        let parts: Vec<&dyn Renderable> = parts.iter().map(|p| p.as_ref()).collect();
        stack(&parts, console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let parts = self.parts(console);
        let parts: Vec<&dyn Renderable> = parts.iter().map(|p| p.as_ref()).collect();
        stack_measure(&parts, console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> String {
        serde_json::json!({
            "packages": [
                {"id": "app 0.1.0", "name": "app", "version": "0.1.0", "source": null,
                 "features": {"default": ["json"], "json": ["serde/std", "dep:serde_json"],
                              "fast": ["serde?/unstable"]},
                 "dependencies": [
                    {"name": "serde", "features": ["derive"], "uses_default_features": true,
                     "optional": false},
                    {"name": "serde_json", "optional": true, "features": [],
                     "uses_default_features": true},
                    {"name": "my-log", "rename": "logger", "optional": false,
                     "features": [], "uses_default_features": false}]},
                {"id": "serde 1.0.0", "name": "serde", "version": "1.0.0", "source": null,
                 "features": {"default": ["std"], "std": [], "derive": ["serde_derive"],
                              "serde_derive": ["dep:serde_derive"], "unstable": []},
                 "dependencies": [{"name": "serde_derive", "optional": true, "features": [],
                                   "uses_default_features": true}]},
                {"id": "serde_derive 1.0.0", "name": "serde_derive", "version": "1.0.0",
                 "source": null, "features": {}, "dependencies": []},
                {"id": "serde_json 1.0.0", "name": "serde_json", "version": "1.0.0",
                 "source": null, "features": {}, "dependencies": []},
                {"id": "my-log 0.1.0", "name": "my-log", "version": "0.1.0",
                 "source": null, "features": {"default": [], "color": []}, "dependencies": []},
            ],
            "workspace_members": ["app 0.1.0"],
            "resolve": {"root": "app 0.1.0", "nodes": [
                {"id": "app 0.1.0", "features": ["default", "json"], "deps": [
                    {"name": "serde", "pkg": "serde 1.0.0"},
                    {"name": "serde_json", "pkg": "serde_json 1.0.0"},
                    {"name": "logger", "pkg": "my-log 0.1.0"}]},
                {"id": "serde 1.0.0", "features": ["default", "derive", "serde_derive", "std"],
                 "deps": [{"name": "serde_derive", "pkg": "serde_derive 1.0.0"}]},
                {"id": "serde_derive 1.0.0", "features": [], "deps": []},
                {"id": "serde_json 1.0.0", "features": [], "deps": []},
                {"id": "my-log 0.1.0", "features": [], "deps": []},
            ]}
        })
        .to_string()
    }

    fn render(tree: &FeatureTree) -> String {
        Console::builder()
            .width(80)
            .color_system(None)
            .build()
            .render_to_string(tree)
    }

    #[test]
    fn a_crate_shows_its_features_and_who_asked() {
        let graph = FeatureGraph::from_json(&metadata()).unwrap();
        let tree = FeatureTree::new(graph, Some("serde")).unwrap();
        assert_eq!(
            render(&tree),
            "serde v1.0.0  4 features enabled\n\
             ├── default\n\
             │   └── std\n\
             ├── derive\n\
             │   └── serde_derive\n\
             ├── serde_derive\n\
             │   └── dep:serde_derive → serde_derive v1.0.0\n\
             ├── std\n\
             └── requested by\n    \
                 └── app v0.1.0: default features, derive\n        \
                     └── its feature json: std"
        );
    }

    #[test]
    fn the_root_shows_optional_and_weak_entries() {
        let graph = FeatureGraph::from_json(&metadata()).unwrap();
        let tree = FeatureTree::new(graph, Some("app@0.1.0")).unwrap();
        let out = render(&tree);
        assert!(out.contains("serde/std → serde v1.0.0"), "{out}");
        assert!(out.contains("dep:serde_json → serde_json v1.0.0"), "{out}");
        assert!(!out.contains("fast"), "fast is not enabled: {out}");
    }

    #[test]
    fn renamed_dependencies_are_matched() {
        let graph = FeatureGraph::from_json(&metadata()).unwrap();
        let log = graph.find("my-log")[0];
        let requests = graph.requests(log);
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].default_features);
    }

    #[test]
    fn every_crate_with_features_without_a_spec() {
        let graph = FeatureGraph::from_json(&metadata()).unwrap();
        let tree = FeatureTree::new(graph, None).unwrap();
        assert_eq!(tree.targets().len(), 2);
        let out = render(&tree);
        assert!(out.starts_with("app v0.1.0"), "{out}");
        assert!(
            out.ends_with("3 other crates have no features enabled"),
            "{out}"
        );
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(FeatureGraph::from_json("{").is_err());
        assert!(FeatureGraph::from_json("[]").is_err());
        let graph = FeatureGraph::from_json(&metadata()).unwrap();
        assert!(FeatureTree::new(graph, Some("nope")).is_err());
        let unknown = r#"{"packages": [], "workspace_members": [],
            "resolve": {"nodes": [{"id": "x 1.0.0"}]}}"#;
        assert!(FeatureGraph::from_json(unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown package"));
    }
}
