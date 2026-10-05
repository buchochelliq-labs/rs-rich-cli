//! DOT sources from `tests/fixtures/dot`, drawn natively. Snapshots live in
//! `tests/snapshots`; set `UPDATE_SNAPSHOTS=1` to rewrite them after a
//! deliberate change, then review the diff.

use rich::cells::cell_len;
use rich::Console;
use rich_diagram::dot::{self, Dot};
use rich_diagram::{Diagram, Direction, Graph, Shape, Stroke};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/dot/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn render(source: &str, width: usize, ascii: bool) -> String {
    let console = Console::builder().width(width).color_system(None).build();
    console.render_export(&Dot::new(source).ascii(ascii))
}

fn check(name: &str, source: &str, width: usize, ascii: bool) {
    let actual = render(source, width, ascii);
    for line in actual.lines() {
        assert!(
            cell_len(line) <= width,
            "{name}: a line is wider than {width}: {line:?}"
        );
    }
    let path = format!("{}/tests/snapshots/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {path}; run with UPDATE_SNAPSHOTS=1"));
    assert_eq!(actual, expected, "{name} changed:\n{actual}");
}

#[test]
fn fixtures_render() {
    for name in ["services", "pipeline", "undirected"] {
        let source = fixture(&format!("{name}.dot"));
        check(&format!("dot_{name}"), &source, 80, false);
        check(&format!("dot_{name}_ascii"), &source, 80, true);
    }
}

#[test]
fn the_services_fixture_parses_as_written() {
    let parsed = dot::parse(&fixture("services.dot")).unwrap();
    assert!(parsed.directed && !parsed.strict);
    assert_eq!(parsed.name.as_deref(), Some("services"));
    assert_eq!(parsed.label.as_deref(), Some("Request path"));
    assert_eq!(parsed.graph.direction(), Direction::LeftRight);
    let shapes: Vec<(&str, Shape)> = parsed
        .graph
        .nodes()
        .iter()
        .map(|node| (node.label.as_str(), node.shape))
        .collect();
    assert_eq!(
        shapes,
        [
            ("Browser", Shape::Round),
            ("API", Shape::Rect),
            ("Postgres", Shape::Cylinder),
            ("Cache?", Shape::Rhombus),
        ]
    );
    let strokes: Vec<Stroke> = parsed.graph.edges().iter().map(|e| e.stroke).collect();
    assert_eq!(
        strokes,
        [Stroke::Solid, Stroke::Solid, Stroke::Dotted, Stroke::Thick]
    );
    assert_eq!(parsed.clusters.len(), 1);
    assert_eq!(parsed.clusters[0].label.as_deref(), Some("Backend"));
}

#[test]
fn strict_merges_repeated_edges() {
    let parsed = dot::parse(&fixture("pipeline.dot")).unwrap();
    assert!(parsed.strict);
    assert_eq!(parsed.name.as_deref(), Some("ci pipeline"));
    // checkout→build, build→test, test→deploy, test→lint, test→docs.
    assert_eq!(parsed.graph.edges().len(), 5);
    assert_eq!(parsed.graph.nodes()[3].label, "Deploy\nto prod");
}

/// A DOT graph draws exactly as the same graph built in code.
#[test]
fn dot_and_the_builder_draw_the_same() {
    let console = Console::builder().width(60).color_system(None).build();
    let from_dot = console.render_to_string(&Dot::new(
        "digraph { rankdir=LR; node [shape=box]; a -> b [label=go]; b -> c [style=dotted] }",
    ));
    let built = Graph::new(Direction::LeftRight)
        .edge("a", "b")
        .label("go")
        .edge("b", "c")
        .stroke(Stroke::Dotted);
    assert_eq!(from_dot, console.render_to_string(&Diagram::new(built)));
}

/// Unsupported input fails with the construct and its line, and nothing is
/// drawn: the renderable shows the reason and the source.
#[test]
fn unsupported_constructs_are_refused_not_drawn() {
    for (file, expected) in [
        (
            "port.dot",
            "line 3: a node port (`a:…`) is not supported (connect the node itself)",
        ),
        (
            "html_label.dot",
            "line 2: an HTML-like label is not supported (use a quoted string)",
        ),
        (
            "record.dot",
            "line 2: the `record` shape is not supported (use a box with a multi-line label)",
        ),
    ] {
        let source = fixture(file);
        let error = dot::parse(&source).unwrap_err();
        assert_eq!(error.to_string(), expected, "{file}");
        let out = render(&source, 120, false);
        assert!(
            out.starts_with(&format!("DOT: {expected}\n")),
            "{file}: {out}"
        );
        assert!(
            !out.contains('┌') && !out.contains('│'),
            "{file} drew: {out}"
        );
    }
}

#[test]
fn narrow_widths_crop_and_say_so() {
    let source = fixture("services.dot");
    for width in [1, 10, 30] {
        let out = render(&source, width, false);
        for line in out.lines() {
            assert!(cell_len(line) <= width, "{width}: {line:?}");
        }
    }
    assert!(render(&source, 30, false).contains("cropped to 30 of"));
}

#[test]
fn it_measures_to_its_drawing_and_fits_a_panel() {
    use rich::measure::Measurement;
    use rich::panel::Panel;
    use rich::Renderable;
    let console = Console::builder().width(80).color_system(None).build();
    let dot = Dot::new("digraph { rankdir=LR; a -> b }");
    let width = rich_diagram::draw(&dot.parsed().unwrap().graph, false)
        .unwrap()
        .width;
    assert_eq!(
        dot.measure(&console, &console.options()),
        Measurement::new(width, width)
    );
    let out = console.render_to_string(&Panel::fit(Box::new(dot)));
    assert!(out.lines().all(|line| cell_len(line) == width + 4), "{out}");
}

#[test]
fn an_empty_graph_says_so() {
    let out = render("digraph {}", 40, false);
    assert!(out.starts_with("DOT: the graph has no nodes"), "{out}");
}

#[test]
fn labels_cannot_carry_escape_sequences() {
    let out = render("digraph { a [label=\"x\u{1b}[31my\"] }", 40, false);
    assert!(!out.contains('\u{1b}'), "{out:?}");
}

#[cfg(feature = "plugin")]
mod plugin {
    use rich::markdown::Markdown;
    use rich::Console;
    use rich_diagram::plugin::DotPlugin;
    use rich_ext::plugin::Capability;
    use rich_ext::ExtensionRegistry;

    #[test]
    fn registers_fence_renderers_and_a_source_renderer() {
        let mut registry = ExtensionRegistry::with_defaults();
        registry.add_plugin(&DotPlugin::default()).unwrap();
        let plugin = registry
            .plugins()
            .iter()
            .find(|plugin| plugin.metadata.id == "dot")
            .unwrap();
        assert_eq!(
            plugin.capabilities,
            [
                Capability::FenceRenderer("dot".into()),
                Capability::FenceRenderer("graphviz".into()),
                Capability::Renderer("dot".into()),
            ]
        );
        let console = Console::builder().width(60).color_system(None).build();
        let markdown = Markdown::new(
            "# Graph\n\n```dot\ndigraph { rankdir=LR; a -> b }\n```\n\n```graphviz\ngraph { x -- y }\n```\n",
        )
        .fence_renderer(registry.fences().unwrap());
        let out = console.render_to_string(&markdown);
        assert!(
            out.contains("( a )─►( b )") || out.contains("│ a ├─►│ b │"),
            "{out}"
        );
        assert!(out.contains("│ x │") || out.contains("x"), "{out}");
        assert!(
            !out.contains("rankdir"),
            "the fence was shown as code: {out}"
        );

        let rendered = registry
            .renderer("dot")
            .unwrap()
            .render("digraph { p -> q }")
            .unwrap();
        let out = console.render_to_string(rendered.as_ref());
        assert!(out.contains('▼'), "{out}");
    }
}

#[cfg(feature = "graphviz")]
mod graphviz {
    use rich_diagram::graphviz::{render_svg, GraphvizError, GraphvizOptions};

    #[test]
    fn a_missing_program_is_reported() {
        let options = GraphvizOptions {
            program: "rich-diagram-no-such-dot".into(),
            ..GraphvizOptions::default()
        };
        assert_eq!(
            render_svg("digraph { a }", &options),
            Err(GraphvizError::NotFound("rich-diagram-no-such-dot".into()))
        );
    }

    #[test]
    fn oversized_sources_are_refused_before_running() {
        let options = GraphvizOptions {
            max_input: 4,
            ..GraphvizOptions::default()
        };
        assert!(matches!(
            render_svg("digraph { a }", &options),
            Err(GraphvizError::TooLarge { limit: 4, .. })
        ));
    }

    /// A stand-in `dot`: a shell script that checks its arguments and
    /// echoes a fixed SVG.
    #[cfg(unix)]
    #[test]
    fn a_stand_in_dot_gets_the_source_on_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile_dir();
        let program = dir.join("dot");
        std::fs::write(
            &program,
            "#!/bin/sh\n[ \"$1\" = -Tsvg ] || exit 3\nsource=$(cat)\nprintf '<svg>%s</svg>' \"$source\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let options = GraphvizOptions {
            program: program.clone(),
            ..GraphvizOptions::default()
        };
        assert_eq!(
            render_svg("digraph { a }", &options).unwrap(),
            "<svg>digraph { a }</svg>"
        );
        let failing = dir.join("dot-fails");
        std::fs::write(
            &failing,
            "#!/bin/sh\necho 'Error: syntax error in line 1' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
        let options = GraphvizOptions {
            program: failing,
            ..options
        };
        let error = render_svg("digraph {", &options).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Graphviz failed: Error: syntax error in line 1"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rich-diagram-graphviz-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

/// Like core's renderables, a drawing leaves the newline after its last
/// line to `print`: printing a diagram, with or without a label or notes, or
/// a refused source, is not followed by a blank line.
#[test]
fn printing_a_drawing_ends_with_one_newline() {
    let console = Console::builder().width(30).color_system(None).build();
    let graph = Graph::new(Direction::LeftRight)
        .node("a", "A")
        .node("b", "B")
        .edge("a", "b");
    let cases: Vec<(&str, Box<dyn rich::Renderable>)> = vec![
        ("diagram", Box::new(Diagram::new(graph))),
        ("dot", Box::new(Dot::new("digraph { a -> b }"))),
        (
            "label",
            Box::new(Dot::new("digraph { label=\"L\"; a -> b }")),
        ),
        (
            "cropped",
            Box::new(Dot::new(
                "digraph { aaaaaaaaaa -> bbbbbbbbbb -> cccccccccc }",
            )),
        ),
        ("refused", Box::new(Dot::new("digraph { a:p -> b }"))),
    ];
    for (name, renderable) in cases {
        let out = console.render_export(renderable.as_ref());
        assert!(
            out.ends_with('\n') && !out.ends_with("\n\n"),
            "{name}: {out:?}"
        );
    }
}
