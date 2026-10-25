//! `noctiluca`: a terminal web browser. See `noctiluca --help` and
//! `docs/design/browser.md`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use noctiluca::actions::{self, ACTIONS};
use noctiluca::args::{parse_args, Args, Run, USAGE};
use noctiluca::bookmarks::{self, Bookmarks};
use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::config::{data_dir, Config, View};
use noctiluca::engine::Engine;
use noctiluca::file_url;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match parse_args(&args) {
        Ok(Run::Help) => {
            println!("{USAGE}");
            Ok(())
        }
        Ok(Run::Version) => {
            println!("noctiluca {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Ok(Run::Keys { site, config }) => keys(site.as_deref(), config.as_deref()),
        Ok(Run::Browse(args)) => browse(args),
        Err(error) => {
            eprintln!("noctiluca: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("noctiluca: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `noctiluca keys`: every action's keys, and a site's own.
fn keys(site: Option<&str>, path: Option<&Path>) -> std::io::Result<()> {
    let config = Config::load(path)?;
    for warning in &config.warnings {
        eprintln!("noctiluca: config: {warning}");
    }
    let (keymap, warnings) = actions::keymap(config.preset, &config.keys);
    for warning in warnings {
        eprintln!("noctiluca: config: {warning}");
    }
    match site {
        None => {
            for action in ACTIONS {
                println!(
                    "{:>16}  {:<16}  {}",
                    actions::keys_label(&keymap, action.id),
                    action.id,
                    action.label
                );
            }
            for site in &config.sites {
                for key in &site.keys {
                    println!(
                        "{:>16}  {}  [site {}]",
                        key.keys,
                        key.target.describe(),
                        site.pattern
                    );
                }
            }
        }
        Some(host) => {
            let url = if host.contains("://") {
                host.to_string()
            } else {
                format!("https://{host}/")
            };
            let matching: Vec<_> = config
                .sites
                .iter()
                .filter(|s| noctiluca::config::site_match(&s.pattern, &url).is_some())
                .collect();
            if matching.is_empty() {
                println!("No site keys for {host}.");
            }
            for site in matching {
                for key in &site.keys {
                    println!(
                        "{:>16}  {}  [site {}]",
                        key.keys,
                        key.target.describe(),
                        site.pattern
                    );
                }
            }
        }
    }
    Ok(())
}

/// What the words on the command line open.
fn start_url(args: &Args, config: &Config, marks: &Bookmarks) -> Option<String> {
    if args.target.is_empty() {
        return None;
    }
    if let [one] = args.target.as_slice() {
        let path = Path::new(one);
        if path.exists() && !one.contains("://") {
            return file_url(path).ok();
        }
    }
    let typed = args.target.join(" ");
    Some(
        config
            .destination(&typed, "about:blank", &marks.items)
            .url()
            .to_string(),
    )
}

/// The engine: Chrome; or, built with the test-only `fake-engine`
/// feature, a test's saved snapshots when `NOCTILUCA_FAKE_ENGINE` names
/// their directory (the end-to-end test and the docs tape run the binary
/// so, with no browser). A release build ignores the variable.
fn engine(args: &Args, config: &Config) -> std::io::Result<Box<dyn Engine>> {
    #[cfg(feature = "fake-engine")]
    if let Some(dir) = std::env::var_os("NOCTILUCA_FAKE_ENGINE") {
        return Ok(Box::new(noctiluca::testing::FakeEngine::from_dir(
            Path::new(&dir),
        )?));
    }
    let profile = args
        .profile
        .clone()
        .or_else(|| config.profile.clone())
        .map(|name| {
            data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("profiles")
                .join(name)
        });
    let binary = args
        .chrome
        .clone()
        .or_else(|| std::env::var_os("RICH_EMBED_CHROME").map(PathBuf::from));
    let (columns, rows) = terminal_size();
    let backend = ChromeBackend::start(ChromeOptions {
        binary,
        audio: args.audio || config.audio,
        profile,
        quarantine: None,
        no_js: args.no_js,
        columns,
        rows,
    })
    .map_err(|e| {
        let root = std::env::var("USER").is_ok_and(|u| u == "root");
        if root {
            std::io::Error::new(
                e.kind(),
                format!(
                    "{e} (Chrome cannot run its sandbox as root: run Noctiluca as a normal user)"
                ),
            )
        } else {
            e
        }
    })?;
    Ok(Box::new(backend))
}

fn terminal_size() -> (u16, u16) {
    let var = |name: &str| std::env::var(name).ok().and_then(|v| v.parse().ok());
    (var("COLUMNS").unwrap_or(100), var("LINES").unwrap_or(40))
}

fn browse(args: Args) -> std::io::Result<()> {
    let mut config = Config::load(args.config.as_deref())?;
    if let Some(fps) = args.fps {
        config.fps = fps;
    }
    config.audio |= args.audio;
    config.adapt = noctiluca::args::adapt_of(&args, config.adapt)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let marks = bookmarks::default_path()
        .map(|p| Bookmarks::load(&p))
        .transpose()?
        .unwrap_or_default();
    let url = start_url(&args, &config, &marks);
    if let Some(how) = args.dump {
        for warning in &config.warnings {
            eprintln!("noctiluca: config: {warning}");
        }
        let url = url.unwrap_or_default();
        let page =
            noctiluca::dump::read_page(engine(&args, &config)?, &url, Duration::from_secs(60))?;
        println!("{}", noctiluca::dump::dump(&page, how));
        return Ok(());
    }
    if args.view.unwrap_or(config.view) == View::Raster {
        return raster(&args, url.unwrap_or_else(|| "about:blank".into()));
    }
    let engine = engine(&args, &config)?;
    noctiluca::app::app(
        engine,
        noctiluca::app::Options {
            config,
            bookmarks: marks,
            url,
            view: args.view,
            images: args.images,
        },
    )
    .run()
}

/// The raster view, for debugging: the page as Chrome paints it, in half
/// blocks, through rs-rich-embed's web view.
fn raster(args: &Args, url: String) -> std::io::Result<()> {
    use rich_intuituive::prelude::*;
    let binary = args.chrome.clone().or_else(noctiluca::cdp::find_chrome);
    let audio = args.audio;
    App::new(move || {
        let mut engine = rich_embed::ChromeEngine::new().audio(audio);
        if let Some(binary) = binary {
            engine = engine.binary(binary);
        }
        rich_embed::web_view_with(engine, url)
            .address_bar(true)
            .node()
            .on_key("ctrl+q", |cx| cx.quit())
    })
    .run()
}
