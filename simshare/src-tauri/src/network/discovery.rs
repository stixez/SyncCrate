use crate::state::PeerInfo;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::sync::OnceLock;
use tokio::sync::Mutex;
use uuid::Uuid;

const SERVICE_TYPE: &str = "_simshare._tcp.local.";

static DAEMON: OnceLock<Mutex<Option<ServiceDaemon>>> = OnceLock::new();

fn get_daemon_lock() -> &'static Mutex<Option<ServiceDaemon>> {
    DAEMON.get_or_init(|| Mutex::new(None))
}

pub async fn start_broadcast(
    name: String,
    port: u16,
    mod_count: usize,
    pin_required: bool,
    game_version: Option<String>,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;

    let pin_flag = if pin_required { "true" } else { "false" };
    let gv = game_version.unwrap_or_default();
    let host_name = format!("simshare-{}.local.", Uuid::new_v4().to_string().split('-').next().unwrap_or("host"));
    let service = ServiceInfo::new(
        SERVICE_TYPE,
        &name,
        &host_name,
        "",
        port,
        [
            ("version", env!("CARGO_PKG_VERSION")),
            ("name", &name),
            ("mods", &mod_count.to_string()),
            ("pin_required", pin_flag),
            ("game_version", &gv),
        ]
        .as_ref(),
    )
    .map_err(|e| e.to_string())?
    .enable_addr_auto();

    daemon.register(service).map_err(|e| e.to_string())?;

    let mut lock = get_daemon_lock().lock().await;
    *lock = Some(daemon);

    Ok(())
}

pub async fn stop_broadcast() {
    let mut lock = get_daemon_lock().lock().await;
    if let Some(daemon) = lock.take() {
        let _ = daemon.shutdown();
    }
}

pub async fn scan_for_hosts(_app: tauri::AppHandle) -> Result<Vec<PeerInfo>, String> {
    tokio::task::spawn_blocking(|| {
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        let receiver = daemon.browse(SERVICE_TYPE).map_err(|e| e.to_string())?;

        let mut peers: Vec<PeerInfo> = Vec::new();
        let timeout = std::time::Duration::from_secs(3);
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            match receiver.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    let name = info
                        .get_properties()
                        .get("name")
                        .map(|v| v.val_str().to_string())
                        .unwrap_or_else(|| info.get_fullname().to_string());

                    let mod_count: usize = info
                        .get_properties()
                        .get("mods")
                        .and_then(|v| v.val_str().parse().ok())
                        .unwrap_or(0);

                    let version = info
                        .get_properties()
                        .get("version")
                        .map(|v| v.val_str().to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    let ip = info
                        .get_addresses()
                        .iter()
                        .next()
                        .map(|a| a.to_string())
                        .unwrap_or_default();

                    let port = info.get_port();

                    let pin_required = info
                        .get_properties()
                        .get("pin_required")
                        .map(|v| v.val_str() == "true")
                        .unwrap_or(false);

                    let game_version = info
                        .get_properties()
                        .get("game_version")
                        .map(|v| v.val_str().to_string())
                        .filter(|v| !v.is_empty());

                    let game_info = game_version.map(|gv| crate::state::GameInfo {
                        game_version: Some(gv),
                        installed_packs: Vec::new(),
                    });

                    // Deduplicate by (ip, port) — mDNS can fire multiple
                    // ServiceResolved events for the same host (IPv4 + IPv6, etc.)
                    if !peers.iter().any(|p| p.ip == ip && p.port == port) {
                        peers.push(PeerInfo {
                            id: Uuid::new_v4().to_string(),
                            name,
                            ip,
                            port,
                            mod_count,
                            version,
                            pin_required,
                            game_info,
                        });
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }

        let _ = daemon.shutdown();
        Ok(peers)
    })
    .await
    .map_err(|e| e.to_string())?
}
