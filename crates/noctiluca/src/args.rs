//! `noctiluca`'s command line, in the hand-written style of `rs-rich-cli`'s
//! commands (`fn parse_args(&[String]) -> Result<…, String>`).

use std::path::PathBuf;

use crate::config::{Images, View};

/// What `--dump` prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dump {
    /// The page's text, block by block.
    Text,
    /// Its controls, as the accessibility tree has them.
    Ax,
    /// The whole model, as JSON.
    Json,
}

/// The browser's switches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// An address, a file, or a keyword and its words.
    pub target: Vec<String>,
    pub view: Option<View>,
    pub audio: bool,
    pub profile: Option<String>,
    pub chrome: Option<PathBuf>,
    pub images: Option<Images>,
    pub fps: Option<u32>,
    pub dump: Option<Dump>,
    pub no_js: bool,
    pub config: Option<PathBuf>,
}

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    Browse(Args),
    /// `noctiluca keys [--site HOST]`: the keys, and a site's.
    Keys {
        site: Option<String>,
        config: Option<PathBuf>,
    },
    Help,
    Version,
}

pub const USAGE: &str = "\
noctiluca: a terminal web browser

usage: noctiluca [OPTIONS] [URL | FILE | KEYWORD WORDS…]
       noctiluca keys [--site HOST] [--config PATH]

  --reader | --projection | --raster   the first view (default: projection)
  --audio                              let pages play sound (off by default)
  --profile NAME                       keep a profile between runs (opt-in)
  --chrome PATH                        the browser to run (default: a system Chrome)
  --images ansi|kitty|sixel|iterm2|none
                                       how images reach the terminal
  --fps N                              the media frame cap
  --dump text|ax|json                  print the page's model and exit
  --no-js                              turn the page's script off
  --config PATH                        another config file
  -h, --help | -V, --version

F1 lists every action and its keys while browsing.";

fn value_of(
    name: &str,
    inline: Option<String>,
    rest: &mut std::slice::Iter<'_, String>,
) -> Result<String, String> {
    match inline {
        Some(value) => Ok(value),
        None => rest
            .next()
            .cloned()
            .ok_or_else(|| format!("{name} needs a value")),
    }
}

/// The command line `args` (without the program's name).
pub fn parse_args(args: &[String]) -> Result<Run, String> {
    if args.first().is_some_and(|a| a == "keys") {
        let (mut site, mut config) = (None, None);
        let mut rest = args[1..].iter();
        while let Some(arg) = rest.next() {
            let (name, inline) = split(arg);
            match name {
                "--site" => site = Some(value_of(name, inline, &mut rest)?),
                "--config" => config = Some(PathBuf::from(value_of(name, inline, &mut rest)?)),
                "-h" | "--help" => return Ok(Run::Help),
                other => return Err(format!("keys: unknown switch {other:?}")),
            }
        }
        return Ok(Run::Keys { site, config });
    }
    let mut parsed = Args::default();
    let mut rest = args.iter();
    let mut words = false;
    while let Some(arg) = rest.next() {
        if words || !arg.starts_with('-') || arg == "-" {
            parsed.target.push(arg.clone());
            continue;
        }
        let (name, inline) = split(arg);
        let mut view = |v: View| -> Result<(), String> {
            if parsed.view.is_some_and(|w| w != v) {
                return Err("--reader, --projection and --raster: choose one".into());
            }
            parsed.view = Some(v);
            Ok(())
        };
        match name {
            "--" => words = true,
            "-h" | "--help" => return Ok(Run::Help),
            "-V" | "--version" => return Ok(Run::Version),
            "--reader" => view(View::Reader)?,
            "--projection" => view(View::Projection)?,
            "--raster" => view(View::Raster)?,
            "--audio" => parsed.audio = true,
            "--no-js" => parsed.no_js = true,
            "--profile" => {
                let name = value_of(name, inline, &mut rest)?;
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                {
                    return Err("--profile takes a name: letters, digits, - and _".into());
                }
                parsed.profile = Some(name);
            }
            "--chrome" => parsed.chrome = Some(PathBuf::from(value_of(name, inline, &mut rest)?)),
            "--config" => parsed.config = Some(PathBuf::from(value_of(name, inline, &mut rest)?)),
            "--images" => {
                let value = value_of(name, inline, &mut rest)?;
                parsed.images = Some(
                    Images::parse(&value)
                        .filter(|i| *i != Images::Auto)
                        .ok_or_else(|| {
                            format!(
                                "--images takes ansi, kitty, sixel, iterm2 or none, not {value:?}"
                            )
                        })?,
                );
            }
            "--fps" => {
                let value = value_of(name, inline, &mut rest)?;
                parsed.fps = Some(
                    value
                        .parse::<u32>()
                        .ok()
                        .filter(|n| (1..=60).contains(n))
                        .ok_or_else(|| format!("--fps takes 1 to 60, not {value:?}"))?,
                );
            }
            "--dump" => {
                let value = value_of(name, inline, &mut rest)?;
                parsed.dump = Some(match value.as_str() {
                    "text" => Dump::Text,
                    "ax" => Dump::Ax,
                    "json" => Dump::Json,
                    other => return Err(format!("--dump takes text, ax or json, not {other:?}")),
                });
            }
            other => return Err(format!("unknown switch {other:?} (see --help)")),
        }
    }
    if parsed.dump.is_some() && parsed.target.is_empty() {
        return Err("--dump needs a page".into());
    }
    Ok(Run::Browse(parsed))
}

/// `--name=value` as the name and the value.
fn split(arg: &str) -> (&str, Option<String>) {
    match arg.split_once('=') {
        Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
        _ => (arg, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Run, String> {
        parse_args(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn switches_and_a_keyword_with_its_words() {
        let Run::Browse(args) =
            parse(&["--reader", "--images=kitty", "--fps", "12", "gh", "rs-rich"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(args.view, Some(View::Reader));
        assert_eq!(args.images, Some(Images::Kitty));
        assert_eq!(args.fps, Some(12));
        assert_eq!(args.target, ["gh", "rs-rich"]);
        let Run::Browse(args) = parse(&[
            "--audio",
            "--no-js",
            "--profile",
            "work",
            "--dump",
            "ax",
            "x.html",
        ])
        .unwrap() else {
            panic!()
        };
        assert!(args.audio && args.no_js);
        assert_eq!(
            (args.profile.as_deref(), args.dump),
            (Some("work"), Some(Dump::Ax))
        );
        assert_eq!(
            parse(&["keys", "--site", "github.com"]).unwrap(),
            Run::Keys {
                site: Some("github.com".into()),
                config: None
            }
        );
        assert_eq!(parse(&["-h"]).unwrap(), Run::Help);
    }

    #[test]
    fn bad_switches_say_what_is_wrong() {
        assert!(parse(&["--reader", "--raster"])
            .unwrap_err()
            .contains("choose one"));
        assert!(parse(&["--images", "png"])
            .unwrap_err()
            .contains("ansi, kitty"));
        assert!(parse(&["--fps", "0"]).is_err());
        assert!(parse(&["--profile", "../x"]).is_err());
        assert!(parse(&["--dump", "text"])
            .unwrap_err()
            .contains("needs a page"));
        assert!(parse(&["--nope"]).unwrap_err().contains("unknown switch"));
        let Run::Browse(args) = parse(&["--", "--reader"]).unwrap() else {
            panic!()
        };
        assert_eq!(args.target, ["--reader"]);
    }
}
