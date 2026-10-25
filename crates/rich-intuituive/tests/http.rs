//! The Slumber clone (examples/http.rs): its collection, its templates, and
//! the app driven by keys against a small HTTP server on 127.0.0.1 that
//! each test starts. The example needs rs-rich-ext's `yaml` and `jsonpath`.

mod common;

#[path = "../examples/http.rs"]
#[allow(dead_code)]
mod http;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use http::*;
use intuituive::{App, Driver};

/// A server that answers each connection on a thread of its own and keeps
/// the requests it was sent.
struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let count = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let seen = seen.clone();
                let count = count.clone();
                std::thread::spawn(move || answer(stream, &seen, &count));
            }
        });
        Server { port, requests }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    fn last(&self) -> String {
        self.requests().last().cloned().unwrap_or_default()
    }
}

fn answer(mut stream: TcpStream, seen: &Mutex<Vec<String>>, count: &AtomicUsize) {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    // The head, then as much body as it says.
    let head_end = loop {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            return;
        }
        raw.extend_from_slice(&buf[..n]);
        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let length: usize = (head.lines())
        .find_map(|l| l.strip_prefix("Content-Length: "))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    while raw.len() < head_end + length {
        let n = stream.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let request = String::from_utf8_lossy(&raw).to_string();
    seen.lock().unwrap().push(request.clone());
    let path = head.split(' ').nth(1).unwrap_or("/").to_string();
    let (status, kind, body) = if path.starts_with("/slow") {
        // Held until the client goes away.
        let _ = stream.read(&mut buf);
        return;
    } else if path.starts_with("/fish") && head.starts_with("POST") {
        let body = String::from_utf8_lossy(&raw[head_end..]).to_string();
        ("201 Created", "application/json", body)
    } else if path.starts_with("/fish") || path.starts_with("/v2/fish") {
        let n = count.fetch_add(1, Ordering::SeqCst) + 1;
        let body = format!(
            r#"{{"call":{n},"fish":[{{"id":1,"name":"Barry","species":"Barracuda"}},{{"id":2,"name":"Fishy","species":"Sunfish"}}]}}"#
        );
        ("200 OK", "application/json", body)
    } else if path.starts_with("/chunked") {
        let chunks = "5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\n\r\n{chunks}"
        );
        let _ = stream.write_all(response.as_bytes());
        return;
    } else {
        ("404 Not Found", "text/plain", "no such fish".to_string())
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nX-Served-By: test\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

const COLLECTION: &str = r##"
profiles:
  local:
    name: Local
    default: true
    data:
      host: http://127.0.0.1:PORT
      greeting: hello
  other:
    name: Other
    data:
      host: http://127.0.0.1:PORT/v2
      greeting: howdy

.base:
  headers:
    Accept: application/json

requests:
  fish:
    name: Fish
    requests:
      list_fish:
        $ref: "#/.base"
        name: List Fish
        method: GET
        url: "{{ host }}/fish"
        query:
          big: "true"
          say: "{{ greeting }}"
      create_fish:
        $ref: "#/.base"
        name: Create Fish
        method: POST
        url: "{{ host }}/fish"
        body:
          type: json
          data: { "name": "Barry", "says": "{{ greeting }}" }
  slow:
    name: Slow
    url: "{{ host }}/slow"
  broken:
    name: Broken
    url: "{{ nowhere }}/x"
  secure:
    name: Secure
    url: https://example.com/
  chunked:
    name: Chunked
    url: "{{ host }}/chunked"
"##;

fn collection(port: u16) -> String {
    COLLECTION.replace("PORT", &port.to_string())
}

/// The collection written for one test.
fn file(name: &str, port: u16) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("intuituive-http-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.yml"));
    std::fs::write(&path, collection(port)).unwrap();
    path
}

fn app(name: &str, server: &Server) -> App {
    http_app(file(name, server.port)).wait_for_tasks(true)
}

fn driver(name: &str, server: &Server) -> Driver {
    let mut driver = app(name, server).driver(100, 30);
    driver.settle();
    driver
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn text(driver: &Driver) -> String {
    rows(driver).join("\n")
}

/// The body pane's rows: those between the tab strip and the filter box.
fn has(driver: &Driver, needle: &str) -> bool {
    text(driver).contains(needle)
}

#[track_caller]
fn assert_has(driver: &Driver, needle: &str) {
    assert!(
        has(driver, needle),
        "no {needle:?} on the screen:\n{}",
        text(driver)
    );
}

#[track_caller]
fn assert_lacks(driver: &Driver, needle: &str) {
    assert!(
        !has(driver, needle),
        "{needle:?} on the screen:\n{}",
        text(driver)
    );
}

// -- The collection, templates and requests, without the app ------------------

#[test]
fn it_reads_slumbers_collection_format() {
    let c = parse_collection(&collection(8000)).unwrap();
    let names: Vec<&str> = c.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Local", "Other"]);
    assert!(c.profiles[0].default);
    let Entry::Folder { name, children, .. } = &c.recipes[0] else {
        panic!("a folder first: {:?}", c.recipes[0]);
    };
    assert_eq!(name, "Fish");
    let Entry::Recipe(list) = &children[0] else {
        panic!("a recipe in it");
    };
    assert_eq!(
        (list.id.as_str(), list.method.as_str()),
        ("list_fish", "GET")
    );
    assert_eq!(list.url, "{{ host }}/fish");
    // `$ref: "#/.base"` brings the shared headers; the `.base` key is not
    // a recipe.
    assert_eq!(
        list.headers,
        [("Accept".to_string(), "application/json".to_string())]
    );
    assert_eq!(
        list.query[1],
        ("say".to_string(), "{{ greeting }}".to_string())
    );
    let Entry::Recipe(create) = &children[1] else {
        panic!("a second recipe");
    };
    assert_eq!(
        create.body,
        Body::Json("{\n  \"name\": \"Barry\",\n  \"says\": \"{{ greeting }}\"\n}".into())
    );
    assert_eq!(c.recipes.len(), 5, "{:?}", c.recipes);
}

#[test]
fn templates_read_profile_fields_and_name_the_unknown() {
    let fields = vec![
        ("host".to_string(), "http://h:1".to_string()),
        ("api".to_string(), "{{ host }}/api".to_string()),
    ];
    assert_eq!(
        render("{{api}}/fish", &fields).unwrap(),
        "http://h:1/api/fish"
    );
    assert_eq!(
        render("{{ nope }}", &fields).unwrap_err(),
        "Unknown field `nope`"
    );
    let looped = vec![("a".to_string(), "{{ a }}".to_string())];
    assert!(render("{{ a }}", &looped)
        .unwrap_err()
        .contains("refers to itself"));
}

#[test]
fn a_recipe_builds_with_its_edits() {
    let c = parse_collection(&collection(8000)).unwrap();
    let Entry::Folder { children, .. } = &c.recipes[0] else {
        unreachable!()
    };
    let Entry::Recipe(list) = &children[0] else {
        unreachable!()
    };
    let fields = c.profiles[0].data.clone();
    let built = build(list, &fields, &Edits::default()).unwrap();
    assert_eq!(built.url, "http://127.0.0.1:8000/fish?big=true&say=hello");
    let mut edits = Edits::default();
    edits.query_off.insert(0);
    edits.query.insert(1, "hi there".into());
    edits.headers.insert(0, "text/plain".into());
    let built = build(list, &fields, &edits).unwrap();
    assert_eq!(built.url, "http://127.0.0.1:8000/fish?say=hi+there");
    let header = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(
        built.headers,
        [
            header("Accept", "text/plain"),
            header("User-Agent", "slumber-intuituive")
        ]
    );
    assert_eq!(
        curl(&built),
        "curl -XGET --url 'http://127.0.0.1:8000/fish?say=hi+there' --header 'Accept: text/plain' \
         --header 'User-Agent: slumber-intuituive'"
    );
}

#[test]
fn the_filter_takes_jsonpath_or_a_shell_command() {
    let body = r#"{"fish": [{"name": "Barry"}, {"name": "Fishy"}]}"#;
    assert_eq!(filter(body, "$.fish[1].name").unwrap(), "\"Fishy\"");
    assert_eq!(
        filter(body, "$.fish[*].name").unwrap(),
        "[\n  \"Barry\",\n  \"Fishy\"\n]"
    );
    assert_eq!(filter("a\nb\nc\n", "tail -n 1").unwrap(), "c\n");
    assert!(filter(body, "$.[").is_err());
}

#[test]
fn the_client_speaks_http_over_a_socket() {
    let server = Server::start();
    let request = Built {
        method: "POST".into(),
        url: format!("http://127.0.0.1:{}/fish", server.port),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: Some("{\"a\":1}".into()),
    };
    let response = send(&request, &Conn::default()).unwrap();
    assert_eq!(
        (response.status, response.reason.as_str()),
        (201, "Created")
    );
    assert_eq!(response.body, b"{\"a\":1}");
    let seen = server.last();
    assert!(seen.starts_with("POST /fish HTTP/1.1\r\n"), "{seen}");
    assert!(seen.contains("Content-Length: 7\r\n"), "{seen}");
    // Chunked bodies are put back together; https is refused, not tried.
    let chunked = Built {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{}/chunked", server.port),
        headers: Vec::new(),
        body: None,
    };
    assert_eq!(
        send(&chunked, &Conn::default()).unwrap().body,
        b"hello world"
    );
    let secure = Built {
        url: "https://example.com/".into(),
        ..chunked
    };
    assert!(send(&secure, &Conn::default()).unwrap_err().contains("TLS"));
}

// -- The app ------------------------------------------------------------------

#[test]
fn it_shows_slumbers_main_view() {
    let server = Server::start();
    let d = driver("main", &server);
    let rows = rows(&d);
    assert!(rows[0].starts_with("╭─ Recipe [r] ─"), "{}", rows[0]);
    assert!(rows[0].contains("╭─ Profile [p] ─"), "{}", rows[0]);
    assert!(rows[1].starts_with("│▾ Fish"), "{}", rows[1]);
    assert!(rows[2].starts_with("│    List Fish"), "{}", rows[2]);
    assert!(rows[1].contains("│Local"), "{}", rows[1]);
    // The selected pane double-bordered, as Slumber's; the recipe's id on
    // its top edge.
    assert_has(&d, "╔═ Recipe [1] ═");
    assert_has(&d, "URL │ Query │ Headers │ Body │ Authentication");
    assert_has(&d, " list_fish ═╗");
    assert_has(&d, &format!("GET http://127.0.0.1:{}/fish", server.port));
    assert_has(&d, "╭─ Request / Response [2] ─");
    assert_has(&d, "History [ctrl h]");
    assert_has(&d, "No request history for this recipe & profile");
    assert!(
        rows[29].ends_with("x actions / ? help / q quit"),
        "{}",
        rows[29]
    );
}

#[test]
fn enter_sends_the_request_and_shows_the_json_body() {
    let server = Server::start();
    let mut d = driver("send", &server);
    d.press("enter");
    let seen = server.last();
    assert!(
        seen.starts_with("GET /fish?big=true&say=hello HTTP/1.1\r\n"),
        "{seen}"
    );
    assert!(
        seen.contains(&format!("Host: 127.0.0.1:{}\r\n", server.port)),
        "{seen}"
    );
    assert!(seen.contains("Accept: application/json\r\n"), "{seen}");
    assert_has(&d, " 200 OK ");
    assert_has(&d, "Request │ Body │ Headers");
    // Pretty, as rich's JSON lays it out.
    assert_has(&d, "│  \"call\": 1,");
    assert_has(&d, "\"name\": \"Barry\",");
    assert_has(&d, "'/' to filter body with JSONPath or a shell command");
}

#[test]
fn the_exchange_tabs_show_the_request_and_the_response_headers() {
    let server = Server::start();
    let mut d = driver("tabs", &server);
    d.press("enter 2 h");
    let url = format!(
        "GET http://127.0.0.1:{}/fish?big=true&say=hello",
        server.port
    );
    assert_has(&d, &url);
    assert_has(&d, "Accept: application/json");
    assert_has(&d, "User-Agent: slumber-intuituive");
    d.press("l l");
    assert_has(&d, "X-Served-By");
    assert_has(&d, "Content-Length");
    // ← wraps round to the last tab, → to the first.
    d.press("l");
    assert_has(&d, &url);
}

#[test]
fn space_turns_a_parameter_off_and_e_edits_a_header() {
    let server = Server::start();
    let mut d = driver("edit", &server);
    d.press("l");
    assert_has(&d, "▣ big       true");
    d.press("j space");
    assert_has(&d, "□ say       hello");
    d.press("enter");
    assert!(
        server.last().starts_with("GET /fish?big=true HTTP/1.1"),
        "{}",
        server.last()
    );
    // `e` edits the header's value in a box over its row.
    d.press("l e");
    assert_has(&d, "Accept");
    d.press("ctrl+u");
    d.type_text("text/plain");
    d.press("enter");
    assert_has(&d, "▣ Accept text/plain");
    d.press("enter");
    assert!(
        server.last().contains("Accept: text/plain\r\n"),
        "{}",
        server.last()
    );
    // `z` resets it.
    d.press("z");
    assert_has(&d, "▣ Accept application/json");
}

#[test]
fn a_profile_changes_what_the_templates_fill_in() {
    let server = Server::start();
    let mut d = driver("profiles", &server);
    d.press("p");
    assert_has(&d, "╔═ Profile [p] ═");
    assert_has(&d, "║Other");
    // The bottom pane shows the profile's fields, the recipe goes up top.
    assert_has(&d, "╭─ Profile ─");
    assert_has(&d, "greeting hello");
    assert_has(&d, "│List Fish");
    d.press("j");
    assert_has(&d, "greeting howdy");
    d.press("r enter");
    let seen = server.last();
    assert!(
        seen.starts_with("GET /v2/fish?big=true&say=howdy HTTP/1.1"),
        "{seen}"
    );
    assert_has(&d, "│Other");
}

#[test]
fn the_filter_box_runs_jsonpath_or_a_shell_command() {
    let server = Server::start();
    let mut d = driver("filter", &server);
    d.press("enter 2 slash");
    d.type_text("$.fish[*].name");
    d.press("enter");
    assert_has(&d, "\"Fishy\"");
    assert_lacks(&d, "species");
    d.press("slash ctrl+u");
    d.type_text("grep Fishy");
    d.press("enter");
    assert_has(&d, "\"name\": \"Fishy\",");
    assert_lacks(&d, "Barry");
    // An empty query shows the whole body again.
    d.press("slash ctrl+u enter");
    assert_has(&d, "Barry");
}

#[test]
fn the_history_lists_each_request_and_shows_the_one_picked() {
    let server = Server::start();
    let mut d = driver("history", &server);
    d.press("enter enter");
    assert_has(&d, "\"call\": 2,");
    d.press("ctrl+h");
    assert_has(&d, "╔═ Request History [ctrl h] ═");
    let ok = rows(&d).iter().filter(|r| r.contains(" 200 OK ")).count();
    assert_eq!(ok, 3, "two history rows and the status line:\n{}", text(&d));
    d.press("j");
    assert_has(&d, "\"call\": 1,");
    // Sending again shows the newest.
    d.press("enter");
    assert_has(&d, "\"call\": 3,");
}

#[test]
fn esc_cancels_a_request_in_flight() {
    let server = Server::start();
    // Not waiting for tasks: the request never answers.
    let mut d = http_app(file("cancel", server.port)).driver(100, 30);
    d.press("r j j enter");
    assert_has(&d, "Loading...");
    d.press("esc");
    assert_has(&d, "Cancel request?");
    d.press("y");
    assert_has(&d, "Request cancelled");
    d.press("ctrl+h");
    assert_has(&d, "Cancelled");
}

#[test]
fn template_and_transport_errors_show_in_the_exchange_pane() {
    let server = Server::start();
    let mut d = driver("errors", &server);
    d.press("r j j j enter");
    assert_has(&d, "Build error");
    assert_has(&d, "Unknown field `nowhere`");
    d.press("r j enter");
    assert_has(&d, "Request error");
    assert_has(&d, "https needs TLS");
}

#[test]
fn a_post_sends_its_templated_json_body() {
    let server = Server::start();
    let mut d = driver("post", &server);
    d.press("r j");
    assert_has(&d, " create_fish ─╮");
    d.press("1 l l l");
    // The body tab shows the JSON with its template filled in.
    assert_has(&d, "\"says\": \"hello\"");
    d.press("enter");
    let seen = server.last();
    assert!(seen.starts_with("POST /fish HTTP/1.1"), "{seen}");
    assert!(
        seen.contains("Content-Type: application/json\r\n"),
        "{seen}"
    );
    assert!(
        seen.ends_with("{\n  \"name\": \"Barry\",\n  \"says\": \"hello\"\n}"),
        "{seen}"
    );
    assert_has(&d, " 201 Created ");
}

#[test]
fn e_on_the_body_hands_it_to_the_editor() {
    let server = Server::start();
    let mut d = driver("editor", &server);
    d.press("r j 1 l l l e");
    let handoff = d.take_handoff().expect("the editor is run");
    let file = PathBuf::from(handoff.command.get_args().last().unwrap());
    let body = std::fs::read_to_string(&file).unwrap();
    assert!(
        body.contains("{{ greeting }}"),
        "the template, not its value: {body}"
    );
    std::fs::write(&file, "{\"name\": \"Edited\"}").unwrap();
    d.handed_back(handoff, Ok(Some(0)));
    d.settle();
    assert_has(&d, "\"name\": \"Edited\"");
    d.press("enter");
    assert!(
        server.last().ends_with("{\"name\": \"Edited\"}"),
        "{}",
        server.last()
    );
}

#[test]
fn layout_keys_hide_the_sidebar_and_fill_the_screen() {
    let server = Server::start();
    let mut d = driver("layout", &server);
    d.press("s");
    assert_lacks(&d, "▾ Fish");
    // Both headers show while the sidebar is closed.
    let top = &rows(&d)[1];
    assert!(
        top.contains("│Local") && top.contains("│List Fish"),
        "{top}"
    );
    d.press("f");
    assert_lacks(&d, "Request / Response");
    assert_lacks(&d, "│Local");
    assert_has(&d, "╔═ Recipe [1] ═");
    d.press("esc");
    assert_has(&d, "Request / Response");
    // Tab moves to the bottom pane, and fullscreen shows that one.
    d.press("tab f");
    assert_has(&d, "Request / Response");
    assert_lacks(&d, "Recipe [1]");
    // In fullscreen, Tab moves on to the hidden pane and shows it in its
    // place (the sidebar is closed, so it is passed by), and round.
    d.press("tab");
    assert_has(&d, "Recipe [1]");
    assert_lacks(&d, "Request / Response");
    d.press("tab");
    assert_has(&d, "Request / Response");
    assert_lacks(&d, "Recipe [1]");
    d.press("shift+tab");
    assert_has(&d, "╔═ Recipe [1] ═");
}

#[test]
fn help_lists_the_keys_by_section() {
    let server = Server::start();
    let mut d = driver("help", &server);
    // The keys of the focused pane (the recipe's) come first, then the
    // app's.
    d.press("question");
    assert_has(&d, "╭─ Help ─");
    for line in [
        "Recipe",
        "Toggle row",
        "Reset edit",
        "Pane Navigation",
        "Request history",
    ] {
        assert_has(&d, line);
    }
    // Typing searches them.
    d.type_text("send");
    assert_has(&d, "Send request");
    assert_lacks(&d, "Toggle row");
}

#[test]
fn the_actions_menu_copies_the_request_as_curl() {
    let server = Server::start();
    let mut d = driver("actions", &server);
    d.set_clipboard(true);
    d.press("enter x");
    assert_has(&d, "Copy as cURL");
    d.press("j enter");
    let copies = d.take_copies();
    let url = format!("http://127.0.0.1:{}/fish?big=true&say=hello", server.port);
    assert_eq!(
        copies,
        [format!(
            "curl -XGET --url '{url}' --header 'Accept: application/json' \
             --header 'User-Agent: slumber-intuituive'"
        )]
    );
}

#[test]
fn f5_reads_the_collection_again() {
    let server = Server::start();
    let path = file("reload", server.port);
    let mut d = http_app(path.clone()).wait_for_tasks(true).driver(100, 30);
    d.settle();
    let changed = collection(server.port).replace("name: List Fish", "name: All the Fish");
    std::fs::write(&path, changed).unwrap();
    d.press("f5");
    assert_has(&d, "All the Fish");
    assert_has(&d, "Reloaded collection from");
}

#[test]
fn a_click_selects_the_pane_and_tab_goes_on_from_it() {
    use intuituive::interact::{Button, Event, Mouse, MouseKind};
    let server = Server::start();
    let mut d = driver("click", &server);
    // A click in the sidebar's tree selects the sidebar pane (its border
    // doubles); Tab then goes to the next pane, the top one.
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        d.send(Event::Mouse(Mouse::new(kind, 5, 2)));
    }
    assert_has(&d, "╔═ Recipe [r] ═");
    d.press("tab");
    assert_has(&d, "╭─ Recipe [r] ─");
    assert_has(&d, "╔═ Recipe [1] ═");
}

#[test]
fn a_reload_keeps_the_selected_recipe_where_it_moved() {
    let server = Server::start();
    let path = file("moved", server.port);
    let mut d = http_app(path.clone()).wait_for_tasks(true).driver(100, 30);
    d.press("r j");
    assert_has(&d, " create_fish ─╮");
    // A recipe added before it: the tree keeps its selection by id.
    let changed = collection(server.port).replace(
        "    requests:\n      list_fish:",
        "    requests:\n      new_fish:\n        name: New Fish\n        url: \"{{ host }}/fish\"\n      list_fish:",
    );
    std::fs::write(&path, changed).unwrap();
    d.press("f5");
    assert_has(&d, "New Fish");
    assert_has(&d, " create_fish ─╮");
}
