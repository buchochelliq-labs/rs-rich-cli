//! `--redact-preview` on `rich capture` and `rich log` (#722): what the
//! redactor would mask, shown before anything is written.
//!
//! Not upstream: a binary-boundary convenience over rich-ext's
//! [`Redactor::preview`] family and its [`Preview`] renderable. The preview
//! replaces the command's usual output; no cast, export or log output is
//! written, and a dry-run line on stderr names what would have been.

use rich::Console;
use rich_ext::redact::{Preview, Redactor};

use super::{authoring, viewers, Cli, ExitClass, ExitCode, ReportFormat};

/// `rich capture`'s previews: the command line (as
/// [`Redactor::redact_args`] masks it) and the output (as the ANSI
/// redaction masks it).
pub(crate) fn capture(
    captured: &viewers::Captured,
    redactor: &Redactor,
) -> Vec<(&'static str, Preview)> {
    let output: Vec<u8> = captured
        .chunks
        .iter()
        .flat_map(|(_, bytes)| bytes.iter().copied())
        .collect();
    vec![
        (
            "command",
            redactor.preview_args(&captured.command).title("command"),
        ),
        (
            "output",
            redactor
                .preview_ansi(&String::from_utf8_lossy(&output))
                .title("output"),
        ),
    ]
}

/// The previews as one JSON document: per source its line count, matches
/// and findings (lines and columns from 1), and the files a run without
/// `--redact-preview` would write.
pub(crate) fn json(previews: &[(&str, Preview)], would_write: &[String]) -> serde_json::Value {
    let sources: Vec<serde_json::Value> = previews
        .iter()
        .map(|(name, preview)| {
            let counts: serde_json::Map<String, serde_json::Value> = preview
                .counts()
                .into_iter()
                .map(|(kind, n)| (kind, n.into()))
                .collect();
            let findings: Vec<serde_json::Value> = preview
                .findings()
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "line": f.line + 1,
                        "column": f.column + 1,
                        "kind": f.kind,
                        "text": f.original,
                        "replacement": f.replacement,
                        "hidden": f.hidden,
                    })
                })
                .collect();
            serde_json::json!({
                "source": name,
                "lines": preview.total_lines(),
                "lines_affected": preview.lines_affected(),
                "matches": preview.findings().len(),
                "counts": counts,
                "findings": findings,
            })
        })
        .collect();
    let matches: usize = previews.iter().map(|(_, p)| p.findings().len()).sum();
    serde_json::json!({
        "redact_preview": {
            "matches": matches,
            "sources": sources,
            "would_write": would_write,
            "written": [],
        }
    })
}

/// The dry-run line: nothing was written, and what would have been.
pub(crate) fn dry_run(would_write: &[String]) -> String {
    if would_write.is_empty() {
        "rich: --redact-preview: dry run, nothing was written".into()
    } else {
        format!(
            "rich: --redact-preview: dry run, nothing was written (without it: {})",
            would_write.join(", ")
        )
    }
}

/// Show `previews` (with `--report json`, as one JSON document on stdout)
/// and the dry-run line, and exit 0: a preview is not the command's run.
pub(crate) fn emit(
    cli: &Cli,
    console: &Console,
    named: Vec<(&'static str, Preview)>,
    would_write: &[String],
) -> ExitCode {
    if cli.report_format == ReportFormat::Json {
        let document = json(&named, would_write);
        let text = serde_json::to_string_pretty(&document).unwrap_or_default();
        authoring::out(&format!("{text}\n"));
    } else {
        for (index, (_, preview)) in named.iter().enumerate() {
            if index > 0 {
                console.print_str("");
            }
            console.print(preview);
        }
        eprintln!("{}", dry_run(would_write));
    }
    ExitClass::Success.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lists_findings_from_one() {
        let redactor = Redactor::secrets();
        let previews = vec![
            (
                "command",
                redactor.preview_args(&["deploy", "--token", "abc"]),
            ),
            ("output", redactor.preview("ok\npassword=pw\n")),
        ];
        let doc = json(&previews, &["out.svg".to_string()]);
        let root = &doc["redact_preview"];
        assert_eq!(root["matches"], 2);
        assert_eq!(root["would_write"][0], "out.svg");
        let output = &root["sources"][1];
        assert_eq!(output["lines"], 2);
        assert_eq!(output["counts"]["key-value"], 1);
        let finding = &output["findings"][0];
        assert_eq!(
            (finding["line"].as_u64(), finding["column"].as_u64()),
            (Some(2), Some(10))
        );
        assert_eq!(finding["text"], "pw");
        assert_eq!(finding["replacement"], "********");
    }

    #[test]
    fn dry_run_names_what_would_be_written() {
        assert_eq!(
            dry_run(&[]),
            "rich: --redact-preview: dry run, nothing was written"
        );
        assert!(
            dry_run(&["a.cast".into(), "b.png".into()]).ends_with("(without it: a.cast, b.png)")
        );
    }
}
