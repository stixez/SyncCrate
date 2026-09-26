//! The main window's size and position, kept between launches (it always
//! opened at 1100×700 in the middle of the screen). Written on close and on
//! quit; a maximized window keeps the size it had before maximizing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct WindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    #[serde(default)]
    maximized: bool,
}

/// tauri.conf.json's minWidth / minHeight.
const MIN_SIZE: (u32, u32) = (900, 600);

fn path() -> std::path::PathBuf {
    crate::utils::config_root().join("synccrate").join("window_state.json")
}

/// Whether the window's title bar lands on one of the monitors (x, y, width,
/// height): a monitor unplugged since would otherwise open it off-screen.
fn on_screen(s: &WindowState, monitors: &[(i32, i32, u32, u32)]) -> bool {
    let (px, py) = (s.x + 60, s.y + 12);
    monitors.iter().any(|&(x, y, w, h)| px >= x && py >= y && px < x + w as i32 && py < y + h as i32)
}

pub fn save(window: &tauri::WebviewWindow) {
    // A minimized window reports a far-off position on Windows (-32000).
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let previous: Option<WindowState> = crate::utils::read_json_strict(&path()).ok().flatten();
    let state = if window.is_maximized().unwrap_or(false) {
        match previous {
            Some(p) => WindowState { maximized: true, ..p },
            None => return,
        }
    } else {
        let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) else { return };
        WindowState { x: pos.x, y: pos.y, width: size.width, height: size.height, maximized: false }
    };
    if previous != Some(state) {
        if let Err(e) = crate::utils::write_json_atomic(&path(), &state) {
            log::warn!("{e}");
        }
    }
}

pub fn restore(window: &tauri::WebviewWindow) {
    let Some(s) = crate::utils::read_json_strict::<WindowState>(&path()).ok().flatten() else { return };
    if s.width >= MIN_SIZE.0 && s.height >= MIN_SIZE.1 {
        let _ = window.set_size(tauri::PhysicalSize::new(s.width, s.height));
    }
    let monitors: Vec<(i32, i32, u32, u32)> = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| (m.position().x, m.position().y, m.size().width, m.size().height))
        .collect();
    if on_screen(&s, &monitors) {
        let _ = window.set_position(tauri::PhysicalPosition::new(s.x, s.y));
    }
    if s.maximized {
        let _ = window.maximize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_on_an_unplugged_monitor_isnt_put_back_there() {
        let at = |x, y| WindowState { x, y, width: 1100, height: 700, maximized: false };
        let screens = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        assert!(on_screen(&at(100, 100), &screens));
        assert!(on_screen(&at(2500, 200), &screens), "second monitor");
        assert!(!on_screen(&at(5000, 200), &screens), "right of every monitor");
        assert!(!on_screen(&at(100, -200), &screens), "title bar above the screen");
        assert!(!on_screen(&at(100, 100), &[]));
    }
}
