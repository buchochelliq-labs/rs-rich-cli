use std::time::Duration;

pub fn retry_policy() -> (u32, Duration) {
<<<<<<< HEAD
    let attempts = 5;
    let delay = ms(250);
||||||| base
    let attempts = 3;
    let delay = ms(100);
=======
    let attempts = 3;
    let delay = secs(1);
>>>>>>> feature/backoff
    (attempts, delay)
}

pub fn label() -> &'static str {
<<<<<<< HEAD
    "retry"
=======
    "retry with backoff"
>>>>>>> feature/backoff
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}
