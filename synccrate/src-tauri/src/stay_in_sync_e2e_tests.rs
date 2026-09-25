//! End-to-end: "stay in sync" pulls only a host's new, non-script files and
//! leaves everything that needs a decision alone.

use crate::commands::stay_in_sync::auto_pull_inner;
use crate::testutil::*;

#[tokio::test]
async fn auto_pull_adds_new_files_only_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("stay-host");
    let client_dir = temp_dir("stay-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&host_dir, "Mods/tool.ts4script", b"SCRIPT");
    write_file(&host_dir, "Mods/changed.package", b"HOST");
    write_file(&client_dir, "Mods/changed.package", b"MINE");
    write_file(&client_dir, "Mods/only-mine.package", b"KEEP");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state.clone()).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let r = auto_pull_inner(&client_state, null_events()).await.expect("auto pull");
    assert_eq!((r.pulled, r.scripts_held, r.needs_review), (1, 1, 1), "{r:?}");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"AAAA");
    assert!(!file_exists(&client_dir, "Mods/tool.ts4script"), "scripts wait for a manual sync");
    assert_eq!(read_file(&client_dir, "Mods/changed.package"), b"MINE", "never replaces a file");
    assert_eq!(read_file(&client_dir, "Mods/only-mine.package"), b"KEEP");
    assert!(client_state.lock().await.connections[&peer_id].sync_plan.is_none(), "no leftover plan");

    // The host adds a mod later: the next pull gets just that one.
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    crate::commands::files::scan_files_inner(&host_state, None, true).await.expect("host rescan");
    let r = auto_pull_inner(&client_state, null_events()).await.expect("auto pull 2");
    assert_eq!(r.pulled, 1, "{r:?}");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB");
    let r = auto_pull_inner(&client_state, null_events()).await.expect("auto pull 3");
    assert_eq!(r.pulled, 0);

    // An open plan (the user reviewing a sync) is never replaced.
    compute_plan(&client_state).await.expect("plan");
    let r = auto_pull_inner(&client_state, null_events()).await.expect("auto pull 4");
    assert!(r.skipped.as_deref().is_some_and(|s| s.contains("plan")), "{r:?}");
    assert!(client_state.lock().await.connections[&peer_id].sync_plan.is_some());

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn auto_pull_does_nothing_for_a_host() {
    let _g = e2e_guard().await;
    let dir = temp_dir("stay-as-host");
    let state = make_state("sims4", &dir);
    set_host(&state, "Host").await;
    let r = auto_pull_inner(&state, null_events()).await.unwrap();
    assert_eq!(r.pulled, 0);
    assert!(r.skipped.is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn auto_pull_extends_the_last_sync_record_instead_of_replacing_it_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("stay-undo-host");
    let client_dir = temp_dir("stay-undo-client");
    write_file(&host_dir, "Mods/changed.package", b"HOST");
    write_file(&client_dir, "Mods/changed.package", b"MINE");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state.clone()).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    // A manual sync that replaces a file.
    compute_plan(&client_state).await.expect("plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/changed.package".into(), crate::state::Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("manual sync");
    let manual = crate::commands::undo::read_record("sims4").expect("record");
    assert_eq!(manual.replaced.len(), 1);

    // Then stay-in-sync pulls a new mod.
    write_file(&host_dir, "Mods/new.package", b"NEW");
    crate::commands::files::scan_files_inner(&host_state, None, true).await.expect("host rescan");
    let r = auto_pull_inner(&client_state, null_events()).await.expect("auto pull");
    assert_eq!(r.pulled, 1, "{r:?}");

    let merged = crate::commands::undo::read_record("sims4").expect("record");
    assert_eq!(merged.sync_id, manual.sync_id, "same record, extended");
    assert_eq!(merged.replaced.len(), 1, "the manual replacement is still undoable");
    assert_eq!(merged.presync_backup_id, manual.presync_backup_id);
    assert!(merged.added.iter().any(|f| f.relative_path == "Mods/new.package"));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}
