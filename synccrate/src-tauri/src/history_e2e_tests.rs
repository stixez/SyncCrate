//! End-to-end: a real sync replaces a file, its previous version shows up
//! in the file history, and restoring it puts the old bytes back. The
//! history index is per game and shared by all E2E tests (one redirected
//! config dir), so these tests only look at their own uniquely named files.

use crate::commands::history::{self, FileVersion};
use crate::state::{AppState, Resolution};
use crate::testutil::*;
use std::sync::Arc;
use tokio::sync::Mutex;

async fn versions(state: &Arc<Mutex<AppState>>, path: &str) -> Vec<FileVersion> {
    let game = state.lock().await.active_game.clone();
    let root = crate::utils::backups_dir();
    // The command's inner logic, without a tauri::State.
    tokio::task::spawn_blocking({
        let path = path.to_string();
        move || {
            let raw = std::fs::read(root.join("history").join(format!("{game}.json"))).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_slice(&raw).unwrap_or(serde_json::json!({"entries": []}));
            let mut out: Vec<FileVersion> = serde_json::from_value(v["entries"].clone()).unwrap_or_default();
            out.reverse();
            out.retain(|e| e.path == path && e.pending.is_none());
            out.sort_by(|a, b| b.at.cmp(&a.at));
            out
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn replaced_file_is_kept_and_can_be_restored_tcp() {
    let _g = e2e_guard().await;
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let replaced = format!("Mods/hist-{tag}.package");
    let kept = format!("Mods/hist-keep-{tag}.package");
    let host_dir = temp_dir("hist-host");
    let client_dir = temp_dir("hist-client");
    write_file(&host_dir, &replaced, b"HOST_V2");
    write_file(&client_dir, &replaced, b"CLIENT_V1");
    write_file(&host_dir, &kept, b"HOST_K");
    write_file(&client_dir, &kept, b"CLIENT_K");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Alex").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    compute_plan(&client_state).await.expect("plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, replaced.clone(), Resolution::UseTheirs, None).await.expect("resolve");
    crate::commands::sync::resolve_conflict_inner(&client_state, kept.clone(), Resolution::KeepMine, None).await.expect("resolve");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, &replaced), b"HOST_V2");

    let v = versions(&client_state, &replaced).await;
    assert_eq!(v.len(), 1, "one previous version");
    assert_eq!(v[0].reason, history::REASON_REPLACED);
    assert_eq!(v[0].peer, "Alex");
    assert_eq!(v[0].hash, sha256_hex(b"CLIENT_V1"));
    assert!(versions(&client_state, &kept).await.is_empty(), "a file the sync didn't change has no version");

    // Roll back one file, with the session still connected.
    history::restore_file_version_inner(&client_state, "sims4", &v[0].id).await.expect("restore");
    assert_eq!(read_file(&client_dir, &replaced), b"CLIENT_V1");
    let v = versions(&client_state, &replaced).await;
    assert_eq!(v[0].reason, history::REASON_BEFORE_RESTORE, "the synced version is kept too");
    assert_eq!(v[0].hash, sha256_hex(b"HOST_V2"));

    // Restoring is refused while a sync is running.
    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = true;
    assert!(history::restore_file_version_inner(&client_state, "sims4", &v[0].id).await.unwrap_err().contains("sync"));
    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = false;

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn history_off_keeps_nothing_tcp() {
    let _g = e2e_guard().await;
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let path = format!("Mods/hist-off-{tag}.package");
    let host_dir = temp_dir("hist-off-host");
    let client_dir = temp_dir("hist-off-client");
    write_file(&host_dir, &path, b"NEW");
    write_file(&client_dir, &path, b"OLD");

    crate::commands::sync::set_keep_file_history(false).await.unwrap();

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    compute_plan(&client_state).await.expect("plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, path.clone(), Resolution::UseTheirs, None).await.expect("resolve");
    let result = run_sync_now(&client_state).await;

    crate::commands::sync::set_keep_file_history(true).await.unwrap();
    result.expect("sync");
    assert!(versions(&client_state, &path).await.is_empty());

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// With "back up before sync" on, history indexes the presync backup's copy
/// (no second copy of each file) and the version still restores.
#[tokio::test]
async fn history_reuses_the_presync_backup_and_still_restores_tcp() {
    let _g = e2e_guard().await;
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let path = format!("Mods/hist-presync-{tag}.package");
    let host_dir = temp_dir("hist-presync-host");
    let client_dir = temp_dir("hist-presync-client");
    write_file(&host_dir, &path, b"NEW_FROM_HOST");
    write_file(&client_dir, &path, b"MY_OLD_COPY");
    crate::commands::sync::set_auto_backup_config(true, false, 4, 5).await.unwrap();

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Alex").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    compute_plan(&client_state).await.expect("plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, path.clone(), Resolution::UseTheirs, None).await.expect("resolve");
    let result = run_sync_now(&client_state).await;
    crate::commands::sync::set_auto_backup_config(false, false, 4, 5).await.unwrap();
    result.expect("sync");

    let v = versions(&client_state, &path).await;
    assert_eq!(v.len(), 1, "one version, taken from the presync backup");
    assert_eq!(v[0].hash, sha256_hex(b"MY_OLD_COPY"));
    assert_eq!(v[0].peer, "Alex");
    history::restore_file_version_inner(&client_state, "sims4", &v[0].id).await.expect("restore");
    assert_eq!(read_file(&client_dir, &path), b"MY_OLD_COPY");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}
