//! The host facilities that only a native build has: Ctrl+C handlers, and the
//! message for a feature a WASI build (the docs playground's `rich`, built for
//! `wasm32-wasip1`) does not have. A native build calls straight through.

/// Run `handler` on Ctrl+C. Under WASI there are no signals: the page that
/// runs the module stops it by terminating its worker, so this does nothing.
#[cfg(not(target_os = "wasi"))]
pub(crate) fn on_interrupt<F>(handler: F) -> Result<(), ctrlc::Error>
where
    F: FnMut() + Send + 'static,
{
    ctrlc::set_handler(handler)
}

/// Run `handler` on Ctrl+C. Under WASI there are no signals: the page that
/// runs the module stops it by terminating its worker, so this does nothing.
#[cfg(target_os = "wasi")]
pub(crate) fn on_interrupt<F>(handler: F) -> Result<(), std::convert::Infallible>
where
    F: FnMut() + Send + 'static,
{
    drop(handler);
    Ok(())
}

/// The message for `feature`, which needs the host (processes, signals,
/// file watching or the capability file system) and so is missing under WASI.
/// Callers test `cfg!(target_os = "wasi")`, so a native build never calls it.
#[cfg_attr(not(target_os = "wasi"), allow(dead_code))]
pub(crate) fn unavailable(feature: &str) -> String {
    format!("{feature} is not available in the browser (this `rich` is WebAssembly, on WASI)")
}
