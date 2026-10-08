//! End-to-end tests for "Share as a link" (`crate::source_links`): a real
//! host and client (see `testutil.rs`; every test holds `e2e_guard()`).

use crate::network::protocol::{self, Message};
use crate::source_links::LinkStatus;
use crate::state::SyncAction;
use crate::testutil::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Links live in the (test) config dir and are shared by every test in the
/// binary: clear them even when an assertion fails.
struct ClearLinks(&'static str);
impl Drop for ClearLinks {
    fn drop(&mut self) {
        let _ = crate::source_links::save(self.0, &[]);
    }
}

fn host_with_links(dir: &std::path::Path) -> ClearLinks {
    write_file(dir, "Mods/free.package", b"FREE");
    write_file(dir, "Mods/solo.package", b"SOLO");
    write_file(dir, "Mods/Creator/a.package", b"AAAA");
    write_file(dir, "Mods/Creator/sub/b.package", b"BBBB");
    let guard = ClearLinks("sims4");
    crate::source_links::set_link("sims4", "Mods/solo.package", "https://solo.example/get", Some("Solo")).unwrap();
    crate::source_links::set_link("sims4", "Mods/creator", "https://creator.example", None).unwrap();
    guard
}

/// What the host answers when this client asks for `path` directly, over
/// its live connection.
async fn request_directly(client_state: &Arc<Mutex<crate::state::AppState>>, peer_id: &str, path: &str) -> Message {
    let stream = client_state.lock().await.connections.get(peer_id).unwrap().stream.clone();
    let mut s = stream.lock().await;
    protocol::send_message(&mut s, &Message::FileRequest { path: path.to_string() }).await.unwrap();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), protocol::recv_message(&mut s)).await.expect("host answers").unwrap();
        if !matches!(msg, Message::GameInfoExchange { .. } | Message::Ping) {
            return msg;
        }
    }
}

#[tokio::test]
async fn linked_files_are_listed_never_sent_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("links-host");
    let client_dir = temp_dir("links-client");
    let _links = host_with_links(&host_dir);
    // The friend has another version of one file in the linked folder.
    write_file(&client_dir, "Mods/Creator/a.package", b"MINE");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    // The host's manifest leaves the linked files out.
    {
        let s = client_state.lock().await;
        let conn = s.connections.get(&peer_id).unwrap();
        let mut keys: Vec<&String> = conn.remote_manifest.as_ref().unwrap().files.keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["Mods/free.package"]);
        assert_eq!(conn.remote_links.len(), 3);
    }

    let plan = compute_plan(&client_state).await.expect("plan");
    assert_eq!(plan.actions.len(), 1, "{:?}", plan.actions);
    assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/free.package"));
    assert_eq!(plan.total_bytes, 4);
    let rows: Vec<(&str, LinkStatus, &str)> = plan.source_links.iter().map(|i| (i.path.as_str(), i.status, i.url.as_str())).collect();
    assert_eq!(
        rows,
        vec![
            ("Mods/Creator/a.package", LinkStatus::Different, "https://creator.example"),
            ("Mods/Creator/sub/b.package", LinkStatus::Missing, "https://creator.example"),
            ("Mods/solo.package", LinkStatus::Missing, "https://solo.example/get"),
        ]
    );
    assert_eq!(plan.source_links[2].label.as_deref(), Some("Solo"));

    // Asking for a linked file by name is refused; the connection carries on.
    for path in ["Mods/solo.package", "Mods/Creator/sub/b.package", "mods/creator/A.package"] {
        match request_directly(&client_state, &peer_id, path).await {
            Message::Error { message } => assert_eq!(message, crate::source_links::REFUSAL),
            other => panic!("{path}: expected a refusal, got {other:?}"),
        }
    }

    run_sync_now(&client_state).await.expect("sync");
    assert_eq!(read_file(&client_dir, "Mods/free.package"), b"FREE");
    assert_eq!(read_file(&client_dir, "Mods/Creator/a.package"), b"MINE", "the friend's own copy stays");
    assert!(!file_exists(&client_dir, "Mods/solo.package"));
    assert!(!file_exists(&client_dir, "Mods/Creator/sub/b.package"));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// Internet joins go through the same host code.
#[tokio::test]
async fn linked_files_are_listed_never_sent_iroh() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("links-iroh-host");
    let client_dir = temp_dir("links-iroh-client");
    let _links = host_with_links(&host_dir);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let (host_ep, client_ep) = iroh_pair().await;
    tokio::spawn(serve_one_iroh_connection(host_ep.clone(), host_state));
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_iroh(client_ep, &host_ep, client_state.clone(), &peer_id).await.expect("iroh connect");

    let plan = compute_plan(&client_state).await.expect("plan");
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.source_links.len(), 3);
    assert!(plan.source_links.iter().all(|i| i.status == LinkStatus::Missing));
    assert!(matches!(request_directly(&client_state, &peer_id, "Mods/Creator/a.package").await, Message::Error { .. }));

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// Only links tell a file apart: with none, the host shares everything as before.
#[tokio::test]
async fn without_links_everything_syncs_as_before_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("links-none-host");
    let client_dir = temp_dir("links-none-client");
    let links = host_with_links(&host_dir);
    drop(links);

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");

    let plan = compute_plan(&client_state).await.expect("plan");
    assert_eq!(plan.actions.len(), 4);
    assert!(plan.source_links.is_empty());

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}
