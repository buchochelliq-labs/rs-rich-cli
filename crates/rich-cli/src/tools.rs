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
    sanitize: bool,
) -> Result<TextDiffOutcome, String> {
    if sanitize {
        // Names and contents both reach the terminal. ANSI captures keep the
        // styles they are compared by; everything else in them is shown.
        let ansi = contents.len() == 2 && contents.iter().any(|c| has_ansi(c));
        let names: Vec<String> = names
            .iter()
            .map(|n| rich_ext::sanitize_terminal_controls(n))
            .collect();
        let contents: Vec<String> = contents
            .iter()
            .map(|c| {
                if ansi {
                    crate::controls::neutralize_for_decoder(c)
                } else {
                    rich_ext::sanitize_terminal_controls(c)
                }
            })
            .collect();
        return text_diff(options, &names, &contents, threshold, false);
    }
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

/// Whether `bench` is the first positional, with no render mode selected (as
/// for `rich docs`: `rich -p bench` still prints the word).
fn is_bench(args: &[String]) -> bool {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            return false;
        }
        if super::VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
            continue;
        }
        if super::mode_flag_alias(arg).is_some() {
            return false;
        }
        if !arg.starts_with('-') || arg == "-" {
            return arg == "bench";
        }
    }
    false
}

/// `rich bench compare BASELINE CANDIDATE [--threshold PCT]`: compare two
/// benchmark runs (`rich_ext::qa::bench` JSON files, or criterion output
/// directories) and exit 5 when any benchmark regressed. `Ok(false)` when the
/// command line is not this command.
pub(crate) fn bench_dispatch(args: &[String]) -> Result<bool, String> {
    use rich_ext::qa::bench::{compare, BenchRun, CompareOptions, ComparisonView};
    if !is_bench(args) {
        return Ok(false);
    }
    // `rich bench --help` and `rich bench compare --help` show that command.
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h")
    {
        let mut path = vec!["bench"];
        let mut words = args.iter().filter(|arg| !arg.starts_with('-'));
        if words.nth(1).is_some_and(|word| word == "compare") {
            path.push("compare");
        }
        let no_color = super::cli_spec::no_color_requested(args);
        if let Some(help) = super::cli_spec::subcommand_help(&path, no_color) {
            super::authoring::out(&format!("{help}\n"));
        }
        return Ok(true);
    }
    let mut positionals = Vec::new();
    let mut threshold = None;
    let mut width = None;
    let mut no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--threshold" => {
                threshold = Some(
                    iter.next()
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .ok_or("--threshold requires a percentage")?,
                );
            }
            "-w" | "--width" => {
                width = Some(
                    iter.next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .filter(|v| *v > 0)
                        .ok_or(format!("{arg} requires a positive number of columns"))?,
                );
            }
            "--no-color" => no_color = true,
            "--color" => no_color = false,
            "--no-config" | "--machine-json" => {}
            "--report" => {
                iter.next();
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!(
                    "unknown option {other:?} for rich bench compare; it takes --threshold PCT \
                     and --width N"
                ));
            }
            other => positionals.push(other.to_string()),
        }
    }
    let [_, command, baseline, candidate] = positionals.as_slice() else {
        return Err("usage: rich bench compare BASELINE CANDIDATE [--threshold PCT]".into());
    };
    if command != "compare" {
        return Err(format!("unknown bench command {command:?}; use compare"));
    }
    let load = |path: &str| {
        let result = if std::path::Path::new(path).is_dir() {
            BenchRun::from_criterion_dir(path)
        } else {
            BenchRun::load(path)
        };
        result.map_err(|err| format!("cannot read benchmark run {path}: {err}"))
    };
    let (mut baseline, mut candidate) = match (load(baseline), load(candidate)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(err), _) | (_, Err(err)) => {
            let _ = super::emit_error(
                super::wants_json_report(args),
                super::ExitClass::Input,
                &err,
            );
            std::process::exit(i32::from(super::ExitClass::Input.code()));
        }
    };
    let mut options = CompareOptions::default();
    if let Some(threshold) = threshold {
        options.threshold_pct = threshold;
    }
    // Benchmark names come from the files; show any controls in them.
    for run in [&mut baseline, &mut candidate] {
        for measurement in &mut run.measurements {
            measurement.name = rich_ext::sanitize_terminal_controls(&measurement.name);
        }
    }
    let mut comparison = compare(&baseline, &candidate, &options);
    flag_changes_from_zero(&mut comparison, &options);
    let mut console = Console::builder().no_color(no_color);
    if let Some(width) = width {
        console = console.width(width);
    }
    console.build().print(&ComparisonView::new(&comparison));
    if comparison.has_regressions() {
        let _ = super::emit_error(
            super::wants_json_report(args),
            super::ExitClass::Gate,
            "benchmark regression",
        );
        std::process::exit(i32::from(super::ExitClass::Gate.code()));
    }
    if super::wants_json_report(args) {
        super::emit_success_report(super::ReportFormat::Json);
    }
    Ok(true)
}

/// `compare` calls any change from a 0 ns baseline 0% and so unchanged, but
/// 0 → 5 ns is a change of unbounded size: show it as `+inf%` (or `-inf%`)
/// and judge it as a regression (or improvement) unless it is within the
/// noise.
fn flag_changes_from_zero(
    comparison: &mut rich_ext::qa::bench::Comparison,
    options: &rich_ext::qa::bench::CompareOptions,
) {
    use rich_ext::qa::bench::{Noise, Statistic, Verdict};
    for row in &mut comparison.rows {
        let (Some(b), Some(c)) = (&row.baseline, &row.candidate) else {
            continue;
        };
        let value = |m: &rich_ext::qa::bench::Measurement| match options.statistic {
            Statistic::Mean => m.mean,
            Statistic::Median => m.median,
        };
        let (bv, cv) = (value(b), value(c));
        if bv > 0.0 || cv == bv {
            continue;
        }
        let noisy = match options.noise {
            Noise::Stddev => (cv - bv).abs() <= (b.stddev.powi(2) + c.stddev.powi(2)).sqrt(),
            Noise::Ignore => false,
        };
        let up = cv > bv;
        row.change_pct = Some(if up { f64::INFINITY } else { f64::NEG_INFINITY });
        if !noisy {
            row.verdict = if up {
                Verdict::Regression
            } else {
                Verdict::Improvement
            };
        }
    }
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
            false,
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
            false,
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
            false,
        )
        .err()
        .unwrap();
        assert!(err.contains("give two files to compare"), "{err}");
    }

    #[test]
    fn ansi_inputs_diff_on_visible_text_and_styles() {
        let old = "\u{1b}[31mred\u{1b}[0m\nsame\n";
        let new = "\u{1b}[32mred\u{1b}[0m\nsame\n";
        // Sanitizing keeps the styles an ANSI capture is compared by.
        for sanitize in [false, true] {
            let outcome = text_diff(
                &ToolOptions::default(),
                &names(&["a.ansi", "b.ansi"]),
                &names(&[old, new]),
                None,
                sanitize,
            )
            .unwrap();
            let out = render(outcome.renderable.as_ref());
            assert!(out.contains('~'), "a style-only change must show:\n{out}");
        }
    }

    #[test]
    fn bench_is_a_command_word_only_in_first_position() {
        let args = |items: &[&str]| names(items);
        assert!(is_bench(&args(&["bench", "compare", "a", "b"])));
        assert!(is_bench(&args(&["--no-color", "bench"])));
        assert!(!is_bench(&args(&["--title", "bench", "file.txt"])));
        assert!(!is_bench(&args(&["-p", "bench"])));
        assert!(!is_bench(&args(&["notes.txt", "bench"])));
    }

    #[test]
    fn ansi_explain_decodes_sgr() {
        let out = render(ansi_explain(&ToolOptions::default(), "\u{1b}[1;31mhi\u{1b}[0m").as_ref());
        assert!(out.contains("bold"), "{out}");
        assert!(out.contains("hi"), "{out}");
    }
}
