//! Mermaid diagrams for the `rich` Rust port.
//!
//! [`Mermaid`] renders a diagram's source. Flowcharts (`graph` / `flowchart`)
//! are drawn as text with box-drawing characters, by this crate alone. With the
//! optional `mmdc` feature, and when asked for with [`Backend::Mmdc`], every
//! diagram type is rendered by Mermaid's own CLI and shown as an image through
//! `rs-rich-art`. Whatever cannot be drawn is shown as its source, in a code
//! block, under a one-line note saying why.
//!
//! [`MermaidPlugin`] registers the same through the plugin API: a fence
//! renderer for ```` ```mermaid ```` blocks in Markdown and a `mermaid` source
//! renderer. It uses only public items of `rich` and `rich_plugin_api`.
//!
//! ```
//! use rich::Console;
//! use rich_mermaid::Mermaid;
//!
//! let console = Console::builder().width(40).color_system(None).build();
//! let out = console.render_to_string(&Mermaid::new("graph LR\n  A --> B"));
//! assert!(out.contains("│ A ├─►│ B │"), "{out}");
//! ```

use std::sync::Arc;

use rich::cells::{cell_len, set_cell_size};
use rich::console::{Console, ConsoleOptions};
use rich::protocol::{FenceRenderer, Renderable};
use rich::segment::Segment;
use rich::style::Style;
use rich::syntax::Syntax;
use rich::text::Text;
use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar, SourceRenderer};

pub mod flowchart;
pub mod layout;
#[cfg(feature = "mmdc")]
pub mod mmdc;

pub use flowchart::{parse, Flowchart, ParseError};
pub use layout::{draw, Diagram};

/// Which renderer to try first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    /// Draw flowcharts as text; show other diagram types as source.
    #[default]
    Text,
    /// Try Mermaid's own CLI first. Needs the `mmdc` feature and `mmdc`
    /// installed; falls back to text (or source) when it is unavailable or
    /// fails. Without a graphics protocol, flowcharts still prefer text.
    Mmdc,
}

/// How to render diagrams.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MermaidOptions {
    pub backend: Backend,
    /// Draw with ASCII only. `None` follows [`Console::ascii_only`].
    pub ascii: Option<bool>,
    /// How to run `mmdc` for [`Backend::Mmdc`].
    #[cfg(feature = "mmdc")]
    pub mmdc: mmdc::MmdcOptions,
}

/// A Mermaid diagram.
#[derive(Clone, Debug)]
pub struct Mermaid {
    source: String,
    options: MermaidOptions,
}

impl Mermaid {
    pub fn new(source: impl Into<String>) -> Self {
        Mermaid {
            source: source.into(),
            options: MermaidOptions::default(),
        }
    }

    pub fn options(mut self, options: MermaidOptions) -> Self {
        self.options = options;
        self
    }

    pub fn backend(mut self, backend: Backend) -> Self {
        self.options.backend = backend;
        self
    }

    pub fn ascii(mut self, ascii: bool) -> Self {
        self.options.ascii = Some(ascii);
        self
    }

    fn render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let parsed = flowchart::parse(&self.source);
        match self.options.backend {
            Backend::Text => {}
            #[cfg(feature = "mmdc")]
            Backend::Mmdc => return self.render_mmdc(parsed, console, options),
            #[cfg(not(feature = "mmdc"))]
            Backend::Mmdc => {
                return self.render_text(
                    parsed,
                    console,
                    options,
                    Some("this build has no mmdc backend".into()),
                )
            }
        }
        self.render_text(parsed, console, options, None)
    }

    /// Draw a flowchart as text, or show the source with the reason it could
    /// not be. `why` explains a fallback from another backend.
    fn render_text(
        &self,
        parsed: Result<Flowchart, ParseError>,
        console: &Console,
        options: &ConsoleOptions,
        why: Option<String>,
    ) -> Vec<Segment> {
        let chart = match parsed {
            Ok(chart) if chart.nodes.is_empty() => {
                return self.source_block(console, options, "the flowchart has no nodes")
            }
            Ok(chart) => chart,
            Err(error) => {
                let reason = match why {
                    Some(why) => format!("{why}; {error}"),
                    None => error.to_string(),
                };
                return self.source_block(console, options, &reason);
            }
        };
        let ascii = self.options.ascii.unwrap_or_else(|| console.ascii_only());
        let diagram = match layout::draw(&chart, ascii) {
            Ok(diagram) => diagram,
            Err(reason) => {
                return self.source_block(console, options, &format!("too large to draw: {reason}"))
            }
        };
        let width = options.max_width;
        let mut lines: Vec<String> = diagram
            .lines
            .iter()
            .map(|line| {
                if cell_len(line) > width {
                    set_cell_size(line, width).trim_end().to_string()
                } else {
                    line.clone()
                }
            })
            .collect();
        // Cropping can leave rows empty at either end.
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let leading = lines.iter().take_while(|line| line.is_empty()).count();
        lines.drain(..leading);
        let mut segments = Vec::new();
        for line in lines {
            segments.push(Segment::new(line, None));
            segments.push(Segment::line());
        }
        let mut notes: Vec<String> = Vec::new();
        if let Some(why) = why {
            notes.push(format!("{why}; drawn as text"));
        }
        notes.extend(chart.notes.iter().cloned());
        if diagram.width > width {
            notes.push(format!("cropped to {width} of {} columns", diagram.width));
        }
        for text in notes {
            segments.extend(note(&text, console, options));
        }
        segments
    }

    /// The source as a code block, under a note.
    fn source_block(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        reason: &str,
    ) -> Vec<Segment> {
        let mut segments = note(reason, console, options);
        if self.source.trim().is_empty() {
            return segments;
        }
        // Core keeps ESC in text, as upstream does; the source comes from a
        // document, so drop control characters before showing it.
        let source: String = self
            .source
            .trim_end_matches('\n')
            .chars()
            .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
            .collect();
        let syntax = Syntax::new(source.as_str(), "mermaid")
            .word_wrap(true)
            .padding(1);
        segments.extend(syntax.rich_render(console, options));
        segments
    }

    #[cfg(feature = "mmdc")]
    fn render_mmdc(
        &self,
        parsed: Result<Flowchart, ParseError>,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Vec<Segment> {
        use rich_art::{ImageArt, ImageMode};

        let (graphics, color) = graphics(console);
        // Block characters blur the text in a diagram. A flowchart reads
        // better drawn as text unless the terminal can show real pixels.
        if !graphics && matches!(&parsed, Ok(chart) if !chart.nodes.is_empty()) {
            return self.render_text(parsed, console, options, None);
        }
        let png = match mmdc::render_png(&self.source, &self.options.mmdc) {
            Ok(png) => png,
            Err(error) => {
                return self.render_text(parsed, console, options, Some(error.to_string()))
            }
        };
        let mode = if graphics {
            ImageMode::Sixel
        } else if color {
            ImageMode::Quadrants
        } else {
            ImageMode::Ascii
        };
        let art = match ImageArt::from_bytes(&png) {
            Ok(art) => art.mode(mode).max_width(options.max_width),
            Err(error) => {
                return self.render_text(
                    parsed,
                    console,
                    options,
                    Some(format!("mmdc wrote an unreadable image: {error}")),
                )
            }
        };
        match art.render(console, options) {
            Ok(mut segments) => {
                end_line(&mut segments);
                if !graphics {
                    segments.extend(note(
                        "drawn with block characters; a terminal with Sixel graphics shows it sharper",
                        console,
                        options,
                    ));
                }
                segments
            }
            Err(error) => self.render_text(parsed, console, options, Some(error.to_string())),
        }
    }
}

/// Whether the destination shows real pixels (Sixel), and whether it has colour.
#[cfg(feature = "mmdc")]
fn graphics(console: &Console) -> (bool, bool) {
    use rich::protocol::{ConsoleEnvironment, Support};
    if let Some(environment) = console.render_environment() {
        let caps = environment.capabilities();
        return (
            caps.interactive && caps.sixel != Support::Unsupported,
            caps.color_system.is_some(),
        );
    }
    let caps = rich_art::image_art::RenderCapabilities::from_console(console);
    (caps.sixel_supported, caps.color)
}

/// A dim note, wrapped to the width, ending its line.
fn note(text: &str, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
    let style = Style::parse("dim italic").expect("valid style");
    let mut segments =
        Text::styled(format!("Mermaid: {text}"), style).rich_render(console, options);
    end_line(&mut segments);
    segments
}

/// End the last line, so whatever follows starts on a line of its own.
fn end_line(segments: &mut Vec<Segment>) {
    if segments
        .iter()
        .rev()
        .find(|segment| !segment.text.is_empty())
        .is_some_and(|segment| !segment.text.ends_with('\n'))
    {
        segments.push(Segment::line());
    }
}

impl Renderable for Mermaid {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.render(console, options)
    }
}

/// Renders ```` ```mermaid ```` fences in [`rich::markdown::Markdown`].
#[derive(Clone, Debug, Default)]
pub struct MermaidFences {
    pub options: MermaidOptions,
}

impl FenceRenderer for MermaidFences {
    fn render_fence(
        &self,
        language: &str,
        code: &str,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Option<Vec<Segment>> {
        if !language.eq_ignore_ascii_case("mermaid") {
            return None;
        }
        let diagram = Mermaid::new(code).options(self.options.clone());
        Some(diagram.rich_render(console, options))
    }
}

impl SourceRenderer for MermaidFences {
    fn render(&self, source: &str) -> Result<Box<dyn Renderable + Send + Sync>, PluginError> {
        Ok(Box::new(Mermaid::new(source).options(self.options.clone())))
    }
}

/// Registers the `mermaid` fence renderer and source renderer.
#[derive(Clone, Debug, Default)]
pub struct MermaidPlugin {
    pub options: MermaidOptions,
}

impl MermaidPlugin {
    pub fn new(options: MermaidOptions) -> Self {
        MermaidPlugin { options }
    }
}

impl Plugin for MermaidPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("mermaid", "Mermaid diagrams", env!("CARGO_PKG_VERSION"))
            .description("Mermaid flowcharts as text, and every diagram type through mmdc")
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        let fences = Arc::new(MermaidFences {
            options: self.options.clone(),
        });
        registrar.fence_renderer("mermaid", fences.clone());
        registrar.renderer("mermaid", fences);
        Ok(())
    }
}
