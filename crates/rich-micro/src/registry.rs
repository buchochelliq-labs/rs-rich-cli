//! The layered registry (#567, #583).
//!
//! Four layers, in ascending precedence: built-in < user
//! (`~/.config/rich/micro/`) < trusted project (`.rich/micro/`) < inline (added
//! in code). A name resolves to the highest layer that has it, as an asset or
//! as an alias; an alias resolves only within its own layer. Within a layer,
//! packages load in file-name order and the first to claim a name keeps it;
//! every later claim is reported as a [`Collision`]. A project's assets load
//! only when the caller says the project is trusted, under the same rule as
//! project config: a cloned repository cannot restyle `success` or put images
//! in your terminal.
//!
//! [`MicroRegistry::explain`] shows how a name resolved, reusing
//! `rich_ext::cli_doc::Precedence`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rich_ext::cli_doc::{Explanation, Layer as PrecedenceLayer, Precedence};

use crate::error::MicroError;
use crate::model::{Layer, MicroAsset, Origin};
use crate::name::check_name;
use crate::package::{self, Limits, Loaded};

/// Where each file-backed layer lives. Build one with
/// [`MicroRoots::from_env`], or set the fields for tests and embedders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MicroRoots {
    /// Load the library's own built-in set ([`crate::builtin`]), compiled
    /// in. On in [`from_env`](Self::from_env); off in `default()`.
    pub builtin_set: bool,
    /// Built-in packages shipped on disk, if any, loaded after the
    /// library's own set: for distributions that ship more.
    pub builtin: Option<PathBuf>,
    /// The user's directory, normally `~/.config/rich/micro/`.
    pub user: Option<PathBuf>,
    /// The project's directory, normally `<project>/.rich/micro/`.
    pub project: Option<PathBuf>,
    /// Whether to load [`project`](Self::project). Off unless the caller
    /// decided the project is trusted.
    pub project_trusted: bool,
}

impl MicroRoots {
    /// The user layer from `HOME` (or `USERPROFILE`), as `rich`'s CLI finds
    /// its config (`~/.config/rich/`), and the project layer under
    /// `project_root`, **untrusted**: call
    /// [`trust_project`](Self::trust_project) to load it.
    pub fn from_env(project_root: Option<&Path>) -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        MicroRoots {
            builtin_set: true,
            builtin: None,
            user: home.as_deref().map(Self::user_dir),
            project: project_root.map(Self::project_dir),
            project_trusted: false,
        }
    }

    /// `<home>/.config/rich/micro`.
    pub fn user_dir(home: &Path) -> PathBuf {
        home.join(".config").join("rich").join("micro")
    }

    /// `<project>/.rich/micro`.
    pub fn project_dir(project_root: &Path) -> PathBuf {
        project_root.join(".rich").join("micro")
    }

    /// Load the project layer (or not).
    pub fn trust_project(mut self, trusted: bool) -> Self {
        self.project_trusted = trusted;
        self
    }
}

/// A package that could not be loaded, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub layer: Layer,
    pub path: PathBuf,
    /// The package's path within its pack, for a package in a pack.
    pub package: Option<String>,
    pub error: MicroError,
}

/// Two claims on one name within a layer. The first (in file-name order, or
/// the order of [`MicroRegistry::add`] calls) keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    pub layer: Layer,
    pub name: String,
    /// The claim that kept the name.
    pub kept: String,
    /// The claim that lost it.
    pub dropped: String,
}

/// What loading reported. Loading never fails as a whole: a bad package is
/// rejected and the rest still load.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    pub rejected: Vec<Rejected>,
    pub collisions: Vec<Collision>,
    /// A project directory that exists but was not loaded, because the
    /// project is not trusted.
    pub untrusted_project: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
struct LayerAssets {
    /// Where the layer came from, for `explain`.
    origin: Option<String>,
    assets: BTreeMap<String, Arc<MicroAsset>>,
    /// alias -> asset name in this layer.
    aliases: BTreeMap<String, String>,
}

/// Assets by name, in layers. Cheap to clone (assets are shared).
///
/// ```
/// use rich_micro::{Layer, MicroAsset, MicroRegistry};
///
/// let mut registry = MicroRegistry::new();
/// registry.add(Layer::User, MicroAsset::new("ok", "check mark")?.with_text("ok")?)?;
/// registry.add(Layer::Inline, MicroAsset::new("ok", "tick")?.with_text("v")?)?;
/// let asset = registry.resolve("ok").unwrap();
/// assert_eq!(asset.alt(), "tick");
/// assert_eq!(asset.origin().layer, Layer::Inline);
/// # Ok::<(), rich_micro::MicroError>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct MicroRegistry {
    layers: [LayerAssets; 4],
}

impl MicroRegistry {
    /// An empty registry: every layer present, none holding anything.
    /// [`builtin`](Self::builtin) has the library's built-in set.
    pub fn new() -> Self {
        let mut registry = MicroRegistry::default();
        registry.layers[Layer::BuiltIn.index()].origin = Some("rs-rich-micro".to_string());
        registry.layers[Layer::Inline.index()].origin = Some("code".to_string());
        registry
    }

    /// A registry holding the library's built-in set ([`crate::builtin`])
    /// in its built-in layer.
    ///
    /// ```
    /// use rich_micro::MicroRegistry;
    ///
    /// let registry = MicroRegistry::builtin();
    /// let loading = registry.require("status/loading")?;
    /// assert_eq!(loading.license(), Some("MIT"));
    /// # Ok::<(), rich_micro::MicroError>(())
    /// ```
    pub fn builtin() -> Self {
        let mut registry = MicroRegistry::new();
        registry.load_builtin_set(&mut LoadReport::default());
        registry
    }

    /// Add the library's built-in set to the built-in layer.
    pub fn load_builtin_set(&mut self, report: &mut LoadReport) {
        for (pack, loaded) in crate::builtin::PACKS
            .iter()
            .zip(crate::builtin::packs(Layer::BuiltIn))
        {
            let path = PathBuf::from(format!("builtin:{pack}"));
            match loaded {
                Ok(pack) => {
                    for asset in pack.assets {
                        report.collisions.extend(self.insert(Layer::BuiltIn, asset));
                    }
                    for (package, error) in pack.rejected {
                        report.rejected.push(Rejected {
                            layer: Layer::BuiltIn,
                            path: path.clone(),
                            package: Some(package),
                            error,
                        });
                    }
                }
                Err(error) => report.rejected.push(Rejected {
                    layer: Layer::BuiltIn,
                    path,
                    package: None,
                    error,
                }),
            }
        }
    }

    /// Load every file-backed layer in `roots`, and the built-in set when
    /// `roots.builtin_set`. The project layer loads only when
    /// `roots.project_trusted`.
    pub fn load(roots: &MicroRoots, limits: &Limits) -> (Self, LoadReport) {
        let mut registry = MicroRegistry::new();
        let mut report = LoadReport::default();
        if roots.builtin_set {
            registry.load_builtin_set(&mut report);
        }
        if let Some(dir) = &roots.builtin {
            registry.load_dir(Layer::BuiltIn, dir, limits, &mut report);
        }
        if let Some(dir) = &roots.user {
            registry.load_dir(Layer::User, dir, limits, &mut report);
        }
        if let Some(dir) = &roots.project {
            if roots.project_trusted {
                registry.load_dir(Layer::Project, dir, limits, &mut report);
            } else if dir.is_dir() {
                report.untrusted_project = Some(dir.clone());
            }
        }
        (registry, report)
    }

    /// Load every package and pack in `dir` into `layer`, in file-name order.
    /// Entries are package or pack directories and `.richmicro` / `.zip`
    /// archives; hidden entries and other files are skipped. An entry named
    /// like an archive that is not a regular file (a FIFO, socket or device)
    /// is rejected without being opened. A missing directory is an empty
    /// layer.
    pub fn load_dir(&mut self, layer: Layer, dir: &Path, limits: &Limits, report: &mut LoadReport) {
        if layer != Layer::BuiltIn || self.layers[layer.index()].assets.is_empty() {
            self.layers[layer.index()].origin = Some(dir.display().to_string());
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                report.rejected.push(Rejected {
                    layer,
                    path: dir.to_path_buf(),
                    package: None,
                    error: MicroError::Io(format!("{}: {error}", dir.display())),
                });
                return;
            }
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || name.is_empty() {
                    return false;
                }
                path.is_dir()
                    || path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e == "richmicro" || e == "zip")
            })
            .collect();
        paths.sort();
        for path in paths {
            match package::load(&path, layer, limits) {
                Ok(Loaded::Package(asset)) => {
                    report.collisions.extend(self.insert(layer, *asset));
                }
                Ok(Loaded::Pack(pack)) => {
                    for asset in pack.assets {
                        report.collisions.extend(self.insert(layer, asset));
                    }
                    for (package, error) in pack.rejected {
                        report.rejected.push(Rejected {
                            layer,
                            path: path.clone(),
                            package: Some(package),
                            error,
                        });
                    }
                }
                Err(error) => report.rejected.push(Rejected {
                    layer,
                    path,
                    package: None,
                    error,
                }),
            }
        }
    }

    /// Add `asset` to `layer` (its origin's layer is set to it). The first
    /// claim on a name in a layer keeps it: a later one is dropped and
    /// returned as a [`Collision`], and so is an alias that clashes.
    pub fn add(&mut self, layer: Layer, asset: MicroAsset) -> Result<Vec<Collision>, MicroError> {
        asset.validate()?;
        Ok(self.insert(layer, asset))
    }

    fn insert(&mut self, layer: Layer, asset: MicroAsset) -> Vec<Collision> {
        let origin = Origin {
            layer,
            ..asset.origin().clone()
        };
        let asset = asset.with_origin(origin);
        let slot = &mut self.layers[layer.index()];
        let mut collisions = Vec::new();
        let claim = |name: &str, asset: &MicroAsset| format!("{name} from {}", asset.origin());
        let name = asset.name().to_string();
        let existing = slot.assets.get(&name).cloned().or_else(|| {
            slot.aliases
                .get(&name)
                .and_then(|target| slot.assets.get(target).cloned())
        });
        if let Some(existing) = existing {
            collisions.push(Collision {
                layer,
                name: name.clone(),
                kept: claim(existing.name(), &existing),
                dropped: claim(&name, &asset),
            });
            return collisions;
        }
        for alias in asset.aliases() {
            let taken = slot
                .assets
                .get(alias)
                .or_else(|| slot.aliases.get(alias).and_then(|t| slot.assets.get(t)))
                .cloned();
            match taken {
                Some(existing) => collisions.push(Collision {
                    layer,
                    name: alias.clone(),
                    kept: claim(existing.name(), &existing),
                    dropped: format!("alias {alias} of {}", claim(&name, &asset)),
                }),
                None => {
                    slot.aliases.insert(alias.clone(), name.clone());
                }
            }
        }
        slot.assets.insert(name, Arc::new(asset));
        collisions
    }

    /// Remove `name` (not an alias) from `layer`, with its aliases.
    pub fn remove(&mut self, layer: Layer, name: &str) -> Option<Arc<MicroAsset>> {
        let slot = &mut self.layers[layer.index()];
        let removed = slot.assets.remove(name)?;
        slot.aliases.retain(|_, target| target != name);
        Some(removed)
    }

    /// The asset `name` (or an alias) means in `layer` alone.
    pub fn get(&self, layer: Layer, name: &str) -> Option<&Arc<MicroAsset>> {
        let slot = &self.layers[layer.index()];
        slot.assets.get(name).or_else(|| {
            slot.aliases
                .get(name)
                .and_then(|target| slot.assets.get(target))
        })
    }

    /// The asset `name` resolves to: the highest layer that has it.
    pub fn resolve(&self, name: &str) -> Option<&Arc<MicroAsset>> {
        Layer::ALL
            .iter()
            .rev()
            .find_map(|layer| self.get(*layer, name))
    }

    /// [`resolve`](Self::resolve), with an error that names the asset.
    pub fn require(&self, name: &str) -> Result<&Arc<MicroAsset>, MicroError> {
        check_name(name)?;
        self.resolve(name)
            .ok_or_else(|| MicroError::UnknownAsset(name.to_string()))
    }

    /// Every lower layer's claim on `name` that the winner hides, highest
    /// first.
    pub fn shadowed(&self, name: &str) -> Vec<&Arc<MicroAsset>> {
        let mut claims = Layer::ALL
            .iter()
            .rev()
            .filter_map(|layer| self.get(*layer, name));
        claims.next();
        claims.collect()
    }

    /// Every effective asset name (not aliases), sorted.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .layers
            .iter()
            .flat_map(|slot| slot.assets.keys().map(String::as_str))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Every effective asset, by name.
    pub fn assets(&self) -> Vec<&Arc<MicroAsset>> {
        self.names()
            .into_iter()
            .filter_map(|name| self.resolve(name))
            .collect()
    }

    /// The assets in one layer, by name.
    pub fn layer(&self, layer: Layer) -> impl Iterator<Item = &Arc<MicroAsset>> {
        self.layers[layer.index()].assets.values()
    }

    /// Every layer as a [`Precedence`]: one key per name or alias, its value
    /// the asset it stands for and where that came from.
    pub fn precedence(&self) -> Precedence {
        let mut precedence = Precedence::new();
        for layer in Layer::ALL {
            let slot = &self.layers[layer.index()];
            let mut entry = PrecedenceLayer::new(layer.as_str());
            if let Some(origin) = &slot.origin {
                // A layer directory's path is the filesystem's, and may hold
                // terminal controls: shown, never run.
                entry = entry.origin(rich_ext::sanitize_terminal_controls(origin));
            }
            let mut values: BTreeMap<&str, String> = BTreeMap::new();
            for (name, asset) in &slot.assets {
                values.insert(name, describe(asset));
            }
            for (alias, target) in &slot.aliases {
                values.insert(alias, format!("alias of {target}"));
            }
            for (key, value) in values {
                entry = entry.value(key, value);
            }
            precedence = precedence.layer(entry);
        }
        precedence
    }

    /// How `name` resolved: the winning layer first, then what it overrides.
    /// A renderable; `None` when no layer has the name.
    pub fn explain(&self, name: &str) -> Option<Explanation> {
        self.precedence().explain(name)
    }
}

fn describe(asset: &MicroAsset) -> String {
    let mut out = format!("{} {}", asset.name(), asset.size());
    if let Some(version) = asset.version() {
        out.push_str(&format!(" v{version}"));
    }
    if let Some(pack) = &asset.origin().pack {
        out.push_str(&format!(" (pack {pack})"));
    }
    out
}
