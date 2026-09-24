//! End-to-end sync tests: two real `AppState`s (host + client) connected over
//! a real transport, driving the actual handshake/plan/execute code paths.
//! See `testutil.rs` for the shared helpers and why every test holds
//! `e2e_guard()` for its whole body.

use crate::state::{Resolution, SyncAction};
use crate::testutil::*;

#[tokio::test]
async fn happy_path_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("happy-host");
    let client_dir = temp_dir("happy-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    write_file(&host_dir, "Mods/c.package", b"CCCC");
    write_file(&client_dir, "Mods/a.package", b"AAAA");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;

    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("compute plan");
    let receives: Vec<_> = plan
        .actions
        .iter()
        .filter_map(|a| match a {
            SyncAction::ReceiveFromRemote(f) => Some(f.relative_path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(receives.len(), 2, "expected exactly B and C, got {:?}", receives);
    assert!(receives.contains(&"Mods/b.package".to_string()));
    assert!(receives.contains(&"Mods/c.package".to_string()));
    assert!(plan.actions.iter().all(|a| !matches!(a, SyncAction::Conflict { .. })));

    run_sync_now(&client_state).await.expect("sync");

    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"AAAA");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB");
    assert_eq!(read_file(&client_dir, "Mods/c.package"), b"CCCC");
    assert!(no_leftover_temp_files(&client_dir));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn happy_path_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("happy-iroh-host");
    let client_dir = temp_dir("happy-iroh-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    write_file(&client_dir, "Mods/a.package", b"AAAA");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let (host_ep, client_ep) = iroh_pair().await;
    tokio::spawn(serve_one_iroh_connection(host_ep.clone(), host_state));

    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_iroh(client_ep, &host_ep, client_state.clone(), &peer_id)
        .await
        .expect("iroh connect");

    let plan = compute_plan(&client_state).await.expect("compute plan");
    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/b.package"));

    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn wrong_game_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("wrong-game-host");
    let client_dir = temp_dir("wrong-game-client");
    write_file(&host_dir, "Mods/secret.package", b"SIMS4MOD");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;

    // Client has ETS2 selected; the host is sharing Sims 4.
    let client_state = make_state("ets2", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    let err = connect_client_tcp_expect_err(client_state.clone(), port, &peer_id).await;

    assert_eq!(
        crate::network::protocol::parse_wrong_game(&err),
        Some("sims4"),
        "expected a wrong-game error naming the host's game, got: {}",
        err
    );
    assert!(!client_state.lock().await.connections.contains_key(&peer_id));
    // Nothing was ever written into the client's folders.
    assert!(!file_exists(&client_dir, "Mods/secret.package"));
    assert!(walkdir::WalkDir::new(&client_dir).into_iter().filter_map(|e| e.ok()).all(|e| !e.file_type().is_file()));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn old_host_foreign_files_filtered_tcp() {
    let _g = e2e_guard().await;
    let client_dir = temp_dir("old-host-client");

    let mut files = std::collections::HashMap::new();
    files.insert("mod/truck1.scs".to_string(), b"FOREIGN1".to_vec());
    files.insert("mod/truck2.scs".to_string(), b"FOREIGN2".to_vec());
    files.insert("mod/truck3.scs".to_string(), b"FOREIGN3".to_vec());
    files.insert("Mods/legit.package".to_string(), b"LEGIT".to_vec());

    // No game_id on the Welcome: simulates a host older than 0.5.6.
    let port = start_fake_old_host_tcp(files, None).await;

    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("compute plan");
    assert_eq!(plan.skipped_foreign, 3, "the three ETS2-shaped paths should be dropped");
    assert!(plan.warning.is_some(), "an old, mostly-foreign host should set the plan warning");
    assert!(plan.host_game.is_none());
    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/legit.package"));

    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/legit.package"), b"LEGIT");
    assert!(!client_dir.join("mod").exists(), "foreign-game folder must never be created");

    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn conflict_resolutions_tcp() {
    let _g = e2e_guard().await;

    async fn setup(label: &str) -> (std::path::PathBuf, std::path::PathBuf, std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>, u16) {
        let host_dir = temp_dir(&format!("conflict-host-{label}"));
        let client_dir = temp_dir(&format!("conflict-client-{label}"));
        let host_state = make_state("sims4", &host_dir);
        set_host(&host_state, "Host").await;
        let port = start_tcp_host(host_state).await;
        let client_state = make_state("sims4", &client_dir);
        (host_dir, client_dir, client_state, port)
    }

    // Use theirs: the download replaces the local file.
    {
        let (host_dir, client_dir, client_state, port) = setup("theirs").await;
        write_file(&host_dir, "Mods/conflict.package", b"HOST_V");
        write_file(&client_dir, "Mods/conflict.package", b"CLIENT_V");
        let peer_id = new_peer_id();
        mark_pending_client(&client_state, &peer_id).await;
        connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
        let plan = compute_plan(&client_state).await.expect("compute plan");
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::Conflict { .. }));
        crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/conflict.package".to_string(), Resolution::UseTheirs, None)
            .await
            .expect("resolve");
        run_sync_now(&client_state).await.expect("sync");
        assert_eq!(read_file(&client_dir, "Mods/conflict.package"), b"HOST_V");
        let _ = std::fs::remove_dir_all(&host_dir);
        let _ = std::fs::remove_dir_all(&client_dir);
    }

    // Keep mine: nothing changes locally.
    {
        let (host_dir, client_dir, client_state, port) = setup("mine").await;
        write_file(&host_dir, "Mods/conflict.package", b"HOST_V");
        write_file(&client_dir, "Mods/conflict.package", b"CLIENT_V");
        let peer_id = new_peer_id();
        mark_pending_client(&client_state, &peer_id).await;
        connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
        compute_plan(&client_state).await.expect("compute plan");
        crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/conflict.package".to_string(), Resolution::KeepMine, None)
            .await
            .expect("resolve");
        run_sync_now(&client_state).await.expect("sync");
        assert_eq!(read_file(&client_dir, "Mods/conflict.package"), b"CLIENT_V");
        let _ = std::fs::remove_dir_all(&host_dir);
        let _ = std::fs::remove_dir_all(&client_dir);
    }

    // Keep both: the remote copy lands under a `_remote` name; a second
    // colliding case picks `_remote2`.
    {
        let (host_dir, client_dir, client_state, port) = setup("both").await;
        write_file(&host_dir, "Mods/conflict.package", b"HOST_V");
        write_file(&client_dir, "Mods/conflict.package", b"CLIENT_V");
        write_file(&host_dir, "Mods/collide.package", b"HOST2");
        write_file(&client_dir, "Mods/collide.package", b"CLIENT2");
        write_file(&client_dir, "Mods/collide_remote.package", b"UNRELATED");
        let peer_id = new_peer_id();
        mark_pending_client(&client_state, &peer_id).await;
        connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
        compute_plan(&client_state).await.expect("compute plan");
        crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/conflict.package".to_string(), Resolution::KeepBoth, None)
            .await
            .expect("resolve conflict.package");
        crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/collide.package".to_string(), Resolution::KeepBoth, None)
            .await
            .expect("resolve collide.package");
        run_sync_now(&client_state).await.expect("sync");
        assert_eq!(read_file(&client_dir, "Mods/conflict.package"), b"CLIENT_V");
        assert_eq!(read_file(&client_dir, "Mods/conflict_remote.package"), b"HOST_V");
        assert_eq!(read_file(&client_dir, "Mods/collide.package"), b"CLIENT2");
        assert_eq!(read_file(&client_dir, "Mods/collide_remote.package"), b"UNRELATED", "unrelated pre-existing file must survive");
        assert_eq!(read_file(&client_dir, "Mods/collide_remote2.package"), b"HOST2", "the taken _remote name forces _remote2");
        let _ = std::fs::remove_dir_all(&host_dir);
        let _ = std::fs::remove_dir_all(&client_dir);
    }
}

#[tokio::test]
async fn conflict_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("conflict-iroh-host");
    let client_dir = temp_dir("conflict-iroh-client");
    write_file(&host_dir, "Mods/conflict.package", b"HOST_V");
    write_file(&client_dir, "Mods/conflict.package", b"CLIENT_V");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let (host_ep, client_ep) = iroh_pair().await;
    tokio::spawn(serve_one_iroh_connection(host_ep.clone(), host_state));

    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_iroh(client_ep, &host_ep, client_state.clone(), &peer_id)
        .await
        .expect("iroh connect");

    let plan = compute_plan(&client_state).await.expect("compute plan");
    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(&plan.actions[0], SyncAction::Conflict { .. }));

    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/conflict.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/conflict.package"), b"HOST_V");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn case_only_names_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("case-host");
    let client_dir = temp_dir("case-client");
    write_file(&host_dir, "Mods/CC/Hair.package", b"HOSTHAIR");
    write_file(&client_dir, "Mods/cc/hair.package", b"CLIENTHAIR");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("compute plan");
    assert_eq!(plan.actions.len(), 1);
    match &plan.actions[0] {
        SyncAction::Conflict { local, remote } => {
            assert_eq!(local.relative_path, "Mods/cc/hair.package");
            assert_eq!(remote.relative_path, "Mods/CC/Hair.package");
        }
        other => panic!("expected a conflict, got {:?}", other),
    }

    // Never a silent overwrite: executing without resolving refuses.
    let err = run_sync_now(&client_state).await.expect_err("unresolved conflicts must refuse to sync");
    assert!(err.contains("Resolve all conflicts"), "unexpected error: {err}");
    assert_eq!(read_file(&client_dir, "Mods/cc/hair.package"), b"CLIENTHAIR");
    // Windows filesystems are case-insensitive, so "Mods/CC/Hair.package" is
    // the same file as "Mods/cc/hair.package" — assert there's exactly one
    // directory entry rather than checking the case-variant path directly.
    let cc_entries: Vec<_> = std::fs::read_dir(client_dir.join("Mods/cc")).unwrap().filter_map(|e| e.ok()).collect();
    assert_eq!(cc_entries.len(), 1, "no case-variant twin should have been created");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn changed_since_compare_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("changed-host");
    let client_dir = temp_dir("changed-client");
    write_file(&host_dir, "Mods/x.package", b"HOSTX");
    write_file(&client_dir, "Mods/x.package", b"CLIENTX");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    compute_plan(&client_state).await.expect("compute plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/x.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");

    // The user (or the game) edits the file after compare, before execute.
    write_file(&client_dir, "Mods/x.package", b"CHANGED_LOCALLY");

    let err = run_sync_now(&client_state).await.expect_err("a locally-changed file must not be overwritten");
    assert!(err.contains("1 file(s) failed"), "unexpected error: {err}");
    assert_eq!(read_file(&client_dir, "Mods/x.package"), b"CHANGED_LOCALLY");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn host_changed_since_compare_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("hostchanged-host");
    let client_dir = temp_dir("hostchanged-client");
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    write_file(&host_dir, "Mods/extra.package", b"ORIGINAL");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("compute plan");
    assert_eq!(plan.actions.len(), 2);

    // The host changes `extra.package` after the compare (mtime must move
    // too, or the host's hash cache would still report the old, matching
    // hash and this test would degenerate into a no-op change).
    write_file_mtime(&host_dir, "Mods/extra.package", b"CHANGED_ON_HOST", 4_000_000_000);

    let err = run_sync_now(&client_state).await.expect_err("the host-changed file must fail");
    assert!(err.contains("1 file(s) failed"), "unexpected error: {err}");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB", "the untouched file should still succeed");
    assert!(!file_exists(&client_dir, "Mods/extra.package"), "the host-changed file must never be written");
    assert!(no_leftover_temp_files(&client_dir));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn disabled_mods_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("disabled-host");
    let client_dir = temp_dir("disabled-client");

    // 1) Same content, client's copy disabled: in sync, nothing downloaded.
    write_file(&host_dir, "Mods/same.package", b"SAME");
    write_file(&client_dir, "Mods/same.package.disabled", b"SAME");

    // 2) Different content, client's copy disabled: conflict; "use theirs"
    //    must write to the disabled path and keep it disabled.
    write_file(&host_dir, "Mods/diff.package", b"HOST_DIFF");
    write_file(&client_dir, "Mods/diff.package.disabled", b"CLIENT_DIFF");

    // 3) Host has it disabled, client has it enabled: no action at all —
    //    the host having a disabled copy must never create a disabled twin.
    write_file(&host_dir, "Mods/onlyhost.package.disabled", b"X");
    write_file(&client_dir, "Mods/onlyhost.package", b"Y");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("compute plan");
    assert_eq!(plan.disabled_locally, 1, "case 1: same content while disabled locally");
    assert_eq!(plan.disabled_on_host, 1, "case 3: host-disabled/client-enabled is a no-op");
    assert_eq!(plan.actions.len(), 1, "only case 2 (the differing disabled copy) should need a decision");
    let SyncAction::Conflict { local, .. } = &plan.actions[0] else { panic!("expected a conflict") };
    assert_eq!(local.relative_path, "Mods/diff.package.disabled");

    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/diff.package.disabled".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");

    assert!(!file_exists(&client_dir, "Mods/same.package"), "case 1 must not create an enabled twin");
    assert_eq!(read_file(&client_dir, "Mods/diff.package.disabled"), b"HOST_DIFF", "case 2 stays disabled");
    assert!(!file_exists(&client_dir, "Mods/diff.package"), "case 2 must not also appear enabled");
    assert!(!file_exists(&client_dir, "Mods/onlyhost.package.disabled"), "case 3 must not create a disabled twin");
    assert_eq!(read_file(&client_dir, "Mods/onlyhost.package"), b"Y", "case 3 leaves the client's enabled copy alone");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn plan_invalidation_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("invalid-host");
    let client_dir = temp_dir("invalid-client");
    let other_dir = temp_dir("invalid-client-other");
    write_file(&host_dir, "Mods/a.package", b"AAAA");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    compute_plan(&client_state).await.expect("compute plan");
    // The game folder changes after the compare.
    client_state.lock().await.game_paths.insert("sims4".to_string(), other_dir.to_string_lossy().to_string());

    let err = run_sync_now(&client_state).await.expect_err("a changed game folder must refuse to run the old plan");
    assert!(err.contains("compare again"), "unexpected error: {err}");
    assert!(!file_exists(&other_dir, "Mods/a.package"));
    assert!(!file_exists(&client_dir, "Mods/a.package"));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
    let _ = std::fs::remove_dir_all(&other_dir);
}

#[tokio::test]
async fn resume_after_cancel_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("resume-host");
    let client_dir = temp_dir("resume-client");
    const FILE_COUNT: usize = 8;
    for i in 0..FILE_COUNT {
        // Large enough that a cancel reliably lands before all of them land.
        let content = format!("FILE-{i}-").repeat(50_000);
        write_file(&host_dir, &format!("Mods/f{i}.package"), content.as_bytes());
    }

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    compute_plan(&client_state).await.expect("compute plan");

    let handle = spawn_sync(client_state.clone());
    wait_until_file_exists(&client_dir, "Mods/f0.package").await;
    let was_syncing = crate::commands::sync::cancel_sync_inner(&client_state).await.expect("cancel");
    assert!(was_syncing);
    let result = handle.await.expect("join");
    assert_eq!(result, Err(crate::commands::sync::SYNC_CANCELLED.to_string()));

    // Not every file made it across before the cancellation.
    let synced_before_resume = (0..FILE_COUNT).filter(|i| file_exists(&client_dir, &format!("Mods/f{i}.package"))).count();
    assert!(synced_before_resume < FILE_COUNT, "test is not exercising a real mid-sync cancel");

    // Resume: compare and sync again.
    compute_plan(&client_state).await.expect("compute plan again");
    run_sync_now(&client_state).await.expect("resumed sync");

    for i in 0..FILE_COUNT {
        let expected = format!("FILE-{i}-").repeat(50_000);
        assert_eq!(read_file(&client_dir, &format!("Mods/f{i}.package")), expected.as_bytes());
    }
    assert!(no_leftover_temp_files(&client_dir));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// Resets `auto_backup_before_sync` to off when dropped, even on panic, so a
/// failing assertion in this test can't leave the flag on for every later
/// test in the process (they share the redirected `SYNCCRATE_CONFIG_DIR`).
struct ResetAutoBackup;
impl Drop for ResetAutoBackup {
    fn drop(&mut self) {
        let mut config = crate::commands::sync::read_sync_config();
        config.auto_backup_before_sync = false;
        let path = crate::utils::sync_config_path();
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap());
    }
}

/// The backup byte/mtime/dedup/GC/restore behavior is already fully unit
/// tested directly against `presync_targets`/`create_backup_inner` in
/// `commands/backup.rs`. What those tests don't cover is the wiring: that a
/// real `execute_sync` run actually calls `create_presync_backup` with the
/// plan's real targets. This test closes that one gap.
#[tokio::test]
async fn presync_backup_created_before_real_sync_tcp() {
    let _g = e2e_guard().await;
    let _reset = ResetAutoBackup;
    let host_dir = temp_dir("presync-host");
    let client_dir = temp_dir("presync-client");
    write_file(&host_dir, "Mods/conflict.package", b"HOST_V");
    write_file(&host_dir, "Mods/new.package", b"NEW");
    write_file(&client_dir, "Mods/conflict.package", b"CLIENT_V");

    crate::commands::sync::set_auto_backup_config(true, false, 4, 5)
        .await
        .expect("enable auto backup");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    compute_plan(&client_state).await.expect("compute plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/conflict.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");

    let backups = crate::commands::backup::list_backups().await.expect("list backups");
    let presync = backups
        .iter()
        .find(|b| b.kind == "presync")
        .expect("execute_sync should have created a presync backup");
    // Exactly the replaced file (the client's pre-sync 8-byte content), never
    // the freshly-received `new.package` (nothing local for it to replace).
    assert_eq!(presync.file_count, 1, "backup should contain exactly the one replaced file");
    assert_eq!(presync.total_size, "CLIENT_V".len() as u64, "backup should hold the local file's original bytes");

    assert_eq!(read_file(&client_dir, "Mods/conflict.package"), b"HOST_V");
    assert_eq!(read_file(&client_dir, "Mods/new.package"), b"NEW");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn multi_client_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("multi-host");
    let client_a_dir = temp_dir("multi-client-a");
    let client_b_dir = temp_dir("multi-client-b");
    write_file(&host_dir, "Mods/shared.package", b"SHARED");
    write_file(&host_dir, "Mods/a_only.package", b"A_ONLY");
    write_file(&client_b_dir, "Mods/a_only.package", b"STALE_ON_B_ONLY_NAME_COLLIDES_NOT");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;

    let client_a = make_state("sims4", &client_a_dir);
    let peer_a = new_peer_id();
    mark_pending_client(&client_a, &peer_a).await;
    connect_client_tcp(client_a.clone(), port, &peer_a).await.expect("connect A");

    let client_b = make_state("sims4", &client_b_dir);
    let peer_b = new_peer_id();
    mark_pending_client(&client_b, &peer_b).await;
    connect_client_tcp(client_b.clone(), port, &peer_b).await.expect("connect B");

    let (plan_a, plan_b) = tokio::join!(compute_plan(&client_a), compute_plan(&client_b));
    let plan_a = plan_a.expect("plan A");
    let plan_b = plan_b.expect("plan B");
    // B already has a_only.package with different content -> conflict for B, plain receive for A.
    assert!(plan_a.actions.iter().any(|a| matches!(a, SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/a_only.package")));
    assert!(plan_b.actions.iter().any(|a| matches!(a, SyncAction::Conflict { local, .. } if local.relative_path == "Mods/a_only.package")));

    crate::commands::sync::resolve_conflict_inner(&client_b, "Mods/a_only.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve B's conflict");

    let (result_a, result_b) = tokio::join!(run_sync_now(&client_a), run_sync_now(&client_b));
    result_a.expect("sync A");
    result_b.expect("sync B");

    assert_eq!(read_file(&client_a_dir, "Mods/shared.package"), b"SHARED");
    assert_eq!(read_file(&client_a_dir, "Mods/a_only.package"), b"A_ONLY");
    assert_eq!(read_file(&client_b_dir, "Mods/shared.package"), b"SHARED");
    assert_eq!(read_file(&client_b_dir, "Mods/a_only.package"), b"A_ONLY");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_a_dir);
    let _ = std::fs::remove_dir_all(&client_b_dir);
}
