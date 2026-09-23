//! `rich` — a Rust port of the `rich-cli` terminal toolbox.
//!
//! This binary mirrors the upstream `rich-cli` command-line tool and is built on
//! the [`rich`] library crate. It implements the rendering modes `--print`,
//! `--markdown`, `--json`, `--syntax`, `--csv`, `--ipynb`, and `--rule`, plus
//! width/justify, HTML/SVG export to a file (`--export-html PATH` / `-o PATH`,
//! `--export-svg PATH`, which may be combined), the
//! `--panel`/`--padding` decorators (with `--title`/`--caption`/`--style`), a
//! plain-file printer (with extension auto-detection), **URL fetch** (`rich <url>`,
//! behind the default `fetch` feature), **paging** (`--pager`), and a capability
//! demo — i.e. the whole common rich-cli surface.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod authoring;
mod batch;
mod batch_output;
mod batch_paths;
#[cfg(test)]
mod batch_races;
mod cli_spec;
mod config;
mod demo;
mod doctor;
mod inspect;
mod render_target;
mod structured_log;
mod tools;
mod watch;
use batch::run_batch;
use config::{config_args, ConfigRoots};

use rich::cells::cell_len;
use rich::markdown::Markdown;
use rich::measure::Measurement;
use rich::protocol::{LineRenderable, OwnedTableRows};
use rich::r#box::{Box as BoxSet, ASCII, ASCII2, DOUBLE, HEAVY, HEAVY_HEAD, ROUNDED, SQUARE};
use rich::text::Text;
use rich::{
    filesize, Align, AnsiDecoder, Bar, ColorSystem, Columns, Console, ConsoleOptions, Constrain,
    Control, Highlighter, HorizontalAlign, ISO8601Highlighter, Json, Justify, Layout, Live,
    LiveRender, LogLevel, LogRender, Padding, Panel, Pretty, Progress, ProgressBar, ProgressColumn,
    Renderable, Rule, Segment, Spinner, Status, Style, Styled, Syntax, Table, Traceback, Tree,
    DEFAULT_TERMINAL_THEME,
};
use rich_ext::cli::CliExtensions;
use rich_ext::encoding::{has_utf16_bom, Encoding};
use rich_ext::sanitize_terminal_controls;
use rich_ext::ConsoleExt;

/// Boxed `Text` helper to cut down on `Box::new(Text::new(...))` noise.
fn text(content: &str) -> Box<dyn Renderable> {
    Box::new(Text::new(content))
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How to render the resource. Mirrors the mutually-exclusive rich-cli flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// No explicit flag: auto-detect by file extension, else print plain.
    Auto,
    /// `-p/--print`: interpret the resource as console markup.
    Print,
    /// `-m/--markdown`: render the resource as Markdown.
    Markdown,
    /// `-j/--json`: pretty-print the resource as JSON.
    Json,
    /// `-x/--syntax`: syntax-highlight the resource (language from extension).
    Syntax,
    /// `--csv`: render a CSV/TSV resource as a table.
    Csv,
    /// `.ipynb`: render a Jupyter notebook (markdown + code cells + outputs).
    Ipynb,
    /// `--gif`: animate one or more GIFs in place.
    Gif,
    /// `--diff`: perceptually compare two images (takes exactly two resources).
    Diff,
    /// `--image`: render a single still image as ASCII art, half-blocks, or Sixel.
    Image,
    /// `--rule`: draw a horizontal rule (the resource, if any, is its title).
    Rule,
    /// `--jsonl`: stream JSON Lines / NDJSON records.
    JsonLines,
    /// `--log`: stream common structured-log JSONL records.
    Log,
    /// `--inspect`: explore structured data (not upstream; see `inspect.rs`).
    Inspect,
    /// `ansi explain` / `--ansi-explain`: decode escape sequences (`tools.rs`).
    AnsiExplain,
}

impl Mode {
    fn streams_records(self) -> bool {
        matches!(self, Self::JsonLines | Self::Log)
    }

    fn draws_directly(self) -> bool {
        matches!(self, Self::Gif | Self::Diff | Self::Image) || self.streams_records()
    }

    fn allows_extensions(self) -> bool {
        !matches!(self, Self::Gif | Self::Diff | Self::Image | Self::Rule)
            && !self.streams_records()
    }

    fn accepts_multiple_resources(self) -> bool {
        matches!(self, Self::Gif | Self::Diff)
    }
}

struct ModeSpec {
    mode: Mode,
    primary: &'static str,
    aliases: &'static [&'static str],
}

const MODE_SPECS: &[ModeSpec] = &[
    ModeSpec {
        mode: Mode::Auto,
        primary: "auto",
        aliases: &[],
    },
    ModeSpec {
        mode: Mode::Print,
        primary: "print",
        aliases: &["print"],
    },
    ModeSpec {
        mode: Mode::Markdown,
        primary: "markdown",
        aliases: &["markdown", "md"],
    },
    ModeSpec {
        mode: Mode::Json,
        primary: "json",
        aliases: &["json"],
    },
    ModeSpec {
        mode: Mode::Syntax,
        primary: "syntax",
        aliases: &["syntax", "code"],
    },
    ModeSpec {
        mode: Mode::Csv,
        primary: "csv",
        aliases: &["csv", "tsv"],
    },
    ModeSpec {
        mode: Mode::Ipynb,
        primary: "ipynb",
        aliases: &["ipynb", "notebook"],
    },
    ModeSpec {
        mode: Mode::Gif,
        primary: "gif",
        aliases: &["gif"],
    },
    ModeSpec {
        mode: Mode::Diff,
        primary: "diff",
        aliases: &["diff"],
    },
    ModeSpec {
        mode: Mode::Image,
        primary: "image",
        aliases: &["image"],
    },
    ModeSpec {
        mode: Mode::Rule,
        primary: "rule",
        aliases: &["rule"],
    },
    ModeSpec {
        mode: Mode::JsonLines,
        primary: "jsonl",
        aliases: &["jsonl", "ndjson"],
    },
    ModeSpec {
        mode: Mode::Log,
        primary: "log",
        aliases: &["log", "logs"],
    },
    ModeSpec {
        mode: Mode::Inspect,
        primary: "inspect",
        aliases: &["inspect"],
    },
    ModeSpec {
        mode: Mode::AnsiExplain,
        primary: "ansi",
        aliases: &["ansi", "ansi-explain"],
    },
];

const RENDER_MODE_FLAGS: &str =
    "--print/--markdown/--json/--syntax/--csv/--ipynb/--rule/--gif/--diff/--image/--jsonl/--log/--inspect/--ansi-explain";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReportFormat {
    Human,
    Json,
}

impl ReportFormat {
    fn apply_option(&mut self, option: &str, value: Option<&str>) -> Result<(), String> {
        match option {
            "--report" => {
                *self = match value.ok_or("--report requires one of: human, json")? {
                    "human" => Self::Human,
                    "json" => Self::Json,
                    other => return Err(format!("unknown report format {other:?} (human, json)")),
                };
            }
            "--machine-json" => *self = Self::Json,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitClass {
    Success,
    Usage,
    Input,
    Data,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    Gate,
}

impl ExitClass {
    fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Usage => 2,
            Self::Input => 3,
            Self::Data => 4,
            Self::Gate => 5,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Usage => "usage",
            Self::Input => "input",
            Self::Data => "data",
            Self::Gate => "gate",
        }
    }

    fn exit_code(self) -> ExitCode {
        ExitCode::from(self.code())
    }
}

/// How `--diff` draws the image part of its report.
///
/// Detection of terminal graphics support is a heuristic and will be wrong
/// somewhere (there is no reliable probe that works when output is piped), so
/// this is exposed rather than inferred: when the guess is wrong the user
/// picks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ImageMode {
    /// Sixel where it looks supported, else half-blocks, else ASCII.
    Auto,
    /// Real pixels via the Sixel graphics protocol.
    Sixel,
    /// Half-block characters — works in any truecolour terminal.
    Blocks,
    /// Unicode Braille cells.
    Braille,
    /// Unicode quadrant blocks: 2×2 pixels, two colours per cell.
    Quadrants,
    /// A character ramp, the jp2a-style rendering. No colour required.
    Ascii,
    /// Skip the picture; print only the numbers.
    None,
}

impl std::str::FromStr for ImageMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "sixel" => Ok(Self::Sixel),
            "blocks" | "block" | "half-block" | "half-blocks" => Ok(Self::Blocks),
            "braille" => Ok(Self::Braille),
            "quadrants" | "quadrant" => Ok(Self::Quadrants),
            "ascii" | "art" => Ok(Self::Ascii),
            "none" | "off" => Ok(Self::None),
            other => Err(format!(
                "unknown image mode {other:?} (auto, sixel, blocks, quadrants, braille, ascii, none)"
            )),
        }
    }
}

/// Parsed command line.
#[derive(Clone)]
struct Cli {
    mode: Mode,
    resource: Option<String>,
    /// Every positional argument. Only `--gif` accepts more than one (so
    /// several animations can share the screen); other modes reject extras.
    /// Read only by the GIF player, hence unused without the `art` feature.
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    resources: Vec<String>,
    /// `--loop N`: how many times `--gif` repeats (0 = forever).
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    loops: Option<usize>,
    /// `--threshold PCT`: with `--diff`, exit non-zero when the changed
    /// percentage of the canvas exceeds this. Makes the tool a CI gate.
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    diff_threshold: Option<f32>,
    /// `--image-mode`: how `--diff`/`--image` draws its picture.
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_mode: ImageMode,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_fit: Option<String>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_anchor: Option<String>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_background: Option<[u8; 3]>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_color: Option<String>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_dither: Option<String>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_rotate: Option<u16>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_flip_horizontal: bool,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_flip_vertical: bool,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_grayscale: bool,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_max_width: Option<usize>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_max_height: Option<usize>,
    /// `--image-brightness`, `--image-contrast`, `--image-gamma` (1.0 = unchanged).
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_brightness: Option<f32>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_contrast: Option<f32>,
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    image_gamma: Option<f32>,
    log_presentation: String,
    theme_styles: std::collections::BTreeMap<String, Style>,
    /// `--height N`: with `--image`, render this many rows instead of the
    /// backend's default.
    #[cfg_attr(not(feature = "art"), allow(dead_code))]
    height: Option<usize>,
    extensions: CliExtensions,
    width: Option<usize>,
    justify: Option<Justify>,
    no_color: bool,
    /// `--export-html PATH` (`-o PATH`): also write a self-contained HTML
    /// document to PATH. The resource is still rendered to the terminal.
    export_html: Option<String>,
    /// `--export-svg PATH`: also write a self-contained SVG document to PATH.
    export_svg: Option<String>,
    /// `--panel BOX`: wrap the output in a [`Panel`] with the named box.
    /// `--panel none` is upstream's default and means *no panel at all*, so it
    /// parses to `None` rather than to `box.NONE`.
    panel: Option<BoxSet>,
    /// `--padding T[,R[,B,L]]`: wrap the output in [`Padding`].
    padding: Option<(usize, usize, usize, usize)>,
    /// `--title`: the panel title, and the CSV table's title.
    title: Option<String>,
    /// `--caption`: the panel subtitle, and the CSV table's caption.
    caption: Option<String>,
    /// `-S/--panel-style`: the panel *border* style.
    panel_style: Option<Style>,
    /// `-s/--style`: a style laid under the whole renderable (upstream's
    /// `Styled`), applied outside the panel.
    style: Option<Style>,
    /// `-e/--expand`: make `--panel`/`--padding` fill the available width
    /// instead of shrinking to their content. Implied by `--width`.
    expand: bool,
    /// `--pager`: page the output through the system pager.
    pager: bool,
    auto_pager: bool,
    /// `-y/--hyperlinks`: render a Markdown link as an OSC 8 hyperlink.
    /// Upstream's flag, and upstream's default of **false** — see
    /// [`build_markdown`], which is where it is consumed.
    hyperlinks: bool,
    /// `--sanitize`: visibly neutralize terminal controls from input and labels.
    sanitize: bool,
    /// `--report json`: emit a stable result/error envelope to stderr while
    /// leaving stdout for rendered content.
    report_format: ReportFormat,
    /// Re-render a changing local file or URL while stdout is a terminal.
    watch: bool,
    /// Polling interval for `--watch`, in seconds.
    watch_interval: f64,
    /// Avoid emitting unchanged URL responses.
    watch_cache: bool,
    /// Quiet period, in seconds, that collapses a burst of file events into
    /// one re-render (`--watch-debounce`).
    watch_debounce: f64,
    /// `--watch-poll`: skip file events and poll local files at the interval.
    watch_poll: bool,
    /// `--watch-exit-on-error`: end the watch non-zero when a render fails.
    watch_exit_on_error: bool,
    batch: bool,
    progress: bool,
    continue_on_error: bool,
    jobs: usize,
    dry_run: bool,
    batch_paths: batch_paths::BatchPathOptions,
    worker_args: Vec<String>,
    overwrite: bool,
    collision: CollisionPolicy,
    data: inspect::DataOptions,
    tools: tools::ToolOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CollisionPolicy {
    Error,
    Overwrite,
    Suffix,
}

/// Build the `-m/--markdown` renderable, honouring `-y/--hyperlinks`.
///
/// Upstream is `Markdown(markdown_data, code_theme=theme, hyperlinks=hyperlinks)`
/// with `@click.option("--hyperlinks", "-y", is_flag=True)`, so the default is
/// **false**: `rich -m` renders `[text](url)` as `text (url)`, with the URL
/// visible, and only `-y` turns it into an OSC 8 hyperlink whose target the
/// reader cannot see.
///
/// Without this, `rich -m` emitted the OSC 8 rendering unconditionally and the
/// destination appeared nowhere in the byte stream — so every URL in a document
/// was unrecoverable as soon as the output was piped or `NO_COLOR` was set.
fn build_markdown(source: &str, hyperlinks: bool) -> Markdown {
    Markdown::new(source).hyperlinks(hyperlinks)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if demo::requested(&args) {
        return demo::dispatch(&args);
    }
    if doctor::requested(&args) {
        return doctor::dispatch(&args);
    }
    match parse(&args) {
        Ok(None) => ExitCode::SUCCESS, // help/version already printed
        Ok(Some(cli)) => {
            if cli.batch {
                if let Err(error) = batch::install_interrupt_handler() {
                    return fail(
                        &cli,
                        ExitClass::Input,
                        format!("cannot install batch interruption handler: {error}"),
                    );
                }
            }
            run(cli)
        }
        Err(message) => emit_error(
            wants_json_report(&args),
            ExitClass::Usage,
            &format!("{message} (try --help)"),
        ),
    }
}

/// Options that consume the following argument as their value. Needed to tell a
/// positional subcommand from an option's value when scanning raw arguments.
const VALUE_OPTIONS: &[&str] = &[
    "--batch-input-root",
    "--batch-name-template",
    "--log-presentation",
    "-w",
    "--width",
    "-o",
    "--export-html",
    "--export-svg",
    "--panel",
    "--padding",
    "--title",
    "--caption",
    "-s",
    "--style",
    "-S",
    "--panel-style",
    "--report",
    "--image-mode",
    "--image-fit",
    "--image-anchor",
    "--image-background",
    "--height",
    "--theme",
    "--theme-style",
    "--image-color",
    "--image-dither",
    "--image-rotate",
    "--image-max-width",
    "--image-max-height",
    "--image-brightness",
    "--image-contrast",
    "--image-gamma",
    "--demo-section",
    "--demo-delay",
    "--watch-interval",
    "--interval",
    "--watch-debounce",
    "--gif-mode",
    "--encoding",
    "--threshold",
    "--loop",
    "--jobs",
    "--collision",
    "--format",
    "--select",
    "--find",
    "--max-depth",
    "--max-length",
    "--compare",
    "--context",
    "--language",
    "--config",
    "--profile",
];

/// Whether the command line already picks a render mode, by flag *or* by the
/// positional subcommand form. Config must not inject a `mode` default over
/// `rich json file.json`, which selects JSON without ever naming `--json`.
fn selects_mode_explicitly(args: &[String]) -> bool {
    explicit_mode(args).is_some()
}

/// The render mode the command line picks by flag or subcommand, if any.
fn explicit_mode(args: &[String]) -> Option<Mode> {
    let mut iter = args.iter();
    let mut seen_positional = false;
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            if let Some(flag) = mode_flag_alias(arg) {
                return command_mode(&flag[2..]);
            }
            continue;
        }
        if !seen_positional {
            seen_positional = true;
            if let Some(mode) = command_mode(arg) {
                return Some(mode);
            }
        }
    }
    None
}

/// The canonical long flag for a render-mode option, or `None` for anything else.
fn mode_flag_alias(arg: &str) -> Option<&'static str> {
    Some(match arg {
        "--print" | "-p" => "--print",
        "--markdown" | "-m" => "--markdown",
        "--json" | "-j" => "--json",
        "--syntax" | "-x" => "--syntax",
        "--csv" => "--csv",
        "--ipynb" => "--ipynb",
        "--jsonl" | "--ndjson" => "--jsonl",
        "--log" => "--log",
        "--rule" => "--rule",
        "--image" => "--image",
        "--gif" => "--gif",
        "--diff" => "--diff",
        "--inspect" => "--inspect",
        "--ansi-explain" => "--ansi-explain",
        _ => return None,
    })
}

fn emit_error(json: bool, class: ExitClass, message: &str) -> ExitCode {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({
                "ok": false,
                "code": class.name(),
                "exit_code": class.code(),
                "message": message,
                "error": {
                    "message": message,
                },
            })
        );
    } else {
        eprintln!("rich: {message}");
    }
    class.exit_code()
}

fn emit_success_report(format: ReportFormat) {
    if format == ReportFormat::Json {
        eprintln!(
            "{}",
            serde_json::json!({
                "ok": true,
                "code": ExitClass::Success.name(),
                "exit_code": ExitClass::Success.code(),
                "result": {},
            })
        );
    }
}

fn fail(cli: &Cli, class: ExitClass, message: impl AsRef<str>) -> ExitCode {
    // A watched resource's live region shows its own error.
    if watch::capture_error(message.as_ref()) {
        return class.exit_code();
    }
    emit_error(
        cli.report_format == ReportFormat::Json,
        class,
        message.as_ref(),
    )
}

fn success(cli: &Cli) -> ExitCode {
    if !watch::capturing() {
        emit_success_report(cli.report_format);
    }
    ExitClass::Success.exit_code()
}

/// Match `text` against a simple glob supporting `*` (any run of characters)
/// and `?` (exactly one). Both apply within a single path segment.
///
/// Substring containment is not a glob: `*.md` used that way also matched
/// `notes.md.bak`, and a `?` pattern matched nothing at all because the literal
/// `?` was still being searched for.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut retry) = (None, 0usize);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            retry = t;
            p += 1;
        } else if let Some(open) = star {
            p = open + 1;
            retry += 1;
            t = retry;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

fn has_glob_meta(value: &str) -> bool {
    value.contains('*') || value.contains('?')
}

/// Split `dir/sub/*.md` into its literal directory and its name pattern.
fn split_glob(resource: &str) -> (PathBuf, String) {
    let meta = resource.find(['*', '?']).unwrap_or(0);
    match resource[..meta].rfind(['/', '\\']) {
        Some(cut) => (
            PathBuf::from(&resource[..cut]),
            resource[cut + 1..].to_string(),
        ),
        None => (PathBuf::from("."), resource.to_string()),
    }
}

/// Collect every file under `dir`, skipping symlinks.
///
/// `Path::is_dir` follows links, so a directory containing a link to itself or
/// to an ancestor pushed that directory back onto the stack forever, building
/// ever longer paths until the process ran out of memory. Not following links
/// at all keeps the walk finite without having to track device/inode identity,
/// which is not portable.
fn collect_files(dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        batch::check_interrupted()?;
        let entries = std::fs::read_dir(&current)
            .map_err(|error| format!("cannot scan {}: {error}", current.display()))?;
        for entry in entries {
            batch::check_interrupted()?;
            let entry = entry.map_err(|error| format!("cannot scan {}: {error}", dir.display()))?;
            let path = entry.path();
            let kind = std::fs::symlink_metadata(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?
                .file_type();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                out.push(path.to_string_lossy().into_owned());
            }
        }
    }
    Ok(())
}

fn expand_batch_resources(resources: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for resource in resources {
        batch::check_interrupted()?;
        if resource == "-" || is_url(resource) {
            return Err("batch resources must be local files or directories".into());
        }
        if has_glob_meta(resource) {
            let (dir, pattern) = split_glob(resource);
            if pattern.contains('/') || pattern.contains('\\') {
                return Err(format!(
                    "batch glob {resource:?} may only use * and ? in the final path segment"
                ));
            }
            let entries = std::fs::read_dir(&dir)
                .map_err(|error| format!("cannot scan {}: {error}", dir.display()))?;
            for entry in entries {
                batch::check_interrupted()?;
                let entry =
                    entry.map_err(|error| format!("cannot scan {}: {error}", dir.display()))?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                if glob_match(&pattern, &entry.file_name().to_string_lossy()) {
                    out.push(path.to_string_lossy().into_owned());
                }
            }
            continue;
        }
        let path = Path::new(resource);
        if path.is_file() {
            out.push(resource.clone());
        } else if path.is_dir() {
            collect_files(path, &mut out)?;
        } else {
            return Err(format!("batch resource does not exist: {resource}"));
        }
    }
    batch::check_interrupted()?;
    out.sort();
    out.dedup();
    batch::check_interrupted()?;
    if out.is_empty() {
        return Err("batch plan contains no files".into());
    }
    Ok(out)
}

/// Where one batch item's exports are written.
///
/// The kind of each export is carried explicitly. Recovering it by sniffing the
/// path extension silently dropped exports whose name did not end in `.html` or
/// `.svg`, so `--export-html result.out` planned a path and then wrote nothing.
#[derive(Default, Clone)]
struct BatchOutputs {
    html: Option<String>,
    svg: Option<String>,
}

fn batch_export_path(
    path: Option<&str>,
    input: &str,
    index: usize,
    total: usize,
) -> Option<String> {
    let path = path?;
    let p = Path::new(path);
    if total == 1 {
        return Some(path.into());
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let stem = Path::new(input)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = if ext.is_empty() {
        format!("{stem}-{index}")
    } else {
        format!("{stem}.{ext}")
    };
    Some(if p.is_dir() {
        p.join(name).to_string_lossy().into_owned()
    } else {
        p.with_file_name(name).to_string_lossy().into_owned()
    })
}

/// `out.html` -> `out-2.html`, keeping an extension-less name intact.
fn suffixed_path(path: &Path, suffix: usize) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = match path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{stem}-{suffix}.{ext}"),
        None => format!("{stem}-{suffix}"),
    };
    path.with_file_name(name).to_string_lossy().into_owned()
}

/// Settle one planned output against the rest of the plan and against the disk.
///
/// `suffix` has to consider existing files too, not just in-plan duplicates:
/// a policy whose whole purpose is to avoid clobbering must not quietly
/// overwrite a file that was already there.
fn resolve_destination(
    cli: &Cli,
    candidate: String,
    taken: &mut std::collections::BTreeSet<String>,
) -> Result<String, (ExitClass, String)> {
    let occupied = |path: &str, taken: &std::collections::BTreeSet<String>| {
        taken.contains(path) || (!cli.overwrite && Path::new(path).exists())
    };
    match cli.collision {
        CollisionPolicy::Overwrite => {
            taken.insert(candidate.clone());
            Ok(candidate)
        }
        CollisionPolicy::Suffix => {
            let mut chosen = candidate.clone();
            let mut suffix = 2;
            while occupied(&chosen, taken) {
                batch::check_interrupted().map_err(|message| (ExitClass::Input, message))?;
                chosen = suffixed_path(Path::new(&candidate), suffix);
                suffix += 1;
            }
            taken.insert(chosen.clone());
            Ok(chosen)
        }
        CollisionPolicy::Error => {
            if taken.contains(&candidate) {
                return Err((
                    ExitClass::Usage,
                    format!("batch output collision: {candidate}"),
                ));
            }
            if !cli.overwrite && Path::new(&candidate).exists() {
                return Err((
                    ExitClass::Input,
                    format!("refusing to overwrite existing output: {candidate} (use --overwrite)"),
                ));
            }
            taken.insert(candidate.clone());
            Ok(candidate)
        }
    }
}

/// Run one batch item with its diagnostics captured, yielding its failure class.
/// Reuse resolved rendering flags without re-reading config in batch workers.
fn worker_args(args: &[String]) -> Vec<String> {
    let mut result = vec!["--no-config".into()];
    let mut iter = args.iter();
    let mut seen_resource = false;
    let mut selected_mode = false;
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            let value = iter.next();
            if !matches!(
                arg.as_str(),
                "--jobs"
                    | "--batch-input-root"
                    | "--batch-name-template"
                    | "--collision"
                    | "--report"
                    | "-o"
                    | "--export-html"
                    | "--export-svg"
                    | "--config"
                    | "--profile"
            ) {
                result.push(arg.clone());
                if let Some(value) = value {
                    result.push(value.clone());
                }
            }
        } else if arg.starts_with('-') && arg != "-" {
            selected_mode |= mode_flag_alias(arg).is_some();
            if !matches!(
                arg.as_str(),
                "--batch"
                    | "--batch-preserve-dirs"
                    | "--no-batch-preserve-dirs"
                    | "--no-batch"
                    | "--dry-run"
                    | "--progress"
                    | "--no-progress"
                    | "--continue-on-error"
                    | "--no-continue-on-error"
                    | "--overwrite"
                    | "--no-overwrite"
                    | "--machine-json"
            ) {
                result.push(arg.clone());
            }
        } else if !seen_resource {
            if let Some(mode) = command_mode(arg).filter(|_| !selected_mode) {
                selected_mode = true;
                result.push(format!("--{}", mode_name(mode)));
            } else {
                seen_resource = true;
            }
        }
    }
    result
}

fn wants_json_report(args: &[String]) -> bool {
    let mut format = ReportFormat::Human;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--report" => {
                if let Some(value) = iter.next() {
                    let _ = format.apply_option("--report", Some(value));
                }
            }
            "--machine-json" => {
                let _ = format.apply_option("--machine-json", None);
            }
            "--" => break,
            _ => {}
        }
    }
    format == ReportFormat::Json
}

/// Map a `--panel` box name to a box set (port of rich-cli's `BOXES` +
/// `getattr(box, name.upper())`).
///
/// `none` is upstream's *default* for `--panel`, and upstream guards the whole
/// panel step with `if panel != "none"` — so it means "no panel", not "a panel
/// drawn with `box.NONE`". Drawing box.NONE put an invisible one-cell frame and
/// a blank line around the output, which reads as the tool having mangled the
/// file for no reason.
fn parse_box(name: &str) -> Result<Option<BoxSet>, String> {
    Ok(Some(match name.to_ascii_lowercase().as_str() {
        "none" => return Ok(None),
        "ascii" => ASCII,
        "ascii2" => ASCII2,
        "square" => SQUARE,
        "rounded" => ROUNDED,
        "heavy" => HEAVY,
        "double" => DOUBLE,
        other => {
            return Err(format!(
                "unknown panel box {other:?} (use none/ascii/ascii2/square/rounded/heavy/double)"
            ))
        }
    }))
}

/// Parse a `--padding` value: 1, 2, or 4 comma-separated integers, unpacked into
/// `(top, right, bottom, left)`. Port of rich-cli's padding parsing + upstream's
/// `Padding.unpack`.
fn parse_padding(value: &str) -> Result<(usize, usize, usize, usize), String> {
    let error = || "padding should be 1, 2, or 4 integers separated by commas".to_string();
    let parts: Result<Vec<usize>, _> = value
        .split(',')
        .map(|p| p.trim().parse::<usize>())
        .collect();
    let parts = parts.map_err(|_| error())?;
    match parts.as_slice() {
        [p] => Ok((*p, *p, *p, *p)),
        [v, h] => Ok((*v, *h, *v, *h)),
        [t, r, b, l] => Ok((*t, *r, *b, *l)),
        _ => Err(error()),
    }
}

/// Set the render mode, rejecting a second, conflicting mode flag.
fn mode_name(mode: Mode) -> &'static str {
    MODE_SPECS
        .iter()
        .find_map(|spec| (spec.mode == mode).then_some(spec.primary))
        .unwrap_or("unknown")
}

fn command_mode(command: &str) -> Option<Mode> {
    MODE_SPECS
        .iter()
        .find_map(|spec| spec.aliases.contains(&command).then_some(spec.mode))
}

fn set_mode(current: &mut Mode, mode: Mode) -> Result<(), String> {
    if *current != Mode::Auto && *current != mode {
        return Err(format!(
            "only one render mode ({RENDER_MODE_FLAGS}) may be given"
        ));
    }
    *current = mode;
    Ok(())
}

/// Parse args into a [`Cli`], or `Ok(None)` when `--help`/`--version` handled it.
fn parse(args: &[String]) -> Result<Option<Cli>, String> {
    let roots = ConfigRoots::default();
    if authoring::dispatch(args)? {
        return Ok(None);
    }
    if tools::bench_dispatch(args)? {
        return Ok(None);
    }
    if let Some(output) = config::inspect(args, &roots)? {
        println!("{output}");
        return Ok(None);
    }
    let merged = config_args(args, &roots)?;
    parse_inner(&merged)
}

fn parse_inner(args: &[String]) -> Result<Option<Cli>, String> {
    let mut mode = Mode::Auto;
    let mut resources: Vec<String> = Vec::new();
    let mut loops = None;
    let mut diff_threshold = None;
    let mut image_mode = ImageMode::Auto;
    let mut image_fit = None;
    let mut image_anchor = None;
    let mut image_background = None;
    let mut image_color = None;
    let mut image_dither = None;
    let mut image_rotate = None;
    let mut image_flip_horizontal = false;
    let mut image_flip_vertical = false;
    let mut image_grayscale = false;
    let mut image_max_width = None;
    let mut image_max_height = None;
    let mut image_brightness = None;
    let mut image_contrast = None;
    let mut image_gamma = None;
    let mut log_presentation = String::from("plain");
    let mut theme_styles = std::collections::BTreeMap::new();
    let mut height = None;
    let mut extensions = CliExtensions::default();
    let mut data = inspect::DataOptions::default();
    let mut tool_options = tools::ToolOptions::default();
    // `rich ansi explain FILE`: the second word belongs to the command.
    let mut ansi_word = false;
    let mut width = None;
    let mut justify = None;
    let mut no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    let mut export_html: Option<String> = None;
    let mut export_svg: Option<String> = None;
    let mut panel = None;
    let mut padding = None;
    let mut title = None;
    let mut caption = None;
    let mut panel_style = None;
    let mut style = None;
    let mut expand = false;
    let mut pager = false;
    let mut auto_pager = false;
    let mut hyperlinks = false;
    let mut sanitize = false;
    let mut report_format = ReportFormat::Human;
    let mut watch = false;
    let mut watch_interval = 1.0;
    let mut watch_cache = false;
    let mut watch_debounce: Option<f64> = None;
    let mut watch_poll = false;
    let mut watch_exit_on_error = false;
    let mut batch = false;
    let mut progress = true;
    let mut continue_on_error = false;
    let mut jobs = 1usize;
    let mut dry_run = false;
    let mut batch_paths = batch_paths::BatchPathOptions::default();
    let mut overwrite = false;
    let mut collision = CollisionPolicy::Error;
    // Set by `--`: everything after it is a positional argument, however much it
    // looks like a flag. Without this nothing beginning with `-` could be
    // printed or opened at all — `rich -p -- "-5 degrees"` and `rich -- -weird.md`
    // both died with "unknown option".
    let mut end_of_options = false;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if end_of_options {
            resources.push(arg.to_string());
            continue;
        }
        if extensions.parse_option(arg, &mut iter)? {
            continue;
        }
        if data.parse_option(arg, &mut iter)? {
            continue;
        }
        if tool_options.parse_option(arg, &mut iter)? {
            continue;
        }
        match arg.as_str() {
            "--" => end_of_options = true,
            "-h" | "--help" => {
                print_help(no_color || cli_spec::no_color_requested(args));
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("rich (rs-rich-cli) {VERSION}");
                return Ok(None);
            }
            "-p" | "--print" => set_mode(&mut mode, Mode::Print)?,
            "-m" | "--markdown" => set_mode(&mut mode, Mode::Markdown)?,
            "-j" | "--json" => set_mode(&mut mode, Mode::Json)?,
            "-x" | "--syntax" => set_mode(&mut mode, Mode::Syntax)?,
            "--csv" => set_mode(&mut mode, Mode::Csv)?,
            "--ipynb" => set_mode(&mut mode, Mode::Ipynb)?,
            "--gif" => set_mode(&mut mode, Mode::Gif)?,
            "--diff" => set_mode(&mut mode, Mode::Diff)?,
            "--image" => set_mode(&mut mode, Mode::Image)?,
            "--jsonl" | "--ndjson" => set_mode(&mut mode, Mode::JsonLines)?,
            "--log" => set_mode(&mut mode, Mode::Log)?,
            "--inspect" => set_mode(&mut mode, Mode::Inspect)?,
            "--ansi-explain" => set_mode(&mut mode, Mode::AnsiExplain)?,
            "--log-presentation" => {
                let value = iter
                    .next()
                    .ok_or("--log-presentation requires plain or rich")?;
                if !matches!(value.as_str(), "plain" | "rich") {
                    return Err("--log-presentation requires plain or rich".into());
                }
                log_presentation = value.clone();
            }
            "--report" => {
                let value = iter.next().map(String::as_str);
                report_format.apply_option("--report", value)?;
            }
            "--machine-json" => {
                report_format.apply_option("--machine-json", None)?;
            }
            "--progress" => progress = true,
            "--no-progress" => progress = false,
            "--theme-style" => {
                let binding = iter.next().ok_or("--theme-style requires NAME=STYLE")?;
                let (name, value) = binding
                    .split_once('=')
                    .ok_or("--theme-style requires NAME=STYLE")?;
                config::validate_theme_binding(name, value)?;
                let style = Style::parse(value)
                    .map_err(|error| format!("invalid theme style {name}: {error}"))?;
                theme_styles.insert(name.to_owned(), style);
            }
            "--image-color" => {
                const USAGE: &str =
                    "--image-color requires truecolor, ansi256, ansi16 or grayscale";
                let value = iter.next().ok_or(USAGE)?;
                if !matches!(
                    value.as_str(),
                    "truecolor" | "ansi256" | "ansi16" | "grayscale"
                ) {
                    return Err(USAGE.into());
                }
                image_color = Some(value.clone());
            }
            flag @ ("--image-max-width" | "--image-max-height") => {
                let usage = format!("{flag} requires a positive integer");
                let value = iter
                    .next()
                    .and_then(|v| v.parse::<usize>().ok())
                    .filter(|v| *v > 0)
                    .ok_or(usage)?;
                if flag == "--image-max-width" {
                    image_max_width = Some(value);
                } else {
                    image_max_height = Some(value);
                }
            }
            flag @ ("--image-brightness" | "--image-contrast" | "--image-gamma") => {
                let gamma = flag == "--image-gamma";
                let usage = if gamma {
                    format!("{flag} requires a finite number greater than 0 (1.0 = unchanged)")
                } else {
                    format!("{flag} requires a finite number of at least 0 (1.0 = unchanged)")
                };
                let value = iter
                    .next()
                    .and_then(|v| v.parse::<f32>().ok())
                    .filter(|v| v.is_finite() && (*v > 0.0 || (!gamma && *v == 0.0)))
                    .ok_or(usage)?;
                match flag {
                    "--image-brightness" => image_brightness = Some(value),
                    "--image-contrast" => image_contrast = Some(value),
                    _ => image_gamma = Some(value),
                }
            }
            "--image-flip-horizontal" => image_flip_horizontal = true,
            "--no-image-flip-horizontal" => image_flip_horizontal = false,
            "--image-flip-vertical" => image_flip_vertical = true,
            "--no-image-flip-vertical" => image_flip_vertical = false,
            "--image-grayscale" => image_grayscale = true,
            "--no-image-grayscale" => image_grayscale = false,
            "--image-rotate" => {
                let value = iter
                    .next()
                    .ok_or("--image-rotate requires 0, 90, 180 or 270")?;
                image_rotate = Some(match value.as_str() {
                    "0" => 0,
                    "90" => 90,
                    "180" => 180,
                    "270" => 270,
                    _ => return Err("--image-rotate requires 0, 90, 180 or 270".into()),
                });
            }
            "--image-dither" => {
                let value = iter
                    .next()
                    .ok_or("--image-dither requires none, floyd-steinberg or bayer4x4")?;
                if !matches!(value.as_str(), "none" | "floyd-steinberg" | "bayer4x4") {
                    return Err("--image-dither requires none, floyd-steinberg or bayer4x4".into());
                }
                image_dither = Some(value.clone());
            }
            "--image-mode" => {
                let value = iter.next().ok_or(
                    "--image-mode requires one of: auto, sixel, blocks, quadrants, braille, ascii, none",
                )?;
                image_mode = value.parse()?;
            }
            "--image-fit" => {
                let value = iter
                    .next()
                    .ok_or("--image-fit requires contain, cover or stretch")?;
                if !matches!(value.as_str(), "contain" | "cover" | "stretch") {
                    return Err("--image-fit requires contain, cover or stretch".into());
                }
                image_fit = Some(value.clone());
            }
            "--image-anchor" => {
                let value = iter.next().ok_or("--image-anchor requires a position")?;
                if !matches!(
                    value.as_str(),
                    "center"
                        | "top"
                        | "bottom"
                        | "left"
                        | "right"
                        | "top-left"
                        | "top-right"
                        | "bottom-left"
                        | "bottom-right"
                ) {
                    return Err("--image-anchor requires center/top/bottom/left/right/top-left/top-right/bottom-left/bottom-right".into());
                }
                image_anchor = Some(value.clone());
            }
            "--image-background" => {
                let value = iter.next().ok_or("--image-background requires #RRGGBB")?;
                let bytes = value.as_bytes();
                if bytes.len() != 7
                    || bytes[0] != b'#'
                    || !bytes[1..].iter().all(u8::is_ascii_hexdigit)
                {
                    return Err("--image-background requires #RRGGBB".into());
                }
                let mut rgb = [0; 3];
                for (index, channel) in rgb.iter_mut().enumerate() {
                    *channel = u8::from_str_radix(&value[1 + index * 2..3 + index * 2], 16)
                        .map_err(|_| "--image-background requires #RRGGBB")?;
                }
                image_background = Some(rgb);
            }
            "--height" => {
                let value = iter.next().ok_or("--height requires a number")?;
                let rows: usize = value
                    .parse()
                    .map_err(|_| format!("invalid height '{value}'"))?;
                if rows == 0 {
                    return Err("--height must be at least 1".into());
                }
                height = Some(rows);
            }
            "--threshold" => {
                let value = iter
                    .next()
                    .ok_or("--threshold requires a percentage, e.g. --threshold 2")?;
                let percent: f32 = value
                    .parse()
                    .map_err(|_| format!("invalid threshold '{value}'"))?;
                // `"NaN"` and `"inf"` both parse as f32, and every comparison
                // against NaN is false — so an empty template variable or a bad
                // substitution would turn the gate off and report a pass. A gate
                // that can be silently disabled is worse than no gate.
                if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                    return Err(format!(
                        "threshold must be a percentage between 0 and 100, got '{value}'"
                    ));
                }
                diff_threshold = Some(percent);
            }
            "--loop" => {
                let value = iter.next().ok_or("--loop requires a count (0 = forever)")?;
                loops = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid loop count {value:?}"))?,
                );
            }
            "--rule" => set_mode(&mut mode, Mode::Rule)?,
            "--left" => justify = Some(Justify::Left),
            "--right" if justify != Some(Justify::Left) => justify = Some(Justify::Right),
            "--right" => {}
            "--center" if justify.is_none() => justify = Some(Justify::Center),
            "--center" => {}
            "--no-color" => no_color = true,
            "--pager" => {
                pager = true;
                auto_pager = false;
            }
            "--auto-pager" => {
                auto_pager = true;
                pager = false;
            }
            "--no-pager" => {
                pager = false;
                auto_pager = false;
            }
            "--no-auto-pager" => auto_pager = false,
            "--color" => no_color = false,
            "--no-watch" => watch = false,
            "--no-watch-cache" => watch_cache = false,
            "--no-watch-poll" => watch_poll = false,
            "--no-watch-exit-on-error" => watch_exit_on_error = false,
            "--no-batch" => batch = false,
            "--no-continue-on-error" => continue_on_error = false,
            "--no-overwrite" => {
                overwrite = false;
                collision = CollisionPolicy::Error;
            }
            "--no-sanitize" => sanitize = false,
            "--dry-run" => dry_run = true,
            "--batch-preserve-dirs" => batch_paths.preserve_dirs = true,
            "--no-batch-preserve-dirs" => batch_paths.preserve_dirs = false,
            "--batch-input-root" => {
                batch_paths.input_root = Some(PathBuf::from(
                    iter.next().ok_or("--batch-input-root requires a path")?,
                ))
            }
            "--batch-name-template" => {
                batch_paths.template = Some(
                    batch_paths::FilenameTemplate::parse(
                        iter.next()
                            .ok_or("--batch-name-template requires a template")?,
                    )
                    .map_err(|e| e.to_string())?,
                )
            }
            "--watch" => watch = true,
            "--watch-cache" => watch_cache = true,
            "--watch-poll" => watch_poll = true,
            "--watch-exit-on-error" => watch_exit_on_error = true,
            "--watch-debounce" => {
                let value = iter
                    .next()
                    .ok_or("--watch-debounce requires seconds (0 or more)")?;
                let seconds = value
                    .parse::<f64>()
                    .map_err(|_| format!("invalid watch debounce {value:?}"))?;
                if !seconds.is_finite() || !(0.0..=3600.0).contains(&seconds) {
                    return Err("--watch-debounce must be between 0 and 3600 seconds".into());
                }
                watch_debounce = Some(seconds);
            }
            "--watch-interval" | "--interval" => {
                let value = iter
                    .next()
                    .ok_or("--watch-interval requires seconds greater than zero")?;
                watch_interval = value
                    .parse::<f64>()
                    .map_err(|_| format!("invalid watch interval {value:?}"))?;
                if !watch_interval.is_finite() || watch_interval <= 0.0 {
                    return Err("--watch-interval must be greater than zero".into());
                }
            }
            "--batch" => batch = true,
            "--continue-on-error" => continue_on_error = true,
            "--overwrite" => {
                overwrite = true;
                collision = CollisionPolicy::Overwrite;
            }
            "--collision" => {
                collision = match iter
                    .next()
                    .ok_or("--collision requires error, overwrite, or suffix")?
                    .as_str()
                {
                    "error" => CollisionPolicy::Error,
                    "overwrite" => CollisionPolicy::Overwrite,
                    "suffix" => CollisionPolicy::Suffix,
                    other => return Err(format!("unknown collision policy {other:?}")),
                };
                overwrite = collision == CollisionPolicy::Overwrite;
            }
            "--jobs" => {
                jobs = iter
                    .next()
                    .ok_or("--jobs requires a positive number")?
                    .parse()
                    .map_err(|_| "--jobs requires a positive number".to_string())?;
                if jobs == 0 {
                    return Err("--jobs must be at least 1".into());
                }
            }
            "--sanitize" => sanitize = true,
            // Upstream's `@click.option("--hyperlinks", "-y", is_flag=True,
            // help="Render hyperlinks in markdown.")`. Accepted in every mode,
            // as click accepts it — it is read only where markdown is rendered.
            "-y" | "--hyperlinks" => hyperlinks = true,
            // Both take a PATH and both may be given at once, matching
            // upstream: the resource still renders to the terminal and the
            // files are written alongside it.
            "--export-html" | "-o" => {
                export_html = Some(iter.next().ok_or("--export-html requires a PATH")?.clone());
            }
            "--export-svg" => {
                export_svg = Some(iter.next().ok_or("--export-svg requires a PATH")?.clone());
            }
            "--panel" => {
                let value = iter.next().ok_or("--panel requires a box name")?;
                panel = parse_box(value)?;
            }
            "--padding" => {
                let value = iter.next().ok_or("--padding requires a value")?;
                padding = Some(parse_padding(value)?);
            }
            "-e" | "--expand" => expand = true,
            "--title" => {
                title = Some(iter.next().ok_or("--title requires a value")?.clone());
            }
            "--caption" => {
                caption = Some(iter.next().ok_or("--caption requires a value")?.clone());
            }
            // `-s/--style` is the style of the *renderable* (upstream wraps it in
            // `Styled`); `-S/--panel-style` is the border. They were fused into
            // one flag that set the border, so there was no way to style the
            // content at all and `--style` aborted without `--panel`.
            "-s" | "--style" => {
                let value = iter.next().ok_or("--style requires a value")?;
                style = Some(Style::parse(value).map_err(|e| format!("invalid --style: {e}"))?);
            }
            "-S" | "--panel-style" => {
                let value = iter.next().ok_or("--panel-style requires a value")?;
                panel_style =
                    Some(Style::parse(value).map_err(|e| format!("invalid --panel-style: {e}"))?);
            }
            "-w" | "--width" => {
                let value = iter.next().ok_or("--width requires a number")?;
                let columns: usize = value.parse().map_err(|_| {
                    // The common cause is a missing value, in which case this
                    // has just eaten the filename — say so rather than quoting
                    // the path back with debug escaping.
                    if std::path::Path::new(value).exists() {
                        format!("--width needs a number, but got the file '{value}' — a value is missing")
                    } else {
                        format!("invalid width '{value}'")
                    }
                })?;
                if columns == 0 {
                    return Err("--width must be at least 1".into());
                }
                width = Some(columns);
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other:?}"));
            }
            other
                if mode == Mode::Auto && resources.is_empty() && command_mode(other).is_some() =>
            {
                ansi_word = other == "ansi";
                set_mode(&mut mode, command_mode(other).expect("checked above"))?;
            }
            "explain" if ansi_word && resources.is_empty() => ansi_word = false,
            other => resources.push(other.to_string()),
        }
    }

    // `--format` makes automatic mode an input mode: with no RESOURCE it reads
    // stdin, as `--json` does, instead of showing the demo.
    if data.format.is_some() {
        if !matches!(mode, Mode::Auto | Mode::Inspect) {
            return Err(format!(
                "--format cannot be combined with --{}",
                mode_name(mode)
            ));
        }
        if mode == Mode::Auto && resources.is_empty() && !batch {
            resources.push("-".into());
        }
    }
    if let Some(flag) = data.inspect_only_option().filter(|_| mode != Mode::Inspect) {
        return Err(format!("{flag} only has an effect with --inspect"));
    }
    if let Some(flag) = tool_options.diff_option().filter(|_| mode != Mode::Diff) {
        return Err(format!("{flag} only has an effect with --diff"));
    }
    if let Some(flag) = tool_options
        .ansi_option()
        .filter(|_| mode != Mode::AnsiExplain)
    {
        return Err(format!("{flag} only has an effect with --ansi-explain"));
    }
    data.validate()?;

    if watch && (batch || pager || auto_pager) {
        return Err("--watch cannot be combined with --batch or paging".into());
    }

    // `--gif`/`--diff` consume their whole resource list as one unit (a diff is
    // a pair), but a batch rewrites each item to a single resource — `run_diff`
    // would then index `resources[1]` and panic. Reject the combination.
    if batch {
        if mode.accepts_multiple_resources() {
            return Err(format!(
                "--batch cannot be combined with {}",
                if mode == Mode::Diff {
                    "--diff"
                } else {
                    "--gif"
                }
            ));
        }
        if resources.is_empty() {
            return Err("--batch needs at least one file, directory or glob".into());
        }
        // One pager per item would be N interactive pagers over one plan.
        if pager || auto_pager {
            return Err("--pager cannot be combined with --batch".into());
        }
    }

    // Only `--gif` animates several resources at once; every other mode renders
    // exactly one.
    if mode == Mode::Diff && !(1..=2).contains(&resources.len()) {
        return Err(
            "--diff needs two images or files to compare, or one patch: --diff before after".into(),
        );
    }
    if mode == Mode::Image {
        if resources.is_empty() {
            return Err(
                "--image requires an image file: rich --image photo.png (or `-` for stdin)".into(),
            );
        }
        if image_mode == ImageMode::None {
            return Err(
                "--image-mode none has no effect with --image; use auto, ascii, blocks, or sixel"
                    .into(),
            );
        }
    }
    let quantized = matches!(
        image_color.as_deref(),
        Some("ansi256" | "ansi16" | "grayscale")
    );
    if (quantized
        || matches!(
            image_dither.as_deref(),
            Some("floyd-steinberg" | "bayer4x4")
        ))
        && !matches!(
            image_mode,
            ImageMode::Auto | ImageMode::Ascii | ImageMode::Blocks | ImageMode::Quadrants
        )
    {
        return Err(
            "image color processing supports only --image-mode ascii, blocks or quadrants".into(),
        );
    }
    if matches!(
        image_dither.as_deref(),
        Some("floyd-steinberg" | "bayer4x4")
    ) && !quantized
    {
        return Err(format!(
            "--image-dither {} requires --image-color ansi256, ansi16 or grayscale",
            image_dither.as_deref().unwrap()
        ));
    }
    if image_anchor.is_some() && image_fit.as_deref() != Some("cover") {
        return Err("--image-anchor requires --image-fit cover".into());
    }
    if (batch_paths.preserve_dirs
        || batch_paths.template.is_some()
        || batch_paths.input_root.is_some())
        && !batch
    {
        return Err("batch path options require --batch".into());
    }
    if batch_paths.preserve_dirs && batch_paths.input_root.is_none() {
        return Err("--batch-preserve-dirs requires --batch-input-root".into());
    }
    if dry_run && !batch {
        return Err("--dry-run requires --batch".into());
    }
    if jobs > 1 && (!batch || (export_html.is_none() && export_svg.is_none())) {
        return Err("--jobs greater than 1 requires --batch with a file export".into());
    }
    if image_fit.is_some() && height.is_none() {
        return Err("--image-fit requires --height to define the target rectangle".into());
    }
    // `--gif` animates in place and the demo writes its own console, so neither
    // goes through the export path: both accepted -o/--export-svg, wrote no
    // file, and exited 0. Everywhere else a bad export path is a hard error, so
    // silence here reads as success.
    let effective_mode = if mode == Mode::Auto {
        detect_mode(resources.first().map(String::as_str))
    } else {
        mode
    };
    extensions.validate(
        effective_mode == Mode::Gif,
        effective_mode.allows_extensions()
            && !(mode == Mode::Auto && resources.is_empty())
            && !(mode == Mode::Print && resources.first().is_some_and(|r| r != "-" && !is_url(r))),
    )?;
    let exporting = export_html.is_some() || export_svg.is_some();
    if exporting {
        let unsupported = if effective_mode == Mode::Gif {
            Some("--gif")
        } else if mode == Mode::Auto && resources.is_empty() {
            Some("the capability demo")
        } else {
            None
        };
        if let Some(what) = unsupported {
            return Err(format!("--export-html/--export-svg cannot capture {what}"));
        }
    }

    // A flag whose mode is absent is almost always a mistake, and silence is the
    // dangerous response: `--threshold` without `--diff` used to render the file
    // and exit 0, so a CI job that lost its `--diff` — a typo, a refactor, an
    // argument reordered — became a permanently green gate. That is the same
    // failure the NaN check closed, reached from the other side.
    let orphans = [
        (
            "--threshold",
            diff_threshold.is_some(),
            "--diff",
            mode == Mode::Diff,
        ),
        (
            "--image-mode",
            image_mode != ImageMode::Auto,
            "--diff/--image",
            mode == Mode::Diff || mode == Mode::Image,
        ),
        ("--height", height.is_some(), "--image", mode == Mode::Image),
        (
            "--image-rotate",
            image_rotate.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-flip-horizontal",
            image_flip_horizontal,
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-flip-vertical",
            image_flip_vertical,
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-grayscale",
            image_grayscale,
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-color",
            image_color.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-max-width",
            image_max_width.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-max-height",
            image_max_height.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-brightness",
            image_brightness.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-contrast",
            image_contrast.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-gamma",
            image_gamma.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-dither",
            image_dither.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-fit",
            image_fit.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--image-background",
            image_background.is_some(),
            "--image",
            mode == Mode::Image,
        ),
        (
            "--loop",
            loops.is_some(),
            "--gif",
            effective_mode == Mode::Gif,
        ),
        // `--title`/`--caption` are deliberately NOT here: upstream feeds them to
        // the CSV table as well as to the panel, so requiring `--panel` made
        // `rich --csv sales.csv --title Sales` — a documented upstream use —
        // impossible. `-S`/`-e` really do nothing without a panel.
        (
            "--panel-style",
            panel_style.is_some(),
            "--panel",
            panel.is_some(),
        ),
        (
            "--expand",
            expand,
            "--panel/--padding",
            panel.is_some() || padding.is_some(),
        ),
        ("--watch-interval", watch_interval != 1.0, "--watch", watch),
        ("--watch-cache", watch_cache, "--watch", watch),
        (
            "--watch-debounce",
            watch_debounce.is_some(),
            "--watch",
            watch,
        ),
        ("--watch-poll", watch_poll, "--watch", watch),
        (
            "--watch-exit-on-error",
            watch_exit_on_error,
            "--watch",
            watch,
        ),
    ];
    if let Some((flag, _, needs, _)) = orphans
        .iter()
        .find(|(_, given, _, mode_present)| *given && !*mode_present)
    {
        return Err(format!("{flag} only has an effect with {needs}"));
    }

    // These decorate a single rendered resource; --diff composes its own report
    // and quietly dropped them, which reads as the flag having no effect.
    let demo = mode == Mode::Auto && resources.is_empty();
    if effective_mode.draws_directly() || demo {
        let mode_name = if effective_mode == Mode::Gif {
            "--gif"
        } else if effective_mode == Mode::Image {
            "--image"
        } else if effective_mode.streams_records() {
            mode_name(effective_mode)
        } else if demo {
            "the capability demo"
        } else {
            "--diff"
        };
        let unsupported = [
            (
                "--pager",
                (pager || auto_pager)
                    && (effective_mode == Mode::Gif || effective_mode.streams_records() || demo),
            ),
            (
                "--export-html/--export-svg",
                (export_html.is_some() || export_svg.is_some()) && effective_mode.streams_records(),
            ),
            ("--width", demo && width.is_some()),
            ("--theme-style", demo && !theme_styles.is_empty()),
            (
                "--hyperlinks",
                hyperlinks && (effective_mode.streams_records() || demo),
            ),
            ("--panel", panel.is_some()),
            ("--padding", padding.is_some()),
            ("--title", title.is_some()),
            ("--caption", caption.is_some()),
            ("--style", style.is_some()),
            ("--panel-style", panel_style.is_some()),
            ("--expand", expand),
            ("--left/--center/--right", justify.is_some()),
        ];
        if let Some((flag, _)) = unsupported.iter().find(|(_, given)| *given) {
            return Err(format!("{flag} cannot be combined with {mode_name}"));
        }
    }
    // `--watch` may follow several local files, one live region each; the
    // watcher itself checks that every one of them is a watchable file.
    if !mode.accepts_multiple_resources() && resources.len() > 1 && !batch && !watch {
        return Err("only one resource may be given (except with --gif or --watch)".into());
    }
    let resource = resources.first().cloned();

    Ok(Some(Cli {
        mode,
        resource,
        resources,
        loops,
        diff_threshold,
        image_mode,
        image_fit,
        image_anchor,
        image_background,
        image_color,
        image_dither,
        image_rotate,
        image_flip_horizontal,
        image_flip_vertical,
        image_grayscale,
        image_max_width,
        image_max_height,
        image_brightness,
        image_contrast,
        image_gamma,
        log_presentation,
        theme_styles,
        height,
        extensions,
        width,
        justify,
        no_color,
        export_html,
        export_svg,
        panel,
        padding,
        title,
        caption,
        panel_style,
        style,
        expand,
        pager,
        auto_pager,
        hyperlinks,
        sanitize,
        report_format,
        watch,
        watch_interval,
        watch_cache,
        watch_debounce: watch_debounce.unwrap_or(watch::DEFAULT_DEBOUNCE_SECONDS),
        watch_poll,
        watch_exit_on_error,
        batch,
        progress,
        continue_on_error,
        jobs,
        dry_run,
        batch_paths,
        worker_args: worker_args(args),
        overwrite,
        collision,
        data,
        tools: tool_options,
    }))
}

/// Read a resource: `-` (or `None`) means stdin, otherwise a file path.
///
/// A **file** is decoded with replacement, not rejected: upstream opens it as
/// `open(path, "rt", encoding="utf8", errors="replace")`, so a cp1252 export —
/// what Excel writes on a Windows box by default — renders with `�` where the
/// odd byte was. We refused the whole file with exit 1, which is the one
/// outcome that makes the tool useless for exactly the files people reach for
/// it with.
///
/// **Stdin stays strict.** Upstream reads it with `sys.stdin.read()`, whose
/// decode error escapes into rich-cli's `except Exception` and exits non-zero;
/// there is no `errors="replace"` on that path.
fn read_resource(resource: Option<&str>, encoding: Option<Encoding>) -> std::io::Result<String> {
    let content = match resource {
        Some(path) if path != "-" => {
            let file = std::path::Path::new(path);
            // A directory reaches the reader as "Access is denied" on Windows,
            // which sends the reader hunting for a permissions problem.
            if file.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "is a directory, not a file",
                ));
            }
            let bytes = std::fs::read(file)?;
            if let Some(encoding) = encoding {
                return encoding.decode(&bytes).map(normalize_newlines);
            }
            if has_utf16_bom(&bytes) {
                eprintln!("rich: {path} has a UTF-16 BOM; use --encoding utf-16 to decode it (default remains UTF-8 replacement)");
            }
            match String::from_utf8(bytes) {
                Ok(text) => text,
                // An image is not text in any encoding, and lossily decoding one
                // fills the terminal with thousands of replacement characters
                // and no clue. Keep the diagnostic for those; decode everything
                // else, which is what `errors="replace"` is actually for.
                Err(_) if looks_like_image(path) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "looks like an image; use `rich --image <path>` to view it, or \
                         `rich --diff <before> <after>` to compare two images",
                    ))
                }
                Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
            }
        }
        _ => {
            if std::io::stdin().is_terminal() {
                let eof = if cfg!(windows) {
                    "Ctrl-Z then Enter"
                } else {
                    "Ctrl-D"
                };
                eprintln!("rich: reading stdin; finish input with {eof}");
            }
            let mut buffer = Vec::new();
            std::io::stdin().read_to_end(&mut buffer)?;
            decode_strict(buffer, encoding)?
        }
    };
    Ok(normalize_newlines(if encoding.is_some() {
        content
    } else {
        strip_bom(content)
    }))
}

fn sanitize_json_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => *text = sanitize_terminal_controls(text),
        serde_json::Value::Array(values) => values.iter_mut().for_each(sanitize_json_value),
        serde_json::Value::Object(values) => values.values_mut().for_each(sanitize_json_value),
        _ => {}
    }
}

fn sanitize_json_source(content: &str) -> Result<String, serde_json::Error> {
    let mut value = serde_json::from_str::<serde_json::Value>(content)?;
    sanitize_json_value(&mut value);
    serde_json::to_string(&value)
}

/// Strict stdin/URL input; the default file path separately retains replacement.
fn decode_strict(bytes: Vec<u8>, encoding: Option<Encoding>) -> std::io::Result<String> {
    if let Some(encoding) = encoding {
        return encoding.decode(&bytes);
    }
    String::from_utf8(bytes).map_err(|err| std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        if has_utf16_bom(err.as_bytes()) {
            "input has a UTF-16 BOM; use --encoding utf-16"
        } else {
            "input is not valid UTF-8 text; for known headerless UTF-16 select --encoding utf-16le or utf-16be"
        },
    ))
}

/// Translate CRLF and lone CR to LF, as Python's universal newlines do.
///
/// Upstream reads text files in universal-newline mode, so it **never sees a
/// CR**; `std::fs::read_to_string` hands them straight through. That gap made
/// `--syntax` on a CRLF file render as a blank rectangle: each line was emitted
/// as `code` + CR + padding, so the CR returned the cursor to column 0 and the
/// padding overwrote the code. Exit 0, nothing visible, and piping hid it
/// because the bytes were all present — only a terminal acts on the CR.
fn normalize_newlines(text: String) -> String {
    const CR: char = '\r';
    const LF: char = '\n';
    if !text.contains(CR) {
        return text;
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == CR {
            // CRLF and a lone CR both collapse to a single LF, matching
            // Python's universal-newline translation.
            if chars.peek() == Some(&LF) {
                chars.next();
            }
            out.push(LF);
        } else {
            out.push(c);
        }
    }
    out
}

/// Drop a leading UTF-8 byte-order mark.
///
/// Windows editors write one by default and it is invisible: it made valid JSON
/// fail to parse at "column 1", and Markdown render its first heading as
/// literal text at exit 0 — in both cases with nothing pointing at the cause.
/// Whether a path's extension names an image format `--diff`/`--image` could
/// read.
///
/// Used only to improve an error message, so a false negative costs nothing.
fn looks_like_image(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| {
            matches!(
                e.as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "tif" | "tiff" | "ico" | "avif"
            )
        })
}

fn strip_bom(text: String) -> String {
    match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => text,
    }
}

/// Whether `resource` is an `http(s)` URL (rich-cli's `rich <url>`). The scheme
/// is matched case-insensitively, as RFC 3986 specifies.
fn is_url(resource: &str) -> bool {
    let head: String = resource.chars().take(8).collect::<String>().to_lowercase();
    head.starts_with("http://") || head.starts_with("https://")
}

/// The lowercased file extension of a resource, taken from its basename (the
/// last path segment) with any `?query`/`#fragment` stripped — so it works for
/// URLs (`…/main.rs?raw=1` → `rs`) as well as plain paths.
fn resource_ext(resource: &str) -> Option<String> {
    let basename = resource.rsplit(['/', '\\']).next().unwrap_or(resource);
    let basename = basename.split(['?', '#']).next().unwrap_or(basename);
    basename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
}

/// The maximum *decoded* response body we will hold in memory, in bytes.
/// Exceeding it is a hard error (we never render a truncated resource).
#[cfg(feature = "fetch")]
const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;

/// Fetch a URL over HTTP(S), returning `(body, content_type)`. Port of
/// rich-cli's `requests.get`, with two deliberate safety bounds upstream lacks:
/// a 30-second global timeout and [`MAX_BODY_BYTES`].
///
/// Like `requests.get`, a non-2xx status is **not** an error — the body is
/// returned so an error page still renders. TLS verification is on (rustls with
/// bundled webpki roots).
#[cfg(feature = "fetch")]
fn fetch_url(url: &str, encoding: Option<Encoding>) -> Result<(String, Option<String>), String> {
    use std::time::Duration;
    let mut response = ureq::get(url)
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        // Match `requests.get`: don't turn 4xx/5xx into an error, so the
        // response body (often a useful error page) is rendered instead.
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|err| format!("cannot fetch {url}: {err}"))?;
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    // Bound the *decoded* body: `.reader()` yields post-decompression bytes, so
    // capping it here (rather than via ureq's `limit()`, whose LimitReader sits
    // *under* the gzip decoder and would only bound compressed bytes) also
    // defuses a `Content-Encoding: gzip` bomb. Read one byte past the limit so
    // an oversized body is detectable rather than silently truncated.
    let mut buffer = Vec::new();
    response
        .body_mut()
        .with_config()
        .reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_end(&mut buffer)
        .map_err(|err| format!("cannot read {url}: {err}"))?;
    if buffer.len() as u64 > MAX_BODY_BYTES {
        return Err(format!(
            "{url} exceeds the {} MiB limit",
            MAX_BODY_BYTES / (1024 * 1024)
        ));
    }
    let body =
        decode_strict(buffer, encoding).map_err(|err| format!("cannot decode {url}: {err}"))?;
    Ok((body, content_type))
}

/// Stub used when the crate is built without the `fetch` feature.
#[cfg(not(feature = "fetch"))]
fn fetch_url(url: &str, _encoding: Option<Encoding>) -> Result<(String, Option<String>), String> {
    Err(format!(
        "cannot fetch {url}: this build has no URL support (rebuild with the `fetch` feature)"
    ))
}

/// The high-level render mode implied by a response `Content-Type` (the MIME
/// type, ignoring any `; charset=…`). Used only when neither a flag nor the URL
/// extension already picked a mode.
fn content_type_mode(content_type: &str) -> Option<Mode> {
    let mime = mime_of(content_type);
    match mime.as_str() {
        "text/markdown" | "text/x-markdown" => Some(Mode::Markdown),
        "application/json" | "text/json" => Some(Mode::Json),
        "text/csv" => Some(Mode::Csv),
        _ => None,
    }
}

/// A syntect lexer name for a response `Content-Type`, for syntax-highlighting a
/// fetched resource that has no informative extension. A small common subset of
/// upstream's Pygments MIME table.
fn content_type_lexer(content_type: &str) -> Option<&'static str> {
    let mime = mime_of(content_type);
    Some(match mime.as_str() {
        "text/html" | "application/xhtml+xml" => "html",
        "text/css" => "css",
        "application/javascript" | "text/javascript" => "javascript",
        "application/xml" | "text/xml" => "xml",
        "text/x-python" | "application/x-python" | "text/x-python3" => "python",
        "text/x-rust" => "rust",
        "text/x-c" | "text/x-csrc" => "c",
        "application/x-sh" | "text/x-shellscript" => "bash",
        "application/x-yaml" | "text/yaml" | "text/x-yaml" => "yaml",
        "application/toml" | "text/x-toml" => "toml",
        _ => return None,
    })
}

/// Whether an extension names no particular language, so a `Content-Type` that
/// does should win when picking a syntax lexer.
fn is_uninformative_ext(ext: &str) -> bool {
    matches!(ext, "txt" | "text" | "log" | "dat" | "out")
}

/// The bare MIME type from a `Content-Type` header (lowercased, `; charset=…`
/// and surrounding whitespace stripped).
fn mime_of(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Resolve `Mode::Auto` to a concrete mode from the resource's file extension.
fn detect_mode(resource: Option<&str>) -> Mode {
    match resource.and_then(resource_ext).as_deref() {
        Some("md") | Some("markdown") => Mode::Markdown,
        Some("json") => Mode::Json,
        Some("csv") | Some("tsv") => Mode::Csv,
        Some("ipynb") => Mode::Ipynb,
        Some("gif") => Mode::Gif,
        // Anything the table above does not divert is source code, and upstream
        // highlights it: `rich main.rs` is syntax-highlighted with no flag at
        // all. We printed it raw instead, so `rich hello.py` produced no
        // highlighting and no styling in an export — while `docs/cli.md` line 12
        // advertised the upstream behaviour and `--help` documented ours. The
        // two contradicted each other; this resolves them the way the docs (and
        // upstream) say.
        Some(_) => Mode::Syntax,
        // No extension to go on (including stdin): stay in Auto and let the
        // caller decide, rather than guessing a lexer for something that may not
        // be source at all.
        None => Mode::Auto,
    }
}

/// Force a renderable to a fixed width, wherever on the screen it lands. Port
/// of rich-cli's `ForceWidth`.
///
/// `--width N` used to shrink the *console* to N, which also moved every
/// alignment inside N columns: `rich -w 20 --center` centred within 20 rather
/// than within the terminal, and an `--export-svg` frame came out N wide.
/// Upstream leaves the console alone and narrows only the renderable.
struct ForceWidth {
    child: Box<dyn Renderable>,
    width: usize,
}

impl Renderable for ForceWidth {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.child
            .rich_render(console, &options.update_width(self.width))
    }

    fn measure(&self, _console: &Console, _options: &ConsoleOptions) -> Measurement {
        Measurement::new(self.width, self.width)
    }
}

/// Position a renderable within the available width. Port of the horizontal
/// axis of `rich.align.Align`, which is what `console.print(justify=…)` wraps
/// every renderable in.
///
/// The core crate's [`Align`] follows upstream: it constrains the child to its
/// measured width, `set_shape`s the block, then pads it. But in this port
/// `Table`, `Json` and friends measure as the whole console (see
/// `measure_rendered`), so core `Align` would not move them. This wrapper takes
/// the width to constrain to from the caller instead.
struct Aligned {
    child: Box<dyn Renderable>,
    /// The width to render the child *within* — what upstream's `Align` reads
    /// out of `console.measure(renderable)` and hands to `Constrain`. The block
    /// it then pads is measured from the output, not from this: a `--csv` table
    /// given `-w 40` still renders at its own 23 columns, and it is 23 that gets
    /// centred.
    width: usize,
    justify: Justify,
}

impl Renderable for Aligned {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = self.width.clamp(1, options.max_width);
        // The child renders with the console's own justify, not this alignment.
        // Upstream puts `print(justify=…)` into the render options, but rich-cli
        // builds its `--print`/`--rule` text with an explicit `justify="default"`
        // — a truthy string in Python — which blocks the inheritance, so
        // `-w 9 --center -p mid` centres a three-cell block within the terminal
        // rather than centring "mid" inside nine columns first. Every other
        // renderable this CLI builds (`Table`, `Json`, `Syntax`, `Markdown`)
        // ignores `options.justify` in this port, so there is nothing else for
        // the inheritance to reach.
        let lines = console.render_lines(self.child.as_ref(), &options.update_width(width), false);
        let widths: Vec<usize> = lines
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum())
            .collect();
        // `Segment.get_shape` then `Segment.set_shape`: square the block off at
        // the widest line it actually produced, so a multi-line child moves as
        // one block instead of each line drifting to its own width.
        let shape = widths.iter().copied().max().unwrap_or(0);

        let excess = options.max_width.saturating_sub(shape);
        let (left, right) = match self.justify {
            Justify::Right => (excess, 0),
            Justify::Center => (excess / 2, excess - excess / 2),
            _ => (0, excess),
        };
        let blank = |count: usize| Segment::new(" ".repeat(count), Some(Style::new()));

        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, (line, line_width)) in lines.into_iter().zip(widths).enumerate() {
            if left > 0 {
                segments.push(blank(left));
            }
            segments.extend(line);
            let pad = shape - line_width + right;
            if pad > 0 {
                segments.push(blank(pad));
            }
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }
}

/// The width a renderable actually occupies, found by rendering it.
///
/// `Table`, `Json` and friends inherit `(max_width, max_width)` from the
/// `Renderable` default in the core crate, so asking them to measure themselves
/// always answers "the whole console" and a fitted panel would not shrink at
/// all. Neither pads its output to the space it is given, though, so the widest
/// rendered line *is* upstream's `Measurement.get(…).maximum`, clamped to the
/// console — which is what a fitted `Panel` needs.
///
/// Not usable for `Syntax`, whose lines *are* padded out to the full width by
/// its background: that one is measured from the source instead.
fn measure_rendered(console: &Console, renderable: &dyn Renderable) -> usize {
    console
        .render_lines(renderable, &console.options(), false)
        .iter()
        .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
        .max()
        .unwrap_or(0)
}

/// Only labels consumed by a panel or CSV table should be parsed. Other modes
/// ignore these options upstream, including malformed unused markup.
fn validate_labels(cli: &Cli) -> Result<(), String> {
    for label in cli.title.iter().chain(cli.caption.iter()) {
        Text::from_markup(label).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Apply upstream's decorator chain — padding, panel, style, fixed width,
/// alignment — and print the result. Port of the tail of rich-cli's `main`.
///
/// `fit` is the width the renderable measures at; `None` means "fills whatever
/// it is given", which is what a `Markdown` reports, so nothing shrinks to it.
fn decorate_and_emit(
    cli: &Cli,
    console: &Console,
    export: &Export,
    renderable: Box<dyn Renderable>,
    fit: Option<usize>,
) -> ExitCode {
    if cli.panel.is_some() {
        if let Err(err) = validate_labels(cli) {
            return fail(cli, ExitClass::Data, err);
        }
    }
    let max_width = console.width();
    // `if width > 0: expand = True` — a fixed width is a width to fill.
    let expand = cli.expand || cli.width.is_some();
    let mut renderable = renderable;
    let mut fit = fit;

    // This port's `Padding` and `Panel` always fill the width they are given, so
    // a *fitted* box is a `Constrain` around one. Upstream instead passes
    // `expand=False` and lets the box measure its own child — the same width,
    // reached from the outside, which is the only way that works while the core
    // crate's `Table`/`Syntax`/`Json` still inherit the default measurement.
    if let Some((top, right, bottom, left)) = cli.padding {
        let padded: Box<dyn Renderable> =
            Box::new(Padding::new(renderable, (top, right, bottom, left)));
        // `Padding.__rich_measure__`: the child plus the horizontal padding,
        // capped at the available width.
        fit = fit.map(|width| (width + left + right).min(max_width));
        renderable = if expand {
            padded
        } else {
            Box::new(Constrain::new(padded, fit))
        };
    }

    if let Some(box_set) = cli.panel {
        let mut panel = Panel::new(renderable).box_set(box_set);
        if let Some(title) = cli.title.clone() {
            panel = panel.title(title);
        }
        if let Some(caption) = cli.caption.clone() {
            panel = panel.subtitle(caption);
        }
        if let Some(style) = cli.panel_style.clone() {
            panel = panel.border_style(style);
        }
        // `Panel.__rich_measure__`: the wider of the child (measured in
        // `max_width - 4`) and the title, plus a border and a pad on each side.
        // The *subtitle* is deliberately absent — upstream does not measure it,
        // so a long `--caption` is truncated by the border rather than widening
        // the panel.
        //
        // Panel parses markup, normalizes newlines/tabs, and pads the label by
        // one cell on either side. Measure visible text with the same rules.
        let inner = max_width.saturating_sub(4);
        let child = fit.unwrap_or(inner).min(inner);
        let title_width = cli
            .title
            .as_deref()
            .map(|title| {
                let mut title = Text::from_markup(&rich::emoji::replace(title).replace('\n', " "))
                    .expect("title markup validated before rendering");
                title.expand_tabs(8);
                cell_len(title.plain()) + 2
            })
            .unwrap_or(0);
        fit = Some(child.max(title_width) + 4);
        let panel: Box<dyn Renderable> = Box::new(panel);
        renderable = if expand {
            panel
        } else {
            Box::new(Constrain::new(panel, fit))
        };
    }

    // `-s/--style` lays a style under everything, outside the panel — upstream's
    // `Styled(renderable, text_style)`. It does not change any width.
    if let Some(style) = cli.style.clone() {
        renderable = Box::new(Styled::new(renderable, style));
    }

    // `if width > 0 and not pager` — the pager lays out at its own width.
    if let Some(width) = cli.width.filter(|_| !cli.pager) {
        renderable = Box::new(ForceWidth {
            child: renderable,
            width,
        });
        fit = Some(width);
    }

    if let Some(justify) = cli.justify {
        renderable = Box::new(Aligned {
            child: renderable,
            width: fit.unwrap_or(max_width),
            justify,
        });
    }

    emit_exit_code(cli, emit(console, export, |c| c.print(renderable.as_ref())))
}

fn run(cli: Cli) -> ExitCode {
    if cli.watch {
        return run_watch(cli);
    }
    run_once(cli)
}

fn run_watch(mut cli: Cli) -> ExitCode {
    let Some(resource) = cli.resource.clone() else {
        return fail(
            &cli,
            ExitClass::Usage,
            "--watch requires a file path or URL",
        );
    };
    let several = cli.resources.len() > 1;
    for resource in &cli.resources {
        if resource == "-" || (cli.mode == Mode::Print && !is_url(resource)) {
            return fail(
                &cli,
                ExitClass::Usage,
                "--watch requires a local file or URL, not stdin or literal markup",
            );
        }
        if several && is_url(resource) {
            return fail(
                &cli,
                ExitClass::Usage,
                "--watch with several resources requires local files; watch a URL on its own",
            );
        }
        let effective_mode = if cli.mode == Mode::Auto {
            detect_mode(Some(resource))
        } else {
            cli.mode
        };
        if effective_mode.draws_directly() || effective_mode == Mode::Rule {
            return fail(
                &cli,
                ExitClass::Usage,
                "--watch requires a resource-backed render mode",
            );
        }
    }
    if several && (cli.export_html.is_some() || cli.export_svg.is_some()) {
        return fail(
            &cli,
            ExitClass::Usage,
            "--export-html/--export-svg cannot be combined with watching several files",
        );
    }
    // Redirected output must be a finite, deterministic command. This also
    // keeps `rich --watch file > snapshot.txt` from hanging in a pipeline.
    // Several files render once each, in order, exactly as separate runs.
    if !std::io::stdout().is_terminal() {
        cli.watch = false;
        if !several {
            return run_once(cli);
        }
        let mut first_failure = None;
        for resource in cli.resources.clone() {
            let mut single = cli.clone();
            single.resource = Some(resource.clone());
            single.resources = vec![resource];
            let status = run_once(single);
            if status != ExitClass::Success.exit_code() && first_failure.is_none() {
                first_failure = Some(status);
            }
        }
        return first_failure.unwrap_or_else(|| ExitClass::Success.exit_code());
    }
    let resource = resource.as_str();
    if !is_url(resource) {
        return watch::watch_files(cli);
    }
    // Only URLs remain: they keep the polling-only loop below.
    if is_url(resource) {
        #[cfg(not(feature = "fetch"))]
        {
            cli.watch = false;
            return run_once(cli);
        }
    }

    let interval = std::time::Duration::from_secs_f64(cli.watch_interval);
    let mut previous = None;
    loop {
        #[cfg(feature = "fetch")]
        let cached = if is_url(resource) && cli.watch_cache {
            Some(fetch_url(resource, cli.extensions.encoding))
        } else {
            None
        };
        #[cfg(not(feature = "fetch"))]
        let cached: Option<Result<(String, Option<String>), String>> = None;
        let fingerprint = match cached.as_ref() {
            Some(Ok((body, content_type))) => {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                body.hash(&mut hasher);
                content_type.hash(&mut hasher);
                format!("url:{:x}", hasher.finish())
            }
            Some(Err(error)) => format!("url-error:{error}"),
            None => watch_fingerprint(resource, false, cli.extensions.encoding),
        };
        let changed = previous.as_ref() != Some(&fingerprint);
        if changed || (is_url(resource) && !cli.watch_cache) {
            previous = Some(fingerprint);
            // Each watch frame owns the viewport; clear stale rows when a
            // replacement is shorter, then start the new frame at the top.
            let console = Console::new();
            console.clear();
            console.control(&Control::home());
            let _ = std::io::stdout().flush();
            let mut iteration = cli.clone();
            iteration.watch = false;
            let status = run_once_with_fetch(iteration, cached.and_then(Result::ok));
            if status != ExitClass::Success.exit_code() {
                // A temporary disappearance or parse failure is a frame error,
                // not a reason to abandon a watch that may recover.
                eprintln!("rich: watch will retry after the next change");
            }
        }
        std::thread::sleep(interval);
    }
}

fn watch_fingerprint(resource: &str, cache_url: bool, encoding: Option<Encoding>) -> String {
    if is_url(resource) {
        #[cfg(feature = "fetch")]
        if cache_url {
            return match fetch_url(resource, encoding) {
                Ok((body, content_type)) => {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    body.hash(&mut hasher);
                    content_type.hash(&mut hasher);
                    format!("url:{:x}", hasher.finish())
                }
                Err(error) => format!("url-error:{error}"),
            };
        }
        #[cfg(not(feature = "fetch"))]
        let _ = (cache_url, encoding);
        return "url".to_string();
    }
    match std::fs::metadata(resource) {
        Ok(metadata) if metadata.is_file() => {
            use std::hash::Hasher;
            // Atomic saves and in-place writes may preserve both size and
            // mtime. Read regular files each poll using a fixed-size buffer,
            // so detection is portable and memory does not grow with input.
            let mut file = match std::fs::File::open(resource) {
                Ok(file) => file,
                Err(error) => return format!("file-error:{error}"),
            };
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => hasher.write(&buffer[..count]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return format!("file-error:{error}"),
                }
            }
            format!("file:{:x}", hasher.finish())
        }
        Ok(_) => "not-file".to_string(),
        Err(_) => "missing".to_string(),
    }
}

fn run_once(cli: Cli) -> ExitCode {
    run_once_with_fetch(cli, None)
}

fn cli_theme(cli: &Cli) -> rich::Theme {
    let mut theme = rich::Theme::default_theme();
    for (name, style) in &cli.theme_styles {
        theme.insert(name.clone(), style.clone());
    }
    theme
}

fn run_once_with_fetch(mut cli: Cli, prefetched: Option<(String, Option<String>)>) -> ExitCode {
    if cli.sanitize {
        cli.title = cli
            .title
            .take()
            .map(|title| sanitize_terminal_controls(&title));
        cli.caption = cli
            .caption
            .take()
            .map(|caption| sanitize_terminal_controls(&caption));
    }

    if cli.batch {
        return run_batch(&cli);
    }

    // With no flags and no resource, show the capability demo.
    if cli.mode == Mode::Auto && cli.resource.is_none() {
        run_demo(cli.no_color, std::time::Duration::ZERO);
        return success(&cli);
    }

    let mut mode = match cli.mode {
        Mode::Auto => detect_mode(cli.resource.as_deref()),
        other => other,
    };

    // Modes that render incrementally write directly to the console instead of
    // composing one renderable, so no `ForceWidth` wrapper can reach them.
    let mut builder = Console::builder().no_color(cli.no_color);
    if !cli.theme_styles.is_empty() {
        builder = builder.theme(cli_theme(&cli));
    }
    // Parallel workers spool output, but render for the parent's destination.
    // Pager policy still checks the real stdout handle before starting a pager.
    if let Ok(terminal) = std::env::var("RS_RICH_BATCH_TERMINAL") {
        builder = builder.force_terminal(terminal == "1");
    }
    if let Some(width) = cli.width.filter(|_| mode.draws_directly()) {
        builder = builder.width(width);
    }
    // A watched resource renders into a live region, one column narrower than
    // the terminal (the Live display keeps the last column free).
    if let Some(width) = watch::capture_width() {
        builder = builder.width(width);
    }
    let mut console = builder.build();
    render_target::attach(&mut console);
    console.install_extensions();

    if mode == Mode::Rule {
        if let Some(title) = &cli.resource {
            if let Err(err) = Text::from_markup(title) {
                return fail(&cli, ExitClass::Data, err.to_string());
            }
        }
    }

    // SVG export needs a title (the resource's basename, else "rich") and a
    // stable id. Upstream's auto id hashes Python reprs, so we use a fixed one
    // (see the rich crate's svg module / DIVERGENCES #15). A literal resource
    // (print-mode markup, a rule's title) is not a path, so splitting it on
    // `/` or `\` would title `[b]Hi[/] there` as `] there`: it gets "rich".
    let literal = mode == Mode::Rule
        || (mode == Mode::Print && cli.resource.as_deref().is_some_and(|r| !is_url(r)));
    let mut svg_title = cli
        .resource
        .as_deref()
        .filter(|r| *r != "-" && !literal)
        .map(|r| r.rsplit(['/', '\\']).next().unwrap_or(r).to_string())
        .unwrap_or_else(|| "rich".to_string());
    if cli.sanitize {
        svg_title = sanitize_terminal_controls(&svg_title);
    }
    let export = Export {
        html_path: cli.export_html.as_deref(),
        svg_path: cli.export_svg.as_deref(),
        svg_title: &svg_title,
        pager: cli.pager,
        auto_pager: cli.auto_pager,
    };

    // GIF playback consumes the resource list itself (it can take several) and
    // animates rather than rendering once.
    if mode == Mode::Gif {
        return play_gifs(&cli, &console);
    }

    // `--diff` consumes both resources and reports rather than rendering one:
    // perceptually for two images, as a text, source or patch diff otherwise.
    if mode == Mode::Diff {
        let images = cli.resources.len() == 2 && cli.resources.iter().all(|r| looks_like_image(r));
        if !images {
            return run_text_diff(&cli, &console, &export);
        }
        if let Some(flag) = cli.tools.diff_option() {
            return fail(
                &cli,
                ExitClass::Usage,
                format!("{flag} applies to text diffs, not images"),
            );
        }
        #[cfg(feature = "art")]
        return run_diff(&cli, &console, &export);
        #[cfg(not(feature = "art"))]
        return fail(
            &cli,
            ExitClass::Usage,
            "this build has no image support (rebuild with the `art` feature)",
        );
    }

    // `--image` renders a single still image directly through rich-art's
    // `ImageArt` facade rather than composing a decorated renderable.
    #[cfg(feature = "art")]
    if mode == Mode::Image {
        return run_image(&cli, &console, &export);
    }
    #[cfg(not(feature = "art"))]
    if mode == Mode::Image {
        return fail(
            &cli,
            ExitClass::Usage,
            "this build has no image support (rebuild with the `art` feature)",
        );
    }

    if mode.streams_records() {
        return run_json_lines(&cli, &console, mode == Mode::Log);
    }

    // A rule takes its optional title from the resource string directly (no
    // fetch/read) — but it still goes through the decorators, because upstream
    // wraps it like anything else, so `--rule --panel` really does draw a panel.
    if mode == Mode::Rule {
        let rule = match cli.resource.as_deref() {
            Some(title) if title != "-" && cli.sanitize => {
                Rule::new(sanitize_terminal_controls(title))
            }
            Some(title) if title != "-" => Rule::new(title),
            _ => Rule::line(),
        };
        // `Rule.__rich_measure__` is `Measurement(1, 1)`: a rule claims no width
        // of its own, so a fitted panel around one is 5 cells wide.
        return decorate_and_emit(&cli, &console, &export, Box::new(rule), Some(1));
    }

    // Obtain the content: fetch it over HTTP(S) when the resource is a URL,
    // treat it as a literal markup string under `--print`, else read the
    // file/stdin. A URL also yields a `Content-Type` used below.
    let resource_is_url = matches!(cli.resource.as_deref(), Some(r) if is_url(r));
    let (mut content, content_type) = if let Some(fetched) = prefetched {
        fetched
    } else if resource_is_url {
        match fetch_url(cli.resource.as_deref().unwrap(), cli.extensions.encoding) {
            Ok(fetched) => fetched,
            Err(err) => {
                return fail(&cli, ExitClass::Input, err);
            }
        }
    } else if mode == Mode::Print && matches!(cli.resource.as_deref(), Some(r) if r != "-") {
        (cli.resource.clone().unwrap(), None)
    } else {
        match read_resource(cli.resource.as_deref(), cli.extensions.encoding) {
            Ok(content) => (content, None),
            Err(err) => {
                let name = cli.resource.as_deref().unwrap_or("<stdin>");
                return fail(&cli, ExitClass::Input, format!("cannot read {name}: {err}"));
            }
        }
    };
    // `ansi explain` shows escapes visibly; neutralising them first would
    // leave nothing to explain.
    if cli.sanitize && mode != Mode::AnsiExplain {
        content = sanitize_terminal_controls(&content);
    }

    // `--format` routes input automatic mode would print as text (stdin, no
    // extension) — or, with a named format, any input — to a renderer.
    let mut routed_language = None;
    let mut routed = false;
    if let Some(format) = cli.data.format.filter(|_| cli.mode == Mode::Auto) {
        if mode == Mode::Auto || matches!(format, inspect::InputFormat::Named(_)) {
            routed = true;
            mode = match inspect::route(format, &content, cli.resource.as_deref()) {
                inspect::Route::Json => Mode::Json,
                inspect::Route::Syntax(lexer) => {
                    routed_language = Some(lexer);
                    Mode::Syntax
                }
                inspect::Route::Text => Mode::Auto,
            };
        }
    }

    // For a URL that neither a flag nor a telltale extension resolved: upstream
    // renders fetched content as syntax-highlighted source by default, unless the
    // Content-Type marks it markdown / json / csv.
    if resource_is_url && mode == Mode::Auto && !routed {
        mode = content_type
            .as_deref()
            .and_then(content_type_mode)
            .unwrap_or(Mode::Syntax);
    }

    // Pre-parse JSON so a parse error surfaces before any (HTML) rendering.
    let json_content = if mode == Mode::Json && cli.sanitize {
        match sanitize_json_source(content.trim()) {
            Ok(content) => Some(content),
            Err(err) => {
                return fail(&cli, ExitClass::Data, format!("invalid JSON: {err}"));
            }
        }
    } else {
        None
    };

    let json = if mode == Mode::Json {
        let source = json_content.as_deref().unwrap_or_else(|| content.trim());
        match Json::new(source) {
            Ok(json) => Some(json),
            Err(err) => {
                return fail(&cli, ExitClass::Data, format!("invalid JSON: {err}"));
            }
        }
    } else {
        None
    };

    // Same treatment for a notebook. Without this the parse failure was printed
    // from inside the render closure, which cannot influence the exit code — so
    // a broken notebook reported an error and exited 0, while a broken .json
    // exited 1. A malformed input must not look like success to a script.
    if mode == Mode::Ipynb {
        let notebook = match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(mut value) if value.get("cells").and_then(|c| c.as_array()).is_some() => {
                if cli.sanitize {
                    sanitize_json_value(&mut value);
                }
                value
            }
            Ok(_) => {
                return fail(&cli, ExitClass::Data, "not a notebook: no `cells` array");
            }
            Err(err) => {
                return fail(
                    &cli,
                    ExitClass::Data,
                    format!("invalid notebook JSON: {err}"),
                );
            }
        };
        let renderable = build_ipynb(&notebook, cli.hyperlinks, cli.justify);
        let fit = renderable.measure(&console, &console.options()).maximum;
        return decorate_and_emit(&cli, &console, &export, Box::new(renderable), Some(fit));
    }

    // Markup comes from the user in Print mode, so a mistake in it should be
    // reported rather than printed literally (upstream raises `MarkupError`).
    // Checked up front because the render closure below cannot fail.
    if mode == Mode::Print {
        if let Err(err) = console.try_build_text(&content) {
            return fail(&cli, ExitClass::Data, err.to_string());
        }
    }

    // The highlighting language comes from the resource extension, falling back
    // (for a URL) to a lexer guessed from its Content-Type. An *uninformative*
    // extension (.txt/.log/…) doesn't shadow a Content-Type that names a real
    // language, so `snippet.txt` served as `text/x-python` still highlights.
    let extension = resource_ext(cli.resource.as_deref().unwrap_or_default())
        .filter(|ext| !is_uninformative_ext(ext));
    let language = routed_language
        .map(str::to_string)
        .or(extension)
        .or_else(|| {
            content_type
                .as_deref()
                .and_then(content_type_lexer)
                .map(str::to_string)
        })
        .unwrap_or_default();

    // Build the renderable, and the width a non-expanding `Panel`/`Padding`
    // would shrink around it — upstream's `Measurement.get(…).maximum`.
    let (renderable, fit): (Box<dyn Renderable>, Option<usize>) = match mode {
        // `Markdown` defines no `__rich_measure__`, so upstream measures it as
        // the whole available width and a panel around it never shrinks.
        Mode::Markdown => (Box::new(build_markdown(&content, cli.hyperlinks)), None),
        Mode::Json => {
            // `rich.json.JSON` sets `text.no_wrap = True`, and every decorator
            // upstream can wrap the document in — `Padding`, `Panel`, `Styled`,
            // `ForceWidth` — is a `ConsoleRenderable`, so the `Text` reaches
            // `Text.__rich_console__` with the flag intact and each line is
            // CROPPED at the width. A *bare* document is a `Text`, which
            // `_collect_renderables` re-joins through `Text(sep).join(...)`;
            // `Text.join` copies its metadata from the separator, so the flag
            // is lost and the default fold wrap applies instead.
            //
            // `--left/--center/--right` is not on the list: `align_append`
            // wraps whatever `check_text` produced, so the join — and the loss
            // — has already happened by the time `Align` sees it.
            //
            // `--pager` skips `ForceWidth`, but pages through
            // `console.render_lines(renderable, …)`, which renders the `Text`
            // directly and keeps the flag.
            let nested = cli.padding.is_some()
                || cli.panel.is_some()
                || cli.style.is_some()
                || cli.width.is_some()
                || cli.pager
                || cli.auto_pager;
            let json = json.expect("json parsed above").no_wrap(nested);
            #[cfg(feature = "json-escape-safe")]
            let json = json.escape_safe(true);
            let fit = measure_rendered(&console, &json);
            (Box::new(json), Some(fit))
        }
        Mode::Csv => {
            if let Err(err) = validate_labels(&cli) {
                return fail(&cli, ExitClass::Data, err);
            }
            // `.tsv` only picks the fallback dialect; the sniffer reads the file
            // itself, so a comma-separated `.tsv` still renders as a table.
            let fallback = csv_fallback_delimiter(cli.resource.as_deref());
            let table = match build_csv_table(
                &content,
                fallback,
                cli.title.as_deref(),
                cli.caption.as_deref(),
            ) {
                Some(table) => table,
                // Upstream's `on_error(str(error))`. The message is CPython's,
                // verbatim, because it is the one a script would grep for.
                None => {
                    return fail(&cli, ExitClass::Data, "Could not determine delimiter");
                }
            };
            // Stream undecorated CSV rows: measuring or collecting the complete
            // styled table first multiplies memory by hundreds on tall inputs.
            // Containers, alignment, paging and exports still need the full view.
            if cli.panel.is_none()
                && cli.padding.is_none()
                && cli.style.is_none()
                && cli.justify.is_none()
                && !cli.pager
                && !cli.auto_pager
                && cli.export_html.is_none()
                && cli.export_svg.is_none()
                && !watch::capturing()
            {
                let mut options = console.options();
                options.max_width = cli.width.unwrap_or_else(|| {
                    table
                        .measure(&console, &options)
                        .maximum
                        .min(options.max_width)
                        .max(1)
                });
                let stdout = std::io::stdout();
                let mut output = std::io::BufWriter::new(stdout.lock());
                let result = table
                    .try_for_each_line(&console, &options, |line| {
                        let line = Segment::crop_lines(&line, console.width());
                        writeln!(output, "{}", console.segments_to_string(&line))
                    })
                    .and_then(|()| output.flush());
                if let Err(error) = result {
                    // A consumer such as `head` may finish before this table.
                    // Match the other stdout paths: a closed pipe is success.
                    if error.kind() == std::io::ErrorKind::BrokenPipe {
                        return success(&cli);
                    }
                    return fail(
                        &cli,
                        ExitClass::Input,
                        format!("could not write CSV output: {error}"),
                    );
                }
                return success(&cli);
            }
            let fit = measure_rendered(&console, &table);
            (Box::new(table), Some(fit))
        }
        Mode::Syntax => {
            // `Syntax.__rich_measure__`: the widest source line, plus padding and
            // a line-number column — neither of which this CLI turns on.
            let fit = content.lines().map(cell_len).max().unwrap_or(0);
            (
                Box::new(Syntax::new(content.as_str(), language.as_str()).word_wrap(true)),
                Some(fit),
            )
        }
        Mode::AnsiExplain => {
            let view = tools::ansi_explain(&cli.tools, &content);
            let fit = view.measure(&console, &console.options()).maximum;
            (view, Some(fit))
        }
        Mode::Inspect => {
            let encoding = cli.extensions.encoding;
            let read = |path: &str| {
                read_resource(Some(path), encoding)
                    .map_err(|err| format!("cannot read {path}: {err}"))
            };
            let view = match inspect::build(&cli.data, &content, cli.resource.as_deref(), read) {
                Ok(view) => view,
                Err(err) => return fail(&cli, ExitClass::Data, err),
            };
            let fit = view.measure(&console, &console.options()).maximum;
            (view, Some(fit))
        }
        // Print + auto: parse markup (Print) or take plain text (auto).
        _ => {
            let text = if mode == Mode::Print {
                console.build_text(&content)
            } else {
                Text::new(content.as_str())
            };
            let fit = text.measurement().1;
            (Box::new(text), Some(fit))
        }
    };
    decorate_and_emit(&cli, &console, &export, renderable, fit)
}

/// Turn an "everything written successfully" flag into a process exit code.
/// A failed `--export-html`/`--export-svg` write must not report success.
#[allow(dead_code)]
fn exit_code(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn emit_exit_code(cli: &Cli, result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => success(cli),
        Err(message) => fail(cli, ExitClass::Input, message),
    }
}

fn write_rendered_stdout(console: &Console, renderable: &dyn Renderable) -> std::io::Result<()> {
    let output = console.render_export(renderable);
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(output.as_bytes())
}

fn run_json_lines(cli: &Cli, console: &Console, log_mode: bool) -> ExitCode {
    if cli.extensions.encoding.is_some() {
        return fail(
            cli,
            ExitClass::Usage,
            "--encoding is not supported with streaming JSONL/log mode",
        );
    }
    if matches!(cli.resource.as_deref(), Some(resource) if is_url(resource)) {
        return fail(
            cli,
            ExitClass::Usage,
            "streaming JSONL/log mode reads files or stdin, not URLs",
        );
    }

    let mut input: Box<dyn BufRead> = match cli.resource.as_deref() {
        Some(path) if path != "-" => {
            let file = std::path::Path::new(path);
            if file.is_dir() {
                return fail(
                    cli,
                    ExitClass::Input,
                    format!("cannot read {path}: is a directory, not a file"),
                );
            }
            match std::fs::File::open(file) {
                Ok(file) => Box::new(std::io::BufReader::new(file)),
                Err(err) => {
                    return fail(cli, ExitClass::Input, format!("cannot read {path}: {err}"));
                }
            }
        }
        _ => {
            if std::io::stdin().is_terminal() {
                let eof = if cfg!(windows) {
                    "Ctrl-Z then Enter"
                } else {
                    "Ctrl-D"
                };
                eprintln!("rich: reading stdin; finish input with {eof}");
            }
            Box::new(std::io::BufReader::new(std::io::stdin()))
        }
    };

    let mut line = String::new();
    let mut line_number = 0usize;
    loop {
        line.clear();
        let read = match input.read_line(&mut line) {
            Ok(read) => read,
            Err(err) => {
                return fail(
                    cli,
                    ExitClass::Input,
                    format!("cannot read JSONL stream: {err}"),
                );
            }
        };
        if read == 0 {
            break;
        }
        line_number += 1;
        let record = line.trim_end_matches(['\r', '\n']);
        let mut value = match serde_json::from_str::<serde_json::Value>(record) {
            Ok(value) => value,
            Err(err) => {
                return fail(
                    cli,
                    ExitClass::Data,
                    format!("invalid JSONL at line {line_number}: {err}"),
                );
            }
        };
        if cli.sanitize {
            sanitize_json_value(&mut value);
        }
        let wrote = if log_mode {
            if cli.log_presentation == "rich" {
                write_rendered_stdout(console, &structured_log::event(&value))
            } else {
                let text = Text::new(format_log_record(&value));
                write_rendered_stdout(console, &text)
            }
        } else {
            let source = serde_json::to_string(&value).expect("JSON value serializes");
            let json = Json::new(&source).expect("serialized JSON parses");
            write_rendered_stdout(console, &json)
        };
        if let Err(error) = wrote {
            if error.kind() == std::io::ErrorKind::BrokenPipe {
                return success(cli);
            }
            return fail(
                cli,
                ExitClass::Input,
                format!("could not write JSONL output: {error}"),
            );
        }
    }
    success(cli)
}

fn format_log_record(value: &serde_json::Value) -> String {
    let Some(object) = value.as_object() else {
        return serde_json::to_string(value).expect("JSON value serializes");
    };
    let take_value = |keys: &[&'static str]| {
        keys.iter().find_map(|key| {
            object.get(*key).map(|value| {
                let display = value.as_str().map(str::to_string).unwrap_or_else(|| {
                    serde_json::to_string(value).expect("JSON value serializes")
                });
                (*key, display, value.is_string())
            })
        })
    };
    let timestamp = take_value(&["timestamp", "time", "@timestamp"]);
    let level = take_value(&["level", "severity"]);
    let message = take_value(&["message", "msg"]);
    let mut rest = serde_json::Map::new();
    for (key, value) in object {
        let consumed = timestamp
            .as_ref()
            .is_some_and(|(consumed, _, _)| consumed == key)
            || level
                .as_ref()
                .is_some_and(|(consumed, _, _)| consumed == key)
            || message
                .as_ref()
                .is_some_and(|(consumed, _, _)| consumed == key);
        if !consumed {
            rest.insert(key.clone(), value.clone());
        }
    }
    let mut parts = Vec::new();
    if let Some((_, timestamp, _)) = timestamp {
        parts.push(timestamp);
    }
    if let Some((_, level, is_string)) = level {
        parts.push(if is_string {
            level.to_ascii_uppercase()
        } else {
            level
        });
    }
    if let Some((_, message, _)) = message {
        parts.push(message);
    }
    if !rest.is_empty() {
        parts.push(
            serde_json::to_string(&serde_json::Value::Object(rest)).expect("JSON value serializes"),
        );
    }
    if parts.is_empty() {
        serde_json::to_string(value).expect("JSON value serializes")
    } else {
        parts.join(" ")
    }
}

/// A CSV dialect: everything `csv.Sniffer` decides and `csv.reader` consumes.
#[derive(Debug, Clone, Copy)]
struct Dialect {
    delimiter: char,
    quotechar: char,
    doublequote: bool,
    skipinitialspace: bool,
}

impl Dialect {
    /// `csv.get_dialect("excel")` (or `"excel-tab"` for a tab): upstream's
    /// fallback for a `.csv`/`.tsv` the sniffer cannot read.
    fn excel(delimiter: char) -> Self {
        Dialect {
            delimiter,
            quotechar: '"',
            doublequote: true,
            skipinitialspace: false,
        }
    }
}

/// Whether `c` is a `\w` character for CPython's `re` over `str`.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The `[^\w\n"']` class the sniffer accepts as a candidate delimiter.
fn is_delimiter_char(c: char) -> bool {
    !is_word_char(c) && c != '\n' && c != '"' && c != '\''
}

/// The `["']` class the sniffer accepts as a candidate quote character.
fn is_quote_char(c: char) -> bool {
    c == '"' || c == '\''
}

/// One hit from the quote/delimiter scan: the quote, the delimiter bracketing
/// it (absent in the fourth, delimiter-free pattern), and whether a space sat
/// between the two.
struct QuoteHit {
    quote: char,
    delim: Option<char>,
    space: bool,
}

/// Port of `csv.Sniffer.sniff`, restricted to `delimiters` when given (rich-cli
/// passes `",\t|;"`). `None` is CPython's `csv.Error`: no delimiter found.
///
/// `doublequote` is **not** sniffed; it is left at excel's `true`. CPython
/// decides it with a regex that only fires when a `""` pair sits inside a
/// quoted field containing neither the delimiter nor a newline, and when it
/// does not fire `csv.reader` stops unescaping — `"he said ""hi"""` reads back
/// as `he said "hi"""`. Reproducing that can only ever make a well-formed file
/// render worse, and upstream's own fallback (`csv.get_dialect("excel")`, used
/// for every file the sniffer rejects) already sets it true. The divergence is
/// confined to files that use `""` escaping *and* defeat the heuristic, e.g. a
/// `""` inside a multi-line cell: there we unescape and upstream does not.
fn sniff(sample: &str, delimiters: Option<&[char]>) -> Option<Dialect> {
    let data: Vec<char> = sample.chars().collect();
    let (quote, mut delimiter, mut skipinitialspace) = guess_quote_and_delimiter(&data, delimiters);
    if delimiter.is_none() {
        let (guessed, spaced) = guess_delimiter(sample, delimiters);
        delimiter = guessed;
        skipinitialspace = spaced;
    }
    Some(Dialect {
        delimiter: delimiter?,
        // `_csv.reader` won't accept an empty quotechar, so upstream falls back
        // to `"` when the scan found no quotes at all.
        quotechar: quote.unwrap_or('"'),
        doublequote: true,
        skipinitialspace,
    })
}

/// Port of `csv.Sniffer._guess_quote_and_delimiter`: look for text enclosed in
/// two identical quotes that are themselves bracketed by the same character.
///
/// CPython uses four backreferencing regexes (`(?P=quote)`, `(?P=delim)`),
/// which no Rust regex engine can express, so they are scanned by hand below.
/// The first pattern that hits anywhere decides; the most frequent quote wins,
/// and so does the most frequent delimiter seen beside it.
fn guess_quote_and_delimiter(
    data: &[char],
    delimiters: Option<&[char]>,
) -> (Option<char>, Option<char>, bool) {
    let hits = quote_hits(data);
    if hits.is_empty() {
        return (None, None, false);
    }

    // Insertion-ordered tallies: Python's `max(dict, key=dict.get)` returns the
    // FIRST key holding the maximum, so the order these were first seen in
    // decides ties.
    let mut quotes: Vec<(char, usize)> = Vec::new();
    let mut delims: Vec<(char, usize)> = Vec::new();
    let mut spaces = 0usize;
    fn bump(table: &mut Vec<(char, usize)>, key: char) {
        match table.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 += 1,
            None => table.push((key, 1)),
        }
    }
    for hit in &hits {
        bump(&mut quotes, hit.quote);
        // The fourth pattern has no delimiter group at all, so it contributes
        // only a quote — upstream `continue`s past both tallies below.
        let Some(delim) = hit.delim else { continue };
        if delimiters.is_none_or(|allowed| allowed.contains(&delim)) {
            bump(&mut delims, delim);
        }
        if hit.space {
            spaces += 1;
        }
    }

    let first_max = |table: &[(char, usize)]| -> Option<(char, usize)> {
        let mut best: Option<(char, usize)> = None;
        for &(key, count) in table {
            if best.is_none_or(|(_, seen)| count > seen) {
                best = Some((key, count));
            }
        }
        best
    };
    let quotechar = first_max(&quotes).map(|(key, _)| key);
    match first_max(&delims) {
        Some((delim, count)) => (quotechar, Some(delim), count == spaces),
        // A single column of quoted data: quotes but nothing bracketing them.
        None => (quotechar, None, false),
    }
}

/// One of CPython's four quote/delimiter patterns, scanned over the sample.
type QuoteScan = fn(&[char]) -> Vec<QuoteHit>;

/// Run CPython's four quote/delimiter patterns in order, returning the hits of
/// the first that matches anything.
fn quote_hits(data: &[char]) -> Vec<QuoteHit> {
    let scans: [QuoteScan; 4] = [
        scan_delim_quote_delim,
        scan_line_quote_delim,
        scan_delim_quote_line,
        scan_line_quote_line,
    ];
    for scan in scans {
        let hits = scan(data);
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}

/// `(?P<delim>[^\w\n"'])(?P<space> ?)(?P<quote>["']).*?(?P=quote)(?P=delim)` —
/// `,"some text",`. The ` ?` needs no backtracking: if the space is there and
/// the next character is not a quote, dropping the space only offers the space
/// itself as the quote, which it is not.
fn scan_delim_quote_delim(data: &[char]) -> Vec<QuoteHit> {
    scan_delim_quote(data, |data, quote, delim, from| {
        let mut k = from;
        while k + 1 < data.len() {
            if data[k] == quote && data[k + 1] == delim {
                return Some(k + 2);
            }
            k += 1;
        }
        None
    })
}

/// `(?P<delim>[^\w\n"'])(?P<space> ?)(?P<quote>["']).*?(?P=quote)(?:$|\n)` —
/// `,"some text"` at the end of a line.
fn scan_delim_quote_line(data: &[char]) -> Vec<QuoteHit> {
    scan_delim_quote(data, |data, quote, _delim, from| {
        let mut k = from;
        while k < data.len() {
            if data[k] == quote && (k + 1 == data.len() || data[k + 1] == '\n') {
                // `$` is zero-width and the engine prefers it, so the match ends
                // at the closing quote either way.
                return Some(k + 1);
            }
            k += 1;
        }
        None
    })
}

/// The shared `<delim><space?><quote> … ` head of patterns one and three;
/// `close` finds the closing quote and reports where the match ends.
fn scan_delim_quote(
    data: &[char],
    close: fn(&[char], char, char, usize) -> Option<usize>,
) -> Vec<QuoteHit> {
    let mut hits = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if is_delimiter_char(data[i]) {
            let delim = data[i];
            let mut j = i + 1;
            let mut space = false;
            if j < data.len() && data[j] == ' ' {
                space = true;
                j += 1;
            }
            if j < data.len() && is_quote_char(data[j]) {
                let quote = data[j];
                if let Some(end) = close(data, quote, delim, j + 1) {
                    hits.push(QuoteHit {
                        quote,
                        delim: Some(delim),
                        space,
                    });
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    hits
}

/// `(?:^|\n)(?P<quote>["']).*?(?P=quote)(?P<delim>[^\w\n"'])(?P<space> ?)` —
/// `"some text",` at the start of a line.
fn scan_line_quote_delim(data: &[char]) -> Vec<QuoteHit> {
    scan_line_quote(data, |data, quote, from| {
        let mut k = from;
        while k + 1 < data.len() {
            if data[k] == quote && is_delimiter_char(data[k + 1]) {
                let space = k + 2 < data.len() && data[k + 2] == ' ';
                return Some((Some(data[k + 1]), space, if space { k + 3 } else { k + 2 }));
            }
            k += 1;
        }
        None
    })
}

/// `(?:^|\n)(?P<quote>["']).*?(?P=quote)(?:$|\n)` — a whole line that is one
/// quoted field, with no delimiter to learn from.
fn scan_line_quote_line(data: &[char]) -> Vec<QuoteHit> {
    scan_line_quote(data, |data, quote, from| {
        let mut k = from;
        while k < data.len() {
            if data[k] == quote && (k + 1 == data.len() || data[k + 1] == '\n') {
                return Some((None, false, k + 1));
            }
            k += 1;
        }
        None
    })
}

/// The shared `(?:^|\n)<quote> … ` head of patterns two and four; `close`
/// reports `(delimiter, saw a space, match end)`.
#[allow(clippy::type_complexity)]
fn scan_line_quote(
    data: &[char],
    close: fn(&[char], char, usize) -> Option<(Option<char>, bool, usize)>,
) -> Vec<QuoteHit> {
    let mut hits = Vec::new();
    let mut i = 0;
    while i < data.len() {
        // `^` is zero-width at a line start; the `\n` branch consumes the
        // newline and puts the quote on the character after it. The engine tries
        // them in that order at each position.
        let mut starts: Vec<usize> = Vec::new();
        if i == 0 || data[i - 1] == '\n' {
            starts.push(i);
        }
        if data[i] == '\n' {
            starts.push(i + 1);
        }
        let mut advanced = false;
        for quote_at in starts {
            if quote_at >= data.len() || !is_quote_char(data[quote_at]) {
                continue;
            }
            let quote = data[quote_at];
            if let Some((delim, space, end)) = close(data, quote, quote_at + 1) {
                hits.push(QuoteHit {
                    quote,
                    delim,
                    space,
                });
                i = end;
                advanced = true;
                break;
            }
        }
        if !advanced {
            i += 1;
        }
    }
    hits
}

/// Port of `csv.Sniffer._guess_delimiter`: the character whose per-line
/// occurrence count is most consistent across the sample wins.
fn guess_delimiter(sample: &str, delimiters: Option<&[char]>) -> (Option<char>, bool) {
    // `filter(None, data.split('\n'))` — blank lines carry no evidence.
    let data: Vec<&str> = sample.split('\n').filter(|line| !line.is_empty()).collect();
    if data.is_empty() {
        return (None, false);
    }
    /// CPython scans `[chr(c) for c in range(127)]` — 7-bit ASCII.
    const ASCII: usize = 127;

    let chunk_length = 10.min(data.len());
    let mut iteration = 0usize;
    // Per character, an insertion-ordered list of (occurrences on a line, how
    // many lines had exactly that many) — upstream's "meta-frequency".
    let mut char_frequency: Vec<Vec<(usize, usize)>> = vec![Vec::new(); ASCII];
    // The winning (frequency, confidence) per character. Confidence can go
    // negative once every competing frequency is subtracted, hence `isize`.
    let mut modes: Vec<Option<(usize, isize)>> = vec![None; ASCII];
    let mut delims: Vec<(char, (usize, isize))> = Vec::new();

    let (mut start, mut end) = (0usize, chunk_length);
    while start < data.len() {
        iteration += 1;
        for line in &data[start..end.min(data.len())] {
            for (code, table) in char_frequency.iter_mut().enumerate() {
                let c = code as u8 as char;
                // Counted even when zero: a character absent from a line is
                // evidence against it being the delimiter.
                let freq = line.matches(c).count();
                match table.iter_mut().find(|(seen, _)| *seen == freq) {
                    Some(entry) => entry.1 += 1,
                    None => table.push((freq, 1)),
                }
            }
        }

        for (code, items) in char_frequency.iter().enumerate() {
            if items.len() == 1 && items[0].0 == 0 {
                continue;
            }
            if items.len() > 1 {
                // The first frequency with the highest count, less the sum of
                // every other count.
                let mut best = 0usize;
                for (index, item) in items.iter().enumerate() {
                    if item.1 > items[best].1 {
                        best = index;
                    }
                }
                let others: usize = items
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != best)
                    .map(|(_, item)| item.1)
                    .sum();
                modes[code] = Some((items[best].0, items[best].1 as isize - others as isize));
            } else if let Some(&(freq, count)) = items.first() {
                modes[code] = Some((freq, count as isize));
            }
        }

        let total = (chunk_length * iteration).min(data.len()) as f64;
        let mut consistency = 1.0f64;
        let threshold = 0.9f64;
        while delims.is_empty() && consistency >= threshold {
            for (code, mode) in modes.iter().enumerate() {
                let Some((freq, count)) = *mode else { continue };
                let c = code as u8 as char;
                if freq > 0
                    && count > 0
                    && (count as f64 / total) >= consistency
                    && delimiters.is_none_or(|allowed| allowed.contains(&c))
                {
                    delims.push((c, (freq, count)));
                }
            }
            consistency -= 0.01;
        }

        if delims.len() == 1 {
            let delim = delims[0].0;
            return (Some(delim), saw_space_after(data[0], delim));
        }

        start = end;
        end += chunk_length;
    }

    if delims.is_empty() {
        return (None, false);
    }
    if delims.len() > 1 {
        for preferred in [',', '\t', ';', ' ', ':'] {
            if delims.iter().any(|(c, _)| *c == preferred) {
                return (Some(preferred), saw_space_after(data[0], preferred));
            }
        }
    }
    // `items = [(v, k) …]; items.sort()` — ordered by the mode, then the char.
    let best = delims
        .iter()
        .max_by_key(|(c, mode)| (*mode, *c))
        .expect("delims is non-empty");
    (Some(best.0), saw_space_after(data[0], best.0))
}

/// Upstream's `skipinitialspace` test: every delimiter on the first line is
/// followed by a space.
fn saw_space_after(line: &str, delimiter: char) -> bool {
    line.matches(delimiter).count() == line.matches(&format!("{delimiter} ")).count()
}

/// What `csv.Sniffer.has_header` decides a column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnType {
    /// Every value so far parsed as a Python `complex`.
    Number,
    /// They did not, but all had this many characters.
    Length(usize),
}

/// Whether a column is still in play, and what it has looked like so far.
#[derive(Debug, Clone, Copy)]
enum ColumnVote {
    /// In `columnTypes` with the value `None`: no data row typed it yet.
    Untyped,
    Typed(ColumnType),
    /// `del columnTypes[col]` — the column was inconsistent.
    Dropped,
}

/// Port of `csv.Sniffer.has_header`: if a column is one consistent type in every
/// row *except* the first, the first row is a label. Each column casts one vote.
///
/// The typing is CPython's current one — a single `complex` attempt, falling
/// back to the string's length. The historical `for thisType in [int, float,
/// complex]` loop is NOT equivalent: it makes a column mixing `10` and `9.5`
/// read as inconsistent (int then float), so `price,note / 10,aa / 9.5,bbb`
/// loses its header.
///
/// `None` is CPython's `csv.Error` escaping from the re-sniff, which upstream
/// handles the same way as a failed `sniff`.
fn has_header(sample: &str) -> Option<bool> {
    // Note the missing `delimiters` argument: upstream re-sniffs here with the
    // full candidate set, not the four it renders with.
    let dialect = sniff(sample, None)?;
    let mut rows = read_csv_rows(sample, &dialect).into_iter();
    let header = rows.next()?;
    let columns = header.len();
    let mut votes = vec![ColumnVote::Untyped; columns];

    for (checked, row) in rows.enumerate() {
        // An arbitrary cap, "to keep it sane" — the 22nd data row and beyond are
        // never looked at, however inconsistent they are.
        if checked > 20 {
            break;
        }
        if row.len() != columns {
            continue;
        }
        for (col, vote) in votes.iter_mut().enumerate() {
            if matches!(vote, ColumnVote::Dropped) {
                continue;
            }
            let this = if parses_as_complex(&row[col]) {
                ColumnType::Number
            } else {
                ColumnType::Length(row[col].chars().count())
            };
            match vote {
                ColumnVote::Untyped => *vote = ColumnVote::Typed(this),
                ColumnVote::Typed(known) if *known != this => *vote = ColumnVote::Dropped,
                _ => {}
            }
        }
    }

    let mut tally = 0isize;
    for (col, vote) in votes.iter().enumerate() {
        match vote {
            ColumnVote::Dropped => {}
            // `colType` is still `None`, and `None(header[col])` raises
            // TypeError — which counts as "the header does not fit the column".
            ColumnVote::Untyped => tally += 1,
            ColumnVote::Typed(ColumnType::Length(len)) => {
                if header[col].chars().count() == *len {
                    tally -= 1;
                } else {
                    tally += 1;
                }
            }
            ColumnVote::Typed(ColumnType::Number) => {
                if parses_as_complex(&header[col]) {
                    tally -= 1;
                } else {
                    tally += 1;
                }
            }
        }
    }
    Some(tally > 0)
}

/// Whether Python's `complex(value)` would succeed — the type test at the heart
/// of `has_header`.
///
/// `complex()` is much looser than "looks like a number": it takes surrounding
/// whitespace, one level of parentheses, a sign, `inf`/`infinity`/`nan` in any
/// case, an exponent, `_` digit separators and an imaginary `j` suffix. All of
/// that is honoured. The single narrowing is the digit set — Python accepts any
/// Unicode decimal digit, this accepts ASCII — which can only ever move one
/// has-header vote, and only for a column written in non-ASCII numerals.
fn parses_as_complex(value: &str) -> bool {
    let trimmed = value.trim();
    let body = match trimmed.strip_prefix('(') {
        Some(inner) => match inner.strip_suffix(')') {
            Some(inner) => inner.trim(),
            None => return false,
        },
        None if trimmed.ends_with(')') => return false,
        None => trimmed,
    };
    if body.is_empty() {
        return false;
    }
    // `a`, `aj`, or `a±bj`. The split is the first sign that is not an
    // exponent's, so `1e-5` stays one number.
    let chars: Vec<char> = body.chars().collect();
    let split = (1..chars.len())
        .find(|&i| matches!(chars[i], '+' | '-') && !matches!(chars[i - 1], 'e' | 'E'));
    match split {
        Some(index) => {
            let at: usize = chars[..index].iter().map(|c| c.len_utf8()).sum();
            let (real, imaginary) = body.split_at(at);
            is_float_literal(real) && is_imaginary_literal(imaginary)
        }
        None => is_float_literal(body) || is_imaginary_literal(body),
    }
}

/// A Python float literal: optional sign, then `inf`/`infinity`/`nan`, or
/// digits with an optional fraction and exponent.
fn is_float_literal(value: &str) -> bool {
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    if matches!(
        body.to_ascii_lowercase().as_str(),
        "inf" | "infinity" | "nan"
    ) {
        return true;
    }
    let chars: Vec<char> = body.chars().collect();
    let mut index = 0usize;
    let mut digits = 0usize;
    let take_digits = |index: &mut usize, digits: &mut usize| {
        while *index < chars.len() && (chars[*index].is_ascii_digit() || chars[*index] == '_') {
            if chars[*index].is_ascii_digit() {
                *digits += 1;
            }
            *index += 1;
        }
    };
    take_digits(&mut index, &mut digits);
    if index < chars.len() && chars[index] == '.' {
        index += 1;
        take_digits(&mut index, &mut digits);
    }
    if digits == 0 {
        return false;
    }
    if index < chars.len() && (chars[index] == 'e' || chars[index] == 'E') {
        index += 1;
        if index < chars.len() && matches!(chars[index], '+' | '-') {
            index += 1;
        }
        let mut exponent = 0usize;
        take_digits(&mut index, &mut exponent);
        if exponent == 0 {
            return false;
        }
    }
    index == chars.len()
}

/// `<number>j`, or a bare `j`/`+j`/`-j` (which Python reads as ±1j).
fn is_imaginary_literal(value: &str) -> bool {
    let Some(body) = value.strip_suffix(['j', 'J']) else {
        return false;
    };
    matches!(body, "" | "+" | "-") || is_float_literal(body)
}

/// Where [`read_csv_rows`] is within a record. Port of `_csv.c`'s reader states
/// (its `EAT_CRNL` is unreachable here: universal newlines ran first).
enum State {
    StartRecord,
    StartField,
    InField,
    InQuotedField,
    QuoteInQuotedField,
}

/// Parse `content` into rows of fields with `dialect`. Port of `_csv.reader`'s
/// state machine, minus the escape character (no sniffed dialect sets one) and
/// `strict` (always off, as `QUOTE_MINIMAL` leaves it).
///
/// A blank line yields an **empty** row, as `csv.reader` does — upstream drops
/// those before building the table, where a `[""]` row would have drawn a
/// spurious blank line in it. `\r` never reaches here: the reader has already
/// applied Python's universal newlines.
fn read_csv_rows(content: &str, dialect: &Dialect) -> Vec<Vec<String>> {
    // Strip a leading UTF-8 BOM so it doesn't cling to the first header cell.
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut state = State::StartRecord;

    for c in content.chars() {
        match state {
            State::StartRecord => {
                if c == '\n' {
                    rows.push(Vec::new());
                } else {
                    state = State::StartField;
                    // Re-dispatch this character as the start of a field.
                    read_csv_char(c, dialect, &mut state, &mut field, &mut row, &mut rows);
                }
            }
            _ => read_csv_char(c, dialect, &mut state, &mut field, &mut row, &mut rows),
        }
    }
    if !matches!(state, State::StartRecord) {
        row.push(std::mem::take(&mut field));
        row.shrink_to_fit();
        rows.push(std::mem::take(&mut row));
    }
    rows
}

/// One character of [`read_csv_rows`]'s state machine, for every state but
/// `StartRecord`.
fn read_csv_char(
    c: char,
    dialect: &Dialect,
    state: &mut State,
    field: &mut String,
    row: &mut Vec<String>,
    rows: &mut Vec<Vec<String>>,
) {
    let end_record = |field: &mut String, row: &mut Vec<String>, rows: &mut Vec<Vec<String>>| {
        row.push(std::mem::take(field));
        // Completed records keep only their fields, not Vec's spare growth slots.
        row.shrink_to_fit();
        rows.push(std::mem::take(row));
    };
    match state {
        State::StartRecord => unreachable!("handled by the caller"),
        State::StartField => {
            if c == '\n' {
                end_record(field, row, rows);
                *state = State::StartRecord;
            } else if c == dialect.quotechar {
                *state = State::InQuotedField;
            } else if c == ' ' && dialect.skipinitialspace {
                // Stay in StartField, swallowing the padding.
            } else if c == dialect.delimiter {
                row.push(std::mem::take(field));
            } else {
                field.push(c);
                *state = State::InField;
            }
        }
        State::InField => {
            if c == '\n' {
                end_record(field, row, rows);
                *state = State::StartRecord;
            } else if c == dialect.delimiter {
                row.push(std::mem::take(field));
                *state = State::StartField;
            } else {
                field.push(c);
            }
        }
        State::InQuotedField => {
            if c == dialect.quotechar {
                *state = if dialect.doublequote {
                    State::QuoteInQuotedField
                } else {
                    // Without doublequote the quote simply ends the quoted part;
                    // anything after it, quotes included, is literal.
                    State::InField
                };
            } else {
                field.push(c);
            }
        }
        State::QuoteInQuotedField => {
            if c == dialect.quotechar {
                field.push(c);
                *state = State::InQuotedField;
            } else if c == dialect.delimiter {
                row.push(std::mem::take(field));
                *state = State::StartField;
            } else if c == '\n' {
                end_record(field, row, rows);
                *state = State::StartRecord;
            } else {
                field.push(c);
                *state = State::InField;
            }
        }
    }
}

/// Whether `value` matches rich-cli's `is_number`, i.e. Python's
/// `re.fullmatch(r"\-?[0-9]*?\.?[0-9]*?", value)`.
///
/// Note how loose that is: `-`, `.`, `-.` and the empty string all pass, and a
/// leading space does not. The caller only asks about non-empty cells.
fn is_number(value: &str) -> bool {
    let body = value.strip_prefix('-').unwrap_or(value);
    let (integer, fraction) = match body.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (body, None),
    };
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    digits(integer) && fraction.is_none_or(digits)
}

/// Build the CSV table for `content`, sniffing the dialect and the header the
/// way upstream's `render_csv` does.
///
/// `fallback_delimiter` is the dialect to use when `csv.Sniffer` raises, and
/// `None` means upstream has none to offer: its `except csv.Error` arm falls
/// back to `csv.get_dialect("excel")` for a resource whose name ends `.csv`
/// and to `"excel-tab"` for one ending `.tsv`, and for **everything else** —
/// any other extension, a bare name, a URL, stdin — calls
/// `on_error(str(error))`, which prints the message and exits non-zero.
///
/// So `None` here is a failure, and the caller must report it. Rendering a
/// fabricated one-column table at exit 0 instead is the worst possible
/// outcome: `rich --csv "$f" && publish` proceeds on a file nothing could
/// parse, with empty stderr to say so.
fn build_csv_table(
    content: &str,
    fallback_delimiter: Option<char>,
    title: Option<&str>,
    caption: Option<&str>,
) -> Option<Table> {
    // The sniffer sees only the first 1024 *characters* — upstream's
    // `csv_data[:1024]` — however long the file is.
    let sample: String = content.chars().take(1024).collect();
    let sniffed = sniff(&sample, Some(&[',', '\t', '|', ';'])).zip(has_header(&sample));
    let (dialect, header) = match sniffed {
        Some(sniffed) => sniffed,
        None => (Dialect::excel(fallback_delimiter?), true),
    };
    Some(render_csv(
        read_csv_rows(content, &dialect),
        header,
        title,
        caption,
    ))
}

/// The dialect upstream falls back to when `csv.Sniffer` cannot read
/// `resource`, or `None` when it has none — a failure, not a default.
///
/// Upstream tests the **resource string**, not the detected language:
/// `resource.lower().endswith(".csv")` then `.endswith(".tsv")`. So a URL
/// ending `.tsv` gets the tab dialect, and `-` (stdin) gets nothing.
fn csv_fallback_delimiter(resource: Option<&str>) -> Option<char> {
    let name = resource.unwrap_or_default().to_lowercase();
    if name.ends_with(".csv") {
        Some(',')
    } else if name.ends_with(".tsv") {
        Some('\t')
    } else {
        None
    }
}

/// Build a table from parsed CSV `rows`, mirroring rich-cli's `render_csv`: a
/// blue border, `HEAVY_HEAD` when the sniffer found a header and `SQUARE` when
/// it did not, and any all-numeric column right-justified with bold-green body
/// and header cells.
fn render_csv(
    rows: Vec<Vec<String>>,
    has_header: bool,
    title: Option<&str>,
    caption: Option<&str>,
) -> Table {
    let mut table = Table::new()
        .border_style(Style::parse("blue").expect("valid style"))
        .show_header(has_header)
        .box_set(if has_header { HEAVY_HEAD } else { SQUARE });
    if let Some(title) = title {
        table = table.title(title);
    }
    if let Some(caption) = caption {
        table = table.caption(caption);
    }

    let empty: Vec<String> = Vec::new();
    let (header, body) = if has_header {
        match rows.split_first() {
            Some((header, body)) => (header, body),
            None => return table,
        }
    } else {
        (&empty, rows.as_slice())
    };
    // `[row for row in rows if row]`: a blank line is not a table row.
    let data: Vec<&Vec<String>> = body.iter().filter(|row| !row.is_empty()).collect();

    // A row may carry more fields than the header names, and upstream's
    // `table.add_row(*row)` grows the table to hold them. Columns came from the
    // header alone, so those fields had nowhere to go and were dropped -- a
    // silent data loss in a tool whose job is showing you the file.
    let widest = data.iter().map(|row| row.len()).max().unwrap_or(0);
    let columns = header.len().max(widest);

    for index in 0..columns {
        // A column is numeric when no data cell is a non-empty non-number. A
        // row too SHORT to reach the column disqualifies it (upstream's
        // `except Exception: break`), while an empty cell does not; and an
        // empty data set counts as numeric, as upstream's `for … else` does.
        let numeric = data.iter().all(|row| match row.get(index) {
            Some(value) => value.is_empty() || is_number(value),
            None => false,
        });
        let name = header.get(index).map(String::as_str).unwrap_or("");
        if numeric {
            table.add_column_justify(name, Justify::Right);
            table.column_style(Style::parse("bold green").expect("valid style"));
            table.column_header_fill(Style::parse("bold green").expect("valid style"));
        } else {
            table.add_column(name);
        }
    }
    // Measurement/style inference is complete. Move the parsed cells into the
    // table rather than retaining and cloning the complete input row set.
    drop(data);
    table.extend_owned_rows(
        rows.into_iter()
            .skip(usize::from(has_header))
            .filter(|row| !row.is_empty())
            .collect(),
    );
    table
}

/// Join a notebook cell/output `source` field (a JSON string, or an array of
/// line strings that already include their trailing newlines).
fn join_source(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => String::new(),
    }
}

/// Join a notebook `traceback`, whose elements are lines WITHOUT trailing
/// newlines (unlike `source`/`text`, which carry their own). Using
/// [`join_source`] here fused a whole stack trace onto one line.
fn join_traceback(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(
                "
",
            ),
        _ => String::new(),
    }
}

/// Build an `In [n]:` / `Out[n]:` execution-count label. Port of rich-cli's
/// `[green]In [[#66ff00]{count}[/#66ff00]]:[/green]` markup (built as spans to
/// avoid markup-escaping the literal brackets).
fn io_label(word: &str, count: Option<i64>, base: &str, number: &str) -> Text {
    let base_style = Style::parse(base).ok();
    let number_style = Style::parse(number).ok();
    let n = count.map_or_else(|| " ".to_string(), |c| c.to_string());
    let mut text = Text::new("");
    text.append(&format!("{word}["), base_style.clone().map(Into::into));
    text.append(&n, number_style.map(Into::into));
    text.append("]:", base_style.map(Into::into));
    text
}

/// Port of the Group returned by rich-cli's `render_ipynb`. Keeping the cells
/// composable lets the common padding/panel/style/width/alignment chain apply.
struct Notebook {
    items: Vec<Box<dyn Renderable>>,
    // Console.print(justify=...) also reaches the group's Text children in
    // upstream. Their individual lines align within the group's measured width.
    justify: Option<Justify>,
}

impl Renderable for Notebook {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut options = options.clone();
        if let Some(justify) = self.justify {
            options.justify = justify;
        }
        let mut segments = Vec::new();
        for item in &self.items {
            let rendered = item.rich_render(console, &options);
            if !rendered.is_empty() {
                segments.extend(rendered);
                segments.push(Segment::line());
            }
        }
        // Console supplies the final newline. Empty groups produce no output.
        if !segments.is_empty() {
            segments.pop();
        }
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.items
            .iter()
            .map(|item| item.measure(console, options))
            .fold(Measurement::new(0, 0), |width, item| {
                Measurement::new(
                    width.minimum.max(item.minimum),
                    width.maximum.max(item.maximum),
                )
            })
    }
}

/// Text.from_ansi joins decoded lines, preserving their style spans.
fn notebook_ansi(source: &str) -> Text {
    let mut result = Text::new("");
    for (index, line) in AnsiDecoder::new().decode(source).iter().enumerate() {
        if index > 0 {
            result.append("\n", None);
        }
        result = result.append_text(line);
    }
    result
}

/// Port of rich-cli 1.8.1's notebook cell/output grouping. Parse once, then use
/// the same renderable for terminal output, paging and recorded exports.
fn build_ipynb(
    notebook: &serde_json::Value,
    hyperlinks: bool,
    justify: Option<Justify>,
) -> Notebook {
    let language = notebook["metadata"]["kernelspec"]["language"]
        .as_str()
        .or_else(|| notebook["metadata"]["language_info"]["name"].as_str())
        .unwrap_or("python");
    let empty = Vec::new();
    let mut items: Vec<Box<dyn Renderable>> = Vec::new();
    let mut new_line = true;
    for cell in notebook["cells"].as_array().unwrap_or(&empty) {
        if new_line {
            items.push(text(""));
        }
        if cell.get("execution_count").is_some() {
            items.push(Box::new(io_label(
                "In ",
                cell["execution_count"].as_i64().filter(|n| *n != 0),
                "green",
                "#66ff00",
            )));
        }
        let source = join_source(&cell["source"]);
        items.push(match cell["cell_type"].as_str().unwrap_or("") {
            "markdown" => Box::new(build_markdown(&source, hyperlinks)),
            "code" => Box::new(
                Panel::new(Box::new(Syntax::new(&source, language)))
                    .border_style(Style::parse("dim").expect("valid style")),
            ),
            _ => text(&source),
        });
        new_line = true;
        for output in cell["outputs"].as_array().unwrap_or(&empty) {
            let rendered = match output["output_type"].as_str().unwrap_or("") {
                "stream" => {
                    new_line = false;
                    notebook_ansi(&join_source(&output["text"]))
                }
                "error" => {
                    new_line = true;
                    notebook_ansi(join_traceback(&output["traceback"]).trim_end())
                }
                "execute_result" => {
                    new_line = true;
                    let mut label = io_label(
                        "Out",
                        output["execution_count"].as_i64().filter(|n| *n != 0),
                        "red",
                        "#ee4b2b",
                    );
                    label.append("\n", None);
                    label.append_text(&notebook_ansi(&join_source(&output["data"]["text/plain"])))
                }
                _ => continue,
            };
            items.push(Box::new(rendered));
        }
    }
    Notebook { items, justify }
}

fn diff_threshold_exceeded(changed: f32, limit: f32) -> bool {
    // Use the formatter's rounding for BOTH displayed operands, including
    // ties. f32::round uses a different tie rule from the report's formatting.
    let displayed = |value: f32| {
        format!("{value:.1}")
            .parse::<f32>()
            .expect("formatted percentage is a number")
    };
    displayed(changed) > displayed(limit)
}

/// `rich diff` for anything but an image pair: text, ANSI captures and
/// source side by side or unified, or one patch (`git diff | rich diff -`).
fn run_text_diff(cli: &Cli, console: &Console, export: &Export) -> ExitCode {
    let mut contents = Vec::with_capacity(cli.resources.len());
    for resource in &cli.resources {
        let content = if is_url(resource) {
            fetch_url(resource, cli.extensions.encoding).map(|(content, _)| content)
        } else {
            read_resource(Some(resource), cli.extensions.encoding)
                .map_err(|err| format!("cannot read {resource}: {err}"))
        };
        match content {
            Ok(content) => contents.push(content),
            Err(err) => return fail(cli, ExitClass::Input, err),
        }
    }
    let names: Vec<String> = cli
        .resources
        .iter()
        .map(|r| {
            if r == "-" {
                "<stdin>".to_string()
            } else {
                r.clone()
            }
        })
        .collect();
    let outcome = match tools::text_diff(&cli.tools, &names, &contents, cli.diff_threshold) {
        Ok(outcome) => outcome,
        Err(err) => return fail(cli, ExitClass::Data, err),
    };
    let fit = outcome
        .renderable
        .measure(console, &console.options())
        .maximum;
    let code = decorate_and_emit(cli, console, export, outcome.renderable, Some(fit));
    if code != ExitCode::SUCCESS || !outcome.failed {
        return code;
    }
    fail(cli, ExitClass::Gate, "diff threshold exceeded")
}

/// Compare two images perceptually and report where they differ.
///
/// Prints a heat map of the ΔE field plus a ranked table of changed regions.
/// The table is the point: it is text, so it survives a pipe, a log, and a CI
/// transcript, which a picture does not.
///
/// With `--threshold`, exits non-zero when the changed percentage exceeds it,
/// which is what makes this usable as a visual-regression gate.
/// Open an image file, translating a few common failure modes (a directory
/// instead of a file, an unrecognized extension) into one actionable message.
/// Shared by `--diff` (which loads two images) and `--image` (which loads
/// one) so neither reimplements the other's error handling.
///
/// The directory check reached the plain read path but not this one, so
/// `rich --diff a.png somedir` still reported "Access is denied".
#[cfg(feature = "art")]
fn open_image_path(path: &str) -> Result<rich_art::image::DynamicImage, String> {
    if std::path::Path::new(path).is_dir() {
        return Err(format!("cannot read {path}: is a directory, not a file"));
    }
    match rich_art::image::open(path) {
        Ok(image) => Ok(image),
        Err(err) => {
            use rich_art::image::error::{ImageFormatHint, UnsupportedErrorKind};
            let hint = match &err {
                rich_art::image::ImageError::Unsupported(error) => match error.kind() {
                    UnsupportedErrorKind::Format(ImageFormatHint::PathExtension(ext)) =>
                        Some(format!("unsupported image extension {}; use a supported image format such as PNG or JPEG", ext.display())),
                    _ => None,
                },
                _ => None,
            };
            Err(format!(
                "cannot read {path}: {}",
                hint.unwrap_or_else(|| err.to_string())
            ))
        }
    }
}

/// Map the CLI's own `ImageMode` (which also carries `None`, meaning "print no
/// picture at all") onto `rich_art`'s `ImageMode` (which has no such variant ?
/// a facade that always draws something has no notion of "nothing"). Argument
/// parsing rejects `--image-mode none` together with `--image` before this can
/// ever see it.
#[cfg(feature = "art")]
fn to_art_image_mode(mode: ImageMode) -> rich_art::ImageMode {
    match mode {
        ImageMode::Auto => rich_art::ImageMode::Auto,
        ImageMode::Sixel => rich_art::ImageMode::Sixel,
        ImageMode::Blocks => rich_art::ImageMode::Blocks,
        ImageMode::Braille => rich_art::ImageMode::Braille,
        ImageMode::Quadrants => rich_art::ImageMode::Quadrants,
        ImageMode::Ascii => rich_art::ImageMode::Ascii,
        ImageMode::None => unreachable!("--image-mode none is rejected during argument parsing"),
    }
}

/// Render one still image through rich-art's [`rich_art::ImageArt`] facade.
///
/// This function only opens the file and maps CLI flags onto the facade's
/// builder; picking a backend, sizing and colour handling all stay inside
/// [`rich_art::ImageArt`]; nothing here reimplements that logic (see
/// AGENTS.md's mirror/ext boundary).
#[cfg(feature = "art")]
fn run_image(cli: &Cli, console: &Console, export: &Export) -> ExitCode {
    use rich_art::ImageArt;

    let Some(resource) = cli.resource.as_deref() else {
        return fail(
            cli,
            ExitClass::Usage,
            "--image requires an image file: rich --image photo.png (or `-` for stdin)",
        );
    };

    let image = if resource == "-" {
        let mut buffer = Vec::new();
        if let Err(err) = std::io::stdin().read_to_end(&mut buffer) {
            return fail(cli, ExitClass::Input, format!("cannot read stdin: {err}"));
        }
        match rich_art::image::load_from_memory(&buffer) {
            Ok(image) => image,
            Err(err) => {
                return fail(
                    cli,
                    ExitClass::Input,
                    format!("cannot decode image from stdin: {err}"),
                )
            }
        }
    } else {
        match open_image_path(resource) {
            Ok(image) => image,
            Err(message) => return fail(cli, ExitClass::Input, message),
        }
    };

    let width = cli.width.unwrap_or_else(|| console.width());
    let mut art = ImageArt::new(image)
        .transforms(rich_art::ImageTransforms {
            rotation: match cli.image_rotate {
                Some(90) => rich_art::Rotation::Clockwise90,
                Some(180) => rich_art::Rotation::Clockwise180,
                Some(270) => rich_art::Rotation::Clockwise270,
                _ => rich_art::Rotation::None,
            },
            flip_horizontal: cli.image_flip_horizontal,
            flip_vertical: cli.image_flip_vertical,
            grayscale: cli.image_grayscale,
            brightness: cli.image_brightness.unwrap_or(1.0),
            contrast: cli.image_contrast.unwrap_or(1.0),
            gamma: cli.image_gamma.unwrap_or(1.0),
        })
        .mode(to_art_image_mode(cli.image_mode))
        .width(width)
        .color(!cli.no_color);
    if let Some(height) = cli.height {
        art = art.height(height);
    }
    if let Some(columns) = cli.image_max_width {
        art = art.max_width(columns);
    }
    if let Some(rows) = cli.image_max_height {
        art = art.max_height(rows);
    }

    if let Some(fit) = cli.image_fit.as_deref() {
        art = art.fit(match fit {
            "cover" => rich_art::ImageFit::Cover,
            "stretch" => rich_art::ImageFit::Stretch,
            _ => rich_art::ImageFit::Contain,
        });
    }
    if let Some(anchor) = cli.image_anchor.as_deref() {
        use rich_art::ImageAnchor;
        art = art.anchor(match anchor {
            "top" => ImageAnchor::Top,
            "bottom" => ImageAnchor::Bottom,
            "left" => ImageAnchor::Left,
            "right" => ImageAnchor::Right,
            "top-left" => ImageAnchor::TopLeft,
            "top-right" => ImageAnchor::TopRight,
            "bottom-left" => ImageAnchor::BottomLeft,
            "bottom-right" => ImageAnchor::BottomRight,
            _ => ImageAnchor::Center,
        });
    }
    if let Some(background) = cli.image_background {
        art = art.background(background);
    }
    if let Some(color) = cli.image_color.as_deref() {
        art = art.color_mode(match color {
            "ansi256" => rich_art::ImageColorMode::Ansi256,
            "ansi16" => rich_art::ImageColorMode::Ansi16,
            "grayscale" => rich_art::ImageColorMode::Grayscale,
            _ => rich_art::ImageColorMode::TrueColor,
        });
    }
    if let Some(dither) = cli.image_dither.as_deref() {
        art = art.dither(match dither {
            "floyd-steinberg" => rich_art::Dither::FloydSteinberg,
            "bayer4x4" => rich_art::Dither::Bayer4x4,
            _ => rich_art::Dither::None,
        });
    }

    // Validate up front: `Renderable::rich_render` cannot fail and would
    // silently fall back to ASCII, which is the wrong answer for a CLI that
    // promised a clear error (e.g. `--image-mode sixel` on a build without
    // the `sixel` feature, or an image Sixel cannot encode).
    let options = console.options();
    if let Err(err) = art.render(console, &options) {
        return fail(cli, ExitClass::Input, err.to_string());
    }

    let terminal_only = Export {
        html_path: None,
        svg_path: None,
        ..*export
    };
    if let Err(message) = emit(console, &terminal_only, |c| c.print(&art)) {
        return fail(cli, ExitClass::Input, message);
    }
    if export.html_path.is_some() || export.svg_path.is_some() {
        use rich::protocol::RenderEnvironment;
        use rich_ext::target::{RenderTarget, TargetKind, TargetOverrides};
        let mut caps = render_target::observe(console, TargetOverrides::default()).capabilities;
        caps.color_system = if cli.no_color {
            None
        } else {
            Some(ColorSystem::Truecolor)
        };
        let target = RenderTarget::new(TargetKind::Html, caps, console.theme().clone());
        let export_console = target.console();
        if let Err(err) =
            art.render_with_environment(&export_console, &export_console.options(), &target)
        {
            return fail(cli, ExitClass::Input, err.to_string());
        }
        let segments = target.segments(&art);
        debug_assert!(!target.capabilities().interactive);
        if let Err(message) = save_exports(&export_console, export, &segments) {
            return fail(cli, ExitClass::Input, message);
        }
    }
    success(cli)
}

#[cfg(feature = "art")]
fn run_diff(cli: &Cli, console: &Console, export: &Export) -> ExitCode {
    use rich::Table;
    use rich_art::imagediff::{diff, DiffSettings};
    use rich_art::{AsciiArt, BlockArt, BrailleArt, SixelArt};

    let (before_path, after_path) = (&cli.resources[0], &cli.resources[1]);
    let before = match open_image_path(before_path) {
        Ok(image) => image,
        Err(message) => return fail(cli, ExitClass::Input, message),
    };
    let after = match open_image_path(after_path) {
        Ok(image) => image,
        Err(message) => return fail(cli, ExitClass::Input, message),
    };

    let report = match diff(&before, &after, &DiffSettings::default()) {
        Ok(report) => report,
        Err(err) => return fail(cli, ExitClass::Data, err.to_string()),
    };

    let width = cli.width.unwrap_or_else(|| console.width());
    let changed = report.changed_fraction * 100.0;
    let naive = report.naive_changed_fraction * 100.0;

    // Decided before rendering so the verdict can be part of the output (and
    // therefore part of an --export-svg capture), while the exit code is
    // settled outside the closure.
    // Compare the number that is PRINTED, not the one behind it. Comparing full
    // precision against a one-decimal display produced "FAIL 5.4% changed, limit
    // 5.4%" -- a verdict that contradicts itself and cannot be explained from
    // the output, which is what anyone tuning a threshold to the reported figure
    // runs straight into.
    let failed = cli
        .diff_threshold
        .is_some_and(|limit| diff_threshold_exceeded(changed, limit));

    let render_report = |c: &Console, for_export: bool| {
        // Leave room for the summary, the table and the prompt, so the top of
        // the picture is not scrolled off before the reader sees it.
        let rows_cap = console.height().saturating_sub(14).clamp(6, 30);

        // Resolve `auto` from what this console can ACTUALLY do, not from the
        // --no-color flag alone. Redirected output has no colour, and a
        // half-block render without colour is a solid rectangle of identical
        // characters -- 30 rows carrying no information. Sixel is a control
        // sequence, so it is dropped entirely when the destination is not a
        // terminal.
        let has_color = c.color_system().is_some() && !cli.no_color;
        let is_terminal = c.is_terminal();
        let mut mode = cli.image_mode;
        if mode == ImageMode::Auto {
            if !has_color && !for_export && cli.report_format == ReportFormat::Human {
                eprintln!("rich: auto image mode has no terminal colour, drawing the diff as ASCII instead");
            }
            mode = if !has_color {
                ImageMode::Ascii
            } else if is_terminal && rich_art::sixel::is_probably_supported() {
                ImageMode::Sixel
            } else {
                ImageMode::Blocks
            };
        }

        // An explicit choice still has to produce something. Downgrade rather
        // than emit a rectangle of identical blocks or nothing at all, and say
        // why on stderr so the change is visible rather than mysterious.
        if matches!(mode, ImageMode::Blocks | ImageMode::Quadrants) && !has_color {
            if !for_export && cli.report_format == ReportFormat::Human {
                eprintln!("rich: no colour available, drawing the diff as ASCII art");
            }
            mode = ImageMode::Ascii;
        }
        if mode == ImageMode::Sixel && !is_terminal {
            if cli.image_mode != ImageMode::Sixel
                && !for_export
                && cli.report_format == ReportFormat::Human
            {
                eprintln!(
                    "rich: Sixel graphics need a terminal, drawing the diff as {} instead",
                    if has_color { "blocks" } else { "ASCII art" }
                );
            }
            mode = if has_color {
                ImageMode::Blocks
            } else {
                ImageMode::Ascii
            };
        }

        match mode {
            ImageMode::None => {}
            ImageMode::Ascii => c.print(
                &AsciiArt::new(report.heatmap())
                    .width(width)
                    .color(has_color),
            ),
            ImageMode::Blocks => c.print(
                &BlockArt::new(report.heatmap())
                    .width(width)
                    .height(rows_cap),
            ),
            ImageMode::Quadrants => c.print(
                &rich_art::QuadrantArt::new(report.heatmap())
                    .width(width)
                    .height(rows_cap),
            ),
            ImageMode::Braille => c.print(
                &BrailleArt::new(report.heatmap())
                    .width(width)
                    .height(rows_cap),
            ),
            ImageMode::Sixel => {
                let art = SixelArt::new(report.heatmap())
                    .width(width)
                    .height(rows_cap);
                // Encoding can still fail on a terminal; the report must stay
                // useful, so fall back rather than leaving an empty gap.
                if art.encode(width).is_some() {
                    c.print(&art);
                } else {
                    if cli.report_format == ReportFormat::Human {
                        eprintln!("rich: could not encode Sixel, drawing the diff as blocks");
                    }
                    c.print(
                        &BlockArt::new(report.heatmap())
                            .width(width)
                            .height(rows_cap),
                    );
                }
            }
            ImageMode::Auto => unreachable!("resolved above"),
        }
        // The blank line separates the picture from the summary, so it belongs
        // to the picture — with --image-mode none it was just a stray first
        // line in every report and every redirected file.
        if mode != ImageMode::None {
            c.print_str("");
        }
        c.print_str(&format!(
            "[bold]{changed:.1}%[/] of the canvas changed perceptibly \
             [dim](a plain pixel diff would say {naive:.1}%)[/]"
        ));

        if report.regions.is_empty() {
            c.print_str("[dim]No region large enough to report.[/]");
        } else {
            let mut table = Table::new().title("Where it changed");
            table.add_column("#");
            table.add_column_justify("Share", rich::Justify::Right);
            table.add_column_justify("Mean ΔE", rich::Justify::Right);
            table.add_column_justify("Area px", rich::Justify::Right);
            table.add_column("Box (x,y w×h)");
            for (rank, r) in report.regions.iter().enumerate() {
                table.add_row(&[
                    &format!("{}", rank + 1),
                    &format!("{:.0}%", r.share_of_change * 100.0),
                    &format!("{:.1}", r.mean_delta_e),
                    &format!("{}", r.area_px),
                    &format!("{},{} {}×{}", r.x, r.y, r.width, r.height),
                ]);
            }
            c.print(&table);
        }

        // The gate is compared against the perceptual figure, never the naive
        // one — gating on a pixel diff is what makes visual regression testing
        // useless in the first place.
        if let Some(limit) = cli.diff_threshold {
            if failed {
                c.print_str(&format!(
                    "[bold red]FAIL[/] {changed:.1}% changed, limit {limit:.1}%"
                ));
            } else {
                c.print_str(&format!(
                    "[bold green]OK[/] {changed:.1}% changed, within {limit:.1}%"
                ));
            }
        }
    };

    let terminal_only = Export {
        html_path: None,
        svg_path: None,
        ..*export
    };
    let mut wrote = emit(console, &terminal_only, |c| render_report(c, false));
    if export.html_path.is_some() || export.svg_path.is_some() {
        // HTML/SVG support truecolor independently of stdout. Render the already
        // loaded report for that destination; never reread images or recompute
        // the comparison. A nonterminal console prevents Sixel control data.
        let mut export_console = Console::builder()
            .force_terminal(false)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(cli.no_color)
            .width(console.width())
            .height(console.height())
            .theme(console.theme().clone())
            .build();
        export_console.install_extensions();
        let segments = export_console.record_output(|c| render_report(c, true));
        wrote = wrote.and_then(|()| save_exports(&export_console, export, &segments));
    }

    if let Err(message) = wrote {
        return fail(cli, ExitClass::Input, message);
    }
    if failed {
        return fail(cli, ExitClass::Gate, "diff threshold exceeded");
    }
    success(cli)
}

/// Animate every `--gif` resource at once, sharing the console width.
#[cfg(feature = "art")]
fn play_gifs(cli: &Cli, console: &Console) -> ExitCode {
    use rich_art::{AnimatedArt, Repeat, Stage};

    if cli.resources.is_empty() {
        return fail(cli, ExitClass::Usage, "--gif needs at least one GIF path");
    }
    let count = cli.resources.len();
    const GAP: usize = 2;

    // Share the width between the animations, leaving room for the gaps.
    let total = cli.width.unwrap_or_else(|| console.width());
    let per_gif = total
        .saturating_sub(GAP * count.saturating_sub(1))
        .checked_div(count)
        .unwrap_or(total)
        .max(8);

    let repeat = match cli.loops {
        Some(0) => Repeat::Forever,
        Some(n) => Repeat::Times(n),
        None => Repeat::Times(1),
    };

    let mut stage = Stage::new().gap(GAP);
    for path in &cli.resources {
        match AnimatedArt::from_path(path) {
            Ok(art) => {
                stage = stage.with(
                    art.width(per_gif)
                        .blocks(cli.extensions.gif_blocks())
                        .color(!cli.no_color)
                        .repeat(repeat)
                        // Colour art is byte-heavy; keep it comfortable.
                        .max_fps(30.0),
                );
            }
            Err(err) => {
                return fail(cli, ExitClass::Input, format!("cannot read {path}: {err}"));
            }
        }
    }

    if !console.is_terminal() && cli.report_format == ReportFormat::Human {
        eprintln!("rich: GIF animation needs a terminal; rendering the first frame only");
    }

    // `play` needs its own console (it moves into the Live display).
    let mut builder = Console::builder().no_color(cli.no_color);
    if !cli.theme_styles.is_empty() {
        builder = builder.theme(cli_theme(cli));
    }
    // Parallel workers spool output, but render for the parent's destination.
    // Pager policy still checks the real stdout handle before starting a pager.
    if let Ok(terminal) = std::env::var("RS_RICH_BATCH_TERMINAL") {
        builder = builder.force_terminal(terminal == "1");
    }
    if let Some(width) = cli.width {
        builder = builder.width(width);
    }
    match stage.play_stdout(builder.build()) {
        Ok(()) => success(cli),
        Err(err) => fail(cli, ExitClass::Input, format!("playback failed: {err}")),
    }
}

/// Without the `art` feature there is no GIF support.
#[cfg(not(feature = "art"))]
fn play_gifs(_cli: &Cli, _console: &Console) -> ExitCode {
    fail(
        _cli,
        ExitClass::Usage,
        "this build has no GIF support (rebuild with the `art` feature)",
    )
}

/// Where a render action's output goes: straight to the terminal, or captured
/// into a self-contained HTML or SVG document.
struct Export<'a> {
    /// `--export-html PATH`, if given.
    html_path: Option<&'a str>,
    /// `--export-svg PATH`, if given.
    svg_path: Option<&'a str>,
    /// Document title for the SVG frame (the resource's file name).
    svg_title: &'a str,
    /// `--pager`: page the terminal output instead of writing it straight out.
    pager: bool,
    auto_pager: bool,
}

/// Render once and deliver it everywhere it was asked for.
///
/// The exports do **not** replace the terminal output — upstream prints the
/// resource *and* saves the files, and both `--export-html` and `--export-svg`
/// may be given together. So when either is set the render is recorded once and
/// the same segments are turned into terminal bytes, HTML and SVG. Diff reports
/// use separate destination capabilities after loading/comparing the images;
/// ordinary input renderables are recorded once here.
///
/// Returns false if a file could not be written, so the caller can exit non-zero.
fn should_page(
    explicit: bool,
    automatic: bool,
    terminal: bool,
    lines: usize,
    height: usize,
) -> bool {
    terminal && (explicit || (automatic && lines > height.saturating_sub(1)))
}

fn emit(console: &Console, export: &Export, render: impl FnOnce(&Console)) -> Result<(), String> {
    if watch::capturing() {
        // A live watch region repaints these segments itself.
        watch::capture_segments(console.record_output(render));
        return Ok(());
    }
    if export.html_path.is_none()
        && export.svg_path.is_none()
        && !export.pager
        && !export.auto_pager
    {
        render(console);
        return Ok(());
    }

    let segments = console.record_output(render);
    let mut first_error = None;

    // The terminal still gets the output, exports or not.
    let terminal = console.segments_to_string(&segments);
    if should_page(
        export.pager,
        export.auto_pager,
        std::io::stdout().is_terminal(),
        terminal.lines().count(),
        console.height(),
    ) {
        // The text is already rendered, so hand it straight to the pager
        // rather than re-rendering through `Console::page`.
        if let Err(err) = rich::pager::Pager::show(&rich::pager::SystemPager, &terminal) {
            first_error = Some(format!("cannot page output: {err}"));
        }
    } else {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        if let Err(err) = lock.write_all(terminal.as_bytes()) {
            first_error = Some(format!("could not write output: {err}"));
        }
    }

    save_exports(console, export, &segments).and(first_error.map_or(Ok(()), Err))
}

fn save_exports(console: &Console, export: &Export, segments: &[Segment]) -> Result<(), String> {
    let filtered: Vec<Segment> = segments.iter().filter(|s| !s.control).cloned().collect();
    let segments = filtered.as_slice();
    let mut first_error = None;
    if let Some(path) = export.html_path {
        // CSS-class stylesheet form, as upstream's `save_html` default.
        let html = rich::export::export_html_classes(segments, &DEFAULT_TERMINAL_THEME);
        if let Err(err) = std::fs::write(path, html) {
            first_error = Some(format!("failed to save HTML: {err}"));
        }
    }
    if let Some(path) = export.svg_path {
        let svg = rich::svg::export_svg(
            segments,
            &rich::SVG_EXPORT_THEME,
            export.svg_title,
            // Upstream derives this id by hashing Python reprs; a fixed one keeps
            // our output deterministic (DIVERGENCES #15).
            "rich-cli",
            console.width(),
        );
        if let Err(err) = std::fs::write(path, svg) {
            first_error.get_or_insert_with(|| format!("failed to save SVG: {err}"));
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// `--help`: the [`cli_spec`] help, through a stdout console.
fn print_help(no_color: bool) {
    cli_spec::print_help(no_color);
}

fn build_demo_console(no_color: bool) -> Console {
    // Force truecolor so the demo looks the same regardless of TERM — but only
    // when it is actually going to a terminal. Forcing it unconditionally wrote
    // escape sequences into a pipe, and made `--no-color` a no-op on this path
    // alone while every other mode honoured it.
    let to_terminal = std::io::stdout().is_terminal() && !no_color;
    let mut console = Console::builder()
        .force_terminal(to_terminal)
        .no_color(no_color)
        .color_system(if to_terminal {
            Some(ColorSystem::Truecolor)
        } else {
            None
        })
        // `[error]`/`[warning]`/`[info]` are rich-ext's additions, not upstream
        // styles — the core theme is a faithful 154-entry port.
        .theme(rich_ext::extended_theme())
        .build();
    console.install_extensions();
    console
}

fn run_demo(no_color: bool, delay: std::time::Duration) -> Console {
    let to_terminal = std::io::stdout().is_terminal() && !no_color;
    let console = build_demo_console(no_color);

    demo::section(&console, delay, "rs-rich-cli");
    console.print_str("[bold magenta]rs-rich-cli[/] — a Rust port of [italic]rich[/]");
    console.print_str(
        "Core renderables track Python rich 15.0.0; Rust extensions add more workflows.",
    );

    // Markup, color, theme, and the rich-ext highlighter.
    demo::section(&console, delay, "markup · color · theme · extension");
    console.print_str(
        "Styles:   [bold]bold[/] [dim]dim[/] [italic]italic[/] [underline]underline[/] [reverse]reverse[/]",
    );
    console.print_str(
        "Color:    [red]red[/] [green]green[/] [blue]blue[/] [#ff8800]#ff8800[/] [white on blue]on blue[/]",
    );
    console.print_str("Theme:    [error]error[/], [warning]warning[/], [info]info[/]");
    // Upstream's theme stack: a pushed theme applies until its guard drops.
    let mut themed = build_demo_console(no_color);
    let release =
        rich::Theme::from_styles([("release", "bold magenta")], true).expect("valid demo theme");
    themed.use_theme(release).print_str(
        "Stack:    [release]release[/] from a pushed theme, popped when its guard drops",
    );
    console.print_str("Extension: numbers like 3.14 and 2026 are auto-highlighted");
    // Built-in ReprHighlighter (highlight=true): auto-colors numbers, bools,
    // strings, paths, calls, etc.
    // A second console, so it needs the same terminal/colour decision as the
    // main one — building it with force_terminal(true) unconditionally was what
    // leaked escape sequences into a pipe after the demo itself stopped.
    let hl = Console::builder()
        .force_terminal(to_terminal)
        .no_color(no_color)
        .color_system(if to_terminal {
            Some(ColorSystem::Truecolor)
        } else {
            None
        })
        .highlight(true)
        .build();
    hl.print_str("Highlight:  result = func(42, True, None, '/usr/bin')");
    // ISO8601Highlighter — colors date/time/timezone fields.
    let mut stamp = Text::new("2023-06-15T13:45:30+02:00");
    ISO8601Highlighter::new().highlight(&mut stamp);
    console.print(&Text::new("ISO 8601:   ").append_text(&stamp));
    console.print_str("Emoji:     :rocket: :fire: :sparkles: :thumbs_up: :snake: :coffee:");

    // Rule variants (title alignment).
    demo::section(&console, delay, "rule");
    console.print(&Rule::line());
    demo::section(&console, delay, "centered");
    console.print(&Rule::new("left").align(HorizontalAlign::Left));
    console.print(&Rule::new("right").align(HorizontalAlign::Right));

    // Panels with different boxes.
    demo::section(&console, delay, "panel");
    console.print(&Panel::new(text("rounded box (default)")).title("rounded"));
    console.print(
        &Panel::new(text("square box"))
            .box_set(SQUARE)
            .title("square"),
    );
    console.print(
        &Panel::new(text("double box"))
            .box_set(DOUBLE)
            .title("double"),
    );
    console.print(
        &Panel::new(text("title + subtitle"))
            .title("top")
            .subtitle("bottom"),
    );
    // Legacy-Windows box substitution: a ROUNDED panel falls back to SQUARE.
    let legacy = Console::builder()
        .force_terminal(to_terminal)
        .no_color(no_color)
        .color_system(if to_terminal {
            Some(ColorSystem::Truecolor)
        } else {
            None
        })
        .width(console.width())
        .legacy_windows(true)
        .build();
    legacy.print(&Panel::new(text("rounded → square on legacy Windows")).title("legacy"));

    // Padding (shown inside a panel so the blank space is visible).
    demo::section(&console, delay, "padding");
    console.print(
        &Panel::new(Box::new(Padding::new(text("padded (1, 4)"), (1, 4, 1, 4)))).title("padding"),
    );

    // Styled — lay one style under an entire renderable.
    demo::section(&console, delay, "styled");
    console.print(&Styled::new(
        Box::new(Panel::new(text("green under the whole panel")).title("styled")),
        Style::parse("green").unwrap(),
    ));

    // Horizontal alignment (fills the console width).
    demo::section(&console, delay, "align");
    console.print(&Align::left(text("← left")));
    console.print(&Align::center(text("center")));
    console.print(&Align::right(text("right →")));

    // Text justify (via print(justify=…)).
    demo::section(&console, delay, "justify");
    console.print_justified("left justified", Justify::Left);
    console.print_justified("centered", Justify::Center);
    console.print_justified("right justified", Justify::Right);

    // Constrain — same panel, capped to 24 cells.
    demo::section(&console, delay, "constrain (width 24)");
    console.print(&Constrain::new(
        Box::new(Panel::new(text("constrained")).title("≤24")),
        Some(24),
    ));

    // Table.
    demo::section(&console, delay, "table");
    let mut table = Table::new().title("ported renderables");
    // A styled column and a fixed-width column (content wraps with ellipsis).
    table
        .add_column("Renderable")
        .column_style(Style::parse("cyan").unwrap());
    table.add_column("Module").column_width(12);
    table.add_column_justify("Parity", Justify::Center);
    for (renderable, module, parity) in [
        ("Text / markup", "text, markup", "✓"),
        ("Rule", "rule", "✓"),
        ("Panel", "panel", "✓"),
        ("Padding", "padding", "✓"),
        ("Align", "align", "✓"),
        ("Constrain", "constrain", "✓"),
        ("Columns", "columns", "✓"),
        ("Table", "table", "✓"),
        ("Tree", "tree", "✓"),
        ("ProgressBar", "progress_bar", "✓"),
    ] {
        table.add_row(&[renderable, module, parity]);
    }
    console.print(&table);

    // Columns — packs items into as many columns as fit the width.
    demo::section(&console, delay, "columns");
    let months = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    console.print(&Columns::new(
        months.iter().map(|m| m.to_string()).collect(),
    ));

    // Tree.
    demo::section(&console, delay, "tree");
    let mut tree = Tree::new("rs-rich-cli");
    let core = tree.add("crates/rich (core, mirrors rich 15.0.0)");
    core.add("color · style · text · console");
    core.add("box · rule · panel · padding · align");
    core.add("table · tree · wrap · filesize");
    tree.add("crates/rich-ext (our extensions)");
    tree.add("crates/rich-cli (this binary)");
    console.print(&tree);

    // Progress bars at a few completion levels (0 → 100%).
    demo::section(&console, delay, "progress bar");
    for pct in [0.0, 33.0, 66.0, 100.0] {
        console.print(&ProgressBar::new(100.0, pct).width(48));
    }

    // Progress display with upstream's default columns (description, bar,
    // percentage, time remaining), driven by a simulated clock so the frame is
    // deterministic: speeds and ETAs come from real task samples.
    demo::section(&console, delay, "progress");
    let clock = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0f64.to_bits()));
    let set_time =
        |seconds: f64| clock.store(seconds.to_bits(), std::sync::atomic::Ordering::SeqCst);
    let reader = clock.clone();
    let now = move || f64::from_bits(reader.load(std::sync::atomic::Ordering::SeqCst));
    let mut progress = Progress::new().clock(now.clone());
    let downloading = progress.add_task("Downloading", 100.0, 0.0);
    let processing = progress.add_task("Processing", 100.0, 0.0);
    progress.add_task("Waiting", 100.0, 0.0);
    set_time(4.0);
    progress.advance(downloading, 20.0);
    progress.advance(processing, 60.0);
    set_time(8.0);
    progress.advance(downloading, 30.0);
    progress.advance(processing, 40.0);
    console.print(&progress);

    // Time, rate and spinner columns on a simulated transfer.
    let mut transfer = Progress::new().clock(now).columns(vec![
        ProgressColumn::spinner(),
        ProgressColumn::Description,
        ProgressColumn::Bar,
        ProgressColumn::Download,
        ProgressColumn::TransferSpeed,
        ProgressColumn::TimeElapsed,
        ProgressColumn::time_remaining(),
    ]);
    let iso = transfer.add_task("ubuntu.iso", 4_700_000_000.0, 0.0);
    let notes = transfer.add_task("notes.txt", 42_000.0, 0.0);
    set_time(12.0);
    transfer.advance(iso, 1_200_000_000.0);
    transfer.advance(notes, 20_000.0);
    set_time(20.0);
    transfer.advance(iso, 900_000_000.0);
    transfer.advance(notes, 22_000.0);
    console.print(&transfer);

    // Progress with custom columns — description + bar + M-of-N counter.
    let mut mofn = Progress::new().columns(vec![
        ProgressColumn::Description,
        ProgressColumn::Bar,
        ProgressColumn::MofN,
    ]);
    mofn.add_task("Files", 8.0, 3.0);
    mofn.add_task("Chunks", 128.0, 128.0);
    console.print(&mofn);

    // Progress with a download column — completed/total in shared byte units.
    let mut download = Progress::new().columns(vec![
        ProgressColumn::Description,
        ProgressColumn::Bar,
        ProgressColumn::Download,
    ]);
    download.add_task("archive.tar", 10_000_000.0, 4_200_000.0);
    console.print(&download);

    // JSON — pretty-printed and highlighted.
    demo::section(&console, delay, "json");
    if let Ok(json) = Json::new(
        r#"{"port": "rich", "version": "15.0.0", "parity": true, "widgets": ["panel", "table", "tree"]}"#,
    ) {
        console.print(&json);
    }

    // Pretty — colorize a Rust value's Debug output (Rust-native rich.pretty).
    demo::section(&console, delay, "pretty");
    let value = vec![("panel", 1u32), ("table", 2), ("tree", 3)];
    console.print(&Pretty::new(&value));

    // Log records — upstream's LogRender grid: a repeated time is blanked.
    demo::section(&console, delay, "log");
    let log = LogRender::new().show_level(true);
    for (level, message) in [
        (LogLevel::Info, "server started on port 8080"),
        (LogLevel::Debug, "cache warm: 128 entries"),
        (LogLevel::Warn, "disk usage at 85%"),
        (LogLevel::Error, "connection reset by peer"),
    ] {
        console.print(&log.render(
            &console,
            Text::new(message),
            Some(Text::new("[12:00:00]")),
            level.text(),
            Some("main.rs"),
            Some(42),
            None,
        ));
    }

    // Traceback — render an error + its source chain (Rust-native rich.traceback).
    demo::section(&console, delay, "traceback");
    let cause = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file: config.toml");
    let err = std::io::Error::other(cause);
    console.print(&Traceback::new(&err));

    // Bar — a filled span within a range (eighth-block resolution).
    demo::section(&console, delay, "bar (range)");
    for (begin, end) in [(0.0, 100.0), (0.0, 62.0), (20.0, 80.0), (55.0, 100.0)] {
        console.print(&Bar::new(100.0, begin, end).width(48));
    }

    // Markdown — headings, inline styles, lists, block quotes, tables, and rules.
    demo::section(&console, delay, "markdown");
    console.print(&Markdown::new(
        "# Heading\n\nA paragraph with **bold**, *italic*, `code`, ~~struck~~ text, and a [link](https://example.com).\nTilde runs pair as upstream pairs them: a ~~~x~~~ b.\n\n- bullet item\n- another\n\n1. first\n2. second\n\n> a block quote\n\n| Name | Age |\n| :--- | ---: |\n| Alice | 30 |\n| Bob | 7 |\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n---",
    ));

    // Syntax highlighting (via syntect — functional, not byte-parity with rich).
    demo::section(&console, delay, "syntax");
    console.print(&Syntax::new(
        "fn main() {\n    let msg = \"hello, rich\";\n    println!(\"{msg}\");\n}",
        "rust",
    ));

    // Control codes — cursor/screen sequences (shown escaped, not executed).
    demo::section(&console, delay, "control codes");
    let show_escape = |label: &str, control: &Control| {
        // Escaped so the sequence is visible (and its `[` isn't read as markup).
        let escaped = control
            .as_str()
            .replace('\x1b', "\\x1b")
            .replace('\x07', "\\a")
            .replace('\r', "\\r");
        console.print(&Text::new(format!("  {label:>14}: {escaped}")));
    };
    show_escape("clear screen", &Control::clear());
    show_escape("home", &Control::home());
    show_escape("move (2,-1)", &Control::move_(2, -1));
    show_escape("move_to (3,4)", &Control::move_to(3, 4));
    show_escape("hide cursor", &Control::show_cursor(false));
    show_escape("bell", &Control::bell());
    // LiveRender — the in-place redraw sequence for a 3-line render.
    let live_render = LiveRender::new(text("a\nb\nc"));
    let _ = console.render_to_string(&live_render); // render once to record the shape
    show_escape("live refresh", &live_render.position_cursor());
    // Live — the full control stream for a two-frame in-place update (captured
    // to a buffer so the demo stays static instead of animating).
    let live_console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(20)
        .build();
    let mut live = Live::new(text("frame one"), live_console, Vec::<u8>::new());
    live.start();
    live.update(text("frame two"));
    live.stop();
    let stream = String::from_utf8_lossy(live.writer())
        .replace('\x1b', "\\x1b")
        .replace('\r', "\\r")
        .replace('\n', "\\n");
    console.print(&Text::new(format!("  {:>14}: {stream}", "live stream")));

    // ANSI decoder — parse a raw SGR string back into styled Text, then re-render.
    demo::section(&console, delay, "ansi decoder");
    let raw =
        "\x1b[1;31mred bold\x1b[0m \x1b[38;5;214morange\x1b[0m \x1b[3;4mitalic underline\x1b[0m";
    console.print(&Text::new(format!(
        "  input:  {}",
        raw.replace('\x1b', "\\x1b")
    )));
    let mut decoder = rich::AnsiDecoder::new();
    for line in decoder.decode(raw) {
        console.print(&Text::new("  output: ").append_text(&line));
    }

    // Capture / export — record a rendering to a buffer, then strip its styles.
    demo::section(&console, delay, "capture · export_text");
    let plain = console.export_text(|c| {
        c.print(&Panel::new(text("captured panel")).box_set(SQUARE));
    });
    console.print(&Text::new("  export_text (styles stripped):"));
    for line in plain.lines() {
        console.print(&Text::new(format!("    {line}")));
    }
    // export_html — the inline CSS a style compiles to (via `--export-html`).
    let html_rule = Style::parse("bold red")
        .unwrap()
        .get_html_style(&DEFAULT_TERMINAL_THEME);
    console.print(&Text::new(format!(
        "  export_html: [bold red] → <span style=\"{html_rule}\">"
    )));

    // Layout — split a region into ratioed rows/columns; Panel leaves expand
    // to fill their region (rendered here at a fixed size).
    demo::section(&console, delay, "layout");
    let panel_leaf = |title: &str, body: &str| {
        Layout::with_renderable(Box::new(
            Panel::new(text(body))
                .box_set(SQUARE)
                .title(title.to_string()),
        ))
    };
    let mut header = Layout::new();
    header.split_row(vec![
        panel_leaf("left", "pane A"),
        panel_leaf("right", "pane B"),
    ]);
    let mut root = Layout::new();
    root.split_column(vec![
        header,
        Layout::with_renderable(text("footer (fixed size 3)")).size(3),
    ]);
    let grid = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(48)
        .height(7)
        .build();
    for line in grid.export_text(|c| c.print(&root)).lines() {
        console.print(&Text::new(line));
    }

    // Spinners — a few frames of each built-in (animation needs a Live loop).
    demo::section(&console, delay, "spinner (frames)");
    for name in ["dots", "line", "arrow", "simpleDots"] {
        let spinner = Spinner::new(name);
        let frames: String = (0..6)
            .map(|i| console.render_to_string(&spinner.render(i as f64 * 0.15)))
            .collect::<Vec<_>>()
            .join(" ");
        console.print_str(&format!("  {name:>11}: {frames}"));
    }
    // Status — a spinner + message (green frame; animation needs a Live loop).
    let status = Status::new("Loading…").renderable().render(0.0);
    console.print(&Text::new("       status: ").append_text(&status));

    // CSV rendered as a table (blue border, numeric columns bold-green + right).
    demo::section(&console, delay, "csv");
    let csv = "Product,Qty,Price\nWidget,3,9.99\nGadget,12,19.50\nGizmo,1,4.25";
    if let Some(table) = build_csv_table(csv, Some(','), None, None) {
        console.print(&table);
    }

    // filesize.
    demo::section(&console, delay, "filesize");
    for bytes in [1u64, 999, 1_000, 1_500, 1_000_000, 1_500_000_000] {
        console.print_str(&format!("  {bytes:>13} → {}", filesize::decimal(bytes)));
    }
    console.print(&Rule::line());
    console
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::ColorSystem;

    #[cfg(feature = "art")]
    #[test]
    fn diff_threshold_uses_the_reports_tie_rounding() {
        // The report prints 0.2 against 0.2, then 0.8 against 0.7.
        assert!(!diff_threshold_exceeded(0.25, 0.24));
        assert!(diff_threshold_exceeded(0.75, 0.74));
        assert!(!diff_threshold_exceeded(0.75, 0.76));
    }

    #[test]
    fn read_csv_rows_handles_quotes_and_delimiters() {
        let comma = Dialect::excel(',');
        // Quoted field containing the delimiter, and `""` escaping.
        let rows = read_csv_rows("a,\"b,c\",d\n\"he said \"\"hi\"\"\",2\n", &comma);
        assert_eq!(
            rows,
            vec![
                vec!["a".to_string(), "b,c".to_string(), "d".to_string()],
                vec!["he said \"hi\"".to_string(), "2".to_string()],
            ]
        );
        // No trailing empty row after a final newline (`\r` is gone by now:
        // the reader applies universal newlines first).
        assert_eq!(read_csv_rows("x\ny\n", &comma), vec![vec!["x"], vec!["y"]]);
        // Tab delimiter.
        assert_eq!(
            read_csv_rows("a\tb", &Dialect::excel('\t')),
            vec![vec!["a", "b"]]
        );
        // A leading UTF-8 BOM is stripped, not glued to the first cell.
        assert_eq!(read_csv_rows("\u{feff}a,b", &comma), vec![vec!["a", "b"]]);
        // A blank line is an EMPTY record, as `csv.reader` reports it — upstream
        // then drops it, where a `[""]` row would have drawn a blank table row.
        assert_eq!(
            read_csv_rows("a,b\n\nc,d\n", &comma),
            vec![vec!["a", "b"], vec![], vec!["c", "d"]]
        );
        // `skipinitialspace` eats the padding after a delimiter, but only there.
        let padded = Dialect {
            skipinitialspace: true,
            ..Dialect::excel(',')
        };
        assert_eq!(
            read_csv_rows("a,  b , c", &padded),
            vec![vec!["a", "b ", "c"]]
        );
    }

    #[test]
    fn the_sniffer_finds_the_delimiter_and_the_header() {
        // Every expectation here was read out of CPython's own `csv.Sniffer`.
        let candidates = [',', '\t', '|', ';'];
        let sniffed =
            |sample: &str| sniff(sample, Some(&candidates)).map(|dialect| dialect.delimiter);
        assert_eq!(
            sniffed("name;age;city\nAlice;30;Paris\nBob;25;Lyon\n"),
            Some(';')
        );
        assert_eq!(sniffed("a|b|c\n1|2|3\n4|5|6\n"), Some('|'));
        assert_eq!(sniffed("a\tb\tc\n1\t2\t3\n4\t5\t6\n"), Some('\t'));
        assert_eq!(sniffed("a, b, c\n1, 2, 3\n4, 5, 6\n"), Some(','));
        // A quoted, multi-line cell: only the quote/delimiter scan can find the
        // comma here, because the line counts are inconsistent.
        assert_eq!(
            sniffed("name,bio\nAlice,\"line one\nline two\"\nBob,short\n"),
            Some(',')
        );
        // Ragged rows and a single column defeat the sniffer, exactly as they do
        // upstream — that is what the excel fallback is for.
        assert_eq!(sniffed("a,b,c\n1,2\n3,4,5,6\n"), None);
        assert_eq!(sniffed("alpha\nbeta\ngamma\n"), None);

        assert_eq!(has_header("name,age\nAlice,30\nBob,25\n"), Some(true));
        // A column mixing an int and a float must stay ONE type. The historical
        // `for thisType in [int, float, complex]` loop reads it as inconsistent
        // and loses the header.
        assert_eq!(has_header("price,note\n10,aa\n9.5,bbb\n"), Some(true));
        assert_eq!(has_header("1,2,3\n4,5,6\n7,8,9\n"), Some(false));
    }

    #[test]
    fn detects_urls() {
        assert!(is_url("http://example.com"));
        assert!(is_url("https://example.com/x"));
        // RFC 3986: the scheme is case-insensitive.
        assert!(is_url("HTTPS://example.com"));
        assert!(is_url("Http://example.com"));
        assert!(!is_url("example.com"));
        assert!(!is_url("./file.md"));
        assert!(!is_url("-"));
        assert!(!is_url("")); // must not panic on a short/empty resource
        assert!(!is_url("ht"));
        assert!(!is_url("ftp://example.com"));
    }

    #[test]
    fn uninformative_extensions_defer_to_content_type() {
        assert!(is_uninformative_ext("txt"));
        assert!(is_uninformative_ext("log"));
        assert!(!is_uninformative_ext("py"));
        assert!(!is_uninformative_ext("rs"));
    }

    #[test]
    fn resource_ext_uses_basename_without_query() {
        assert_eq!(resource_ext("a.md").as_deref(), Some("md"));
        assert_eq!(resource_ext("path/to/b.RS").as_deref(), Some("rs"));
        assert_eq!(
            resource_ext("https://x.com/c.py?raw=1").as_deref(),
            Some("py")
        );
        // Dots in the host must not be mistaken for an extension.
        assert_eq!(resource_ext("https://api.example.com/data"), None);
        assert_eq!(resource_ext("https://x.com/"), None);
        assert_eq!(resource_ext("noext"), None);
    }

    #[test]
    fn content_type_maps_mode_and_lexer() {
        assert_eq!(mime_of("text/html; charset=utf-8"), "text/html");
        assert_eq!(content_type_mode("text/markdown"), Some(Mode::Markdown));
        assert_eq!(
            content_type_mode("application/json; charset=utf-8"),
            Some(Mode::Json)
        );
        assert_eq!(content_type_mode("text/csv"), Some(Mode::Csv));
        assert_eq!(content_type_mode("text/html"), None); // -> syntax, not a mode
        assert_eq!(content_type_lexer("text/html"), Some("html"));
        assert_eq!(content_type_lexer("text/x-python"), Some("python"));
        assert_eq!(content_type_lexer("text/plain"), None);
    }

    #[test]
    fn join_source_handles_string_and_array() {
        use serde_json::json;
        assert_eq!(join_source(&json!("a\nb")), "a\nb");
        // Array elements already carry their trailing newlines; they concat.
        assert_eq!(join_source(&json!(["a\n", "b"])), "a\nb");
        assert_eq!(join_source(&json!(null)), "");
    }

    #[test]
    fn ipynb_extension_auto_detects() {
        assert_eq!(detect_mode(Some("notebook.ipynb")), Mode::Ipynb);
    }

    #[test]
    fn io_label_builds_execution_count() {
        // "In [1]:" — the plain text (styles aside) should read back exactly.
        assert_eq!(
            io_label("In ", Some(1), "green", "#66ff00").plain(),
            "In [1]:"
        );
        assert_eq!(io_label("Out", None, "red", "#ee4b2b").plain(), "Out[ ]:");
    }

    #[test]
    fn parse_padding_unpacks_like_upstream() {
        assert_eq!(parse_padding("2"), Ok((2, 2, 2, 2)));
        assert_eq!(parse_padding("1,2"), Ok((1, 2, 1, 2)));
        assert_eq!(parse_padding("1,2,3,4"), Ok((1, 2, 3, 4)));
        assert_eq!(parse_padding(" 1 , 2 "), Ok((1, 2, 1, 2)));
        assert!(parse_padding("1,2,3").is_err()); // 3 values not allowed
        assert!(parse_padding("x").is_err());
    }

    #[test]
    fn parse_box_maps_names() {
        assert!(matches!(parse_box("rounded"), Ok(Some(_))));
        assert!(matches!(parse_box("HEAVY"), Ok(Some(_))));
        // `none` is upstream's default and means NO panel, not `box.NONE`.
        assert!(matches!(parse_box("none"), Ok(None)));
        assert!(parse_box("bogus").is_err());
    }

    #[test]
    fn paging_policy_respects_viewport_and_redirection() {
        assert!(!should_page(true, true, false, 100, 24));
        assert!(!should_page(false, true, true, 23, 24));
        assert!(should_page(false, true, true, 24, 24));
        assert!(should_page(true, false, true, 1, 24));
        assert!(!should_page(false, false, true, 100, 24));
    }

    #[test]
    fn worker_arguments_preserve_resources_named_like_commands() {
        for source in [["--markdown", "json", "b.md"], ["markdown", "json", "b.md"]] {
            let args: Vec<String> = source.into_iter().map(str::to_string).collect();
            assert_eq!(worker_args(&args), ["--no-config", "--markdown"]);
        }
    }

    #[test]
    fn worker_arguments_keep_option_values_and_remove_batch_operands() {
        let args: Vec<String> = [
            "--batch",
            "--title",
            "--batch",
            "--jobs",
            "2",
            "json",
            "--export-html",
            "out.html",
            "--",
            "--pager",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(
            worker_args(&args),
            ["--no-config", "--title", "--batch", "--json"]
        );
    }

    #[test]
    fn parses_pager_flag() {
        let s = |v: &str| v.to_string();
        assert!(parse(&[s("--pager"), s("x")]).unwrap().unwrap().pager);
        assert!(!parse(&[s("x")]).unwrap().unwrap().pager);
    }

    #[test]
    fn parses_watch_options_and_rejects_orphans() {
        let s = |v: &str| v.to_string();
        let cli = parse(&[
            s("--watch"),
            s("--watch-interval"),
            s("0.25"),
            s("--watch-cache"),
            s("file.md"),
        ])
        .unwrap()
        .unwrap();
        assert!(cli.watch);
        assert_eq!(cli.watch_interval, 0.25);
        assert!(cli.watch_cache);
        assert!(parse(&[s("--watch-cache"), s("file.md")]).is_err());
        assert!(parse(&[s("--watch-interval"), s("2"), s("file.md")]).is_err());
    }

    #[test]
    fn parses_multi_file_watch_options() {
        let s = |v: &str| v.to_string();
        let parse = parse_inner;
        let cli = parse(&[
            s("--watch"),
            s("--watch-debounce"),
            s("0.3"),
            s("--watch-poll"),
            s("--watch-exit-on-error"),
            s("a.md"),
            s("b.json"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(cli.resources, ["a.md", "b.json"]);
        assert_eq!(cli.watch_debounce, 0.3);
        assert!(cli.watch_poll && cli.watch_exit_on_error);
        let default = parse(&[s("--watch"), s("a.md")]).unwrap().unwrap();
        assert_eq!(default.watch_debounce, watch::DEFAULT_DEBOUNCE_SECONDS);
        assert!(!default.watch_poll && !default.watch_exit_on_error);
        for orphan in [
            vec![s("--watch-debounce"), s("0.2"), s("a.md")],
            vec![s("--watch-poll"), s("a.md")],
            vec![s("--watch-exit-on-error"), s("a.md")],
        ] {
            assert!(parse(&orphan)
                .err()
                .unwrap()
                .contains("only has an effect with --watch"));
        }
        for bad in ["-0.1", "nan", "inf", "soon", "3601"] {
            assert!(parse(&[s("--watch"), s("--watch-debounce"), s(bad), s("a.md")]).is_err());
        }
        // Several resources remain an error without --watch (or --gif/--batch).
        assert!(parse(&[s("a.md"), s("b.md")]).is_err());
    }

    #[test]
    fn watch_fingerprint_distinguishes_missing_and_replaced_files() {
        let path = std::env::temp_dir().join(format!("rs-rich-watch-{}.txt", std::process::id(),));
        let _ = std::fs::remove_file(&path);
        let missing = watch_fingerprint(path.to_str().unwrap(), false, None);
        std::fs::write(&path, "first").unwrap();
        let first = watch_fingerprint(path.to_str().unwrap(), false, None);
        std::fs::write(&path, "second content").unwrap();
        let second = watch_fingerprint(path.to_str().unwrap(), false, None);
        let _ = std::fs::remove_file(&path);
        assert_eq!(missing, "missing");
        assert_ne!(first, missing);
        assert_ne!(first, second);
    }

    #[test]
    fn parses_batch_controls_and_cli_values_override_config_defaults() {
        let s = |v: &str| v.to_string();
        let cli = parse(&[
            s("--no-config"),
            s("--batch"),
            s("--jobs"),
            s("3"),
            s("-o"),
            s("out.html"),
            s("--continue-on-error"),
            s("--collision"),
            s("suffix"),
            s("input"),
        ])
        .unwrap()
        .unwrap();
        assert!(cli.batch);
        assert_eq!(cli.jobs, 3);
        assert!(cli.continue_on_error);
        assert_eq!(cli.collision, CollisionPolicy::Suffix);
    }

    #[test]
    fn batch_expansion_is_recursive_sorted_and_deduplicated() {
        let root = std::env::temp_dir().join(format!("rich-batch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("richbatchonly.md"), "# b").unwrap();
        std::fs::write(nested.join("richbatchnested.md"), "# a").unwrap();

        let root_arg = root.to_string_lossy().into_owned();
        let glob_arg = root
            .join("*richbatchonly.md")
            .to_string_lossy()
            .into_owned();
        let inputs = expand_batch_resources(&[root_arg, glob_arg]).unwrap();
        assert_eq!(inputs.len(), 2);
        assert!(inputs[0].ends_with("richbatchnested.md"));
        assert!(inputs[1].ends_with("richbatchonly.md"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_output_suffixes_are_deterministic_for_collisions() {
        let inputs = ["one.md", "two.md"];
        let first = batch_export_path(Some("out.html"), inputs[0], 0, 2).unwrap();
        let second = batch_export_path(Some("out.html"), inputs[1], 1, 2).unwrap();
        assert_eq!(first, "one.html");
        assert_eq!(second, "two.html");
        assert_eq!(
            Path::new("one.html")
                .with_file_name("one-2.html")
                .to_string_lossy(),
            "one-2.html"
        );
    }

    #[test]
    fn config_profiles_support_profiles_section_and_source_errors() {
        let root = std::env::temp_dir().join(format!("rich-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("config.toml");
        std::fs::write(&config, "[profiles.ci]\nmode = \"json\"\njobs = 4\n").unwrap();
        let args = vec![
            "--config".to_string(),
            config.to_string_lossy().into_owned(),
            "--profile".to_string(),
            "ci".to_string(),
        ];
        let merged = config_args(
            &args,
            &ConfigRoots {
                home: None,
                cwd: root.clone(),
            },
        )
        .unwrap();
        assert!(merged.windows(1).any(|arg| arg[0] == "--json"));
        assert!(merged
            .windows(2)
            .any(|pair| pair[0] == "--jobs" && pair[1] == "4"));

        let bad = root.join("bad.toml");
        std::fs::write(&bad, "[defaults]\nnot-a-setting = true\n").unwrap();
        let error = config_args(
            &["--config".to_string(), bad.to_string_lossy().into_owned()],
            &ConfigRoots {
                home: None,
                cwd: root.clone(),
            },
        )
        .unwrap_err();
        assert!(error.contains("config "));
        assert!(error.contains("unknown key"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_rejects_modes_that_consume_their_whole_resource_list() {
        let s = |v: &str| v.to_string();
        let error = parse(&[s("--no-config"), s("--batch"), s("--diff"), s("a"), s("b")])
            .err()
            .unwrap();
        assert!(error.contains("--batch cannot be combined with --diff"));
        let error = parse(&[s("--no-config"), s("--batch"), s("--gif"), s("a")])
            .err()
            .unwrap();
        assert!(error.contains("--batch cannot be combined with --gif"));
        // One pager per item would open N interactive pagers over one plan.
        let error = parse(&[s("--no-config"), s("--batch"), s("--pager"), s("a")])
            .err()
            .unwrap();
        assert!(error.contains("--pager cannot be combined with --batch"));
    }

    #[test]
    fn glob_matches_metacharacters_rather_than_substrings() {
        assert!(glob_match("*.md", "notes.md"));
        // `.md` appears in the name, but the pattern anchors at the end.
        assert!(!glob_match("*.md", "notes.md.bak"));
        assert!(glob_match("a?c.md", "abc.md"));
        assert!(!glob_match("a?c.md", "ac.md"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
    }

    #[test]
    fn batch_expansion_skips_symlinked_directories() {
        let root = std::env::temp_dir().join(format!("rich-batch-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.md"), "# a").unwrap();
        // A link back to the containing directory used to be walked forever.
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&root, root.join("loop")).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&root, root.join("loop")).is_ok();
        let inputs = expand_batch_resources(&[root.to_string_lossy().into_owned()]).unwrap();
        if linked {
            assert_eq!(inputs.len(), 1, "symlinked directory must not be followed");
        }
        assert!(inputs[0].ends_with("a.md"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn subcommand_mode_wins_over_config_mode_default() {
        let s = |v: &str| v.to_string();
        assert!(selects_mode_explicitly(&[s("json"), s("file.json")]));
        assert!(selects_mode_explicitly(&[s("--markdown"), s("file.md")]));
        // A value that happens to look like a subcommand is not a subcommand.
        assert!(!selects_mode_explicitly(&[
            s("--title"),
            s("json"),
            s("file")
        ]));
        assert!(!selects_mode_explicitly(&[s("file.md")]));

        let root = std::env::temp_dir().join(format!("rich-config-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("config.toml");
        std::fs::write(&config, "[defaults]\nmode = \"markdown\"\n").unwrap();
        let merged = config_args(
            &[
                "--config".to_string(),
                config.to_string_lossy().into_owned(),
                "json".to_string(),
                "file.json".to_string(),
            ],
            &ConfigRoots {
                home: None,
                cwd: root.clone(),
            },
        )
        .unwrap();
        assert!(
            !merged.iter().any(|arg| arg == "--markdown"),
            "config mode must not override an explicit subcommand: {merged:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn suffix_collisions_step_past_files_already_on_disk() {
        let root = std::env::temp_dir().join(format!("rich-batch-suffix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("out.html");
        std::fs::write(&target, "existing").unwrap();
        let target = target.to_string_lossy().into_owned();

        let cli = parse(&[
            "--no-config".to_string(),
            "--batch".to_string(),
            "--collision".to_string(),
            "suffix".to_string(),
            "x".to_string(),
        ])
        .unwrap()
        .unwrap();
        let mut taken = std::collections::BTreeSet::new();
        let first = resolve_destination(&cli, target.clone(), &mut taken).unwrap();
        assert!(
            first.ends_with("out-2.html"),
            "existing file must be kept: {first}"
        );
        let second = resolve_destination(&cli, target.clone(), &mut taken).unwrap();
        assert!(second.ends_with("out-3.html"));

        // `error` refuses rather than clobbering, and says how to proceed.
        let mut cli_error = cli.clone();
        cli_error.collision = CollisionPolicy::Error;
        let mut taken = std::collections::BTreeSet::new();
        let (class, message) =
            resolve_destination(&cli_error, target.clone(), &mut taken).unwrap_err();
        assert_eq!(class, ExitClass::Input);
        assert!(message.contains("--overwrite"));

        // `--overwrite` opts in explicitly.
        let mut cli_overwrite = cli.clone();
        cli_overwrite.overwrite = true;
        let mut taken = std::collections::BTreeSet::new();
        assert_eq!(
            resolve_destination(&cli_overwrite, target.clone(), &mut taken).unwrap(),
            target
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn batch_outputs_keep_their_export_kind() {
        // The planned path need not end in `.html`; the kind is carried, not sniffed.
        let outputs = BatchOutputs {
            html: batch_export_path(Some("result.out"), "a.md", 0, 1),
            ..BatchOutputs::default()
        };
        assert_eq!(outputs.html.as_deref(), Some("result.out"));
        assert!(outputs.svg.is_none());
    }

    #[test]
    fn parses_export_flags() {
        let s = |v: &str| v.to_string();
        // Both take a PATH, and the resource is a separate positional.
        let cli = parse(&[s("--export-svg"), s("out.svg"), s("x")])
            .unwrap()
            .unwrap();
        assert_eq!(cli.export_svg.as_deref(), Some("out.svg"));
        assert!(cli.export_html.is_none());
        assert_eq!(cli.resource.as_deref(), Some("x"));

        let cli = parse(&[s("-o"), s("out.html"), s("x")]).unwrap().unwrap();
        assert_eq!(cli.export_html.as_deref(), Some("out.html"));

        // Both together is allowed — upstream writes both files.
        let cli = parse(&[
            s("--export-html"),
            s("a.html"),
            s("--export-svg"),
            s("b.svg"),
            s("x"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(cli.export_html.as_deref(), Some("a.html"));
        assert_eq!(cli.export_svg.as_deref(), Some("b.svg"));

        // A missing PATH is an error rather than swallowing the next argument.
        assert!(parse(&[s("--export-html")]).is_err());
        // The two export formats are mutually exclusive.
    }

    #[test]
    fn is_number_matches_pattern() {
        // Every case checked against Python's own
        // `re.compile(r"\-?[0-9]*?\.?[0-9]*?").fullmatch`. The loose ones are
        // not oversights: that pattern really does accept a lone `-`, a lone
        // `.` and the empty string, and really does reject a padded `" 12 "`.
        for ok in [
            "0", "42", "-7", "3.14", "-0.5", "", "-", ".", "-.", "5.", ".5",
        ] {
            assert!(is_number(ok), "{ok:?} should be numeric");
        }
        for no in [" 12 ", "1.2.3", "1e5", "abc", "5%"] {
            assert!(!is_number(no), "{no:?} should not be numeric");
        }
    }

    #[test]
    fn render_csv_matches_upstream() {
        // Byte-parity with the Table real rich-cli's render_csv builds for this
        // CSV (captured from rich 15.0.0): HEAVY_HEAD box, blue border, the
        // numeric Age column right-justified with bold-green body + header cells.
        let table = build_csv_table(
            "Name,Age,City\nAlice,30,NYC\nBob,25,LA\n",
            Some(','),
            None,
            None,
        )
        .expect("the sniffer reads this one");
        let out = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(40)
            .no_color(false)
            .build()
            .render_to_string(&table);
        let expected = concat!(
            "\x1b[34m┏━━━━━━━┳━━━━━┳━━━━━━┓\x1b[0m\n",
            "\x1b[34m┃\x1b[0m\x1b[1m \x1b[0m\x1b[1mName \x1b[0m\x1b[1m \x1b[0m\x1b[34m┃\x1b[0m",
            "\x1b[1;32m \x1b[0m\x1b[1;32mAge\x1b[0m\x1b[1;32m \x1b[0m\x1b[34m┃\x1b[0m",
            "\x1b[1m \x1b[0m\x1b[1mCity\x1b[0m\x1b[1m \x1b[0m\x1b[34m┃\x1b[0m\n",
            "\x1b[34m┡━━━━━━━╇━━━━━╇━━━━━━┩\x1b[0m\n",
            "\x1b[34m│\x1b[0m Alice \x1b[34m│\x1b[0m\x1b[1;32m \x1b[0m\x1b[1;32m 30\x1b[0m",
            "\x1b[1;32m \x1b[0m\x1b[34m│\x1b[0m NYC  \x1b[34m│\x1b[0m\n",
            "\x1b[34m│\x1b[0m Bob   \x1b[34m│\x1b[0m\x1b[1;32m \x1b[0m\x1b[1;32m 25\x1b[0m",
            "\x1b[1;32m \x1b[0m\x1b[34m│\x1b[0m LA   \x1b[34m│\x1b[0m\n",
            "\x1b[34m└───────┴─────┴──────┘\x1b[0m",
        );
        assert_eq!(out, expected);
    }
}
