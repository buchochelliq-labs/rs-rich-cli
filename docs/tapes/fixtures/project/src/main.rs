//! orbit: watch a queue, run its jobs, report progress.

mod event_loop;
mod render;

fn main() {
    let workers = std::env::args()
        .skip_while(|arg| arg != "--workers")
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(4);
    event_loop::run(workers);
}
