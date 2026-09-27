#[test]
fn defaults() {
    assert_eq!(orbit::scheduler::DEFAULT_WORKERS, 4);
    assert_eq!(orbit::scheduler::RETRIES, 3);
}
