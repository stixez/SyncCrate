//! End-to-end: content outside the game folder (`ContentType::roots`).
//! Valheim keeps its mods in the game folder and its worlds in AppData;
//! both sync, a friend without the worlds folder is told why, and a world
//! can be handed over like any shared save. Hold `e2e_guard()`.

use crate::commands::crew;
use crate::crews;
use crate::network::handoff_net;
use crate::state::AppState;
use crate::testutil::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A Valheim game folder plus a separate worlds folder, pinned for this state.
fn valheim(label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let game = temp_dir(&format!("{label}-game"));
    let worlds = temp_dir(&format!("{label}-worlds"));
    crate::utils::pin_extra_roots(&game.to_string_lossy(), HashMap::from([("@worlds".to_string(), worlds.clone())]));
    (game, worlds)
}

async fn scan(state: &Arc<Mutex<AppState>>) {
    crate::commands::files::scan_files_inner(state, None, true).await.expect("scan");
}

#[test]
fn external_paths_resolve_through_safe_join() {
    let (game, worlds) = valheim("roots-unit");
    let base = game.to_string_lossy().to_string();
    std::fs::write(worlds.join("Midgard.db"), b"W").unwrap();
    let p = crate::utils::safe_join(&base, "@worlds/Midgard.db").unwrap();
    assert!(p.starts_with(std::fs::canonicalize(&worlds).unwrap()) || p.starts_with(&worlds));
    assert!(crate::utils::safe_join(&base, "@worlds/../escape.db").is_err());
    assert!(crate::utils::safe_join(&base, "@nope/x.db").unwrap_err().contains("isn't on this PC"));
    assert_eq!(crate::utils::content_path(&base, "@worlds/Midgard.db").unwrap(), worlds.join("Midgard.db"));
    assert!(crate::utils::content_path(&base, "@nope/x").is_none());
    let def = crate::registry::load_registry().games.into_iter().find(|g| g.id == "valheim").unwrap();
    let ct = def.content_types.iter().find(|c| c.id == "worlds").unwrap();
    assert_eq!(crate::utils::ct_dir(&base, ct).unwrap(), worlds);
    assert_eq!(crate::utils::manifest_path(&base, ct, &worlds.join("Midgard.fwl")).as_deref(), Some("@worlds/Midgard.fwl"));
    // Spelled with backslashes and "./", the prefix still resolves (it was
    // written into the game folder as a literal "@worlds" folder).
    assert!(crate::utils::content_path(&base, ".\\@worlds\\Midgard.db").is_some_and(|p| p.starts_with(&worlds)));
    assert!(crate::registry::is_external_path(".\\@worlds\\Midgard.db"));
    // A picked folder that's gone (unplugged drive) is not swapped for another.
    let picked = HashMap::from([("worlds".to_string(), worlds.join("gone").to_string_lossy().to_string())]);
    let mut def_cts = def.content_types.clone();
    for c in &mut def_cts {
        if c.id == "worlds" {
            c.roots = vec![worlds.to_string_lossy().to_string()];
        }
    }
    assert!(crate::utils::resolve_roots(&def_cts, &picked).is_empty());
    assert_eq!(crate::utils::resolve_roots(&def_cts, &HashMap::new()).get("@worlds"), Some(&worlds));
    // Other games' folders never resolve against this one.
    assert!(crate::utils::safe_join(&temp_dir("roots-other").to_string_lossy(), "@worlds/Midgard.db").is_err());
    let _ = std::fs::remove_dir_all(&game);
    let _ = std::fs::remove_dir_all(&worlds);
}

#[tokio::test]
async fn worlds_outside_the_game_folder_sync_tcp() {
    let _g = e2e_guard().await;
    let (host_game, host_worlds) = valheim("roots-host");
    let (client_game, client_worlds) = valheim("roots-client");
    write_file(&host_game, "BepInEx/plugins/Mod.dll", b"MOD");
    write_file(&host_worlds, "Midgard.db", b"WORLD");
    write_file(&host_worlds, "Midgard.fwl", b"META");
    // Valheim's own rolling backups and unrelated files stay put.
    write_file(&host_worlds, "Midgard_backup_auto-20240101.db", b"OLD");
    write_file(&host_worlds, "notes.txt", b"x");

    let host_state = make_state("valheim", &host_game);
    set_host(&host_state, "Host").await;
    scan(&host_state).await;
    {
        let s = host_state.lock().await;
        let mut keys: Vec<&String> = s.local_manifest.files.keys().collect();
        keys.sort();
        assert_eq!(keys, ["@worlds/Midgard.db", "@worlds/Midgard.fwl", "BepInEx/plugins/Mod.dll"]);
    }
    let port = start_tcp_host(host_state.clone()).await;
    let client_state = make_state("valheim", &client_game);
    scan(&client_state).await;
    let peer = new_peer_id();
    mark_pending_client(&client_state, &peer).await;
    connect_client_tcp(client_state.clone(), port, &peer).await.expect("connect");
    let plan = compute_plan(&client_state).await.expect("plan");
    assert_eq!(plan.actions.len(), 3, "{:?}", plan.actions);
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(std::fs::read(client_worlds.join("Midgard.db")).unwrap(), b"WORLD");
    assert_eq!(std::fs::read(client_worlds.join("Midgard.fwl")).unwrap(), b"META");
    assert_eq!(read_file(&client_game, "BepInEx/plugins/Mod.dll"), b"MOD");
    assert!(!client_game.join("@worlds").exists(), "nothing lands in the game folder under the prefix");
    assert!(no_leftover_temp_files(&client_worlds));

    // A friend who never ran the game has no worlds folder: the worlds are
    // skipped with a reason, the mods still come.
    let lonely_game = temp_dir("roots-lonely-game");
    crate::utils::pin_extra_roots(&lonely_game.to_string_lossy(), HashMap::new());
    let lonely = make_state("valheim", &lonely_game);
    scan(&lonely).await;
    let peer2 = new_peer_id();
    mark_pending_client(&lonely, &peer2).await;
    connect_client_tcp(lonely.clone(), port, &peer2).await.expect("connect");
    let plan = compute_plan(&lonely).await.expect("plan");
    assert_eq!(plan.actions.len(), 1, "only the mod: {:?}", plan.actions);
    assert!(plan.notice.as_deref().is_some_and(|n| n.contains("folder outside the game")), "{:?}", plan.notice);

    for d in [&host_game, &host_worlds, &client_game, &client_worlds, &lonely_game] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[tokio::test]
async fn a_picked_folder_must_be_a_save_folder() {
    let _g = e2e_guard().await;
    let (game, worlds) = valheim("roots-pick");
    let state = make_state("valheim", &game);
    let mut st = state.lock().await;
    let pick = |st: &mut AppState, p: &Path| crate::commands::files::set_extra_folder_inner(st, "valheim", "worlds", Some(p.to_string_lossy().to_string()));
    // The game folder, a folder inside it, or one holding it: scanned twice.
    assert!(pick(&mut st, &game).unwrap_err().contains("overlaps"));
    std::fs::create_dir_all(game.join("BepInEx")).unwrap();
    assert!(pick(&mut st, &game.join("BepInEx")).unwrap_err().contains("overlaps"));
    assert!(pick(&mut st, game.parent().unwrap()).unwrap_err().contains("overlaps"));
    if let Some(home) = dirs::home_dir() {
        assert!(pick(&mut st, &home).unwrap_err().contains("user folder"));
    }
    assert!(pick(&mut st, &game.join("nope")).unwrap_err().contains("isn't a folder"));
    // A real worlds folder is fine (and remembered), and can be forgotten again.
    pick(&mut st, &worlds).expect("a worlds folder");
    assert!(crate::commands::files::extra_folder_overrides("valheim").contains_key("worlds"));
    crate::commands::files::set_extra_folder_inner(&mut st, "valheim", "worlds", None).unwrap();
    assert!(crate::commands::files::extra_folder_overrides("valheim").is_empty());
    // Only external folders can be changed, and not mid-session.
    assert!(crate::commands::files::set_extra_folder_inner(&mut st, "valheim", "plugins", Some(worlds.to_string_lossy().to_string())).is_err());
    st.session_type = crate::state::SessionType::Host;
    assert!(pick(&mut st, &worlds).unwrap_err().contains("Disconnect"));
    drop(st);
    let _ = std::fs::remove_dir_all(&game);
    let _ = std::fs::remove_dir_all(&worlds);
}

/// The host's file list as a raw client with these Hello features sees it.
async fn host_manifest_for(port: u16, features: Vec<String>) -> Vec<String> {
    use crate::network::protocol::{self, Message};
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut s = crate::network::stream::PeerStream::tcp(tcp);
    let hello = Message::Hello {
        name: "Old friend".into(),
        version: "0.7.0".into(),
        pin: None,
        supports_compression: false,
        game_id: Some("valheim".into()),
        node_id: None,
        crews: vec![],
        features,
    };
    protocol::send_message(&mut s, &hello).await.unwrap();
    assert!(matches!(protocol::recv_message(&mut s).await.unwrap(), Message::Welcome { .. }));
    protocol::send_message(&mut s, &Message::ManifestRequest).await.unwrap();
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(15), protocol::recv_message(&mut s)).await.expect("host answers").unwrap();
        if let Message::ManifestResponse { manifest, .. } = msg {
            let _ = protocol::send_message(&mut s, &Message::Disconnect).await;
            let mut keys: Vec<String> = manifest.files.into_keys().collect();
            keys.sort();
            return keys;
        }
    }
}

#[tokio::test]
async fn an_older_friend_never_gets_paths_it_would_misplace_tcp() {
    let _g = e2e_guard().await;
    let (game, worlds) = valheim("roots-old");
    write_file(&game, "BepInEx/plugins/Mod.dll", b"MOD");
    write_file(&worlds, "Midgard.db", b"WORLD");
    let host_state = make_state("valheim", &game);
    set_host(&host_state, "Host").await;
    scan(&host_state).await;
    let port = start_tcp_host(host_state.clone()).await;
    // 0.7.0 would have written "@worlds/Midgard.db" into its game folder.
    assert_eq!(host_manifest_for(port, vec![]).await, ["BepInEx/plugins/Mod.dll"]);
    assert_eq!(host_manifest_for(port, vec![crate::registry::EXTERNAL_FOLDERS_FEATURE.into()]).await, ["@worlds/Midgard.db", "BepInEx/plugins/Mod.dll"]);
    let _ = std::fs::remove_dir_all(&game);
    let _ = std::fs::remove_dir_all(&worlds);
}

#[tokio::test]
async fn a_world_outside_the_game_folder_is_handed_over_iroh() {
    let _g = e2e_guard().await;
    let (host_game, host_worlds) = valheim("roots-ho-host");
    let (client_game, client_worlds) = valheim("roots-ho-client");
    write_file(&host_worlds, "Midgard.db", b"HOST-WORLD");
    write_file(&host_worlds, "Midgard.fwl", b"META");
    write_file(&host_worlds, "Midgard.db.old", b"HOST-OLD");

    let (host_ep, client_ep) = iroh_pair().await;
    let host_state = make_state("valheim", &host_game);
    host_state.lock().await.local_node_id = Some(crews::node_id_hex(&host_ep.id()));
    let client_state = make_state("valheim", &client_game);
    client_state.lock().await.local_node_id = Some(crews::node_id_hex(&client_ep.id()));
    let c = crew::create_crew_inner(&host_state, "Vikings", "Host").await.expect("crew");
    let link = {
        let s = host_state.lock().await;
        crews::encode_invite(s.crews.get(&c.id).unwrap(), s.local_node_id.as_deref().unwrap(), "Host").unwrap()
    };
    let invite = crews::decode_invite(crews::invite_payload(&link), |g| g == "valheim").unwrap();
    crew::join_crew_inner(&client_state, invite, "Ann").await.expect("join");
    set_host(&host_state, "Host").await;
    scan(&host_state).await;
    scan(&client_state).await;
    serve_iroh_connections(host_ep.clone(), host_state.clone());
    connect_iroh_with_pin(client_ep, &host_ep, client_state.clone(), None).await.expect("connect");

    let events = null_events();
    let unit = "worlds/Midgard";
    crate::commands::handoff::share_inner(&host_state, &events, &c.id, "valheim", unit).await.expect("share");
    handoff_net::take(&client_state, &events, &c.id, "valheim", unit).await.expect("take");
    assert_eq!(std::fs::read(client_worlds.join("Midgard.db")).unwrap(), b"HOST-WORLD");
    assert_eq!(std::fs::read(client_worlds.join("Midgard.db.old")).unwrap(), b"HOST-OLD");

    std::fs::write(client_worlds.join("Midgard.db"), b"ANN-WORLD").unwrap();
    std::fs::remove_file(client_worlds.join("Midgard.db.old")).unwrap();
    handoff_net::give(&client_state, &events, &c.id, "valheim", unit).await.expect("give");
    assert_eq!(std::fs::read(host_worlds.join("Midgard.db")).unwrap(), b"ANN-WORLD");
    assert!(!host_worlds.join("Midgard.db.old").exists());
    assert!(no_leftover_temp_files(&host_worlds));
    assert!(!Path::new(&host_game).join("@worlds").exists());

    for d in [&host_game, &host_worlds, &client_game, &client_worlds] {
        let _ = std::fs::remove_dir_all(d);
    }
}
