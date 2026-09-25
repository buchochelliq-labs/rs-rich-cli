//! [`SourceDiff`]: a syntax-highlighted source diff.

use std::sync::Arc;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, StyleType, Syntax, Text};

use super::engine::split_lines_inclusive;
use super::view::{text_lines, DiffView, LineLinks, Side};
use super::Layout;
use crate::hyperlink::Hyperlinker;

/// The language name to hand core [`Syntax`] for `path`: its extension, or
/// the file name when it has none (`Makefile`).
pub(crate) fn language_for_path(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_string(),
        _ => name.to_string(),
    }
}

/// A style without its background, so diff line styles show through.
fn foreground_only(style: &Style) -> Style {
    let definition = style.definition();
    let kept = match definition.split_once(" on ") {
        Some((before, _)) => before.to_string(),
        None if definition.starts_with("on ") => String::new(),
        None => definition,
    };
    Style::parse(&kept).unwrap_or_default()
}

/// Highlight `code` as `language` and split it into lines, one per
/// `split_lines_inclusive` line. The whole side is highlighted at once so
/// multi-line constructs (block comments, strings) colour correctly. With a
/// `console`, its default code highlighter applies.
pub(crate) fn highlight_lines(
    code: &str,
    language: Option<&str>,
    console: Option<&Console>,
) -> Vec<Text> {
    let count = split_lines_inclusive(code).len();
    let Some(language) = language else {
        return split_lines_inclusive(code)
            .into_iter()
            .map(|l| Text::new(super::engine::strip_eol(l)))
            .collect();
    };
    let syntax = Syntax::new(code, language);
    let highlighted = match console {
        Some(console) => syntax.highlight_for(console),
        None => syntax.highlight(),
    };
    let mut plain = Text::new(highlighted.plain());
    for span in highlighted.spans() {
        if let StyleType::Style(style) = &span.style {
            let fg = foreground_only(style);
            if !fg.is_null() {
                plain.stylize(fg, span.start, span.end);
            }
        }
    }
    let mut lines = text_lines(&plain);
    lines.resize_with(count, || Text::new(""));
    lines
}

/// A syntax-highlighted diff of two versions of a source file.
///
/// Each side is highlighted whole, then split into lines; word-level
/// emphasis goes on top. The language comes from [`language`](Self::language)
/// or the extension of [`path`](Self::path).
///
/// ```
/// use rich::Console;
/// use rich_ext::diff::SourceDiff;
///
/// let diff = SourceDiff::new("fn a() {}\n", "fn b() {}\n").path("src/lib.rs");
/// let console = Console::builder().width(40).no_color(true).build();
/// assert!(console.render_to_string(&diff).contains("+ fn b() {}"));
/// ```
#[derive(Clone, Debug)]
pub struct SourceDiff {
    old: String,
    new: String,
    language: Option<String>,
    old_path: Option<String>,
    new_path: Option<String>,
    layout: Layout,
    line_numbers: bool,
    wrap: bool,
    context: usize,
    titles: bool,
    linker: Option<Hyperlinker>,
}

impl SourceDiff {
    /// Diff two versions of a source text.
    pub fn new(old: impl Into<String>, new: impl Into<String>) -> Self {
        SourceDiff {
            old: old.into(),
            new: new.into(),
            language: None,
            old_path: None,
            new_path: None,
            layout: Layout::Unified,
            line_numbers: true,
            wrap: true,
            context: 3,
            titles: true,
            linker: None,
        }
    }
    /// The language (a name or extension core `Syntax` knows).
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }
    /// The file's path, for the title, links and (without
    /// [`language`](Self::language)) the language.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        let path = path.into();
        self.old_path = Some(path.clone());
        self.new_path = Some(path);
        self
    }
    /// Different old and new paths (a rename).
    pub fn paths(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.old_path = Some(old.into());
        self.new_path = Some(new.into());
        self
    }
    /// Unified (default) or side by side.
    pub fn layout(mut self, layout: Layout) -> Self {
        self.layout = layout;
        self
    }
    /// Show line numbers (default on).
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers = show;
        self
    }
    /// Wrap long lines (default) or truncate them.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }
    /// Unchanged lines around each change (default 3).
    pub fn context(mut self, lines: usize) -> Self {
        self.context = lines;
        self
    }
    /// Show the paths as titles (default on, when a path is set).
    pub fn titles(mut self, show: bool) -> Self {
        self.titles = show;
        self
    }
    /// Link line numbers through an editor or web URL template with `{path}`
    /// and `{line}` (and `{column}`, always 1), such as
    /// `vscode://file/{path}:{line}`.
    pub fn link_template(mut self, template: impl Into<String>) -> Self {
        self.linker = Some(Hyperlinker::new().editor(template));
        self
    }
    /// Link line numbers through a [`Hyperlinker`] (`file://` URLs by
    /// default, relative paths resolved against its base directory).
    pub fn hyperlinker(mut self, linker: Hyperlinker) -> Self {
        self.linker = Some(linker);
        self
    }

    fn resolved_language(&self) -> Option<String> {
        self.language.clone().or_else(|| {
            self.new_path
                .as_deref()
                .or(self.old_path.as_deref())
                .map(language_for_path)
        })
    }

    /// The view this diff renders as.
    pub fn view(&self) -> DiffView {
        self.view_with(None)
    }

    /// [`view`](Self::view), highlighting with `console`'s default code
    /// highlighter. Rendering uses this.
    pub fn view_for(&self, console: &Console) -> DiffView {
        self.view_with(Some(console))
    }

    fn view_with(&self, console: Option<&Console>) -> DiffView {
        let language = self.resolved_language();
        let old = highlight_lines(&self.old, language.as_deref(), console);
        let new = highlight_lines(&self.new, language.as_deref(), console);
        let links: Option<LineLinks> = match (&self.linker, &self.old_path, &self.new_path) {
            (Some(linker), Some(old_path), Some(new_path)) => {
                let (linker, old_path, new_path) =
                    (linker.clone(), old_path.clone(), new_path.clone());
                Some(Arc::new(move |side: Side, line: usize| {
                    let path = if side == Side::Old {
                        &old_path
                    } else {
                        &new_path
                    };
                    linker.file_url(path, Some(line), None)
                }))
            }
            _ => None,
        };
        let mut view = DiffView::from_parts(
            old,
            new,
            &split_lines_inclusive(&self.old),
            &split_lines_inclusive(&self.new),
            false,
        )
        .layout(self.layout)
        .line_numbers(self.line_numbers)
        .wrap(self.wrap)
        .context(self.context)
        .links(links);
        if self.titles {
            if let (Some(old), Some(new)) = (&self.old_path, &self.new_path) {
                view = view.titles(old.clone(), new.clone());
            }
        }
        view
    }
}

impl Renderable for SourceDiff {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.view_for(console).rich_render(console, options)
    }
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.view_for(console).measure(console, options)
    }
}
