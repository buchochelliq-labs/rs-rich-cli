//! Rendering the status page.

/// One worker's line: its number, job and time.
pub fn worker_line(number: usize, job: &str, seconds: u64) -> String {
    let slow = if seconds > 60 { " (slow)" } else { "" };
    format!("worker {number}: {job} for {seconds}s{slow}")
}
