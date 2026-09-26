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

/// Compare & Sync clicked before the host's manifest has arrived: the plan
/// waits for it instead of failing with "No remote manifest".
#[tokio::test]
async fn plan_waits_for_late_manifest_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("late-host");
    let client_dir = temp_dir("late-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;

    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    // Simulate the window before the manifest lands, then deliver it late.
    let manifest = client_state.lock().await.connections.get_mut(&peer_id).unwrap().remote_manifest.take();
    assert!(manifest.is_some());
    let late = client_state.clone();
    let pid = peer_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        late.lock().await.connections.get_mut(&pid).unwrap().remote_manifest = manifest;
    });

    let plan = compute_plan(&client_state).await.expect("plan should wait for the manifest");
    assert!(plan.actions.iter().any(|a| matches!(a, SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/a.package")));

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

// ---------------------------------------------------------------------------
// Undo last sync

async fn undo(client_state: &std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>) -> Result<crate::commands::backup::UndoResult, String> {
    tokio::time::timeout(std::time::Duration::from_secs(15), crate::commands::undo::undo_last_sync_inner(client_state, None))
        .await
        .expect("undo_last_sync timed out")
}

#[tokio::test]
async fn undo_happy_path_tcp() {
    let _g = e2e_guard().await;
    let _reset = ResetAutoBackup;
    crate::commands::sync::set_auto_backup_config(true, false, 4, 5).await.expect("enable auto backup");

    let host_dir = temp_dir("undo-happy-host");
    let client_dir = temp_dir("undo-happy-client");
    write_file(&host_dir, "Mods/a.package", b"NEW_A");
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    write_file(&host_dir, "Mods/c.package", b"CCCC");
    write_file_mtime(&client_dir, "Mods/a.package", b"OLD_A", 1_700_000_000);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    compute_plan(&client_state).await.expect("compute plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/a.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");

    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"NEW_A");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB");
    assert_eq!(read_file(&client_dir, "Mods/c.package"), b"CCCC");

    // Session is still open, matching the real "undo right after sync" flow.
    client_state.lock().await.session_type = crate::state::SessionType::None;
    let summary = undo(&client_state).await.expect("undo");
    assert_eq!(summary.restored, 1, "the replaced file: {:?}", summary.skipped);
    assert_eq!(summary.removed, 2, "the two added files: {:?}", summary.skipped);
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);

    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"OLD_A");
    let meta = std::fs::metadata(client_dir.join("Mods/a.package")).unwrap();
    assert_eq!(
        meta.modified().unwrap(),
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        "undo must restore the exact original mtime"
    );
    assert!(!file_exists(&client_dir, "Mods/b.package"));
    assert!(!file_exists(&client_dir, "Mods/c.package"));
    assert!(no_leftover_temp_files(&client_dir));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn undo_happy_path_iroh() {
    let _g = e2e_guard().await;
    let _reset = ResetAutoBackup;
    crate::commands::sync::set_auto_backup_config(true, false, 4, 5).await.expect("enable auto backup");

    let host_dir = temp_dir("undo-happy-iroh-host");
    let client_dir = temp_dir("undo-happy-iroh-client");
    write_file(&host_dir, "Mods/a.package", b"NEW_A");
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    write_file_mtime(&client_dir, "Mods/a.package", b"OLD_A", 1_700_000_000);

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

    compute_plan(&client_state).await.expect("compute plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/a.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"NEW_A");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB");

    client_state.lock().await.session_type = crate::state::SessionType::None;
    let summary = undo(&client_state).await.expect("undo");
    assert_eq!(summary.restored, 1, "{:?}", summary.skipped);
    assert_eq!(summary.removed, 1, "{:?}", summary.skipped);
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);

    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"OLD_A");
    assert!(!file_exists(&client_dir, "Mods/b.package"));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn undo_keep_both_removes_only_the_remote_copy_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("undo-both-host");
    let client_dir = temp_dir("undo-both-client");
    write_file(&host_dir, "Mods/x.package", b"HOST_V");
    write_file_mtime(&client_dir, "Mods/x.package", b"CLIENT_V", 1_700_000_000);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    compute_plan(&client_state).await.expect("compute plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/x.package".to_string(), Resolution::KeepBoth, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/x_remote.package"), b"HOST_V");

    client_state.lock().await.session_type = crate::state::SessionType::None;
    let summary = undo(&client_state).await.expect("undo");
    assert_eq!(summary.removed, 1);
    assert_eq!(summary.restored, 0);
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);

    assert!(!file_exists(&client_dir, "Mods/x_remote.package"));
    // The original, never touched by this sync, is untouched by the undo too.
    assert_eq!(read_file(&client_dir, "Mods/x.package"), b"CLIENT_V");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn undo_keeps_an_added_file_the_user_edited_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("undo-editadd-host");
    let client_dir = temp_dir("undo-editadd-client");
    write_file(&host_dir, "Mods/new.package", b"FROM_HOST");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    compute_plan(&client_state).await.expect("compute plan");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/new.package"), b"FROM_HOST");

    // The user (or the game) edits the newly-added file before undoing.
    write_file(&client_dir, "Mods/new.package", b"EDITED_BY_USER");

    client_state.lock().await.session_type = crate::state::SessionType::None;
    let summary = undo(&client_state).await.expect("undo");
    assert_eq!(summary.removed, 0);
    assert_eq!(summary.skipped.len(), 1);
    assert!(summary.skipped[0].contains("Mods/new.package"), "{:?}", summary.skipped);

    assert_eq!(read_file(&client_dir, "Mods/new.package"), b"EDITED_BY_USER");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn undo_never_overwrites_a_replaced_file_the_user_edited_tcp() {
    let _g = e2e_guard().await;
    let _reset = ResetAutoBackup;
    crate::commands::sync::set_auto_backup_config(true, false, 4, 5).await.expect("enable auto backup");

    let host_dir = temp_dir("undo-editrep-host");
    let client_dir = temp_dir("undo-editrep-client");
    write_file(&host_dir, "Mods/a.package", b"NEW_A");
    write_file(&client_dir, "Mods/a.package", b"OLD_A");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    compute_plan(&client_state).await.expect("compute plan");
    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/a.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"NEW_A");

    // The user edits the file the sync just wrote, before undoing.
    write_file(&client_dir, "Mods/a.package", b"EDITED_BY_USER");

    client_state.lock().await.session_type = crate::state::SessionType::None;
    let summary = undo(&client_state).await.expect("undo");
    assert_eq!(summary.restored, 0);
    assert_eq!(summary.skipped.len(), 1);
    assert!(summary.skipped[0].contains("Mods/a.package"), "{:?}", summary.skipped);

    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"EDITED_BY_USER");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn undo_after_cancel_reverts_only_what_was_written_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("undo-cancel-host");
    let client_dir = temp_dir("undo-cancel-client");
    const FILE_COUNT: usize = 8;
    for i in 0..FILE_COUNT {
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

    let synced_before_undo: Vec<usize> =
        (0..FILE_COUNT).filter(|i| file_exists(&client_dir, &format!("Mods/f{i}.package"))).collect();
    assert!(synced_before_undo.len() < FILE_COUNT, "test is not exercising a real mid-sync cancel");
    assert!(!synced_before_undo.is_empty());

    client_state.lock().await.session_type = crate::state::SessionType::None;
    let summary = undo(&client_state).await.expect("undo");
    assert_eq!(summary.removed, synced_before_undo.len());
    assert!(summary.skipped.is_empty(), "{:?}", summary.skipped);

    for i in 0..FILE_COUNT {
        assert!(!file_exists(&client_dir, &format!("Mods/f{i}.package")), "f{i} should have been removed by undo");
    }
    assert!(no_leftover_temp_files(&client_dir));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn second_undo_is_refused_and_undo_targets_the_newer_sync_tcp() {
    let _g = e2e_guard().await;
    let client_dir = temp_dir("undo-twice-client");

    async fn sync_one_file(client_state: &std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>, host_dir: &std::path::Path, path: &str, content: &[u8]) {
        write_file(host_dir, path, content);
        let host_state = make_state("sims4", host_dir);
        set_host(&host_state, "Host").await;
        let port = start_tcp_host(host_state).await;
        let peer_id = new_peer_id();
        mark_pending_client(client_state, &peer_id).await;
        connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
        compute_plan(client_state).await.expect("compute plan");
        run_sync_now(client_state).await.expect("sync");
        let mut s = client_state.lock().await;
        s.connections.remove(&peer_id);
        s.session_type = crate::state::SessionType::None;
    }

    let client_state = make_state("sims4", &client_dir);
    let host_dir_1 = temp_dir("undo-twice-host-1");
    sync_one_file(&client_state, &host_dir_1, "Mods/one.package", b"ONE").await;
    assert!(file_exists(&client_dir, "Mods/one.package"));

    let summary = undo(&client_state).await.expect("first undo");
    assert_eq!(summary.removed, 1);
    assert!(!file_exists(&client_dir, "Mods/one.package"));

    let err = undo(&client_state).await.expect_err("a second undo must be refused");
    assert!(err.contains("Nothing to undo"), "unexpected error: {err}");

    let host_dir_2 = temp_dir("undo-twice-host-2");
    sync_one_file(&client_state, &host_dir_2, "Mods/two.package", b"TWO").await;
    assert!(file_exists(&client_dir, "Mods/two.package"));

    let summary = undo(&client_state).await.expect("undo after the newer sync");
    assert_eq!(summary.removed, 1);
    assert!(!file_exists(&client_dir, "Mods/two.package"));
    // Nothing from the first (already-undone) sync should be touched again.
    assert!(!file_exists(&client_dir, "Mods/one.package"));

    let _ = std::fs::remove_dir_all(&host_dir_1);
    let _ = std::fs::remove_dir_all(&host_dir_2);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn presync_backup_survives_pruning_while_its_record_exists_tcp() {
    let _g = e2e_guard().await;
    let _reset = ResetAutoBackup;
    // max_count = 1: without protecting the record's own backup, creating a
    // newer presync backup would immediately prune the older one.
    crate::commands::sync::set_auto_backup_config(true, false, 4, 1).await.expect("enable auto backup");

    let client_dir = temp_dir("undo-gc-client");
    write_file(&client_dir, "Mods/a.package", b"C1");
    write_file(&client_dir, "Mods/b.package", b"C2");
    write_file(&client_dir, "Mods/c.package", b"C3");
    let client_state = make_state("sims4", &client_dir);

    async fn replace_one(
        client_state: &std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>,
        path: &str,
        host_content: &[u8],
    ) -> String {
        let host_dir = temp_dir("undo-gc-host");
        write_file(&host_dir, path, host_content);
        let host_state = make_state("sims4", &host_dir);
        set_host(&host_state, "Host").await;
        let port = start_tcp_host(host_state).await;
        let peer_id = new_peer_id();
        mark_pending_client(client_state, &peer_id).await;
        connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
        compute_plan(client_state).await.expect("compute plan");
        crate::commands::sync::resolve_conflict_inner(client_state, path.to_string(), Resolution::UseTheirs, None)
            .await
            .expect("resolve");
        run_sync_now(client_state).await.expect("sync");
        client_state.lock().await.connections.remove(&peer_id);
        let _ = std::fs::remove_dir_all(&host_dir);
        crate::commands::undo::read_record("sims4").expect("record").presync_backup_id.expect("presync backup")
    }

    async fn backup_exists(id: &str) -> bool {
        crate::commands::backup::list_backups().await.unwrap().iter().any(|b| b.id == id)
    }

    let p1 = replace_one(&client_state, "Mods/a.package", b"H1").await;
    assert!(backup_exists(&p1).await);

    // Round 2 creates a newer presync backup while p1's record is still the
    // current one (round 2 hasn't finished writing its own record yet at the
    // moment its presync backup is created and pruned) — p1 must survive.
    let p2 = replace_one(&client_state, "Mods/b.package", b"H2").await;
    assert_ne!(p1, p2);
    assert!(backup_exists(&p1).await, "p1's backup was pruned while its record still pointed to it");
    assert!(backup_exists(&p2).await);

    // Round 3: p1's record was superseded by round 2, so p1 is no longer
    // protected and falls out of the keep-1 window like any other backup.
    let p3 = replace_one(&client_state, "Mods/c.package", b"H3").await;
    assert_ne!(p2, p3);
    assert!(!backup_exists(&p1).await, "p1 should be pruned once its record is no longer current");
    assert!(backup_exists(&p2).await);
    assert!(backup_exists(&p3).await);

    let _ = std::fs::remove_dir_all(&client_dir);
}

// ---------------------------------------------------------------------------
// Modpacks

async fn pack_plan(client_state: &std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>, pack: crate::state::ModPack) -> Result<crate::state::SyncPlan, String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(15),
        crate::commands::modpack::compute_pack_sync_plan_inner(client_state, None, pack),
    )
    .await
    .expect("compute_pack_sync_plan timed out")
}

#[tokio::test]
async fn pack_import_syncs_exactly_the_missing_files_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("pack-happy-host");
    let client_dir = temp_dir("pack-happy-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    write_file(&client_dir, "Mods/a.package", b"AAAA");
    // A local extra the pack doesn't mention — must survive untouched.
    write_file(&client_dir, "Mods/extra.package", b"MINE");

    let pack = test_pack("sims4", &[("Mods/a.package", b"AAAA"), ("Mods/b.package", b"BBBB")]);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let cmp = crate::commands::modpack::compare_pack_inner(&client_state, pack.clone()).await.expect("compare");
    assert!(!cmp.wrong_game);
    assert_eq!(cmp.have, 1);
    assert_eq!(cmp.missing.len(), 1);
    assert_eq!(cmp.missing[0].relative_path, "Mods/b.package");

    let plan = pack_plan(&client_state, pack).await.expect("pack plan");
    assert_eq!(plan.actions.len(), 1, "only the missing file should be queued");
    assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/b.package"));
    assert!(plan.pack_unavailable.is_empty());

    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"AAAA");
    assert_eq!(read_file(&client_dir, "Mods/extra.package"), b"MINE", "an untouched local extra must survive");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn pack_import_happy_path_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("pack-happy-iroh-host");
    let client_dir = temp_dir("pack-happy-iroh-client");
    write_file(&host_dir, "Mods/b.package", b"BBBB");

    let pack = test_pack("sims4", &[("Mods/b.package", b"BBBB")]);

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

    let plan = pack_plan(&client_state, pack).await.expect("pack plan");
    assert_eq!(plan.actions.len(), 1);
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/b.package"), b"BBBB");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn pack_import_conflict_never_silently_overwrites_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("pack-conflict-host");
    let client_dir = temp_dir("pack-conflict-client");
    write_file(&host_dir, "Mods/a.package", b"PACK_CONTENT");
    write_file(&client_dir, "Mods/a.package", b"MY_OWN_CONTENT");

    let pack = test_pack("sims4", &[("Mods/a.package", b"PACK_CONTENT")]);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let cmp = crate::commands::modpack::compare_pack_inner(&client_state, pack.clone()).await.expect("compare");
    assert_eq!(cmp.different.len(), 1);
    assert_eq!(cmp.different[0].relative_path, "Mods/a.package");

    let plan = pack_plan(&client_state, pack).await.expect("pack plan");
    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(&plan.actions[0], SyncAction::Conflict { .. }));

    let err = run_sync_now(&client_state).await.expect_err("unresolved conflicts must refuse to sync");
    assert!(err.contains("Resolve all conflicts"), "unexpected error: {err}");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"MY_OWN_CONTENT", "never a silent overwrite");

    crate::commands::sync::resolve_conflict_inner(&client_state, "Mods/a.package".to_string(), Resolution::UseTheirs, None)
        .await
        .expect("resolve");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"PACK_CONTENT");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn pack_import_reports_unavailable_and_still_syncs_the_rest_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("pack-unavail-host");
    let client_dir = temp_dir("pack-unavail-client");
    // Host has "b" but not the pack's exact content for it, and doesn't have "c" at all.
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&host_dir, "Mods/b.package", b"HOST_VERSION_OF_B");

    let pack = test_pack(
        "sims4",
        &[("Mods/a.package", b"AAAA"), ("Mods/b.package", b"PACK_VERSION_OF_B"), ("Mods/c.package", b"CCCC")],
    );

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = pack_plan(&client_state, pack).await.expect("pack plan");
    assert_eq!(plan.actions.len(), 1, "only the available file should be queued");
    assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/a.package"));
    let mut unavailable = plan.pack_unavailable.clone();
    unavailable.sort();
    assert_eq!(unavailable, vec!["Mods/b.package".to_string(), "Mods/c.package".to_string()]);

    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"AAAA");
    assert!(!file_exists(&client_dir, "Mods/b.package"), "unavailable files must never be written");
    assert!(!file_exists(&client_dir, "Mods/c.package"));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn pack_for_another_game_is_refused_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("pack-wronggame-host");
    let client_dir = temp_dir("pack-wronggame-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");

    // The pack is for ETS2; the client has Sims 4 active.
    let pack = test_pack("ets2", &[("mod/truck.scs", b"TRUCK")]);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let cmp = crate::commands::modpack::compare_pack_inner(&client_state, pack.clone()).await.expect("compare");
    assert!(cmp.wrong_game, "the pack's game doesn't match the active game");

    let err = pack_plan(&client_state, pack).await.expect_err("a wrong-game pack must be refused");
    assert!(err.contains("different game"), "unexpected error: {err}");
    assert!(walkdir::WalkDir::new(&client_dir).into_iter().filter_map(|e| e.ok()).all(|e| !e.file_type().is_file()), "nothing should be written");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn undo_refused_during_a_sync_or_while_hosting_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("undo-guard-host");
    let client_dir = temp_dir("undo-guard-client");
    write_file(&host_dir, "Mods/new.package", b"FROM_HOST");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    compute_plan(&client_state).await.expect("compute plan");
    run_sync_now(&client_state).await.expect("sync");

    // Simulate a sync in progress (independent of session state).
    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = true;
    let err = undo(&client_state).await.expect_err("undo must refuse during a sync");
    assert!(err.contains("sync is in progress"), "unexpected error: {err}");
    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = false;

    // A host never rewrites files it may be serving.
    client_state.lock().await.session_type = crate::state::SessionType::Host;
    let err = undo(&client_state).await.expect_err("undo must refuse while hosting");
    assert!(err.contains("hosting"), "unexpected error: {err}");
    client_state.lock().await.session_type = crate::state::SessionType::Client;

    // Still connected as a client: undo works (the "Undo" toast right after a
    // sync is shown while connected) and drops any plan computed before it.
    compute_plan(&client_state).await.ok();
    let summary = undo(&client_state).await.expect("undo while connected");
    assert_eq!(summary.removed, 1);
    assert!(!file_exists(&client_dir, "Mods/new.package"));
    assert!(client_state.lock().await.connections[&peer_id].sync_plan.is_none(), "stale plan dropped");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// A clicked invite link only classifies: the client stays idle and nothing
/// is written. The code it carries then drives the ordinary connect path
/// (what the Join button does), which still stops at a plan the user has to
/// confirm. While a sync runs, links are refused outright.
#[tokio::test]
async fn join_link_leads_to_normal_connect_and_writes_nothing_tcp() {
    use crate::commands::open_intent::{intent_from_raw, OpenIntent};
    use crate::network::joincode;
    let _g = e2e_guard().await;
    let host_dir = temp_dir("link-host");
    let client_dir = temp_dir("link-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&client_dir, "Mods/mine.package", b"MINE");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let code = joincode::encode(&joincode::JoinInfo {
        addresses: vec!["127.0.0.1".parse().unwrap()],
        port,
        pin: None,
        internet_id: None,
    })
    .unwrap();

    let client_state = make_state("sims4", &client_dir);
    // Discord-style trailing punctuation included.
    let link = format!("synccrate://join/{code}?game=sims4).");
    let Some(OpenIntent::Join { code: got, game_id }) = intent_from_raw(&client_state, &link, None).await else {
        panic!("join link not recognized");
    };
    assert_eq!(game_id, "sims4");
    // A link for another game is surfaced as-is (the UI shows the switch
    // prompt); the active game is not touched.
    let other = format!("synccrate://join/{code}?game=valheim");
    assert!(matches!(intent_from_raw(&client_state, &other, None).await, Some(OpenIntent::Join { ref game_id, .. }) if game_id == "valheim"));
    {
        let s = client_state.lock().await;
        assert_eq!(s.session_type, crate::state::SessionType::None);
        assert!(s.connections.is_empty());
        assert_eq!(s.active_game, "sims4");
    }
    assert!(!file_exists(&client_dir, "Mods/a.package"));

    let info = joincode::decode(&got).unwrap();
    assert_eq!(info.port, port);
    assert_eq!(info.addresses, vec![std::net::Ipv4Addr::LOCALHOST]);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), info.port, &peer_id).await.expect("connect");
    {
        let s = client_state.lock().await;
        assert!(s.connections.get(&peer_id).is_some_and(|c| c.sync_plan.is_none() && !c.is_syncing));
    }
    assert!(!file_exists(&client_dir, "Mods/a.package"), "connecting must not sync on its own");
    assert_eq!(read_file(&client_dir, "Mods/mine.package"), b"MINE");

    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = true;
    assert!(matches!(intent_from_raw(&client_state, &link, None).await, Some(OpenIntent::Invalid { .. })));
    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = false;

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

// ---------------------------------------------------------------------------
// Apply pack exactly

use crate::commands::pack_apply;

async fn preview_apply(state: &std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>, pack: &crate::state::ModPack) -> pack_apply::PackApplyPreview {
    pack_apply::preview_pack_apply_inner(state, pack.clone()).await.expect("preview")
}

fn rels(items: &[pack_apply::ApplyItem]) -> Vec<&str> {
    items.iter().map(|i| i.relative_path.as_str()).collect()
}

fn mtime_of(base: &std::path::Path, rel: &str) -> std::time::SystemTime {
    std::fs::metadata(base.join(rel)).unwrap().modified().unwrap()
}

/// Client folder with pack files, a missing pack file, an extra mod, an extra
/// save and a disabled copy of a pack file. Returns (host, client, pack).
fn exact_fixture(label: &str) -> (std::path::PathBuf, std::path::PathBuf, crate::state::ModPack) {
    let host_dir = temp_dir(&format!("{label}-host"));
    let client_dir = temp_dir(&format!("{label}-client"));
    for (p, c) in [("Mods/a.package", &b"AAAA"[..]), ("Mods/b.package", b"BBBB"), ("Mods/CC/c.package", b"CCCC")] {
        write_file(&host_dir, p, c);
    }
    write_file(&client_dir, "Mods/a.package", b"AAAA");
    write_file(&client_dir, "Mods/cc/C.package.disabled", b"CCCC");
    write_file(&client_dir, "Mods/extra.package", b"MINE");
    write_file(&client_dir, "Saves/Slot_00000001.save", b"SAVE");
    let pack = test_pack("sims4", &[("Mods/a.package", b"AAAA"), ("Mods/b.package", b"BBBB"), ("Mods/CC/c.package", b"CCCC")]);
    (host_dir, client_dir, pack)
}

async fn apply_exact_flow(client_state: &std::sync::Arc<tokio::sync::Mutex<crate::state::AppState>>, client_dir: &std::path::Path, pack: crate::state::ModPack) {
    crate::commands::tags::set_tags_for_test("sims4", "Mods/extra.package", &["mine"]);
    let preview = preview_apply(client_state, &pack).await;
    assert!(preview.available, "{:?}", preview.unavailable_reason);
    assert_eq!(preview.to_download.len(), 1);
    assert_eq!(preview.to_download[0].relative_path, "Mods/b.package");
    assert!(preview.conflicts.is_empty());
    assert_eq!(rels(&preview.to_enable), ["Mods/cc/C.package.disabled"]);
    assert_eq!(rels(&preview.to_disable), ["Mods/extra.package"], "the save is out of scope");

    // Download step not done yet: the rename step refuses and touches nothing.
    let err = pack_apply::apply_pack_exact_inner(client_state, pack.clone(), preview.clone()).await.expect_err("missing files");
    assert!(err.contains("still missing"), "unexpected error: {err}");
    assert!(file_exists(client_dir, "Mods/extra.package"));

    pack_plan(client_state, pack.clone()).await.expect("pack plan");
    run_sync_now(client_state).await.expect("sync");
    let result = pack_apply::apply_pack_exact_inner(client_state, pack, preview).await.expect("apply");
    assert_eq!((result.enabled, result.disabled), (1, 1));
    assert!(result.skipped.is_empty(), "{:?}", result.skipped);

    assert_eq!(read_file(client_dir, "Mods/b.package"), b"BBBB", "missing file arrived");
    assert_eq!(read_file(client_dir, "Mods/extra.package.disabled"), b"MINE", "extra disabled, not deleted");
    assert!(!file_exists(client_dir, "Mods/extra.package"));
    assert_eq!(read_file(client_dir, "Saves/Slot_00000001.save"), b"SAVE", "save untouched");
    assert_eq!(read_file(client_dir, "Mods/cc/C.package"), b"CCCC", "pack file re-enabled");
    assert_eq!(crate::commands::tags::tags_for_test("sims4", "Mods/extra.package.disabled"), ["mine"], "tags follow the file");
}

#[tokio::test]
async fn apply_pack_exactly_tcp() {
    let _g = e2e_guard().await;
    let (host_dir, client_dir, pack) = exact_fixture("exact");
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    apply_exact_flow(&client_state, &client_dir, pack).await;
    pack_apply::delete_record("sims4");
    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn apply_pack_exactly_iroh() {
    let _g = e2e_guard().await;
    let (host_dir, client_dir, pack) = exact_fixture("exact-iroh");
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let (host_ep, client_ep) = iroh_pair().await;
    tokio::spawn(serve_one_iroh_connection(host_ep.clone(), host_state));
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_iroh(client_ep, &host_ep, client_state.clone(), &peer_id).await.expect("iroh connect");

    apply_exact_flow(&client_state, &client_dir, pack).await;
    pack_apply::delete_record("sims4");
    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn apply_pack_exactly_conflict_is_never_silently_overwritten_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("exact-conflict-host");
    let client_dir = temp_dir("exact-conflict-client");
    write_file(&host_dir, "Mods/a.package", b"PACK_CONTENT");
    write_file(&client_dir, "Mods/a.package", b"MY_OWN_CONTENT");
    let pack = test_pack("sims4", &[("Mods/a.package", b"PACK_CONTENT")]);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let preview = preview_apply(&client_state, &pack).await;
    assert_eq!(preview.conflicts.len(), 1);
    assert!(preview.to_disable.is_empty() && preview.to_enable.is_empty(), "a differing pack file is a conflict, not an extra");
    let plan = pack_plan(&client_state, pack.clone()).await.expect("pack plan");
    assert!(matches!(&plan.actions[0], SyncAction::Conflict { .. }));
    run_sync_now(&client_state).await.expect_err("unresolved conflicts must refuse to sync");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"MY_OWN_CONTENT");

    // "Keep mine" leaves it different: applying refuses (it used to report
    // "Pack applied" with the wrong version live) and never touches it.
    let err = pack_apply::apply_pack_exact_inner(&client_state, pack, preview).await.expect_err("a kept conflict isn't the pack");
    assert!(err.contains("aren't the pack's version"), "{err}");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"MY_OWN_CONTENT");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn revert_pack_apply_restores_renames_and_reports_moved_files() {
    let _g = e2e_guard().await;
    let dir = temp_dir("exact-revert");
    write_file_mtime(&dir, "Mods/a.package", b"AAAA", 1_600_000_000);
    write_file_mtime(&dir, "Mods/Sub/x.package", b"XXXX", 1_600_000_100);
    write_file_mtime(&dir, "Mods/y.package", b"YYYY", 1_600_000_200);
    write_file_mtime(&dir, "Mods/c.package.disabled", b"CCCC", 1_600_000_300);
    let pack = test_pack("sims4", &[("Mods/a.package", b"AAAA"), ("Mods/c.package", b"CCCC")]);
    let before: Vec<_> = ["Mods/Sub/x.package", "Mods/c.package.disabled"].iter().map(|p| mtime_of(&dir, p)).collect();

    // Nothing to download, so no host is needed at all.
    let state = make_state("sims4", &dir);
    let preview = preview_apply(&state, &pack).await;
    let result = pack_apply::apply_pack_exact_inner(&state, pack, preview).await.expect("apply");
    assert_eq!((result.enabled, result.disabled), (1, 2));
    let record = pack_apply::read_record("sims4").expect("apply record");
    assert_eq!(record.moves.len(), 3);

    // The user moves one disabled file elsewhere before reverting.
    std::fs::rename(dir.join("Mods/y.package.disabled"), dir.join("Mods/y-moved.package.disabled")).unwrap();

    let r = pack_apply::revert_pack_apply_inner(&state, None).await.expect("revert");
    assert_eq!(r.reverted, 2);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].relative_path, "Mods/y.package.disabled");
    assert_eq!(read_file(&dir, "Mods/Sub/x.package"), b"XXXX");
    assert_eq!(read_file(&dir, "Mods/c.package.disabled"), b"CCCC");
    assert!(!file_exists(&dir, "Mods/c.package"));
    let after: Vec<_> = ["Mods/Sub/x.package", "Mods/c.package.disabled"].iter().map(|p| mtime_of(&dir, p)).collect();
    assert_eq!(before, after, "renames keep mtimes");
    assert!(file_exists(&dir, "Mods/y-moved.package.disabled"), "the moved file stays where the user put it");
    assert!(pack_apply::read_record("sims4").is_none(), "one revert per apply");
    pack_apply::revert_pack_apply_inner(&state, None).await.expect_err("nothing left to revert");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn apply_pack_exactly_is_refused_for_games_that_cannot_disable() {
    let _g = e2e_guard().await;
    let dir = temp_dir("exact-none");
    write_file(&dir, "Mods/ModA/manifest.json", b"A");
    write_file(&dir, "Mods/Extra/manifest.json", b"E");
    let pack = test_pack("stardew_valley", &[("Mods/ModA/manifest.json", b"A")]);
    let state = make_state("stardew_valley", &dir);

    let preview = preview_apply(&state, &pack).await;
    assert!(!preview.available);
    assert!(preview.unavailable_reason.as_deref().unwrap_or("").contains("can't be disabled"));
    assert!(preview.to_disable.is_empty());
    let err = pack_apply::apply_pack_exact_inner(&state, pack, preview).await.expect_err("refused");
    assert!(err.contains("can't be disabled"), "unexpected error: {err}");
    assert!(file_exists(&dir, "Mods/Extra/manifest.json"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn apply_pack_exactly_skips_a_disable_whose_target_exists() {
    let _g = e2e_guard().await;
    let dir = temp_dir("exact-taken");
    write_file(&dir, "Mods/a.package", b"AAAA");
    write_file(&dir, "Mods/extra.package", b"NEW");
    write_file(&dir, "Mods/extra.package.disabled", b"OLD");
    let pack = test_pack("sims4", &[("Mods/a.package", b"AAAA")]);
    let state = make_state("sims4", &dir);

    let preview = preview_apply(&state, &pack).await;
    assert_eq!(rels(&preview.to_disable), ["Mods/extra.package"]);
    let result = pack_apply::apply_pack_exact_inner(&state, pack, preview).await.expect("apply");
    assert_eq!(result.disabled, 0);
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(read_file(&dir, "Mods/extra.package"), b"NEW");
    assert_eq!(read_file(&dir, "Mods/extra.package.disabled"), b"OLD", "never overwritten");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn apply_pack_exactly_refused_during_sync_hosting_restore_or_changed_folder_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("exact-guard-host");
    let client_dir = temp_dir("exact-guard-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&client_dir, "Mods/a.package", b"AAAA");
    write_file(&client_dir, "Mods/extra.package", b"MINE");
    let pack = test_pack("sims4", &[("Mods/a.package", b"AAAA")]);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    let preview = preview_apply(&client_state, &pack).await;
    let apply = || pack_apply::apply_pack_exact_inner(&client_state, pack.clone(), preview.clone());

    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = true;
    assert!(apply().await.expect_err("sync").contains("sync is in progress"));
    assert!(pack_apply::revert_pack_apply_inner(&client_state, None).await.expect_err("sync").contains("sync is in progress"));
    client_state.lock().await.connections.get_mut(&peer_id).unwrap().is_syncing = false;

    client_state.lock().await.session_type = crate::state::SessionType::Host;
    assert!(apply().await.expect_err("hosting").contains("Stop hosting"));
    client_state.lock().await.session_type = crate::state::SessionType::Client;

    {
        let _restore = crate::commands::backup::try_begin_restoring().expect("no restore running");
        assert!(apply().await.expect_err("restore").contains("restore"));
    }

    let mut moved = preview.clone();
    moved.base_path = format!("{}-elsewhere", moved.base_path);
    let err = pack_apply::apply_pack_exact_inner(&client_state, pack.clone(), moved).await.expect_err("changed folder");
    assert!(err.contains("changed since the preview"), "unexpected error: {err}");
    assert!(file_exists(&client_dir, "Mods/extra.package"), "nothing renamed by a refused apply");

    // All guards clear: a client session is fine (the download step needs one).
    let result = apply().await.expect("apply");
    assert_eq!(result.disabled, 1);
    pack_apply::delete_record("sims4");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// A file the plan wanted to download but that was already there (the local
/// manifest was stale) was never written by the sync, so undo must leave it.
#[tokio::test]
async fn undo_never_deletes_a_file_the_sync_found_already_present_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("undo-present-host");
    let client_dir = temp_dir("undo-present-client");
    write_file(&host_dir, "Mods/same.package", b"SAME");
    write_file(&host_dir, "Mods/new.package", b"NEW");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    let plan = compute_plan(&client_state).await.expect("plan");
    assert_eq!(plan.actions.len(), 2);
    // The same mod gets copied in by hand after the compare.
    write_file(&client_dir, "Mods/same.package", b"SAME");
    run_sync_now(&client_state).await.expect("sync");

    let record = crate::commands::undo::read_record("sims4").expect("record");
    let added: Vec<_> = record.added.iter().map(|f| f.relative_path.as_str()).collect();
    assert_eq!(added, vec!["Mods/new.package"], "only what the sync wrote");
    undo(&client_state).await.expect("undo");
    assert!(!file_exists(&client_dir, "Mods/new.package"));
    assert_eq!(read_file(&client_dir, "Mods/same.package"), b"SAME", "the hand-copied file stays");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}
