//! End-to-end tests for the security fixes: they drive the real listener,
//! handshake and sync code with hostile peers (see `testutil.rs`; every test
//! holds `e2e_guard()`).

use crate::network::protocol::{self, Message};
use crate::state::{FileInfo, SyncAction, SyncPlan};
use crate::testutil::*;
use std::time::Duration;

async fn raw_hello(port: u16, pin: Option<&str>) -> Message {
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut s = crate::network::stream::PeerStream::tcp(tcp);
    let hello = Message::Hello {
        name: "Guesser".into(),
        version: "0.6.0".into(),
        pin: pin.map(str::to_string),
        supports_compression: false,
        game_id: Some("sims4".into()),
        node_id: None,
        crews: vec![],
        features: vec![],
    };
    protocol::send_message(&mut s, &hello).await.unwrap();
    protocol::recv_message(&mut s).await.unwrap()
}

fn error_text(m: Message) -> String {
    match m {
        Message::Error { message } => message,
        other => format!("{other:?}"),
    }
}

#[tokio::test]
async fn pin_guessing_is_locked_out_tcp() {
    let _g = e2e_guard().await;
    let dir = temp_dir("pin-guard-host");
    let host_state = make_state("sims4", &dir);
    set_host(&host_state, "Host").await;
    host_state.lock().await.session_pin = Some("54321".into());
    let port = start_tcp_host(host_state.clone()).await;

    for guess in ["11111", "22222", "33333"] {
        assert_eq!(error_text(raw_hello(port, Some(guess)).await), "Invalid PIN");
    }
    // Locked now: even the right PIN is refused from this address.
    let locked = error_text(raw_hello(port, Some("54321")).await);
    assert!(locked.contains("Too many wrong PIN attempts"), "{locked}");
    // A wrong PIN never reaches the handshake past this point, so nothing was added.
    assert!(host_state.lock().await.connections.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

/// An unauthenticated peer can't make the host buffer a 10 MB message, and
/// the host keeps serving everyone else.
#[tokio::test]
async fn oversized_hello_is_refused_and_the_host_stays_up_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("big-hello-host");
    let client_dir = temp_dir("big-hello-client");
    write_file(&host_dir, "Mods/a.package", b"AAAA");
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state.clone()).await;

    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut s = crate::network::stream::PeerStream::tcp(tcp);
    s.write_all(&(9_000_000u32).to_be_bytes()).await.unwrap();
    s.flush().await.unwrap();
    // The host hangs up instead of waiting for 9 MB.
    let closed = tokio::time::timeout(Duration::from_secs(5), s.wait_readable(Duration::from_secs(5))).await;
    assert!(matches!(closed, Ok(Ok(Some(false))) | Ok(Err(_))), "{closed:?}");

    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("a normal client still connects");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

fn receive(path: &str, content: &[u8]) -> SyncAction {
    SyncAction::ReceiveFromRemote(FileInfo {
        relative_path: path.into(),
        size: content.len() as u64,
        hash: sha256_hex(content),
        modified: 0,
        file_type: "CustomContent".into(),
    })
}

/// A malicious host serving a proxy DLL outside the content folders: it's
/// dropped from the compare, and even a plan that lists it (a pack, a crew
/// set, a bug) is refused at write time.
#[tokio::test]
async fn a_host_cannot_write_outside_the_content_folders_tcp() {
    let _g = e2e_guard().await;
    let client_dir = temp_dir("outside-client");
    let mut files = std::collections::HashMap::new();
    files.insert("dinput8.dll".to_string(), b"MZ-evil".to_vec());
    files.insert("Mods/ok.package".to_string(), b"OK".to_vec());
    let port = start_fake_old_host_tcp(files, Some("sims4".into())).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("plan");
    let paths: Vec<_> = plan.actions.iter().filter_map(|a| match a { SyncAction::ReceiveFromRemote(f) => Some(f.relative_path.as_str()), _ => None }).collect();
    assert_eq!(paths, vec!["Mods/ok.package"], "the DLL never makes it into the plan");

    // Force it into a plan anyway.
    {
        let mut s = client_state.lock().await;
        let base = s.active_game_path().unwrap();
        let conn = s.connections.get_mut(&peer_id).unwrap();
        let mut forced = SyncPlan { game_id: "sims4".into(), base_path: base, ..Default::default() };
        forced.actions = vec![receive("dinput8.dll", b"MZ-evil"), receive("Mods/ok.package", b"OK")];
        conn.sync_plan = Some(forced);
    }
    let err = run_sync_now(&client_state).await.expect_err("the DLL is reported as failed");
    assert!(err.contains("1 file"), "{err}");
    assert!(!file_exists(&client_dir, "dinput8.dll"), "never written outside Mods/Saves/...");
    assert_eq!(read_file(&client_dir, "Mods/ok.package"), b"OK");

    let _ = std::fs::remove_dir_all(&client_dir);
}

/// The host's FileHeader must match the size its file list promised.
#[tokio::test]
async fn a_host_cannot_send_more_than_its_file_list_said_tcp() {
    let _g = e2e_guard().await;
    let client_dir = temp_dir("size-client");
    let mut files = std::collections::HashMap::new();
    files.insert("Mods/big.package".to_string(), vec![b'x'; 4096]);
    let port = start_fake_old_host_tcp(files, Some("sims4".into())).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    {
        let mut s = client_state.lock().await;
        let base = s.active_game_path().unwrap();
        let conn = s.connections.get_mut(&peer_id).unwrap();
        let mut plan = SyncPlan { game_id: "sims4".into(), base_path: base, ..Default::default() };
        // The list claimed 10 bytes; the host streams 4096.
        let mut f = receive("Mods/big.package", &vec![b'x'; 4096]);
        if let SyncAction::ReceiveFromRemote(ref mut info) = f {
            info.size = 10;
        }
        plan.actions = vec![f];
        conn.sync_plan = Some(plan);
    }
    let err = run_sync_now(&client_state).await.expect_err("the oversized file fails");
    assert!(err.contains("1 file"), "{err}");
    assert!(!file_exists(&client_dir, "Mods/big.package"));
    assert!(no_leftover_temp_files(&client_dir));

    let _ = std::fs::remove_dir_all(&client_dir);
}
