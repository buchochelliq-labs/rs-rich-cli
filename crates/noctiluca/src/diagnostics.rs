//! Opt-in measurement events. Disabled runs pay one cached boolean check;
//! enabled runs emit counts/timings, never resource URLs or page contents.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RICH_MEDIA_TRACE").is_some())
}
