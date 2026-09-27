//! The event loop: take jobs, hand them to workers, collect results.

use std::time::Duration;

/// One job from the queue.
pub struct Job {
    pub name: String,
    pub tries: u32,
}

/// Run `workers` jobs at once until the queue is empty.
pub fn run(workers: usize) {
    let mut queue: Vec<Job> = Vec::new();
    while !queue.is_empty() {
        let batch: Vec<Job> = queue.drain(..workers.min(queue.len())).collect();
        for mut job in batch {
            job.tries += 1;
            if job.tries < 3 {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
