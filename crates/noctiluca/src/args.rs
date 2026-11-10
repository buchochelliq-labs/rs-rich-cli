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
    /// `--audio`: sound on, over SSH too.
    pub audio: bool,
    /// `--mute` (or `--no-audio`): sound off, here too.
    pub mute: bool,
    pub profile: Option<String>,
    pub chrome: Option<PathBuf>,
    pub images: Option<Images>,
    pub fps: Option<u32>,
    pub dump: Option<Dump>,
    pub no_js: bool,
    pub config: Option<PathBuf>,
    /// `--adapt LIST`: rules switched off (`-name`) or on (`+name`).
    pub adapt: Option<String>,
    /// `--no-adapt`: the faithful projection, every rule off.
    pub no_adapt: bool,
    /// `--no-mouse`: leave the mouse to the terminal (its own selection
    /// and scrollback), so clicks and the wheel do not reach the browser.
    pub no_mouse: bool,
    /// `--transparent`: where a page sets no background, the terminal's
    /// own shows.
    pub transparent: bool,
    /// `--ssh HOST`: Chrome runs here, its network goes through HOST
    /// ([`crate::ssh`]).
    pub ssh: Option<String>,
    /// `--ssh-arg ARG`, repeatable: the user's own options for that ssh.
    pub ssh_args: Vec<String>,
    /// `--split DIR`: open in a new pane of the terminal, beside this one
    /// (`right`, `left`, `up`, `down`), and return.
    pub split: Option<rich_interact::host::Direction>,
    /// `--control`: listen on the control socket, for `noctiluca action`
    /// ([`crate::control`]).
    pub control: bool,
    /// `--read-only`: listen, and refuse every verb that changes anything.
    pub read_only: bool,
}

/// What the command line asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Browse(Args),
    /// `noctiluca keys [--site HOST]`: the keys, and a site's.
    Keys {
        site: Option<String>,
        config: Option<PathBuf>,
    },
    /// `noctiluca ls [--json]`: the sessions listening.
    Ls {
        json: bool,
    },
    /// `noctiluca action …`: one verb to a running session.
    Action(crate::control::ActionArgs),
    /// `noctiluca ls --help` or `noctiluca action --help`.
    ControlHelp,
    Help,
    Version,
}

pub const USAGE: &str = "\
noctiluca: a terminal web browser

usage: noctiluca [OPTIONS] [URL | FILE | KEYWORD WORDS…]
       noctiluca keys [--site HOST] [--config PATH]
       noctiluca ls [--json]
       noctiluca action [--session PID] [--json] VERB [ARGS…]

  --reader | --projection | --pixels | --raster
                                       the first view (default: projection;
                                       pixels needs Kitty graphics)
  --view NAME                          the same: projection, reader, pixels, raster
  --audio                              let pages play sound over SSH too (on by
                                       default here, off over SSH: it plays where
                                       Chrome runs)
  --mute, --no-audio                   keep pages quiet
  --profile NAME                       keep a profile between runs (opt-in)
  --chrome PATH                        the browser to run (default: a system Chrome)
  --images ansi|kitty|sixel|iterm2|none
                                       how images reach the terminal
  --fps N                              the media frame cap
  --dump text|ax|json                  print the page's model and exit
  --no-js                              turn the page's script off
  --no-mouse                           leave the mouse to the terminal (no
                                       clicks; its own text selection)
  --transparent                        let the terminal's background show where a
                                       page sets none of its own
  --ssh [USER@]HOST                    run Chrome here, but make every request
                                       through HOST (its localhost is localhost)
  --ssh-arg ARG                        an option for that ssh (repeatable:
                                       --ssh-arg=-p --ssh-arg=2222)
  --split right|left|up|down           open in a new pane beside this one (tmux,
                                       WezTerm, kitty, zellij, or a terminal with
                                       terminal-browser's socket) and return
  --adapt -RULE,+RULE,…                switch terminal adaptations off or on
                                       (decorative-images, collapse-nav, …;
                                       ? while browsing lists them)
  --no-adapt                           every adaptation off: the faithful view
  --control                            listen for `noctiluca action` (a socket of
                                       your own user; Unix only for now)
  --read-only                          listen, but refuse every action that
                                       changes anything
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
    let help = |rest: &[String]| rest.iter().any(|a| a == "-h" || a == "--help");
    match args.first().map(String::as_str) {
        Some("ls") if help(&args[1..]) => return Ok(Run::ControlHelp),
        Some("ls") => {
            return match &args[1..] {
                [] => Ok(Run::Ls { json: false }),
                [flag] if flag == "--json" => Ok(Run::Ls { json: true }),
                other => Err(format!("ls: unknown switch {:?}", other[0])),
            };
        }
        Some("action") if help(&args[1..]) => return Ok(Run::ControlHelp),
        Some("action") => {
            return crate::control::parse_action(&args[1..])
                .map(Run::Action)
                .map_err(|e| format!("action: {e}"));
        }
        _ => {}
    }
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
                return Err("--reader, --projection, --pixels and --raster: choose one".into());
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
            "--pixels" => view(View::Pixels)?,
            "--view" => {
                let value = value_of(name, inline, &mut rest)?;
                let chosen = View::parse(&value).ok_or_else(|| {
                    format!("--view takes projection, reader, pixels or raster, not {value:?}")
                })?;
                view(chosen)?
            }
            "--control" => parsed.control = true,
            "--read-only" => parsed.read_only = true,
            "--audio" => parsed.audio = true,
            "--mute" | "--no-audio" => parsed.mute = true,
            "--no-js" => parsed.no_js = true,
            "--no-mouse" => parsed.no_mouse = true,
            "--transparent" => parsed.transparent = true,
            "--no-adapt" => parsed.no_adapt = true,
            "--adapt" => {
                let value = value_of(name, inline, &mut rest)?;
                crate::adapt::Adapt::default()
                    .apply(&value)
                    .map_err(|e| format!("--adapt: {e}"))?;
                let list = parsed
                    .adapt
                    .take()
                    .map_or(value.clone(), |l| format!("{l},{value}"));
                parsed.adapt = Some(list);
            }
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
            "--ssh" => {
                let host = value_of(name, inline, &mut rest)?;
                crate::ssh::check_host(&host)?;
                parsed.ssh = Some(host);
            }
            "--ssh-arg" => parsed.ssh_args.push(value_of(name, inline, &mut rest)?),
            "--split" => {
                let value = value_of(name, inline, &mut rest)?;
                parsed.split = Some(rich_interact::host::Direction::parse(&value).ok_or_else(
                    || format!("--split takes right, left, up or down, not {value:?}"),
                )?);
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
    if !parsed.ssh_args.is_empty() && parsed.ssh.is_none() {
        return Err("--ssh-arg needs --ssh HOST".into());
    }
    Ok(Run::Browse(parsed))
}

/// The adaptations `args` ask for, starting from `configured` (the
/// config's): `--adapt`'s list applied, then `--no-adapt` switching all
/// of them off.
/// Whether pages play sound: on where Noctiluca runs on the terminal's own
/// machine, off when it runs over SSH (`over_ssh`: `SSH_CONNECTION` or
/// `SSH_TTY` set), where Chrome's sound would play on a machine the user is
/// not at. `--ssh HOST` keeps Chrome (and its sound) on this machine, with
/// only the network on the far side, so it changes nothing here. `--audio`
/// (or `[media] audio = true`) turns sound on over SSH too; `--mute` turns
/// it off anywhere.
pub fn sound_on(args: &Args, config_audio: bool, over_ssh: bool) -> bool {
    if args.mute {
        return false;
    }
    args.audio || config_audio || !over_ssh
}

pub fn adapt_of(
    args: &Args,
    configured: crate::adapt::Adapt,
) -> Result<crate::adapt::Adapt, String> {
    let mut adapt = match &args.adapt {
        Some(list) => configured.apply(list)?,
        None => configured,
    };
    if args.no_adapt {
        adapt = crate::adapt::Adapt::none();
    }
    Ok(adapt)
}

/// The command line `args` without `--split` and its value: what the new
/// pane runs.
pub fn without_split(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--" {
            out.push(arg.clone());
            out.extend(rest.cloned());
            break;
        }
        if arg == "--split" {
            rest.next();
            continue;
        }
        if arg.starts_with("--split=") {
            continue;
        }
        out.push(arg.clone());
    }
    out
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
            "--no-mouse",
            "--profile",
            "work",
            "--dump",
            "ax",
            "x.html",
        ])
        .unwrap() else {
            panic!()
        };
        assert!(args.audio && args.no_js && args.no_mouse);
        assert!(!args.mute);
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
    fn ssh_takes_a_host_and_the_user_s_own_options() {
        let Run::Browse(args) = parse(&[
            "--ssh",
            "me@devbox",
            "--ssh-arg=-p",
            "--ssh-arg",
            "2222",
            "http://localhost:3000",
        ])
        .unwrap() else {
            panic!()
        };
        assert_eq!(args.ssh.as_deref(), Some("me@devbox"));
        assert_eq!(args.ssh_args, ["-p", "2222"]);
        assert_eq!(args.target, ["http://localhost:3000"]);
    }

    #[test]
    fn split_opens_beside_with_the_rest_of_the_line() {
        let Run::Browse(args) = parse(&["--split", "right", "--reader", "x.html"]).unwrap() else {
            panic!()
        };
        assert_eq!(args.split, Some(rich_interact::host::Direction::Right));
        assert!(
            parse(&["--split=sideways"])
                .unwrap_err()
                .contains("right, left")
        );
        let line = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            without_split(&line(&["--split", "right", "--reader", "x.html"])),
            ["--reader", "x.html"]
        );
        assert_eq!(
            without_split(&line(&["--split=down", "--", "--split"])),
            ["--", "--split"]
        );
    }

    #[test]
    fn the_pixel_view_and_the_control_socket() {
        let Run::Browse(args) = parse(&["--view", "pixels", "--read-only", "x.html"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(args.view, Some(View::Pixels));
        assert!(args.read_only && !args.control);
        let Run::Browse(args) = parse(&["--pixels", "--control"]).unwrap() else {
            panic!()
        };
        assert_eq!(args.view, Some(View::Pixels));
        assert!(args.control);
        assert!(
            parse(&["--pixels", "--reader"])
                .unwrap_err()
                .contains("choose one")
        );
        assert!(parse(&["--view=sideways"]).is_err());
        assert_eq!(parse(&["ls"]).unwrap(), Run::Ls { json: false });
        assert_eq!(parse(&["ls", "--json"]).unwrap(), Run::Ls { json: true });
        assert_eq!(parse(&["action", "--help"]).unwrap(), Run::ControlHelp);
        let Run::Action(action) = parse(&["action", "click", "12"]).unwrap() else {
            panic!()
        };
        assert_eq!(action.verb, "click");
        assert!(
            parse(&["action", "dance"])
                .unwrap_err()
                .starts_with("action:")
        );
    }

    #[test]
    fn adaptations_switch_off_one_by_one_or_all() {
        let Run::Browse(args) =
            parse(&["--adapt=-decorative-images,-collapse-nav", "x.html"]).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            args.adapt.as_deref(),
            Some("-decorative-images,-collapse-nav")
        );
        let Run::Browse(args) = parse(&["--no-adapt", "x.html"]).unwrap() else {
            panic!()
        };
        assert!(args.no_adapt);
        assert!(
            parse(&["--adapt", "-sparkles"])
                .unwrap_err()
                .contains("not an adaptation")
        );
    }

    #[test]
    fn bad_switches_say_what_is_wrong() {
        assert!(
            parse(&["--reader", "--raster"])
                .unwrap_err()
                .contains("choose one")
        );
        assert!(
            parse(&["--images", "png"])
                .unwrap_err()
                .contains("ansi, kitty")
        );
        assert!(parse(&["--fps", "0"]).is_err());
        assert!(parse(&["--profile", "../x"]).is_err());
        assert!(
            parse(&["--dump", "text"])
                .unwrap_err()
                .contains("needs a page")
        );
        assert!(parse(&["--nope"]).unwrap_err().contains("unknown switch"));
        assert!(
            parse(&["--ssh", "-oProxyCommand=x"])
                .unwrap_err()
                .contains("--ssh-arg")
        );
        assert!(
            parse(&["--ssh-arg", "-p", "x.html"])
                .unwrap_err()
                .contains("needs --ssh")
        );
        let Run::Browse(args) = parse(&["--", "--reader"]).unwrap() else {
            panic!()
        };
        assert_eq!(args.target, ["--reader"]);
    }

    #[test]
    fn sound_is_on_here_off_over_ssh_and_the_flags_win() {
        let browse = |words: &[&str]| match parse(words).unwrap() {
            Run::Browse(args) => args,
            _ => panic!(),
        };
        let plain = browse(&["x.html"]);
        // Here: on. Over SSH (the environment says so): off.
        assert!(sound_on(&plain, false, false));
        assert!(!sound_on(&plain, false, true));
        // `--ssh HOST`: Chrome runs here, only its network is far: the
        // sound plays here, so it stays on (and off when this session is
        // itself over SSH).
        let tunnel = Args {
            ssh: Some("far".into()),
            ..plain.clone()
        };
        assert!(sound_on(&tunnel, false, false));
        assert!(!sound_on(&tunnel, false, true));
        // `--audio`, or the config, turn it on over SSH too.
        let loud = browse(&["--audio", "x.html"]);
        assert!(sound_on(&loud, false, true));
        assert!(sound_on(&plain, true, true));
        // `--mute` / `--no-audio`: off anywhere, whatever else says.
        for flag in ["--mute", "--no-audio"] {
            let quiet = browse(&[flag, "x.html"]);
            assert!(quiet.mute);
            assert!(!sound_on(&quiet, false, false));
            assert!(!sound_on(&quiet, true, false));
        }
        assert!(!sound_on(
            &browse(&["--audio", "--mute", "x.html"]),
            false,
            false
        ));
    }
}
