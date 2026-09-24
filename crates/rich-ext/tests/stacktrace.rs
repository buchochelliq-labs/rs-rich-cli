//! Stack traces captured from real runtimes (Python 3.11, Node 22, OpenJDK,
//! Rust) parse into the shared shape and render with their causes.
use rich::{Console, Renderable};
use rich_ext::hyperlink::Hyperlinker;
use rich_ext::stacktrace::{self, CauseKind, Language, StackTrace, TraceParser};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/traces")
        .join(name);
    std::fs::read_to_string(path).expect("fixture")
}

fn plain(renderable: &dyn Renderable) -> String {
    let console = Console::builder().width(100).build();
    console.render_to_string(renderable)
}

#[test]
fn python_chains_keep_both_relations_in_order() {
    let trace = stacktrace::parse(&fixture("python.txt")).expect("parsed");
    assert_eq!(trace.language, Language::Python);
    assert_eq!(trace.kind.as_deref(), Some("json.decoder.JSONDecodeError"));
    // Implicit chaining: raised while handling the ValueError.
    assert_eq!(trace.cause_kind, CauseKind::DuringHandling);
    let value_error = trace.cause.as_deref().expect("cause");
    assert_eq!(value_error.kind.as_deref(), Some("ValueError"));
    assert_eq!(value_error.message.as_deref(), Some("bad config"));
    // `raise … from e`: the KeyError caused it.
    assert_eq!(value_error.cause_kind, CauseKind::CausedBy);
    let key_error = value_error.cause.as_deref().expect("root");
    assert_eq!(key_error.message.as_deref(), Some("'port'"));
    assert_eq!(trace.chain().count(), 3);

    // Most recent call last; stdlib frames are library frames.
    let last = trace.frames.last().unwrap();
    assert_eq!(last.function.as_deref(), Some("raw_decode"));
    assert!(last.library);
    let origin = trace.origin().expect("application frame");
    assert_eq!(origin.function.as_deref(), Some("outer"));
    assert_eq!(origin.line, Some(13));
    assert_eq!(origin.source.as_deref(), Some("json.loads(\"{\")"));
    // The `~~^^^` marker rows are not taken for source.
    assert_eq!(
        key_error.frames[1].source.as_deref(),
        Some("return {}[\"port\"]")
    );
}

#[test]
fn node_traces_skip_the_excerpt_and_follow_cause() {
    let trace = stacktrace::parse(&fixture("node.txt")).expect("parsed");
    assert_eq!(trace.language, Language::JavaScript);
    assert_eq!(trace.kind.as_deref(), Some("TypeError"));
    assert_eq!(trace.message.as_deref(), Some("cannot save"));
    let innermost = trace.frames.last().unwrap();
    assert_eq!(innermost.function.as_deref(), Some("save"));
    assert_eq!((innermost.line, innermost.column), (Some(2), Some(52)));
    assert!(
        trace.frames[0].library,
        "node: internals are library frames"
    );
    assert!(trace
        .frames
        .iter()
        .any(|frame| frame.metadata.iter().any(|(key, _)| key == "elided")));
    let cause = trace.cause.as_deref().expect("[cause]");
    assert_eq!(cause.kind.as_deref(), Some("Error"));
    assert_eq!(cause.message.as_deref(), Some("disk full"));
    assert_eq!(cause.origin().unwrap().function.as_deref(), Some("load"));
}

#[test]
fn java_traces_take_the_thread_header_and_caused_by() {
    let trace = stacktrace::parse(&fixture("java.txt")).expect("parsed");
    assert_eq!(trace.language, Language::Java);
    assert_eq!(
        trace.kind.as_deref(),
        Some("java.lang.IllegalStateException")
    );
    assert_eq!(
        trace.frames.last().unwrap().function.as_deref(),
        Some("Main.save")
    );
    assert_eq!(
        trace.frames.last().unwrap().path.as_deref(),
        Some("Main.java")
    );
    let cause = trace.cause.as_deref().expect("Caused by");
    assert_eq!(cause.kind.as_deref(), Some("java.io.IOException"));
    assert_eq!(
        cause.frames.len(),
        2,
        "one frame and the `... 2 more` marker"
    );
}

#[test]
fn rust_panics_keep_the_message_location_and_app_frames() {
    let trace = stacktrace::parse(&fixture("rust.txt")).expect("parsed");
    assert_eq!(trace.language, Language::Rust);
    assert_eq!(trace.kind.as_deref(), Some("panic"));
    assert_eq!(
        trace.message.as_deref(),
        Some("port must be a number: ParseIntError { kind: InvalidDigit }")
    );
    let location = trace.location.as_ref().expect("panicked at");
    assert_eq!(location.path.as_deref(), Some("main.rs"));
    assert_eq!((location.line, location.column), (Some(1), Some(46)));
    let origin = trace.origin().expect("application frame");
    assert_eq!(origin.function.as_deref(), Some("main::parse"));
    assert_eq!(origin.path.as_deref(), Some("./main.rs"));
    let app: Vec<_> = trace.frames.iter().filter(|f| !f.library).collect();
    assert_eq!(app.len(), 2, "{app:?}");
}

#[test]
fn rendering_collapses_library_frames_and_orders_causes_first() {
    let trace = stacktrace::parse(&fixture("python.txt")).unwrap();
    let text = plain(&trace);
    let key = text.find("KeyError: 'port'").unwrap();
    let value = text.find("ValueError: bad config").unwrap();
    let json = text.find("json.decoder.JSONDecodeError").unwrap();
    assert!(key < value && value < json, "{text}");
    assert!(text.contains("The error above caused the following error:"));
    assert!(text.contains("while handling the error above"));
    assert!(text.contains("… 3 library frames"), "{text}");
    assert!(text.contains("at /srv/app/app.py:13"));
    let all = plain(&trace.render_options().show_library(true));
    assert!(all.contains("raw_decode") && !all.contains("library frames"));
}

#[test]
fn frame_locations_link_on_terminals_only() {
    let trace = stacktrace::parse(&fixture("rust.txt")).unwrap();
    let terminal = Console::builder().force_terminal(true).width(100).build();
    let view = trace
        .render_options()
        .hyperlinker(Hyperlinker::new().base_dir("/srv/app"));
    let out = terminal.render_to_string(&view);
    assert!(out.contains("\x1b]8;"), "{out:?}");
    assert!(out.contains("file:///srv/app/main.rs#1"), "{out:?}");
    assert!(!plain(&view).contains("\x1b"));
}

struct Custom;
impl TraceParser for Custom {
    fn detect(&self, text: &str) -> bool {
        text.starts_with("custom:")
    }
    fn parse(&self, text: &str) -> Option<StackTrace> {
        let mut trace = StackTrace::new(Language::Other("custom".into()));
        trace.message = Some(text["custom:".len()..].trim().to_string());
        Some(trace)
    }
}

#[test]
fn parsers_are_pluggable() {
    let parsers = stacktrace::Parsers::new().with_parser(Custom);
    let trace = parsers.parse("custom: hello").unwrap();
    assert_eq!(trace.language, Language::Other("custom".into()));
    assert!(parsers.parse(&fixture("java.txt")).is_some());
    assert!(stacktrace::parse("just some text").is_none());
}

#[test]
fn capture_reads_this_threads_backtrace() {
    let trace = stacktrace::capture("boom");
    assert_eq!(trace.message.as_deref(), Some("boom"));
    assert!(!trace.frames.is_empty());
}

/// Run `f` on a thread with the default (2 MiB) spawned-thread stack, so a
/// recursion over a long cause chain overflows here as it would in an app.
fn on_normal_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .expect("no stack overflow");
}

#[test]
fn long_java_cause_chains_are_capped_and_render() {
    on_normal_stack(|| {
        let mut input = String::from("E: top\n\tat a.B.c(B.java:1)\n");
        for i in 0..2000 {
            input.push_str(&format!("Caused by: E: x{i}\n"));
        }
        let trace = stacktrace::parse(&input).unwrap();
        let chain: Vec<_> = trace.chain().collect();
        assert_eq!(chain.len(), stacktrace::MAX_CAUSES + 1);
        assert_eq!(
            chain.last().unwrap().omitted_causes,
            2000 - stacktrace::MAX_CAUSES
        );
        let text = plain(&trace);
        assert!(text.contains("… 1936 more causes"), "{}", &text[..200]);
        assert!(text.contains("E: top") && text.contains("E: x63") && !text.contains("E: x64"));
    });
}

#[test]
fn long_python_and_node_cause_chains_are_capped() {
    on_normal_stack(|| {
        let block =
            "Traceback (most recent call last):\n  File \"a.py\", line 1, in f\nValueError: x\n";
        let mut input = String::from(block);
        for _ in 0..50_000 {
            input.push_str(
                "\nThe above exception was the direct cause of the following exception:\n\n",
            );
            input.push_str(block);
        }
        let trace = stacktrace::parse(&input).unwrap();
        assert_eq!(trace.chain().count(), stacktrace::MAX_CAUSES + 1);
        assert!(plain(&trace).contains("… 49936 more causes"));

        let mut input = String::from("Error: top\n    at f (/a.js:1:1)\n");
        for _ in 0..5000 {
            input.push_str("  [cause]: Error: x\n      at g (/b.js:2:2)\n");
        }
        let trace = stacktrace::parse(&input).unwrap();
        assert_eq!(trace.chain().count(), stacktrace::MAX_CAUSES + 1);
        assert!(plain(&trace).contains("… 4936 more causes"));
    });
}

#[test]
fn deep_hand_built_chains_drop_and_render_without_recursing() {
    on_normal_stack(|| {
        let mut trace = StackTrace::new(Language::Rust);
        for i in 0..200_000 {
            let mut next = StackTrace::new(Language::Rust);
            next.message = Some(format!("m{i}"));
            next.cause = Some(Box::new(trace));
            trace = next;
        }
        let text = plain(&trace);
        assert!(text.contains("more causes"), "{}", &text[..200]);
        drop(trace);
    });
}
