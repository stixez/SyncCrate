//! LAN host discovery. Two independent mechanisms run side by side:
//!
//! 1. mDNS (`_synccrate._tcp.local.`) — the original mechanism.
//! 2. UDP broadcast on `DISCOVERY_PORT` — a fallback for networks where
//!    multicast is filtered (many consumer routers / Wi-Fi APs, Windows
//!    "Public" network profile, or another mDNS responder holding port 5353).
//!
//! Results are merged per host instance, and every address a host was seen on
//! is kept (ranked by reachability) so the client can fall back to the next one.

use crate::network::netutil;
use crate::state::PeerInfo;
use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SERVICE_TYPE: &str = "_synccrate._tcp.local.";

/// UDP port for broadcast discovery (separate from the TCP session port).
pub const DISCOVERY_PORT: u16 = 47625;
const DISCOVER_REQUEST: &[u8] = b"SYNCCRATE_DISCOVER_V1";
const SCAN_DURATION: Duration = Duration::from_secs(4);

struct BroadcastHandle {
    daemon: Option<ServiceDaemon>,
    udp_cancel: CancellationToken,
}

static BROADCAST: OnceLock<Mutex<Option<BroadcastHandle>>> = OnceLock::new();
static MDNS_ACTIVE: AtomicBool = AtomicBool::new(false);
static UDP_ACTIVE: AtomicBool = AtomicBool::new(false);

fn broadcast_lock() -> &'static Mutex<Option<BroadcastHandle>> {
    BROADCAST.get_or_init(|| Mutex::new(None))
}

/// Whether at least one discovery mechanism is currently advertising this host.
pub fn discovery_active() -> bool {
    MDNS_ACTIVE.load(Ordering::Relaxed) || UDP_ACTIVE.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UdpAnnouncement {
    instance: String,
    name: String,
    port: u16,
    version: String,
    mods: usize,
    pin_required: bool,
    #[serde(default)]
    game_version: String,
    /// Registry id of the game being shared (empty from hosts before 0.5.6).
    #[serde(default)]
    game: String,
    /// Hex iroh endpoint id (empty from hosts before 0.6.0), so crew members
    /// recognise each other on the LAN without a code.
    #[serde(default)]
    node: String,
}

pub async fn start_broadcast(
    name: String,
    port: u16,
    mod_count: usize,
    pin_required: bool,
    game_version: Option<String>,
    game_id: String,
    node_id: String,
) -> Result<(), String> {
    // Replace any previous broadcast (e.g. a stale one from an earlier session).
    stop_broadcast().await;

    let instance = Uuid::new_v4().to_string();
    let gv = game_version.unwrap_or_default();

    // --- mDNS ---
    let daemon = match register_mdns(&instance, &name, port, mod_count, pin_required, &gv, &game_id, &node_id) {
        Ok(d) => {
            MDNS_ACTIVE.store(true, Ordering::Relaxed);
            Some(d)
        }
        Err(e) => {
            log::warn!("mDNS broadcast unavailable ({}); relying on UDP discovery", e);
            None
        }
    };

    // --- UDP responder ---
    let udp_cancel = CancellationToken::new();
    let announcement = UdpAnnouncement {
        instance,
        name,
        port,
        version: env!("CARGO_PKG_VERSION").to_string(),
        mods: mod_count,
        pin_required,
        game_version: gv,
        game: game_id,
        node: node_id,
    };
    match bind_udp_responder() {
        Ok(socket) => {
            UDP_ACTIVE.store(true, Ordering::Relaxed);
            let cancel = udp_cancel.clone();
            tokio::spawn(async move {
                run_udp_responder(socket, announcement, cancel).await;
                UDP_ACTIVE.store(false, Ordering::Relaxed);
            });
        }
        Err(e) => log::warn!("UDP discovery responder unavailable: {}", e),
    }

    if daemon.is_none() && !UDP_ACTIVE.load(Ordering::Relaxed) {
        return Err("LAN discovery could not start — peers can still join via Direct IP".to_string());
    }

    *broadcast_lock().lock().await = Some(BroadcastHandle { daemon, udp_cancel });
    Ok(())
}

fn register_mdns(
    instance: &str,
    name: &str,
    port: u16,
    mod_count: usize,
    pin_required: bool,
    game_version: &str,
    game_id: &str,
    node_id: &str,
) -> Result<ServiceDaemon, String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    // IPv6 multicast is a frequent source of send errors on Windows and we
    // prefer IPv4 for connections anyway.
    let _ = daemon.disable_interface(IfKind::IPv6);

    let pin_flag = if pin_required { "true" } else { "false" };
    let host_name = format!("synccrate-{}.local.", &instance[..8]);
    // Instance names must be unique on the network; two hosts with the same
    // display name would otherwise collide and one would silently vanish.
    let instance_name = format!("{} ({})", name, &instance[..4]);
    let mods = mod_count.to_string();
    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &instance_name,
        &host_name,
        "",
        port,
        [
            ("version", env!("CARGO_PKG_VERSION")),
            ("name", name),
            ("mods", mods.as_str()),
            ("pin_required", pin_flag),
            ("game_version", game_version),
            ("game", game_id),
            ("instance", instance),
            ("node", node_id),
        ]
        .as_ref(),
    )
    .map_err(|e| e.to_string())?
    .enable_addr_auto();

    daemon.register(service).map_err(|e| e.to_string())?;
    Ok(daemon)
}

fn bind_udp_responder() -> Result<tokio::net::UdpSocket, String> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).map_err(|e| e.to_string())?;
    let _ = socket.set_reuse_address(true);
    socket.set_nonblocking(true).map_err(|e| e.to_string())?;
    let addr: SocketAddr = SocketAddr::from(([0, 0, 0, 0], DISCOVERY_PORT));
    socket.bind(&addr.into()).map_err(|e| format!("bind UDP {}: {}", DISCOVERY_PORT, e))?;
    tokio::net::UdpSocket::from_std(socket.into()).map_err(|e| e.to_string())
}

async fn run_udp_responder(socket: tokio::net::UdpSocket, announcement: UdpAnnouncement, cancel: CancellationToken) {
    let reply = match serde_json::to_vec(&announcement) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut buf = [0u8; 256];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            res = socket.recv_from(&mut buf) => {
                match res {
                    Ok((n, from)) if &buf[..n] == DISCOVER_REQUEST => {
                        let _ = socket.send_to(&reply, from).await;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // Windows reports ICMP port-unreachable from earlier sends as
                        // ConnectionReset on the next recv — harmless, keep going.
                        log::debug!("UDP discovery recv error: {}", e);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

pub async fn stop_broadcast() {
    if let Some(handle) = broadcast_lock().lock().await.take() {
        handle.udp_cancel.cancel();
        if let Some(daemon) = handle.daemon {
            let _ = daemon.shutdown();
        }
    }
    MDNS_ACTIVE.store(false, Ordering::Relaxed);
    UDP_ACTIVE.store(false, Ordering::Relaxed);
}

/// A host as seen by one discovery mechanism, before merging.
struct Sighting {
    key: String,
    name: String,
    port: u16,
    mod_count: usize,
    version: String,
    pin_required: bool,
    game_version: Option<String>,
    game_id: Option<String>,
    node_id: Option<String>,
    addrs: Vec<IpAddr>,
}

pub async fn scan_for_hosts() -> Result<Vec<PeerInfo>, String> {
    let mdns = tokio::task::spawn_blocking(scan_mdns);
    let udp = scan_udp();
    let (mdns_res, udp_res) = tokio::join!(mdns, udp);

    let mut sightings = Vec::new();
    let mut errors = Vec::new();
    match mdns_res {
        Ok(Ok(s)) => sightings.extend(s),
        Ok(Err(e)) => errors.push(format!("mDNS: {}", e)),
        Err(e) => errors.push(format!("mDNS: {}", e)),
    }
    match udp_res {
        Ok(s) => sightings.extend(s),
        Err(e) => errors.push(format!("UDP: {}", e)),
    }

    if sightings.is_empty() && errors.len() == 2 {
        return Err(format!("Network discovery failed ({})", errors.join("; ")));
    }
    for e in &errors {
        log::warn!("Discovery partially unavailable: {}", e);
    }

    Ok(merge_sightings(sightings))
}

fn merge_sightings(sightings: Vec<Sighting>) -> Vec<PeerInfo> {
    let mut merged: Vec<Sighting> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for s in sightings {
        if let Some(&i) = index.get(&s.key) {
            let existing = &mut merged[i];
            for a in s.addrs {
                if !existing.addrs.contains(&a) {
                    existing.addrs.push(a);
                }
            }
            if existing.game_version.is_none() {
                existing.game_version = s.game_version;
            }
            if existing.game_id.is_none() {
                existing.game_id = s.game_id;
            }
            if existing.node_id.is_none() {
                existing.node_id = s.node_id;
            }
        } else {
            index.insert(s.key.clone(), merged.len());
            merged.push(s);
        }
    }

    merged
        .into_iter()
        .filter_map(|s| {
            let ranked: Vec<String> = netutil::rank_addresses(&s.addrs).iter().map(|a| a.to_string()).collect();
            let ip = ranked.first()?.clone();
            Some(PeerInfo {
                id: Uuid::new_v4().to_string(),
                name: s.name,
                ip,
                port: s.port,
                mod_count: s.mod_count,
                version: s.version,
                pin_required: s.pin_required,
                game_info: s.game_version.map(|gv| crate::state::GameInfo {
                    game_version: Some(gv),
                    installed_packs: Vec::new(),
                }),
                game_id: s.game_id,
                addresses: ranked,
                node_id: s.node_id,
            })
        })
        .collect()
}

fn scan_mdns() -> Result<Vec<Sighting>, String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let _ = daemon.disable_interface(IfKind::IPv6);
    let receiver = daemon.browse(SERVICE_TYPE).map_err(|e| e.to_string())?;

    let mut out: Vec<Sighting> = Vec::new();
    let start = Instant::now();
    while start.elapsed() < SCAN_DURATION {
        let remaining = SCAN_DURATION.saturating_sub(start.elapsed());
        match receiver.recv_timeout(remaining.min(Duration::from_millis(500))) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let props = info.get_properties();
                let get = |k: &str| props.get(k).map(|v| v.val_str().to_string());
                let name = get("name").unwrap_or_else(|| info.get_fullname().to_string());
                // Older hosts don't send an instance id — fall back to the service name.
                let key = get("instance")
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| info.get_fullname().to_string());
                out.push(Sighting {
                    key,
                    name,
                    port: info.get_port(),
                    mod_count: get("mods").and_then(|v| v.parse().ok()).unwrap_or(0),
                    version: get("version").unwrap_or_else(|| "unknown".to_string()),
                    pin_required: get("pin_required").map(|v| v == "true").unwrap_or(false),
                    game_version: get("game_version").filter(|v| !v.is_empty()),
                    game_id: get("game").filter(|v| !v.is_empty()),
                    node_id: get("node").filter(|v| crate::crews::is_valid_node_id(v)),
                    addrs: info.get_addresses().iter().copied().collect(),
                });
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }

    let _ = daemon.shutdown();
    Ok(out)
}

async fn scan_udp() -> Result<Vec<Sighting>, String> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await.map_err(|e| e.to_string())?;
    socket.set_broadcast(true).map_err(|e| e.to_string())?;
    let targets = netutil::broadcast_addresses();

    let mut out: Vec<Sighting> = Vec::new();
    let start = Instant::now();
    let mut last_send: Option<Instant> = None;
    let mut buf = [0u8; 2048];

    while start.elapsed() < SCAN_DURATION {
        // Re-broadcast every second: UDP is lossy, especially on Wi-Fi.
        if last_send.map_or(true, |t| t.elapsed() >= Duration::from_secs(1)) {
            for t in &targets {
                let _ = socket.send_to(DISCOVER_REQUEST, SocketAddr::from((*t, DISCOVERY_PORT))).await;
            }
            last_send = Some(Instant::now());
        }

        match tokio::time::timeout(Duration::from_millis(250), socket.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if let Ok(a) = serde_json::from_slice::<UdpAnnouncement>(&buf[..n]) {
                    let name: String = a.name.chars().filter(|c| !c.is_control()).take(64).collect();
                    if name.is_empty() || a.port == 0 {
                        continue;
                    }
                    out.push(Sighting {
                        key: a.instance,
                        name,
                        port: a.port,
                        mod_count: a.mods,
                        version: a.version.chars().take(32).collect(),
                        pin_required: a.pin_required,
                        game_version: Some(a.game_version).filter(|v| !v.is_empty()),
                        game_id: Some(a.game).filter(|v| !v.is_empty()),
                        node_id: Some(a.node).filter(|v| crate::crews::is_valid_node_id(v)),
                        // The reply's source address is by definition reachable from us.
                        addrs: vec![from.ip()],
                    });
                }
            }
            Ok(Err(_)) | Err(_) => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sighting(key: &str, addr: &str) -> Sighting {
        Sighting {
            key: key.into(),
            name: "Host".into(),
            port: 9847,
            mod_count: 3,
            version: "0.5.0".into(),
            pin_required: false,
            game_version: None,
            game_id: None,
            node_id: None,
            addrs: vec![addr.parse().unwrap()],
        }
    }

    #[test]
    fn merges_mdns_and_udp_sightings_of_same_host() {
        let peers = merge_sightings(vec![
            sighting("abc", "172.20.1.1"),
            sighting("abc", "192.168.1.10"),
            sighting("def", "192.168.1.11"),
        ]);
        assert_eq!(peers.len(), 2);
        let abc = peers.iter().find(|p| p.addresses.len() == 2).unwrap();
        assert_eq!(abc.ip, abc.addresses[0]);
    }

    #[test]
    fn announcement_roundtrip() {
        let a = UdpAnnouncement {
            instance: "i".into(),
            name: "n".into(),
            port: 1,
            version: "v".into(),
            mods: 2,
            pin_required: true,
            game_version: String::new(),
            game: "sims4".into(),
            node: "ab".into(),
        };
        let bytes = serde_json::to_vec(&a).unwrap();
        let b: UdpAnnouncement = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(b.port, 1);
        assert!(b.pin_required);
        assert_eq!(b.game, "sims4");
        assert_eq!(b.node, "ab");
    }

    #[test]
    fn announcement_from_a_host_without_node_still_parses() {
        let old = r#"{"instance":"i","name":"n","port":1,"version":"0.5.6","mods":0,"pin_required":false,"game":"sims4"}"#;
        let a: UdpAnnouncement = serde_json::from_str(old).unwrap();
        assert!(a.node.is_empty());
    }
}
