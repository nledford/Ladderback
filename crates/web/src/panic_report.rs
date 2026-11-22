//! What the web UI writes to the browser console when it panics (#1411).

/// Replaces the panic hook.
pub(crate) fn install() {
    console_error_panic_hook::set_once();
}

#[cfg(test)]
#[cfg(target_arch = "wasm32")]
mod tests;
