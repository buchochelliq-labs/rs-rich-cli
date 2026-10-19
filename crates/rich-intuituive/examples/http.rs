//! A terminal HTTP client in the style of
//! [Slumber](https://github.com/LucasPickering/slumber), a ratatui app,
//! rebuilt on intuiTUIve. It reuses none of Slumber's code: it is a rebuild
//! of its main view, to learn what the framework lacks for it.
//!
//! Ported from Slumber (<https://github.com/LucasPickering/slumber>), by
//! Lucas Pickering, MIT licence. Its design, keys, collection format and
//! layout are his; the credit for them is his.
//!
//!     cargo run -p rs-rich-intuituive --example http \
//!         --features rich-ext/yaml,rich-ext/jsonpath,watch -- slumber.yml
//!
//! A collection is Slumber's YAML: `profiles` (each with `data` fields that
//! templates read as `{{ host }}`) and `requests` (recipes, or folders with
//! `requests` of their own), with `$ref: "#/.base"` to reuse a map.
//! Requests go over plain HTTP/1.1 on `std::net::TcpStream`: no TLS, no
//! proxy, no compression (see docs/design/gaps/http.md). With the crate's
//! `watch` feature the collection is read again when its file changes.
//!
//! Keys, as Slumber's defaults: enter sends · esc cancels · tab / shift+tab
//! and 1 / 2 move between panes · r recipes · p profiles · ctrl+h history ·
//! s sidebar · f fullscreen · [ ] resize · ← → (h l) tabs · space toggles a
//! row · e edits · z resets an edit · / filters the body (JSONPath, or a
//! shell command) · : exports it to a command · v views it in a pager ·
//! x actions · F5 reloads · ? help · q quits.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use intuituive::prelude::*;
use intuituive::widgets::{
    table_with, tabs, tree_with, vsplit, Column, RowRects, TableOptions, TreeItem,
};
use intuituive::{Offset, Origin, PromptOptions};
use rich::markup::escape;
use rich::Style;
use rich_ext::data::{self, Format, Selectors, Value};

type Doc = rich_ext::data::Node;

// ---------------------------------------------------------------------------
// The collection
// ---------------------------------------------------------------------------

/// A collection file: profiles and recipes, as Slumber's `slumber.yml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collection {
    pub profiles: Vec<Profile>,
    pub recipes: Vec<Entry>,
}

/// A set of template fields: `{{ host }}` reads `host`.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub data: Vec<(String, String)>,
}

/// A recipe, or a folder of them.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Folder {
        id: String,
        name: String,
        children: Vec<Entry>,
    },
    Recipe(Recipe),
}

/// A request's template.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Body,
    pub auth: Option<Auth>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    None,
    /// Text sent as it is (after templating).
    Raw(String),
    /// JSON, kept as pretty text whose strings may hold templates.
    Json(String),
    /// `application/x-www-form-urlencoded` fields.
    Form(Vec<(String, String)>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Auth {
    Basic { username: String, password: String },
    Bearer(String),
}

/// Parse a collection from Slumber's YAML (rs-rich-ext's `data` parser).
pub fn parse_collection(text: &str) -> Result<Collection, String> {
    let root = data::parse(Format::Yaml, text).map_err(|e| e.to_string())?;
    let root = resolve_refs(&root, &root, 0)?;
    let mut collection = Collection::default();
    if let Some(Value::Map(profiles)) = root.get("profiles").map(|n| &n.value) {
        for (id, node) in profiles {
            let data = match node.get("data") {
                Some(data) => pairs(data)?,
                None => Vec::new(),
            };
            collection.profiles.push(Profile {
                id: id.clone(),
                name: node.get("name").map_or(Ok(id.clone()), scalar)?,
                default: matches!(
                    node.get("default").map(|n| &n.value),
                    Some(Value::Bool(true))
                ),
                data,
            });
        }
    }
    if let Some(requests) = root.get("requests") {
        collection.recipes = entries(requests)?;
    }
    Ok(collection)
}

/// Replace each `$ref: "#/a/b"` with the map it points at, the keys beside
/// it winning: Slumber's way of sharing headers and profile data.
fn resolve_refs(node: &Doc, root: &Doc, depth: usize) -> Result<Doc, String> {
    if depth > 64 {
        return Err("$ref: the references go round in a loop".into());
    }
    match &node.value {
        Value::Map(map) => {
            let mut out: Vec<(String, Doc)> = Vec::new();
            if let Some(target) = node.get("$ref") {
                let target = target.as_str().ok_or("$ref takes a string")?;
                let path = target
                    .strip_prefix("#/")
                    .ok_or_else(|| format!("$ref `{target}`: only `#/…` references are read"))?;
                let mut at = root;
                for part in path.split('/') {
                    at = at
                        .get(part)
                        .ok_or_else(|| format!("$ref `{target}`: no `{part}` there"))?;
                }
                if let Value::Map(base) = resolve_refs(at, root, depth + 1)?.value {
                    out = base;
                }
            }
            for (key, value) in map.iter().filter(|(k, _)| k != "$ref") {
                let value = resolve_refs(value, root, depth + 1)?;
                match out.iter_mut().find(|(k, _)| k == key) {
                    Some(slot) => slot.1 = value,
                    None => out.push((key.clone(), value)),
                }
            }
            Ok(Doc::new(Value::Map(out)))
        }
        Value::Seq(items) => Ok(Doc::new(Value::Seq(
            items
                .iter()
                .map(|item| resolve_refs(item, root, depth + 1))
                .collect::<Result<_, _>>()?,
        ))),
        _ => Ok(node.clone()),
    }
}

/// A scalar as the text a template sees.
fn scalar(node: &Doc) -> Result<String, String> {
    Ok(match &node.value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) | Value::DateTime(s) => s.clone(),
        Value::Seq(_) | Value::Map(_) => node.to_json().to_string(),
    })
}

/// A map's entries as text; a list value repeats its key (`?a=1&a=2`).
fn pairs(node: &Doc) -> Result<Vec<(String, String)>, String> {
    let Value::Map(map) = &node.value else {
        return Err(format!("expected a map, found {}", node.type_name()));
    };
    let mut out = Vec::new();
    for (key, value) in map {
        match &value.value {
            Value::Seq(items) => {
                for item in items {
                    out.push((key.clone(), scalar(item)?));
                }
            }
            _ => out.push((key.clone(), scalar(value)?)),
        }
    }
    Ok(out)
}

fn entries(node: &Doc) -> Result<Vec<Entry>, String> {
    let Value::Map(map) = &node.value else {
        return Err("`requests` must be a map".into());
    };
    let mut out = Vec::new();
    for (id, node) in map.iter().filter(|(id, _)| !id.starts_with('.')) {
        let name = node.get("name").map_or(Ok(id.clone()), scalar)?;
        if let Some(children) = node.get("requests") {
            out.push(Entry::Folder {
                id: id.clone(),
                name,
                children: entries(children)?,
            });
            continue;
        }
        let url = node
            .get("url")
            .ok_or_else(|| format!("recipe `{id}` has no url"))?;
        let field = |key: &str| node.get(key).map_or(Ok(Vec::new()), pairs);
        let body = match node.get("body") {
            None => Body::None,
            Some(body) => match (&body.value, body.get("type").and_then(Doc::as_str)) {
                (Value::String(text), _) => Body::Raw(text.clone()),
                (_, Some("json")) => {
                    let json =
                        (body.get("data")).map_or("null".into(), |d| d.to_json().to_string());
                    Body::Json(pretty_json(&json).unwrap_or(json))
                }
                (_, Some("form_urlencoded")) => {
                    Body::Form(body.get("data").map_or(Ok(Vec::new()), pairs)?)
                }
                (_, other) => {
                    return Err(format!("recipe `{id}`: body type {other:?} is not read"))
                }
            },
        };
        let auth = match node.get("authentication") {
            None => None,
            Some(auth) => {
                let get = |key: &str| auth.get(key).map_or(Ok(String::new()), scalar);
                match auth.get("type").and_then(Doc::as_str) {
                    Some("basic") => Some(Auth::Basic {
                        username: get("username")?,
                        password: get("password")?,
                    }),
                    Some("bearer") => Some(Auth::Bearer(get("token")?)),
                    other => return Err(format!("recipe `{id}`: authentication {other:?}")),
                }
            }
        };
        out.push(Entry::Recipe(Recipe {
            id: id.clone(),
            name,
            method: node
                .get("method")
                .map_or(Ok("GET".into()), scalar)?
                .to_uppercase(),
            url: scalar(url)?,
            query: field("query")?,
            headers: field("headers")?,
            body,
            auth,
        }));
    }
    Ok(out)
}

/// JSON as rich's `JSON` lays it out: two spaces a level.
fn pretty_json(text: &str) -> Option<String> {
    rich::Json::new(text)
        .ok()
        .map(|json| json.text().plain().to_string())
}

impl Collection {
    /// The entry at a tree path.
    fn entry(&self, path: &[usize]) -> Option<&Entry> {
        let (first, rest) = path.split_first()?;
        let mut at = self.recipes.get(*first)?;
        for i in rest {
            at = match at {
                Entry::Folder { children, .. } => children.get(*i)?,
                Entry::Recipe(_) => return None,
            };
        }
        Some(at)
    }

    /// The path of the first recipe, depth first.
    fn first_recipe(&self) -> Vec<usize> {
        fn find(entries: &[Entry], path: &mut Vec<usize>) -> bool {
            for (i, entry) in entries.iter().enumerate() {
                path.push(i);
                match entry {
                    Entry::Recipe(_) => return true,
                    Entry::Folder { children, .. } if find(children, path) => return true,
                    Entry::Folder { .. } => {}
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        find(&self.recipes, &mut path);
        path
    }
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

/// A part of a template: text, or a `{{ … }}` expression.
enum Piece<'a> {
    Raw(&'a str),
    Expr(&'a str),
}

fn pieces(template: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open..].find("}}") else {
            break;
        };
        if open > 0 {
            out.push(Piece::Raw(&rest[..open]));
        }
        out.push(Piece::Expr(rest[open + 2..open + close].trim()));
        rest = &rest[open + close + 2..];
    }
    if !rest.is_empty() {
        out.push(Piece::Raw(rest));
    }
    out
}

/// One expression's value: a profile field, itself a template.
fn eval(expr: &str, fields: &[(String, String)], depth: usize) -> Result<String, String> {
    if depth > 16 {
        return Err(format!("`{expr}` refers to itself"));
    }
    if expr.is_empty()
        || !expr
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!("`{expr}`: only profile fields are templated here"));
    }
    let (_, value) = (fields.iter())
        .find(|(name, _)| name == expr)
        .ok_or_else(|| format!("Unknown field `{expr}`"))?;
    render_at(value, fields, depth + 1)
}

fn render_at(template: &str, fields: &[(String, String)], depth: usize) -> Result<String, String> {
    let mut out = String::new();
    for piece in pieces(template) {
        match piece {
            Piece::Raw(text) => out.push_str(text),
            Piece::Expr(expr) => out.push_str(&eval(expr, fields, depth)?),
        }
    }
    Ok(out)
}

/// A template with the profile's fields filled in.
pub fn render(template: &str, fields: &[(String, String)]) -> Result<String, String> {
    render_at(template, fields, 0)
}

/// A template as Slumber previews it: rendered, the parts that came from a
/// field underlined, an error reversed in red where its expression was.
fn preview(template: &str, fields: &[(String, String)]) -> String {
    pieces(template)
        .into_iter()
        .map(|piece| match piece {
            Piece::Raw(text) => escape(text),
            Piece::Expr(expr) => match eval(expr, fields, 0) {
                Ok(value) => format!("[underline yellow]{}[/]", escape(&value)),
                Err(_) => format!("[white on red]{}[/]", escape(&format!("{{{{{expr}}}}}"))),
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Building and sending a request
// ---------------------------------------------------------------------------

/// What the user changed for this session, as Slumber's overrides: never
/// written back to the collection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edits {
    pub url: Option<String>,
    pub query: HashMap<usize, String>,
    pub headers: HashMap<usize, String>,
    pub query_off: HashSet<usize>,
    pub headers_off: HashSet<usize>,
    pub body: Option<String>,
}

/// A request, built and ready to send.
#[derive(Clone, Debug, PartialEq)]
pub struct Built {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// A response as read off the socket.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        (self.headers.iter())
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn is_json(&self) -> bool {
        self.header("content-type")
            .is_some_and(|t| t.contains("json"))
    }
}

/// The rows of a table with the user's edits and disabled rows applied.
fn rows_of(
    rows: &[(String, String)],
    edits: &HashMap<usize, String>,
    off: &HashSet<usize>,
) -> Vec<(String, String)> {
    (rows.iter().enumerate())
        .filter(|(i, _)| !off.contains(i))
        .map(|(i, (k, v))| (k.clone(), edits.get(&i).unwrap_or(v).clone()))
        .collect()
}

fn encoded_pairs(rows: &[(String, String)], fields: &[(String, String)]) -> Result<String, String> {
    let pairs: Result<Vec<String>, String> = (rows.iter())
        .map(|(k, v)| Ok(format!("{}={}", encode(k), encode(&render(v, fields)?))))
        .collect();
    Ok(pairs?.join("&"))
}

/// A recipe made into a request with the profile's fields and the user's
/// edits.
pub fn build(recipe: &Recipe, fields: &[(String, String)], edits: &Edits) -> Result<Built, String> {
    let mut url = render(edits.url.as_deref().unwrap_or(&recipe.url), fields)?;
    let query = rows_of(&recipe.query, &edits.query, &edits.query_off);
    if !query.is_empty() {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&encoded_pairs(&query, fields)?);
    }
    let mut headers: Vec<(String, String)> = Vec::new();
    for (k, v) in rows_of(&recipe.headers, &edits.headers, &edits.headers_off) {
        headers.push((k, render(&v, fields)?));
    }
    match &recipe.auth {
        Some(Auth::Basic { username, password }) => {
            let pair = format!(
                "{}:{}",
                render(username, fields)?,
                render(password, fields)?
            );
            headers.push((
                "Authorization".into(),
                format!("Basic {}", base64(pair.as_bytes())),
            ));
        }
        Some(Auth::Bearer(token)) => {
            headers.push((
                "Authorization".into(),
                format!("Bearer {}", render(token, fields)?),
            ));
        }
        None => {}
    }
    // Slumber's default header, unless the recipe sets its own.
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
    {
        headers.push(("User-Agent".into(), "slumber-intuituive".into()));
    }
    let mut content_type = |value: &str| {
        if !headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            headers.push(("Content-Type".into(), value.into()));
        }
    };
    let body = match (&edits.body, &recipe.body) {
        (_, Body::None) => None,
        (Some(text), Body::Json(_)) | (None, Body::Json(text)) => {
            content_type("application/json");
            Some(render(text, fields)?)
        }
        (Some(text), _) | (None, Body::Raw(text)) => Some(render(text, fields)?),
        (None, Body::Form(form)) => {
            content_type("application/x-www-form-urlencoded");
            Some(encoded_pairs(form, fields)?)
        }
    };
    Ok(Built {
        method: recipe.method.clone(),
        url,
        headers,
        body,
    })
}

/// Percent-encoding for a query or form value.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n =
            (chunk.iter().enumerate()).fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The socket of a request in flight, so a cancel can shut it.
pub type Conn = Arc<Mutex<Option<TcpStream>>>;

/// Send `request` over HTTP/1.1 and read the whole response.
pub fn send(request: &Built, conn: &Conn) -> Result<Response, String> {
    let rest = match request.url.split_once("://") {
        Some(("http", rest)) => rest,
        Some(("https", _)) => {
            return Err("https needs TLS, which this clone's std::net client does not speak".into())
        }
        Some((scheme, _)) => return Err(format!("unsupported scheme `{scheme}`")),
        None => return Err(format!("`{}` is not a URL: it has no scheme", request.url)),
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_string()
    };
    let address = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    let addr = (address.to_socket_addrs())
        .map_err(|e| format!("{authority}: {e}"))?
        .next()
        .ok_or_else(|| format!("{authority}: no address"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|e| format!("connecting to {authority}: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
    *conn.lock().unwrap() = stream.try_clone().ok();
    let mut head = format!(
        "{} {path} HTTP/1.1\r\nHost: {authority}\r\n",
        request.method
    );
    for (k, v) in &request.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    let body = request.body.as_deref().unwrap_or("");
    if request.body.is_some() {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("Connection: close\r\n\r\n");
    (stream.write_all(head.as_bytes()))
        .and_then(|()| stream.write_all(body.as_bytes()))
        .map_err(|e| format!("sending: {e}"))?;
    let mut raw = Vec::new();
    (stream.read_to_end(&mut raw)).map_err(|e| format!("reading: {e}"))?;
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Result<Response, String> {
    let end = (raw.windows(4).position(|w| w == b"\r\n\r\n"))
        .ok_or("the connection closed before the response's headers ended")?;
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let status = (parts.nth(1).and_then(|s| s.parse().ok()))
        .ok_or_else(|| format!("not an HTTP status line: {status_line:?}"))?;
    let reason = parts.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let mut response = Response {
        status,
        reason,
        headers,
        body: raw[end + 4..].to_vec(),
    };
    let chunked = (response.header("transfer-encoding")).is_some_and(|t| t.contains("chunked"));
    if chunked {
        response.body = dechunk(&response.body);
    } else if let Some(n) = response
        .header("content-length")
        .and_then(|n| n.parse().ok())
    {
        response.body.truncate(n);
    }
    Ok(response)
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(eol) = raw.windows(2).position(|w| w == b"\r\n") {
        let size = String::from_utf8_lossy(&raw[..eol]);
        let size = size.split(';').next().unwrap_or("").trim();
        let Ok(n) = usize::from_str_radix(size, 16) else {
            break;
        };
        let start = eol + 2;
        if n == 0 || start + n > raw.len() {
            break;
        }
        out.extend_from_slice(&raw[start..start + n]);
        raw = raw.get(start + n + 2..).unwrap_or(&[]);
    }
    out
}

/// The request as a `curl` command (the actions menu's "Copy as cURL").
pub fn curl(request: &Built) -> String {
    let quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let mut out = format!("curl -X{} --url {}", request.method, quote(&request.url));
    for (k, v) in &request.headers {
        out.push_str(&format!(" --header {}", quote(&format!("{k}: {v}"))));
    }
    if let Some(body) = &request.body {
        out.push_str(&format!(" --data {}", quote(body)));
    }
    out
}

// ---------------------------------------------------------------------------
// Request history
// ---------------------------------------------------------------------------

/// One request and what became of it: an entry in the history.
#[derive(Clone, Debug, PartialEq)]
pub struct Exchange {
    pub id: u64,
    pub recipe: String,
    pub profile: String,
    /// When it was sent, as `HH:MM:SS`.
    pub started: String,
    pub since: Instant,
    pub request: Option<Built>,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Loading,
    Done(Response, Duration),
    Failed(String, Duration),
    BuildError(String),
    Cancelled,
}

impl Exchange {
    /// A history row: when, how long, and what came back.
    fn summary(&self) -> String {
        let took = match &self.outcome {
            Outcome::Done(_, d) | Outcome::Failed(_, d) => millis(*d),
            _ => "-".into(),
        };
        let what = match &self.outcome {
            Outcome::Loading => "Loading...".into(),
            Outcome::Done(r, _) => status_markup(r),
            Outcome::Failed(..) => "[red]Request error[/]".into(),
            Outcome::BuildError(_) => "[red]Build error[/]".into(),
            Outcome::Cancelled => "Cancelled".into(),
        };
        format!("{} / {took}  {what}", self.started)
    }

    fn loading(&self) -> bool {
        self.outcome == Outcome::Loading
    }
}

fn millis(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.2}s", d.as_secs_f64())
    }
}

fn status_markup(r: &Response) -> String {
    let style = if r.status < 400 {
        "white on green"
    } else {
        "white on red"
    };
    format!("[{style}] {} {} [/]", r.status, escape(&r.reason))
}

fn size(n: usize) -> String {
    match n {
        n if n < 1000 => format!("{n} B"),
        n if n < 1_000_000 => format!("{:.1} KB", n as f64 / 1000.0),
        n => format!("{:.1} MB", n as f64 / 1_000_000.0),
    }
}

/// A request in flight.
#[derive(Clone)]
struct Running {
    id: u64,
    task: Task,
    conn: Conn,
}

// ---------------------------------------------------------------------------
// The app's state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Sidebar {
    Recipe,
    Profile,
    History,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Pane {
    Sidebar,
    Top,
    Bottom,
}

const RECIPE_TABS: [&str; 5] = ["URL", "Query", "Headers", "Body", "Authentication"];
const EXCHANGE_TABS: [&str; 3] = ["Request", "Body", "Headers"];

/// What the selected recipe entry is: the top pane shows a page for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Recipe,
    Folder,
    None,
}

/// Which panes show: the sidebar's list while it is open, and the one pane
/// in fullscreen. The view is built once for each layout (a `switch`), as
/// intuiTUIve has no node shown or hidden by a signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Layout {
    sidebar: Option<Sidebar>,
    full: Option<Pane>,
}

/// A node a pane focuses, in one layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Target {
    Sidebar,
    Recipe(Kind, usize),
    Exchange(bool, usize),
    Profile,
}

/// Where each focus target is: (layout, target) to node id, filled in as
/// the switches build their pages.
type Registry = HashMap<(Layout, Target), u64>;

/// Every piece of state, as signals: `Copy`, so each handler takes it.
#[derive(Clone, Copy)]
struct Slumber {
    path: Signal<PathBuf>,
    collection: Signal<Collection>,
    tree: Signal<Vec<usize>>,
    expanded: Signal<HashSet<Vec<usize>>>,
    profile: Signal<usize>,
    sidebar: Signal<Sidebar>,
    sidebar_open: Signal<bool>,
    fullscreen: Signal<bool>,
    pane: Signal<Pane>,
    ratio: Signal<f64>,
    recipe_tab: Signal<usize>,
    exchange_tab: Signal<usize>,
    query_row: Signal<usize>,
    header_row: Signal<usize>,
    edits: Signal<HashMap<String, Edits>>,
    history: Signal<Vec<Exchange>>,
    history_row: Signal<usize>,
    pinned: Signal<Option<u64>>,
    next_id: Signal<u64>,
    running: Signal<Vec<Running>>,
    tick: Signal<u64>,
    filter: Signal<String>,
    filtered: Signal<Option<(u64, Result<String, String>)>>,
    notice: Signal<Option<String>>,
    focus: Signal<Registry>,
    /// A target to focus once its page is built.
    want: Signal<Option<(Layout, Target)>>,
}

impl Slumber {
    fn new(path: PathBuf, collection: Collection) -> Slumber {
        let profile = (collection.profiles.iter())
            .position(|p| p.default)
            .unwrap_or(0);
        let first = collection.first_recipe();
        // The folders down to the first recipe start open.
        let open: HashSet<Vec<usize>> = (1..first.len()).map(|n| first[..n].to_vec()).collect();
        Slumber {
            path: signal(path),
            tree: signal(first),
            expanded: signal(open),
            collection: signal(collection),
            profile: signal(profile),
            sidebar: signal(Sidebar::Recipe),
            sidebar_open: signal(true),
            fullscreen: signal(false),
            pane: signal(Pane::Top),
            ratio: signal(0.5),
            recipe_tab: signal(0),
            exchange_tab: signal(1),
            query_row: signal(0),
            header_row: signal(0),
            edits: signal(HashMap::new()),
            history: signal(Vec::new()),
            history_row: signal(0),
            pinned: signal(None),
            next_id: signal(1),
            running: signal(Vec::new()),
            tick: signal(0),
            filter: signal(String::new()),
            filtered: signal(None),
            notice: signal(None),
            focus: signal(Registry::new()),
            want: signal(None),
        }
    }

    fn entry(&self) -> Option<Entry> {
        let path = self.tree.get();
        self.collection.with(|c| c.entry(&path).cloned())
    }

    fn recipe(&self) -> Option<Recipe> {
        match self.entry()? {
            Entry::Recipe(recipe) => Some(recipe),
            Entry::Folder { .. } => None,
        }
    }

    fn profile(&self) -> Option<Profile> {
        let at = self.profile.get();
        self.collection.with(|c| c.profiles.get(at).cloned())
    }

    fn fields(&self) -> Vec<(String, String)> {
        self.profile().map(|p| p.data).unwrap_or_default()
    }

    fn profile_id(&self) -> String {
        self.profile().map(|p| p.id).unwrap_or_default()
    }

    fn edits_for(&self, recipe: &str) -> Edits {
        self.edits
            .with(|e| e.get(recipe).cloned().unwrap_or_default())
    }

    fn edit(&self, recipe: &str, change: impl FnOnce(&mut Edits)) {
        self.edits
            .update(|e| change(e.entry(recipe.to_string()).or_default()));
    }

    /// The history of the selected recipe under the selected profile, the
    /// newest first.
    fn recipe_history(&self) -> Vec<Exchange> {
        let (Some(recipe), profile) = (self.recipe(), self.profile_id()) else {
            return Vec::new();
        };
        self.history.with(|h| {
            (h.iter().rev())
                .filter(|x| x.recipe == recipe.id && x.profile == profile)
                .cloned()
                .collect()
        })
    }

    /// The exchange the bottom pane shows: the one picked in the history,
    /// else the latest.
    fn shown(&self) -> Option<Exchange> {
        let history = self.recipe_history();
        let pinned = self.pinned.get();
        let picked = pinned.and_then(|id| history.iter().find(|x| x.id == id));
        picked.or(history.first()).cloned()
    }

    fn notify(&self, cx: &mut Ctx, message: impl Into<String>) {
        let message = message.into();
        self.notice.set(Some(message.clone()));
        let notice = self.notice;
        cx.after(Duration::from_secs(5), move |_| {
            if notice.get().as_deref() == Some(message.as_str()) {
                notice.set(None);
            }
        });
    }

    /// The panes in Tab order: the sidebar while it is open, then the top
    /// and bottom panes.
    fn panes(&self) -> Vec<Pane> {
        let mut panes = Vec::new();
        if self.sidebar_open.get() {
            panes.push(Pane::Sidebar);
        }
        panes.extend([Pane::Top, Pane::Bottom]);
        panes
    }

    fn kind(&self) -> Kind {
        match self.entry() {
            Some(Entry::Recipe(_)) => Kind::Recipe,
            Some(Entry::Folder { .. }) => Kind::Folder,
            None => Kind::None,
        }
    }

    fn layout(&self) -> Layout {
        Layout {
            sidebar: self.sidebar_open.get().then(|| self.sidebar.get()),
            full: self.fullscreen.get().then(|| self.pane.get()),
        }
    }

    /// The node the selected pane focuses.
    fn target(&self) -> Target {
        match self.pane.get() {
            Pane::Sidebar => Target::Sidebar,
            Pane::Top => Target::Recipe(self.kind(), self.recipe_tab.get()),
            Pane::Bottom if self.sidebar.get() == Sidebar::Profile => Target::Profile,
            Pane::Bottom => Target::Exchange(self.shown().is_some(), self.exchange_tab.get()),
        }
    }

    /// Note that `node` is `target` in `layout`, and return it.
    fn register(&self, layout: Layout, target: Target, node: Node) -> Node {
        let id = node.id();
        self.focus.update(|r| {
            r.insert((layout, target), id);
        });
        node
    }

    fn select_pane(&self, pane: Pane, cx: &mut Ctx) {
        self.pane.set(pane);
        self.refocus(cx);
    }

    /// Focus the selected pane's node. A page a `switch` has not built yet
    /// is focused once it is (see the watch in [`http_app`]).
    fn refocus(&self, cx: &mut Ctx) {
        let slot = (self.layout(), self.target());
        match self.focus.with(|r| r.get(&slot).copied()) {
            Some(id) => {
                self.want.set(None);
                cx.focus(id);
            }
            None => self.want.set(Some(slot)),
        }
    }

    fn cycle_pane(&self, by: isize, cx: &mut Ctx) {
        let panes = self.panes();
        let at = (panes.iter())
            .position(|p| *p == self.pane.get())
            .unwrap_or(0) as isize;
        let next = panes[(at + by).rem_euclid(panes.len() as isize) as usize];
        self.select_pane(next, cx);
    }

    fn open_sidebar(&self, which: Sidebar, cx: &mut Ctx) {
        self.sidebar.set(which);
        self.sidebar_open.set(true);
        self.fullscreen.set(false);
        if which == Sidebar::History {
            let at = self
                .pinned
                .get()
                .and_then(|id| self.recipe_history().iter().position(|x| x.id == id));
            self.history_row.set(at.unwrap_or(0));
        }
        self.select_pane(Pane::Sidebar, cx);
    }

    // -- Requests ----------------------------------------------------------

    fn send_request(&self) {
        let Some(recipe) = self.recipe() else {
            return;
        };
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let built = build(&recipe, &self.fields(), &self.edits_for(&recipe.id));
        let mut exchange = Exchange {
            id,
            recipe: recipe.id.clone(),
            profile: self.profile_id(),
            started: rich_ext::fs::local_time(SystemTime::now(), "%H:%M:%S"),
            since: Instant::now(),
            request: built.as_ref().ok().cloned(),
            outcome: Outcome::Loading,
        };
        self.pinned.set(None);
        self.filtered.set(None);
        let request = match built {
            Ok(request) => request,
            Err(error) => {
                exchange.outcome = Outcome::BuildError(error);
                self.history.update(|h| h.push(exchange));
                return;
            }
        };
        self.history.update(|h| h.push(exchange));
        let conn: Conn = Arc::default();
        let socket = conn.clone();
        let state = *self;
        let task = spawn(
            move || {
                let start = Instant::now();
                (send(&request, &socket), start.elapsed())
            },
            move |(result, took), _| {
                state.running.update(|r| r.retain(|r| r.id != id));
                let outcome = match result {
                    Ok(response) => Outcome::Done(response, took),
                    Err(error) => Outcome::Failed(error, took),
                };
                state.set_outcome(id, outcome);
            },
        );
        self.running.update(|r| r.push(Running { id, task, conn }));
    }

    fn set_outcome(&self, id: u64, outcome: Outcome) {
        self.history.update(|h| {
            if let Some(x) = h.iter_mut().find(|x| x.id == id) {
                x.outcome = outcome;
            }
        });
    }

    /// Cancel a request in flight: its result is dropped and its socket
    /// shut, so its thread ends too.
    fn cancel(&self, id: u64) {
        let mut found = None;
        self.running.update(|r| {
            if let Some(at) = r.iter().position(|r| r.id == id) {
                found = Some(r.remove(at));
            }
        });
        if let Some(running) = found {
            running.task.cancel();
            if let Some(socket) = running.conn.lock().unwrap().take() {
                let _ = socket.shutdown(Shutdown::Both);
            }
            self.set_outcome(id, Outcome::Cancelled);
        }
    }

    /// The response body as text: pretty JSON when it is JSON.
    fn body_text(&self, x: &Exchange) -> Option<String> {
        let Outcome::Done(response, _) = &x.outcome else {
            return None;
        };
        let text = String::from_utf8_lossy(&response.body).into_owned();
        Some(if response.is_json() {
            pretty_json(&text).unwrap_or(text)
        } else {
            text
        })
    }

    /// What the body pane shows: the filter's output once one ran.
    fn visible_body(&self, x: &Exchange) -> Option<String> {
        match self.filtered.get() {
            Some((id, Ok(out))) if id == x.id => Some(out),
            _ => self.body_text(x),
        }
    }

    /// Run the filter box's query on the shown body (see [`filter`]).
    fn run_filter(&self, query: String) {
        let Some(x) = self.shown() else {
            return;
        };
        let id = x.id;
        if query.trim().is_empty() {
            self.filtered.set(None);
            return;
        }
        let Some(body) = self.body_text(&x) else {
            return;
        };
        let filtered = self.filtered;
        spawn(
            move || filter(&body, &query),
            move |result, _| filtered.set(Some((id, result))),
        );
    }
}

/// A body run through a query: JSONPath when it starts with `$`
/// (rs-rich-ext's `data::select`, Slumber's older query language), else a
/// shell command fed the body, as Slumber's query box does now.
pub fn filter(body: &str, query: &str) -> Result<String, String> {
    if query.trim_start().starts_with('$') {
        let doc = data::parse_json(body).map_err(|e| e.to_string())?;
        let selector = (Selectors::default())
            .compile("jsonpath", query.trim())
            .map_err(|e| e.to_string())?;
        let found = selector.select(&doc).map_err(|e| e.to_string())?;
        let values: Vec<String> = found.iter().map(|(_, n)| n.to_json().to_string()).collect();
        let json = if values.len() == 1 {
            values[0].clone()
        } else {
            format!("[{}]", values.join(","))
        };
        return Ok(pretty_json(&json).unwrap_or(json));
    }
    pipe(query, body.as_bytes())
}

/// Run `command` in a shell with `input` on its stdin.
fn pipe(command: &str, input: &[u8]) -> Result<String, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{command}: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped");
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// ---------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------

/// Slumber's pane: its border blue while the focus is inside. (Slumber's
/// is double then too; a panel's kind of border cannot follow the focus.)
/// Titles are markup, so a key's brackets are escaped.
fn pane(node: Node, title: &str) -> Node {
    node.panel(&escape(title))
}

/// Console markup as a `Text`.
fn markup(text: &str) -> rich::Text {
    rich::Text::from_markup(text).unwrap_or_else(|_| rich::Text::new(text))
}

fn error_text(error: &str) -> rich::Text {
    markup(&format!("[red]{}[/]", escape(error)))
}

fn tree_items(entries: &[Entry]) -> Vec<TreeItem> {
    (entries.iter())
        .map(|entry| match entry {
            Entry::Folder { name, children, .. } => {
                TreeItem::new(escape(name)).children(tree_items(children))
            }
            Entry::Recipe(recipe) => TreeItem::new(escape(&recipe.name)),
        })
        .collect()
}

/// The sidebar: the recipe tree, the profiles or the history.
fn sidebar(s: Slumber, layout: Layout, which: Sidebar) -> Node {
    let (node, title) = match which {
        Sidebar::Recipe => {
            let tree = tree_with(
                move || s.collection.with(|c| tree_items(&c.recipes)),
                s.tree,
                s.expanded,
            );
            (tree, "Recipe [r]")
        }
        Sidebar::Profile => {
            let names = move || {
                (s.collection).with(|c| c.profiles.iter().map(|p| escape(&p.name)).collect())
            };
            (list(names, s.profile), "Profile [p]")
        }
        Sidebar::History => {
            let rows = move || s.recipe_history().iter().map(Exchange::summary).collect();
            (list(rows, s.history_row), "Request History [ctrl h]")
        }
    };
    let node = s.register(layout, Target::Sidebar, node);
    if which != Sidebar::History {
        return pane(node, title);
    }
    let none = text(move || match s.recipe_history().is_empty() {
        true => "No request history".into(),
        false => String::new(),
    });
    pane(column([none.auto(), node.flex(1)]), title)
}

/// The boxes above the panes: the profile, and the recipe while the
/// sidebar does not list the recipes, as Slumber's headers.
fn headers(s: Slumber, layout: Layout) -> Node {
    let profile =
        text(move || (s.profile()).map_or("[dim]No profiles[/]".into(), |p| escape(&p.name)))
            .no_wrap();
    let recipe = text(move || match s.entry() {
        Some(Entry::Recipe(r)) => escape(&r.name),
        Some(Entry::Folder { name, .. }) => escape(&name),
        None => "[dim]None[/]".into(),
    })
    .no_wrap();
    let mut boxes = Vec::new();
    if layout.sidebar != Some(Sidebar::Profile) {
        boxes.push(pane(profile, "Profile [p]"));
    }
    if layout.sidebar != Some(Sidebar::Recipe) {
        boxes.push(pane(recipe, "Recipe [r]"));
    }
    row(boxes)
}

type RowsOf = fn(&Recipe) -> &Vec<(String, String)>;
type EditsOf = fn(&Edits) -> (&HashMap<usize, String>, &HashSet<usize>);

/// A key-value table with a checkbox column, as Slumber's query and header
/// tables: space toggles a row, edited values in italics.
fn key_values(
    s: Slumber,
    noun: &'static str,
    (rows, edits): (RowsOf, EditsOf),
    selected: Signal<usize>,
    rects: &RowRects,
) -> Node {
    let columns = vec![
        Column::new("", Size::Fixed(1)),
        Column::new(noun, Size::Auto),
        Column::new("Value", Size::Flex(1)),
    ];
    table_with(
        columns,
        move || {
            let Some(recipe) = s.recipe() else {
                return Vec::new();
            };
            let fields = s.fields();
            let all = s.edits_for(&recipe.id);
            let (edited, off) = edits(&all);
            (rows(&recipe).iter().enumerate())
                .map(|(i, (key, value))| {
                    let check = if off.contains(&i) { "□" } else { "▣" };
                    let value = match edited.get(&i) {
                        Some(v) => format!("[italic]{}[/]", preview(v, &fields)),
                        None => preview(value, &fields),
                    };
                    vec![check.into(), escape(key), value]
                })
                .collect()
        },
        selected,
        TableOptions::default(),
    )
    .row_rects(rects)
}

/// One tab of the recipe pane.
fn recipe_page(s: Slumber, tab: usize, rects: &(RowRects, RowRects)) -> Node {
    match tab {
        0 => text(move || {
            let Some(recipe) = s.recipe() else {
                return String::new();
            };
            let fields = s.fields();
            let url = match &s.edits_for(&recipe.id).url {
                Some(url) => format!("[italic]{}[/]", preview(url, &fields)),
                None => preview(&recipe.url, &fields),
            };
            format!("[bold]{}[/] {url}", recipe.method)
        })
        .focusable(),
        1 => key_values(
            s,
            "Parameter",
            (|r| &r.query, |e| (&e.query, &e.query_off)),
            s.query_row,
            &rects.0,
        ),
        2 => key_values(
            s,
            "Header",
            (|r| &r.headers, |e| (&e.headers, &e.headers_off)),
            s.header_row,
            &rects.1,
        ),
        3 => scroll(renderable(move || {
            let Some(recipe) = s.recipe() else {
                return rich::Text::new("");
            };
            let fields = s.fields();
            let template = match (&s.edits_for(&recipe.id).body, &recipe.body) {
                (_, Body::None) => return markup("[dim]No body[/]"),
                (Some(text), _) | (None, Body::Raw(text)) | (None, Body::Json(text)) => {
                    text.clone()
                }
                (None, Body::Form(form)) => (form.iter())
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            // JSON with its templates filled is highlighted as JSON;
            // anything else shows its template preview.
            if matches!(recipe.body, Body::Json(_)) {
                let rendered = render(&template, &fields).ok();
                if let Some(json) = rendered.and_then(|t| rich::Json::new(&t).ok()) {
                    return json.text();
                }
            }
            markup(&preview(&template, &fields))
        })),
        _ => text(move || match s.recipe().and_then(|r| r.auth) {
            None => "[dim]No authentication[/]".into(),
            Some(Auth::Basic { username, password }) => {
                let fields = s.fields();
                format!(
                    "[bold]Type[/]      Basic\n[bold]Username[/]  {}\n[bold]Password[/]  {}",
                    preview(&username, &fields),
                    preview(&password, &fields)
                )
            }
            Some(Auth::Bearer(token)) => format!(
                "[bold]Type[/]   Bearer\n[bold]Token[/]  {}",
                preview(&token, &s.fields())
            ),
        })
        .focusable(),
    }
}

/// The top pane: the selected recipe's URL, query, headers, body and
/// authentication, in tabs (Slumber's recipe pane), or a folder's
/// recipes.
fn recipe_pane(s: Slumber, layout: Layout, rects: &(RowRects, RowRects)) -> Node {
    let edit_rects = rects.clone();
    let rects = rects.clone();
    let content = switch(
        move || s.kind(),
        move |kind| match kind {
            Kind::Recipe => {
                let rects = rects.clone();
                let pages = switch(
                    move || s.recipe_tab.get(),
                    move |tab| {
                        let page = recipe_page(s, tab, &rects);
                        s.register(layout, Target::Recipe(Kind::Recipe, tab), page)
                    },
                );
                let id = text(move || match s.recipe() {
                    Some(r) => format!("[bold]{}[/]", escape(&r.id)),
                    None => String::new(),
                })
                .no_wrap();
                let titles = || RECIPE_TABS.iter().map(|t| t.to_string()).collect();
                let strip = row([tabs(titles, s.recipe_tab).no_focus().flex(1), id.auto()]);
                column([strip.fixed(1), pages.flex(1)])
            }
            Kind::Folder => {
                let folder = text(move || match s.entry() {
                    Some(Entry::Folder { name, children, .. }) => {
                        let mut lines = vec![format!("[bold]{}[/]", escape(&name))];
                        lines.extend(tree_lines(&children, 1));
                        lines.join("\n")
                    }
                    _ => String::new(),
                })
                .focusable();
                register_all(s, layout, Kind::Folder, folder)
            }
            Kind::None => {
                let empty = label("No recipes defined; add one to your collection").focusable();
                register_all(s, layout, Kind::None, empty)
            }
        },
    );
    pane(content, "Recipe [1]").section("Recipe", |node| {
        node.bind("left h", "Previous tab", move |cx| switch_tab(s, -1, cx))
            .bind("right l", "Next tab", move |cx| switch_tab(s, 1, cx))
            .bind("space", "Toggle row", move |_| toggle_row(s))
            .bind("e", "Edit", move |cx| edit(s, cx, &edit_rects))
            .bind("z", "Reset edit", move |_| reset(s))
    })
}

/// A page that every tab of the recipe pane focuses.
fn register_all(s: Slumber, layout: Layout, kind: Kind, node: Node) -> Node {
    let id = node.id();
    s.focus.update(|r| {
        for tab in 0..RECIPE_TABS.len() {
            r.insert((layout, Target::Recipe(kind, tab)), id);
        }
    });
    node
}

fn tree_lines(entries: &[Entry], depth: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in entries {
        let pad = "  ".repeat(depth);
        match entry {
            Entry::Folder { name, children, .. } => {
                lines.push(format!("{pad}{}", escape(name)));
                lines.extend(tree_lines(children, depth + 1));
            }
            Entry::Recipe(r) => {
                lines.push(format!("{pad}[dim]{}[/] {}", r.method, escape(&r.name)))
            }
        }
    }
    lines
}

fn switch_tab(s: Slumber, by: isize, cx: &mut Ctx) {
    let (tab, len) = match s.pane.get() {
        Pane::Bottom => (s.exchange_tab, EXCHANGE_TABS.len()),
        _ => (s.recipe_tab, RECIPE_TABS.len()),
    };
    tab.set((tab.get() as isize + by).rem_euclid(len as isize) as usize);
    s.refocus(cx);
}

fn toggle_row(s: Slumber) {
    let Some(recipe) = s.recipe() else {
        return;
    };
    let flip = |set: &mut HashSet<usize>, i: usize| {
        if !set.remove(&i) {
            set.insert(i);
        }
    };
    let (q, h) = (s.query_row.get(), s.header_row.get());
    match s.recipe_tab.get() {
        1 if q < recipe.query.len() => s.edit(&recipe.id, |e| flip(&mut e.query_off, q)),
        2 if h < recipe.headers.len() => s.edit(&recipe.id, |e| flip(&mut e.headers_off, h)),
        _ => {}
    }
}

/// `e`: the URL or a row's value in a box over it, or the body in
/// `$EDITOR`, as Slumber edits them. Edits last the session.
fn edit(s: Slumber, cx: &mut Ctx, rects: &(RowRects, RowRects)) {
    let Some(recipe) = s.recipe() else {
        return;
    };
    let edits = s.edits_for(&recipe.id);
    let id = recipe.id.clone();
    let tab = s.recipe_tab.get();
    match tab {
        0 => {
            let current = edits.url.unwrap_or(recipe.url);
            cx.prompt("URL", current, move |text, _| {
                s.edit(&id, |e| e.url = Some(text));
            });
        }
        1 | 2 => {
            let (row, rows, rect, edited) = if tab == 1 {
                let row = s.query_row.get();
                (row, &recipe.query, rects.0.get(row), edits.query.get(&row))
            } else {
                let row = s.header_row.get();
                (
                    row,
                    &recipe.headers,
                    rects.1.get(row),
                    edits.headers.get(&row),
                )
            };
            let Some((key, value)) = rows.get(row) else {
                return;
            };
            let current = edited.unwrap_or(value).clone();
            // Over the row, as Slumber edits a cell in place.
            let options = match rect {
                Some(rect) => PromptOptions::new().at(Origin::At(rect), Offset::new(0, -1, 60, 3)),
                None => PromptOptions::new(),
            };
            cx.prompt_with(key.clone(), current, options, move |text, _| {
                s.edit(&id, |e| {
                    let map = if tab == 1 {
                        &mut e.query
                    } else {
                        &mut e.headers
                    };
                    map.insert(row, text);
                });
            });
        }
        3 => {
            let current = match (&edits.body, &recipe.body) {
                (Some(text), _) | (None, Body::Raw(text)) | (None, Body::Json(text)) => {
                    text.clone()
                }
                _ => return,
            };
            let name = format!("slumber-{}-{id}.txt", std::process::id());
            let file = std::env::temp_dir().join(name);
            if let Err(error) = std::fs::write(&file, &current) {
                s.notify(cx, format!("Writing {}: {error}", file.display()));
                return;
            }
            let editor = (std::env::var("VISUAL"))
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| "vim".into());
            let mut words = editor.split_whitespace();
            let mut command = Command::new(words.next().unwrap_or("vim"));
            command.args(words).arg(&file);
            cx.handoff_then(command, move |_, cx| match std::fs::read_to_string(&file) {
                Ok(text) if text != current => s.edit(&id, |e| e.body = Some(text)),
                Ok(_) => {}
                Err(error) => s.notify(cx, format!("Reading {}: {error}", file.display())),
            });
        }
        _ => {}
    }
}

fn reset(s: Slumber) {
    let Some(recipe) = s.recipe() else {
        return;
    };
    let tab = s.recipe_tab.get();
    let (q, h) = (s.query_row.get(), s.header_row.get());
    s.edit(&recipe.id, |e| match tab {
        0 => e.url = None,
        1 => {
            e.query.remove(&q);
            e.query_off.remove(&q);
        }
        2 => {
            e.headers.remove(&h);
            e.headers_off.remove(&h);
        }
        3 => e.body = None,
        _ => {}
    });
}

/// The response body, the filter's output once one ran, highlighted when
/// it is JSON.
fn body_view(s: Slumber) -> Node {
    scroll(renderable(move || {
        let Some(x) = s.shown() else {
            return rich::Text::new("");
        };
        match &x.outcome {
            Outcome::Loading => rich::Text::new("Loading..."),
            Outcome::Cancelled => rich::Text::new("Request cancelled"),
            Outcome::Failed(error, _) | Outcome::BuildError(error) => error_text(error),
            Outcome::Done(..) => match s.filtered.get() {
                Some((id, Err(error))) if id == x.id => error_text(&error),
                _ => {
                    let text = s.visible_body(&x).unwrap_or_default();
                    match rich::Json::new(&text) {
                        Ok(json) if text.trim_start().starts_with(['{', '[']) => json.text(),
                        _ => rich::Text::new(&text),
                    }
                }
            },
        }
    }))
}

/// The request as it was sent.
fn request_view(s: Slumber) -> Node {
    scroll(text(move || {
        let Some(x) = s.shown() else {
            return String::new();
        };
        let Some(built) = &x.request else {
            return match &x.outcome {
                Outcome::BuildError(error) => format!("[red]{}[/]", escape(error)),
                _ => String::new(),
            };
        };
        let mut lines = vec![format!("[bold]{}[/] {}", built.method, escape(&built.url))];
        for (k, v) in &built.headers {
            lines.push(format!("[bold]{}[/]: {}", escape(k), escape(v)));
        }
        if let Some(body) = &built.body {
            lines.push(String::new());
            lines.push(escape(&pretty_json(body).unwrap_or_else(|| body.clone())));
        }
        lines.join("\n")
    }))
}

/// One tab of the exchange pane, its focus target registered. The body's
/// tab has the filter box under it; `/` there focuses the box.
fn exchange_page(s: Slumber, layout: Layout, tab: usize) -> Node {
    let target = Target::Exchange(true, tab);
    match tab {
        0 => s.register(layout, target, request_view(s)),
        1 => {
            let body = s.register(layout, target, body_view(s));
            let body_id = body.id();
            let filter_box = input(s.filter)
                .placeholder("'/' to filter body with JSONPath or a shell command")
                .on_submit(move |query, cx| {
                    s.run_filter(query.to_string());
                    cx.focus(body_id);
                })
                .on_key("esc", move |cx| cx.focus(body_id));
            let filter_id = filter_box.id();
            column([body.flex(1), filter_box.fixed(1)]).section("Response", |node| {
                node.bind("slash", "Filter body", move |cx| cx.focus(filter_id))
            })
        }
        _ => {
            let headers = table_with(
                vec![
                    Column::new("Header", Size::Auto),
                    Column::new("Value", Size::Flex(1)),
                ],
                move || match s.shown().map(|x| x.outcome) {
                    Some(Outcome::Done(r, _)) => (r.headers.iter())
                        .map(|(k, v)| vec![escape(k), escape(v)])
                        .collect(),
                    _ => Vec::new(),
                },
                signal(0usize),
                TableOptions::default(),
            );
            s.register(layout, target, headers)
        }
    }
}

/// The status line over the exchange's tabs: Slumber's metadata row.
fn meta(s: Slumber) -> Node {
    text(move || {
        let Some(x) = s.shown() else {
            return String::new();
        };
        match &x.outcome {
            Outcome::Loading => {
                s.tick.get();
                format!("Loading... {}", millis(x.since.elapsed()))
            }
            Outcome::Done(r, took) => {
                format!(
                    "{}  {}  {}",
                    status_markup(r),
                    millis(*took),
                    size(r.body.len())
                )
            }
            Outcome::Failed(..) => "[red]Request error[/]".into(),
            Outcome::BuildError(_) => "[red]Build error[/]".into(),
            Outcome::Cancelled => "Request cancelled".into(),
        }
    })
    .no_wrap()
}

/// The bottom pane: the shown exchange's request, response body (with the
/// filter box) and headers (Slumber's exchange pane).
fn exchange_pane(s: Slumber, layout: Layout) -> Node {
    let content = switch(
        move || s.shown().is_some(),
        move |shown| {
            if !shown {
                let empty = text(move || match s.entry() {
                    Some(Entry::Folder { .. }) => {
                        "Select a recipe to see its request history".into()
                    }
                    Some(Entry::Recipe(_)) => "No request history for this recipe & profile".into(),
                    None => String::new(),
                })
                .focusable();
                let id = empty.id();
                s.focus.update(|r| {
                    for tab in 0..EXCHANGE_TABS.len() {
                        r.insert((layout, Target::Exchange(false, tab)), id);
                    }
                });
                return empty;
            }
            let pages = switch(
                move || s.exchange_tab.get(),
                move |tab| exchange_page(s, layout, tab),
            );
            let titles = || EXCHANGE_TABS.iter().map(|t| t.to_string()).collect();
            let strip = tabs(titles, s.exchange_tab).no_focus();
            column([meta(s).fixed(1), strip.fixed(1), pages.flex(1)])
        },
    );
    pane(content, "Request / Response [2]")
        .title_right(escape("History [ctrl h]"))
        .section("Response", |node| {
            node.bind("left h", "Previous tab", move |cx| switch_tab(s, -1, cx))
                .bind("right l", "Next tab", move |cx| switch_tab(s, 1, cx))
                .bind("slash", "Filter body", move |cx| {
                    // To the body's tab, whose own `/` focuses its box.
                    s.exchange_tab.set(1);
                    s.refocus(cx);
                })
                .bind("colon", "Export body", move |cx| export(s, cx))
                .bind("v", "View body in pager", move |cx| view_body(s, cx))
        })
}

/// `:`: the body piped to a command, the first line it prints in the
/// footer.
fn export(s: Slumber, cx: &mut Ctx) {
    let Some(body) = s.shown().and_then(|x| s.visible_body(&x)) else {
        return;
    };
    cx.prompt("Export", "", move |command, _| {
        spawn(
            move || pipe(&command, body.as_bytes()),
            move |result, cx| {
                let line = match result {
                    Ok(out) => escape(out.lines().next().unwrap_or("")),
                    Err(error) => format!("[red]{}[/]", escape(&error)),
                };
                s.notify(cx, line);
            },
        );
    });
}

/// `v`: the body in `$PAGER` (`less` by default).
fn view_body(s: Slumber, cx: &mut Ctx) {
    let Some(body) = s.shown().and_then(|x| s.visible_body(&x)) else {
        return;
    };
    let file = std::env::temp_dir().join(format!("slumber-body-{}.txt", std::process::id()));
    if std::fs::write(&file, body).is_err() {
        return;
    }
    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less".into());
    let mut words = pager.split_whitespace();
    let mut command = Command::new(words.next().unwrap_or("less"));
    command.args(words).arg(file);
    cx.handoff(command);
}

/// The bottom pane while the profile list is open: the profile's fields,
/// rendered.
fn profile_pane(s: Slumber, layout: Layout) -> Node {
    let fields = table_with(
        vec![
            Column::new("Field", Size::Auto),
            Column::new("Value", Size::Flex(1)),
        ],
        move || {
            let fields = s.fields();
            (fields.iter())
                .map(|(k, v)| vec![escape(k), preview(v, &fields)])
                .collect()
        },
        signal(0usize),
        TableOptions::default(),
    );
    pane(s.register(layout, Target::Profile, fields), "Profile")
}

/// The actions menu (`x`), Slumber's.
fn actions(s: Slumber, cx: &mut Ctx) {
    let shown = s.shown();
    let request = shown.as_ref().and_then(|x| x.request.clone());
    let body = shown.as_ref().and_then(|x| s.visible_body(x));
    let loading = shown.as_ref().is_some_and(Exchange::loading);
    let items = [
        "Copy URL",
        "Copy as cURL",
        "Copy Body",
        "Resend Request",
        "Delete Request",
        "Reload Collection",
    ];
    let items = items.iter().map(|i| i.to_string()).collect();
    cx.pick(
        "Actions",
        Origin::Center,
        items,
        move |pick, cx| match pick {
            0 | 1 => match request {
                Some(r) => cx.copy(if pick == 0 { r.url.clone() } else { curl(&r) }),
                None => s.notify(cx, "No request to copy"),
            },
            2 => match body {
                Some(body) => cx.copy(body),
                None => s.notify(cx, "No body to copy"),
            },
            3 if !loading => s.send_request(),
            4 => {
                if let Some(x) = shown {
                    s.cancel(x.id);
                    s.history.update(|h| h.retain(|e| e.id != x.id));
                    s.pinned.set(None);
                }
            }
            5 => reload(s, cx),
            _ => {}
        },
    );
}

/// Read the collection again, keeping what is selected where it still is.
fn reload(s: Slumber, cx: &mut Ctx) {
    let path = s.path.get();
    let read = std::fs::read_to_string(&path).map_err(|e| e.to_string());
    match read.and_then(|text| parse_collection(&text)) {
        Ok(collection) => {
            if collection.entry(&s.tree.get()).is_none() {
                s.tree.set(collection.first_recipe());
            }
            if s.profile.get() >= collection.profiles.len() {
                s.profile.set(0);
            }
            s.collection.set(collection);
            let path = escape(&path.display().to_string());
            s.notify(cx, format!("Reloaded collection from {path}"));
        }
        Err(error) => s.notify(
            cx,
            format!("[red]Error loading collection: {}[/]", escape(&error)),
        ),
    }
}

fn footer(s: Slumber) -> Node {
    row([
        text(move || match s.notice.get() {
            Some(notice) => notice,
            None => format!("[dim]{}[/]", escape(&s.path.get().display().to_string())),
        })
        .no_wrap()
        .flex(1),
        label("[white on blue]x actions / ? help / q quit[/]").auto(),
    ])
}

/// The panes of one layout (see [`Layout`]).
fn arrange(s: Slumber, layout: Layout, rects: &(RowRects, RowRects)) -> Node {
    let side = layout.sidebar.unwrap_or(Sidebar::Recipe);
    let bottom = move || match s.sidebar.get_untracked() {
        Sidebar::Profile => profile_pane(s, layout),
        _ => exchange_pane(s, layout),
    };
    match layout.full {
        Some(Pane::Sidebar) => return sidebar(s, layout, side),
        Some(Pane::Top) => return recipe_pane(s, layout, rects),
        Some(Pane::Bottom) => return bottom(),
        None => {}
    }
    let split = vsplit(recipe_pane(s, layout, rects), bottom(), s.ratio);
    let main = column([headers(s, layout).fixed(3), split.flex(1)]);
    match layout.sidebar {
        Some(which) => row([sidebar(s, layout, which).fixed(30), main.flex(1)]),
        None => main,
    }
}

/// Slumber's look: its default theme's blue for the focus and the
/// selection.
fn theme() -> Theme {
    let parse = |s: &str| Style::parse(s).expect("a style");
    let mut theme = Theme::dark()
        .style("selected", parse("bold white on blue"))
        .style("tree.selected", parse("bold blue"))
        .style("tabs.selected", parse("bold underline blue"))
        .style("tabs.selected.blur", parse("bold underline blue"))
        .style("table.header", parse("bold underline"))
        .style("popup.title", parse("blue"));
    theme.border = parse("white");
    theme.border_focused = parse("bold blue");
    theme
}

/// The app on the collection at `path`.
pub fn http_app(path: PathBuf) -> App {
    let loaded = (std::fs::read_to_string(&path))
        .map_err(|e| format!("{}: {e}", path.display()))
        .and_then(|text| parse_collection(&text));
    App::new(move || {
        let (collection, problem) = match loaded {
            Ok(collection) => (collection, None),
            Err(error) => (Collection::default(), Some(error)),
        };
        let s = Slumber::new(path.clone(), collection);
        if let Some(problem) = problem {
            let message = format!("[red]Error loading collection: {}[/]", escape(&problem));
            s.notice.set(Some(message));
        }
        // A tick while a request is in flight, for its elapsed time.
        every(Duration::from_millis(100), move |_| {
            if s.shown().is_some_and(|x| x.loading()) {
                s.tick.update(|t| *t += 1);
            }
        });
        // A history row picked shows its exchange; another recipe or
        // profile shows its latest again.
        watch(
            move || s.history_row.get(),
            move |row, _| {
                if s.sidebar.get_untracked() == Sidebar::History {
                    s.pinned.set(s.recipe_history().get(row).map(|x| x.id));
                }
            },
        );
        watch(
            move || (s.tree.get(), s.profile.get()),
            move |_, _| {
                s.pinned.set(None);
                s.filtered.set(None);
            },
        );
        // The focus follows the pane's content when it changes under it:
        // another layout, another kind of entry, the first exchange.
        watch(
            move || (s.layout(), s.kind(), s.shown().is_some()),
            move |_, cx| s.refocus(cx),
        );
        // A target wanted before its page was built, focused once it is.
        watch(
            move || (s.want.get(), s.focus.with(Registry::len)),
            move |(want, _), cx| {
                if let Some(id) =
                    want.and_then(|slot| s.focus.with_untracked(|r| r.get(&slot).copied()))
                {
                    s.want.set(None);
                    cx.focus(id);
                }
            },
        );
        #[cfg(feature = "watch")]
        {
            // Slumber reads the collection again when its file changes.
            let file = s.path.get_untracked();
            if let Ok(watch) = intuituive::watch_path(&file, move |_, cx| reload(s, cx)) {
                std::mem::forget(watch);
            }
        }
        let rects = (RowRects::new(), RowRects::new());
        let panes = switch(move || s.layout(), move |layout| arrange(s, layout, &rects));
        column([panes.flex(1), footer(s).fixed(1)])
            .section("Pane Navigation", |node| {
                node.bind("tab", "Next pane", move |cx| s.cycle_pane(1, cx))
                    .bind("shift+tab", "Previous pane", move |cx| s.cycle_pane(-1, cx))
                    .bind("1", "Select top pane", move |cx| {
                        s.select_pane(Pane::Top, cx)
                    })
                    .bind("2", "Select bottom pane", move |cx| {
                        s.select_pane(Pane::Bottom, cx)
                    })
                    .bind("r", "Recipe list", move |cx| {
                        s.open_sidebar(Sidebar::Recipe, cx)
                    })
                    .bind("p", "Profile list", move |cx| {
                        s.open_sidebar(Sidebar::Profile, cx)
                    })
                    .bind("ctrl+h", "Request history", move |cx| {
                        s.open_sidebar(Sidebar::History, cx)
                    })
            })
            .section("Layout Control", |node| {
                node.bind("s", "Toggle sidebar", move |cx| {
                    s.sidebar_open.update(|o| *o = !*o);
                    if !s.sidebar_open.get() && s.pane.get() == Pane::Sidebar {
                        s.pane.set(Pane::Top);
                    }
                    s.refocus(cx);
                })
                .bind("f", "Fullscreen", move |cx| {
                    s.fullscreen.update(|f| *f = !*f);
                    s.refocus(cx);
                })
                .bind("bracketleft", "Shrink top pane", move |_| {
                    s.ratio.update(|r| *r = (*r - 0.05).max(0.1))
                })
                .bind("bracketright", "Grow top pane", move |_| {
                    s.ratio.update(|r| *r = (*r + 0.05).min(0.9))
                })
            })
            .section("Interaction", |node| {
                node.bind("enter", "Send request", move |_| {
                    if !s.shown().is_some_and(|x| x.loading()) {
                        s.send_request();
                    }
                })
                .bind("esc", "Cancel request / exit fullscreen", move |cx| {
                    if s.fullscreen.get() {
                        s.fullscreen.set(false);
                        s.refocus(cx);
                    } else if let Some(x) = s.shown().filter(Exchange::loading) {
                        cx.confirm("Cancel request?", "", Vec::new(), move |_| s.cancel(x.id));
                    }
                })
                .bind("x", "Actions", move |cx| actions(s, cx))
            })
            .section("Collection Management", |node| {
                node.bind("f5", "Reload collection", move |cx| reload(s, cx))
            })
            .section("General", |node| node.bind("q", "Quit", |cx| cx.quit()))
    })
    .theme(theme())
    .help_key("?")
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    // The collection named, else Slumber's file names in this directory.
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            [
                "slumber.yml",
                "slumber.yaml",
                ".slumber.yml",
                ".slumber.yaml",
            ]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from("slumber.yml"))
        });
    rich_ext::syntax::preload(&["json"]);
    http_app(path).run()
}
