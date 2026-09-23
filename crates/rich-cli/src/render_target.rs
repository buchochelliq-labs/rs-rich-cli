//! CLI boundary: observe the selected console once, then attach immutable policy.
use rich::protocol::{ConsoleEnvironment, Support};
use rich::Console;
use rich_ext::target::{
    resolve_capabilities, DetectedCapabilities, RenderTarget, TargetKind, TargetObservations,
    TargetOverrides,
};
use std::sync::Arc;

pub(super) fn observe(console: &Console, overrides: TargetOverrides) -> DetectedCapabilities {
    #[cfg(feature = "art")]
    let sixel_hint = if console.is_terminal() && rich_art::sixel::is_probably_supported() {
        Support::Inferred
    } else {
        Support::Unsupported
    };
    #[cfg(not(feature = "art"))]
    let sixel_hint = Support::Unsupported;
    let mut resolved = resolve_capabilities(
        TargetObservations {
            width: Some(console.width()),
            height: Some(console.height()),
            is_terminal: console.is_terminal(),
            color_system: console.color_system(),
            unicode: !console.ascii_only(),
            hyperlinks: console.is_terminal(),
            sixel_hint,
        },
        overrides,
    );
    // Console stores resolved dimensions, not their origins. Reconstruct only
    // provenance here; rendering continues to use the immutable console values.
    let terminal = terminal_size::terminal_size();
    for (name, configured, environment) in [
        ("width", overrides.width, "COLUMNS"),
        ("height", overrides.height, "LINES"),
    ] {
        let environment_size = std::env::var(environment)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0);
        if configured.is_none() && environment_size.is_none() && terminal.is_none() {
            if let Some((_, origin)) = resolved.origins.iter_mut().find(|(key, _)| key == name) {
                *origin = rich_ext::target::CapabilityOrigin::Default;
            }
        }
    }
    resolved
}
pub(super) fn attach(console: &mut Console) {
    let caps = observe(console, TargetOverrides::default()).capabilities;
    let kind = if caps.interactive {
        TargetKind::Terminal
    } else {
        TargetKind::Capture
    };
    let target = RenderTarget::new(kind, caps, console.theme().clone());
    console.set_render_environment(Some(Arc::new(target)));
}
