//! Documentation formats: Markdown, reStructuredText, Org, AsciiDoc and man
//! pages, behind one [`DocumentFormat`] interface.
//!
//! Not in upstream `rich`, which renders Markdown only. Each format is a
//! *provider*: an id (`"org"`), the file names it claims, a content sniffer
//! that says how confident it is a text is in that format, and a renderer
//! from source text to a renderable, with [`Diagnostic`]s for what it could
//! not show as written. A [`DocumentRegistry`] holds providers, finds one by
//! id, file name or content ([`DocumentRegistry::detect`]), and renders.
//! Nothing here depends on an output backend: a renderable prints to a
//! terminal, an HTML or SVG export, or any other console target alike.
//!
//! Markdown and reStructuredText wrap core's `Markdown` and
//! [`rst::RestructuredText`](crate::rst::RestructuredText). Org, AsciiDoc and
//! man pages parse into a small shared [model] (headings, paragraphs with
//! inline spans, lists, code, tables, quotes, admonitions, definition lists,
//! rules, image placeholders) that [`DocumentView`] draws with core's
//! renderables and Markdown's theme styles, so the five formats look alike.
//! The format-specific styles (TODO keywords, admonitions, drawers) are the
//! theme keys in [`STYLES`].
//!
//! ```
//! use rich::Console;
//! use rich_ext::document::{DocumentRegistry, RenderOptions};
//!
//! let registry = DocumentRegistry::with_builtins();
//! let detected = registry.detect(Some("notes.org"), "* Heading", 0.9).unwrap();
//! assert_eq!(detected.format, "org");
//!
//! let rendered = registry
//!     .render("= Title\n\nNOTE: Mind the gap.", "asciidoc", &RenderOptions::new())
//!     .unwrap();
//! let console = Console::builder().width(30).color_system(None).build();
//! let out = console.render_to_string(rendered.renderable.as_ref());
//! assert!(out.contains("Note") && out.contains("Mind the gap."));
//! ```
//!
//! # Third-party formats
//!
//! Register any [`DocumentFormat`] with [`DocumentRegistry::register`]. A
//! plugin cannot declare a document format through `PluginRegistrar` (that
//! would need a new registrar method in `rich-plugin-api`); it can register a
//! `SourceRenderer` with `renderer(name, …)`, and a host wraps that in a
//! [`SourceRendererFormat`] (adding the extensions it claims) to put it in a
//! registry. See `docs/PLUGINS.md`.

pub mod asciidoc;
pub mod man;
pub mod model;
pub mod org;
mod render;
mod scan;

use std::fmt;
use std::sync::Arc;

use rich::{Renderable, Text};

pub use model::{Block, Document, Inline};
pub use render::{DocumentView, RenderOptions};

/// Theme keys this module adds, chained into
/// [`extended_theme`](crate::theme::extended_theme). The renderer falls back
/// to these values when a console's theme lacks them, and reads the
/// `markdown.*` keys for everything Markdown also draws.
pub const STYLES: &[(&str, &str)] = &[
    ("document.todo", "bold red"),
    ("document.done", "bold green"),
    ("document.priority", "bold yellow"),
    ("document.tag", "dim"),
    ("document.byline", "dim"),
    ("document.caption", "dim italic"),
    ("document.properties", "dim"),
    ("document.properties.key", "bold"),
    ("document.term", "bold"),
    ("document.underline", "underline"),
    ("document.callout", "bold reverse"),
    ("document.checkbox", "bold"),
    ("document.checkbox.checked", "bold green"),
    ("document.checkbox.partial", "bold yellow"),
    ("document.attribution", "italic"),
    ("document.timestamp", "cyan"),
    ("document.footnote", "dim"),
    ("document.mark", "black on yellow"),
    ("document.banner", "dim"),
    ("document.aside", "dim"),
    ("document.admonition", "bold"),
    ("document.admonition.note", "bold cyan"),
    ("document.admonition.tip", "bold green"),
    ("document.admonition.important", "bold magenta"),
    ("document.admonition.warning", "bold yellow"),
    ("document.admonition.caution", "bold red"),
];

/// A content-detection confidence at or above which a format is a likely
/// match: what `rich --format auto` routes on.
pub const LIKELY: f32 = 0.6;

/// A confidence at or above which a format is all but certain: what routes
/// an extensionless file with no flag.
pub const CERTAIN: f32 = 0.9;

/// How serious a [`Diagnostic`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something not drawn that a reader would not miss: a formatting
    /// request, an export-only block.
    Info,
    /// Content not shown as written: an `include::` not followed, an
    /// equation left out, a block that never closed.
    Warning,
}

/// Something a parser could not show as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The 1-based source line, when known.
    pub line: Option<usize>,
    pub severity: Severity,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// A parsed document and what the parser could not show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
}

/// A rendered document: the renderable to print and the diagnostics.
pub struct Rendered {
    pub renderable: Box<dyn Renderable + Send + Sync>,
    pub diagnostics: Vec<Diagnostic>,
}

impl fmt::Debug for Rendered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rendered")
            .field("diagnostics", &self.diagnostics)
            .finish_non_exhaustive()
    }
}

/// How a file name relates to a format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameMatch {
    /// Not this format's name.
    No,
    /// Possibly this format, if the content agrees: `ls.1` may be a man
    /// page or a rotated log, `key.asc` AsciiDoc or a PGP signature.
    Maybe,
    /// This format's own extension (`.org`, `.adoc`).
    Yes,
}

/// A documentation format: how to recognise it and how to draw it.
pub trait DocumentFormat: Send + Sync {
    /// The format's id: lowercase, stable, usable as a CLI value (`"org"`).
    fn id(&self) -> &str;

    /// The name people call it (`"Org"`).
    fn name(&self) -> &str;

    /// Other ids accepted for it (`"adoc"` for AsciiDoc).
    fn aliases(&self) -> Vec<String> {
        Vec::new()
    }

    /// The file extensions it claims, lowercase, without the dot.
    fn extensions(&self) -> Vec<String> {
        Vec::new()
    }

    /// How `file_name` (a base name) relates to this format. By default a
    /// name whose extension is one of [`extensions`](Self::extensions) is
    /// [`NameMatch::Yes`].
    fn match_name(&self, file_name: &str) -> NameMatch {
        match extension(file_name) {
            Some(ext)
                if self
                    .extensions()
                    .iter()
                    .any(|own| own.eq_ignore_ascii_case(ext)) =>
            {
                NameMatch::Yes
            }
            _ => NameMatch::No,
        }
    }

    /// How confident it is that `text` is in this format, from 0 to 1. See
    /// [`LIKELY`] and [`CERTAIN`].
    fn sniff(&self, _text: &str) -> f32 {
        0.0
    }

    /// What the format supports (`"tables"`, `"admonitions"`, …), for
    /// listings and for callers that want to know before rendering.
    fn features(&self) -> Vec<String> {
        Vec::new()
    }

    /// Parse `source` into the shared [model], for formats built on it.
    fn parse(&self, _source: &str) -> Option<Parsed> {
        None
    }

    /// Render `source`. The default draws [`parse`](Self::parse)'s document
    /// with [`DocumentView`], or the source as plain text when the format
    /// does not parse into the model.
    fn render(&self, source: &str, options: &RenderOptions) -> Rendered {
        match self.parse(source) {
            Some(parsed) => {
                let diagnostics = parsed.diagnostics.clone();
                Rendered {
                    renderable: Box::new(
                        DocumentView::from_parsed(parsed).options(options.clone()),
                    ),
                    diagnostics,
                }
            }
            None => Rendered {
                renderable: Box::new(Text::new(source)),
                diagnostics: Vec::new(),
            },
        }
    }
}

/// The extension of a file name: after its last dot, unless that is the
/// first character (`.bashrc` has none).
pub fn extension(file_name: &str) -> Option<&str> {
    let (stem, ext) = file_name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then_some(ext)
}

/// The base name of a path or URL: its last segment, without a query or
/// fragment.
pub fn base_name(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.split(['?', '#']).next().unwrap_or(name)
}

/// Why a format was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectedBy {
    /// The file name's extension.
    Name,
    /// An extension that may mean this format, confirmed by the content.
    NameAndContent,
    /// The content alone.
    Content,
}

/// A detected format.
#[derive(Clone, Debug, PartialEq)]
pub struct Detection {
    /// The format's id.
    pub format: String,
    /// From 0 to 1; 1 for a format's own extension.
    pub confidence: f32,
    pub by: DetectedBy,
}

/// Why a format could not be registered or used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    /// No registered format has this id or alias.
    Unknown { name: String, known: Vec<String> },
    /// The id or alias is taken.
    Conflict { name: String },
    /// The id or alias is not a lowercase name (see `rich_plugin_api::is_valid_name`).
    InvalidName { name: String },
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::Unknown { name, known } => write!(
                f,
                "unknown document format {name:?}; known: {}",
                known.join(", ")
            ),
            DocumentError::Conflict { name } => {
                write!(f, "a document format is already registered as {name:?}")
            }
            DocumentError::InvalidName { name } => {
                write!(f, "{name:?} is not a valid document format name")
            }
        }
    }
}

impl std::error::Error for DocumentError {}

/// Document formats by id, alias, file name and content.
#[derive(Clone, Default)]
pub struct DocumentRegistry {
    formats: Vec<Arc<dyn DocumentFormat>>,
}

impl fmt::Debug for DocumentRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.formats.iter().map(|format| format.id().to_string()))
            .finish()
    }
}

impl DocumentRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        DocumentRegistry::default()
    }

    /// The built-in formats, in detection order: Markdown (with the
    /// `markdown` feature), reStructuredText, Org, AsciiDoc and man pages.
    pub fn with_builtins() -> Self {
        let mut registry = DocumentRegistry::new();
        for format in builtins() {
            registry
                .register(format)
                .expect("the built-in formats have distinct names");
        }
        registry
    }

    /// Add `format`. Its id and aliases must be valid names that no
    /// registered format uses.
    pub fn register(&mut self, format: Arc<dyn DocumentFormat>) -> Result<(), DocumentError> {
        let mut names = vec![format.id().to_string()];
        names.extend(format.aliases());
        for name in &names {
            if !rich_plugin_api::is_valid_name(name) || name.chars().any(|c| c.is_uppercase()) {
                return Err(DocumentError::InvalidName { name: name.clone() });
            }
            if self.get(name).is_some() {
                return Err(DocumentError::Conflict { name: name.clone() });
            }
        }
        self.formats.push(format);
        Ok(())
    }

    /// The registered formats, in registration order.
    pub fn formats(&self) -> &[Arc<dyn DocumentFormat>] {
        &self.formats
    }

    /// The format with this id or alias (case-insensitive).
    pub fn get(&self, name: &str) -> Option<Arc<dyn DocumentFormat>> {
        let name = name.to_ascii_lowercase();
        self.formats
            .iter()
            .find(|format| format.id() == name || format.aliases().contains(&name))
            .cloned()
    }

    /// Every id, in registration order.
    pub fn ids(&self) -> Vec<String> {
        self.formats
            .iter()
            .map(|format| format.id().to_string())
            .collect()
    }

    /// Each format's confidence that `text` is in it, most confident first
    /// (ties keep registration order).
    pub fn sniff(&self, text: &str) -> Vec<(String, f32)> {
        let mut scores: Vec<(String, f32)> = self
            .formats
            .iter()
            .map(|format| (format.id().to_string(), format.sniff(text).clamp(0.0, 1.0)))
            .collect();
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        scores
    }

    /// The format of a document, from its path (a file path or URL; `None`
    /// for standard input) and its text:
    ///
    /// 1. A format's own extension wins outright (`notes.org`).
    /// 2. An extension a format may use (`ls.1`, `key.asc`) needs the
    ///    content to agree at [`LIKELY`] or above.
    /// 3. Any other extension is never a document here: `main.c` stays
    ///    source code whatever it contains.
    /// 4. Without an extension, the content alone decides, when the most
    ///    confident format reaches `threshold` and leads the next by 0.1.
    pub fn detect(&self, path: Option<&str>, text: &str, threshold: f32) -> Option<Detection> {
        if let Some(path) = path {
            let name = base_name(path);
            let mut maybe: Vec<&Arc<dyn DocumentFormat>> = Vec::new();
            for format in &self.formats {
                match format.match_name(name) {
                    NameMatch::Yes => {
                        return Some(Detection {
                            format: format.id().to_string(),
                            confidence: 1.0,
                            by: DetectedBy::Name,
                        })
                    }
                    NameMatch::Maybe => maybe.push(format),
                    NameMatch::No => {}
                }
            }
            if !maybe.is_empty() {
                return maybe
                    .into_iter()
                    .map(|format| (format, format.sniff(text)))
                    .filter(|(_, confidence)| *confidence >= LIKELY)
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(format, confidence)| Detection {
                        format: format.id().to_string(),
                        confidence: confidence.min(1.0),
                        by: DetectedBy::NameAndContent,
                    });
            }
            if extension(name).is_some() {
                return None;
            }
        }
        let scores = self.sniff(text);
        let (best, confidence) = scores.first()?.clone();
        let runner_up = scores.get(1).map_or(0.0, |(_, score)| *score);
        (confidence >= threshold && confidence - runner_up >= 0.1).then_some(Detection {
            format: best,
            confidence,
            by: DetectedBy::Content,
        })
    }

    /// Render `source` as the format named `format` (an id or alias).
    pub fn render(
        &self,
        source: &str,
        format: &str,
        options: &RenderOptions,
    ) -> Result<Rendered, DocumentError> {
        let provider = self.get(format).ok_or_else(|| DocumentError::Unknown {
            name: format.to_string(),
            known: self.ids(),
        })?;
        Ok(provider.render(source, options))
    }
}

/// The built-in providers, in detection order.
pub fn builtins() -> Vec<Arc<dyn DocumentFormat>> {
    let mut formats: Vec<Arc<dyn DocumentFormat>> = vec![
        Arc::new(RstFormat),
        Arc::new(OrgFormat),
        Arc::new(AsciiDocFormat),
        Arc::new(ManFormat),
    ];
    #[cfg(feature = "markdown")]
    formats.insert(0, Arc::new(MarkdownFormat));
    formats
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// Markdown, drawn by core's `Markdown` (a faithful port of upstream's).
#[cfg(feature = "markdown")]
#[derive(Clone, Copy, Debug, Default)]
pub struct MarkdownFormat;

#[cfg(feature = "markdown")]
impl DocumentFormat for MarkdownFormat {
    fn id(&self) -> &str {
        "markdown"
    }

    fn name(&self) -> &str {
        "Markdown"
    }

    fn aliases(&self) -> Vec<String> {
        strings(&["md"])
    }

    fn extensions(&self) -> Vec<String> {
        strings(&["md", "markdown"])
    }

    fn sniff(&self, text: &str) -> f32 {
        sniff_markdown(text)
    }

    fn features(&self) -> Vec<String> {
        strings(&[
            "headings", "emphasis", "lists", "code", "tables", "quotes", "links", "images", "rules",
        ])
    }

    fn render(&self, source: &str, options: &RenderOptions) -> Rendered {
        let mut markdown = rich::markdown::Markdown::new(source).hyperlinks(options.hyperlinks);
        if let Some(theme) = &options.code_theme {
            markdown = markdown.code_theme(theme.as_str());
        }
        if let Some(justify) = options.justify {
            markdown = markdown.justify(justify);
        }
        for fence in &options.fences {
            markdown = markdown.fence_renderer(fence.clone());
        }
        Rendered {
            renderable: Box::new(markdown),
            diagnostics: Vec::new(),
        }
    }
}

/// reStructuredText, drawn as `rich-rst` draws it ([`crate::rst`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct RstFormat;

impl DocumentFormat for RstFormat {
    fn id(&self) -> &str {
        "rst"
    }

    fn name(&self) -> &str {
        "reStructuredText"
    }

    fn aliases(&self) -> Vec<String> {
        strings(&["rest", "restructuredtext"])
    }

    fn extensions(&self) -> Vec<String> {
        strings(&["rst"])
    }

    fn sniff(&self, text: &str) -> f32 {
        sniff_rst(text)
    }

    fn features(&self) -> Vec<String> {
        strings(&[
            "headings",
            "emphasis",
            "lists",
            "code",
            "tables",
            "quotes",
            "links",
            "images",
            "admonitions",
            "definition-lists",
            "field-lists",
            "footnotes",
        ])
    }

    fn render(&self, source: &str, options: &RenderOptions) -> Rendered {
        let mut document = crate::rst::RestructuredText::new(source).default_lexer(
            options
                .default_language
                .clone()
                .unwrap_or_else(|| "python".into()),
        );
        if let Some(theme) = &options.code_theme {
            document = document.code_theme(theme.as_str());
        }
        Rendered {
            renderable: Box::new(document),
            diagnostics: Vec::new(),
        }
    }
}

/// Org mode ([`org`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct OrgFormat;

impl DocumentFormat for OrgFormat {
    fn id(&self) -> &str {
        "org"
    }

    fn name(&self) -> &str {
        "Org"
    }

    fn aliases(&self) -> Vec<String> {
        strings(&["orgmode", "org-mode"])
    }

    fn extensions(&self) -> Vec<String> {
        strings(&["org"])
    }

    fn sniff(&self, text: &str) -> f32 {
        org::sniff(text)
    }

    fn features(&self) -> Vec<String> {
        strings(&[
            "headings",
            "todo-keywords",
            "priorities",
            "tags",
            "emphasis",
            "lists",
            "checkboxes",
            "definition-lists",
            "code",
            "tables",
            "quotes",
            "links",
            "images",
            "drawers",
            "keywords",
            "footnotes",
            "timestamps",
            "admonitions",
            "rules",
        ])
    }

    fn parse(&self, source: &str) -> Option<Parsed> {
        Some(org::parse(source))
    }
}

/// AsciiDoc ([`asciidoc`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct AsciiDocFormat;

impl DocumentFormat for AsciiDocFormat {
    fn id(&self) -> &str {
        "asciidoc"
    }

    fn name(&self) -> &str {
        "AsciiDoc"
    }

    fn aliases(&self) -> Vec<String> {
        strings(&["adoc"])
    }

    fn extensions(&self) -> Vec<String> {
        strings(&["adoc", "asciidoc"])
    }

    /// `.adoc` and `.asciidoc` are AsciiDoc; `.asc` and `.ad` are when the
    /// content says so (`.asc` is also an armoured PGP file).
    fn match_name(&self, file_name: &str) -> NameMatch {
        match extension(file_name).map(str::to_ascii_lowercase).as_deref() {
            Some("adoc" | "asciidoc") => NameMatch::Yes,
            Some("asc" | "ad") => NameMatch::Maybe,
            _ => NameMatch::No,
        }
    }

    fn sniff(&self, text: &str) -> f32 {
        asciidoc::sniff(text)
    }

    fn features(&self) -> Vec<String> {
        strings(&[
            "headings",
            "attributes",
            "emphasis",
            "lists",
            "checklists",
            "definition-lists",
            "callouts",
            "code",
            "tables",
            "quotes",
            "admonitions",
            "sidebars",
            "links",
            "images",
            "footnotes",
            "rules",
        ])
    }

    fn parse(&self, source: &str) -> Option<Parsed> {
        Some(asciidoc::parse(source))
    }
}

/// Man pages in roff: the man and mdoc macros ([`man`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct ManFormat;

impl DocumentFormat for ManFormat {
    fn id(&self) -> &str {
        "man"
    }

    fn name(&self) -> &str {
        "man page"
    }

    fn aliases(&self) -> Vec<String> {
        strings(&["roff", "troff", "groff", "nroff", "mdoc"])
    }

    fn extensions(&self) -> Vec<String> {
        strings(&[
            "man", "roff", "mdoc", "1", "2", "3", "4", "5", "6", "7", "8", "9",
        ])
    }

    /// `.man`, `.roff` and `.mdoc` are man pages; a manual section (`.1` to
    /// `.9`, optionally with a suffix such as `.3pm` or `.1ssl`) is one when
    /// the content says so (`syslog.1` is a rotated log).
    fn match_name(&self, file_name: &str) -> NameMatch {
        let Some(ext) = extension(file_name).map(str::to_ascii_lowercase) else {
            return NameMatch::No;
        };
        if matches!(ext.as_str(), "man" | "roff" | "mdoc") {
            return NameMatch::Yes;
        }
        let mut chars = ext.chars();
        let section = chars.next().is_some_and(|c| ('1'..='9').contains(&c));
        let suffix: String = chars.collect();
        let suffix_ok = suffix.len() <= 8
            && suffix.chars().next().is_none_or(|c| c.is_ascii_lowercase())
            && suffix.chars().all(|c| c.is_ascii_alphanumeric());
        if section && suffix_ok {
            NameMatch::Maybe
        } else {
            NameMatch::No
        }
    }

    fn sniff(&self, text: &str) -> f32 {
        man::sniff(text)
    }

    fn features(&self) -> Vec<String> {
        strings(&[
            "headings",
            "paragraphs",
            "fonts",
            "tagged-paragraphs",
            "lists",
            "indentation",
            "examples",
            "links",
            "tables",
            "mdoc",
            "conditionals",
        ])
    }

    fn parse(&self, source: &str) -> Option<Parsed> {
        Some(man::parse(source))
    }
}

/// A plugin's `SourceRenderer` as a document format: the renderer draws the
/// source; the id, name, extensions and an optional sniffer come from the
/// host that wraps it.
pub struct SourceRendererFormat {
    id: String,
    name: String,
    extensions: Vec<String>,
    renderer: Arc<dyn rich_plugin_api::SourceRenderer>,
    sniffer: Option<Sniffer>,
}

/// A content sniffer: how confident it is a text is in the format.
type Sniffer = Box<dyn Fn(&str) -> f32 + Send + Sync>;

impl SourceRendererFormat {
    /// Wrap `renderer` as the format `id`.
    pub fn new(id: impl Into<String>, renderer: Arc<dyn rich_plugin_api::SourceRenderer>) -> Self {
        let id = id.into();
        SourceRendererFormat {
            name: id.clone(),
            id,
            extensions: Vec::new(),
            renderer,
            sniffer: None,
        }
    }

    /// The name people call the format.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The extensions it claims, without the dot.
    #[must_use]
    pub fn extensions(mut self, extensions: &[&str]) -> Self {
        self.extensions = extensions
            .iter()
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .collect();
        self
    }

    /// A content sniffer (0 to 1).
    #[must_use]
    pub fn sniffer(mut self, sniffer: impl Fn(&str) -> f32 + Send + Sync + 'static) -> Self {
        self.sniffer = Some(Box::new(sniffer));
        self
    }
}

impl DocumentFormat for SourceRendererFormat {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn extensions(&self) -> Vec<String> {
        self.extensions.clone()
    }

    fn sniff(&self, text: &str) -> f32 {
        self.sniffer.as_ref().map_or(0.0, |sniff| sniff(text))
    }

    fn render(&self, source: &str, _options: &RenderOptions) -> Rendered {
        match self.renderer.render(source) {
            Ok(renderable) => Rendered {
                renderable,
                diagnostics: Vec::new(),
            },
            Err(error) => Rendered {
                renderable: Box::new(Text::new(source)),
                diagnostics: vec![Diagnostic {
                    line: None,
                    severity: Severity::Warning,
                    message: format!("the {} renderer failed: {error}; shown as text", self.id),
                }],
            },
        }
    }
}

/// Markdown's tells. Capped below [`CERTAIN`]: Markdown-looking text is
/// everywhere (a `#` comment, a `*` bullet), so content alone only ever
/// makes it likely.
fn sniff_markdown(text: &str) -> f32 {
    let text = scan::sample(text);
    if text.starts_with("#!") {
        return 0.0;
    }
    let (mut fences, mut headings, mut links, mut emphasis, mut lists) = (0, 0, 0, 0, 0);
    for line in text.lines().take(4000) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fences += 1;
        }
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') && line.starts_with('#')
        {
            headings += 1;
        }
        if line.contains("](") && line.contains('[') {
            links += 1;
        }
        if line.contains("**") || line.contains("`") {
            emphasis += 1;
        }
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("1. ") {
            lists += 1;
        }
    }
    let mut score: f32 = 0.0;
    let mut signals = 0;
    for (present, weight) in [
        (fences >= 2, 0.7),
        (links > 0, 0.6),
        (headings >= 2, 0.6),
        (headings == 1, 0.45),
        (emphasis > 0, 0.35),
        (lists >= 2, 0.3),
    ] {
        if present {
            score = score.max(weight);
            signals += 1;
        }
    }
    if signals > 1 {
        score += 0.05 * (signals - 1) as f32;
    }
    score.min(0.8)
}

/// reStructuredText's tells: directives, title adornments, roles. Capped
/// below [`CERTAIN`], as Markdown's are.
fn sniff_rst(text: &str) -> f32 {
    let text = scan::sample(text);
    if text.starts_with("#!") {
        return 0.0;
    }
    let lines: Vec<&str> = text.lines().take(4000).collect();
    let (mut directives, mut adornments, mut roles, mut fields) = (0, 0, 0, 0);
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix(".. ") {
            if rest.contains("::") {
                directives += 1;
            }
        }
        let first = line.chars().next();
        let adornment = line.len() >= 3
            && first.is_some_and(|c| "=-~^\"'`#*+".contains(c))
            && line.chars().all(|c| Some(c) == first);
        if adornment && index > 0 {
            let title = lines[index - 1].trim_end();
            if !title.is_empty() && title.chars().count() <= line.chars().count() {
                adornments += 1;
            }
        }
        if line.contains(":ref:`") || line.contains(":doc:`") || line.contains("``") {
            roles += 1;
        }
        if trimmed.starts_with(':') && trimmed[1..].contains(": ") {
            fields += 1;
        }
    }
    let mut score: f32 = 0.0;
    let mut signals = 0;
    for (present, weight) in [
        (directives > 0, 0.8),
        (roles > 0, 0.5),
        (adornments >= 2, 0.5),
        (fields >= 2, 0.4),
    ] {
        if present {
            score = score.max(weight);
            signals += 1;
        }
    }
    if signals > 1 {
        score += 0.05 * (signals - 1) as f32;
    }
    score.min(0.85)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_found_by_id_alias_and_extension() {
        let registry = DocumentRegistry::with_builtins();
        assert_eq!(registry.get("adoc").unwrap().id(), "asciidoc");
        assert_eq!(registry.get("ROFF").unwrap().id(), "man");
        for (path, format) in [
            ("notes.org", "org"),
            ("guide.adoc", "asciidoc"),
            ("README.rst", "rst"),
            ("dir/ls.man", "man"),
            ("https://example.com/a.ORG?raw=1", "org"),
        ] {
            let detected = registry.detect(Some(path), "", CERTAIN).unwrap();
            assert_eq!(
                (detected.format.as_str(), detected.by),
                (format, DetectedBy::Name)
            );
        }
    }

    #[test]
    fn a_section_extension_needs_roff_inside() {
        let registry = DocumentRegistry::with_builtins();
        let page = ".TH LS 1\n.SH NAME\nls \\- list\n";
        let detected = registry.detect(Some("ls.1"), page, CERTAIN).unwrap();
        assert_eq!(
            (detected.format.as_str(), detected.by),
            ("man", DetectedBy::NameAndContent)
        );
        assert_eq!(
            registry.detect(Some("syslog.1"), "Jan 1 boot\n", CERTAIN),
            None
        );
        assert_eq!(registry.detect(Some("report.2024"), page, CERTAIN), None);
    }

    #[test]
    fn source_code_is_never_a_document() {
        let registry = DocumentRegistry::with_builtins();
        let looks_like_man = ".TH X 1\n.SH NAME\nx\n";
        assert_eq!(registry.detect(Some("main.c"), looks_like_man, 0.0), None);
        assert_eq!(registry.detect(Some("x.txt"), "#+TITLE: x\n", 0.0), None);
    }

    #[test]
    fn content_decides_without_an_extension() {
        let registry = DocumentRegistry::with_builtins();
        let org = "#+TITLE: Notes\n* TODO Write\n";
        let detected = registry.detect(None, org, CERTAIN).unwrap();
        assert_eq!(
            (detected.format.as_str(), detected.by),
            ("org", DetectedBy::Content)
        );
        // Markdown never reaches CERTAIN on content alone.
        assert_eq!(
            registry.detect(Some("README"), "# Title\n\nSee [x](y).\n", CERTAIN),
            None
        );
        assert_eq!(
            registry
                .detect(
                    None,
                    "# Title\n\nSee [x](y).\n\n```\ncode\n```\n```\n",
                    LIKELY
                )
                .map(|d| d.format),
            Some("markdown".to_string())
        );
        assert_eq!(registry.detect(None, "just some text\n", LIKELY), None);
    }

    #[test]
    fn registering_checks_names() {
        let mut registry = DocumentRegistry::with_builtins();
        assert_eq!(
            registry.register(Arc::new(OrgFormat)),
            Err(DocumentError::Conflict { name: "org".into() })
        );
        struct Bad;
        impl DocumentFormat for Bad {
            fn id(&self) -> &str {
                "Not Valid"
            }
            fn name(&self) -> &str {
                "bad"
            }
        }
        assert!(matches!(
            registry.register(Arc::new(Bad)),
            Err(DocumentError::InvalidName { .. })
        ));
    }

    #[test]
    fn a_source_renderer_becomes_a_format() {
        struct Upper;
        impl rich_plugin_api::SourceRenderer for Upper {
            fn render(
                &self,
                source: &str,
            ) -> Result<Box<dyn Renderable + Send + Sync>, rich_plugin_api::PluginError>
            {
                Ok(Box::new(Text::new(source.to_uppercase())))
            }
        }
        let mut registry = DocumentRegistry::with_builtins();
        registry
            .register(Arc::new(
                SourceRendererFormat::new("shout", Arc::new(Upper)).extensions(&["shout"]),
            ))
            .unwrap();
        let detected = registry.detect(Some("a.shout"), "", CERTAIN).unwrap();
        assert_eq!(detected.format, "shout");
        let rendered = registry
            .render("hi", "shout", &RenderOptions::new())
            .unwrap();
        let console = rich::Console::builder()
            .width(10)
            .color_system(None)
            .build();
        assert_eq!(console.render_to_string(rendered.renderable.as_ref()), "HI");
    }

    #[test]
    fn unknown_formats_name_the_known_ones() {
        let registry = DocumentRegistry::with_builtins();
        let error = registry
            .render("x", "textile", &RenderOptions::new())
            .unwrap_err();
        assert!(error.to_string().contains("org"));
    }
}
