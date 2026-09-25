//! End-to-end tests for session chat over the real handshake and message
//! loops (see `testutil.rs`; every test holds `e2e_guard()`).

use crate::commands::chat::send_chat_inner;
use crate::state::AppState;
use crate::testutil::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

async fn texts(state: &Arc<Mutex<AppState>>) -> Vec<String> {
    state.lock().await.chat.messages.iter().map(|m| m.text.clone()).collect()
}

/// Poll until every `want` line is in `state`'s chat log.
async fn wait_for_lines(state: &Arc<Mutex<AppState>>, want: &[&str]) {
    let ok = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let have = texts(state).await;
            if want.iter().all(|w| have.iter().any(|h| h == w)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(ok.is_ok(), "timed out waiting for {want:?}; log: {:?}", texts(state).await);
}

#[tokio::test]
async fn chat_between_host_and_client_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("chat-host");
    let client_dir = temp_dir("chat-client");
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    host_state.lock().await.local_display_name = "Alex".into();
    let port = start_tcp_host(host_state.clone()).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    assert!(client_state.lock().await.chat.available, "a new host offers chat");

    send_chat_inner(&host_state, "hello crew").await.expect("host send");
    send_chat_inner(&client_state, "hi\nthere").await.expect("client send");
    wait_for_lines(&client_state, &["tester joined", "hello crew", "hi there"]).await;
    wait_for_lines(&host_state, &["hi there"]).await;
    assert!(client_state.lock().await.chat.outbox.is_empty(), "delivered lines leave the outbox");

    let log = client_state.lock().await.chat.messages.clone();
    let seqs: Vec<u64> = log.iter().map(|m| m.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "ordered by host seq: {seqs:?}");
    assert!(log.iter().any(|m| m.system && m.text == "tester joined"));
    assert_eq!(log.iter().find(|m| m.text == "hello crew").unwrap().from, "Alex");

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// Chat polls share the stream with file transfers; a sync with chat going
/// on must still transfer every file intact.
#[tokio::test]
async fn chat_during_a_sync_never_corrupts_the_transfer_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("chat-sync-host");
    let client_dir = temp_dir("chat-sync-client");
    for i in 0..30 {
        write_file(&host_dir, &format!("Mods/m{i}.package"), format!("CONTENT-{i}-").repeat(2000).as_bytes());
    }
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state.clone()).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("plan");
    assert_eq!(plan.actions.len(), 30);
    let sync = spawn_sync(client_state.clone());
    for i in 0..5 {
        send_chat_inner(&client_state, &format!("client {i}")).await.expect("client send");
        send_chat_inner(&host_state, &format!("host {i}")).await.expect("host send");
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    sync.await.unwrap().expect("sync with chat running");
    for i in 0..30 {
        assert_eq!(read_file(&client_dir, &format!("Mods/m{i}.package")), format!("CONTENT-{i}-").repeat(2000).as_bytes());
    }
    wait_for_lines(&client_state, &["client 4", "host 4", "tester finished syncing 30 files"]).await;
    wait_for_lines(&host_state, &["client 0", "client 4", "tester finished syncing 30 files"]).await;

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn chat_happy_path_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("chat-iroh-host");
    let client_dir = temp_dir("chat-iroh-client");
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let (host_ep, client_ep) = iroh_pair().await;
    tokio::spawn(serve_one_iroh_connection(host_ep.clone(), host_state.clone()));
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_iroh(client_ep, &host_ep, client_state.clone(), &peer_id).await.expect("iroh connect");

    send_chat_inner(&client_state, "over the internet").await.expect("send");
    wait_for_lines(&host_state, &["over the internet"]).await;
    send_chat_inner(&host_state, "got it").await.expect("send");
    wait_for_lines(&client_state, &["got it"]).await;

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// An older host can't parse `ChatSync`: the client must never send it, and
/// the session must work exactly as before.
#[tokio::test]
async fn old_host_gets_no_chat_messages_and_still_syncs_tcp() {
    let _g = e2e_guard().await;
    let client_dir = temp_dir("chat-old-host-client");
    let mut files = std::collections::HashMap::new();
    files.insert("Mods/a.package".to_string(), b"AAAA".to_vec());
    let port = start_fake_old_host_tcp(files, Some("sims4".into())).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    assert!(!client_state.lock().await.chat.available);
    let err = send_chat_inner(&client_state, "anyone?").await.unwrap_err();
    assert!(err.contains("too old"), "{err}");
    // Give the idle loop time to (wrongly) poll; the fake host would then
    // fail to parse it and drop the connection before the sync.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    compute_plan(&client_state).await.expect("plan");
    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/a.package"), b"AAAA");

    let _ = std::fs::remove_dir_all(&client_dir);
}

#[tokio::test]
async fn chat_outside_a_session_is_refused() {
    let _g = e2e_guard().await;
    let dir = temp_dir("chat-none");
    let state = make_state("sims4", &dir);
    assert!(send_chat_inner(&state, "hello?").await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
