//! Markup conformance (#569, #570, #586): adjacent codes, escapes, unknown
//! names, tags, and the post-parse transform path.

mod common;

use std::sync::Arc;

use rich::style::StyleType;
use rich::{Console, Text};
use rich_micro::{
    expand, markup_text, render_markup, DiagnosticKind, FallbackPreference, Layer, Limits,
    MicroAsset, MicroError, MicroExt, MicroMeta, MicroRegistry, MicroTransform, PAD_CELL,
};

/// The fixtures as the built-in layer: `status/check` (alias `check`, ✅ or
/// OK), `fun/spark` (✨), `dot` (1x1, `*`).
fn registry() -> MicroRegistry {
    let mut registry = MicroRegistry::new();
    let mut report = Default::default();
    registry.load_dir(
        Layer::BuiltIn,
        &common::fixtures().join("builtin"),
        &Limits::default(),
        &mut report,
    );
    assert_eq!(report, Default::default());
    registry
}

fn console() -> Console {
    Console::builder().width(60).build()
}

fn parse(markup: &str) -> (Text, Vec<rich_micro::Diagnostic>) {
    markup_text(markup, &registry(), true, FallbackPreference::Emoji).unwrap()
}

/// Every placeholder in `text`: (plain slice, meta).
fn placements(text: &Text) -> Vec<(String, MicroMeta)> {
    let mut out: Vec<(String, MicroMeta)> = Vec::new();
    for span in text.spans() {
        if let StyleType::Style(style) = &span.style {
            if let Some(meta) = MicroMeta::from_style(style) {
                out.push((text.plain()[span.start..span.end].to_string(), meta));
            }
        }
    }
    out
}

#[test]
fn core_emoji_leaves_micro_tokens_alone() {
    assert_eq!(
        rich::emoji::replace(":micro:rocket: :fire:"),
        ":micro:rocket: 🔥"
    );
}

#[test]
fn tokens_become_tagged_placeholders() {
    let (text, diagnostics) = parse("Deploying :micro:status/check: done");
    assert!(diagnostics.is_empty());
    assert_eq!(text.plain(), "Deploying ✅ done");
    let found = placements(&text);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "✅");
    assert_eq!(
        (found[0].1.name.as_str(), found[0].1.cols, found[0].1.rows),
        ("status/check", 2, 1)
    );
    // Through an alias.
    assert_eq!(parse(":micro:check:").0.plain(), "✅");
}

#[test]
fn adjacent_codes() {
    assert_eq!(parse(":micro:check::fire:").0.plain(), "✅🔥");
    assert_eq!(parse(":fire::micro:check:").0.plain(), "🔥✅");
    assert_eq!(parse(":fire::micro:check::fire:").0.plain(), "🔥✅🔥");
    // Core's emoji scan would otherwise pair `:b:` and eat the token's colon.
    assert_eq!(parse("a:b:micro:check:").0.plain(), "a:b✅");
    let (text, _) = parse(":micro:check::micro:check::micro:dot:");
    assert_eq!(text.plain(), "✅✅*");
    let found = placements(&text);
    assert_eq!(found.len(), 3);
    // Each occurrence is its own placement, even of the same asset.
    assert_ne!(found[0].1.id, found[1].1.id);
    // A micro name that is also an emoji name stays a micro asset.
    let mut registry = registry();
    registry
        .add(
            Layer::Inline,
            MicroAsset::new("fire", "flame")
                .unwrap()
                .with_text("FI")
                .unwrap(),
        )
        .unwrap();
    let (text, _) = markup_text(
        ":micro:fire::fire:",
        &registry,
        true,
        FallbackPreference::Emoji,
    )
    .unwrap();
    assert_eq!(text.plain(), "FI🔥");
}

#[test]
fn escapes() {
    let (text, diagnostics) = parse(r"\:micro:check: is literal, :micro:check: is not");
    assert_eq!(text.plain(), ":micro:check: is literal, ✅ is not");
    assert!(diagnostics.is_empty());
    assert_eq!(placements(&text).len(), 1);
    // Core's own `\[` escape still works beside it.
    assert_eq!(parse(r"\[b] :micro:dot:").0.plain(), "[b] *");
}

#[test]
fn unknown_and_malformed_names_stay_literal() {
    let (text, diagnostics) = parse(":micro:nope: and :micro:Bad: and :micro:");
    assert_eq!(text.plain(), ":micro:nope: and :micro:Bad: and :micro:");
    let kinds: Vec<&DiagnosticKind> = diagnostics.iter().map(|d| &d.kind).collect();
    assert_eq!(
        kinds,
        [
            &DiagnosticKind::UnknownAsset("nope".into()),
            &DiagnosticKind::Malformed,
            &DiagnosticKind::Malformed
        ]
    );
    assert_eq!(diagnostics[0].offset, 0);
    assert_eq!(diagnostics[0].token, ":micro:nope:");
    assert!(diagnostics[0]
        .to_string()
        .contains("unknown micro asset \"nope\""));
    assert!(placements(&text).is_empty());
}

#[test]
fn markup_tags_style_placeholders_and_are_not_scanned() {
    let (text, _) =
        parse("[bold]:micro:check:[/bold] [link=https://x.test/:micro:check:]go[/link]");
    assert_eq!(text.plain(), "✅ go");
    let console = console();
    let segments = text.render(console.theme(), &rich::Style::new());
    let asset = segments.iter().find(|s| s.text == "✅").unwrap();
    let style = asset.style.as_ref().unwrap();
    assert_eq!(style.attr(0), Some(true));
    assert!(MicroMeta::from_style(style).is_some());
}

#[test]
fn render_markup_uses_the_console() {
    let console = console();
    let (text, diagnostics) = render_markup(
        &console,
        "[red]:micro:fun/spark:[/] :thumbs_up:",
        &registry(),
        FallbackPreference::Emoji,
    );
    assert!(diagnostics.is_empty());
    assert_eq!(text.plain(), "✨ 👍");
    // Printed plainly, the fallback is all there is: no escapes.
    let plain = Console::builder().width(40).force_terminal(false).build();
    assert_eq!(plain.render_to_string(&text).trim_end(), "✨ 👍");
}

#[test]
fn reserved_characters_leave_tokens_as_written() {
    let (text, diagnostics) = parse("\u{100000} :micro:check:");
    assert_eq!(text.plain(), "\u{100000} :micro:check:");
    assert_eq!(diagnostics[0].kind, DiagnosticKind::Reserved);
}

#[test]
fn fallback_preferences_and_padding() {
    let registry = registry();
    let text_only = markup_text(":micro:check:", &registry, true, FallbackPreference::Text)
        .unwrap()
        .0;
    assert_eq!(text_only.plain(), "OK");
    let alt = markup_text(":micro:check:", &registry, true, FallbackPreference::Alt)
        .unwrap()
        .0;
    assert_eq!(alt.plain(), "gr");
    // Narrow fallbacks are padded with blanks that are not whitespace.
    let mut registry = registry;
    let wide = MicroAsset::new("wide", "wide thing")
        .unwrap()
        .with_size("4x1".parse().unwrap())
        .unwrap()
        .with_text("ab")
        .unwrap();
    registry.add(Layer::Inline, wide).unwrap();
    let text = markup_text(":micro:wide:", &registry, true, FallbackPreference::Emoji)
        .unwrap()
        .0;
    assert_eq!(text.plain(), format!("ab{PAD_CELL}{PAD_CELL}"));
    assert_eq!(text.cell_len(), 4);
    // No fallback at all: the alt text, cut to fit, spaces made blank.
    let bare = MicroAsset::new("bare", "a b c")
        .unwrap()
        .with_size("3x1".parse().unwrap())
        .unwrap();
    registry.add(Layer::Inline, bare).unwrap();
    let text = markup_text(":micro:bare:", &registry, true, FallbackPreference::Emoji)
        .unwrap()
        .0;
    assert_eq!(text.plain(), format!("a{PAD_CELL}b"));
}

#[test]
fn post_parse_expand_keeps_spans() {
    let registry = registry();
    let mut text = Text::new("ok :micro:check: ok");
    text.stylize("red", 0, 19);
    text.stylize("blue", 3, 16);
    let (expanded, diagnostics) = expand(&text, &registry, FallbackPreference::Emoji);
    assert!(diagnostics.is_empty());
    assert_eq!(expanded.plain(), "ok ✅ ok");
    let ranges: Vec<(usize, usize)> = expanded.spans().iter().map(|s| (s.start, s.end)).collect();
    // red over everything, blue over the placeholder, then the tag.
    assert_eq!(ranges, [(0, 9), (3, 6), (3, 6)]);

    // On this path core's emoji pass has run first: a code written right
    // after a token is not an emoji (the markup path handles it).
    let parsed = console().render_str(":micro:check::fire:", None);
    let (expanded, _) = expand(&parsed, &registry, FallbackPreference::Emoji);
    assert_eq!(expanded.plain(), "✅:fire:");
}

#[test]
fn transform_and_extension_trait() {
    let registry = Arc::new(registry());
    let pipeline = rich_ext::transform::Pipeline::new()
        .then("micro", MicroTransform::new(Arc::clone(&registry)));
    let text = pipeline.apply(Text::new("build :micro:check:")).unwrap();
    assert_eq!(text.plain(), "build ✅");

    let mut text = Text::new("a ");
    text.append_micro(&registry, "dot")
        .unwrap()
        .append(" b", None);
    assert_eq!(text.plain(), "a * b");
    assert!(matches!(
        text.append_micro(&registry, "missing"),
        Err(MicroError::UnknownAsset(_))
    ));
    let mut text = Text::new("x :micro:fun/spark: :micro:nope:");
    let diagnostics = text.expand_micro(&registry);
    assert_eq!(text.plain(), "x ✨ :micro:nope:");
    assert_eq!(diagnostics.len(), 1);
}

#[test]
fn plugin_registers_the_transform() {
    let registry = Arc::new(registry());
    let mut extensions = rich_ext::ExtensionRegistry::new();
    extensions
        .add_plugin(&rich_micro::MicroPlugin::new(Arc::clone(&registry)))
        .unwrap();
    let transform = extensions.transform("micro").unwrap();
    assert_eq!(
        transform
            .transform(Text::new(":micro:dot:"))
            .unwrap()
            .plain(),
        "*"
    );
}

/// Every tagged run in rendered segments: (text, meta).
fn segment_placements(segments: &[rich::Segment]) -> Vec<(String, MicroMeta)> {
    segments
        .iter()
        .filter_map(|segment| {
            MicroMeta::from_segment(segment).map(|meta| (segment.text.clone(), meta))
        })
        .collect()
}

#[test]
fn markdown_tokens_become_tagged_placeholders() {
    // 0.0.15 workstream 6: `:micro:` in Markdown, not only in --print.
    use rich::markdown::Markdown;
    use rich_micro::PreparedMarkdown;
    let registry = registry();
    let source = "# Status :micro:check:\n\nDeploy **:micro:status/check:** done, \
                  `:micro:check:` \\:micro:check: :micro:nope:\n\n```\n:micro:check:\n```\n";
    let prepared = PreparedMarkdown::new(source, &registry, FallbackPreference::Emoji);
    assert!(prepared.has_assets());
    assert_eq!(
        prepared
            .diagnostics()
            .iter()
            .map(|d| &d.kind)
            .collect::<Vec<_>>(),
        [&DiagnosticKind::UnknownAsset("nope".into())]
    );
    let console = Console::builder()
        .width(60)
        .no_color(false)
        .force_terminal(true)
        .build();
    let view = prepared.view(Markdown::new(prepared.source()));
    let segments = console.render(&view, None);
    let found = segment_placements(&segments);
    assert_eq!(found.len(), 2, "{found:?}");
    for (cells, meta) in &found {
        assert_eq!(cells, "✅");
        assert_eq!((meta.name.as_str(), meta.cols), ("status/check", 2));
    }
    // The bold around the token covers its placeholder.
    assert!(
        segments.iter().any(|s| MicroMeta::from_segment(s).is_some()
            && s.style.as_ref().and_then(|style| style.attr(0)) == Some(true)),
        "{segments:?}"
    );
    let shown: String = segments.iter().map(|s| s.text.as_str()).collect();
    assert!(!shown.contains(|c: char| ('\u{100000}'..='\u{10fffd}').contains(&c)));
    // Code, escapes and unknown names stay as written.
    assert_eq!(shown.matches(":micro:check:").count(), 3, "{shown}");
    assert!(shown.contains(":micro:nope:"));
    // The stand-ins measure as the asset: every line is as wide as it is
    // with the emoji written in.
    let widths = |segments: Vec<rich::Segment>| -> Vec<usize> {
        let mut lines = vec![0];
        for segment in segments {
            if segment.text == "\n" {
                lines.push(0);
            } else {
                *lines.last_mut().unwrap() += segment.cell_length();
            }
        }
        lines
    };
    let written = Markdown::new(&source.replacen(":micro:check:", "✅", 1).replacen(
        ":micro:status/check:",
        "✅",
        1,
    ));
    assert_eq!(widths(segments), widths(console.render(&written, None)));
}

/// Each token left as written in a prepared Markdown source.
fn kept_tokens(source: &str) -> Vec<String> {
    let prepared =
        rich_micro::PreparedMarkdown::new(source, &registry(), FallbackPreference::Emoji);
    let out = prepared.source();
    out.match_indices(":micro:check:")
        .map(|(at, _)| out[..at].lines().last().unwrap_or("").trim().to_string())
        .collect()
}

#[test]
fn markdown_code_is_whatever_the_parser_calls_code() {
    // Indented blocks, fences in quotes and in list items, and code spans
    // across a line break are code too: their tokens stay as written.
    let source = "Para :micro:check:\n\n    indented :micro:check:\n\n> ```\n> quoted \
                  :micro:check:\n> ```\n\n- item\n\n    ```\n    listfence :micro:check:\n    \
                  ```\n\nspan ``a\n:micro:check: b``\n";
    let prepared =
        rich_micro::PreparedMarkdown::new(source, &registry(), FallbackPreference::Emoji);
    assert!(prepared.has_assets());
    assert_eq!(
        kept_tokens(source),
        ["indented", "> quoted", "listfence", "span ``a"],
        "{:?}",
        prepared.source()
    );
    assert!(!prepared.source().starts_with("Para :micro:"));
}

#[test]
fn markdown_link_destinations_keep_their_tokens() {
    // A token in a destination is part of the URL; the link text expands.
    let source = "[go :micro:check:](https://ex.com/:micro:check:/x) \
                  ![alt](a:micro:check:.png) <https://ex.com/:micro:check:> \
                  [ref :micro:check:][r]\n\n[r]: https://ex.com/:micro:check:/y\n";
    let prepared =
        rich_micro::PreparedMarkdown::new(source, &registry(), FallbackPreference::Emoji);
    let out = prepared.source();
    assert!(out.contains("(https://ex.com/:micro:check:/x)"), "{out:?}");
    assert!(out.contains("(a:micro:check:.png)"), "{out:?}");
    assert!(out.contains("<https://ex.com/:micro:check:>"), "{out:?}");
    assert!(
        out.contains("[r]: https://ex.com/:micro:check:/y"),
        "{out:?}"
    );
    assert!(!out.contains("[go :micro:"), "{out:?}");
    assert!(!out.contains("[ref :micro:"), "{out:?}");
    assert_eq!(out.matches(":micro:check:").count(), 4, "{out:?}");
    // Rendered, the link still points at the URL as written.
    let console = Console::builder().width(200).build();
    let shown = console.render_export(&prepared.view(rich::markdown::Markdown::new(out)));
    assert!(!shown.contains('\u{100000}'), "{shown:?}");
}
