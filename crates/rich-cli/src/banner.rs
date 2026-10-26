//! `rich banner TEXT`: a FIGlet banner, with a font, a style or colour
//! gradient, alignment and width fitting (0.0.19, #708). Not upstream: a
//! binary-boundary convenience (see `docs/PORTING.md`) over rs-rich-art's
//! public FIGlet API (`FigletFont`, `figlet::render`) and rs-rich-ext's
//! `palette::gradient`, behind the CLI's `art` feature.
//!
//! The bundled font is `standard`; `--font` also takes a FIGfont (`.flf`)
//! file, or the name of one in `$FIGLET_FONTDIR` or figlet's own font
//! directories, so an installed figlet's fonts work by name.
use super::*;

use rich::{Color, Segment, Style};
use rich_art::figlet::{self, FigletFont};
use rich_art::Justify as FigletJustify;
use rich_ext::cli_doc::{ArgSpec, CommandSpec};

/// The largest font file read: real FIGfonts are tens of kilobytes.
const FONT_LIMIT: u64 = 4 * 1024 * 1024;

/// The widest `--width`: wide enough for any terminal, small enough that a
/// layout of that many columns stays cheap.
const MAX_WIDTH: usize = 10_000;

/// Named gradients for `--gradient`.
const PRESETS: &[(&str, &[&str])] = &[
    (
        "rainbow",
        &[
            "#ff0000", "#ff8000", "#ffff00", "#00c000", "#0080ff", "#8000ff",
        ],
    ),
    ("sunset", &["#ff5f6d", "#ffc371"]),
    ("ocean", &["#2193b0", "#6dd5ed"]),
    ("forest", &["#134e5e", "#71b280"]),
    ("fire", &["#f12711", "#f5af19"]),
];

/// Whether the command line is `rich banner …` (not `rich -p banner`).
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("banner")
}

/// The command's help: registered with the root spec in `cli_spec`.
pub(super) fn command() -> CommandSpec {
    let presets = PRESETS
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ");
    CommandSpec::new("banner")
        .about(
            "Draw TEXT as a FIGlet banner: a font, a style or colour gradient, alignment and \
             width fitting. Built with the `art` feature",
        )
        .usage("banner [OPTIONS] [TEXT]...")
        .arg(ArgSpec::positional("text").multiple(true).help(
            "The words to draw, joined with spaces; none (or `-`) reads stdin. A newline \
             starts a new banner row",
        ))
        .arg(ArgSpec::option("font").value_name("NAME|FILE").help(
            "standard (bundled, the default), a FIGfont .flf file, or the name of one in \
             $FIGLET_FONTDIR, /usr/share/figlet or /usr/local/share/figlet",
        ))
        .arg(
            ArgSpec::flag("list-fonts").help(
                "List the bundled font and the .flf fonts found in those directories, then exit",
            ),
        )
        .arg(
            ArgSpec::option("style")
                .value_name("STYLE")
                .help("A style for every row, such as `bold cyan` or `white on blue`"),
        )
        .arg(
            ArgSpec::option("gradient")
                .value_name("COLORS")
                .help(format!(
                    "Colour the strokes along a gradient: two or more comma-separated colours \
                     (`#ff0000,blue`), or a preset: {presets}. Overrides --style's colour"
                )),
        )
        .arg(
            ArgSpec::option("direction")
                .value_name("D")
                .help("Which way --gradient runs (default horizontal)")
                .choices(["horizontal", "vertical", "diagonal"]),
        )
        .arg(
            ArgSpec::option("align")
                .value_name("A")
                .help("Position the banner within the width (default left)")
                .choices(["left", "center", "right"]),
        )
        .arg(
            ArgSpec::option("width")
                .short('w')
                .value_name("N")
                .help("Lay the banner out to N columns (default: the terminal's width)"),
        )
        .arg(
            ArgSpec::option("fit")
                .value_name("F")
                .help(
                    "When the banner is wider than the width: wrap it at spaces onto more rows \
                     (the default, as figlet does), print TEXT as plain text instead (plain), or \
                     let it overflow (none)",
                )
                .choices(["wrap", "plain", "none"]),
        )
        .arg(ArgSpec::flag("force-terminal").help(
            "Write colour and styles even when stdout is not a terminal (for a pager or a file)",
        ))
        .example("rich banner Hello", "A banner in the standard font")
        .example(
            "rich banner --gradient rainbow --align center 'Ship it'",
            "Centred, in rainbow colours",
        )
        .example(
            "rich banner --font slant --fit plain Release",
            "An installed figlet font; plain text on a narrow terminal",
        )
}

/// How the banner meets the width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    Wrap,
    Plain,
    None,
}

/// Which way a gradient runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Horizontal,
    Vertical,
    Diagonal,
}

#[derive(Debug)]
struct Args {
    help: bool,
    list_fonts: bool,
    words: Vec<String>,
    font: Option<String>,
    style: Option<Style>,
    gradient: Vec<Color>,
    direction: Direction,
    align: FigletJustify,
    width: Option<usize>,
    fit: Fit,
    force_terminal: bool,
}

/// The colours a `--gradient` value names.
fn parse_gradient(value: &str) -> Result<Vec<Color>, String> {
    let lower = value.trim().to_ascii_lowercase();
    if let Some((_, stops)) = PRESETS.iter().find(|(name, _)| *name == lower) {
        return Ok(stops
            .iter()
            .map(|stop| Color::parse(stop).expect("preset colours parse"))
            .collect());
    }
    let stops = value
        .split(',')
        .map(|stop| {
            Color::parse(stop.trim()).map_err(|_| {
                format!(
                    "--gradient: {stop:?} is not a colour (use names, #rrggbb or rgb(r,g,b), or a \
                     preset: {})",
                    PRESETS
                        .iter()
                        .map(|(name, _)| *name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if stops.len() < 2 {
        return Err(format!(
            "--gradient needs two or more colours, or a preset; for one colour use --style \
             (got {value:?})"
        ));
    }
    if stops.iter().any(|stop| stop.get_truecolor().is_none()) {
        return Err("--gradient cannot use `default`: it has no RGB value to blend".into());
    }
    Ok(stops)
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        help: false,
        list_fonts: false,
        words: Vec::new(),
        font: None,
        style: None,
        gradient: Vec::new(),
        direction: Direction::Horizontal,
        align: FigletJustify::Left,
        width: None,
        fit: Fit::Wrap,
        force_terminal: false,
    };
    let mut seen_command = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if !arg.starts_with('-') || arg == "-" {
            if !seen_command && arg == "banner" {
                seen_command = true;
            } else {
                parsed.words.push(arg.clone());
            }
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || -> Result<String, String> {
            match inline.clone() {
                Some(value) => Ok(value),
                None => iter
                    .next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value")),
            }
        };
        match name {
            "--" => {
                parsed.words.extend(iter.cloned());
                break;
            }
            "--font" => parsed.font = Some(value()?),
            "--list-fonts" => parsed.list_fonts = true,
            "--style" => {
                let text = value()?;
                parsed.style = Some(
                    Style::parse(&text)
                        .map_err(|error| format!("--style: invalid style {text:?}: {error}"))?,
                );
            }
            "--gradient" => parsed.gradient = parse_gradient(&value()?)?,
            "--direction" => {
                parsed.direction = match value()?.as_str() {
                    "horizontal" => Direction::Horizontal,
                    "vertical" => Direction::Vertical,
                    "diagonal" => Direction::Diagonal,
                    other => {
                        return Err(format!(
                            "--direction takes horizontal, vertical or diagonal, not {other:?}"
                        ))
                    }
                }
            }
            "--align" => {
                parsed.align = match value()?.as_str() {
                    "left" => FigletJustify::Left,
                    "center" | "centre" => FigletJustify::Center,
                    "right" => FigletJustify::Right,
                    other => {
                        return Err(format!(
                            "--align takes left, center or right, not {other:?}"
                        ))
                    }
                }
            }
            "-w" | "--width" => {
                let text = value()?;
                match text.parse::<usize>() {
                    Ok(n) if (1..=MAX_WIDTH).contains(&n) => parsed.width = Some(n),
                    _ => {
                        return Err(format!(
                            "--width takes a column count from 1 to {MAX_WIDTH}, not {text:?}"
                        ))
                    }
                }
            }
            "--fit" => {
                parsed.fit = match value()?.as_str() {
                    "wrap" => Fit::Wrap,
                    "plain" => Fit::Plain,
                    "none" => Fit::None,
                    other => return Err(format!("--fit takes wrap, plain or none, not {other:?}")),
                }
            }
            "--force-terminal" => parsed.force_terminal = true,
            "--help" => parsed.help = true,
            "--no-color" | "--color" | "--no-config" => {}
            "--report" => {
                value()?;
            }
            other => return Err(format!("unknown option {other} for `rich banner`")),
        }
    }
    Ok(parsed)
}

/// Where `--font NAME` is looked for, after the path itself.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("FIGLET_FONTDIR").filter(|dir| !dir.is_empty()) {
        dirs.push(PathBuf::from(dir));
    }
    dirs.push(PathBuf::from("/usr/share/figlet"));
    dirs.push(PathBuf::from("/usr/local/share/figlet"));
    dirs
}

/// The font `--font` names: (class, message) on failure.
fn load_font(name: Option<&str>) -> Result<FigletFont, (ExitClass, String)> {
    let Some(name) = name.filter(|name| *name != "standard") else {
        return Ok(FigletFont::standard());
    };
    let given = fs_path(name);
    let path = if given.is_file() {
        Some(given)
    } else if !name.contains(['/', '\\']) {
        let file = if name.ends_with(".flf") {
            name.to_string()
        } else {
            format!("{name}.flf")
        };
        font_dirs()
            .into_iter()
            .map(|dir| dir.join(&file))
            .find(|path| path.is_file())
    } else {
        None
    };
    let Some(path) = path else {
        return Err((
            ExitClass::Input,
            format!(
                "no font {name:?}: not a file, and no {name}.flf in $FIGLET_FONTDIR, \
                 /usr/share/figlet or /usr/local/share/figlet (try --list-fonts)"
            ),
        ));
    };
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|file| file.take(FONT_LIMIT + 1).read_to_end(&mut bytes))
        .map_err(|error| {
            (
                ExitClass::Input,
                format!("cannot read font {}: {error}", path.display()),
            )
        })?;
    if bytes.len() as u64 > FONT_LIMIT {
        return Err((
            ExitClass::Input,
            format!("font {} is larger than 4 MiB", path.display()),
        ));
    }
    let source = String::from_utf8_lossy(&bytes);
    FigletFont::parse(&source).map_err(|error| {
        (
            ExitClass::Data,
            format!("cannot use font {}: {error}", path.display()),
        )
    })
}

/// `--list-fonts`: the bundled font, then each directory's `.flf` files.
fn list_fonts() -> String {
    let mut out = String::from("standard (bundled)\n");
    for dir in font_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".flf").map(str::to_string)
            })
            .collect();
        names.sort();
        for name in names {
            out.push_str(&format!("{name} ({})\n", dir.display()));
        }
    }
    out
}

/// The banner's lines, or the text's own lines under `--fit plain`.
fn layout(text: &str, font: &FigletFont, width: usize, args: &Args) -> Vec<String> {
    let lines = |banner: String| -> Vec<String> {
        banner
            .strip_suffix('\n')
            .unwrap_or(&banner)
            .split('\n')
            .map(str::to_string)
            .collect()
    };
    // Laid out with room to spare, it never wraps: the natural banner.
    let natural = lines(figlet::render(
        text,
        font,
        usize::MAX / 4,
        FigletJustify::Left,
    ));
    let natural_width = natural
        .iter()
        .map(|line| rich::cells::cell_len(line))
        .max()
        .unwrap_or(0);
    // FIGlet keeps each row one column short of its width.
    let fits = natural_width < width;
    match args.fit {
        Fit::Plain if !fits => text.lines().map(str::to_string).collect(),
        Fit::None => lines(figlet::render(
            text,
            font,
            width.max(natural_width + 1),
            args.align,
        )),
        _ => lines(figlet::render(text, font, width, args.align)),
    }
}

/// The banner's lines painted: `style` under every column, and the
/// gradient's colour on every stroke.
struct Painted {
    lines: Vec<String>,
    style: Option<Style>,
    gradient: Vec<Color>,
    direction: Direction,
}

impl Painted {
    fn colour_at(&self, row: usize, column: usize, width: usize) -> Color {
        let share = |at: usize, of: usize| {
            if of <= 1 {
                0.0
            } else {
                at as f64 / (of - 1) as f64
            }
        };
        let t = match self.direction {
            Direction::Horizontal => share(column, width),
            Direction::Vertical => share(row, self.lines.len()),
            Direction::Diagonal => (share(column, width) + share(row, self.lines.len())) / 2.0,
        };
        rich_ext::palette::gradient(&self.gradient, t)
    }
}

impl Renderable for Painted {
    fn rich_render(&self, _console: &Console, _options: &rich::ConsoleOptions) -> Vec<Segment> {
        // The gradient spans the drawn strokes, not the trailing blanks.
        let width = self
            .lines
            .iter()
            .map(|line| rich::cells::cell_len(line.trim_end()))
            .max()
            .unwrap_or(0);
        let base = self.style.clone();
        let mut out = Vec::new();
        for (row, line) in self.lines.iter().enumerate() {
            if row > 0 {
                out.push(Segment::line());
            }
            if self.gradient.is_empty() {
                out.push(Segment::new(line.clone(), base.clone()));
                continue;
            }
            let mut column = 0;
            for ch in line.chars() {
                let style = if ch.is_whitespace() {
                    base.clone()
                } else {
                    let colour = Style::new().with_color(self.colour_at(row, column, width));
                    Some(match &base {
                        Some(base) => base.combine(&colour),
                        None => colour,
                    })
                };
                out.push(Segment::new(ch.to_string(), style));
                column += rich::cells::cell_len(ch.encode_utf8(&mut [0; 4]));
            }
        }
        // Merge neighbours that ended up with the same style.
        Segment::simplify(&out)
    }
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    let no_color = cli_spec::no_color_requested(args);
    let parsed = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            return emit_error(
                json,
                ExitClass::Usage,
                &format!("{message} (try `rich banner --help`)"),
            )
        }
    };
    if parsed.help {
        if let Some(help) = cli_spec::subcommand_help(&["banner"], no_color) {
            authoring::out(&format!("{help}\n"));
        }
        return ExitCode::SUCCESS;
    }
    if parsed.list_fonts {
        authoring::out(&list_fonts());
        return ExitCode::SUCCESS;
    }
    let font = match load_font(parsed.font.as_deref()) {
        Ok(font) => font,
        Err((class, message)) => return emit_error(json, class, &message),
    };
    let text = if parsed.words.is_empty() || parsed.words == ["-"] {
        let mut input = String::new();
        if let Err(error) = std::io::stdin().read_to_string(&mut input) {
            return emit_error(
                json,
                ExitClass::Input,
                &format!("cannot read stdin: {error}"),
            );
        }
        input.trim_end_matches(['\n', '\r']).replace("\r\n", "\n")
    } else {
        parsed.words.join(" ")
    };
    if text.trim().is_empty() {
        return emit_error(
            json,
            ExitClass::Usage,
            "`rich banner` needs TEXT to draw (try `rich banner --help`)",
        );
    }
    // A control character would be drawn as nothing and could run as one.
    let text: String = text
        .chars()
        .filter(|ch| *ch == '\n' || !ch.is_control())
        .collect();
    let builder = || {
        let builder = crate::console_builder().no_color(no_color);
        let builder = if parsed.force_terminal {
            builder.force_terminal(true)
        } else {
            builder
        };
        match parsed.width {
            Some(width) => builder.width(width),
            None => builder,
        }
    };
    let width = builder().build().width();
    let lines = layout(&text, &font, width, &parsed);
    let painted = Painted {
        lines,
        style: parsed.style.clone(),
        gradient: parsed.gradient.clone(),
        direction: parsed.direction,
    };
    // Never let the console wrap a row: `--fit none` may be wider.
    let widest = painted
        .lines
        .iter()
        .map(|line| rich::cells::cell_len(line))
        .max()
        .unwrap_or(0);
    let console = builder().width(width.max(widest)).build();
    authoring::out(&format!("{}\n", console.render_to_string(&painted)));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn parses_every_option() {
        let parsed = parse_args(&args(&[
            "banner",
            "--font",
            "standard",
            "--style=bold",
            "--gradient",
            "red,#0000ff",
            "--direction",
            "vertical",
            "--align",
            "center",
            "-w",
            "40",
            "--fit",
            "plain",
            "Hello",
            "world",
        ]))
        .unwrap();
        assert_eq!(parsed.words, ["Hello", "world"]);
        assert_eq!(parsed.gradient.len(), 2);
        assert_eq!(parsed.direction, Direction::Vertical);
        assert_eq!(parsed.align, FigletJustify::Center);
        assert_eq!(parsed.width, Some(40));
        assert_eq!(parsed.fit, Fit::Plain);
        assert!(parsed.style.is_some());
    }

    #[test]
    fn rejects_bad_values() {
        for bad in [
            &["banner", "--gradient", "red"][..],
            &["banner", "--gradient", "red,notacolour"],
            &["banner", "--gradient", "default,red"],
            &["banner", "--align", "middle"],
            &["banner", "--fit", "squash"],
            &["banner", "--width", "0"],
            &["banner", "--direction", "up"],
            &["banner", "--style", "not a style"],
            &["banner", "--bogus"],
            &["banner", "--font"],
        ] {
            assert!(parse_args(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn presets_resolve() {
        for (name, stops) in PRESETS {
            assert_eq!(parse_gradient(name).unwrap().len(), stops.len());
        }
        assert_eq!(parse_gradient("RAINBOW").unwrap().len(), 6);
    }

    #[test]
    fn fit_modes_lay_out_differently() {
        let font = FigletFont::standard();
        let mut parsed = parse_args(&args(&["banner"])).unwrap();
        let wrapped = layout("Hello world", &font, 30, &parsed);
        assert_eq!(wrapped.len(), 12, "two banner rows: {wrapped:#?}");
        parsed.fit = Fit::Plain;
        assert_eq!(layout("Hello world", &font, 30, &parsed), ["Hello world"]);
        // Plain only when it does not fit.
        assert_eq!(layout("Hi", &font, 30, &parsed).len(), 6);
        parsed.fit = Fit::None;
        let wide = layout("Hello world", &font, 30, &parsed);
        assert_eq!(wide.len(), 6);
        assert!(wide.iter().any(|line| line.len() > 30));
    }

    #[test]
    fn gradients_run_each_way() {
        let painted = |direction| Painted {
            lines: vec!["ab".into(), "cd".into()],
            style: None,
            gradient: parse_gradient("#000000,#ffffff").unwrap(),
            direction,
        };
        let hex = |c: Color| c.get_truecolor().unwrap().hex();
        let horizontal = painted(Direction::Horizontal);
        assert_eq!(hex(horizontal.colour_at(0, 0, 2)), "#000000");
        assert_eq!(hex(horizontal.colour_at(1, 1, 2)), "#ffffff");
        let vertical = painted(Direction::Vertical);
        assert_eq!(hex(vertical.colour_at(0, 1, 2)), "#000000");
        assert_eq!(hex(vertical.colour_at(1, 0, 2)), "#ffffff");
        let diagonal = painted(Direction::Diagonal);
        assert_eq!(hex(diagonal.colour_at(0, 1, 2)), "#808080");
    }
}
