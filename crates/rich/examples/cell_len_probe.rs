//! Width probe: reads one string per line, prints its measured cell width.
//!
//! Used by `scripts/`-adjacent parity sweeps to compare `cells::cell_len`
//! against upstream `rich.cells.cell_len` without going through a renderer.

fn main() {
    use std::io::{self, BufRead, Write};
    let out = io::stdout();
    let mut out = out.lock();
    for line in io::stdin().lock().lines() {
        let line = line.expect("read line");
        writeln!(out, "{}", rich::cells::cell_len(&line)).expect("write");
    }
}
