//! Compile against the same optimized rich library used by the measured CLI.
use rich::{Console, Syntax};
use std::{env, fs, time::Instant};
fn main() {
    let args: Vec<_> = env::args().collect();
    let code = fs::read_to_string(&args[1]).expect("source");
    let console = Console::builder().width(100).no_color(true).build();
    let syntax = Syntax::new(code, "rust").word_wrap(true);
    let mut first = String::new();
    let mut times = Vec::new();
    for run in 0..6 {
        let start = Instant::now();
        let output = console.render_to_string(&syntax);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        if run == 0 { first = output; } else { assert_eq!(output, first); }
    }
    fs::write(&args[2], &first).expect("captured output");
    println!("{{\"first_render_ms\":{},\"warm_render_ms\":{:?},\"output_bytes\":{}}}", times[0], &times[1..], first.len());
}
