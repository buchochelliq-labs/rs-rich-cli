//! The DOM renderer: its messages for a small app at the protocol level
//! (cells, styles, the caret, the accessibility tree and its focus,
//! announcements), and the page and a session over real sockets on
//! 127.0.0.1.

mod common;

use std::time::Duration;

use common::{connect_with, get, read_close, read_until, TOKEN};
use rich_web::dom::{self, Dom};
use rich_web::intuituive::interact::{Event, Input, Key};
use rich_web::intuituive::prelude::*;
use rich_web::intuituive::Driver;
use rich_web::{Renderer, Server};
use serde_json::{json, Value};
use tungstenite::Message;

/// A heading, a list of three files and a Save button; F5 announces, F6
/// copies.
fn app() -> App {
    App::new(|| {
        let picked = signal(0usize);
        column([
            label("Files").label("Heading").auto(),
            list(
                || vec!["a.txt".into(), "b.txt".into(), "c.txt".into()],
                picked,
            )
            .label("Files")
            .fixed(3),
            label("Save")
                .auto()
                .focusable()
                .on_click(|cx| cx.announce("Saved", false)),
        ])
        .on_key("f5", |cx| cx.announce("Saved", true))
        .on_key("f6", |cx| cx.copy("abc"))
    })
}

/// Bring `driver` up to date and draw: the messages `dom` sends for it.
fn step(driver: &mut Driver, dom: &mut Dom) -> Vec<Value> {
    driver.update(Duration::ZERO);
    let _ = driver.render();
    dom.update(driver)
        .iter()
        .map(|m| serde_json::from_str(m).unwrap())
        .collect()
}

fn find<'a>(messages: &'a [Value], t: &str) -> Option<&'a Value> {
    messages.iter().find(|m| m["t"] == t)
}

/// A line's text, from its runs.
fn line_text(runs: &Value) -> String {
    runs.as_array()
        .unwrap()
        .iter()
        .map(|run| run[0].as_str().unwrap())
        .collect()
}

fn node_named<'a>(tree: &'a Value, role: &str) -> &'a Value {
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["attrs"].as_array().unwrap()[0] == json!(["role", role]))
        .unwrap_or_else(|| panic!("a {role} in {tree}"))
}

#[test]
fn the_first_messages_draw_every_line_and_the_tree() {
    let mut driver = app().driver(20, 6);
    let mut dom = Dom::new();
    assert_eq!(dom.hello(), r#"{"t":"hello","protocol":1}"#);
    let messages = step(&mut driver, &mut dom);
    assert_eq!(messages.len(), 2, "{messages:?}");

    let frame = find(&messages, "frame").unwrap();
    assert_eq!(
        (frame["cols"].as_u64(), frame["rows"].as_u64()),
        (Some(20), Some(6))
    );
    let lines = frame["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 6, "every line, the first time");
    let text: Vec<String> = lines.iter().map(|l| line_text(&l[1])).collect();
    assert_eq!(text[0].trim_end(), "Files");
    assert!(text[1].contains("a.txt"), "{text:?}");
    assert!(text.iter().any(|l| l.contains("Save")), "{text:?}");
    assert_eq!(frame["cursor"], Value::Null);
    // Styles are sent once, as CSS, by number; runs refer to them.
    for style in frame["styles"].as_array().unwrap() {
        assert!(style[0].as_u64().unwrap() >= 1);
        assert!(style[1].as_str().unwrap().contains(':'), "{style}");
    }

    let tree = find(&messages, "tree").unwrap();
    let heading = &tree["nodes"][0];
    assert_eq!(
        heading["attrs"],
        json!([["role", "text"], ["aria-label", "Heading"]])
    );
    assert_eq!(heading["text"], "Heading");
    let list = node_named(tree, "list");
    assert_eq!(
        list["attrs"],
        json!([["role", "list"], ["aria-label", "Files"]])
    );
    assert_eq!(
        list["item"],
        json!({
            "role": "listitem",
            "attrs": [["aria-selected", "true"], ["aria-posinset", "1"], ["aria-setsize", "3"]],
            "text": "a.txt",
        })
    );
    assert_eq!(list["rect"][1], 1, "below the heading");
    assert_eq!(tree["focus"], list["id"], "the list has the focus");
    let button = node_named(tree, "button");
    assert_eq!(
        button["attrs"],
        json!([["role", "button"], ["aria-label", "Save"]])
    );

    // Nothing changed: nothing to send.
    assert_eq!(step(&mut driver, &mut dom), Vec::<Value>::new());
}

#[test]
fn changes_send_only_what_changed() {
    let mut driver = app().driver(20, 6);
    let mut dom = Dom::new();
    step(&mut driver, &mut dom);

    // Down: the next file is selected; a few lines and the item change.
    driver.event(Event::Key(Key::parse("down").unwrap()));
    let messages = step(&mut driver, &mut dom);
    let frame = find(&messages, "frame").expect("a frame");
    let lines = frame["lines"].as_array().unwrap();
    assert!(!lines.is_empty() && lines.len() < 6, "{lines:?}");
    let tree = find(&messages, "tree").expect("a tree");
    let item = &node_named(tree, "list")["item"];
    assert_eq!(item["text"], "b.txt");
    assert_eq!(item["attrs"][1], json!(["aria-posinset", "2"]));

    // Tab: the focus moves to the button.
    driver.event(Event::Key(Key::parse("tab").unwrap()));
    let messages = step(&mut driver, &mut dom);
    let tree = find(&messages, "tree").expect("a tree");
    assert_eq!(tree["focus"], node_named(tree, "button")["id"]);

    // A resize draws every line again.
    driver.event(Event::Resize {
        columns: 24,
        rows: 7,
    });
    let messages = step(&mut driver, &mut dom);
    let frame = find(&messages, "frame").expect("a frame");
    assert_eq!(frame["lines"].as_array().unwrap().len(), 7);
    assert_eq!(frame["cols"], 24);
}

#[test]
fn announcements_and_copies_become_messages() {
    let mut driver = app().driver(20, 6);
    let mut dom = Dom::new();
    step(&mut driver, &mut dom);
    driver.event(Event::Key(Key::parse("f5").unwrap()));
    step(&mut driver, &mut dom);
    let said: Vec<Value> = driver
        .take_announcements()
        .iter()
        .map(|a| serde_json::from_str(&dom::say(a)).unwrap())
        .collect();
    assert_eq!(said, [json!({"t": "say", "text": "Saved", "urgent": true})]);
    assert_eq!(
        serde_json::from_str::<Value>(&dom::copy(3, "abc")).unwrap(),
        json!({"t": "copy", "n": 3, "text": "abc"})
    );
}

#[test]
fn styles_wide_characters_and_the_caret() {
    let app = App::new(|| {
        column([
            label("[bold red]R[/] 中x").fixed(1),
            repeating(|| Input::new("Say"), |_line: String, _| {}),
        ])
    });
    let mut driver = app.driver(12, 4);
    let mut dom = Dom::new();
    let messages = step(&mut driver, &mut dom);
    let frame = find(&messages, "frame").unwrap();
    let styles = frame["styles"].as_array().unwrap();
    let red = styles
        .iter()
        .find(|s| s[1] == "color: #cc0000; text-decoration-color: #cc0000; font-weight: bold")
        .expect("bold red, as CSS");
    let first = &frame["lines"][0][1];
    assert_eq!(first[0], json!(["R", red[0]]));
    let wide = first
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run[0] == "中")
        .expect("the wide character");
    assert_eq!(wide[2], 2, "marked wide");
    // The text box has the focus: its caret is on the screen.
    let cursor = frame["cursor"].as_array().expect("a caret");
    assert!(
        cursor[1].as_u64().unwrap() >= 1,
        "below the label: {cursor:?}"
    );
    let tree = find(&messages, "tree").unwrap();
    assert_eq!(node_named(tree, "textbox")["id"], tree["focus"]);
}

#[test]
fn the_page_is_chosen_by_the_server_or_the_address() {
    let dom_server = Server::bind("127.0.0.1:0", app)
        .unwrap()
        .token(TOKEN)
        .renderer(Renderer::Dom)
        .title("Files")
        .spawn()
        .unwrap();
    let xterm_server = Server::bind("127.0.0.1:0", app)
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    let is_dom = |page: &str| page.contains("<script src=\"dom.js\"></script>");
    let page = |addr, query: &str| {
        let (status, page) = get(addr, &format!("/?token={TOKEN}{query}"));
        assert!(status.contains("200"), "{status}");
        page
    };
    let dom_addr = dom_server.local_addr();
    let xterm_addr = xterm_server.local_addr();
    let dom_page = page(dom_addr, "");
    assert!(is_dom(&dom_page));
    assert!(dom_page.contains("role=\"application\" aria-label=\"Files\""));
    assert!(dom_page.contains("aria-live=\"assertive\""));
    assert!(dom_page.contains("Content-Security-Policy: default-src 'none'"));
    assert!(!is_dom(&page(dom_addr, "&renderer=xterm")));
    assert!(!is_dom(&page(xterm_addr, "")));
    assert!(is_dom(&page(xterm_addr, "&renderer=dom")));
    assert!(!is_dom(&page(xterm_addr, "&renderer=canvas")));
    // The page's script, served by the crate; no token needed for it.
    let (status, script) = get(xterm_addr, "/dom.js");
    assert!(status.contains("200"), "{status}");
    assert!(script.contains("var PROTOCOL = 1;"));
    // Without the token, no page of either kind.
    let (status, _) = get(dom_addr, "/?renderer=dom");
    assert!(status.contains("403"), "{status}");
}

#[test]
fn a_dom_session_runs_end_to_end() {
    let server = Server::bind("127.0.0.1:0", app)
        .unwrap()
        .token(TOKEN)
        .spawn()
        .unwrap();
    let mut socket = connect_with(server.local_addr(), 20, 6, "&renderer=dom");
    let first = read_until(&mut socket, r#""t":"tree""#);
    assert!(
        first.starts_with(r#"{"t":"hello","protocol":1}"#),
        "{first}"
    );
    assert!(first.contains(r#""t":"frame""#));
    assert!(!first.contains("\x1b["), "no terminal output");

    // Keys as the page sends them: Down, then F5 (an announcement).
    socket.send(Message::text("d\x1b[B")).unwrap();
    read_until(&mut socket, r#"["aria-posinset","2"]"#);
    socket.send(Message::text("d\x1b[15~")).unwrap();
    read_until(&mut socket, r#"{"t":"say","text":"Saved","urgent":true}"#);

    // F6 copies; once the page says the clipboard took it, a toast says so,
    // and is announced.
    socket.send(Message::text("d\x1b[17~")).unwrap();
    read_until(&mut socket, r#"{"t":"copy","n":1,"text":"abc"}"#);
    socket.send(Message::text("c1:1")).unwrap();
    read_until(&mut socket, "Copied 3 characters");

    // A click on the button, as an SGR report.
    let save_row = 5;
    socket
        .send(Message::text(format!(
            "d\x1b[<0;2;{save_row}M\x1b[<0;2;{save_row}m"
        )))
        .unwrap();
    read_until(&mut socket, r#"{"t":"say","text":"Saved","urgent":false}"#);

    // Ctrl+C quits: the session closes, with no terminal teardown.
    socket.send(Message::text("d\x03")).unwrap();
    let last = read_close(&mut socket);
    assert!(!last.contains("\x1b["), "{last:?}");
}
