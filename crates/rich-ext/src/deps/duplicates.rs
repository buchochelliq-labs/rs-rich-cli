//! Consolidating duplicates (#420): the crates resolved at more than one
//! version, who pulls each version in, and which version most of them
//! already use.
//!
//! [`DepGraph::duplicates`] says *which* crates are duplicated;
//! [`DepGraph::consolidation`] says what it would take to undo it. For each
//! duplicated crate it lists every version with the packages that depend on
//! it, and names the version the most dependents already use (the newest by
//! SemVer, on a tie). Whether the others can move onto it depends on their
//! version requirements, which the resolved graph does not record, so it is
//! not claimed. [`Consolidation`] draws that as a tree.
//!
//! ```
//! use rich::Console;
//! use rich_ext::deps::duplicates::Consolidation;
//! use rich_ext::deps::DepGraph;
//!
//! let pkg = |name: &str, version: &str| {
//!     format!(r#"{{"id": "{name} {version}", "name": "{name}", "version": "{version}",
//!         "source": "registry+https://github.com/rust-lang/crates.io-index"}}"#)
//! };
//! let dep = |id: &str| format!(
//!     r#"{{"name": "x", "pkg": "{id}", "dep_kinds": [{{"kind": null, "target": null}}]}}"#
//! );
//! let metadata = format!(
//!     r#"{{"packages": [{}, {}, {}, {}],
//!         "workspace_members": ["app 0.1.0"],
//!         "resolve": {{"root": "app 0.1.0", "nodes": [
//!           {{"id": "app 0.1.0", "deps": [{}, {}]}},
//!           {{"id": "old 1.0.0", "deps": [{}]}},
//!           {{"id": "log 0.3.0", "deps": []}}, {{"id": "log 0.4.0", "deps": []}}]}}}}"#,
//!     pkg("app", "0.1.0"), pkg("old", "1.0.0"), pkg("log", "0.3.0"), pkg("log", "0.4.0"),
//!     dep("old 1.0.0"), dep("log 0.4.0"), dep("log 0.3.0"),
//! );
//! let graph = DepGraph::from_json(&metadata).unwrap();
//! let console = Console::builder().width(60).color_system(None).build();
//! assert_eq!(
//!     console.render_to_string(&Consolidation::new(&graph)),
//!     "consolidation: 1 crate at several versions\n\
//!      log: 2 versions, 2 dependents; 1 uses v0.4.0 (newest)\n\
//!      ├── v0.4.0 ← app v0.1.0\n\
//!      └── v0.3.0 ← old v1.0.0"
//! );
//! ```

use std::collections::BTreeMap;

use rich::{Console, ConsoleOptions, Renderable, Segment, Text, Tree};

use super::{clean, compare_versions, theme_style, DepGraph, DepKind};

/// The most dependents listed beside one version; the rest are counted.
pub const MAX_LISTED: usize = 8;

/// One version of a duplicated crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateVersion {
    pub version: String,
    /// The packages resolved at this version (usually one; more when the
    /// same version comes from several sources), by index.
    pub packages: Vec<usize>,
    /// What depends on it, by index, sorted by name and deduplicated.
    pub dependents: Vec<usize>,
}

/// A crate resolved at several versions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Duplicate {
    pub name: String,
    /// Every version, newest first.
    pub versions: Vec<DuplicateVersion>,
    /// Which of [`versions`](Self::versions) the most dependents use (the
    /// newest on a tie).
    pub shared: usize,
}

impl Duplicate {
    /// The version the most dependents use.
    pub fn shared_version(&self) -> &DuplicateVersion {
        &self.versions[self.shared]
    }

    /// Every distinct dependent, whichever version it uses.
    pub fn dependent_count(&self) -> usize {
        let mut all: Vec<usize> = self
            .versions
            .iter()
            .flat_map(|v| v.dependents.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all.len()
    }

    /// The dependents on another version than the most-used one: what
    /// would have to change to leave one copy. A package that depends on
    /// both versions (through a renamed dependency) is among them.
    pub fn to_move(&self) -> Vec<usize> {
        let mut moving: Vec<usize> = self
            .versions
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.shared)
            .flat_map(|(_, v)| v.dependents.iter().copied())
            .collect();
        moving.sort_unstable();
        moving.dedup();
        moving
    }
}

impl DepGraph {
    /// The crates resolved at more than one version among the packages the
    /// kinds in `kinds` reach (as [`DepGraph::duplicates_in`] finds them),
    /// by name, each with who depends on which version.
    pub fn consolidation(&self, kinds: &[DepKind]) -> Vec<Duplicate> {
        let duplicates = self.duplicates_in(kinds);
        if duplicates.is_empty() {
            return Vec::new();
        }
        let reachable = self.reachable(kinds);
        let dependents = self.dependents(kinds);
        let mut by_name: BTreeMap<&str, BTreeMap<&str, DuplicateVersion>> = BTreeMap::new();
        for (index, package) in self.packages.iter().enumerate() {
            if !reachable[index] || !duplicates.contains_key(&package.name) {
                continue;
            }
            let entry = by_name
                .entry(&package.name)
                .or_default()
                .entry(&package.version)
                .or_insert_with(|| DuplicateVersion {
                    version: package.version.clone(),
                    packages: Vec::new(),
                    dependents: Vec::new(),
                });
            entry.packages.push(index);
            entry
                .dependents
                .extend(dependents[index].iter().map(|(from, _)| *from));
        }
        by_name
            .into_iter()
            .map(|(name, versions)| {
                let mut versions: Vec<DuplicateVersion> = versions.into_values().collect();
                versions.sort_by(|a, b| compare_versions(&b.version, &a.version));
                for version in &mut versions {
                    version.dependents.sort_by(|&a, &b| {
                        let (a, b) = (&self.packages[a], &self.packages[b]);
                        (&a.name, &a.version).cmp(&(&b.name, &b.version))
                    });
                    version.dependents.dedup();
                }
                // Newest first, so the first of the most-used is the newest.
                let most = versions
                    .iter()
                    .map(|v| v.dependents.len())
                    .max()
                    .unwrap_or(0);
                let shared = versions
                    .iter()
                    .position(|v| v.dependents.len() == most)
                    .unwrap_or(0);
                Duplicate {
                    name: name.to_string(),
                    versions,
                    shared,
                }
            })
            .collect()
    }
}

/// The consolidation summary as a tree per duplicated crate: its versions,
/// newest first, each with what depends on it, and the version the most
/// dependents use named in the heading. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct Consolidation {
    graph: DepGraph,
    duplicates: Vec<Duplicate>,
}

impl Consolidation {
    /// The duplicates among every kind of dependency.
    pub fn new(graph: &DepGraph) -> Self {
        Consolidation {
            graph: graph.clone(),
            duplicates: graph.consolidation(&[DepKind::Normal, DepKind::Build, DepKind::Dev]),
        }
    }

    /// Only the duplicates reached through the kinds in `kinds`.
    pub fn kinds(mut self, kinds: &[DepKind]) -> Self {
        self.duplicates = self.graph.consolidation(kinds);
        self
    }

    pub fn duplicates(&self) -> &[Duplicate] {
        &self.duplicates
    }

    fn heading(&self, console: &Console) -> Text {
        let heading = match self.duplicates.len() {
            0 => "consolidation: no crate is resolved at more than one version".to_string(),
            1 => "consolidation: 1 crate at several versions".to_string(),
            count => format!("consolidation: {count} crates at several versions"),
        };
        Text::styled(heading, theme_style(console, "deps.section"))
    }

    /// The tree of one duplicated crate.
    pub fn tree(&self, duplicate: &Duplicate, console: &Console) -> Tree {
        let shared = duplicate.shared_version();
        let dependents = duplicate.dependent_count();
        let mut label = Text::new("");
        label.append(
            &clean(&duplicate.name),
            Some(theme_style(console, "deps.duplicate").into()),
        );
        let newest = if duplicate.shared == 0 {
            " (newest)"
        } else {
            ""
        };
        label.append(
            &format!(
                ": {} versions, {dependents} dependent{}; {} use{} v{}{newest}",
                duplicate.versions.len(),
                if dependents == 1 { "" } else { "s" },
                shared.dependents.len(),
                if shared.dependents.len() == 1 {
                    "s"
                } else {
                    ""
                },
                clean(&shared.version),
            ),
            None,
        );
        let mut tree = Tree::new(label);
        for version in &duplicate.versions {
            let mut text = Text::new("");
            text.append(
                &format!("v{}", clean(&version.version)),
                Some(theme_style(console, "deps.version").into()),
            );
            let names: Vec<String> = version
                .dependents
                .iter()
                .take(MAX_LISTED)
                .map(|&d| self.graph.display(d))
                .collect();
            if !names.is_empty() {
                text.append(" ← ", Some(theme_style(console, "deps.section").into()));
                text.append(&names.join(", "), None);
                let more = version.dependents.len().saturating_sub(MAX_LISTED);
                if more > 0 {
                    text.append(
                        &format!(" and {more} more"),
                        Some(theme_style(console, "deps.section").into()),
                    );
                }
            }
            tree.add(text);
        }
        tree
    }

    fn parts(&self, console: &Console) -> (Text, Vec<Tree>) {
        let trees = self
            .duplicates
            .iter()
            .map(|duplicate| self.tree(duplicate, console))
            .collect();
        (self.heading(console), trees)
    }
}

impl Renderable for Consolidation {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (heading, trees) = self.parts(console);
        let mut segments = heading.rich_render(console, options);
        for tree in trees {
            if segments
                .iter()
                .rev()
                .find(|segment| !segment.text.is_empty())
                .is_some_and(|segment| !segment.text.ends_with('\n'))
            {
                segments.push(Segment::line());
            }
            segments.extend(tree.rich_render(console, options));
        }
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let (heading, trees) = self.parts(console);
        let mut measured = heading.measure(console, options);
        for tree in trees {
            let tree = tree.measure(console, options);
            measured = rich::measure::Measurement::new(
                measured.minimum.max(tree.minimum),
                measured.maximum.max(tree.maximum),
            );
        }
        measured
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> String {
        let crates_io = "registry+https://github.com/rust-lang/crates.io-index";
        let pkg = |name: &str, version: &str| {
            serde_json::json!({"id": format!("{name} {version}"), "name": name,
                "version": version, "source": crates_io})
        };
        let dep = |id: &str, kind: Option<&str>| {
            serde_json::json!({"name": id.split(' ').next(), "pkg": id,
                "dep_kinds": [{"kind": kind, "target": null}]})
        };
        serde_json::json!({
            "packages": [
                pkg("app", "0.1.0"), pkg("a", "1.0.0"), pkg("b", "1.0.0"),
                pkg("c", "1.0.0"), pkg("syn", "1.0.109"), pkg("syn", "2.0.0"),
                pkg("t", "1.0.0"),
            ],
            "workspace_members": ["app 0.1.0"],
            "resolve": {"root": "app 0.1.0", "nodes": [
                {"id": "app 0.1.0", "deps": [dep("a 1.0.0", None), dep("b 1.0.0", None),
                    dep("c 1.0.0", None), dep("t 1.0.0", Some("dev"))]},
                {"id": "a 1.0.0", "deps": [dep("syn 2.0.0", None)]},
                {"id": "b 1.0.0", "deps": [dep("syn 2.0.0", None)]},
                {"id": "c 1.0.0", "deps": [dep("syn 1.0.109", None)]},
                {"id": "t 1.0.0", "deps": [dep("syn 1.0.109", None)]},
                {"id": "syn 1.0.109", "deps": []},
                {"id": "syn 2.0.0", "deps": []},
            ]}
        })
        .to_string()
    }

    #[test]
    fn the_most_used_version_is_the_one_to_share() {
        let graph = DepGraph::from_json(&metadata()).unwrap();
        let all = graph.consolidation(&[DepKind::Normal, DepKind::Build, DepKind::Dev]);
        assert_eq!(all.len(), 1);
        let syn = &all[0];
        assert_eq!(syn.versions.len(), 2);
        // Two each: the tie goes to the newest.
        assert_eq!(syn.shared_version().version, "2.0.0");
        assert_eq!(syn.dependent_count(), 4);
        let moving: Vec<String> = syn.to_move().iter().map(|&p| graph.display(p)).collect();
        assert_eq!(moving, ["c v1.0.0", "t v1.0.0"]);
        // Without dev dependencies `t` does not count.
        let no_dev = graph.consolidation(&[DepKind::Normal, DepKind::Build]);
        assert_eq!(no_dev[0].dependent_count(), 3);
    }

    #[test]
    fn renders_a_tree_per_crate() {
        let graph = DepGraph::from_json(&metadata()).unwrap();
        let console = Console::builder().width(80).color_system(None).build();
        assert_eq!(
            console.render_to_string(&Consolidation::new(&graph)),
            "consolidation: 1 crate at several versions\n\
             syn: 2 versions, 4 dependents; 2 use v2.0.0 (newest)\n\
             ├── v2.0.0 ← a v1.0.0, b v1.0.0\n\
             └── v1.0.109 ← c v1.0.0, t v1.0.0"
        );
    }

    #[test]
    fn a_dependent_on_both_versions_still_has_to_move() {
        // `a` also depends on syn 1 (through a renamed dependency).
        let mut metadata: serde_json::Value = serde_json::from_str(&metadata()).unwrap();
        metadata["resolve"]["nodes"][1]["deps"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name": "syn1", "pkg": "syn 1.0.109",
                "dep_kinds": [{"kind": null, "target": null}]}));
        let graph = DepGraph::from_json(&metadata.to_string()).unwrap();
        let all = graph.consolidation(&[DepKind::Normal, DepKind::Build, DepKind::Dev]);
        let syn = &all[0];
        assert_eq!(syn.shared_version().version, "1.0.109");
        let moving: Vec<String> = syn.to_move().iter().map(|&p| graph.display(p)).collect();
        assert_eq!(moving, ["a v1.0.0", "b v1.0.0"]);
    }

    #[test]
    fn a_release_is_newer_than_its_pre_release() {
        let mut metadata: serde_json::Value = serde_json::from_str(&metadata()).unwrap();
        for package in metadata["packages"].as_array_mut().unwrap() {
            if package["id"] == "syn 1.0.109" {
                package["id"] = "syn 2.0.0-alpha".into();
                package["version"] = "2.0.0-alpha".into();
            }
        }
        let text = metadata
            .to_string()
            .replace("syn 1.0.109", "syn 2.0.0-alpha");
        let graph = DepGraph::from_json(&text).unwrap();
        let all = graph.consolidation(&[DepKind::Normal, DepKind::Build, DepKind::Dev]);
        let versions: Vec<&str> = all[0].versions.iter().map(|v| v.version.as_str()).collect();
        // Two dependents each: the tie goes to the release, not the pre-release.
        assert_eq!(versions, ["2.0.0", "2.0.0-alpha"]);
        assert_eq!(all[0].shared_version().version, "2.0.0");
    }

    #[test]
    fn nothing_to_consolidate_says_so() {
        let graph = DepGraph::from_json(&metadata()).unwrap();
        let view = Consolidation::new(&graph).kinds(&[]);
        let console = Console::builder().width(80).color_system(None).build();
        assert!(console
            .render_to_string(&view)
            .contains("no crate is resolved at more than one version"));
    }
}
