//! Where the network/sync/backup layers send their progress and state-change
//! events: the real Tauri emitter in the app, or (in tests) a sink that just
//! records them. This lets the real host/client/sync/backup code paths run
//! against a plain `Arc<Mutex<AppState>>` without a live Tauri `AppHandle` —
//! `tauri::AppHandle`/`tauri::State` can't be constructed outside a running
//! Tauri `App` (the `test` feature's `MockRuntime` is a different `Runtime`
//! type from the app's real `Wry`, so it can't stand in for `tauri::AppHandle`
//! either), which previously made this logic untestable end-to-end.
use std::sync::Arc;

pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// Shared handle to wherever events go. Cloning is cheap (`Arc`), matching the
/// existing `app.clone()` pattern used to move an `AppHandle` into spawned tasks.
pub type Events = Arc<dyn EventSink>;

impl EventSink for tauri::AppHandle {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        let _ = tauri::Emitter::emit(self, event, payload);
    }
}

/// Wrap a real `AppHandle` (from a `#[tauri::command]`) for the lower layers.
pub fn from_app(app: &tauri::AppHandle) -> Events {
    Arc::new(app.clone())
}

/// Discards every event. Used by tests that don't assert on progress/state events.
#[cfg(test)]
pub struct NullSink;

#[cfg(test)]
impl EventSink for NullSink {
    fn emit(&self, _event: &str, _payload: serde_json::Value) {}
}
