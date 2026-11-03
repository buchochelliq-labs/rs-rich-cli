//! The control socket (#816): its private directory, its socket path,
//! read-only sessions, unknown verbs, bounded requests, timeouts and
//! session listing.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rich_interact::control::{
    self, ControlServer, MAX_REQUEST, Request, Verb, check_private_dir, private_dir,
};
use serde_json::{Value, json};

const VERBS: &[Verb] = &[
    Verb::read("info", "who is here"),
    Verb::write("open", "open an address"),
];

/// A fresh scratch directory (not itself private) for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-control-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A private directory in `scratch`, and a server in it answering
/// `info` and `open` with what they were asked.
fn serve(name: &str, read_only: bool, timeout: Duration) -> (PathBuf, control::Closer) {
    let dir = scratch(name).join("sockets");
    private_dir(&dir).unwrap();
    let path = dir.join("app-1.sock");
    let server = ControlServer::bind(&path, VERBS)
        .unwrap()
        .read_only(read_only)
        .timeout(timeout);
    let closer = server.closer();
    std::thread::spawn(move || {
        server.serve(|request: &Request| Ok(json!({"verb": request.verb, "args": request.args})))
    });
    (path, closer)
}

fn call(path: &std::path::Path, verb: &str) -> Result<Value, String> {
    control::call(
        path,
        &Request::new(verb, json!({"url": "x"})),
        Duration::from_secs(5),
    )
    .unwrap()
}

#[test]
fn the_directory_is_made_private_before_anything_is_bound_in_it() {
    let dir = scratch("dir").join("sockets");
    private_dir(&dir).unwrap();
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    // Made again: checked, and still fine.
    private_dir(&dir).unwrap();
}

#[test]
fn a_directory_open_to_others_or_a_symbolic_link_is_refused() {
    let base = scratch("open");
    let open = base.join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = private_dir(&open).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(ControlServer::bind(open.join("app-1.sock"), VERBS).is_err());
    // A link to a private directory is not one.
    let real = base.join("real");
    private_dir(&real).unwrap();
    let link = base.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(check_private_dir(&link).is_err());
    assert!(ControlServer::bind(link.join("app-1.sock"), VERBS).is_err());
}

#[test]
fn the_runtime_directory_falls_back_when_the_variable_is_unsafe() {
    // A relative or missing XDG_RUNTIME_DIR is not used.
    let dir = control::runtime_dir_in("rich-control-test", Some("relative".into())).unwrap();
    assert!(dir.is_absolute());
    check_private_dir(&dir).unwrap();
    let base = scratch("runtime");
    private_dir(&base.join("run")).unwrap();
    let dir = control::runtime_dir_in("app", Some(base.join("run"))).unwrap();
    assert_eq!(dir, base.join("run").join("app"));
    assert!(control::runtime_dir_in("../up", None).is_err());
}

#[test]
fn a_symbolic_link_at_the_socket_path_is_never_followed() {
    let dir = scratch("link").join("sockets");
    private_dir(&dir).unwrap();
    let target = dir.join("elsewhere");
    std::fs::write(&target, "").unwrap();
    let path = dir.join("app-1.sock");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let error = ControlServer::bind(&path, VERBS).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    let error = control::call(
        &path,
        &Request::new("info", json!({})),
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    // The file it pointed at is untouched.
    assert!(target.exists());
}

#[test]
fn a_file_that_is_not_a_left_over_socket_is_not_replaced() {
    let dir = scratch("busy").join("sockets");
    private_dir(&dir).unwrap();
    let path = dir.join("app-1.sock");
    std::fs::write(&path, "keep me").unwrap();
    assert!(ControlServer::bind(&path, VERBS).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me");
    // A socket no one answers is replaced.
    std::fs::remove_file(&path).unwrap();
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    let server = ControlServer::bind(&path, VERBS).unwrap();
    drop(server);
    assert!(!path.exists(), "dropping the server removes its socket");
}

#[test]
fn verbs_are_answered_and_unknown_ones_refused() {
    let (path, closer) = serve("verbs", false, Duration::from_secs(5));
    assert_eq!(call(&path, "info").unwrap()["verb"], "info");
    assert_eq!(call(&path, "open").unwrap()["args"]["url"], "x");
    assert!(call(&path, "rm").unwrap_err().contains("unknown verb"));
    closer.close();
    assert!(!path.exists());
}

#[test]
fn a_read_only_session_refuses_every_verb_that_mutates() {
    let (path, closer) = serve("read-only", true, Duration::from_secs(5));
    assert_eq!(call(&path, "info").unwrap()["verb"], "info");
    let refusal = call(&path, "open").unwrap_err();
    assert!(refusal.contains("read-only"), "{refusal}");
    closer.close();
}

#[test]
fn an_oversized_request_is_refused_unread() {
    let (path, closer) = serve("big", false, Duration::from_secs(5));
    let mut stream = UnixStream::connect(&path).unwrap();
    let big = format!(
        "{{\"verb\":\"info\",\"args\":{{\"x\":\"{}\"}}}}\n",
        "y".repeat(MAX_REQUEST as usize)
    );
    // The server stops reading at its limit; the write may then fail.
    let _ = stream.write_all(big.as_bytes());
    // It answers with a refusal, or (the rest of the request unread)
    // the connection is reset; either way the handler never ran.
    let mut reply = String::new();
    match stream.read_to_string(&mut reply) {
        Ok(_) => assert!(reply.contains("longer than"), "{reply}"),
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::ConnectionReset),
    }
    assert!(!reply.contains("\"ok\":true"), "{reply}");
    assert!(call(&path, "info").is_ok());
    // The client refuses to send one at all.
    let error = control::call(
        &path,
        &Request::new("info", json!({"x": "y".repeat(MAX_REQUEST as usize)})),
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    closer.close();
}

#[test]
fn a_silent_client_times_out() {
    let (path, closer) = serve("slow", false, Duration::from_millis(200));
    let started = Instant::now();
    let mut stream = UnixStream::connect(&path).unwrap();
    let mut reply = String::new();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.read_to_string(&mut reply).unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(reply.contains("bad request"), "{reply}");
    // The server still answers the next one.
    assert!(call(&path, "info").is_ok());
    closer.close();
}

#[test]
fn sessions_lists_this_programs_sockets_only() {
    let dir = scratch("sessions").join("sockets");
    private_dir(&dir).unwrap();
    let a = ControlServer::bind(dir.join("app-1.sock"), VERBS).unwrap();
    let b = ControlServer::bind(dir.join("app-2.sock"), VERBS).unwrap();
    let other = ControlServer::bind(dir.join("other-1.sock"), VERBS).unwrap();
    std::fs::write(dir.join("app-3.sock"), "").unwrap();
    std::os::unix::fs::symlink(dir.join("app-1.sock"), dir.join("app-4.sock")).unwrap();
    let found = control::sessions(&dir, "app-").unwrap();
    assert_eq!(found, [dir.join("app-1.sock"), dir.join("app-2.sock")]);
    drop((a, b, other));
}
