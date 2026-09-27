//! End-to-end: a crew takes turns on a Sims 4 save over local iroh. The host
//! shares it, a friend takes it (their copy becomes the host's), plays, and
//! gives it back (the host's becomes theirs); a normal sync never touches it,
//! and nobody can take it while it's being played. Hold `e2e_guard()`.

use crate::commands::crew;
use crate::crews;
use crate::handoff::SharedSave;
use crate::network::handoff_net;
use crate::state::AppState;
use crate::testutil::*;
use std::sync::Arc;
use tokio::sync::Mutex;

const UNIT: &str = "saves/Slot_00000001";

async fn record(state: &Arc<Mutex<AppState>>, crew: &str) -> SharedSave {
    handoff_net::record(&*state.lock().await, crew, "sims4", UNIT).expect("record")
}

async fn node_of(state: &Arc<Mutex<AppState>>) -> String {
    state.lock().await.local_node_id.clone().unwrap()
}

#[tokio::test]
async fn a_crew_takes_turns_on_a_save_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("handoff-host");
    let client_dir = temp_dir("handoff-client");
    write_file(&host_dir, "Mods/a.package", b"MOD");
    write_file(&host_dir, "Saves/Slot_00000001.save", b"HOST-V1");
    write_file(&host_dir, "Saves/Slot_00000001.save.ver0", b"HOST-OLD");
    write_file(&client_dir, "Mods/a.package", b"MOD");
    // The friend's stale copy from an earlier game night.
    write_file(&client_dir, "Saves/Slot_00000001.save", b"CLIENT-STALE");
    write_file(&client_dir, "Saves/Slot_00000001.save.ver4", b"LEFTOVER");

    let (host_ep, client_ep) = iroh_pair().await;
    let host_state = make_state("sims4", &host_dir);
    host_state.lock().await.local_node_id = Some(crews::node_id_hex(&host_ep.id()));
    let client_state = make_state("sims4", &client_dir);
    client_state.lock().await.local_node_id = Some(crews::node_id_hex(&client_ep.id()));
    let c = crew::create_crew_inner(&host_state, "Legacy Crew", "Host").await.expect("crew");
    let link = {
        let s = host_state.lock().await;
        crews::encode_invite(s.crews.get(&c.id).unwrap(), s.local_node_id.as_deref().unwrap(), "Host").unwrap()
    };
    let invite = crews::decode_invite(link.strip_prefix(crews::INVITE_PREFIX).unwrap(), |g| g == "sims4").unwrap();
    crew::join_crew_inner(&client_state, invite, "Ann").await.expect("join");
    set_host(&host_state, "Host").await;
    crate::commands::files::scan_files_inner(&host_state, None, true).await.unwrap();
    crate::commands::files::scan_files_inner(&client_state, None, true).await.unwrap();
    serve_iroh_connections(host_ep.clone(), host_state.clone());
    let host_peer = connect_iroh_with_pin(client_ep, &host_ep, client_state.clone(), None).await.expect("connect");
    assert!(client_state.lock().await.host_proven);

    let events = null_events();
    crate::commands::handoff::share_inner(&host_state, &events, &c.id, "sims4", UNIT).await.expect("share");
    assert!(crate::commands::handoff::share_inner(&host_state, &events, &c.id, "sims4", UNIT).await.is_err(), "already shared");

    // Take: the friend's copy becomes exactly the host's.
    let fetched = handoff_net::take(&client_state, &events, &c.id, "sims4", UNIT).await.expect("take");
    assert_eq!(fetched, 2);
    assert_eq!(read_file(&client_dir, "Saves/Slot_00000001.save"), b"HOST-V1");
    assert_eq!(read_file(&client_dir, "Saves/Slot_00000001.save.ver0"), b"HOST-OLD");
    assert!(!file_exists(&client_dir, "Saves/Slot_00000001.save.ver4"), "a file the host's save doesn't have goes");
    let ann = node_of(&client_state).await;
    assert_eq!(record(&host_state, &c.id).await.holder, ann);
    assert_eq!(record(&client_state, &c.id).await.holder, ann, "the reply carried the new record");

    // A normal sync never touches a shared save, even when the host's differs.
    write_file(&host_dir, "Saves/Slot_00000001.save", b"HOST-SHOULD-NOT-SPREAD");
    crate::commands::files::scan_files_inner(&host_state, None, true).await.unwrap();
    crate::network::transfer::refresh_remote_manifest(&client_state, &host_peer).await.expect("manifest");
    let plan = crate::commands::sync::compute_sync_plan_inner(&client_state, None).await.expect("plan");
    assert!(plan.actions.is_empty(), "shared save left out of the sync plan: {:?}", plan.actions);
    write_file(&host_dir, "Saves/Slot_00000001.save", b"HOST-V1");
    crate::commands::files::scan_files_inner(&host_state, None, true).await.unwrap();

    // The friend plays: the save changes, a backup rotates.
    write_file(&client_dir, "Saves/Slot_00000001.save", b"ANN-PLAYED");
    write_file(&client_dir, "Saves/Slot_00000001.save.ver1", b"ANN-VER1");
    std::fs::remove_file(client_dir.join("Saves/Slot_00000001.save.ver0")).unwrap();
    crate::commands::files::scan_files_inner(&client_state, None, true).await.unwrap();

    // Give: the host's copy becomes exactly the friend's.
    handoff_net::give(&client_state, &events, &c.id, "sims4", UNIT).await.expect("give");
    assert_eq!(read_file(&host_dir, "Saves/Slot_00000001.save"), b"ANN-PLAYED");
    assert_eq!(read_file(&host_dir, "Saves/Slot_00000001.save.ver1"), b"ANN-VER1");
    assert!(!file_exists(&host_dir, "Saves/Slot_00000001.save.ver0"));
    assert!(no_leftover_temp_files(&host_dir));
    let host_node = node_of(&host_state).await;
    assert_eq!(record(&host_state, &c.id).await.holder, host_node);
    assert_eq!(record(&client_state, &c.id).await.holder, host_node);

    // Only the holder can give; nobody takes a save that's being played.
    assert!(handoff_net::give(&client_state, &events, &c.id, "sims4", UNIT).await.unwrap_err().contains("has this save"));
    {
        let mut st = host_state.lock().await;
        let next = handoff_net::record(&st, &c.id, "sims4", UNIT).unwrap().bumped(crate::utils::timestamp_now(), |s| s.playing = true);
        handoff_net::put(&mut st, &c.id, next).unwrap();
    }
    assert!(handoff_net::take(&client_state, &events, &c.id, "sims4", UNIT).await.unwrap_err().contains("playing"));
    assert_eq!(read_file(&client_dir, "Saves/Slot_00000001.save"), b"ANN-PLAYED", "a refused take changes nothing");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn handoff_is_refused_over_lan_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("handoff-lan-host");
    let client_dir = temp_dir("handoff-lan-client");
    write_file(&host_dir, "Saves/Slot_00000001.save", b"HOST");
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    crate::commands::files::scan_files_inner(&host_state, None, true).await.unwrap();
    let port = start_tcp_host(host_state.clone()).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    let st = client_state.lock().await;
    assert!(st.handoff_available && !st.host_proven);
    drop(st);
    let err = handoff_net::take(&client_state, &null_events(), "0".repeat(32).as_str(), "sims4", UNIT).await.unwrap_err();
    assert_eq!(err, handoff_net::NOT_PROVEN);
    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}
