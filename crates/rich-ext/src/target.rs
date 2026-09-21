//! Explicit destinations for deterministic, nested rendering. No environment probes.
use rich::protocol::{ConsoleEnvironment, RenderEnvironment, Support, TargetCapabilities};
use rich::{Console, Renderable, Segment, Theme};
use std::sync::Arc;

/// Output policy, independent of any writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Terminal,
    PlainStream,
    Capture,
    Html,
    Svg,
    Custom,
}

/// A complete immutable rendering context. Construct one per destination.
#[derive(Clone, Debug)]
pub struct RenderTarget {
    kind: TargetKind,
    capabilities: TargetCapabilities,
    theme: Theme,
}
impl RenderTarget {
    pub fn new(kind: TargetKind, mut caps: TargetCapabilities, theme: Theme) -> Self {
        if !matches!(kind, TargetKind::Terminal | TargetKind::Custom) {
            caps.interactive = false;
            caps.sixel = Support::Unsupported;
        }
        if kind == TargetKind::PlainStream {
            caps.color_system = None;
            caps.hyperlinks = false;
        }
        if !caps.interactive {
            caps.sixel = Support::Unsupported;
        }
        Self {
            kind,
            capabilities: caps,
            theme,
        }
    }
    pub fn kind(&self) -> TargetKind {
        self.kind
    }
    pub fn console(&self) -> Console {
        let c = self.capabilities;
        let mut console = Console::builder()
            .width(c.width)
            .height(c.height)
            .force_terminal(c.interactive)
            .color_system(c.color_system)
            .no_color(c.color_system.is_none())
            .ascii_only(!c.unicode)
            .legacy_windows(false)
            .safe_box(true)
            .emoji(c.unicode)
            .highlight(false)
            .theme(self.theme.clone())
            .build();
        console.set_render_environment(Some(Arc::new(self.clone())));
        console
    }
    pub fn segments(&self, value: &dyn Renderable) -> Vec<Segment> {
        let c = self.capabilities;
        if c.width == 0 || c.height == 0 {
            return Vec::new();
        }
        let console = self.console();
        let mut segments = value.rich_render(
            &console,
            &console.options().update_dimensions(c.width, c.height),
        );
        segments.retain(|segment| !segment.control || c.interactive);
        if !c.hyperlinks {
            for segment in &mut segments {
                segment.style = segment.style.as_ref().map(|s| s.update_link(None));
            }
        }
        segments
    }
    pub fn text(&self, value: &dyn Renderable) -> String {
        self.console().segments_to_string(&self.segments(value))
    }
}
impl RenderEnvironment for RenderTarget {
    fn capabilities(&self) -> TargetCapabilities {
        self.capabilities
    }
}

/// Provenance of each resolved capability, without active terminal probing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityOrigin {
    Configured,
    Detected,
    Inferred,
    Default,
}
#[derive(Clone, Debug)]
pub struct DetectedCapabilities {
    pub capabilities: TargetCapabilities,
    pub origins: Vec<(String, CapabilityOrigin)>,
}
#[derive(Clone, Copy, Debug)]
pub struct TargetObservations {
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub is_terminal: bool,
    pub color_system: Option<rich::ColorSystem>,
    pub unicode: bool,
    pub hyperlinks: bool,
    pub sixel_hint: Support,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct TargetOverrides {
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub interactive: Option<bool>,
    pub color_system: Option<Option<rich::ColorSystem>>,
    pub unicode: Option<bool>,
    pub hyperlinks: Option<bool>,
    pub sixel: Option<Support>,
}
/// Resolve caller-supplied observations and overrides. Never reads environment state.
pub fn resolve_capabilities(
    o: TargetObservations,
    overrides: TargetOverrides,
) -> DetectedCapabilities {
    let mut origins = Vec::new();
    fn select<T>(
        name: &str,
        configured: Option<T>,
        observed: T,
        origin: CapabilityOrigin,
        origins: &mut Vec<(String, CapabilityOrigin)>,
    ) -> T {
        match configured {
            Some(v) => {
                origins.push((name.into(), CapabilityOrigin::Configured));
                v
            }
            None => {
                origins.push((name.into(), origin));
                observed
            }
        }
    }
    use CapabilityOrigin::{Default, Detected, Inferred};
    let capabilities = TargetCapabilities {
        width: select(
            "width",
            overrides.width,
            o.width.unwrap_or(80),
            if o.width.is_some() { Detected } else { Default },
            &mut origins,
        ),
        height: select(
            "height",
            overrides.height,
            o.height.unwrap_or(25),
            if o.height.is_some() {
                Detected
            } else {
                Default
            },
            &mut origins,
        ),
        color_system: select(
            "color_system",
            overrides.color_system,
            o.color_system,
            Detected,
            &mut origins,
        ),
        interactive: select(
            "interactive",
            overrides.interactive,
            o.is_terminal,
            Detected,
            &mut origins,
        ),
        unicode: select(
            "unicode",
            overrides.unicode,
            o.unicode,
            Detected,
            &mut origins,
        ),
        hyperlinks: select(
            "hyperlinks",
            overrides.hyperlinks,
            o.hyperlinks,
            Detected,
            &mut origins,
        ),
        sixel: select(
            "sixel",
            overrides.sixel,
            o.sixel_hint,
            if o.sixel_hint == Support::Inferred {
                Inferred
            } else {
                Detected
            },
            &mut origins,
        ),
    };
    DetectedCapabilities {
        capabilities,
        origins,
    }
}
