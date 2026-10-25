//! `noctiluca [--chrome PATH] FILE`: a local page's headings and
//! paragraphs, read through a headless Chrome, printed as terminal text.
//! The first spike of docs/design/browser.md, not the browser yet.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use noctiluca::{file_url, render, Reader};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let mut chrome = None;
    let mut page = None;
    while let Some(arg) = args.next() {
        if arg == "--chrome" {
            chrome = args.next().map(PathBuf::from);
        } else {
            page = Some(PathBuf::from(arg));
        }
    }
    let Some(page) = page else {
        eprintln!("usage: noctiluca [--chrome PATH] FILE");
        return ExitCode::from(2);
    };
    let chrome = chrome.or_else(|| std::env::var_os("RICH_EMBED_CHROME").map(PathBuf::from));
    let read = || -> std::io::Result<rich::Text> {
        let url = file_url(&page)?;
        let mut reader = Reader::new(chrome, 100, 40)?;
        reader.load(&url, Duration::from_secs(60))?;
        Ok(render(&reader.blocks()?))
    };
    match read() {
        Ok(text) => {
            rich::Console::new().print(&text);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("noctiluca: {error}");
            ExitCode::FAILURE
        }
    }
}
