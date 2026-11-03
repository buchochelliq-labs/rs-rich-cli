//! `Stylesheet::parse` takes time linear in the stylesheet's length: nesting
//! and variables are bounded (the `stylesheet` fuzz target's timeout of
//! 2026-11-03, see `docs/design/fuzzing.md`).

use intuituive::Stylesheet;
use std::time::{Duration, Instant};

/// Far above what any of these takes, even in a debug build, so the tests
/// are not flaky; the inputs took seconds to minutes before the bounds.
const QUICK: Duration = Duration::from_secs(1);

/// Parse `css`, check it was quick, and return the error message, if any.
fn parse_quickly(css: &str) -> Result<(), String> {
    let start = Instant::now();
    let parsed = Stylesheet::parse(css);
    let took = start.elapsed();
    assert!(took < QUICK, "{} bytes took {took:?}", css.len());
    parsed.map(|_| ()).map_err(|error| error.message)
}

fn expands_too_far(result: Result<(), String>) {
    let message = result.unwrap_err();
    assert!(
        message.starts_with("nested selectors expand this stylesheet past "),
        "{message}"
    );
}

/// `a { & { & { … } } }`, `levels` deep with `amps` `&` at each.
fn repeated_parents(levels: usize, amps: usize) -> String {
    let amp = "&".repeat(amps);
    format!(
        "a {{ {} size: 1; {} }}",
        format!("{amp} {{ ").repeat(levels),
        "} ".repeat(levels)
    )
}

#[test]
fn the_fuzz_input_errors_at_once() {
    // The input as the fuzz target reads it: invalid UTF-8 replaced.
    let css = String::from_utf8_lossy(include_bytes!("fixtures/fuzz/stylesheet-timeout.css"));
    expands_too_far(parse_quickly(&css));
}

#[test]
fn repeated_parents_expand_at_most_so_far() {
    // The finding, shrunk: each `&` is its parent's selectors, so five
    // levels of 32 make one selector of 32^5 (33 million) bytes.
    let css = "a{&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&{&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&{&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&\
               {&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&{&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&{size:1}}}}}}";
    expands_too_far(parse_quickly(css));
    for amps in [64, 1024, 8192] {
        expands_too_far(parse_quickly(&repeated_parents(3, amps)));
    }
    // A few `&` still read as their flattened form.
    assert_eq!(parse_quickly(&repeated_parents(3, 4)), Ok(()));
}

#[test]
fn selector_lists_multiply_at_most_so_far() {
    // `a, a, … { a, a, … { … } }`: each nested list is read with each of
    // its parents', k^levels rules.
    let list = |k: usize| vec!["a"; k].join(",");
    for k in [64, 1024, 8192] {
        let l = list(k);
        expands_too_far(parse_quickly(&format!(
            "{l} {{ {l} {{ {l} {{ size: 1; }} }} }}"
        )));
    }
    // As many rules as the bound allows, from 32 KiB: each nested `&` is
    // two bytes (with its comma), so the 8 × 8192 nested rules here are
    // four times its length, as far as it may expand; one more is not.
    let padded = |children: usize| {
        let ands = vec!["&"; children].join(",");
        let css = format!("{} {{ {ands} {{ size: 1; }} }}", list(8192));
        format!("{css}{}", " ".repeat(32 * 1024 - css.len()))
    };
    assert_eq!(parse_quickly(&padded(8)), Ok(()));
    expands_too_far(parse_quickly(&padded(9)));
}

#[test]
fn nesting_is_at_most_32_deep() {
    let deep = |levels: usize| {
        format!(
            "a {{ {} size: 1; {} }}",
            "& { ".repeat(levels),
            "} ".repeat(levels)
        )
    };
    // The rule and 31 nested in it.
    assert_eq!(parse_quickly(&deep(31)), Ok(()));
    for levels in [32, 1000, 16000] {
        assert_eq!(
            parse_quickly(&deep(levels)),
            Err("rules nested more than 32 deep".to_string())
        );
    }
    let error = Stylesheet::parse(&deep(32)).unwrap_err();
    assert_eq!((error.line, error.column), (1, 129), "{error}");
}

#[test]
fn variables_expand_at_most_so_far() {
    // Each `$a: $a$a;` doubles `$a`.
    let css = format!("$a: x; {} a {{ color: $a; }}", "$a: $a$a; ".repeat(40));
    let message = parse_quickly(&css).unwrap_err();
    assert!(
        message.starts_with("variables expand this stylesheet past "),
        "{message}"
    );
    // Many variables, used many times.
    let decls: String = (0..8000).map(|i| format!("$v{i}: red; ")).collect();
    let uses = vec!["$v0"; 8000].join(" ");
    let message = parse_quickly(&format!("{decls} a {{ color: {uses}; }}")).unwrap_err();
    assert!(message.ends_with("is not a colour"), "{message}");
    let css = format!("{decls} a {{ color: $v7999; }}");
    assert_eq!(parse_quickly(&css), Ok(()));
}

#[test]
fn long_bodies_are_read_once() {
    // A body is not scanned again for each declaration or nested rule in it.
    let rules = format!("a {{ {} }}", "b { } ".repeat(16000));
    assert_eq!(parse_quickly(&rules), Ok(()));
    let decls = format!("a {{ {} }}", "size: 1; ".repeat(16000));
    assert_eq!(parse_quickly(&decls), Ok(()));
}
