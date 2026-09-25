//! End-to-end tests for crews: real host + client `AppState`s over TCP and
//! local iroh, driving the real handshake (see `testutil.rs`, and hold
//! `e2e_guard()` for the whole body like every E2E test).

use crate::commands::crew;
use crate::crews;
use crate::state::AppState;
use crate::testutil::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

fn node(n: u8) -> String {
    crews::node_id_hex(&iroh::SecretKey::from_bytes(&[n; 32]).public())
}

async fn set_node(state: &Arc<Mutex<AppState>>, id: String) {
    state.lock().await.local_node_id = Some(id);
}

/// Host creates a crew and (optionally) publishes its folder as the set;
/// the client joins through the invite link. Returns the crew id.
async fn crew_pair(host: &Arc<Mutex<AppState>>, client: &Arc<Mutex<AppState>>, publish: bool) -> String {
    let c = crew::create_crew_inner(host, "Sunday Sims Crew", "Host").await.expect("create crew");
    if publish {
        crew::publish_crew_set_inner(host, &c.id, None, "Host").await.expect("publish");
    }
    let link = {
        let s = host.lock().await;
        crews::encode_invite(s.crews.get(&c.id).unwrap(), s.local_node_id.as_deref().unwrap(), "Host").unwrap()
    };
    let invite = crews::decode_invite(link.strip_prefix(crews::INVITE_PREFIX).unwrap(), |g| g == "sims4").unwrap();
    crew::join_crew_inner(client, invite, "Ann").await.expect("join crew");
    c.id
}

async fn crew_of(state: &Arc<Mutex<AppState>>, id: &str) -> crews::Crew {
    state.lock().await.crews.get(id).cloned().expect("crew")
}

async fn behind(state: &Arc<Mutex<AppState>>, id: &str) -> (bool, usize) {
    let st = crew::crew_status_inner(state, id).await.expect("crew status");
    (st.has_set, st.behind)
}

async fn pack_plan(state: &Arc<Mutex<AppState>>, pack: crate::state::ModPack) -> crate::state::SyncPlan {
    tokio::time::timeout(
        Duration::from_secs(15),
        crate::commands::modpack::compute_pack_sync_plan_inner(state, None, pack),
    )
    .await
    .expect("compute_pack_sync_plan timed out")
    .expect("pack plan")
}

#[tokio::test]
async fn crew_data_needs_iroh_and_then_catch_up_works() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("crew-host");
    let client_dir = temp_dir("crew-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    write_file(&host_dir, "Mods/b.package", b"BBBB");
    write_file(&host_dir, "Mods/c.package", b"CCCC");
    write_file(&client_dir, "Mods/a.package", b"AAAA");

    let (host_ep, client_ep) = iroh_pair().await;
    let host_state = make_state("sims4", &host_dir);
    set_node(&host_state, crews::node_id_hex(&host_ep.id())).await;
    let client_state = make_state("sims4", &client_dir);
    let client_id = crews::node_id_hex(&client_ep.id());
    set_node(&client_state, client_id.clone()).await;
    let crew_id = crew_pair(&host_state, &client_state, true).await;
    set_host(&host_state, "Host").await;

    // Over LAN TCP: the session works, crew data doesn't move either way.
    let port = start_tcp_host(host_state.clone()).await;
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("tcp connect");
    assert!(crew_of(&client_state, &crew_id).await.sets.is_empty(), "no crew set over unauthenticated TCP");
    assert!(crew_of(&client_state, &crew_id).await.last_host.is_none());
    assert!(!crew_of(&host_state, &crew_id).await.members.iter().any(|m| m.node_id == client_id), "host learns nothing over TCP");
    {
        let mut s = client_state.lock().await;
        s.connections.clear();
        s.session_type = crate::state::SessionType::None;
    }

    // Over iroh: the set arrives and the client catches up.
    serve_iroh_connections(host_ep.clone(), host_state.clone());
    connect_iroh_with_pin(client_ep, &host_ep, client_state.clone(), None).await.expect("iroh connect");
    let mine = crew_of(&client_state, &crew_id).await;
    let set = mine.sets.get("sims4").expect("set arrived over iroh").clone();
    assert_eq!(set.pack.files.len(), 3);
    assert_eq!(mine.last_host.clone().unwrap().node_id, crews::node_id_hex(&host_ep.id()));
    assert!(crew_of(&host_state, &crew_id).await.members.iter().any(|m| m.node_id == client_id && m.last_seen > 0));

    assert_eq!(behind(&client_state, &crew_id).await, (true, 2), "b and c are missing");
    let plan = pack_plan(&client_state, set.pack.clone()).await;
    assert_eq!(plan.actions.len(), 2);
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(behind(&client_state, &crew_id).await, (true, 0), "in sync with the crew");
    assert_eq!(read_file(&client_dir, "Mods/c.package"), b"CCCC");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// A host that isn't a member of our crew (e.g. anyone who got the crew id)
/// gets no crew id from us and can't push crew data to us.
#[tokio::test]
async fn crew_ids_go_only_to_hosts_that_are_members_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("crew-stranger-host");
    let client_dir = temp_dir("crew-stranger-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");

    let (host_ep, client_ep) = iroh_pair().await;
    let host_state = make_state("sims4", &host_dir);
    set_node(&host_state, crews::node_id_hex(&host_ep.id())).await;
    let client_state = make_state("sims4", &client_dir);
    set_node(&client_state, crews::node_id_hex(&client_ep.id())).await;
    let crew_id = crew_pair(&host_state, &client_state, true).await;
    // The client's copy no longer lists this host (say it was removed).
    {
        let mut s = client_state.lock().await;
        let host_id = crews::node_id_hex(&host_ep.id());
        crews::set_member_removed(s.crews.get_mut(&crew_id).unwrap(), &host_id, true, crate::utils::timestamp_now()).unwrap();
    }
    set_host(&host_state, "Host").await;
    serve_iroh_connections(host_ep.clone(), host_state.clone());
    connect_iroh_with_pin(client_ep, &host_ep, client_state.clone(), None).await.expect("connect");
    assert!(crew_of(&client_state, &crew_id).await.sets.is_empty(), "no data from a non-member");
    assert_eq!(crew_of(&host_state, &crew_id).await.members.len(), 1, "the host never heard our crew id");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn crew_member_still_needs_the_pin_and_the_right_game_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("crew-pin-host");
    let client_dir = temp_dir("crew-pin-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");

    let (host_ep, first_ep) = iroh_pair().await;
    let host_state = make_state("sims4", &host_dir);
    set_node(&host_state, crews::node_id_hex(&host_ep.id())).await;
    let client_state = make_state("sims4", &client_dir);
    set_node(&client_state, node(2)).await;
    let crew_id = crew_pair(&host_state, &client_state, true).await;
    set_host(&host_state, "Host").await;
    host_state.lock().await.session_pin = Some("43210".into());
    serve_iroh_connections(host_ep.clone(), host_state.clone());
    let only_host = |c: &crews::Crew| c.members.len() == 1;

    // No PIN: refused, and the host learns nothing about the member.
    let err = connect_iroh_with_pin(first_ep, &host_ep, client_state.clone(), None).await.expect_err("no PIN");
    assert!(err.contains("Invalid PIN"), "{err}");
    assert!(only_host(&crew_of(&host_state, &crew_id).await));
    assert!(crew_of(&client_state, &crew_id).await.sets.is_empty(), "no crew set without the PIN");

    // Right PIN but the wrong game selected: refused before any crew data too.
    client_state.lock().await.active_game = "ets2".into();
    let err = connect_iroh_with_pin(iroh_client().await, &host_ep, client_state.clone(), Some("43210".into())).await.expect_err("wrong game");
    assert_eq!(crate::network::protocol::parse_wrong_game(&err), Some("sims4"), "{err}");
    assert!(crew_of(&client_state, &crew_id).await.sets.is_empty());
    assert!(only_host(&crew_of(&host_state, &crew_id).await));

    // Right game and PIN: in, and the set arrives.
    client_state.lock().await.active_game = "sims4".into();
    connect_iroh_with_pin(iroh_client().await, &host_ep, client_state.clone(), Some("43210".into())).await.expect("connects with the PIN");
    assert!(crew_of(&client_state, &crew_id).await.sets.contains_key("sims4"));
    assert!(!only_host(&crew_of(&host_state, &crew_id).await));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn removed_member_gets_no_crew_data_but_the_pin_decides_the_session_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("crew-removed-host");
    let client_dir = temp_dir("crew-removed-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");

    let (host_ep, client_ep) = iroh_pair().await;
    let client_id = crews::node_id_hex(&client_ep.id());
    let host_state = make_state("sims4", &host_dir);
    set_node(&host_state, crews::node_id_hex(&host_ep.id())).await;
    let client_state = make_state("sims4", &client_dir);
    set_node(&client_state, client_id.clone()).await;
    let crew_id = crew_pair(&host_state, &client_state, true).await;
    {
        let mut s = host_state.lock().await;
        let c = s.crews.get_mut(&crew_id).unwrap();
        let ann = crews::CrewMember { node_id: client_id.clone(), name: "Ann".into(), updated_at: 1, removed: false, last_seen: 0 };
        crews::merge_member(&mut c.members, ann);
        crews::set_member_removed(c, &client_id, true, 10).unwrap();
    }
    set_host(&host_state, "Host").await;
    serve_iroh_connections(host_ep.clone(), host_state.clone());
    connect_iroh_with_pin(client_ep, &host_ep, client_state.clone(), None).await.expect("no PIN set, so the session goes ahead");
    let mine = crew_of(&client_state, &crew_id).await;
    assert!(mine.sets.is_empty() && mine.last_host.is_none(), "a removed member gets no crew data");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// Anyone who saw a crew's id could forge an "invite" naming themselves as
/// the inviter; for a crew we're already in, that must not add a member.
#[tokio::test]
async fn a_forged_invite_for_a_crew_we_are_in_adds_nobody() {
    let _g = e2e_guard().await;
    let dir = temp_dir("crew-forged");
    let host_state = make_state("sims4", &dir);
    set_node(&host_state, node(1)).await;
    let client_state = make_state("sims4", &dir);
    set_node(&client_state, node(2)).await;
    let crew_id = crew_pair(&host_state, &client_state, false).await;
    let before = crew_of(&client_state, &crew_id).await.members.len();
    let forged = crews::CrewInvite { v: 1, id: crew_id.clone(), name: "Crew".into(), games: vec!["sims4".into()], from_node: node(66), from_name: "Host".into() };
    crew::join_crew_inner(&client_state, forged, "Ann").await.expect("join returns the crew");
    let after = crew_of(&client_state, &crew_id).await.members;
    assert_eq!(after.len(), before);
    assert!(!after.iter().any(|m| m.node_id == node(66)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn crew_happy_path_iroh_uses_authenticated_ids() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("crew-iroh-host");
    let client_dir = temp_dir("crew-iroh-client");
    write_file(&host_dir, "Mods/b.package", b"BBBB");

    let (host_ep, client_ep) = iroh_pair().await;
    let host_state = make_state("sims4", &host_dir);
    set_node(&host_state, crews::node_id_hex(&host_ep.id())).await;
    let client_state = make_state("sims4", &client_dir);
    // The client *claims* someone else's id; over iroh the host must record
    // the id the QUIC handshake proved instead.
    set_node(&client_state, node(9)).await;
    let crew_id = crew_pair(&host_state, &client_state, true).await;

    set_host(&host_state, "Host").await;
    tokio::spawn(serve_one_iroh_connection(host_ep.clone(), host_state.clone()));
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    let client_id = crews::node_id_hex(&client_ep.id());
    connect_client_iroh(client_ep, &host_ep, client_state.clone(), &peer_id).await.expect("iroh connect");

    let host_members = crew_of(&host_state, &crew_id).await.members;
    assert!(host_members.iter().any(|m| m.node_id == client_id), "authenticated id recorded");
    assert!(!host_members.iter().any(|m| m.node_id == node(9) && m.last_seen > 0), "claimed id ignored over iroh");
    let mine = crew_of(&client_state, &crew_id).await;
    assert_eq!(mine.last_host.clone().unwrap().node_id, crews::node_id_hex(&host_ep.id()));

    let set = mine.sets["sims4"].clone();
    assert_eq!(behind(&client_state, &crew_id).await, (true, 1));
    pack_plan(&client_state, set.pack).await;
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(behind(&client_state, &crew_id).await, (true, 0));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}


#[tokio::test]
async fn crew_member_syncs_normally_with_an_old_host_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("crew-old-host-unused");
    let client_dir = temp_dir("crew-old-host-client");
    let host_state = make_state("sims4", &host_dir);
    set_node(&host_state, node(1)).await;
    let client_state = make_state("sims4", &client_dir);
    set_node(&client_state, node(2)).await;
    let crew_id = crew_pair(&host_state, &client_state, false).await;

    let mut files = std::collections::HashMap::new();
    files.insert("Mods/legit.package".to_string(), b"LEGIT".to_vec());
    // A 0.5.6-shaped Welcome: game_id, but no node id or crews.
    let port = start_fake_old_host_tcp(files, Some("sims4".into())).await;
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    let plan = compute_plan(&client_state).await.expect("plan");
    assert_eq!(plan.actions.len(), 1);
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/legit.package"), b"LEGIT");
    assert!(crew_of(&client_state, &crew_id).await.last_host.is_none(), "an old host can't be a crew host");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn old_client_without_crew_fields_gets_a_plain_welcome_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("crew-old-client-host");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    let host_state = make_state("sims4", &host_dir);
    set_node(&host_state, node(1)).await;
    let c = crew::create_crew_inner(&host_state, "Crew", "Host").await.unwrap();
    crew::publish_crew_set_inner(&host_state, &c.id, None, "Host").await.unwrap();
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state.clone()).await;

    // A hand-rolled 0.5.6 Hello: no node_id, no crews.
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut s = crate::network::stream::PeerStream::tcp(tcp);
    let hello = br#"{"Hello":{"name":"Old","version":"0.5.6","pin":null,"supports_compression":true,"game_id":"sims4"}}"#;
    s.write_all(&(hello.len() as u32).to_be_bytes()).await.unwrap();
    s.write_all(hello).await.unwrap();
    s.flush().await.unwrap();
    match crate::network::protocol::recv_message(&mut s).await.unwrap() {
        crate::network::protocol::Message::Welcome { game_id, crews, .. } => {
            assert_eq!(game_id.as_deref(), Some("sims4"));
            assert!(crews.is_empty(), "no crew data for a client that didn't ask");
        }
        other => panic!("expected Welcome, got {other:?}"),
    }
    assert_eq!(crew_of(&host_state, &c.id).await.members.len(), 1, "an old client isn't added to any crew");

    let _ = std::fs::remove_dir_all(&host_dir);
}
