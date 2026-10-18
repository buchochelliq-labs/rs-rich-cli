//! `rich dot`, `rich deps` and `rich schema`: diagrams from real sources
//! (0.0.15 workstream 4; SQL DDL, Arrow and `--er` in 0.0.16 workstream 7;
//! `rich deps --sizes`, `--baseline` and `--policy` in 0.0.19 workstream 16).
//! Not upstream: binary-boundary conveniences that compose rs-rich-diagram
//! (the DOT parser, the layout), rs-rich-ext (the dependency and schema
//! trees) and rs-rich-data (Arrow schemas, ER diagrams); see
//! `docs/PORTING.md`.
use super::*;

use rich::containers::Renderables;
use rich_diagram::{Diagram, Direction, Graph, Node, Shape, Stroke};
use rich_ext::deps::audit::{AdvisoryKind, AdvisoryReport};
use rich_ext::deps::compare::{Comparison, Quantity, Threshold, Thresholds, Verdict};
use rich_ext::deps::features::{FeatureGraph, FeatureTree};
use rich_ext::deps::licenses::{LicenseReport, LicensedPackage};
use rich_ext::deps::policy::{self, ConflictMode, Policy, PolicyReport};
use rich_ext::deps::sbom::Sbom;
use rich_ext::deps::sizes::{SizeOptions, SizeReport, Sizes};
use rich_ext::deps::timings::{Timings, TimingsReport};
use rich_ext::deps::{DepGraph, DepKind, DepTree, WhyTree};
use rich_ext::schema::{Schema, SchemaDiff, SchemaTree};

/// `--dot-backend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DotBackend {
    /// Draw natively as text. The default.
    Text,
    /// `--export-svg` writes Graphviz's own SVG (the `dot` binary); the
    /// terminal still shows the native drawing.
    Graphviz,
    /// Markdown fences stay code blocks, as upstream renders them.
    Off,
}

impl DotBackend {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "text" => Ok(Self::Text),
            "graphviz" => Ok(Self::Graphviz),
            "off" => Ok(Self::Off),
            other => Err(format!(
                "unknown DOT backend {other:?} (text, graphviz, off)"
            )),
        }
    }
}

/// The options of `rich deps`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GraphSourceOptions {
    /// `--metadata FILE`: read `cargo metadata` JSON instead of running it.
    metadata: Option<String>,
    /// `--why CRATE`: what pulls a crate in.
    why: Option<String>,
    /// `--graph`: draw through the layout instead of a tree.
    graph: bool,
    /// `--depth N`: levels below each root.
    depth: Option<usize>,
    /// `--duplicates`: only the branches that lead to a duplicate.
    duplicates: bool,
    /// `--no-dev`: leave dev dependencies out.
    no_dev: bool,
    /// `--features`: the resolved features instead of the tree.
    features: bool,
    /// `--package CRATE`: with `--features`, whose.
    package: Option<String>,
    /// `--timings FILE`: a `cargo build --timings` report.
    timings: Option<String>,
    /// `--audit FILE`: `cargo audit --json` output.
    audit: Option<String>,
    /// `--licenses`: the licences, grouped.
    licenses: bool,
    /// `--sizes [TARGET_DIR|FILE]`: artifact sizes; `Some(None)` for the
    /// target directory `cargo metadata` names.
    sizes: Option<Option<String>>,
    /// `--build-profile NAME`: with `--sizes`, whose artifacts.
    build_profile: Option<String>,
    /// `--source-sizes`: with `--sizes`, each package's sources too.
    source_sizes: bool,
    /// `--top N`: how many units `--sizes`, `--timings` and `--baseline`
    /// show.
    top: Option<usize>,
    /// `--baseline FILE`: compare `--timings` or `--sizes` with it.
    baseline: Option<String>,
    /// `--threshold VALUE`, repeatable: with `--baseline`, what regresses.
    thresholds: Vec<String>,
    /// `--save FILE`: write the `--timings` or `--sizes` report as JSON.
    save: Option<String>,
    /// `--policy FILE`: a licence policy to check.
    policy: Option<String>,
    /// `--sbom FILE`: the packages of an SPDX or CycloneDX document.
    sbom: Option<String>,
}

impl GraphSourceOptions {
    /// Consume one of these options; anything else stays with the main parser.
    /// `--top` and `--threshold` are taken only for `rich deps` (`deps`):
    /// elsewhere they are `rich profile`'s and `--diff`'s.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut (impl Iterator<Item = &'a String> + Clone),
        deps: bool,
    ) -> Result<bool, String> {
        match arg {
            "--metadata" => {
                self.metadata = Some(rest.next().ok_or("--metadata requires a FILE")?.clone());
            }
            "--why" => {
                self.why = Some(rest.next().ok_or("--why requires a CRATE")?.clone());
            }
            "--graph" => self.graph = true,
            "--depth" => {
                let value = rest.next().ok_or("--depth requires a number")?;
                self.depth = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--depth requires a number, got {value:?}"))?,
                );
            }
            "--duplicates" => self.duplicates = true,
            "--no-dev" => self.no_dev = true,
            "--features" => self.features = true,
            "--package" => {
                self.package = Some(rest.next().ok_or("--package requires a CRATE")?.clone());
            }
            "--timings" => {
                self.timings = Some(rest.next().ok_or("--timings requires a FILE")?.clone());
            }
            "--audit" => {
                self.audit = Some(rest.next().ok_or("--audit requires a FILE")?.clone());
            }
            "--licenses" => self.licenses = true,
            "--sizes" => {
                // An optional value: the next argument, unless it is an option.
                let value = rest
                    .clone()
                    .next()
                    .filter(|value| *value == "-" || !value.starts_with('-'))
                    .cloned();
                if value.is_some() {
                    rest.next();
                }
                self.sizes = Some(value);
            }
            "--build-profile" => {
                let value = rest
                    .next()
                    .ok_or("--build-profile requires a profile name (debug, release, …)")?;
                self.build_profile = Some(value.clone());
            }
            "--source-sizes" => self.source_sizes = true,
            "--baseline" => {
                self.baseline = Some(rest.next().ok_or("--baseline requires a FILE")?.clone());
            }
            "--save" => {
                self.save = Some(rest.next().ok_or("--save requires a FILE")?.clone());
            }
            "--policy" => {
                self.policy = Some(rest.next().ok_or("--policy requires a FILE")?.clone());
            }
            "--sbom" => {
                self.sbom = Some(rest.next().ok_or("--sbom requires a FILE")?.clone());
            }
            "--top" if deps => {
                self.top = Some(
                    rest.next()
                        .and_then(|v| v.trim().replace('_', "").parse::<usize>().ok())
                        .filter(|n| *n > 0)
                        .ok_or("--top requires a positive integer")?,
                );
            }
            "--threshold" if deps => {
                let value = rest
                    .next()
                    .ok_or("--threshold requires a value: a percentage (10%), a time (0.5s) or a size (100KiB)")?;
                self.thresholds.push(value.clone());
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Each option given with the commands it applies to, for the "only has
    /// an effect" check.
    pub(crate) fn given(&self) -> Vec<(&'static str, &'static [&'static str])> {
        [
            ("--metadata", self.metadata.is_some()),
            ("--why", self.why.is_some()),
            ("--graph", self.graph),
            ("--depth", self.depth.is_some()),
            ("--duplicates", self.duplicates),
            ("--no-dev", self.no_dev),
            ("--features", self.features),
            ("--package", self.package.is_some()),
            ("--timings", self.timings.is_some()),
            ("--audit", self.audit.is_some()),
            ("--licenses", self.licenses),
            ("--sizes", self.sizes.is_some()),
            ("--build-profile", self.build_profile.is_some()),
            ("--source-sizes", self.source_sizes),
            ("--top", self.top.is_some()),
            ("--baseline", self.baseline.is_some()),
            ("--threshold", !self.thresholds.is_empty()),
            ("--save", self.save.is_some()),
            ("--policy", self.policy.is_some()),
            ("--sbom", self.sbom.is_some()),
        ]
        .into_iter()
        .filter(|(_, given)| *given)
        .map(|(flag, _)| (flag, &["deps"][..]))
        .collect()
    }

    /// Take `--depth` for `rich profile`, which shares the flag.
    pub(crate) fn take_depth(&mut self) -> Option<usize> {
        self.depth.take()
    }
}

type Failure = (ExitClass, String);

/// The options of `rich schema`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SchemaOptions {
    /// `--er`: draw the schema as an ER diagram.
    er: bool,
}

impl SchemaOptions {
    /// Consume one of these options; anything else stays with the main parser.
    pub(crate) fn parse_option(&mut self, arg: &str) -> bool {
        match arg {
            "--er" => self.er = true,
            _ => return false,
        }
        true
    }

    /// Each option given with the commands it applies to, for the "only has
    /// an effect" check.
    pub(crate) fn given(&self) -> Vec<(&'static str, &'static [&'static str])> {
        [("--er", self.er)]
            .into_iter()
            .filter(|(_, given)| *given)
            .map(|(flag, _)| (flag, &["schema"][..]))
            .collect()
    }
}

/// What `rich deps` shows, and the gate it failed, if any: `--audit` with a
/// vulnerability, `--baseline` with a regression and `--policy` with a
/// violation exit 5 once the report is shown.
pub(crate) struct DepsView {
    pub(crate) view: Box<dyn Renderable>,
    pub(crate) gate: Option<String>,
}

impl DepsView {
    fn shown(view: impl Renderable + 'static) -> Self {
        DepsView {
            view: Box::new(view),
            gate: None,
        }
    }
}

impl GraphSourceOptions {
    /// At most one report instead of the tree, and none of the options
    /// that do not apply to it: those are refused rather than ignored.
    fn check_combination(&self, cli: &Cli) -> Result<(), Failure> {
        let usage = |message: String| Err((ExitClass::Usage, message));
        // `--licenses` and `--policy` are one report: the policy alone, or
        // the licences then the policy.
        let licences = if self.licenses {
            "--licenses"
        } else {
            "--policy"
        };
        let reports: Vec<&str> = [
            ("--why", self.why.is_some()),
            ("--features", self.features),
            ("--timings", self.timings.is_some()),
            ("--sizes", self.sizes.is_some()),
            ("--audit", self.audit.is_some()),
            (licences, self.licenses || self.policy.is_some()),
        ]
        .into_iter()
        .filter(|(_, given)| *given)
        .map(|(flag, _)| flag)
        .collect();
        if reports.len() > 1 {
            return usage(format!("give one of {} at a time", reports.join(", ")));
        }
        if self.package.is_some() && !self.features {
            return usage("--package only has an effect with --features".into());
        }
        let measured = self.timings.is_some() || self.sizes.is_some();
        for (flag, given, needs) in [
            ("--top", self.top.is_some(), measured),
            ("--baseline", self.baseline.is_some(), measured),
            ("--save", self.save.is_some(), measured),
        ] {
            if given && !needs {
                return usage(format!(
                    "{flag} only has an effect with --timings or --sizes"
                ));
            }
        }
        if !self.thresholds.is_empty() && self.baseline.is_none() {
            return usage("--threshold only has an effect with --baseline".into());
        }
        for (flag, given) in [
            ("--build-profile", self.build_profile.is_some()),
            ("--source-sizes", self.source_sizes),
        ] {
            if given && self.sizes.is_none() {
                return usage(format!("{flag} only has an effect with --sizes"));
            }
        }
        if self.sbom.is_some() {
            if !(self.licenses || self.policy.is_some()) {
                return usage("--sbom only has an effect with --licenses or --policy".into());
            }
            for (flag, given) in [
                ("--metadata", self.metadata.is_some()),
                ("a manifest", cli.resource.is_some()),
                ("--no-dev", self.no_dev),
            ] {
                if given {
                    return usage(format!("{flag} has no effect with --sbom"));
                }
            }
        }
        if let Some(save) = &self.save {
            if save == "-" || save.is_empty() {
                return usage("--save writes a file: give its path".into());
            }
        }
        // Standard input can be read once.
        let stdin: Vec<&str> = [
            ("--timings", self.timings.as_deref()),
            (
                "--sizes",
                self.sizes.as_ref().and_then(|input| input.as_deref()),
            ),
            ("--baseline", self.baseline.as_deref()),
            ("--audit", self.audit.as_deref()),
            ("--metadata", self.metadata.as_deref()),
            ("--policy", self.policy.as_deref()),
            ("--sbom", self.sbom.as_deref()),
        ]
        .into_iter()
        .filter(|(_, file)| *file == Some("-"))
        .map(|(flag, _)| flag)
        .collect();
        if stdin.len() > 1 {
            return usage(format!(
                "only one of {} can read standard input (`-`)",
                stdin.join(", ")
            ));
        }
        self.thresholds().map_err(|err| (ExitClass::Usage, err))?;
        let Some(report) = reports.first().filter(|r| **r != "--why") else {
            return Ok(());
        };
        let reads_file = matches!(*report, "--timings" | "--audit");
        for (flag, given) in [
            ("--graph", self.graph),
            ("--depth", self.depth.is_some()),
            ("--duplicates", self.duplicates),
            (
                "--no-dev",
                self.no_dev && !matches!(*report, "--licenses" | "--policy" | "--sizes"),
            ),
            ("--metadata", self.metadata.is_some() && reads_file),
            ("a manifest", cli.resource.is_some() && reads_file),
        ] {
            if given {
                return usage(format!("{flag} has no effect with {report}"));
            }
        }
        Ok(())
    }
}

impl GraphSourceOptions {
    /// What `--baseline` counts as a regression: the defaults for the
    /// report, with each `--threshold` in place.
    fn thresholds(&self) -> Result<Thresholds, String> {
        let quantity = self.quantity();
        let mut thresholds = Thresholds::defaults(quantity);
        for value in &self.thresholds {
            thresholds = Threshold::parse(value)
                .and_then(|threshold| thresholds.with(threshold, quantity))
                .map_err(|err| format!("--threshold: {err}"))?;
        }
        Ok(thresholds)
    }

    fn quantity(&self) -> Quantity {
        if self.timings.is_some() {
            Quantity::Seconds
        } else {
            Quantity::Bytes
        }
    }
}

/// A report file for `--timings` or `--audit` (`-` for stdin), read up to
/// `rich_ext::deps::MAX_INPUT` bytes.
fn report_file(cli: &Cli, file: &str) -> Result<String, Failure> {
    let limit = rich_ext::deps::MAX_INPUT as u64;
    read_resource_limited(Some(file), cli.extensions.encoding, Some(limit)).map_err(|err| {
        let name = if file == "-" { "<stdin>" } else { file };
        (ExitClass::Input, format!("cannot read {name}: {err}"))
    })
}

/// `cargo metadata` output: from `--metadata FILE` (`-` for stdin), else by
/// running `cargo metadata --format-version 1` for the manifest the resource
/// names (a `Cargo.toml` or its directory), else for the working directory.
///
/// Cargo runs with the configuration of the directory it is started in
/// (`.cargo/config.toml` up the tree), which can name programs. `cargo
/// metadata` runs `rustc` to learn the target; the project's `rustc`
/// wrappers are turned off for it (an empty `RUSTC_WRAPPER` and
/// `RUSTC_WORKSPACE_WRAPPER` override the configuration), since metadata
/// needs no wrapper. What remains (a `build.rustc` naming another compiler,
/// the toolchain a `rust-toolchain.toml` selects, fetching an index or git
/// dependencies) is Cargo's own behaviour; `--metadata FILE` runs nothing.
fn metadata(cli: &Cli) -> Result<String, Failure> {
    let options = &cli.graph_sources;
    if let Some(file) = &options.metadata {
        if cli.resource.is_some() {
            return Err((
                ExitClass::Usage,
                "give either --metadata FILE or a manifest, not both".into(),
            ));
        }
        return read_resource(Some(file), cli.extensions.encoding).map_err(|err| {
            let name = if file == "-" { "<stdin>" } else { file };
            (ExitClass::Input, format!("cannot read {name}: {err}"))
        });
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command.args(["metadata", "--format-version", "1"]);
    for wrapper in [
        "RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    ] {
        command.env(wrapper, "");
    }
    if let Some(resource) = &cli.resource {
        let mut manifest = fs_path(resource);
        if manifest.is_dir() {
            manifest.push("Cargo.toml");
        }
        command.arg("--manifest-path").arg(manifest);
    }
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| {
            (
                ExitClass::Input,
                format!("cannot run cargo metadata: {err} (or pass --metadata FILE)"),
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("no output");
        return Err((ExitClass::Input, format!("cargo metadata failed: {first}")));
    }
    String::from_utf8(output.stdout).map_err(|_| {
        (
            ExitClass::Input,
            "cargo metadata wrote invalid UTF-8".into(),
        )
    })
}

/// `rich deps`: the dependency tree, `--why CRATE`'s inverted tree, or with
/// `--graph` either drawn through the layout; or one of the supply-chain
/// reports (`--features`, `--timings FILE`, `--sizes`, `--audit FILE`,
/// `--licenses`, `--policy FILE`), which read what Cargo and its tools
/// wrote and run no scanner.
pub(crate) fn deps(cli: &Cli) -> Result<DepsView, Failure> {
    let options = &cli.graph_sources;
    options.check_combination(cli)?;
    let data = |err: rich_ext::deps::DepsError| (ExitClass::Data, err.to_string());
    if let Some(file) = &options.timings {
        let timings = Timings::parse(&report_file(cli, file)?).map_err(data)?;
        return measured(cli, Measured::Timings(timings), shown_name(file));
    }
    if let Some(input) = &options.sizes {
        let (sizes, name) = sizes(cli, input.as_deref())?;
        return measured(cli, Measured::Sizes(sizes), name);
    }
    if let Some(file) = &options.audit {
        let report = AdvisoryReport::from_cargo_audit(&report_file(cli, file)?).map_err(data)?;
        let vulnerabilities = report.vulnerabilities();
        let gate = (vulnerabilities > 0).then(|| {
            let ids: Vec<&str> = report
                .sorted()
                .into_iter()
                .filter(|a| a.kind == AdvisoryKind::Vulnerability)
                .map(|a| a.id.as_str())
                .collect();
            format!(
                "{vulnerabilities} vulnerabilit{} found: {}",
                if vulnerabilities == 1 { "y" } else { "ies" },
                ids.join(", ")
            )
        });
        return Ok(DepsView {
            view: Box::new(report),
            gate,
        });
    }
    if options.licenses || options.policy.is_some() {
        return licences(cli);
    }
    let json = metadata(cli)?;
    if options.features {
        let graph = FeatureGraph::from_json(&json).map_err(data)?;
        let tree = FeatureTree::new(graph, options.package.as_deref()).map_err(data)?;
        return Ok(DepsView::shown(tree));
    }
    let graph = DepGraph::from_json(&json).map_err(data)?;
    let mut kinds = vec![DepKind::Normal, DepKind::Build, DepKind::Dev];
    if options.no_dev {
        kinds.pop();
    }
    if let Some(spec) = &options.why {
        let reachable = graph.reachable(&[DepKind::Normal, DepKind::Build, DepKind::Dev]);
        let why = WhyTree::new(graph, spec).map_err(|err| (ExitClass::Data, err.to_string()))?;
        let named = why.targets().to_vec();
        let why = why.kinds(&kinds);
        if why.targets().is_empty() {
            // Nothing to draw: say why rather than print nothing.
            let message = if options.no_dev && named.iter().any(|&p| reachable[p]) {
                format!("{spec} is reached only through dev-dependencies (drop --no-dev)")
            } else {
                format!("{spec} is not reached from the roots through the dependency kinds chosen")
            };
            return Err((ExitClass::Data, message));
        }
        if options.graph {
            return Ok(DepsView::shown(Diagram::new(why_diagram(&why, &kinds))));
        }
        return Ok(DepsView::shown(why));
    }
    if options.graph {
        return Ok(DepsView::shown(Diagram::new(deps_diagram(
            &graph,
            &kinds,
            options.depth,
            options.duplicates,
        ))));
    }
    // With --duplicates, the consolidation summary follows the tree.
    let mut tree = DepTree::new(graph)
        .kinds(&kinds)
        .duplicates_only(options.duplicates)
        .summary(true)
        .consolidation(options.duplicates);
    if let Some(depth) = options.depth {
        tree = tree.max_depth(depth);
    }
    Ok(DepsView::shown(tree))
}

/// A report `--baseline` compares and `--save` keeps.
enum Measured {
    Timings(Timings),
    Sizes(Sizes),
}

impl Measured {
    fn to_json(&self) -> String {
        match self {
            Measured::Timings(timings) => timings.to_json(),
            Measured::Sizes(sizes) => sizes.to_json(),
        }
    }
}

/// `--timings` or `--sizes`: saved with `--save FILE`, then shown, or with
/// `--baseline FILE` compared with that (exit 5 on a regression).
fn measured(cli: &Cli, current: Measured, name: String) -> Result<DepsView, Failure> {
    let options = &cli.graph_sources;
    if let Some(file) = &options.save {
        std::fs::write(fs_path(file), current.to_json())
            .map_err(|err| (ExitClass::Input, format!("cannot write {file}: {err}")))?;
    }
    let Some(file) = &options.baseline else {
        let view: Box<dyn Renderable> = match current {
            Measured::Timings(timings) => {
                let report = TimingsReport::new(timings);
                Box::new(match options.top {
                    Some(top) => report.limit(top),
                    None => report,
                })
            }
            Measured::Sizes(sizes) => {
                let report = SizeReport::new(sizes);
                Box::new(match options.top {
                    Some(top) => report.limit(top),
                    None => report,
                })
            }
        };
        return Ok(DepsView { view, gate: None });
    };
    let thresholds = options
        .thresholds()
        .map_err(|err| (ExitClass::Usage, err))?;
    let data =
        |err: rich_ext::deps::DepsError| (ExitClass::Data, format!("{}: {err}", shown_name(file)));
    let mut comparison = match current {
        Measured::Timings(current) => {
            let baseline = Timings::parse(&report_file(cli, file)?).map_err(data)?;
            Comparison::timings(&baseline, &current, thresholds)
        }
        Measured::Sizes(current) => {
            let baseline = if file != "-" && fs_path(file).is_dir() {
                let json = metadata(cli)?;
                size_dir(cli, file, &json)?
            } else {
                Sizes::parse(&report_file(cli, file)?).map_err(data)?
            };
            Comparison::sizes(&baseline, &current, thresholds)
        }
    };
    comparison = comparison.names(shown_name(file), name);
    if let Some(top) = options.top {
        comparison = comparison.limit(top);
    }
    let gate = comparison.failed().then(|| {
        let mut names: Vec<String> = comparison
            .regressions()
            .iter()
            .map(|row| row.key.clone())
            .collect();
        if comparison.total().verdict == Verdict::Regression {
            names.push("the total".into());
        }
        format!(
            "{} regression{} against the baseline ({}): {}",
            names.len(),
            if names.len() == 1 { "" } else { "s" },
            thresholds.describe(comparison.quantity()),
            names.join(", ")
        )
    });
    Ok(DepsView {
        view: Box::new(comparison),
        gate,
    })
}

/// The sizes `--sizes` reads, and the name the report goes by: a target
/// directory (the one `cargo metadata` names when none is given) matched
/// to the packages in `cargo metadata`, or a file of cargo-bloat JSON or a
/// saved report.
fn sizes(cli: &Cli, input: Option<&str>) -> Result<(Sizes, String), Failure> {
    let options = &cli.graph_sources;
    let directory = match input {
        None => true,
        Some("-") => false,
        Some(path) => fs_path(path).is_dir(),
    };
    if !directory {
        let file = input.unwrap_or("-");
        if file != "-"
            && fs_path(file)
                .file_name()
                .is_some_and(|name| name == "Cargo.toml")
        {
            return Err((
                ExitClass::Usage,
                format!(
                    "{file} is a manifest, not a size report: give it before --sizes \
                     (rich deps MANIFEST --sizes)"
                ),
            ));
        }
        let baseline_dir = options
            .baseline
            .as_deref()
            .is_some_and(|b| b != "-" && fs_path(b).is_dir());
        for (flag, given) in [
            ("--build-profile", options.build_profile.is_some()),
            ("--source-sizes", options.source_sizes),
            ("--no-dev", options.no_dev),
            ("--metadata", options.metadata.is_some() && !baseline_dir),
            ("a manifest", cli.resource.is_some() && !baseline_dir),
        ] {
            if given {
                return Err((
                    ExitClass::Usage,
                    format!(
                        "{flag} has no effect with a size report file: it reads a target directory"
                    ),
                ));
            }
        }
        let sizes = Sizes::parse(&report_file(cli, file)?)
            .map_err(|err| (ExitClass::Data, format!("{}: {err}", shown_name(file))))?;
        return Ok((sizes, shown_name(file)));
    }
    let json = metadata(cli)?;
    let target = match input {
        Some(path) => path.to_string(),
        None => rich_ext::deps::sizes::target_directory(&json)
            .map_err(|err| (ExitClass::Data, err.to_string()))?
            .display()
            .to_string(),
    };
    let sizes = size_dir(cli, &target, &json)?;
    let name = match sizes.source() {
        rich_ext::deps::sizes::SizeSource::TargetDir { dir, .. } => dir.clone(),
        _ => target,
    };
    Ok((sizes, name))
}

/// The sizes in a target directory, matched to `json`'s packages; with
/// `--no-dev`, not the dev-only ones.
fn size_dir(cli: &Cli, target: &str, json: &str) -> Result<Sizes, Failure> {
    let options = &cli.graph_sources;
    let path = fs_path(target);
    if !path.is_dir() {
        return Err((
            ExitClass::Input,
            format!("{target}: not a directory (build first, or give the target directory)"),
        ));
    }
    let mut size_options = SizeOptions::default().sources(options.source_sizes);
    size_options.profile = options.build_profile.clone();
    rich_ext::deps::sizes::profile_dir(&path, size_options.profile.as_deref())
        .map_err(|err| (ExitClass::Input, err.to_string()))?;
    let sizes = Sizes::from_target_dir(&path, json, &size_options)
        .map_err(|err| (ExitClass::Data, err.to_string()))?;
    if !options.no_dev {
        return Ok(sizes);
    }
    let graph = DepGraph::from_json(json).map_err(|err| (ExitClass::Data, err.to_string()))?;
    let reached = graph.reachable(&[DepKind::Normal, DepKind::Build]);
    let ids: std::collections::HashSet<&str> = graph
        .packages()
        .iter()
        .zip(&reached)
        .filter(|(_, reached)| **reached)
        .map(|(package, _)| package.id.as_str())
        .collect();
    Ok(sizes.retain(|entry| entry.id.as_deref().is_none_or(|id| ids.contains(id))))
}

/// `--licenses` and `--policy FILE`: the packages from `cargo metadata` (the
/// ones the kinds reach) or from `--sbom FILE`, grouped by licence, checked
/// against the policy, or both. A violation (or, under `conflicts =
/// "deny"`, a conflict) exits 5 once the report is shown.
fn licences(cli: &Cli) -> Result<DepsView, Failure> {
    let options = &cli.graph_sources;
    let data = |err: rich_ext::deps::DepsError| (ExitClass::Data, err.to_string());
    let policy = match &options.policy {
        Some(file) => {
            let text = report_file(cli, file)?;
            let policy = Policy::from_toml(&text)
                .map_err(|err| (ExitClass::Data, format!("{}: {err}", shown_name(file))))?;
            Some((policy, shown_name(file)))
        }
        None => None,
    };
    // The packages, and for each root the packages it ships with (its
    // normal dependencies, all the way down), for the conflicts.
    let (report, roots): (LicenseReport, Vec<(LicensedPackage, Vec<LicensedPackage>)>) =
        match &options.sbom {
            Some(file) => {
                let sbom = Sbom::parse(&report_file(cli, file)?)
                    .map_err(|err| (ExitClass::Data, format!("{}: {err}", shown_name(file))))?;
                let roots = sbom
                    .root()
                    .map(|root| (root.clone(), sbom.report().packages().to_vec()))
                    .into_iter()
                    .collect();
                (sbom.into_report(), roots)
            }
            None => {
                let json = metadata(cli)?;
                let graph = DepGraph::from_json(&json).map_err(data)?;
                let mut kinds = vec![DepKind::Normal, DepKind::Build, DepKind::Dev];
                if options.no_dev {
                    kinds.pop();
                }
                // The packages the kinds reach: with --no-dev, not the dev-only ones.
                let reachable = graph.reachable(&kinds);
                let ids: std::collections::HashSet<&str> = graph
                    .packages()
                    .iter()
                    .zip(&reachable)
                    .filter(|(_, reached)| **reached)
                    .map(|(package, _)| package.id.as_str())
                    .collect();
                let mut report = LicenseReport::from_json(&json)
                    .map_err(data)?
                    .retain(|package| ids.contains(package.id.as_str()));
                if let Some((policy, _)) = policy.as_ref().filter(|(p, _)| p.ignores_private()) {
                    let private =
                        policy::unpublished(&json, policy.private_registries()).map_err(data)?;
                    report = report.retain(|package| !private.contains(&package.id));
                }
                let mut roots = Vec::new();
                let index: std::collections::HashMap<&str, usize> = graph
                    .packages()
                    .iter()
                    .enumerate()
                    .map(|(at, package)| (package.id.as_str(), at))
                    .collect();
                if policy
                    .as_ref()
                    .is_some_and(|(p, _)| p.conflict_mode() != ConflictMode::Ignore)
                {
                    for &root in graph.roots() {
                        let id = &graph.packages()[root].id;
                        let Some(package) = report.packages().iter().find(|p| &p.id == id) else {
                            continue;
                        };
                        let mut alone = graph.clone();
                        alone.set_roots(vec![root]);
                        let ships = alone.reachable(&[DepKind::Normal]);
                        let shipped: Vec<LicensedPackage> = report
                            .packages()
                            .iter()
                            .filter(|p| index.get(p.id.as_str()).is_some_and(|&at| ships[at]))
                            .cloned()
                            .collect();
                        roots.push((package.clone(), shipped));
                    }
                }
                (report, roots)
            }
        };
    let Some((policy, name)) = policy else {
        return Ok(DepsView::shown(report));
    };
    let conflicts = roots
        .iter()
        .flat_map(|(root, shipped)| policy::conflicts(root, shipped))
        .collect();
    let mut checked = PolicyReport::new(&report, &policy)
        .named(name)
        .with_conflicts(conflicts);
    if options.sbom.is_some() && policy.ignores_private() {
        checked = checked.note("`private.ignore` needs cargo metadata to tell what is unpublished; not applied to an SBOM");
    }
    let gate = checked.failed().then(|| {
        let violations = checked.violations();
        let mut parts = Vec::new();
        if !violations.is_empty() {
            let named: Vec<String> = violations
                .iter()
                .map(|v| match v.license.as_str() {
                    "(none)" => format!("{} v{} (no licence)", v.name, v.version),
                    "(licence file)" => format!("{} v{} (a licence file)", v.name, v.version),
                    license => format!("{} v{} ({license})", v.name, v.version),
                })
                .collect();
            parts.push(format!(
                "{} licence policy violation{}: {}",
                violations.len(),
                if violations.len() == 1 { "" } else { "s" },
                named.join(", ")
            ));
        }
        let conflicts: Vec<String> = checked
            .conflicts()
            .iter()
            .filter(|c| {
                policy.conflict_mode() == ConflictMode::Deny
                    && c.severity == policy::Severity::Conflict
            })
            .map(|c| format!("{} v{} ({})", c.name, c.version, c.license))
            .collect();
        if !conflicts.is_empty() {
            parts.push(format!(
                "{} licence conflict{}: {}",
                conflicts.len(),
                if conflicts.len() == 1 { "" } else { "s" },
                conflicts.join(", ")
            ));
        }
        parts.join("; ")
    });
    let view: Box<dyn Renderable> = if options.licenses {
        Box::new(Renderables::new(vec![
            std::sync::Arc::new(report),
            std::sync::Arc::new(Text::new("")),
            std::sync::Arc::new(checked),
        ]))
    } else {
        Box::new(checked)
    };
    Ok(DepsView { view, gate })
}

/// A package as a diagram node: workspace members rounded, crates resolved
/// at several versions as hexagons (so they stand out without colour).
fn package_node(graph: &DepGraph, package: usize, duplicates: &[String]) -> Node {
    let info = &graph.packages()[package];
    let shape = if duplicates.contains(&info.name) {
        Shape::Hexagon
    } else if graph.is_member(package) {
        Shape::Round
    } else {
        Shape::Rect
    };
    Node::new(
        format!("{}@{}", info.name, info.version),
        graph.display(package),
    )
    .shape(shape)
}

fn add_edge(diagram: &mut Graph, from: usize, to: usize, kinds: &[DepKind]) {
    let mut edge = rich_diagram::Edge::new(from, to);
    if !kinds.contains(&DepKind::Normal) {
        edge.stroke = Stroke::Dotted;
        edge.label = Some(
            if kinds.contains(&DepKind::Build) {
                "build"
            } else {
                "dev"
            }
            .into(),
        );
    }
    diagram.add_edge(edge);
}

/// The packages reachable from the roots within `depth` levels, and the
/// edges between them; with `duplicates_only`, only those on a path to a
/// crate resolved at several versions.
fn deps_diagram(
    graph: &DepGraph,
    kinds: &[DepKind],
    depth: Option<usize>,
    duplicates_only: bool,
) -> Graph {
    let duplicates: Vec<String> = graph.duplicates_in(kinds).into_keys().collect();
    let mut diagram = Graph::new(Direction::LeftRight);
    let mut index: Vec<Option<usize>> = vec![None; graph.packages().len()];
    let mut level: Vec<Option<usize>> = vec![None; graph.packages().len()];
    let mut queue = std::collections::VecDeque::new();
    for &root in graph.roots() {
        level[root] = Some(0);
        queue.push_back(root);
    }
    let mut order = Vec::new();
    while let Some(package) = queue.pop_front() {
        order.push(package);
        let here = level[package].unwrap_or(0);
        if depth.is_some_and(|max| here >= max) {
            continue;
        }
        let root = graph.is_root(package);
        for dep in graph.deps(package) {
            let follows = dep
                .kinds
                .iter()
                .any(|k| kinds.contains(k) && (root || *k != DepKind::Dev));
            if follows && level[dep.package].is_none() {
                level[dep.package] = Some(here + 1);
                queue.push_back(dep.package);
            }
        }
    }
    // The edges drawn: between packages within `depth`, of the kinds asked.
    let mut edges = Vec::new();
    for &package in &order {
        let root = graph.is_root(package);
        for dep in graph.deps(package) {
            let kinds: Vec<DepKind> = dep
                .kinds
                .iter()
                .copied()
                .filter(|k| kinds.contains(k) && (root || *k != DepKind::Dev))
                .collect();
            if level[dep.package].is_some() && !kinds.is_empty() {
                edges.push((package, dep.package, kinds));
            }
        }
    }
    if duplicates_only {
        // Keep the packages that are, or lead through drawn edges to, a
        // duplicate.
        let mut leads: Vec<bool> = (0..graph.packages().len())
            .map(|p| level[p].is_some() && duplicates.contains(&graph.packages()[p].name))
            .collect();
        let mut changed = true;
        while changed {
            changed = false;
            for (from, to, _) in &edges {
                if leads[*to] && !leads[*from] {
                    leads[*from] = true;
                    changed = true;
                }
            }
        }
        order.retain(|&p| leads[p]);
        edges.retain(|(from, to, _)| leads[*from] && leads[*to]);
    }
    for &package in &order {
        index[package] = Some(diagram.add_node(package_node(graph, package, &duplicates)));
    }
    for (from, to, kinds) in &edges {
        if let (Some(from), Some(to)) = (index[*from], index[*to]) {
            add_edge(&mut diagram, from, to, kinds);
        }
    }
    diagram
}

/// The crate `--why` asked about, everything that depends on it up to the
/// roots through the kinds in `kinds`, and those edges.
fn why_diagram(why: &WhyTree, kinds: &[DepKind]) -> Graph {
    let graph = why.graph();
    let duplicates: Vec<String> = graph.duplicates_in(kinds).into_keys().collect();
    let dependents = graph.dependents(kinds);
    let mut keep = vec![false; graph.packages().len()];
    let mut stack: Vec<usize> = why.targets().to_vec();
    while let Some(package) = stack.pop() {
        if std::mem::replace(&mut keep[package], true) || graph.is_root(package) {
            continue;
        }
        stack.extend(dependents[package].iter().map(|(parent, _)| *parent));
    }
    let mut diagram = Graph::new(Direction::TopDown);
    let mut index: Vec<Option<usize>> = vec![None; graph.packages().len()];
    for package in (0..keep.len()).filter(|&p| keep[p]) {
        index[package] = Some(diagram.add_node(package_node(graph, package, &duplicates)));
    }
    for package in (0..keep.len()).filter(|&p| keep[p]) {
        for dep in graph.deps(package) {
            // The kinds the inverse traversal followed, if it followed it.
            let followed = dependents[dep.package]
                .iter()
                .find(|(parent, _)| *parent == package);
            if let (Some(from), Some(to), Some((_, kinds))) =
                (index[package], index[dep.package], followed)
            {
                add_edge(&mut diagram, from, to, kinds);
            }
        }
    }
    diagram
}

/// How a schema is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemaFormat {
    /// JSON Schema.
    Json,
    /// SQL DDL: `CREATE TABLE` statements.
    Sql,
    /// An Arrow IPC file or stream (Feather v2).
    Arrow,
}

/// The format a resource's extension names, if it names one.
fn schema_format_by_name(resource: &str) -> Option<SchemaFormat> {
    let path = resource.split(['?', '#']).next().unwrap_or(resource);
    let extension = path
        .rsplit(['/', '\\'])
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("json") => Some(SchemaFormat::Json),
        Some("sql" | "ddl") => Some(SchemaFormat::Sql),
        Some("arrow" | "feather" | "arrows" | "ipc") => Some(SchemaFormat::Arrow),
        _ => None,
    }
}

/// Whether a local file starts with an Arrow IPC file's magic bytes. Only a
/// regular file is looked at: reading a pipe (`/dev/stdin`, `<(…)`) would
/// take its first bytes from the text read next.
fn has_arrow_magic(resource: &str) -> bool {
    use std::io::Read;
    let path = fs_path(resource);
    if !std::fs::metadata(&path).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let mut magic = [0u8; 6];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok_and(|()| &magic == b"ARROW1")
}

/// Whether text with no telling extension is SQL rather than JSON: it
/// starts with a SQL comment or with `CREATE`, which JSON never does.
fn looks_like_sql(content: &str) -> bool {
    let text = content.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("--")
        || text.starts_with("/*")
        || text
            .get(..6)
            .is_some_and(|word| word.eq_ignore_ascii_case("create"))
}

/// A schema read: JSON Schema stays JSON (its views follow `$ref`s), every
/// other format is in the model.
enum LoadedSchema {
    Json(serde_json::Value),
    Model(Schema),
}

impl LoadedSchema {
    fn into_model(self) -> Schema {
        match self {
            LoadedSchema::Json(value) => rich_ext::schema::json::to_model(&value),
            LoadedSchema::Model(model) => model,
        }
    }
}

/// Where a schema came from, and the reader's notes on what it skipped.
struct SchemaSource {
    name: String,
    notes: Vec<String>,
}

/// A schema file (or URL, or `-`) read and parsed: by extension (`.json`;
/// `.sql`, `.ddl`; `.arrow`, `.feather`, `.arrows`, `.ipc`), else an Arrow
/// file by its magic bytes, else SQL when the text starts like SQL, else
/// JSON Schema.
fn schema_file(cli: &Cli, resource: &str) -> Result<(LoadedSchema, SchemaSource), Failure> {
    // `--sanitize`: the file's name, in the view and in errors, shows
    // terminal controls as inert text, whatever the format.
    let inert = |text: &str| {
        if cli.sanitize {
            sanitize_terminal_controls(text)
        } else {
            text.to_string()
        }
    };
    let name = inert(&shown_name(resource));
    let label = inert(resource);
    let local = !is_url(resource) && resource != "-";
    let mut format = schema_format_by_name(resource);
    if format.is_none() && local && has_arrow_magic(resource) {
        format = Some(SchemaFormat::Arrow);
    }
    if format == Some(SchemaFormat::Arrow) {
        if !local {
            return Err((
                ExitClass::Usage,
                format!("{name}: an Arrow schema is read from a file, not a URL or stdin"),
            ));
        }
        let schema = LoadedSchema::Model(arrow_schema(resource, &label)?);
        let notes = Vec::new();
        return Ok((schema, SchemaSource { name, notes }));
    }
    let content = if is_url(resource) {
        fetch_url(resource, cli.extensions.encoding)
            .map(|(content, _)| content)
            .map_err(|err| (ExitClass::Input, err))?
    } else {
        read_resource(Some(resource), cli.extensions.encoding).map_err(|err| {
            let name = if resource == "-" { "<stdin>" } else { &label };
            (ExitClass::Input, format!("cannot read {name}: {err}"))
        })?
    };
    let sql = match format {
        Some(format) => format == SchemaFormat::Sql,
        None => looks_like_sql(&content),
    };
    if sql {
        // The text's names and strings show terminal controls as inert
        // text; the whitespace the reader separates words by stays
        // whitespace, or a statement split by it would be skipped.
        let content = if cli.sanitize {
            sanitize_terminal_controls(&sql_whitespace_as_spaces(&content))
        } else {
            content
        };
        let parsed = rich_ext::schema::sql::parse(&content)
            .map_err(|err| (ExitClass::Data, format!("{label}: {err}")))?;
        let notes = parsed.notes.iter().map(ToString::to_string).collect();
        let schema = LoadedSchema::Model(parsed.schema);
        return Ok((schema, SchemaSource { name, notes }));
    }
    let mut value = rich_ext::schema::parse(&content)
        .map_err(|err| (ExitClass::Data, format!("{label}: {err}")))?;
    if cli.sanitize {
        sanitize_json_schema(&mut value);
    }
    let notes = Vec::new();
    Ok((LoadedSchema::Json(value), SchemaSource { name, notes }))
}

/// Terminal controls in a JSON Schema's strings and keys (property names)
/// as inert text, for `--sanitize`.
fn sanitize_json_schema(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => *text = sanitize_terminal_controls(text),
        serde_json::Value::Array(values) => values.iter_mut().for_each(sanitize_json_schema),
        serde_json::Value::Object(map) => {
            let entries = std::mem::take(map);
            // Keys already inert keep their names, so a renamed key never
            // takes the place of one written that way.
            let taken: std::collections::HashSet<String> = entries
                .keys()
                .filter(|key| sanitize_terminal_controls(key) == **key)
                .cloned()
                .collect();
            for (key, mut value) in entries {
                sanitize_json_schema(&mut value);
                let mut shown = sanitize_terminal_controls(&key);
                if shown != key {
                    // Two keys that read the same once sanitized are both
                    // kept: the later one numbered.
                    let base = shown.clone();
                    let mut n = 2;
                    while taken.contains(&shown) || map.contains_key(&shown) {
                        shown = format!("{base} ({n})");
                        n += 1;
                    }
                }
                map.insert(shown, value);
            }
        }
        _ => {}
    }
}

/// DDL with the control characters the SQL reader takes as whitespace (CR,
/// vertical tab, form feed, NEL) as spaces, and CRLF as LF, so that
/// sanitizing the text leaves its words apart.
fn sql_whitespace_as_spaces(content: &str) -> String {
    content
        .replace("\r\n", "\n")
        .replace(['\r', '\x0b', '\x0c', '\u{85}'], " ")
}

/// An Arrow IPC file's schema, in the model.
#[cfg(feature = "arrow")]
fn arrow_schema(resource: &str, label: &str) -> Result<Schema, Failure> {
    let file = std::fs::File::open(fs_path(resource))
        .map_err(|err| (ExitClass::Input, format!("cannot read {label}: {err}")))?;
    let schema = rich_data::arrow::read_schema(std::io::BufReader::new(file))
        .map_err(|err| (ExitClass::Data, format!("{label}: {err}")))?;
    Ok(rich_data::arrow::schema(&schema))
}

/// Without the `arrow` feature an Arrow schema cannot be read: a usage
/// error that names the feature, rather than a JSON parse error.
#[cfg(not(feature = "arrow"))]
fn arrow_schema(_resource: &str, label: &str) -> Result<Schema, Failure> {
    Err((
        ExitClass::Usage,
        format!(
            "{label}: reading an Arrow schema needs rich built with the `arrow` feature \
             (cargo install rs-rich-cli --features arrow)",
        ),
    ))
}

/// The view, then each reader note (what DDL the reader skipped), dimmed,
/// under a blank line.
fn with_notes(
    view: impl Renderable + Send + Sync + 'static,
    inputs: &[&SchemaSource],
) -> Box<dyn Renderable> {
    let notes: Vec<String> = inputs
        .iter()
        .flat_map(|input| {
            input
                .notes
                .iter()
                .map(move |note| format!("{}: {note}", input.name))
        })
        .collect();
    if notes.is_empty() {
        return Box::new(view);
    }
    let mut text = Text::new("");
    text.append(
        &notes.join("\n"),
        Some(Style::parse("dim").expect("valid style").into()),
    );
    Box::new(Renderables::new(vec![
        std::sync::Arc::new(view),
        std::sync::Arc::new(Text::new("")),
        std::sync::Arc::new(text),
    ]))
}

/// Two models compared: when one is a schema of tables with a single table
/// and the other has no tables (a JSON Schema or an Arrow file against one
/// `CREATE TABLE`), that table is what is compared.
fn comparable(old: Schema, new: Schema) -> (Schema, Schema) {
    let single = |schema: &Schema| match schema.tables() {
        [one] => Some(one.clone()),
        _ => None,
    };
    match (old.tables().is_empty(), new.tables().is_empty()) {
        (true, false) => match single(&new) {
            Some(table) => (old, table),
            None => (old, new),
        },
        (false, true) => match single(&old) {
            Some(table) => (table, new),
            None => (old, new),
        },
        _ => (old, new),
    }
}

/// `rich schema FILE`: the tree; `rich schema OLD NEW`: what changed;
/// `rich schema --er FILE`: the ER diagram.
pub(crate) fn schema(cli: &Cli) -> Result<Box<dyn Renderable>, Failure> {
    let resources: Vec<String> = if cli.resources.is_empty() {
        vec!["-".into()]
    } else {
        cli.resources.clone()
    };
    if cli.schema.er && resources.len() != 1 {
        return Err((
            ExitClass::Usage,
            "--er draws one schema: rich schema --er FILE".into(),
        ));
    }
    match resources.as_slice() {
        [one] => {
            let (schema, input) = schema_file(cli, one)?;
            if cli.schema.er {
                let model = rich_data::er::model(&schema.into_model());
                return Ok(with_notes(rich_diagram::ErDiagram::new(model), &[&input]));
            }
            match schema {
                // JSON Schema keeps its own tree (and its title): unchanged.
                LoadedSchema::Json(value) => Ok(Box::new(SchemaTree::new(value))),
                // DDL and Arrow are titled with the file they came from.
                LoadedSchema::Model(model) => {
                    let mut tree = SchemaTree::from_model(model);
                    if one != "-" {
                        tree = tree.title(input.name.clone());
                    }
                    Ok(with_notes(tree, &[&input]))
                }
            }
        }
        [old, new] => {
            if old == "-" && new == "-" {
                return Err((
                    ExitClass::Usage,
                    "only one schema can come from stdin".into(),
                ));
            }
            let (old_schema, old_input) = schema_file(cli, old)?;
            let (new_schema, new_input) = schema_file(cli, new)?;
            let names = (old_input.name.clone(), new_input.name.clone());
            if let (LoadedSchema::Json(a), LoadedSchema::Json(b)) = (&old_schema, &new_schema) {
                return Ok(Box::new(SchemaDiff::new(a, b).names(names.0, names.1)));
            }
            let (a, b) = comparable(old_schema.into_model(), new_schema.into_model());
            let diff = SchemaDiff::models(&a, &b).names(names.0, names.1);
            Ok(with_notes(diff, &[&old_input, &new_input]))
        }
        _ => Err((
            ExitClass::Usage,
            "schema takes one schema, or two to compare: rich schema OLD NEW".into(),
        )),
    }
}

fn shown_name(resource: &str) -> String {
    if resource == "-" {
        return "<stdin>".into();
    }
    resource
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(resource)
        .to_string()
}

/// `rich dot FILE`: the source parsed natively, and refused (exit 4, naming
/// the construct and its line) when it uses something the native parser
/// does not draw; never a partial drawing.
pub(crate) fn dot(source: &str, resource: Option<&str>) -> Result<Box<dyn Renderable>, Failure> {
    let diagram = rich_diagram::Dot::new(source);
    if let Err(error) = diagram.parsed() {
        let name = resource.filter(|r| *r != "-").unwrap_or("<stdin>");
        return Err((ExitClass::Data, format!("{name}: {error}")));
    }
    Ok(Box::new(diagram))
}

/// With `--dot-backend graphviz` and `--export-svg PATH`, write Graphviz's
/// own SVG of `source` to PATH. A failure is a warning: the export falls
/// back to the native drawing's SVG.
pub(crate) fn graphviz_svg(source: &str, path: &str) -> bool {
    let options = rich_diagram::graphviz::GraphvizOptions::default();
    match rich_diagram::graphviz::render_svg(source, &options) {
        Ok(svg) => match std::fs::write(fs_path(path), svg) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("rich: warning: cannot write {path}: {err}; exporting the text drawing");
                false
            }
        },
        Err(err) => {
            eprintln!("rich: warning: {err}; exporting the text drawing as SVG");
            false
        }
    }
}
