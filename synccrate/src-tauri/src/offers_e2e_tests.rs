//! End-to-end: a friend offers files, the host accepts some, only those
//! arrive; and a host never takes a file it didn't accept.

use crate::commands::offers::{decide_offer_inner, offer_files_inner};
use crate::network::protocol::{self, Message};
use crate::offers::OfferState;
use crate::state::AppState;
use crate::testutil::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

async fn until(what: &str, mut ok: impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>) {
    let done = tokio::time::timeout(Duration::from_secs(15), async {
        while !ok().await {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await;
    assert!(done.is_ok(), "timed out waiting for {what}");
}

fn host_offer_len(host: &Arc<Mutex<AppState>>) -> impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>> {
    let host = host.clone();
    move || {
        let host = host.clone();
        Box::pin(async move { host.lock().await.offers_in.values().any(|o| !o.files.is_empty()) })
    }
}

#[tokio::test]
async fn friend_offers_and_host_takes_only_what_it_accepts_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("offer-host");
    let client_dir = temp_dir("offer-client");
    write_file(&host_dir, "Mods/shared.package", b"SAME");
    write_file(&client_dir, "Mods/shared.package", b"SAME");
    write_file(&client_dir, "Mods/Creator/new1.package", b"NEW-ONE");
    write_file(&client_dir, "Mods/new2.package", b"NEW-TWO");

    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    crate::commands::files::scan_files_inner(&host_state, None, true).await.unwrap();
    let port = start_tcp_host(host_state.clone()).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    assert!(client_state.lock().await.offers_available);

    let offer = offer_files_inner(&client_state, vec!["Mods/Creator/new1.package".into(), "Mods/new2.package".into(), "Mods/shared.package".into()])
        .await
        .expect("offer");
    assert_eq!(offer.files.len(), 2, "the host already has shared.package");

    until("the host to see the offer", host_offer_len(&host_state)).await;
    let host_peer = host_state.lock().await.offers_in.keys().next().cloned().unwrap();
    decide_offer_inner(&host_state, &host_peer, &["Mods/Creator/new1.package".into()], &["Mods/new2.package".into()]).await.expect("decide");

    let dir = host_dir.clone();
    until("the accepted file to arrive", move || {
        let d = dir.clone();
        Box::pin(async move { file_exists(&d, "Mods/Creator/new1.package") })
    })
    .await;
    assert_eq!(read_file(&host_dir, "Mods/Creator/new1.package"), b"NEW-ONE");
    assert!(!file_exists(&host_dir, "Mods/new2.package"), "declined files never arrive");
    assert!(no_leftover_temp_files(&host_dir));

    let cs = client_state.clone();
    until("the client to see both decisions", move || {
        let cs = cs.clone();
        Box::pin(async move {
            let s = cs.lock().await;
            let o = s.offer_out.as_ref().unwrap();
            let state = |p: &str| o.files.iter().find(|f| f.file.relative_path == p).map(|f| f.state.clone());
            state("Mods/Creator/new1.package") == Some(OfferState::Received) && state("Mods/new2.package") == Some(OfferState::Declined)
        })
    })
    .await;

    let _ = std::fs::remove_dir_all(&host_dir);
    let _ = std::fs::remove_dir_all(&client_dir);
}

/// A client that just sends a file (no accepted offer) gets refused and
/// nothing is written, and the connection stays usable.
#[tokio::test]
async fn host_refuses_an_upload_it_did_not_accept_tcp() {
    let _g = e2e_guard().await;
    let host_dir = temp_dir("offer-refuse-host");
    let host_state = make_state("sims4", &host_dir);
    set_host(&host_state, "Host").await;
    let port = start_tcp_host(host_state.clone()).await;

    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut s = crate::network::stream::PeerStream::tcp(tcp);
    let hello = Message::Hello {
        name: "Pusher".into(),
        version: "0.6.0".into(),
        pin: None,
        supports_compression: false,
        game_id: Some("sims4".into()),
        node_id: None,
        crews: vec![],
        features: vec![],
    };
    protocol::send_message(&mut s, &hello).await.unwrap();
    assert!(matches!(protocol::recv_message(&mut s).await.unwrap(), Message::Welcome { .. }));

    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    let body = b"UNINVITED";
    protocol::send_message(&mut s, &Message::FileHeader { path: "Mods/evil.package".into(), size: body.len() as u64, hash: sha256_hex(body) }).await.unwrap();
    protocol::send_message(&mut s, &Message::FileChunk { data: BASE64.encode(body), offset: 0, compressed: false }).await.unwrap();
    protocol::send_message(&mut s, &Message::FileComplete { path: "Mods/evil.package".into() }).await.unwrap();
    match protocol::recv_message(&mut s).await.unwrap() {
        Message::OfferResult { ok, message, .. } => {
            assert!(!ok);
            assert!(message.contains("didn't accept"), "{message}");
        }
        other => panic!("expected OfferResult, got {other:?}"),
    }
    assert!(!file_exists(&host_dir, "Mods/evil.package"));
    // The body was consumed: the stream still works.
    protocol::send_message(&mut s, &Message::OfferSync { files: None }).await.unwrap();
    assert!(matches!(protocol::recv_message(&mut s).await.unwrap(), Message::OfferStatus { .. }));

    let _ = std::fs::remove_dir_all(&host_dir);
}

#[tokio::test]
async fn an_old_host_gets_no_offers_tcp() {
    let _g = e2e_guard().await;
    let client_dir = temp_dir("offer-old-client");
    write_file(&client_dir, "Mods/mine.package", b"MINE");
    let port = start_fake_old_host_tcp(Default::default(), Some("sims4".into())).await;
    let client_state = make_state("sims4", &client_dir);
    let peer_id = new_peer_id();
    mark_pending_client(&client_state, &peer_id).await;
    connect_client_tcp(client_state.clone(), port, &peer_id).await.expect("connect");
    let err = offer_files_inner(&client_state, vec!["Mods/mine.package".into()]).await.unwrap_err();
    assert!(err.contains("too old"), "{err}");
    let _ = std::fs::remove_dir_all(&client_dir);
}
