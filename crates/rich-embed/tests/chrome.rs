//! ChromeEngine against a real browser. Runs only when `RICH_EMBED_CHROME`
//! names a Chrome or Chromium binary, so CI needs none; without it the test
//! says it skipped and passes. The browser keeps its sandbox on, so it must
//! be able to start one (as root it cannot). And against a stand-in
//! script, for the command line the browser is given.

#![cfg(feature = "chrome")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich::Color;
use rich_embed::{web_view_with, ChromeEngine, WebHandle};
use rich_intuituive::prelude::*;
use rich_intuituive::Driver;

/// A web view of an all-red page in a 20 x 6 app, driven until the page
/// draws (or a minute passes), and its handle.
fn open_a_red_page(binary: std::ffi::OsString) -> (Driver, WebHandle, bool) {
    let url = "data:text/html,<title>red</title><body style='margin:0;background:%23ff0000'>";
    let handle: Rc<RefCell<Option<WebHandle>>> = Rc::default();
    let keep = handle.clone();
    let app = App::new(move || {
        let page = web_view_with(ChromeEngine::new().binary(binary), url);
        *keep.borrow_mut() = Some(page.handle());
        page.node()
    });
    let mut driver = app.driver(20, 6);
    let start = Instant::now();
    let red = |driver: &Driver| {
        let screen = driver.screen();
        let cell = screen.cell(10, 3);
        // Red, give or take what JPEG does to it.
        let colour = screen
            .style(cell.style)
            .and_then(|s| s.color().and_then(Color::get_truecolor));
        cell.text == "▀" && colour.is_some_and(|c| c.red > 200 && c.green < 60 && c.blue < 60)
    };
    while !red(&driver) && start.elapsed() < Duration::from_secs(60) {
        driver.update(start.elapsed());
        driver.render();
        std::thread::sleep(Duration::from_millis(20));
    }
    let h = handle.borrow().unwrap();
    let drawn = red(&driver);
    (driver, h, drawn)
}

#[test]
fn a_page_arrives_as_a_screencast_and_draws_as_half_blocks() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let (driver, h, drawn) = open_a_red_page(binary);
    assert!(
        drawn,
        "no red frame: {:?}, error {:?}",
        driver.screen().plain(),
        h.error().get_untracked()
    );
    assert!(h.address().get_untracked().starts_with("data:text/html"));
}

/// On Linux and macOS DevTools runs over a pipe: while a view is open, the
/// browser (and every process it started) listens on no TCP socket.
#[cfg(target_os = "linux")]
#[test]
fn the_browser_listens_on_no_port_while_a_view_is_open() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let (driver, h, drawn) = open_a_red_page(binary);
    assert!(
        drawn,
        "no red frame: {:?}, error {:?}",
        driver.screen().plain(),
        h.error().get_untracked()
    );
    let browser = linux::descendants(std::process::id());
    assert!(!browser.is_empty(), "no browser process found");
    let listening: Vec<_> = browser
        .iter()
        .flat_map(|&pid| linux::listening(pid).into_iter().map(move |at| (pid, at)))
        .collect();
    assert!(
        listening.is_empty(),
        "the browser listens on TCP: {listening:?}"
    );
}

/// The processes under this one and the TCP sockets they listen on, from
/// `/proc`, scoped to each process's own descriptors.
#[cfg(target_os = "linux")]
mod linux {
    use std::fs;

    /// The parent of `pid`, from `/proc/<pid>/stat` (the field after the
    /// command, which may itself hold spaces and parentheses).
    fn parent(pid: u32) -> Option<u32> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after = &stat[stat.rfind(')')? + 1..];
        after.split_whitespace().nth(1)?.parse().ok()
    }

    /// Every process descended from `root`, not `root` itself.
    pub fn descendants(root: u32) -> Vec<u32> {
        let all: Vec<(u32, u32)> = fs::read_dir("/proc")
            .map(|dir| {
                dir.flatten()
                    .filter_map(|e| e.file_name().to_str()?.parse().ok())
                    .filter_map(|pid| Some((pid, parent(pid)?)))
                    .collect()
            })
            .unwrap_or_default();
        let mut found = vec![root];
        let mut at = 0;
        while at < found.len() {
            let pid = found[at];
            found.extend(all.iter().filter(|(_, p)| *p == pid).map(|(c, _)| *c));
            at += 1;
        }
        found.remove(0);
        found
    }

    /// The local addresses of the TCP sockets `pid` holds that listen.
    pub fn listening(pid: u32) -> Vec<String> {
        let held: Vec<String> = fs::read_dir(format!("/proc/{pid}/fd"))
            .map(|dir| {
                dir.flatten()
                    .filter_map(|e| fs::read_link(e.path()).ok())
                    .filter_map(|link| {
                        let link = link.to_string_lossy().into_owned();
                        Some(
                            link.strip_prefix("socket:[")?
                                .strip_suffix(']')?
                                .to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for table in ["tcp", "tcp6"] {
            let text = fs::read_to_string(format!("/proc/{pid}/net/{table}")).unwrap_or_default();
            for line in text.lines().skip(1) {
                let fields: Vec<&str> = line.split_whitespace().collect();
                // `st` 0A is LISTEN; the inode is the tenth field.
                if fields.len() > 9 && fields[3] == "0A" && held.iter().any(|i| i == fields[9]) {
                    out.push(fields[1].to_string());
                }
            }
        }
        out
    }
}

/// A stand-in browser: a script that writes down its arguments, the mode of
/// its profile directory and the first byte the engine sends it on
/// descriptor 3, then idles as a browser would.
#[cfg(unix)]
#[test]
fn the_browser_gets_no_sandbox_switch_and_a_private_profile() {
    use std::os::unix::fs::PermissionsExt;

    use rich_embed::WebEngine;

    let dir = std::env::temp_dir().join(format!("rich-embed-fake-chrome-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let argv = dir.join("argv");
    let script = dir.join("chrome");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             for a in \"$@\"; do case \"$a\" in --user-data-dir=*) \
             stat -c %a \"${{a#--user-data-dir=}}\" > '{dir}/mode' 2>/dev/null || \
             stat -f %Lp \"${{a#--user-data-dir=}}\" > '{dir}/mode';; esac; done\n\
             (head -c 1 <&3 > '{dir}/cdp.tmp' && mv '{dir}/cdp.tmp' '{dir}/cdp') 2>/dev/null &\n\
             printf '%s\\n' \"$@\" > '{argv}.tmp' && mv '{argv}.tmp' '{argv}'\n\
             exec sleep 30\n",
            dir = dir.display(),
            argv = argv.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut engine = ChromeEngine::new().binary(&script).args([
        "-no-sandbox",
        "--No-Sandbox",
        "--disable-seccomp-filter-sandbox",
        "--disable-namespace-sandbox",
        "--disable-setuid-sandbox",
        "--no-zygote-sandbox",
        "--lang=en",
    ]);
    engine.resize(80, 24).unwrap();
    let start = Instant::now();
    let cdp = dir.join("cdp");
    while !(argv.exists() && cdp.exists()) && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let args = std::fs::read_to_string(&argv).unwrap_or_default();
    let mode = std::fs::read_to_string(dir.join("mode")).unwrap_or_default();
    // The first byte the engine wrote on descriptor 3: a DevTools message.
    let first = std::fs::read_to_string(&cdp).unwrap_or_default();
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(args.lines().any(|arg| arg == "--lang=en"), "{args}");
    assert!(
        !args.to_ascii_lowercase().contains("sandbox"),
        "a sandbox switch reached the browser: {args}"
    );
    assert_eq!(mode.trim(), "700", "the profile's mode");
    // DevTools over descriptors 3 and 4, and no port.
    assert!(
        args.lines().any(|arg| arg == "--remote-debugging-pipe"),
        "{args}"
    );
    assert!(!args.contains("--remote-debugging-port"), "{args}");
    assert_eq!(first, "{", "nothing came over descriptor 3");
}

/// An app's own DevTools call reaches the page and its answer comes back;
/// a call DevTools refuses fails that call alone, and the next one works.
#[test]
fn an_apps_devtools_call_reaches_the_page() {
    use rich_embed::WebEngine;
    use serde_json::json;

    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let mut engine = ChromeEngine::new().binary(binary);
    engine.resize(40, 10).unwrap();
    engine
        .open("data:text/html,<title>t</title><h1>Hello</h1>")
        .unwrap();
    let devtools = engine.devtools().unwrap();
    let wait = Duration::from_secs(60);
    let start = Instant::now();
    let title = loop {
        let answer = devtools
            .call(
                "Runtime.evaluate",
                json!({"expression": "document.querySelector('h1')?.textContent ?? ''",
                       "returnByValue": true}),
                wait,
            )
            .unwrap();
        let text = answer["result"]["value"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if text == "Hello" || start.elapsed() > wait {
            break text;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(title, "Hello");
    assert!(devtools.call("No.such.method", json!({}), wait).is_err());
    let again = devtools
        .call("Runtime.evaluate", json!({"expression": "1 + 1"}), wait)
        .unwrap();
    assert_eq!(again["result"]["value"], 2);
}

/// With the screencast off no frame streams in, while the page still loads
/// and answers calls; switched on, frames come.
#[test]
fn the_screencast_runs_only_while_it_is_on() {
    use rich_embed::WebEngine;
    use serde_json::json;

    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let mut engine = ChromeEngine::new().binary(binary).screencast(false);
    engine.resize(40, 10).unwrap();
    engine
        .open("data:text/html,<title>t</title><body style='background:%2300f'><h1>Hi</h1>")
        .unwrap();
    let devtools = engine.devtools().unwrap();
    let frames = devtools.listen(&["Page.screencastFrame"]);
    let wait = Duration::from_secs(60);
    let start = Instant::now();
    loop {
        let answer = devtools
            .call(
                "Runtime.evaluate",
                json!({"expression": "document.querySelector('h1')?.textContent ?? ''",
                       "returnByValue": true}),
                wait,
            )
            .unwrap();
        if answer["result"]["value"] == "Hi" || start.elapsed() > wait {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        frames.recv_timeout(Duration::from_secs(2)).is_err(),
        "a frame came with the screencast off"
    );
    engine.set_screencast(true).unwrap();
    assert!(
        frames.recv_timeout(Duration::from_secs(30)).is_ok(),
        "no frame came with the screencast on"
    );
}
