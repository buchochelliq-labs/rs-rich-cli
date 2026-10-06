//! `rich dot`, `rich deps` and `rich schema`: diagrams from real sources
//! (0.0.15 workstream 4; SQL DDL, Arrow and `--er` in 0.0.16 workstream 7).
//! Not upstream: binary-boundary conveniences that compose rs-rich-diagram
//! (the DOT parser, the layout), rs-rich-ext (the dependency and schema
//! trees) and rs-rich-data (Arrow schemas, ER diagrams); see
//! `docs/PORTING.md`.
use super::*;

use rich::containers::Renderables;
use rich_diagram::{Diagram, Direction, Graph, Node, Shape, Stroke};
use rich_ext::deps::audit::{AdvisoryKind, AdvisoryReport};
use rich_ext::deps::features::{FeatureGraph, FeatureTree};
use rich_ext::deps::licenses::LicenseReport;
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
}

impl GraphSourceOptions {
    /// Consume one of these options; anything else stays with the main parser.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
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
        ]
        .into_iter()
        .filter(|(_, given)| *given)
        .map(|(flag, _)| (flag, &["deps"][..]))
        .collect()
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
/// vulnerability in the report exits 5 once the report is shown.
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
        let reports: Vec<&str> = [
            ("--why", self.why.is_some()),
            ("--features", self.features),
            ("--timings", self.timings.is_some()),
            ("--audit", self.audit.is_some()),
            ("--licenses", self.licenses),
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
        let Some(report) = reports.first().filter(|r| **r != "--why") else {
            return Ok(());
        };
        let reads_file = matches!(*report, "--timings" | "--audit");
        for (flag, given) in [
            ("--graph", self.graph),
            ("--depth", self.depth.is_some()),
            ("--duplicates", self.duplicates),
            ("--no-dev", self.no_dev && *report != "--licenses"),
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
/// reports (`--features`, `--timings FILE`, `--audit FILE`, `--licenses`),
/// which read what Cargo and its tools wrote and run no scanner.
pub(crate) fn deps(cli: &Cli) -> Result<DepsView, Failure> {
    let options = &cli.graph_sources;
    options.check_combination(cli)?;
    let data = |err: rich_ext::deps::DepsError| (ExitClass::Data, err.to_string());
    if let Some(file) = &options.timings {
        let timings = Timings::parse(&report_file(cli, file)?).map_err(data)?;
        return Ok(DepsView::shown(TimingsReport::new(timings)));
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
    if options.licenses {
        // The packages the kinds reach: with --no-dev, not the dev-only ones.
        let reachable = graph.reachable(&kinds);
        let ids: std::collections::HashSet<&str> = graph
            .packages()
            .iter()
            .zip(&reachable)
            .filter(|(_, reached)| **reached)
            .map(|(package, _)| package.id.as_str())
            .collect();
        let report = LicenseReport::from_json(&json)
            .map_err(data)?
            .retain(|package| ids.contains(package.id.as_str()));
        return Ok(DepsView::shown(report));
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
        let root = graph.roots().contains(&package);
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
        let root = graph.roots().contains(&package);
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
        if std::mem::replace(&mut keep[package], true) || graph.roots().contains(&package) {
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
    let name = shown_name(resource);
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
        let schema = LoadedSchema::Model(arrow_schema(resource)?);
        let notes = Vec::new();
        return Ok((schema, SchemaSource { name, notes }));
    }
    let content = if is_url(resource) {
        fetch_url(resource, cli.extensions.encoding)
            .map(|(content, _)| content)
            .map_err(|err| (ExitClass::Input, err))?
    } else {
        read_resource(Some(resource), cli.extensions.encoding).map_err(|err| {
            let name = if resource == "-" { "<stdin>" } else { resource };
            (ExitClass::Input, format!("cannot read {name}: {err}"))
        })?
    };
    let sql = match format {
        Some(format) => format == SchemaFormat::Sql,
        None => looks_like_sql(&content),
    };
    // `--sanitize`: the name, and the text's names and strings, show
    // terminal controls as inert text.
    let name = if cli.sanitize {
        sanitize_terminal_controls(&name)
    } else {
        name
    };
    if sql {
        let content = if cli.sanitize {
            // A line break stays one (CR would show as `␍` in a name).
            sanitize_terminal_controls(&content.replace("\r\n", "\n"))
        } else {
            content
        };
        let parsed = rich_ext::schema::sql::parse(&content)
            .map_err(|err| (ExitClass::Data, format!("{resource}: {err}")))?;
        let notes = parsed.notes.iter().map(ToString::to_string).collect();
        let schema = LoadedSchema::Model(parsed.schema);
        return Ok((schema, SchemaSource { name, notes }));
    }
    let mut value = rich_ext::schema::parse(&content)
        .map_err(|err| (ExitClass::Data, format!("{resource}: {err}")))?;
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
            for (key, mut value) in std::mem::take(map) {
                sanitize_json_schema(&mut value);
                map.insert(sanitize_terminal_controls(&key), value);
            }
        }
        _ => {}
    }
}

/// An Arrow IPC file's schema, in the model.
#[cfg(feature = "arrow")]
fn arrow_schema(resource: &str) -> Result<Schema, Failure> {
    let file = std::fs::File::open(fs_path(resource))
        .map_err(|err| (ExitClass::Input, format!("cannot read {resource}: {err}")))?;
    let schema = rich_data::arrow::read_schema(std::io::BufReader::new(file))
        .map_err(|err| (ExitClass::Data, format!("{resource}: {err}")))?;
    Ok(rich_data::arrow::schema(&schema))
}

/// Without the `arrow` feature an Arrow schema cannot be read: a usage
/// error that names the feature, rather than a JSON parse error.
#[cfg(not(feature = "arrow"))]
fn arrow_schema(resource: &str) -> Result<Schema, Failure> {
    Err((
        ExitClass::Usage,
        format!(
            "{}: reading an Arrow schema needs rich built with the `arrow` feature \
             (cargo install rs-rich-cli --features arrow)",
            shown_name(resource)
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
            let names = (shown_name(old), shown_name(new));
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
