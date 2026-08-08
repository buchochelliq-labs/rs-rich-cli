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

use std::io::Read;
use std::process::ExitCode;

use rich::markdown::Markdown;
use rich::r#box::{Box as BoxSet, ASCII, ASCII2, DOUBLE, HEAVY, NONE, ROUNDED, SQUARE};
use rich::text::Text;
use rich::{
    filesize, Align, AnsiDecoder, Bar, ColorSystem, Columns, Console, Constrain, Control,
    Highlighter, HorizontalAlign, ISO8601Highlighter, Json, Justify, Layout, Live, LiveRender,
    LogLevel, LogRender, Padding, Panel, Pretty, Progress, ProgressBar, ProgressColumn, Renderable,
    Rule, Spinner, Status, Style, Styled, Syntax, Table, Traceback, Tree, DEFAULT_TERMINAL_THEME,
};
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
    /// `--rule`: draw a horizontal rule (the resource, if any, is its title).
    Rule,
}

/// Parsed command line.
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
    width: Option<usize>,
    justify: Option<Justify>,
    no_color: bool,
    /// `--export-html PATH` (`-o PATH`): also write a self-contained HTML
    /// document to PATH. The resource is still rendered to the terminal.
    export_html: Option<String>,
    /// `--export-svg PATH`: also write a self-contained SVG document to PATH.
    export_svg: Option<String>,
    /// `--panel BOX`: wrap the output in a [`Panel`] with the named box.
    panel: Option<BoxSet>,
    /// `--padding T[,R[,B,L]]`: wrap the output in [`Padding`].
    padding: Option<(usize, usize, usize, usize)>,
    /// `--title`: a panel title (only used with `--panel`).
    title: Option<String>,
    /// `--caption`: a panel subtitle (only used with `--panel`).
    caption: Option<String>,
    /// `--style`: the panel border style (only used with `--panel`).
    border_style: Option<Style>,
    /// `--pager`: page the output through the system pager.
    pager: bool,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(None) => ExitCode::SUCCESS, // help/version already printed
        Ok(Some(cli)) => run(cli),
        Err(message) => {
            eprintln!("rich: {message} (try --help)");
            ExitCode::FAILURE
        }
    }
}

/// Map a `--panel` box name to a box set (port of rich-cli's `BOXES` +
/// `getattr(box, name.upper())`).
fn parse_box(name: &str) -> Result<BoxSet, String> {
    match name.to_ascii_lowercase().as_str() {
        "none" => Ok(NONE),
        "ascii" => Ok(ASCII),
        "ascii2" => Ok(ASCII2),
        "square" => Ok(SQUARE),
        "rounded" => Ok(ROUNDED),
        "heavy" => Ok(HEAVY),
        "double" => Ok(DOUBLE),
        other => Err(format!(
            "unknown panel box {other:?} (use none/ascii/ascii2/square/rounded/heavy/double)"
        )),
    }
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
fn set_mode(current: &mut Mode, mode: Mode) -> Result<(), String> {
    if *current != Mode::Auto && *current != mode {
        return Err(
            "only one render mode (--print/--markdown/--json/--syntax/--csv/--ipynb/--rule) \
             may be given"
                .into(),
        );
    }
    *current = mode;
    Ok(())
}

/// Parse args into a [`Cli`], or `Ok(None)` when `--help`/`--version` handled it.
fn parse(args: &[String]) -> Result<Option<Cli>, String> {
    let mut mode = Mode::Auto;
    let mut resources: Vec<String> = Vec::new();
    let mut loops = None;
    let mut width = None;
    let mut justify = None;
    let mut no_color = false;
    let mut export_html: Option<String> = None;
    let mut export_svg: Option<String> = None;
    let mut panel = None;
    let mut padding = None;
    let mut title = None;
    let mut caption = None;
    let mut border_style = None;
    let mut pager = false;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
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
            "--right" => justify = Some(Justify::Right),
            "--center" => justify = Some(Justify::Center),
            "--no-color" => no_color = true,
            "--pager" => pager = true,
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
                panel = Some(parse_box(value)?);
            }
            "--padding" => {
                let value = iter.next().ok_or("--padding requires a value")?;
                padding = Some(parse_padding(value)?);
            }
            "--title" => {
                title = Some(iter.next().ok_or("--title requires a value")?.clone());
            }
            "--caption" => {
                caption = Some(iter.next().ok_or("--caption requires a value")?.clone());
            }
            "--style" => {
                let value = iter.next().ok_or("--style requires a value")?;
                border_style =
                    Some(Style::parse(value).map_err(|e| format!("invalid --style: {e}"))?);
            }
            "-w" | "--width" => {
                let value = iter.next().ok_or("--width requires a number")?;
                width = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid width {value:?}"))?,
                );
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other:?}"));
            }
            other => resources.push(other.to_string()),
        }
    }

    // Only `--gif` animates several resources at once; every other mode renders
    // exactly one.
    if mode != Mode::Gif && resources.len() > 1 {
        return Err("only one resource may be given (except with --gif)".into());
    }
    let resource = resources.first().cloned();

    Ok(Some(Cli {
        mode,
        resource,
        resources,
        loops,
        width,
        justify,
        no_color,
        export_html,
        export_svg,
        panel,
        padding,
        title,
        caption,
        border_style,
        pager,
    }))
}

/// Read a resource: `-` (or `None`) means stdin, otherwise a file path.
fn read_resource(resource: Option<&str>) -> std::io::Result<String> {
    match resource {
        Some(path) if path != "-" => std::fs::read_to_string(path),
        _ => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            Ok(buffer)
        }
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
fn fetch_url(url: &str) -> Result<(String, Option<String>), String> {
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
    let body = String::from_utf8(buffer)
        .map_err(|_| format!("{url} is not valid UTF-8 text (binary content?)"))?;
    Ok((body, content_type))
}

/// Stub used when the crate is built without the `fetch` feature.
#[cfg(not(feature = "fetch"))]
fn fetch_url(url: &str) -> Result<(String, Option<String>), String> {
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
        _ => Mode::Auto,
    }
}

fn run(cli: Cli) -> ExitCode {
    // With no flags and no resource, show the capability demo.
    if cli.mode == Mode::Auto && cli.resource.is_none() {
        run_demo();
        return ExitCode::SUCCESS;
    }

    let mut builder = Console::builder().no_color(cli.no_color);
    if let Some(width) = cli.width {
        builder = builder.width(width);
    }
    let mut console = builder.build();
    console.install_extensions();

    // SVG export needs a title (the resource's basename, else "rich") and a
    // stable id. Upstream's auto id hashes Python reprs, so we use a fixed one
    // (see the rich crate's svg module / DIVERGENCES #15).
    let svg_title = cli
        .resource
        .as_deref()
        .filter(|r| *r != "-")
        .map(|r| r.rsplit(['/', '\\']).next().unwrap_or(r).to_string())
        .unwrap_or_else(|| "rich".to_string());
    let export = Export {
        html_path: cli.export_html.as_deref(),
        svg_path: cli.export_svg.as_deref(),
        svg_title: &svg_title,
        pager: cli.pager,
    };

    let mut mode = match cli.mode {
        Mode::Auto => detect_mode(cli.resource.as_deref()),
        other => other,
    };

    // GIF playback consumes the resource list itself (it can take several) and
    // animates rather than rendering once.
    if mode == Mode::Gif {
        return play_gifs(&cli, &console);
    }

    // A rule takes its optional title from the resource string directly (no
    // fetch/read).
    if mode == Mode::Rule {
        let rule = match cli.resource.as_deref() {
            Some(title) if title != "-" => Rule::new(title),
            _ => Rule::line(),
        };
        return exit_code(emit(&console, &export, |c| c.print(&rule)));
    }

    // Obtain the content: fetch it over HTTP(S) when the resource is a URL,
    // treat it as a literal markup string under `--print`, else read the
    // file/stdin. A URL also yields a `Content-Type` used below.
    let resource_is_url = matches!(cli.resource.as_deref(), Some(r) if is_url(r));
    let (content, content_type) = if resource_is_url {
        match fetch_url(cli.resource.as_deref().unwrap()) {
            Ok(fetched) => fetched,
            Err(err) => {
                eprintln!("rich: {err}");
                return ExitCode::FAILURE;
            }
        }
    } else if mode == Mode::Print && matches!(cli.resource.as_deref(), Some(r) if r != "-") {
        (cli.resource.clone().unwrap(), None)
    } else {
        match read_resource(cli.resource.as_deref()) {
            Ok(content) => (content, None),
            Err(err) => {
                eprintln!(
                    "rich: cannot read {}: {err}",
                    cli.resource.as_deref().unwrap_or("<stdin>")
                );
                return ExitCode::FAILURE;
            }
        }
    };

    // For a URL that neither a flag nor a telltale extension resolved: upstream
    // renders fetched content as syntax-highlighted source by default, unless the
    // Content-Type marks it markdown / json / csv.
    if resource_is_url && mode == Mode::Auto {
        mode = content_type
            .as_deref()
            .and_then(content_type_mode)
            .unwrap_or(Mode::Syntax);
    }

    // Pre-parse JSON so a parse error surfaces before any (HTML) rendering.
    let json = if mode == Mode::Json {
        match Json::new(content.trim()) {
            Ok(json) => Some(json),
            Err(err) => {
                eprintln!("rich: invalid JSON: {err}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    // The highlighting language comes from the resource extension, falling back
    // (for a URL) to a lexer guessed from its Content-Type. An *uninformative*
    // extension (.txt/.log/…) doesn't shadow a Content-Type that names a real
    // language, so `snippet.txt` served as `text/x-python` still highlights.
    let extension = resource_ext(cli.resource.as_deref().unwrap_or_default())
        .filter(|ext| !is_uninformative_ext(ext));
    let language = extension
        .or_else(|| {
            content_type
                .as_deref()
                .and_then(content_type_lexer)
                .map(str::to_string)
        })
        .unwrap_or_default();

    // Pre-build the CSV/TSV table (delimiter from the extension: tab for `.tsv`).
    let csv_table = if mode == Mode::Csv {
        let delimiter = if language == "tsv" { '\t' } else { ',' };
        Some(render_csv(&parse_csv(&content, delimiter)))
    } else {
        None
    };

    // With `--panel`/`--padding`, build the content as a single renderable and
    // wrap it (padding inside, panel outside) — the rich-cli decorator flow.
    if (cli.panel.is_some() || cli.padding.is_some()) && mode != Mode::Ipynb {
        let mut content: Box<dyn Renderable> = match mode {
            Mode::Markdown => Box::new(Markdown::new(&content)),
            Mode::Json => Box::new(Json::new(content.trim()).expect("json validated above")),
            Mode::Csv => {
                let delimiter = if language == "tsv" { '\t' } else { ',' };
                Box::new(render_csv(&parse_csv(&content, delimiter)))
            }
            Mode::Syntax => Box::new(Syntax::new(content.as_str(), language.as_str())),
            _ => {
                // Print + auto: parse markup (Print) or take plain text (auto).
                let mut text = if mode == Mode::Print {
                    console.build_text(&content)
                } else {
                    Text::new(content.as_str())
                };
                if let Some(justify) = cli.justify {
                    text = text.justify(justify);
                }
                Box::new(text)
            }
        };
        if let Some(pad) = cli.padding {
            content = Box::new(Padding::new(content, pad));
        }
        if let Some(box_set) = cli.panel {
            let mut panel = Panel::new(content).box_set(box_set);
            if let Some(title) = cli.title.clone() {
                panel = panel.title(title);
            }
            if let Some(caption) = cli.caption.clone() {
                panel = panel.subtitle(caption);
            }
            if let Some(style) = cli.border_style.clone() {
                panel = panel.border_style(style);
            }
            content = Box::new(panel);
        }
        return exit_code(emit(&console, &export, |c| c.print(content.as_ref())));
    }

    // Markup comes from the user in Print mode, so a mistake in it should be
    // reported rather than printed literally (upstream raises `MarkupError`).
    // Checked up front because the render closure below cannot fail.
    if mode == Mode::Print {
        if let Err(err) = console.try_build_text(&content) {
            eprintln!("rich: {err}");
            return ExitCode::FAILURE;
        }
    }

    let ok = emit(&console, &export, |c| match mode {
        Mode::Markdown => c.print(&Markdown::new(&content)),
        Mode::Json => c.print(json.as_ref().expect("json parsed above")),
        Mode::Csv => c.print(csv_table.as_ref().expect("csv built above")),
        Mode::Ipynb => render_ipynb(c, &content),
        Mode::Syntax => c.print(&Syntax::new(content.as_str(), language.as_str())),
        Mode::Print => match cli.justify {
            Some(justify) => c.print_justified(&content, justify),
            None => c.print_str(&content),
        },
        // Auto with no detected type: print the resource as plain text.
        _ => match cli.justify {
            Some(justify) => c.print_justified(&content, justify),
            None => c.print(&Text::new(content.as_str())),
        },
    });
    exit_code(ok)
}

/// Turn an "everything written successfully" flag into a process exit code.
/// A failed `--export-html`/`--export-svg` write must not report success.
fn exit_code(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Parse CSV/TSV `content` into rows of fields. Handles double-quoted fields
/// (with `""` escaping) that may contain the delimiter or newlines; `\r` outside
/// quotes is dropped (so `\r\n` line endings work). A trailing newline does not
/// produce an empty final row.
fn parse_csv(content: &str, delimiter: char) -> Vec<Vec<String>> {
    // Strip a leading UTF-8 BOM so it doesn't cling to the first header cell.
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    // Whether the current record has any content yet (so a lone `""` or a
    // trailing delimiter still yields a field, but a bare newline does not).
    let mut pending = false;
    let mut chars = content.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
            pending = true;
        } else if c == delimiter {
            row.push(std::mem::take(&mut field));
            pending = true;
        } else if c == '\n' {
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
            pending = false;
        } else if c != '\r' {
            field.push(c);
            pending = true;
        }
    }
    if pending || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Whether `value` is a plain number (`-?[0-9]+(\.[0-9]+)?`). Mirrors rich-cli's
/// `is_number` for the numeric-column heuristic.
fn is_number(value: &str) -> bool {
    let value = value.trim();
    let body = value.strip_prefix('-').unwrap_or(value);
    let mut parts = body.split('.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next();
    if parts.next().is_some() {
        return false; // more than one '.'
    }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(int_part) && frac_part.is_none_or(digits)
}

/// Build a table from parsed CSV `rows`, mirroring rich-cli's `render_csv`:
/// `HEAVY_HEAD` box (the `Table` default), a blue border, the first row as the
/// header, and any all-numeric column right-justified with bold-green body +
/// header cells.
///
/// First slice: the first row is always treated as the header. `csv.Sniffer`'s
/// dialect/has-header heuristics and the title/caption are follow-ups.
fn render_csv(rows: &[Vec<String>]) -> Table {
    let mut table = Table::new().border_style(Style::parse("blue").expect("valid style"));
    let Some((header, data)) = rows.split_first() else {
        return table;
    };
    for (index, name) in header.iter().enumerate() {
        // A column is numeric when no data cell is a non-empty non-number
        // (empty cells are allowed); an empty data set counts as numeric, as
        // upstream's `for … else` does.
        let numeric = data.iter().all(|row| {
            let value = row.get(index).map(String::as_str).unwrap_or("");
            value.is_empty() || is_number(value)
        });
        if numeric {
            table.add_column_justify(name, Justify::Right);
            table.column_style(Style::parse("bold green").expect("valid style"));
            table.column_header_fill(Style::parse("bold green").expect("valid style"));
        } else {
            table.add_column(name);
        }
    }
    for row in data {
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        table.add_row(&cells);
    }
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

/// Decode an output string (which may carry ANSI codes) and print it line by
/// line — the equivalent of upstream's `Text.from_ansi`.
fn print_ansi(console: &Console, text: &str) {
    if text.is_empty() {
        return;
    }
    let mut decoder = AnsiDecoder::new();
    for line in decoder.decode(text) {
        console.print(&line);
    }
}

/// Render one cell output (stream / error / execute_result / display_data).
fn render_output(console: &Console, output: &serde_json::Value, count: Option<i64>) {
    match output["output_type"].as_str().unwrap_or("") {
        "stream" => print_ansi(console, &join_source(&output["text"])),
        "error" => print_ansi(console, &join_source(&output["traceback"])),
        "execute_result" | "display_data" => {
            console.print(&io_label("Out", count, "red", "#ee4b2b"));
            print_ansi(console, &join_source(&output["data"]["text/plain"]));
        }
        _ => {}
    }
}

/// Render a Jupyter notebook: markdown cells as [`Markdown`], code cells as an
/// `In [n]:` label + a dim [`Panel`] of [`Syntax`] + their outputs, blank-line
/// separated. Port of rich-cli's `render_ipynb` (rich outputs like images/HTML
/// are deferred — text/plain, stream, and error tracebacks are handled).
fn render_ipynb(console: &Console, content: &str) {
    let notebook: serde_json::Value = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("rich: invalid notebook JSON: {err}");
            return;
        }
    };
    let language = notebook["metadata"]["kernelspec"]["language"]
        .as_str()
        .or_else(|| notebook["metadata"]["language_info"]["name"].as_str())
        .unwrap_or("python")
        .to_string();
    let empty = Vec::new();
    let cells = notebook["cells"].as_array().unwrap_or(&empty);
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            console.print(&Text::new("")); // blank line between cells
        }
        let source = join_source(&cell["source"]);
        match cell["cell_type"].as_str().unwrap_or("") {
            "markdown" => console.print(&Markdown::new(&source)),
            "code" => {
                let count = cell["execution_count"].as_i64();
                console.print(&io_label("In ", count, "green", "#66ff00"));
                let syntax = Syntax::new(source.as_str(), language.as_str());
                let panel =
                    Panel::new(Box::new(syntax)).border_style(Style::parse("dim").expect("valid"));
                console.print(&panel);
                for output in cell["outputs"].as_array().unwrap_or(&empty) {
                    render_output(console, output, count);
                }
            }
            _ => console.print(&Text::new(source.as_str())),
        }
    }
}

/// Animate every `--gif` resource at once, sharing the console width.
#[cfg(feature = "art")]
fn play_gifs(cli: &Cli, console: &Console) -> ExitCode {
    use rich_art::{AnimatedArt, Repeat, Stage};

    if cli.resources.is_empty() {
        eprintln!("rich: --gif needs at least one GIF path");
        return ExitCode::FAILURE;
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
                        .color(!cli.no_color)
                        .repeat(repeat)
                        // Colour art is byte-heavy; keep it comfortable.
                        .max_fps(30.0),
                );
            }
            Err(err) => {
                eprintln!("rich: cannot read {path}: {err}");
                return ExitCode::FAILURE;
            }
        }
    }

    // `play` needs its own console (it moves into the Live display).
    let mut builder = Console::builder().no_color(cli.no_color);
    if let Some(width) = cli.width {
        builder = builder.width(width);
    }
    match stage.play_stdout(builder.build()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rich: playback failed: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Without the `art` feature there is no GIF support.
#[cfg(not(feature = "art"))]
fn play_gifs(_cli: &Cli, _console: &Console) -> ExitCode {
    eprintln!("rich: this build has no GIF support (rebuild with the `art` feature)");
    ExitCode::FAILURE
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
}

/// Render once and deliver it everywhere it was asked for.
///
/// The exports do **not** replace the terminal output — upstream prints the
/// resource *and* saves the files, and both `--export-html` and `--export-svg`
/// may be given together. So when either is set the render is recorded once and
/// the same segments are turned into terminal bytes, HTML and SVG. Rendering per
/// destination would be wrong rather than merely wasteful: a resource read from
/// standard input only yields its content once.
///
/// Returns false if a file could not be written, so the caller can exit non-zero.
fn emit(console: &Console, export: &Export, render: impl FnOnce(&Console)) -> bool {
    if export.html_path.is_none() && export.svg_path.is_none() {
        if export.pager {
            // Keep styles: unlike a plain `console.pager()`, the point of `rich
            // --pager` is to page *rich* output.
            if let Err(err) = console.page(true, render) {
                eprintln!("rich: cannot page output: {err}");
                return false;
            }
        } else {
            render(console);
        }
        return true;
    }

    let segments = console.record_output(render);
    let mut ok = true;

    // The terminal still gets the output, exports or not.
    let terminal = console.segments_to_string(&segments);
    if export.pager {
        // The text is already rendered, so hand it straight to the pager
        // rather than re-rendering through `Console::page`.
        if let Err(err) = rich::pager::Pager::show(&rich::pager::SystemPager, &terminal) {
            eprintln!("rich: cannot page output: {err}");
            ok = false;
        }
    } else {
        print!("{terminal}");
    }

    if let Some(path) = export.html_path {
        // CSS-class stylesheet form, as upstream's `save_html` default.
        let html = rich::export::export_html_classes(&segments, &DEFAULT_TERMINAL_THEME);
        if let Err(err) = std::fs::write(path, html) {
            eprintln!("rich: failed to save HTML: {err}");
            ok = false;
        }
    }
    if let Some(path) = export.svg_path {
        let svg = rich::svg::export_svg(
            &segments,
            &rich::SVG_EXPORT_THEME,
            export.svg_title,
            // Upstream derives this id by hashing Python reprs; a fixed one keeps
            // our output deterministic (DIVERGENCES #15).
            "rich-cli",
            console.width(),
        );
        if let Err(err) = std::fs::write(path, svg) {
            eprintln!("rich: failed to save SVG: {err}");
            ok = false;
        }
    }
    ok
}

fn print_help() {
    println!(
        "rich {VERSION} — Rust port of the rich-cli terminal toolbox\n\
\n\
USAGE:\n\
    rich [OPTIONS] [RESOURCE]\n\
\n\
RESOURCE is a file path, an http(s) URL, or `-` for stdin.\n\
\n\
RENDER MODE (choose at most one; default auto-detects by extension):\n\
    -p, --print      Interpret RESOURCE as console markup\n\
    -m, --markdown   Render RESOURCE as Markdown\n\
    -j, --json       Pretty-print RESOURCE as JSON\n\
    -x, --syntax     Syntax-highlight RESOURCE (language from its extension)\n\
        --csv        Render RESOURCE as a CSV/TSV table\n\
        --ipynb      Render RESOURCE as a Jupyter notebook\n\
        --gif        Animate one or more GIFs (several play side by side)\n\
        --loop N     With --gif, repeat N times (0 = forever)\n\
        --rule       Draw a horizontal rule (RESOURCE is its title)\n\
\n\
OPTIONS:\n\
    -w, --width N     Set the output width\n\
        --left        Left-justify output\n\
        --center      Center output\n\
        --right       Right-justify output\n\
    -o, --export-html PATH\n\
                      Also write a self-contained HTML document to PATH\n\
        --export-svg PATH\n\
                      Also write a self-contained SVG document to PATH\n\
        --panel BOX   Wrap output in a panel (none/ascii/ascii2/square/rounded/heavy/double)\n\
        --padding P   Wrap output in padding (1, 2, or 4 comma-separated ints)\n\
        --title T     Panel title (with --panel)\n\
        --caption T   Panel caption/subtitle (with --panel)\n\
        --style S     Panel border style, e.g. \"bold red\" (with --panel)\n\
        --pager       Page the output through the system pager ($PAGER, else less/more)\n\
        --no-color    Disable colored output\n\
    -h, --help        Show this help\n\
    -V, --version     Show the version (mirrors upstream rich-cli)\n\
\n\
With no RESOURCE and no mode flag, a capability demo is shown.\n"
    );
}

fn run_demo() {
    // Force truecolor so the demo looks the same regardless of TERM.
    let mut console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        // `[error]`/`[warning]`/`[info]` are rich-ext's additions, not upstream
        // styles — the core theme is a faithful 154-entry port.
        .theme(rich_ext::extended_theme())
        .build();
    console.install_extensions();

    console.print(&Rule::new("rs-rich-cli"));
    console.print_str("[bold magenta]rs-rich-cli[/] — a Rust port of [italic]rich[/]");
    console.print_str("Everything below is byte-parity-tested against Python rich 15.0.0.");

    // Markup, color, theme, and the rich-ext highlighter.
    console.print(&Rule::new("markup · color · theme · extension"));
    console.print_str(
        "Styles:   [bold]bold[/] [dim]dim[/] [italic]italic[/] [underline]underline[/] [reverse]reverse[/]",
    );
    console.print_str(
        "Color:    [red]red[/] [green]green[/] [blue]blue[/] [#ff8800]#ff8800[/] [white on blue]on blue[/]",
    );
    console.print_str("Theme:    [error]error[/], [warning]warning[/], [info]info[/]");
    console.print_str("Extension: numbers like 3.14 and 2026 are auto-highlighted");
    // Built-in ReprHighlighter (highlight=true): auto-colors numbers, bools,
    // strings, paths, calls, etc.
    let hl = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .highlight(true)
        .build();
    hl.print_str("Highlight:  result = func(42, True, None, '/usr/bin')");
    // ISO8601Highlighter — colors date/time/timezone fields.
    let mut stamp = Text::new("2023-06-15T13:45:30+02:00");
    ISO8601Highlighter::new().highlight(&mut stamp);
    console.print(&Text::new("ISO 8601:   ").append_text(&stamp));
    console.print_str("Emoji:     :rocket: :fire: :sparkles: :thumbs_up: :snake: :coffee:");

    // Rule variants (title alignment).
    console.print(&Rule::new("rule"));
    console.print(&Rule::line());
    console.print(&Rule::new("centered"));
    console.print(&Rule::new("left").align(HorizontalAlign::Left));
    console.print(&Rule::new("right").align(HorizontalAlign::Right));

    // Panels with different boxes.
    console.print(&Rule::new("panel"));
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
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(console.width())
        .legacy_windows(true)
        .build();
    legacy.print(&Panel::new(text("rounded → square on legacy Windows")).title("legacy"));

    // Padding (shown inside a panel so the blank space is visible).
    console.print(&Rule::new("padding"));
    console.print(
        &Panel::new(Box::new(Padding::new(text("padded (1, 4)"), (1, 4, 1, 4)))).title("padding"),
    );

    // Styled — lay one style under an entire renderable.
    console.print(&Rule::new("styled"));
    console.print(&Styled::new(
        Box::new(Panel::new(text("green under the whole panel")).title("styled")),
        Style::parse("green").unwrap(),
    ));

    // Horizontal alignment (fills the console width).
    console.print(&Rule::new("align"));
    console.print(&Align::left(text("← left")));
    console.print(&Align::center(text("center")));
    console.print(&Align::right(text("right →")));

    // Text justify (via print(justify=…)).
    console.print(&Rule::new("justify"));
    console.print_justified("left justified", Justify::Left);
    console.print_justified("centered", Justify::Center);
    console.print_justified("right justified", Justify::Right);

    // Constrain — same panel, capped to 24 cells.
    console.print(&Rule::new("constrain (width 24)"));
    console.print(&Constrain::new(
        Box::new(Panel::new(text("constrained")).title("≤24")),
        Some(24),
    ));

    // Table.
    console.print(&Rule::new("table"));
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
    console.print(&Rule::new("columns"));
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
    console.print(&Rule::new("tree"));
    let mut tree = Tree::new("rs-rich-cli");
    let core = tree.add("crates/rich (core, mirrors rich 15.0.0)");
    core.add("color · style · text · console");
    core.add("box · rule · panel · padding · align");
    core.add("table · tree · wrap · filesize");
    tree.add("crates/rich-ext (our extensions)");
    tree.add("crates/rich-cli (this binary)");
    console.print(&tree);

    // Progress bars at a few completion levels (0 → 100%).
    console.print(&Rule::new("progress bar"));
    for pct in [0.0, 33.0, 66.0, 100.0] {
        console.print(&ProgressBar::new(100.0, pct).width(48));
    }

    // Progress display — description + flexing bar + percentage (static frame).
    console.print(&Rule::new("progress"));
    let mut progress = Progress::new();
    progress.add_task("Downloading", 100.0, 50.0);
    progress.add_task("Processing", 100.0, 100.0);
    progress.add_task("Waiting", 100.0, 0.0);
    console.print(&progress);

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
    console.print(&Rule::new("json"));
    if let Ok(json) = Json::new(
        r#"{"port": "rich", "version": "15.0.0", "parity": true, "widgets": ["panel", "table", "tree"]}"#,
    ) {
        console.print(&json);
    }

    // Pretty — colorize a Rust value's Debug output (Rust-native rich.pretty).
    console.print(&Rule::new("pretty"));
    let value = vec![("panel", 1u32), ("table", 2), ("tree", 3)];
    console.print(&Pretty::new(&value));

    // Log records — a severity-colored line per record (Rust-native rich.logging).
    console.print(&Rule::new("log"));
    for (level, message) in [
        (LogLevel::Info, "server started on port 8080"),
        (LogLevel::Debug, "cache warm: 128 entries"),
        (LogLevel::Warn, "disk usage at 85%"),
        (LogLevel::Error, "connection reset by peer"),
    ] {
        console.print(
            &LogRender::new(level, message)
                .time("12:00:00")
                .path("main.rs:42"),
        );
    }

    // Traceback — render an error + its source chain (Rust-native rich.traceback).
    console.print(&Rule::new("traceback"));
    let cause = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file: config.toml");
    let err = std::io::Error::other(cause);
    console.print(&Traceback::new(&err));

    // Bar — a filled span within a range (eighth-block resolution).
    console.print(&Rule::new("bar (range)"));
    for (begin, end) in [(0.0, 100.0), (0.0, 62.0), (20.0, 80.0), (55.0, 100.0)] {
        console.print(&Bar::new(100.0, begin, end).width(48));
    }

    // Markdown — headings, inline styles, lists, block quotes, tables, and rules.
    console.print(&Rule::new("markdown"));
    console.print(&Markdown::new(
        "# Heading\n\nA paragraph with **bold**, *italic*, `code`, and a [link](https://example.com).\n\n- bullet item\n- another\n\n1. first\n2. second\n\n> a block quote\n\n| Name | Age |\n| :--- | ---: |\n| Alice | 30 |\n| Bob | 7 |\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n---",
    ));

    // Syntax highlighting (via syntect — functional, not byte-parity with rich).
    console.print(&Rule::new("syntax"));
    console.print(&Syntax::new(
        "fn main() {\n    let msg = \"hello, rich\";\n    println!(\"{msg}\");\n}",
        "rust",
    ));

    // Control codes — cursor/screen sequences (shown escaped, not executed).
    console.print(&Rule::new("control codes"));
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
    console.print(&Rule::new("ansi decoder"));
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
    console.print(&Rule::new("capture · export_text"));
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
    console.print(&Rule::new("layout"));
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
    console.print(&Rule::new("spinner (frames)"));
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
    console.print(&Rule::new("csv"));
    let csv = "Product,Qty,Price\nWidget,3,9.99\nGadget,12,19.50\nGizmo,1,4.25";
    console.print(&render_csv(&parse_csv(csv, ',')));

    // filesize.
    console.print(&Rule::new("filesize"));
    for bytes in [1u64, 999, 1_000, 1_500, 1_000_000, 1_500_000_000] {
        console.print_str(&format!("  {bytes:>13} → {}", filesize::decimal(bytes)));
    }
    console.print(&Rule::line());
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::ColorSystem;

    #[test]
    fn parse_csv_handles_quotes_and_delimiters() {
        // Quoted field containing the delimiter, and `""` escaping.
        let rows = parse_csv("a,\"b,c\",d\n\"he said \"\"hi\"\"\",2\n", ',');
        assert_eq!(
            rows,
            vec![
                vec!["a".to_string(), "b,c".to_string(), "d".to_string()],
                vec!["he said \"hi\"".to_string(), "2".to_string()],
            ]
        );
        // No trailing empty row after a final newline; `\r\n` endings work.
        assert_eq!(parse_csv("x\r\ny\r\n", ','), vec![vec!["x"], vec!["y"]]);
        // Tab delimiter.
        assert_eq!(parse_csv("a\tb", '\t'), vec![vec!["a", "b"]]);
        // A leading UTF-8 BOM is stripped, not glued to the first cell.
        assert_eq!(parse_csv("\u{feff}a,b", ','), vec![vec!["a", "b"]]);
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
        assert!(parse_box("rounded").is_ok());
        assert!(parse_box("HEAVY").is_ok());
        assert!(parse_box("none").is_ok()); // borderless box
        assert!(parse_box("bogus").is_err());
    }

    #[test]
    fn parses_pager_flag() {
        let s = |v: &str| v.to_string();
        assert!(parse(&[s("--pager"), s("x")]).unwrap().unwrap().pager);
        assert!(!parse(&[s("x")]).unwrap().unwrap().pager);
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
        for ok in ["0", "42", "-7", "3.14", "-0.5", " 12 "] {
            assert!(is_number(ok), "{ok:?} should be numeric");
        }
        for no in ["", "1.2.3", "1e5", "abc", "5%", "-", "."] {
            assert!(!is_number(no), "{no:?} should not be numeric");
        }
    }

    #[test]
    fn render_csv_matches_upstream() {
        // Byte-parity with the Table real rich-cli's render_csv builds for this
        // CSV (captured from rich 15.0.0): HEAVY_HEAD box, blue border, the
        // numeric Age column right-justified with bold-green body + header cells.
        let table = render_csv(&parse_csv("Name,Age,City\nAlice,30,NYC\nBob,25,LA\n", ','));
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
