//! Deterministic snapshots for downstream render regression tests.
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderSnapshot {
    pub schema_version: u32,
    pub width: usize,
    pub height: usize,
    pub plain: String,
    pub ansi: String,
    pub segments: Vec<SnapshotSegment>,
}
pub type SnapshotError = serde_json::Error;
impl RenderSnapshot {
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
        }
    }
    pub fn to_json(&self) -> Result<String, SnapshotError> {
        serde_json::to_string_pretty(self)
    }
    /// First changed line or metadata path, without hiding style-only changes.
    pub fn diff(&self, other: &Self) -> Option<String> {
        if self == other {
            return None;
        }
        if self.plain != other.plain {
            let a: Vec<_> = self.plain.split('\n').collect();
            let b: Vec<_> = other.plain.split('\n').collect();
            for i in 0..a.len().max(b.len()) {
                if a.get(i) != b.get(i) {
                    return Some(format!(
                        "line {}\n-{}\n+{}",
                        i + 1,
                        a.get(i).unwrap_or(&""),
                        b.get(i).unwrap_or(&"")
                    ));
                }
            }
        }
        if self.segments != other.segments {
            return first_difference(
                "segments",
                &serde_json::to_value(&self.segments).ok()?,
                &serde_json::to_value(&other.segments).ok()?,
            );
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
