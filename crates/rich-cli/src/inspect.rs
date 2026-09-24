//! `rich inspect` and `--format`: structured data through `rich_ext::data`.
//!
//! Neither exists upstream. Both are binary-boundary conveniences: they parse
//! with rich-ext's data layer and render its views, and the default command
//! line is untouched. Without `--format`, piped input still renders as plain
//! text exactly as upstream's does; detection is something a user asks for.
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment};
use rich_ext::data::{
    self, DiffView, Explorer, FlatView, Format, Node, Redaction, SearchQuery, SearchResults,
    Selectors, Value, View,
};
use std::borrow::Cow;

/// `--format`'s value: a named format, or `auto` to detect one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputFormat {
    Auto,
    Named(Format),
}

pub(crate) const FORMAT_USAGE: &str = "--format requires auto, json, yaml, toml, xml, ini or env";

impl InputFormat {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        Format::from_name(value)
            .map(Self::Named)
            .ok_or_else(|| FORMAT_USAGE.to_string())
    }
}

/// `--format` and the options that only mean something with `--inspect`.
#[derive(Clone, Debug, Default)]
pub(crate) struct DataOptions {
    pub format: Option<InputFormat>,
    select: Option<String>,
    find: Option<String>,
    flatten: bool,
    table: bool,
    max_depth: Option<usize>,
    max_length: Option<usize>,
    show_paths: bool,
    redact: bool,
    compare: Option<String>,
}

fn positive(flag: &str, value: Option<&String>) -> Result<usize, String> {
    value
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("{flag} requires a positive integer"))
}

impl DataOptions {
    /// Consume one of these options; anything else stays with the main parser.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match arg {
            "--format" => {
                self.format = Some(InputFormat::parse(rest.next().ok_or(FORMAT_USAGE)?)?);
            }
            "--select" => {
                let expression = rest.next().ok_or("--select requires an expression")?;
                // A malformed expression is a usage error, found before any
                // input is read.
                Selectors::default()
                    .compile("jsonpath", expression)
                    .map_err(|err| format!("invalid --select expression: {err}"))?;
                self.select = Some(expression.clone());
            }
            "--find" => {
                self.find = Some(rest.next().ok_or("--find requires a search text")?.clone());
            }
            "--compare" => {
                self.compare = Some(rest.next().ok_or("--compare requires a path")?.clone());
            }
            "--max-depth" => self.max_depth = Some(positive(arg, rest.next())?),
            "--max-length" => self.max_length = Some(positive(arg, rest.next())?),
            "--flatten" => self.flatten = true,
            "--table" => self.table = true,
            "--show-paths" => self.show_paths = true,
            "--redact" => self.redact = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Whether `--redact` was given (it also applies to `rich capture`).
    pub(crate) fn redact(&self) -> bool {
        self.redact
    }

    /// The first inspect-only option given, for the "only has an effect" check.
    /// `--redact` is left out when `redact_elsewhere`: `rich capture` takes it too.
    pub(crate) fn inspect_only_option(&self, redact_elsewhere: bool) -> Option<&'static str> {
        [
            ("--select", self.select.is_some()),
            ("--find", self.find.is_some()),
            ("--flatten", self.flatten),
            ("--table", self.table),
            ("--max-depth", self.max_depth.is_some()),
            ("--max-length", self.max_length.is_some()),
            ("--show-paths", self.show_paths),
            ("--redact", self.redact && !redact_elsewhere),
            ("--compare", self.compare.is_some()),
        ]
        .into_iter()
        .find_map(|(flag, given)| given.then_some(flag))
    }

    /// Views that replace one another; more than one is ambiguous.
    pub(crate) fn validate(&self) -> Result<(), String> {
        let views: Vec<_> = [
            ("--find", self.find.is_some()),
            ("--flatten", self.flatten),
            ("--table", self.table),
            ("--compare", self.compare.is_some()),
        ]
        .into_iter()
        .filter_map(|(flag, given)| given.then_some(flag))
        .collect();
        if views.len() > 1 {
            return Err(format!("{} cannot be combined", views.join(" and ")));
        }
        Ok(())
    }
}

/// Where `--format` sends input that would otherwise render as plain text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Route {
    Json,
    Syntax(&'static str),
    Text,
}

/// A file name worth handing to detection; stdin (`-`) has none.
fn name_hint(resource: Option<&str>) -> Option<&str> {
    resource
        .filter(|r| *r != "-")
        .map(|r| r.rsplit(['/', '\\']).next().unwrap_or(r))
}

/// The format `--format` names, or the one detection finds.
fn resolve(format: InputFormat, content: &str, resource: Option<&str>) -> Option<Format> {
    match format {
        InputFormat::Named(format) => Some(format),
        InputFormat::Auto => Format::detect(content, name_hint(resource)),
    }
}

/// Route input by format: JSON to the JSON renderer, the other formats to
/// highlighting, and anything detection cannot place to plain text.
pub(crate) fn route(format: InputFormat, content: &str, resource: Option<&str>) -> Route {
    match resolve(format, content, resource) {
        Some(Format::Json) => Route::Json,
        Some(Format::Yaml) => Route::Syntax("yaml"),
        Some(Format::Toml) => Route::Syntax("toml"),
        Some(Format::Xml) => Route::Syntax("xml"),
        Some(Format::Ini) => Route::Syntax("ini"),
        Some(Format::Dotenv) => Route::Syntax("bash"),
        None => Route::Text,
    }
}

fn parse_document(
    format: InputFormat,
    content: &str,
    resource: Option<&str>,
) -> Result<(Format, Node), String> {
    let name = name_hint(resource).unwrap_or("<stdin>");
    let format = resolve(format, content, resource).ok_or_else(|| {
        format!(
            "cannot detect the format of {name}; pass --format json, yaml, toml, xml, ini or env"
        )
    })?;
    let node = data::parse(format, content).map_err(|err| {
        // A parser's depth limit is not a syntax error in the document.
        let problem = if err.message.contains("recursion limit") {
            format!("{} too deeply nested to read", format.name())
        } else {
            format!("invalid {}: {}", format.name(), err.message)
        };
        match err.position {
            Some(at) => format!("{name}:{}:{}: {problem}", at.line, at.column),
            None => format!("{name}: {problem}"),
        }
    })?;
    Ok((format, node))
}

/// Search hits borrow their document, so this owns it and builds the view
/// when rendered.
struct Found {
    node: Node,
    query: SearchQuery,
}

impl Found {
    fn view(&self) -> SearchResults<'_> {
        SearchResults::new(&self.node, &self.query).context(2)
    }
}

impl Renderable for Found {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.view().rich_render(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.view().measure(console, options)
    }
}

/// Build `--inspect`'s view of `content`. `read` loads `--compare`'s document.
pub(crate) fn build(
    options: &DataOptions,
    content: &str,
    resource: Option<&str>,
    read: impl FnOnce(&str) -> Result<String, String>,
) -> Result<Box<dyn Renderable>, String> {
    let format = options.format.unwrap_or(InputFormat::Auto);
    let (format, mut node) = parse_document(format, content, resource)?;
    let redaction = options.redact.then(Redaction::secrets);
    if let Some(redaction) = &redaction {
        node = node.redacted(redaction);
    }

    if let Some(other) = &options.compare {
        let text = read(other)?;
        // The other document is read as the same format unless its name says
        // otherwise: comparing `a.json` with `b.yaml` works.
        let other_format = Format::from_file_name(name_hint(Some(other)).unwrap_or(other))
            .map(InputFormat::Named)
            .unwrap_or(InputFormat::Named(format));
        let (_, mut other_node) = parse_document(other_format, &text, Some(other))?;
        if let Some(redaction) = &redaction {
            other_node = other_node.redacted(redaction);
        }
        return Ok(Box::new(DiffView::new(&node, &other_node)));
    }

    let mut label = name_hint(resource).unwrap_or("<stdin>").to_string();
    if let Some(expression) = &options.select {
        let selector = Selectors::default()
            .compile("jsonpath", expression)
            .map_err(|err| format!("invalid --select expression: {err}"))?;
        let hits: Vec<_> = selector
            .select(&node)
            .map_err(|err| format!("--select failed: {err}"))?
            .into_iter()
            .map(|(path, hit)| (path.to_string(), hit.clone()))
            .collect();
        // One hit is explored as itself; several become a map keyed by path.
        node = match <[_; 1]>::try_from(hits) {
            Ok([(path, hit)]) => {
                label = if path.is_empty() { "$".into() } else { path };
                hit
            }
            Err(hits) => Node::new(Value::Map(hits)),
        };
    }

    if let Some(text) = &options.find {
        let query = SearchQuery::text(text.as_str()).case_insensitive(true);
        return Ok(Box::new(Found { node, query }));
    }
    if options.flatten {
        return Ok(Box::new(FlatView::new(node)));
    }
    let untouched = options.select.is_none() && !options.table;
    if untouched && matches!(format, Format::Ini | Format::Dotenv) && options.max_depth.is_none() {
        return Ok(Box::new(data::ConfigFileView::new(node)));
    }
    let mut explorer = Explorer::new(Cow::Owned(node))
        .root_label(label)
        .show_paths(options.show_paths);
    if options.table {
        explorer = explorer.view(View::Table);
    }
    if let Some(depth) = options.max_depth {
        explorer = explorer.max_depth(depth);
    }
    if let Some(length) = options.max_length {
        explorer = explorer.max_length(length);
    }
    Ok(Box::new(explorer))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn parsed(args: &[&str]) -> Result<DataOptions, String> {
        let args = strings(args);
        let mut options = DataOptions::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            assert!(options.parse_option(arg, &mut iter)?, "{arg} not consumed");
        }
        Ok(options)
    }

    fn render(options: &DataOptions, content: &str, resource: Option<&str>) -> String {
        let view = build(options, content, resource, |_| Err("no".into())).unwrap();
        Console::builder()
            .width(60)
            .color_system(None)
            .build()
            .render_export(view.as_ref())
    }

    #[test]
    fn formats_parse_and_reject_unknown_names() {
        assert_eq!(InputFormat::parse("auto"), Ok(InputFormat::Auto));
        assert_eq!(
            InputFormat::parse("env"),
            Ok(InputFormat::Named(Format::Dotenv))
        );
        assert_eq!(InputFormat::parse("csv"), Err(FORMAT_USAGE.to_string()));
        assert!(parsed(&["--max-depth", "0"]).is_err());
        assert!(parsed(&["--format"]).is_err());
    }

    #[test]
    fn routes_follow_the_format_and_fall_back_to_text() {
        let auto = InputFormat::Auto;
        assert_eq!(route(auto, r#"{"a": 1}"#, None), Route::Json);
        assert_eq!(
            route(auto, "a:\n  b: 1\nc: 2\n", Some("-")),
            Route::Syntax("yaml")
        );
        assert_eq!(route(auto, "just some words\n", None), Route::Text);
        // A named format wins over the content.
        let named = InputFormat::Named(Format::Toml);
        assert_eq!(
            route(named, "just some words\n", None),
            Route::Syntax("toml")
        );
    }

    #[test]
    fn inspect_explores_selects_and_finds() {
        let doc = r#"{"servers": [{"name": "a", "port": 80}, {"name": "b", "port": 81}]}"#;
        let out = render(&DataOptions::default(), doc, Some("hosts.json"));
        assert!(out.starts_with("hosts.json"), "{out}");
        assert!(out.contains("port") && out.contains("81"), "{out}");

        let select = parsed(&["--select", "$.servers[*].name"]).unwrap();
        let out = render(&select, doc, None);
        assert!(
            out.contains("servers[0].name") && out.contains("\"b\""),
            "{out}"
        );

        let find = parsed(&["--find", "PORT"]).unwrap();
        let out = render(&find, doc, None);
        assert!(out.contains("servers[1].port: 81"), "{out}");
        assert!(out.contains("2 matches"), "{out}");
    }

    #[test]
    fn inspect_reports_detection_and_parse_errors_with_locations() {
        let err = build(&DataOptions::default(), "plain words\n", None, |_| {
            Err("no".into())
        })
        .err()
        .unwrap();
        assert!(err.contains("cannot detect the format of <stdin>"), "{err}");

        let json = parsed(&["--format", "json"]).unwrap();
        let err = build(&json, "{\n  \"a\": \n}", Some("bad.json"), |_| {
            Err("no".into())
        })
        .err()
        .unwrap();
        assert!(err.starts_with("bad.json:3:1: invalid json:"), "{err}");
    }

    #[test]
    fn dotenv_renders_as_a_config_table_and_redacts() {
        let options = parsed(&["--redact"]).unwrap();
        let out = render(&options, "USER=ada\nAPI_TOKEN=abc123\n", Some(".env"));
        assert!(out.contains("ada"), "{out}");
        assert!(!out.contains("abc123"), "{out}");
    }

    #[test]
    fn compare_reads_the_other_document() {
        let options = parsed(&["--compare", "new.json"]).unwrap();
        let view = build(&options, r#"{"a": 1, "b": 2}"#, Some("old.json"), |path| {
            assert_eq!(path, "new.json");
            Ok(r#"{"a": 1, "b": 3, "c": 4}"#.into())
        })
        .unwrap();
        let out = Console::builder()
            .width(60)
            .color_system(None)
            .build()
            .render_export(view.as_ref());
        assert!(
            out.contains('b') && out.contains('3') && out.contains('c'),
            "{out}"
        );
    }

    #[test]
    fn view_options_are_exclusive() {
        assert!(parsed(&["--flatten", "--table"])
            .unwrap()
            .validate()
            .is_err());
        assert!(parsed(&["--flatten", "--select", "$.a"])
            .unwrap()
            .validate()
            .is_ok());
    }
}
