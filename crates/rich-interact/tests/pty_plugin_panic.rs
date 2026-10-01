//! A plugin component's panic, caught by its `PluginView` while a session
//! is live, leaves the session as it was: raw mode on, and no panic message
//! painted over the view (0.0.14 release-test audit B1). See `support`.
#![cfg(unix)]

mod support;

use rich_interact::plugin::PluginView;
use rich_interact::policy::Policy;
use rich_interact::{run, Component, Context, Event, Flow, Key, RunOptions, View};
use rich_plugin_api::component::{ComponentContext, ComponentEvent, ComponentFlow, ComponentView};
use rich_plugin_api::PluginComponent;
use support::{assert_restored, Pty};

/// Whether the controlling terminal is in raw (non-canonical) mode now.
fn tty_mode() -> &'static str {
    let tty = std::fs::File::open("/dev/tty").unwrap();
    let out = std::process::Command::new("stty")
        .arg("-a")
        .stdin(tty)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.split_whitespace().any(|flag| flag == "-icanon") {
        "RAW"
    } else {
        "COOKED"
    }
}

/// Panics on `p`.
struct Bomb;

impl PluginComponent for Bomb {
    fn handle(&mut self, event: &ComponentEvent, _: &ComponentContext<'_>) -> ComponentFlow {
        match event.key() {
            Some("p") => panic!("boom"),
            _ => ComponentFlow::Ignored,
        }
    }

    fn render(&self, context: &ComponentContext<'_>) -> ComponentView {
        ComponentView::new(context.markup("bomb ready"))
    }
}

/// The plugin's view, answering on `s` with the terminal's mode then.
struct Probe(PluginView);

impl Component for Probe {
    type Output = String;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<String> {
        if event.key() == Some(Key::char('s')) {
            return Flow::Done(format!(
                "after-panic tty is {}; failure={:?}",
                tty_mode(),
                self.0.failure()
            ));
        }
        self.0.handle(event, context)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.0.render(context)
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[test]
fn child() {
    if std::env::var_os("INTERACT_CHILD").is_none() {
        return;
    }
    let options = RunOptions {
        policy: Policy {
            interactive: Some(true),
            ..Policy::default()
        },
        ..RunOptions::default()
    };
    let probe = Probe(PluginView::new("bomb", Box::new(Bomb)));
    println!("OUTCOME {:?}", run(probe, &options));
}

#[test]
fn a_caught_plugin_panic_keeps_the_session() {
    let mut pty = Pty::start("bomb");
    pty.wait_for("bomb ready");
    pty.send("p");
    pty.wait_for("failed: boom");
    pty.send("s");
    std::thread::sleep(std::time::Duration::from_millis(200));
    // Cooked mode would hold the `s` until a line end.
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("after-panic tty is RAW; failure=Some(\\\"boom\\\")"),
        "{output}"
    );
    assert!(
        !output.contains("panicked at"),
        "panic message painted:\n{output}"
    );
    assert_restored(&output, &parser);
}
