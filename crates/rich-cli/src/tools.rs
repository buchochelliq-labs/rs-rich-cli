//! `rich diff` for text, ANSI captures, source and patches, and
//! `rich ansi explain`: CLI glue over `rich_ext::diff` and
//! `rich_ext::ansi_explain`.
//!
//! Neither exists upstream (upstream's CLI has no diff; ours compared images
//! only). Both compose public rich-ext renderables, so they are binary-boundary
//! conveniences like `inspect.rs`.
use rich::measure::Measurement;
use rich::text::Text;
use rich::{Console, ConsoleOptions, Renderable, Segment};
use rich_ext::ansi_explain::{explain, Explanation, ExplanationView, ViewMode};
use rich_ext::diff::{git, DiffView, Layout, SourceDiff};

/// Options for the text diff and `ansi explain`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ToolOptions {
    side_by_side: bool,
    context: Option<usize>,
    language: Option<String>,
    ansi_inline: bool,
    escapes_only: bool,
}

impl ToolOptions {
    /// Consume one of these options; anything else stays with the main parser.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match arg {
            "--side-by-side" => self.side_by_side = true,
            "--context" => {
                self.context = Some(
                    rest.next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .ok_or("--context requires a number of lines")?,
                );
            }
            "--language" => {
                self.language = Some(rest.next().ok_or("--language requires a name")?.clone());
            }
            "--ansi-inline" => self.ansi_inline = true,
            "--escapes-only" => self.escapes_only = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The first text-diff option given, for the "only has an effect" check.
    pub(crate) fn diff_option(&self) -> Option<&'static str> {
        [
            ("--side-by-side", self.side_by_side),
            ("--context", self.context.is_some()),
            ("--language", self.language.is_some()),
        ]
        .into_iter()
        .find_map(|(flag, given)| given.then_some(flag))
    }

    /// The first `ansi explain` option given.
    pub(crate) fn ansi_option(&self) -> Option<&'static str> {
        [
            ("--ansi-inline", self.ansi_inline),
            ("--escapes-only", self.escapes_only),
        ]
        .into_iter()
        .find_map(|(flag, given)| given.then_some(flag))
    }

    fn layout(&self) -> Layout {
        if self.side_by_side {
            Layout::SideBySide
        } else {
            Layout::Unified
        }
    }
}

/// Renderables printed one after another.
struct Stack(Vec<Box<dyn Renderable>>);

impl Renderable for Stack {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        for part in &self.0 {
            // Each part starts on its own line, whether or not the previous
            // one ended with a newline.
            if out.last().is_some_and(|last| !last.text.ends_with('\n')) {
                out.push(Segment::line());
            }
            out.extend(part.rich_render(console, options));
        }
        out
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.0
            .iter()
            .map(|part| part.measure(console, options))
            .fold(Measurement::new(0, 0), |a, b| {
                Measurement::new(a.minimum.max(b.minimum), a.maximum.max(b.maximum))
            })
    }
}

/// A text diff ready to print, and whether it trips `--threshold`.
pub(crate) struct TextDiffOutcome {
    pub renderable: Box<dyn Renderable>,
    pub failed: bool,
}

fn has_ansi(text: &str) -> bool {
    text.contains('\u{1b}')
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `rich diff OLD NEW` for anything that is not an image pair, or
/// `rich diff PATCH` for a unified diff such as `git diff` output.
pub(crate) fn text_diff(
    options: &ToolOptions,
    names: &[String],
    contents: &[String],
    threshold: Option<f32>,
) -> Result<TextDiffOutcome, String> {
    if let [patch] = contents {
        let parsed = git::parse_unified(patch).map_err(|err| {
            format!(
                "{}: not a unified diff (line {}: {}); give two files to compare",
                names[0], err.line, err.message
            )
        })?;
        if parsed.files.is_empty() {
            return Err(format!(
                "{}: no file changes found in it; give two files to compare",
                names[0]
            ));
        }
        let (added, removed) = parsed.stats();
        let view = git::PatchView::new(parsed).layout(options.layout());
        let summary = Text::from_markup(&format!("[dim]{} added, {} removed[/]", added, removed))
            .map_err(|err| err.to_string())?;
        return Ok(TextDiffOutcome {
            renderable: Box::new(Stack(vec![Box::new(view), Box::new(summary)])),
            failed: false,
        });
    }
    let [old, new] = contents else {
        return Err("--diff needs two files to compare, or one patch".into());
    };
    let (old_name, new_name) = (names[0].as_str(), names[1].as_str());
    let context = options.context.unwrap_or(3);
    let ansi = has_ansi(old) || has_ansi(new);
    let (view, stats): (Box<dyn Renderable>, (usize, usize)) = if ansi {
        let view = DiffView::ansi(old, new)
            .layout(options.layout())
            .context(context)
            .titles(old_name, new_name);
        let (added, removed, restyled) = view.stats();
        (Box::new(view), (added + restyled, removed + restyled))
    } else {
        let mut source = SourceDiff::new(old.as_str(), new.as_str())
            .paths(old_name, new_name)
            .layout(options.layout())
            .context(context);
        if let Some(language) = &options.language {
            source = source.language(language.as_str());
        }
        let (added, removed, _) = source.view().stats();
        (Box::new(source), (added, removed))
    };
    let (added, removed) = stats;
    let total = old.lines().count() + new.lines().count();
    let changed = if total == 0 {
        0.0
    } else {
        (added + removed) as f32 * 100.0 / total as f32
    };
    let mut summary = format!(
        "[dim]{} → {}: {added} added, {removed} removed ({changed:.1}% of lines changed)[/]",
        rich::markup::escape(file_name(old_name)),
        rich::markup::escape(file_name(new_name)),
    );
    let mut failed = false;
    if let Some(limit) = threshold {
        failed = super::diff_threshold_exceeded(changed, limit);
        summary.push('\n');
        summary.push_str(&if failed {
            format!("[bold red]FAIL[/] {changed:.1}% changed, limit {limit:.1}%")
        } else {
            format!("[bold green]OK[/] {changed:.1}% changed, within {limit:.1}%")
        });
    }
    let summary = Text::from_markup(&summary).map_err(|err| err.to_string())?;
    Ok(TextDiffOutcome {
        renderable: Box::new(Stack(vec![view, Box::new(summary)])),
        failed,
    })
}

/// `ExplanationView` borrows its explanation; this owns it for the CLI.
struct Explained {
    explanation: Explanation,
    inline: bool,
    escapes_only: bool,
}

impl Explained {
    fn view(&self) -> ExplanationView<'_> {
        let mode = if self.inline {
            ViewMode::Inline
        } else {
            ViewMode::Table
        };
        ExplanationView::new(&self.explanation)
            .mode(mode)
            .escapes_only(self.escapes_only)
    }
}

impl Renderable for Explained {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.view().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.view().measure(console, options)
    }
}

/// `rich ansi explain`: every escape sequence in `content`, decoded.
pub(crate) fn ansi_explain(options: &ToolOptions, content: &str) -> Box<dyn Renderable> {
    Box::new(Explained {
        explanation: explain(content),
        inline: options.ansi_inline,
        escapes_only: options.escapes_only,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(renderable: &dyn Renderable) -> String {
        Console::builder()
            .width(80)
            .no_color(true)
            .build()
            .render_export(renderable)
    }

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn text_diff_reports_changes_and_the_threshold() {
        let contents = names(&["a\nb\nc\n", "a\nB\nc\n"]);
        let outcome = text_diff(
            &ToolOptions::default(),
            &names(&["old.txt", "new.txt"]),
            &contents,
            Some(10.0),
        )
        .unwrap();
        let out = render(outcome.renderable.as_ref());
        assert!(out.contains("-") && out.contains("B"), "{out}");
        assert!(
            out.contains("1 added, 1 removed (33.3% of lines changed)"),
            "{out}"
        );
        assert!(out.contains("FAIL 33.3% changed, limit 10.0%"), "{out}");
        assert!(outcome.failed);
    }

    #[test]
    fn a_single_patch_renders_its_files() {
        let patch = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1 +1 @@\n-fn a() {}\n+fn b() {}\n";
        let outcome = text_diff(
            &ToolOptions::default(),
            &names(&["change.patch"]),
            &names(&[patch]),
            None,
        )
        .unwrap();
        let out = render(outcome.renderable.as_ref());
        assert!(out.contains("x.rs") && out.contains("fn b()"), "{out}");
        assert!(out.contains("1 added, 1 removed"), "{out}");

        let err = text_diff(
            &ToolOptions::default(),
            &names(&["notes.txt"]),
            &names(&["just text\n"]),
            None,
        )
        .err()
        .unwrap();
        assert!(err.contains("give two files to compare"), "{err}");
    }

    #[test]
    fn ansi_inputs_diff_on_visible_text_and_styles() {
        let old = "\u{1b}[31mred\u{1b}[0m\nsame\n";
        let new = "\u{1b}[32mred\u{1b}[0m\nsame\n";
        let outcome = text_diff(
            &ToolOptions::default(),
            &names(&["a.ansi", "b.ansi"]),
            &names(&[old, new]),
            None,
        )
        .unwrap();
        let out = render(outcome.renderable.as_ref());
        assert!(out.contains('~'), "a style-only change must show:\n{out}");
    }

    #[test]
    fn ansi_explain_decodes_sgr() {
        let out = render(ansi_explain(&ToolOptions::default(), "\u{1b}[1;31mhi\u{1b}[0m").as_ref());
        assert!(out.contains("bold"), "{out}");
        assert!(out.contains("hi"), "{out}");
    }
}
