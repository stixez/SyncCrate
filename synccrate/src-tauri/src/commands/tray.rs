//! System tray status line + "Stop hosting / Disconnect" item, and the
//! close-to-tray setting.
//!
//! The tray is refreshed by a 1 s background task that reads `AppState` (cheap,
//! short lock) plus the latest sync percent, captured by backend listeners on
//! `sync-progress` / `sync-complete`. Polling keeps it correct for every state
//! change (host start/stop, peers joining, drops) without hooking each code path.

use crate::state::{AppState, SessionType};
use std::sync::atomic::{AtomicBool, AtomicI16, Ordering};
use std::sync::Arc;
use tauri::menu::MenuItem;
use tauri::tray::TrayIcon;
use tauri::{Listener, Manager, Wry};
use tokio::sync::Mutex;

/// Latest client sync percent (0-100), or -1 when not syncing.
static SYNC_PERCENT: AtomicI16 = AtomicI16::new(-1);
/// Set by the tray "Quit" item so the close handler never hides instead of quitting.
pub static QUITTING: AtomicBool = AtomicBool::new(false);

pub struct TrayHandles {
    pub tray: TrayIcon<Wry>,
    pub status: MenuItem<Wry>,
    pub leave: MenuItem<Wry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrayStatus {
    /// Status line without the app name (menu item text).
    pub line: String,
    /// Label for the leave item ("Stop hosting" / "Disconnect").
    pub leave_label: &'static str,
    pub in_session: bool,
}

impl TrayStatus {
    pub fn tooltip(&self) -> String {
        format!("SyncCrate \u{2014} {}", self.line)
    }
}

/// Pure formatter for the tray status (unit-tested).
pub fn tray_status(
    session_type: &SessionType,
    peer_names: &[String],
    sync_percent: Option<u8>,
) -> TrayStatus {
    match session_type {
        SessionType::None => TrayStatus {
            line: "Idle".to_string(),
            leave_label: "Disconnect",
            in_session: false,
        },
        SessionType::Host => {
            let n = peer_names.len();
            let line = match n {
                0 => "Hosting \u{b7} waiting for friends".to_string(),
                1 => "Hosting \u{b7} 1 friend".to_string(),
                _ => format!("Hosting \u{b7} {} friends", n),
            };
            TrayStatus { line, leave_label: "Stop hosting", in_session: true }
        }
        SessionType::Client => {
            let line = match (sync_percent, peer_names.first()) {
                (Some(p), _) => format!("Syncing {}%", p.min(100)),
                (None, Some(host)) => format!("Connected to {}", host),
                (None, None) => "Connecting\u{2026}".to_string(),
            };
            TrayStatus { line, leave_label: "Disconnect", in_session: true }
        }
    }
}

fn percent_from_progress(payload: &str) -> Option<i16> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    let sent = v.get("bytes_sent")?.as_f64()?;
    let total = v.get("bytes_total")?.as_f64()?;
    if total <= 0.0 {
        return Some(0);
    }
    Some(((sent / total) * 100.0).clamp(0.0, 100.0) as i16)
}

/// Start the listeners + refresh loop. Call once from `setup` after managing
/// `TrayHandles`.
pub fn start_tray_status_updates(app: &tauri::AppHandle, state: Arc<Mutex<AppState>>) {
    app.listen_any("sync-progress", |event| {
        if let Some(p) = percent_from_progress(event.payload()) {
            SYNC_PERCENT.store(p, Ordering::Relaxed);
        }
    });
    for ev in ["sync-complete", "peer-disconnected", "connection-failed"] {
        app.listen_any(ev, |_| SYNC_PERCENT.store(-1, Ordering::Relaxed));
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last: Option<TrayStatus> = None;
        loop {
            let status = {
                let s = state.lock().await;
                let syncing = s.is_any_syncing();
                if !syncing {
                    SYNC_PERCENT.store(-1, Ordering::Relaxed);
                }
                let pct = SYNC_PERCENT.load(Ordering::Relaxed);
                let names: Vec<String> = s.connections.values().map(|c| c.info.name.clone()).collect();
                tray_status(
                    &s.session_type,
                    &names,
                    if syncing && pct >= 0 { Some(pct as u8) } else { None },
                )
            };
            if last.as_ref() != Some(&status) {
                if let Some(h) = app.try_state::<TrayHandles>() {
                    let _ = h.tray.set_tooltip(Some(status.tooltip()));
                    let _ = h.status.set_text(&status.line);
                    let _ = h.leave.set_text(status.leave_label);
                    let _ = h.leave.set_enabled(status.in_session);
                }
                last = Some(status);
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
}

/// Tray "Stop hosting / Disconnect": same as the `disconnect` command (which
/// emits `peer-disconnected` {name: "all"} for the frontend).
pub fn leave_session_from_tray(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Arc<Mutex<AppState>>>().inner().clone();
        crate::commands::session::disconnect_inner(&state, &app).await;
    });
}

#[tauri::command]
pub async fn get_close_to_tray() -> Result<bool, String> {
    Ok(crate::commands::sync::read_sync_config().close_to_tray)
}

#[tauri::command]
pub async fn set_close_to_tray(enabled: bool) -> Result<(), String> {
    let mut config = crate::commands::sync::read_sync_config();
    config.close_to_tray = enabled;
    let path = crate::utils::sync_config_path();
    let data = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn idle_status() {
        let s = tray_status(&SessionType::None, &[], None);
        assert_eq!(s.tooltip(), "SyncCrate \u{2014} Idle");
        assert!(!s.in_session);
    }

    #[test]
    fn hosting_status_pluralizes() {
        assert_eq!(tray_status(&SessionType::Host, &[], None).line, "Hosting \u{b7} waiting for friends");
        assert_eq!(tray_status(&SessionType::Host, &names(&["A"]), None).line, "Hosting \u{b7} 1 friend");
        let s = tray_status(&SessionType::Host, &names(&["A", "B"]), None);
        assert_eq!(s.line, "Hosting \u{b7} 2 friends");
        assert_eq!(s.leave_label, "Stop hosting");
        assert!(s.in_session);
    }

    #[test]
    fn client_status() {
        assert_eq!(tray_status(&SessionType::Client, &names(&["Alex"]), None).line, "Connected to Alex");
        assert_eq!(tray_status(&SessionType::Client, &names(&["Alex"]), Some(40)).line, "Syncing 40%");
        assert_eq!(tray_status(&SessionType::Client, &[], None).line, "Connecting\u{2026}");
        assert_eq!(tray_status(&SessionType::Client, &[], None).leave_label, "Disconnect");
    }

    #[test]
    fn progress_payload_percent() {
        assert_eq!(percent_from_progress(r#"{"bytes_sent":40,"bytes_total":100}"#), Some(40));
        assert_eq!(percent_from_progress(r#"{"bytes_sent":0,"bytes_total":0}"#), Some(0));
        assert_eq!(percent_from_progress("not json"), None);
    }
}
