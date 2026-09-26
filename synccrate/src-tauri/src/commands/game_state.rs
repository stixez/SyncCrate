use crate::state::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;

/// How "Play" starts a game: through Steam when Steam has it installed, or
/// the game's own exe where SyncCrate knows it (The Sims 4 on the EA app,
/// which the exe hands over to).
#[derive(Debug, PartialEq)]
enum Launch {
    SteamUrl(String),
    Exe(std::path::PathBuf),
}

fn launch_for(game_id: &str, steam_app_id: Option<u32>, steam_ids: &std::collections::HashSet<u32>, sims4_dir: Option<std::path::PathBuf>) -> Option<Launch> {
    if let Some(id) = steam_app_id.filter(|id| steam_ids.contains(id)) {
        return Some(Launch::SteamUrl(format!("steam://rungameid/{id}")));
    }
    // sims4, sims4-reshade and sims4-gshade are all the same game.
    if game_id.starts_with("sims4") {
        let exe = sims4_dir?.join("Game").join("Bin").join("TS4_x64.exe");
        return exe.is_file().then_some(Launch::Exe(exe));
    }
    None
}

async fn find_launch(state: &Arc<Mutex<AppState>>, game_id: &str) -> Result<Option<Launch>, String> {
    let (id, steam_app_id) = {
        let st = state.lock().await;
        let id = crate::commands::files::resolve_game(&st, game_id)?;
        let app = st.game_registry.games.iter().find(|g| g.id == id).and_then(|g| g.steam_app_id);
        (id, app)
    };
    tokio::task::spawn_blocking(move || {
        let steam_ids = crate::game_install::steam_installed_app_ids(&crate::utils::steam_steamapps_dirs());
        let sims4 = id.starts_with("sims4").then(crate::packs::sims4_install_dir).flatten();
        launch_for(&id, steam_app_id, &steam_ids, sims4)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Whether "Play" can start this game.
#[tauri::command]
pub async fn can_launch_game(state: tauri::State<'_, Arc<Mutex<AppState>>>, game_id: String) -> Result<bool, String> {
    Ok(find_launch(state.inner(), &game_id).await?.is_some())
}

#[tauri::command]
pub async fn launch_game(state: tauri::State<'_, Arc<Mutex<AppState>>>, game_id: String) -> Result<(), String> {
    let launch = find_launch(state.inner(), &game_id).await?.ok_or("Start this game from its launcher.")?;
    let mut cmd = match &launch {
        Launch::Exe(exe) => {
            let mut c = std::process::Command::new(exe);
            if let Some(dir) = exe.parent() {
                c.current_dir(dir);
            }
            c
        }
        Launch::SteamUrl(url) => {
            #[cfg(target_os = "windows")]
            let mut c = std::process::Command::new(crate::utils::windows_system_exe("explorer.exe"));
            #[cfg(target_os = "macos")]
            let mut c = std::process::Command::new("open");
            #[cfg(target_os = "linux")]
            let mut c = std::process::Command::new("xdg-open");
            c.arg(url);
            c
        }
    };
    cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't start the game: {e}"))
}

/// Whether one of the game's executables (registry `process_names`) is running.
/// Used to warn before syncing so files aren't locked or half-loaded.
#[tauri::command]
pub async fn check_game_running(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<bool, String> {
    let process_names = {
        let app_state = state.lock().await;
        let game_id = game.unwrap_or_else(|| app_state.active_game.clone());
        app_state
            .game_registry
            .games
            .iter()
            .find(|g| g.id == game_id)
            .map(|g| g.process_names.clone())
            .unwrap_or_default()
    };
    is_game_running(&process_names).await
}

/// Whether any of the given executables is running (false when none are known).
pub(crate) async fn is_game_running(process_names: &[String]) -> Result<bool, String> {
    // Tests must not depend on what's open on the dev machine (real case:
    // every undo/apply E2E test failed while The Sims 4 was running).
    if process_names.is_empty() || cfg!(test) {
        return Ok(false);
    }
    let running = tokio::task::spawn_blocking(list_process_names)
        .await
        .map_err(|e| e.to_string())??;
    Ok(any_process_matches(process_names, &running))
}

/// Case-insensitive match of wanted executable names against running process
/// names. Running names may be full paths (macOS `ps`), so compare basenames.
pub(crate) fn any_process_matches(wanted: &[String], running: &[String]) -> bool {
    running.iter().any(|r| {
        let base = r.rsplit(['/', '\\']).next().unwrap_or(r).trim();
        wanted.iter().any(|w| w.trim().eq_ignore_ascii_case(base))
    })
}

#[cfg(windows)]
fn list_process_names() -> Result<Vec<String>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new(crate::utils::windows_system_exe("tasklist.exe"))
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to list processes: {}", e))?;
    Ok(parse_tasklist_csv(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(not(windows))]
fn list_process_names() -> Result<Vec<String>, String> {
    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "comm="])
        .output()
        .map_err(|e| format!("Failed to list processes: {}", e))?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// First column of `tasklist /FO CSV /NH` output: `"TS4_x64.exe","1234",...`
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_tasklist_csv(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix('"')?;
            rest.split('"').next().map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_uses_steam_when_steam_has_it_else_the_sims_4_exe() {
        let steam: std::collections::HashSet<u32> = [1222670].into_iter().collect();
        assert_eq!(launch_for("sims4", Some(1222670), &steam, None), Some(Launch::SteamUrl("steam://rungameid/1222670".into())));
        assert_eq!(launch_for("valheim", Some(892970), &steam, None), None, "not installed through Steam");
        let dir = crate::testutil::temp_dir("launch");
        std::fs::create_dir_all(dir.join("Game/Bin")).unwrap();
        std::fs::write(dir.join("Game/Bin/TS4_x64.exe"), b"").unwrap();
        let none = std::collections::HashSet::new();
        assert_eq!(launch_for("sims4", Some(1222670), &none, Some(dir.clone())), Some(Launch::Exe(dir.join("Game").join("Bin").join("TS4_x64.exe"))));
        assert_eq!(launch_for("sims4", None, &none, Some(dir.join("missing"))), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_process_match_case_insensitive() {
        let wanted = names(&["TS4_x64.exe", "TS4.exe"]);
        assert!(any_process_matches(&wanted, &names(&["explorer.exe", "ts4_x64.EXE"])));
        assert!(!any_process_matches(&wanted, &names(&["explorer.exe", "TS4_x64.exe.bak"])));
    }

    #[test]
    fn test_process_match_basename_of_path() {
        let wanted = names(&["Terraria"]);
        assert!(any_process_matches(
            &wanted,
            &names(&["/Applications/Terraria.app/Contents/MacOS/Terraria"])
        ));
        assert!(any_process_matches(&names(&["game.exe"]), &names(&["C:\\Games\\GAME.exe"])));
    }

    #[test]
    fn test_process_match_empty() {
        assert!(!any_process_matches(&[], &names(&["TS4.exe"])));
        assert!(!any_process_matches(&names(&["TS4.exe"]), &[]));
    }

    #[test]
    fn test_parse_tasklist_csv() {
        let out = "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\r\n\"TS4_x64.exe\",\"4242\",\"Console\",\"1\",\"2,000,000 K\"\r\n";
        assert_eq!(parse_tasklist_csv(out), names(&["System Idle Process", "TS4_x64.exe"]));
    }
}
