//! Org, AsciiDoc and man pages: `--org`, `--asciidoc`, `--man`, the document
//! names `--format` takes, and detection by extension and content.
//!
//! Not upstream: rich-cli renders Markdown and reStructuredText only. This is
//! a binary-boundary convenience (see `docs/PORTING.md`): it routes input to
//! `rich_ext::document`'s public renderers and nothing else. A file with one
//! of these extensions that upstream highlighted as source (`notes.org`,
//! `ls.1`) still is with `--syntax` (or `rich syntax FILE`), as is any file
//! when a source option such as `--line-numbers` is given.
//!
//! Detection, after the flags (which always win):
//!
//! - `.org`, `.adoc`, `.asciidoc`, `.man`, `.roff` and `.mdoc` by extension;
//! - a manual-section extension (`.1` to `.9`, with a suffix such as `.3pm`)
//!   or `.asc` only when the content agrees (`syslog.1` is a rotated log,
//!   `key.asc` a PGP signature);
//! - a file without an extension only when its content is unmistakably
//!   Org, AsciiDoc or roff;
//! - standard input never, unless `--format auto` asks (then a likely match
//!   is enough).
//!
//! Any other extension is never a document here: `main.c` stays C even if a
//! string in it reads `.TH`.

use std::collections::HashSet;
use std::sync::Arc;

use rich::{FenceRenderer, Renderable};
use rich_ext::document::{
    base_name, DocumentFormat, DocumentRegistry, ManFormat, NameMatch, RenderOptions, Severity,
    CERTAIN, LIKELY,
};

use super::{controls, Cli, Mode};

/// The mode for a document format's id.
fn mode_for(id: &str) -> Option<Mode> {
    Some(match id {
        "markdown" => Mode::Markdown,
        "rst" => Mode::Rst,
        "org" => Mode::Org,
        "asciidoc" => Mode::AsciiDoc,
        "man" => Mode::Man,
        _ => return None,
    })
}

/// The mode a `--format` value names, when it names a document format
/// (`org`, `adoc`, `man`, `markdown`, `rst`, or one of their aliases).
pub(crate) fn format_mode(value: &str) -> Option<Mode> {
    let registry = DocumentRegistry::with_builtins();
    let format = registry.get(value)?;
    mode_for(format.id())
}

/// The document names `--format` accepts, for its usage message.
pub(crate) const FORMAT_USAGE: &str = "--format requires auto, json, yaml, toml, xml, ini or env, \
     or a document format: markdown, rst, org, asciidoc or man";

/// Whether `resource` is a URL with no path (`https://example.org`), whose
/// "extension" is part of a host name.
pub(crate) fn bare_host(resource: &str) -> bool {
    super::is_url(resource)
        && resource
            .split_once("://")
            .is_none_or(|(_, rest)| !rest.split(['?', '#']).next().unwrap_or("").contains('/'))
}

/// The document mode for input its extension did not settle: a man page
/// named for its section, an `.asc` AsciiDoc file, an extensionless file
/// that is unmistakably a document, or (with `auto`, which is
/// `--format auto`) piped input that is likely one.
pub(crate) fn by_content(resource: Option<&str>, content: &str, auto: bool) -> Option<Mode> {
    let registry = DocumentRegistry::with_builtins();
    let path = resource.filter(|resource| *resource != "-");
    let detection = match path {
        // A URL's own extension may be confirmed; without one, its
        // Content-Type decides, as upstream's does.
        Some(url) if super::is_url(url) => {
            if bare_host(url) {
                return None;
            }
            registry.detect(Some(url), content, f32::INFINITY)
        }
        Some(path) => registry.detect(Some(path), content, if auto { LIKELY } else { CERTAIN }),
        None if auto => registry.detect(None, content, LIKELY),
        None => None,
    }?;
    mode_for(&detection.format)
}

/// Render `content` as the document `mode` names, printing what could not
/// be shown as warnings (unless the report is JSON, or `--watch` redraws).
pub(crate) fn render(
    mode: Mode,
    content: &str,
    cli: &Cli,
    fences: Option<Arc<dyn FenceRenderer>>,
) -> Box<dyn Renderable> {
    let id = match mode {
        Mode::Org => "org",
        Mode::AsciiDoc => "asciidoc",
        _ => "man",
    };
    let mut options = RenderOptions::new().hyperlinks(cli.hyperlinks);
    if let Some(lexer) = &cli.source.lexer {
        options = options.default_language(lexer.as_str());
    }
    if let Some(fences) = fences {
        options = options.fence_renderer(fences);
    }
    let rendered = DocumentRegistry::with_builtins()
        .render(content, id, &options)
        .expect("a built-in format");
    let quiet = cli.report_format == super::ReportFormat::Json || super::watch::capturing();
    if !quiet {
        let name = cli
            .resource
            .as_deref()
            .filter(|resource| *resource != "-")
            .unwrap_or("<stdin>");
        warn(name, &rendered.diagnostics);
    }
    rendered.renderable
}

/// At most this many warnings print for one document.
const MAX_WARNINGS: usize = 10;

fn warn(name: &str, diagnostics: &[rich_ext::document::Diagnostic]) {
    let mut seen = HashSet::new();
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Warning)
        .filter(|diagnostic| seen.insert(diagnostic.message.clone()))
        .collect();
    for diagnostic in warnings.iter().take(MAX_WARNINGS) {
        let location = match diagnostic.line {
            Some(line) => format!("{name}:{line}"),
            None => name.to_string(),
        };
        let message = format!("rich: warning: {location}: {}", diagnostic.message);
        eprintln!("{}", controls::shown(&message));
    }
    if warnings.len() > MAX_WARNINGS {
        eprintln!(
            "rich: warning: {}: {} more warnings",
            controls::shown(name),
            warnings.len() - MAX_WARNINGS
        );
    }
}

/// The error for a compressed man page (`ls.1.gz`), which is not decoded
/// here; a pipe through `zcat` shows it.
pub(crate) fn compressed_man_page(resource: &str) -> Option<String> {
    let name = base_name(resource);
    let stem = [".gz", ".bz2", ".xz", ".zst", ".Z"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))?;
    (ManFormat.match_name(stem) != NameMatch::No).then(|| {
        format!(
            "{resource} is a compressed man page, which rich does not decode; \
             try: zcat {resource} | rich --man -"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_names_map_to_modes() {
        assert_eq!(format_mode("org"), Some(Mode::Org));
        assert_eq!(format_mode("ADOC"), Some(Mode::AsciiDoc));
        assert_eq!(format_mode("roff"), Some(Mode::Man));
        assert_eq!(format_mode("md"), Some(Mode::Markdown));
        assert_eq!(format_mode("rst"), Some(Mode::Rst));
        assert_eq!(format_mode("json"), None);
        assert_eq!(format_mode("auto"), None);
    }

    #[test]
    fn content_routes_only_where_the_name_allows() {
        let page = ".TH LS 1\n.SH NAME\nls \\- list\n";
        assert_eq!(by_content(Some("ls.1"), page, false), Some(Mode::Man));
        assert_eq!(by_content(Some("syslog.1"), "boot ok\n", false), None);
        assert_eq!(by_content(Some("main.c"), page, true), None);
        assert_eq!(by_content(Some("LS"), page, false), Some(Mode::Man));
        // Standard input only with `--format auto`.
        assert_eq!(by_content(None, page, false), None);
        assert_eq!(by_content(Some("-"), page, true), Some(Mode::Man));
        // A URL needs its own extension.
        assert_eq!(
            by_content(Some("https://x.org/ls.1"), page, false),
            Some(Mode::Man)
        );
        assert_eq!(by_content(Some("https://x.org/ls"), page, true), None);
        assert_eq!(by_content(Some("https://example.org"), page, true), None);
    }

    #[test]
    fn bare_hosts_have_no_extension() {
        assert!(bare_host("https://example.org"));
        assert!(bare_host("https://example.org?x=1"));
        assert!(!bare_host("https://example.org/notes.org"));
        assert!(!bare_host("notes.org"));
    }

    #[test]
    fn compressed_man_pages_get_a_hint() {
        let message = compressed_man_page("/usr/share/man/man1/ls.1.gz").unwrap();
        assert!(message.contains("zcat /usr/share/man/man1/ls.1.gz | rich --man -"));
        assert!(compressed_man_page("ls.1.xz").is_some());
        assert_eq!(compressed_man_page("archive.tar.gz"), None);
        assert_eq!(compressed_man_page("ls.1"), None);
    }
}
