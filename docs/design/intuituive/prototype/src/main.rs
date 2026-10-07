//! The intuiTUIve spike: see `../../intuituive.md`.
//!
//! `cargo run --release` prints the dashboard benchmark (`bench`) and the
//! interop measurements (`interop::bench`).

mod bench;
mod dashboard;
mod interop;
mod paint;
mod reactive;
mod tree;

fn main() {
    bench::run();
    bench::breakdown(80, 24, 500);
    bench::breakdown(200, 60, 500);
    interop::bench();
}
