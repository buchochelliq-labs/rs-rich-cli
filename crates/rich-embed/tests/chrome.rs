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

#[test]
fn a_page_arrives_as_a_screencast_and_draws_as_half_blocks() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
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
    assert!(
        red(&driver),
        "no red frame: {:?}, error {:?}",
        driver.screen().plain(),
        h.error().get_untracked()
    );
    assert!(h.address().get_untracked().starts_with("data:text/html"));
}

/// A stand-in browser: a script that writes down its arguments and the
/// mode of its profile directory, then idles as a browser would.
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
    while !argv.exists() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let args = std::fs::read_to_string(&argv).unwrap_or_default();
    let mode = std::fs::read_to_string(dir.join("mode")).unwrap_or_default();
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(args.lines().any(|arg| arg == "--lang=en"), "{args}");
    assert!(
        !args.to_ascii_lowercase().contains("sandbox"),
        "a sandbox switch reached the browser: {args}"
    );
    assert_eq!(mode.trim(), "700", "the profile's mode");
}
