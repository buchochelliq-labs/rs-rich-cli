//! Deterministic snapshots for downstream render regression tests.
pub mod conformance;

use crate::target::RenderTarget;
use rich::protocol::RenderEnvironment;
use rich::{Renderable, Segment};
use serde::{Deserialize, Serialize};

/// Owned segment data; attributes follow core SGR order, with `not ` for explicit off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotSegment {
    pub text: String,
    pub control: bool,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub attributes: Vec<String>,
    pub link: Option<String>,
}
/// A run of text in one style on one row, as schema 2 stores it: adjacent
/// segments that look the same are merged, so how the output was split into
/// segments does not show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRun {
    pub text: String,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub attributes: Vec<String>,
    pub link: Option<String>,
}
impl SnapshotRun {
    fn same_style(&self, other: &SnapshotRun) -> bool {
        (
            &self.foreground,
            &self.background,
            &self.attributes,
            &self.link,
        ) == (
            &other.foreground,
            &other.background,
            &other.attributes,
            &other.link,
        )
    }
}
/// A render captured for comparison.
///
/// Schema 1 ([`RenderSnapshot::capture`]) stores the segments as rendered.
/// Schema 2 ([`RenderSnapshot::capture_frame`]) stores `rows` of merged runs
/// instead, drops control segments, and keeps `ansi` in the merged encoding;
/// its `segments` is empty. [`RenderSnapshot::diff`] compares a schema 2
/// snapshot with either schema by what shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderSnapshot {
    pub schema_version: u32,
    pub width: usize,
    pub height: usize,
    pub plain: String,
    pub ansi: String,
    pub segments: Vec<SnapshotSegment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<SnapshotRun>>>,
}
pub type SnapshotError = serde_json::Error;
impl RenderSnapshot {
    /// A schema 2 snapshot, built from the render's [`Frame`](crate::frame::Frame).
    pub fn capture_frame(target: &RenderTarget, renderable: &dyn Renderable) -> Self {
        let segments = target.segments(renderable);
        let caps = target.capabilities();
        let frame = crate::frame::Frame::from_segments(&segments);
        let snapshot: Vec<SnapshotSegment> = segments.iter().map(snapshot_segment).collect();
        Self {
            schema_version: 2,
            width: caps.width,
            height: caps.height,
            plain: frame.plain(),
            ansi: frame.to_ansi_merged(&target.console()),
            segments: Vec::new(),
            rows: Some(rows(&snapshot)),
        }
    }
    /// This snapshot as schema 2: rows of merged runs from its segments. A
    /// schema 2 snapshot is returned unchanged. `ansi` is kept as captured.
    pub fn upgrade(&self) -> Self {
        if self.rows.is_some() {
            return self.clone();
        }
        Self {
            schema_version: 2,
            segments: Vec::new(),
            rows: Some(rows(&self.segments)),
            ..self.clone()
        }
    }
    pub fn capture(target: &RenderTarget, renderable: &dyn Renderable) -> Self {
        let segments = target.segments(renderable);
        let caps = target.capabilities();
        Self {
            schema_version: 1,
            width: caps.width,
            height: caps.height,
            plain: segments
                .iter()
                .filter(|s| !s.control)
                .map(|s| s.text.as_str())
                .collect(),
            ansi: target.console().segments_to_string(&segments),
            segments: segments.iter().map(snapshot_segment).collect(),
            rows: None,
        }
    }
    pub fn to_json(&self) -> Result<String, SnapshotError> {
        serde_json::to_string_pretty(self)
    }
    /// How `other` differs, or `None` when equal, without hiding style-only
    /// changes: a `diff -u` of the plain text when it differs, else the
    /// style-changed lines and the first differing segment field, else the
    /// first differing metadata path.
    ///
    /// When either snapshot is schema 2, both are compared as schema 2: size,
    /// plain text and rows, not `ansi` or how the text was segmented.
    pub fn diff(&self, other: &Self) -> Option<String> {
        if self.rows.is_some() || other.rows.is_some() {
            let (a, b) = (self.upgrade(), other.upgrade());
            if (a.width, a.height, &a.plain, &a.rows) == (b.width, b.height, &b.plain, &b.rows) {
                return None;
            }
            if a.plain != b.plain {
                return Some(
                    crate::diff::TextDiff::new(&a.plain, &b.plain).unified("self", "other"),
                );
            }
            let (a_value, b_value) = (
                serde_json::to_value(&a).ok()?,
                serde_json::to_value(&b).ok()?,
            );
            let mut out = String::new();
            let lines = crate::diff::DiffView::ansi(&a.ansi, &b.ansi).style_changed_lines();
            if !lines.is_empty() {
                let lines: Vec<String> = lines.iter().map(usize::to_string).collect();
                out.push_str(&format!("style changed on line {}\n", lines.join(", ")));
            }
            let field = first_difference("rows", &a_value["rows"], &b_value["rows"])
                .or_else(|| first_difference("snapshot", &a_value, &b_value));
            if let Some(field) = field {
                out.push_str(&field);
            }
            return Some(out);
        }
        if self == other {
            return None;
        }
        if self.plain != other.plain {
            return Some(
                crate::diff::TextDiff::new(&self.plain, &other.plain).unified("self", "other"),
            );
        }
        if self.segments != other.segments {
            let view = crate::diff::DiffView::ansi(&self.ansi, &other.ansi);
            let lines = view.style_changed_lines();
            let field = first_difference(
                "segments",
                &serde_json::to_value(&self.segments).ok()?,
                &serde_json::to_value(&other.segments).ok()?,
            );
            let mut out = String::new();
            if !lines.is_empty() {
                let lines: Vec<String> = lines.iter().map(usize::to_string).collect();
                out.push_str(&format!("style changed on line {}\n", lines.join(", ")));
            }
            if let Some(field) = field {
                out.push_str(&field);
            }
            return (!out.is_empty()).then_some(out);
        }
        let a = serde_json::to_value(self).ok()?;
        let b = serde_json::to_value(other).ok()?;
        first_difference("snapshot", &a, &b)
    }
}
fn first_difference(path: &str, a: &serde_json::Value, b: &serde_json::Value) -> Option<String> {
    if a == b {
        return None;
    }
    match (a, b) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (key, v) in a {
                if let Some(diff) = first_difference(
                    &format!("{path}.{key}"),
                    v,
                    b.get(key).unwrap_or(&serde_json::Value::Null),
                ) {
                    return Some(diff);
                }
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            for i in 0..a.len().max(b.len()) {
                if let Some(diff) = first_difference(
                    &format!("{path}[{i}]"),
                    a.get(i).unwrap_or(&serde_json::Value::Null),
                    b.get(i).unwrap_or(&serde_json::Value::Null),
                ) {
                    return Some(diff);
                }
            }
        }
        _ => {}
    }
    Some(format!("{path}: {a} -> {b}"))
}
/// Rows of merged runs from segments: control segments dropped, text split at
/// line breaks, empty runs dropped, and neighbours that look the same joined.
fn rows(segments: &[SnapshotSegment]) -> Vec<Vec<SnapshotRun>> {
    let mut rows: Vec<Vec<SnapshotRun>> = vec![Vec::new()];
    for segment in segments.iter().filter(|s| !s.control) {
        for (index, piece) in segment.text.split('\n').enumerate() {
            if index > 0 {
                rows.push(Vec::new());
            }
            if piece.is_empty() {
                continue;
            }
            let run = SnapshotRun {
                text: piece.to_owned(),
                foreground: segment.foreground.clone(),
                background: segment.background.clone(),
                attributes: segment.attributes.clone(),
                link: segment.link.clone(),
            };
            let row = rows.last_mut().expect("at least one row");
            match row.last_mut() {
                Some(last) if last.same_style(&run) => last.text.push_str(&run.text),
                _ => row.push(run),
            }
        }
    }
    rows
}
fn snapshot_segment(s: &Segment) -> SnapshotSegment {
    const ATTRS: [&str; 13] = [
        "bold",
        "dim",
        "italic",
        "underline",
        "blink",
        "blink2",
        "reverse",
        "conceal",
        "strike",
        "underline2",
        "frame",
        "encircle",
        "overline",
    ];
    let theme = rich::terminal_theme::DEFAULT_TERMINAL_THEME;
    SnapshotSegment {
        text: s.text.clone(),
        control: s.control,
        foreground: s.style.as_ref().and_then(|s| s.color()).map(|c| {
            theme
                .resolve(c, true)
                .hex()
                .trim_start_matches('#')
                .to_owned()
        }),
        background: s.style.as_ref().and_then(|s| s.bgcolor()).map(|c| {
            theme
                .resolve(c, false)
                .hex()
                .trim_start_matches('#')
                .to_owned()
        }),
        attributes: s
            .style
            .as_ref()
            .map(|s| {
                ATTRS
                    .iter()
                    .enumerate()
                    .filter_map(|(i, n)| {
                        s.attr(i)
                            .map(|v| if v { (*n).into() } else { format!("not {n}") })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        link: s.style.as_ref().and_then(|s| s.link()).map(str::to_owned),
    }
}
