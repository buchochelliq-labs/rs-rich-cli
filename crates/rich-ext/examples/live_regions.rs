use rich::protocol::{Support, TargetCapabilities};
use rich::{Segment, Theme};
use rich_ext::{
    live::LiveCoordinator,
    target::{RenderTarget, TargetKind},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let script = std::env::args().any(|arg| arg == "--script");
    let target = RenderTarget::new(
        TargetKind::Terminal,
        TargetCapabilities {
            width: if script { 80 } else { 20 },
            height: if script { 24 } else { 6 },
            color_system: None,
            interactive: true,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    );
    let mut live = LiveCoordinator::new(std::io::stdout(), target);
    let a = live.add(vec![Segment::new("Build: pending", None)])?;
    live.add(vec![Segment::new("Tests: pending", None)])?;
    live.refresh()?;
    if script {
        use std::io::BufRead;
        eprintln!("ready");
        for line in std::io::stdin().lock().lines() {
            let line = line?;
            if line == "quit" {
                break;
            }
            if let Some(size) = line.strip_prefix("resize ") {
                let dimensions = size
                    .split_whitespace()
                    .map(str::parse::<usize>)
                    .collect::<Result<Vec<_>, _>>()?;
                if dimensions.len() != 2 {
                    return Err("resize requires width height".into());
                }
                live.resize(dimensions[0], dimensions[1])?;
            } else if let Some(message) = line.strip_prefix("log ") {
                live.print(&[Segment::new(message, None)])?;
            }
            eprintln!("ready");
        }
        live.finish()?;
        return Ok(());
    }

    live.print(&[Segment::new("Started build", None)])?;
    live.update(a, vec![Segment::new("Build: complete", None)])?;
    live.refresh()?;
    live.finish()?;
    println!("Finished");
    Ok(())
}
