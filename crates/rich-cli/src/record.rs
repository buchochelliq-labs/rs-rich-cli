//! `rich record TAPE…`: run scripted terminal sessions and render them
//! (rs-rich-record, #599). Not upstream: a CLI convenience over the
//! rs-rich-record library.
use super::*;

use std::path::{Path, PathBuf};

use rich_record::record::{self as recorder, Formats, Options};
use rich_record::render::raster::Fonts;
use rich_record::tape;

/// Whether the first word that is not an option is `record`, with no render
/// mode flag before it (`rich -p record` prints the word).
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("record")
}

struct Args {
    tapes: Vec<PathBuf>,
    output: PathBuf,
    bin_dir: Option<PathBuf>,
    check: bool,
    formats: Formats,
    font: Option<PathBuf>,
}

fn parse_formats(value: &str) -> Result<Formats, String> {
    let mut formats = Formats {
        png: false,
        svg: false,
        cast: false,
        gif: false,
        mp4: false,
    };
    for name in value.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        match name {
            "png" => formats.png = true,
            "svg" => formats.svg = true,
            "cast" => formats.cast = true,
            "gif" => formats.gif = true,
            "mp4" => formats.mp4 = true,
            "all" => formats = Formats::ALL,
            other => {
                return Err(format!(
                    "unknown --format {other:?} (choose from png, svg, cast, gif, mp4, all)"
                ))
            }
        }
    }
    Ok(formats)
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        tapes: Vec::new(),
        output: PathBuf::from("recordings"),
        bin_dir: std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        check: false,
        formats: Formats::ALL,
        font: None,
    };
    let mut seen_command = false;
    let mut iter = args.iter();
    let mut options_done = false;
    while let Some(arg) = iter.next() {
        if options_done || !arg.starts_with('-') || arg == "-" {
            if !seen_command && arg == "record" {
                seen_command = true;
            } else {
                parsed.tapes.push(fs_path(arg));
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
            "--" => options_done = true,
            "--check" => parsed.check = true,
            "--no-video" => {
                parsed.formats.gif = false;
                parsed.formats.mp4 = false;
            }
            "--output" | "-o" => parsed.output = fs_path(&value()?),
            "--bin-dir" => parsed.bin_dir = Some(fs_path(&value()?)),
            "--format" => parsed.formats = parse_formats(&value()?)?,
            "--font" => parsed.font = Some(fs_path(&value()?)),
            "--no-color" | "--no-config" => {}
            other => return Err(format!("unknown option {other} for `rich record`")),
        }
    }
    if parsed.tapes.is_empty() {
        return Err("`rich record` needs at least one TAPE".into());
    }
    Ok(parsed)
}

/// A tape's name: its file name without the extension.
fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "tape".into(), |s| s.to_string_lossy().into_owned())
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help")
    {
        let no_color = cli_spec::no_color_requested(args);
        if let Some(help) = cli_spec::subcommand_help(&["record"], no_color) {
            authoring::out(&format!("{help}\n"));
        }
        return ExitCode::SUCCESS;
    }
    let args = match parse_args(args) {
        Ok(args) => args,
        Err(message) => {
            return emit_error(
                json,
                ExitClass::Usage,
                &format!("{message} (try `rich record --help`)"),
            )
        }
    };
    let fonts = match &args.font {
        Some(path) => match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(Fonts::load)
        {
            Ok(fonts) => fonts,
            Err(error) => {
                return emit_error(
                    json,
                    ExitClass::Input,
                    &format!("cannot load --font {}: {error}", path.display()),
                )
            }
        },
        None => Fonts::embedded(),
    };
    let options = Options {
        bin_dir: args.bin_dir.clone(),
        repo: std::env::current_dir().ok(),
        ..Options::default()
    };
    // A tape's name picks its output directory (`--output/<stem>`), which
    // `write` fills and prunes: `...tape` would name `..`, the directory
    // above. Refused, for every tape, before anything runs.
    for path in &args.tapes {
        let stem = stem(path);
        if !recorder::stem_allowed(&stem) {
            return emit_error(
                json,
                ExitClass::Input,
                &format!(
                    "{}: the tape's name {stem:?} cannot name a recording directory; \
                     rename the tape",
                    path.display()
                ),
            );
        }
    }
    let mut failures = 0;
    // Each warning about the machine once, not once per tape.
    let mut warned = std::collections::BTreeSet::new();
    for path in &args.tapes {
        let stem = stem(path);
        let shown = path.display();
        let source = match std::fs::read(path) {
            Ok(source) => source,
            Err(error) => {
                return emit_error(
                    json,
                    ExitClass::Input,
                    &format!("cannot read {shown}: {error}"),
                )
            }
        };
        let parsed = match tape::parse(&String::from_utf8_lossy(&source)) {
            Ok(parsed) => parsed,
            Err(error) => return emit_error(json, ExitClass::Input, &format!("{shown}: {error}")),
        };
        eprintln!("{shown}");
        for warning in recorder::warnings(&parsed) {
            if warned.insert(warning.clone()) {
                eprintln!("  warning: {warning}");
            }
        }
        let recording = match recorder::record(&parsed, &stem, &options) {
            Ok(recording) => recording,
            Err(error) => {
                eprintln!("  FAILED: {error}");
                failures += 1;
                continue;
            }
        };
        let dir = args.output.join(&stem);
        if args.check {
            let problems = recorder::check(&recording, &dir);
            for (name, _) in &recording.shots {
                let bad = problems
                    .iter()
                    .any(|p| matches!(p, recorder::Problem::Differs { name: n, .. } if n == name));
                eprintln!("  {name}: {}", if bad { "DIFFERS" } else { "ok" });
            }
            for problem in &problems {
                if let recorder::Problem::Orphaned { name } = problem {
                    eprintln!("  {name}: ORPHANED");
                }
            }
            for problem in &problems {
                eprintln!("{stem}: {problem}");
            }
            failures += problems.len();
            continue;
        }
        match recorder::write(
            &recording,
            &dir,
            &stem,
            args.formats,
            &fonts,
            &options.theme,
            Some((path, &source)),
        ) {
            Ok(written) => {
                for file in written {
                    eprintln!("  {}", file.display());
                }
                if args.formats.mp4 && !rich_record::render::video::ffmpeg_available() {
                    eprintln!("  ffmpeg not found: skipped {stem}.mp4");
                }
            }
            Err(error) => {
                return emit_error(
                    json,
                    ExitClass::Data,
                    &format!("cannot write {}: {error}", dir.display()),
                )
            }
        }
    }
    if failures > 0 {
        return emit_error(
            json,
            ExitClass::Data,
            &format!("{failures} tape problem(s)"),
        );
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn routes_and_parses() {
        assert!(requested(&strings(&["record", "a.tape"])));
        assert!(requested(&strings(&["--no-color", "record"])));
        assert!(!requested(&strings(&["notes.md"])));
        let args = parse_args(&strings(&[
            "record",
            "--check",
            "--output=media",
            "--format",
            "png,cast",
            "a.tape",
        ]))
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(args.check);
        assert_eq!(args.output, PathBuf::from("media"));
        assert!(args.formats.png && args.formats.cast && !args.formats.gif);
        assert_eq!(args.tapes, [PathBuf::from("a.tape")]);
        assert!(parse_args(&strings(&["record"])).is_err());
        assert!(parse_args(&strings(&["record", "--format", "webm", "a"])).is_err());
    }

    #[test]
    fn stems_come_from_the_file_name() {
        assert_eq!(stem(Path::new("docs/tapes/hero.tape")), "hero");
        assert_eq!(stem(Path::new("...tape")), "..");
        assert_eq!(stem(Path::new("..tape")), ".");
        assert!(!recorder::stem_allowed(&stem(Path::new("x/...tape"))));
        assert!(recorder::stem_allowed(&stem(Path::new("x/a.b.tape"))));
    }
}
