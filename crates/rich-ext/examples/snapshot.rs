use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Text, Theme};
use rich_ext::{
    target::{RenderTarget, TargetKind},
    testing::RenderSnapshot,
};
fn main() {
    let target = RenderTarget::new(
        TargetKind::Capture,
        TargetCapabilities {
            width: 40,
            height: 10,
            color_system: Some(ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    );
    println!(
        "{}",
        RenderSnapshot::capture(&target, &Text::styled("Stable snapshot", "bold cyan"))
            .to_json()
            .unwrap()
    );
}
