//! Internet connectivity via iroh: peer-to-peer QUIC dialled by public key,
//! with NAT hole-punching and encrypted relay fallback (n0's public relays).
//! Only SyncCrate's own traffic uses it — no VPN / virtual adapter involved.
//!
//! One endpoint lives for the whole app run. Its secret key is persisted, so a
//! host's endpoint id (and therefore their join code) stays the same across
//! restarts.

use crate::event_sink::Events;
use crate::network::stream::PeerStream;
use crate::state::{AppState, SessionType};
use iroh::endpoint::{presets, QuicTransportConfig};
use iroh::{Endpoint, EndpointId, SecretKey};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub const ALPN: &[u8] = b"synccrate/1";

static ENDPOINT: tokio::sync::OnceCell<Endpoint> = tokio::sync::OnceCell::const_new();

fn key_path() -> std::path::PathBuf {
    let config = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let dir = config.join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("iroh_secret_key")
}

/// Load (or create and save) this install's secret key.
pub fn secret_key() -> SecretKey {
    let path = key_path();
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) {
            return SecretKey::from_bytes(&arr);
        }
    }
    let key = SecretKey::generate();
    if let Err(e) = write_private(&path, &key.to_bytes()) {
        log::warn!("Could not save iroh key ({}); the join code will change on restart", e);
    }
    key
}

/// Owner-only (0600 on Unix; Windows profile folders are already private)
/// and written to a temp file first, so a crash can't leave half a key.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// This install's endpoint id (known without binding — derived from the key).
pub fn local_id() -> EndpointId {
    secret_key().public()
}

/// The shared endpoint, bound on first use. Incoming connections are only
/// served while hosting; others are refused.
pub async fn endpoint(state: &Arc<Mutex<AppState>>, app: &Events) -> Result<Endpoint, String> {
    let state = state.clone();
    let app = app.clone();
    ENDPOINT
        .get_or_try_init(|| async move {
            let transport = QuicTransportConfig::builder()
                // Our protocol already pings every 20 s; keep the QUIC path
                // alive between pings so relays/NATs don't drop it.
                .keep_alive_interval(Duration::from_secs(10))
                .build();
            let ep = Endpoint::builder(presets::N0)
                .secret_key(secret_key())
                .alpns(vec![ALPN.to_vec()])
                .transport_config(transport)
                .bind()
                .await
                .map_err(|e| format!("Could not start internet connectivity: {}", e))?;
            log::info!("iroh endpoint bound: {}", ep.id());

            let accept_ep = ep.clone();
            tokio::spawn(accept_loop(accept_ep, state, app));
            Ok::<Endpoint, String>(ep)
        })
        .await
        .cloned()
}

async fn accept_loop(ep: Endpoint, state: Arc<Mutex<AppState>>, app: Events) {
    while let Some(incoming) = ep.accept().await {
        let state = state.clone();
        let app = app.clone();
        tokio::spawn(async move {
            let hosting = state.lock().await.session_type == SessionType::Host;
            if !hosting {
                // Not hosting: refuse without completing the handshake.
                drop(incoming);
                return;
            }
            // Counts against the same limit as handshakes in progress, so
            // unauthenticated QUIC connections can't pile up either.
            let Some(slot) = crate::network::transfer::HandshakeSlot::try_acquire() else {
                log::warn!("Refusing internet connection: too many handshakes in progress");
                drop(incoming);
                return;
            };
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("Incoming internet connection failed: {}", e);
                    return;
                }
            };
            let remote = conn.remote_id();
            // The client opens one bidirectional stream and speaks first (Hello).
            let (send, recv) = match tokio::time::timeout(Duration::from_secs(30), conn.accept_bi()).await {
                Ok(Ok(s)) => s,
                _ => {
                    log::warn!("Internet peer {} never opened a stream", remote.fmt_short());
                    return;
                }
            };
            let stream = PeerStream::iroh(conn, send, recv);
            drop(slot);
            let label = format!("internet:{}", remote.fmt_short());
            crate::network::transfer::serve_incoming(stream, state, app, label).await;
        });
    }
}

/// Dial a host by endpoint id (addresses are looked up via n0's discovery;
/// the connection starts on a relay and upgrades to direct when possible).
pub async fn connect(
    state: &Arc<Mutex<AppState>>,
    app: &Events,
    remote: EndpointId,
) -> Result<PeerStream, String> {
    let ep = endpoint(state, app).await?;
    if remote == ep.id() {
        return Err("That join code is your own — share it with a friend instead.".to_string());
    }
    let conn = tokio::time::timeout(Duration::from_secs(25), ep.connect(remote, ALPN))
        .await
        .map_err(|_| {
            "Couldn't reach the host over the internet (timed out). Check the host is still hosting and both PCs are online.".to_string()
        })?
        .map_err(|e| format!("Couldn't reach the host over the internet: {}", e))?;
    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(|e| format!("Internet connection failed: {}", e))?;
    Ok(PeerStream::iroh(conn, send, recv))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_is_stable_across_loads() {
        let a = secret_key().public();
        let b = secret_key().public();
        assert_eq!(a, b);
    }
}
