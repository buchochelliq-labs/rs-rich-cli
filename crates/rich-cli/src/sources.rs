//! `rich dot`, `rich deps` and `rich schema`: diagrams from real sources
//! (0.0.15 workstream 4). Not upstream: binary-boundary conveniences that
//! compose rs-rich-diagram (the DOT parser, the layout) and rs-rich-ext (the
//! dependency and schema trees); see `docs/PORTING.md`.
use super::*;

use rich_diagram::{Diagram, Direction, Graph, Node, Shape, Stroke};
use rich_ext::deps::{DepGraph, DepKind, DepTree, WhyTree};
use rich_ext::schema::{SchemaDiff, SchemaTree};

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
        ]
        .into_iter()
        .filter(|(_, given)| *given)
        .map(|(flag, _)| (flag, &["deps"][..]))
        .collect()
    }
}

type Failure = (ExitClass, String);

/// `cargo metadata` output: from `--metadata FILE` (`-` for stdin), else by
/// running `cargo metadata --format-version 1` for the manifest the resource
/// names (a `Cargo.toml` or its directory), else for the working directory.
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
/// `--graph` either drawn through the layout.
pub(crate) fn deps(cli: &Cli) -> Result<Box<dyn Renderable>, Failure> {
    let options = &cli.graph_sources;
    let json = metadata(cli)?;
    let graph = DepGraph::from_json(&json).map_err(|err| (ExitClass::Data, err.to_string()))?;
    let mut kinds = vec![DepKind::Normal, DepKind::Build, DepKind::Dev];
    if options.no_dev {
        kinds.pop();
    }
    if let Some(spec) = &options.why {
        let why = WhyTree::new(graph, spec).map_err(|err| (ExitClass::Data, err.to_string()))?;
        if options.graph {
            return Ok(Box::new(Diagram::new(why_diagram(&why))));
        }
        return Ok(Box::new(why));
    }
    if options.graph {
        return Ok(Box::new(Diagram::new(deps_diagram(
            &graph,
            &kinds,
            options.depth,
        ))));
    }
    let mut tree = DepTree::new(graph)
        .kinds(&kinds)
        .duplicates_only(options.duplicates)
        .summary(true);
    if let Some(depth) = options.depth {
        tree = tree.max_depth(depth);
    }
    Ok(Box::new(tree))
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
/// edges between them.
fn deps_diagram(graph: &DepGraph, kinds: &[DepKind], depth: Option<usize>) -> Graph {
    let duplicates: Vec<String> = graph.duplicates().into_keys().collect();
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
    for &package in &order {
        index[package] = Some(diagram.add_node(package_node(graph, package, &duplicates)));
    }
    for &package in &order {
        let root = graph.roots().contains(&package);
        for dep in graph.deps(package) {
            let kinds: Vec<DepKind> = dep
                .kinds
                .iter()
                .copied()
                .filter(|k| kinds.contains(k) && (root || *k != DepKind::Dev))
                .collect();
            if let (Some(from), Some(to), false) =
                (index[package], index[dep.package], kinds.is_empty())
            {
                add_edge(&mut diagram, from, to, &kinds);
            }
        }
    }
    diagram
}

/// The crate `--why` asked about, everything that depends on it up to the
/// roots, and those edges.
fn why_diagram(why: &WhyTree) -> Graph {
    let graph = why.graph();
    let duplicates: Vec<String> = graph.duplicates().into_keys().collect();
    let dependents = graph.dependents();
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
            if let (Some(from), Some(to)) = (index[package], index[dep.package]) {
                add_edge(&mut diagram, from, to, &dep.kinds);
            }
        }
    }
    diagram
}

/// A schema file (or URL, or `-`) read and parsed.
fn schema_file(cli: &Cli, resource: &str) -> Result<serde_json::Value, Failure> {
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
    rich_ext::schema::parse(&content).map_err(|err| (ExitClass::Data, format!("{resource}: {err}")))
}

/// `rich schema FILE`: the tree; `rich schema OLD NEW`: what changed.
pub(crate) fn schema(cli: &Cli) -> Result<Box<dyn Renderable>, Failure> {
    let resources: Vec<String> = if cli.resources.is_empty() {
        vec!["-".into()]
    } else {
        cli.resources.clone()
    };
    match resources.as_slice() {
        [one] => Ok(Box::new(SchemaTree::new(schema_file(cli, one)?))),
        [old, new] => {
            if old == "-" && new == "-" {
                return Err((
                    ExitClass::Usage,
                    "only one schema can come from stdin".into(),
                ));
            }
            let diff = SchemaDiff::new(&schema_file(cli, old)?, &schema_file(cli, new)?)
                .names(shown_name(old), shown_name(new));
            Ok(Box::new(diff))
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
