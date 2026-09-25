//! Shared helpers for end-to-end tests that drive two real `AppState`s (host +
//! client) over a real transport (localhost TCP or local iroh). Test-only
//! (`#[cfg(test)]` in lib.rs); not compiled into the app.
//!
//! Several pieces of production state are process-global (the sync
//! cancellation flag, the restore-in-progress flag, the TCP listener's
//! cancellation token, and the on-disk sync checkpoint/history/backup files
//! under the config dir) rather than per-`AppState`. Concurrent test threads
//! would otherwise race on them, so every E2E test must hold [`e2e_guard`]
//! for its whole body.

use crate::event_sink::Events;
use crate::network::protocol::{self, Message};
use crate::network::stream::PeerStream;
use crate::network::transfer;
use crate::state::AppState;
use crate::state::SessionType;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Once};
use std::time::Duration;
use tokio::sync::Mutex;

/// Redirect every config-dir-rooted path (hash cache, sync config/checkpoint/
/// history, backups) at a temp dir for this whole test binary, so E2E tests
/// never touch the developer's real installed-app data. Idempotent.
pub(crate) fn init_test_env() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("synccrate-e2e-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test config dir");
        std::env::set_var("SYNCCRATE_CONFIG_DIR", &dir);
    });
}

static E2E_LOCK: Mutex<()> = Mutex::const_new(());

/// Hold for the whole body of any test that exercises `run_listener`,
/// `execute_sync_inner`/`run_sync`, or backup restore — all of which touch
/// process-global statics or the shared (redirected) config-dir files.
pub(crate) async fn e2e_guard() -> tokio::sync::MutexGuard<'static, ()> {
    init_test_env();
    E2E_LOCK.lock().await
}

pub(crate) fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("synccrate-e2e-{}-{}", label, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create temp game dir");
    dir
}

pub(crate) fn write_file(base: &Path, rel: &str, content: &[u8]) {
    let path = base.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, content).unwrap();
}

/// Write a file and then force its mtime, so tests can control staleness
/// ("changed since compare") deterministically instead of racing the clock.
pub(crate) fn write_file_mtime(base: &Path, rel: &str, content: &[u8], mtime_secs: u64) {
    write_file(base, rel, content);
    set_mtime(base, rel, mtime_secs);
}

pub(crate) fn set_mtime(base: &Path, rel: &str, mtime_secs: u64) {
    let path = base.join(rel);
    let t = std::time::UNIX_EPOCH + Duration::from_secs(mtime_secs);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(t).unwrap();
}

pub(crate) fn read_file(base: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(base.join(rel)).unwrap_or_else(|e| panic!("read {}: {}", rel, e))
}

pub(crate) fn file_exists(base: &Path, rel: &str) -> bool {
    base.join(rel).is_file()
}

pub(crate) async fn wait_until_file_exists(base: &Path, rel: &str) {
    let path = base.join(rel);
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.is_file() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {} to appear", rel));
}

/// No leftover `.tmp` / `.synccrate-keep-*` files anywhere under `base`.
pub(crate) fn no_leftover_temp_files(base: &Path) -> bool {
    walkdir::WalkDir::new(base)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .all(|e| {
            let name = e.file_name().to_string_lossy();
            !name.ends_with(".tmp") && !name.contains(".synccrate-keep-")
        })
}

/// A fresh `AppState` with the real game registry loaded, `game_id` active
/// and pointed at `base`. `is_host` sets up hosting fields (port left at 0 —
/// callers bind their own listener); a client additionally needs
/// [`mark_pending_client`] before connecting.
pub(crate) fn make_state(game_id: &str, base: &Path) -> Arc<Mutex<AppState>> {
    let mut state = AppState::default();
    state.game_registry = crate::registry::load_registry();
    state.active_game = game_id.to_string();
    state.local_display_name = "tester".to_string();
    state
        .game_paths
        .insert(game_id.to_string(), base.to_string_lossy().to_string());
    Arc::new(Mutex::new(state))
}

pub(crate) async fn set_host(state: &Arc<Mutex<AppState>>, name: &str) {
    let mut s = state.lock().await;
    s.session_type = SessionType::Host;
    s.session_name = name.to_string();
}

/// Mark a client `AppState` as mid-connection to `peer_id`, matching what the
/// real `connect_to_peer`/`connect_by_*` commands do before spawning
/// `connect_to_host` — `run_client_session` refuses to store the connection
/// otherwise (`client_attempt_is_active`).
pub(crate) async fn mark_pending_client(state: &Arc<Mutex<AppState>>, peer_id: &str) {
    let mut s = state.lock().await;
    s.session_type = SessionType::Client;
    s.pending_client_peer_id = Some(peer_id.to_string());
}

pub(crate) fn null_events() -> Events {
    Arc::new(crate::event_sink::NullSink)
}

/// Bind a TCP listener on an OS-assigned port and start the real host accept
/// loop against `state`. The listener task is aborted when the test's tokio
/// runtime shuts down (per-test runtime under `#[tokio::test]`).
pub(crate) async fn start_tcp_host(state: Arc<Mutex<AppState>>) -> u16 {
    let (listener, _) = transfer::bind_listener(0).await.expect("bind host listener");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(transfer::run_listener(listener, state, null_events()));
    port
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Connect a client (already marked pending via `mark_pending_client`) to a
/// TCP host on localhost and wait for the handshake to finish successfully.
/// Runs the (otherwise-forever) client message loop in the background.
pub(crate) async fn connect_client_tcp(
    client_state: Arc<Mutex<AppState>>,
    port: u16,
    peer_id: &str,
) -> Result<(), String> {
    let pid = peer_id.to_string();
    let state_for_task = client_state.clone();
    tokio::spawn(async move {
        let _ = transfer::connect_to_host(
            &["127.0.0.1".to_string()],
            port,
            None,
            &pid,
            state_for_task,
            null_events(),
            None,
        )
        .await;
    });
    wait_connected(&client_state, peer_id).await
}

/// Like `connect_client_tcp`, but expects the handshake to fail (e.g.
/// wrong-game) and returns the error message instead of waiting to connect.
pub(crate) async fn connect_client_tcp_expect_err(
    client_state: Arc<Mutex<AppState>>,
    port: u16,
    peer_id: &str,
) -> String {
    let result = tokio::time::timeout(
        CONNECT_TIMEOUT,
        transfer::connect_to_host(
            &["127.0.0.1".to_string()],
            port,
            None,
            peer_id,
            client_state,
            null_events(),
            None,
        ),
    )
    .await
    .expect("connect_to_host did not return within the timeout");
    match result {
        Ok(()) => panic!("expected connect_to_host to fail, but it succeeded"),
        Err(e) => e,
    }
}

async fn wait_connected(state: &Arc<Mutex<AppState>>, peer_id: &str) -> Result<(), String> {
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        loop {
            if state.lock().await.connections.contains_key(peer_id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for the client to connect".to_string())
}

/// Two local iroh endpoints with relays disabled (no network access needed) —
/// the same pattern as `network::stream::tests::protocol_messages_over_iroh`.
pub(crate) async fn iroh_pair() -> (iroh::Endpoint, iroh::Endpoint) {
    use iroh::{endpoint::presets, Endpoint, RelayMode};
    const ALPN: &[u8] = b"synccrate-e2e-test/1";
    let host = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .expect("bind host iroh endpoint");
    let client = Endpoint::builder(presets::Minimal)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("bind client iroh endpoint");
    (host, client)
}

const IROH_ALPN: &[u8] = b"synccrate-e2e-test/1";

/// Host side of one iroh connection: accept it, open the bi stream and hand
/// the resulting `PeerStream` to the real `serve_incoming`. Runs until the
/// peer disconnects.
pub(crate) async fn serve_one_iroh_connection(host_ep: iroh::Endpoint, state: Arc<Mutex<AppState>>) {
    let incoming = tokio::time::timeout(CONNECT_TIMEOUT, host_ep.accept())
        .await
        .expect("timed out waiting for iroh connection")
        .expect("no incoming iroh connection");
    let conn = incoming.await.expect("iroh connection failed");
    let (send, recv) = tokio::time::timeout(CONNECT_TIMEOUT, conn.accept_bi())
        .await
        .expect("timed out accepting iroh bi stream")
        .expect("accept_bi failed");
    let stream = PeerStream::iroh(conn, send, recv);
    transfer::serve_incoming(stream, state, null_events(), "internet:test".to_string()).await;
}

/// Client side of one iroh connection: dial `host_ep`, open the bi stream
/// and run the real (transport-agnostic) client session against it.
pub(crate) async fn connect_client_iroh(
    client_ep: iroh::Endpoint,
    host_ep: &iroh::Endpoint,
    client_state: Arc<Mutex<AppState>>,
    peer_id: &str,
) -> Result<(), String> {
    let conn = tokio::time::timeout(CONNECT_TIMEOUT, client_ep.connect(host_ep.addr(), IROH_ALPN))
        .await
        .map_err(|_| "timed out dialing iroh host".to_string())?
        .map_err(|e| e.to_string())?;
    let (send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    let stream = PeerStream::iroh(conn, send, recv);
    let pid = peer_id.to_string();
    let state_for_task = client_state.clone();
    tokio::spawn(async move {
        // Dropping the endpoint tears down its connections, so it must
        // outlive the session, not just the initial dial.
        let _keep_alive = client_ep;
        let _ = transfer::run_client_session(stream, &[], 0, &pid, state_for_task, null_events(), None).await;
    });
    wait_connected(&client_state, peer_id).await
}

pub(crate) fn new_peer_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `compute_sync_plan_inner`, bounded by a timeout (every await in these
/// helpers is bounded — a hung test should fail fast, not hang the suite).
pub(crate) async fn compute_plan(
    client_state: &Arc<Mutex<AppState>>,
) -> Result<crate::state::SyncPlan, String> {
    tokio::time::timeout(
        Duration::from_secs(15),
        crate::commands::sync::compute_sync_plan_inner(client_state, None),
    )
    .await
    .expect("compute_sync_plan timed out")
}

/// `execute_sync_inner` with a null event sink, bounded by a timeout.
pub(crate) async fn run_sync_now(client_state: &Arc<Mutex<AppState>>) -> Result<(), String> {
    tokio::time::timeout(
        Duration::from_secs(20),
        crate::commands::sync::execute_sync_inner(client_state, null_events(), None),
    )
    .await
    .expect("execute_sync timed out")
}

/// Run `execute_sync_inner` in the background (for tests that need to cancel
/// or otherwise interact with the sync while it's in flight) and return a
/// handle to await its result.
pub(crate) fn spawn_sync(client_state: Arc<Mutex<AppState>>) -> tokio::task::JoinHandle<Result<(), String>> {
    spawn_sync_with_events(client_state, null_events())
}

pub(crate) fn spawn_sync_with_events(
    client_state: Arc<Mutex<AppState>>,
    events: Events,
) -> tokio::task::JoinHandle<Result<(), String>> {
    tokio::spawn(async move {
        crate::commands::sync::execute_sync_inner(&client_state, events, None).await
    })
}

/// A minimal hand-rolled "host" that speaks just enough of the protocol to
/// simulate a pre-0.5.6 host: a `Welcome` with no `game_id`, and files served
/// from an in-memory map (so foreign-game-shaped paths can be served without
/// a real game folder for them). Used only for the "old host" scenario — the
/// real `handle_client` always sends `game_id: Some(..)` now.
pub(crate) async fn start_fake_old_host_tcp(
    files: HashMap<String, Vec<u8>>,
    host_game_id: Option<String>,
) -> u16 {
    let (listener, _) = transfer::bind_listener(0).await.expect("bind fake host listener");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            run_fake_old_host(PeerStream::tcp(stream), files, host_game_id).await;
        }
    });
    port
}

/// A minimal pack listing `files` at the sizes/hashes of the given content
/// bytes (which the test writes to disk itself — this only builds the
/// manifest, matching how a real export never carries the bytes).
pub(crate) fn test_pack(game_id: &str, files: &[(&str, &[u8])]) -> crate::state::ModPack {
    crate::state::ModPack {
        format_version: crate::commands::modpack::FORMAT_VERSION,
        app_version: "0.0.0-test".to_string(),
        game_id: game_id.to_string(),
        name: "Test Pack".to_string(),
        description: String::new(),
        author: String::new(),
        created_at: 0,
        content_types: vec![],
        join: None,
        files: files
            .iter()
            .map(|(path, content)| crate::state::PackFile {
                relative_path: path.to_string(),
                size: content.len() as u64,
                hash: sha256_hex(content),
            })
            .collect(),
    }
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

async fn run_fake_old_host(
    mut stream: PeerStream,
    files: HashMap<String, Vec<u8>>,
    host_game_id: Option<String>,
) {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use crate::state::{FileInfo, FileManifest};

    if protocol::recv_message(&mut stream).await.is_err() {
        return;
    }
    if protocol::send_message(
        &mut stream,
        &Message::Welcome {
            name: "OldHost".to_string(),
            version: "0.5.5".to_string(),
            supports_compression: false,
            game_id: host_game_id,
            node_id: None,
            crews: vec![],
        },
    )
    .await
    .is_err()
    {
        return;
    }

    let mut manifest = FileManifest::default();
    for (path, content) in &files {
        manifest.files.insert(
            path.clone(),
            FileInfo {
                relative_path: path.clone(),
                size: content.len() as u64,
                hash: sha256_hex(content),
                modified: 1_700_000_000,
                file_type: "Mod".to_string(),
            },
        );
    }

    loop {
        match protocol::recv_message(&mut stream).await {
            Ok(Message::ManifestRequest) => {
                let _ = protocol::send_message(&mut stream, &Message::ManifestResponse { manifest: manifest.clone() }).await;
            }
            Ok(Message::FileRequest { path }) => {
                let Some(content) = files.get(&path) else {
                    let _ = protocol::send_message(&mut stream, &Message::Error { message: "File not available".into() }).await;
                    continue;
                };
                let hash = sha256_hex(content);
                if protocol::send_message(&mut stream, &Message::FileHeader { path: path.clone(), size: content.len() as u64, hash }).await.is_err() {
                    break;
                }
                let data = BASE64.encode(content);
                let _ = protocol::send_message(&mut stream, &Message::FileChunk { data, offset: 0, compressed: false }).await;
                let _ = protocol::send_message(&mut stream, &Message::FileComplete { path }).await;
            }
            Ok(Message::ManifestResponse { .. }) | Ok(Message::GameInfoExchange { .. }) | Ok(Message::Ping) => {}
            Ok(Message::Disconnect) | Err(_) => break,
            _ => {}
        }
    }
}
