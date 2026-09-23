//! OS integration: Windows Firewall rules, elevation, network diagnostics.
//!
//! The most common "can't connect" report is Windows Firewall silently
//! dropping inbound connections on the host. When the first-run firewall
//! prompt is dismissed (or answered by a non-admin, or the network is marked
//! "Public"), Windows creates *block* rules for the exe — and block rules win
//! over allow rules, so nothing short of removing them helps. Users discovered
//! that "running as admin" sometimes worked around it. Instead we offer a
//! one-click fix that asks for elevation once (UAC) just to replace the rules.

use crate::network::{discovery, netutil};
use crate::state::AppState;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Default)]
pub struct FirewallStatus {
    /// False on non-Windows platforms (nothing to manage).
    pub supported: bool,
    /// Whether the rule check itself succeeded.
    pub checked: bool,
    pub has_allow_rule: bool,
    pub has_block_rule: bool,
    pub exe_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkDiagnostics {
    pub interfaces: Vec<netutil::LocalInterface>,
    pub firewall: FirewallStatus,
    pub session_port: u16,
    pub discovery_port: u16,
    pub discovery_active: bool,
    pub is_elevated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionTestResult {
    pub reachable: bool,
    pub message: String,
    pub latency_ms: Option<u64>,
}

#[cfg(target_os = "windows")]
mod win {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Run a PowerShell script (passed as -EncodedCommand to avoid all quoting issues).
    pub fn powershell(script: &str) -> Result<std::process::Output, String> {
        use base64::Engine as _;
        let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Failed to run PowerShell: {}", e))
    }

    /// PowerShell single-quoted string literal.
    pub fn ps_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    pub fn is_elevated() -> bool {
        // `net session` only succeeds for administrators.
        std::process::Command::new("net")
            .arg("session")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

fn current_exe_string() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| crate::utils::clean_path(p).to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn firewall_status_blocking() -> FirewallStatus {
    let exe = match current_exe_string() {
        Ok(e) => e,
        Err(_) => return FirewallStatus { supported: true, ..Default::default() },
    };
    // Enum names (Allow/Block, Inbound) are not localized, unlike netsh output.
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $r = Get-NetFirewallApplicationFilter -Program {} | Get-NetFirewallRule | \
              Where-Object {{ $_.Enabled -eq 'True' -and $_.Direction -eq 'Inbound' }}; \
         $a = @($r | Where-Object {{ $_.Action -eq 'Allow' }}).Count; \
         $b = @($r | Where-Object {{ $_.Action -eq 'Block' }}).Count; \
         Write-Output \"$a,$b\"",
        win::ps_quote(&exe)
    );
    let mut status = FirewallStatus { supported: true, exe_path: exe, ..Default::default() };
    if let Ok(out) = win::powershell(&script) {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(line) = text.lines().rev().find(|l| l.contains(',')) {
            let mut parts = line.trim().split(',');
            let allow = parts.next().and_then(|v| v.parse::<u32>().ok());
            let block = parts.next().and_then(|v| v.parse::<u32>().ok());
            if let (Some(a), Some(b)) = (allow, block) {
                status.checked = true;
                status.has_allow_rule = a > 0;
                status.has_block_rule = b > 0;
            }
        }
    }
    status
}

#[cfg(not(target_os = "windows"))]
fn firewall_status_blocking() -> FirewallStatus {
    FirewallStatus { supported: false, checked: true, has_allow_rule: true, ..Default::default() }
}

#[tauri::command]
pub async fn get_firewall_status() -> Result<FirewallStatus, String> {
    tokio::task::spawn_blocking(firewall_status_blocking).await.map_err(|e| e.to_string())
}

/// Replace every firewall rule for this exe with inbound allow rules (TCP +
/// UDP, all profiles). Triggers a single UAC prompt.
#[tauri::command]
pub async fn fix_firewall() -> Result<FirewallStatus, String> {
    #[cfg(target_os = "windows")]
    {
        let exe = current_exe_string()?;
        let inner = format!(
            "$ErrorActionPreference='SilentlyContinue'; $exe = {exe}; \
             Get-NetFirewallApplicationFilter -Program $exe | Get-NetFirewallRule | Remove-NetFirewallRule; \
             Get-NetFirewallRule -DisplayName 'SyncCrate*' | Remove-NetFirewallRule; \
             New-NetFirewallRule -DisplayName 'SyncCrate (TCP-In)' -Direction Inbound -Program $exe -Action Allow -Protocol TCP -Profile Any | Out-Null; \
             New-NetFirewallRule -DisplayName 'SyncCrate (UDP-In)' -Direction Inbound -Program $exe -Action Allow -Protocol UDP -Profile Any | Out-Null",
            exe = win::ps_quote(&exe)
        );
        let inner_encoded = {
            use base64::Engine as _;
            let utf16: Vec<u8> = inner.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            base64::engine::general_purpose::STANDARD.encode(utf16)
        };
        let outer = format!(
            "try {{ $p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
             -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-EncodedCommand','{}'; exit $p.ExitCode }} \
             catch {{ exit 1223 }}",
            inner_encoded
        );
        let output = tokio::task::spawn_blocking(move || win::powershell(&outer))
            .await
            .map_err(|e| e.to_string())??;
        if output.status.code() == Some(1223) {
            return Err("Administrator permission was declined, so the firewall rule wasn't changed.".to_string());
        }
        let status = get_firewall_status().await?;
        if status.checked && status.has_block_rule {
            return Err("A firewall block rule for SyncCrate is still active — it may be enforced by a group policy or third-party firewall.".to_string());
        }
        Ok(status)
    }
    #[cfg(not(target_os = "windows"))]
    {
        get_firewall_status().await
    }
}

#[tauri::command]
pub async fn is_elevated() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(win::is_elevated).await.map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(false)
    }
}

/// Relaunch SyncCrate elevated (needed to write into protected install folders
/// such as `C:\Program Files\...\The Sims 4\Game\Bin` for ReShade/GShade).
#[tauri::command]
pub async fn restart_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let exe = current_exe_string()?;
        let script = format!(
            "try {{ Start-Process -FilePath {} -Verb RunAs; exit 0 }} catch {{ exit 1223 }}",
            win::ps_quote(&exe)
        );
        let output = tokio::task::spawn_blocking(move || win::powershell(&script))
            .await
            .map_err(|e| e.to_string())??;
        if !output.status.success() {
            return Err("Administrator permission was declined.".to_string());
        }
        app.exit(0);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("Restarting as administrator is only supported on Windows.".to_string())
    }
}

/// Probe whether the active game's folder is writable by this process.
#[tauri::command]
pub async fn check_game_path_writable(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<bool, String> {
    let base = {
        let app_state = state.lock().await;
        app_state.active_game_path()?
    };
    Ok(tokio::task::spawn_blocking(move || is_dir_writable(std::path::Path::new(&base)))
        .await
        .map_err(|e| e.to_string())?)
}

pub fn is_dir_writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".synccrate-write-test-{}", uuid::Uuid::new_v4()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[tauri::command]
pub async fn get_network_diagnostics(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
) -> Result<NetworkDiagnostics, String> {
    let session_port = state.lock().await.session_port;
    let interfaces = tokio::task::spawn_blocking(netutil::local_ipv4_interfaces)
        .await
        .map_err(|e| e.to_string())?;
    Ok(NetworkDiagnostics {
        interfaces,
        firewall: get_firewall_status().await?,
        session_port,
        discovery_port: discovery::DISCOVERY_PORT,
        discovery_active: discovery::discovery_active(),
        is_elevated: is_elevated().await.unwrap_or(false),
    })
}

/// Quick TCP reachability test against a host, without joining its session.
#[tauri::command]
pub async fn test_connection(ip: String, port: u16) -> Result<ConnectionTestResult, String> {
    let addr: std::net::IpAddr = ip.trim().parse().map_err(|_| "Invalid IP address".to_string())?;
    let target = std::net::SocketAddr::new(addr, port);
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::TcpStream::connect(target),
    )
    .await;
    Ok(match result {
        Ok(Ok(_)) => ConnectionTestResult {
            reachable: true,
            message: format!("{}:{} is reachable — SyncCrate is listening there.", ip, port),
            latency_ms: Some(start.elapsed().as_millis() as u64),
        },
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => ConnectionTestResult {
            reachable: false,
            message: format!("{} answered, but nothing is listening on port {}. Is the host hosting, and is the port right?", ip, port),
            latency_ms: None,
        },
        Ok(Err(e)) => ConnectionTestResult {
            reachable: false,
            message: format!("Could not reach {}:{} — {}", ip, port, e),
            latency_ms: None,
        },
        Err(_) => ConnectionTestResult {
            reachable: false,
            message: format!("No response from {}:{} after 5s. Most likely the host's firewall is blocking SyncCrate (use \"Fix Windows Firewall\" on the host).", ip, port),
            latency_ms: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_dir_is_writable() {
        assert!(is_dir_writable(&std::env::temp_dir()));
    }

    #[test]
    fn missing_dir_is_not_writable() {
        assert!(!is_dir_writable(std::path::Path::new("Z:/definitely/not/here/synccrate")));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn ps_quote_escapes_single_quotes() {
        assert_eq!(win::ps_quote("C:\\it's\\a.exe"), "'C:\\it''s\\a.exe'");
    }
}
