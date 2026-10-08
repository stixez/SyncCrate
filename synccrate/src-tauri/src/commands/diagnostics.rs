//! "Copy diagnostics": a plain-text report the user pastes into a bug report.
//! Nothing is sent anywhere; the frontend only puts the text on the clipboard.
//!
//! Bug reports kept arriving as "it doesn't connect" with no version, OS or
//! firewall state, and screenshots of the activity log showed join codes and
//! PINs. The report is built by a pure formatter (`format_report`) that runs
//! every free-text value through `Scrubber`, so the privacy rules are tested
//! in one place.

use crate::state::{AppState, SessionType};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const ISSUE_URL: &str = "https://github.com/stixez/SyncCrate/issues/new/choose";
/// Enough to see what went wrong before the report, short enough to read.
pub const MAX_LOG_LINES: usize = 30;
const MAX_LOG_LINE_CHARS: usize = 300;

/// One activity-log entry, as the frontend keeps it (`LogEntry`).
#[derive(Debug, Clone, Deserialize)]
pub struct DiagLogLine {
    /// Milliseconds since the Unix epoch.
    pub timestamp: u64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Hosting,
    Joined,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallSummary {
    NotWindows,
    Allows,
    Blocking,
    NoAllowRule,
    Unknown,
}

/// Everything the report shows, unscrubbed. Gathered by `diagnostics_report`.
#[derive(Debug, Clone)]
pub struct DiagnosticsInput {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub game_id: String,
    pub game_path: Option<String>,
    pub game_path_exists: bool,
    pub session: SessionKind,
    /// Friends connected (host) or hosts connected to (client). Never names.
    pub peers: usize,
    pub syncing: bool,
    pub port: u16,
    pub discovery_on: bool,
    pub internet_ready: bool,
    pub firewall: FirewallSummary,
    pub elevated: bool,
    pub adapters: usize,
    pub virtual_adapters: usize,
    pub last_reach_test: Option<String>,
    /// Bytes per second, 0 = unlimited.
    pub upload_limit: u64,
    pub stay_in_sync: bool,
    pub discord: bool,
    pub log: Vec<DiagLogLine>,
}

/// Removes what identifies the user or their friends from free text: the
/// home folder and Windows username, join codes, PINs, IP addresses, iroh
/// ids and known display names.
#[derive(Debug, Clone, Default)]
pub struct Scrubber {
    pub home: Option<String>,
    pub user: Option<String>,
    pub names: Vec<String>,
}

impl Scrubber {
    pub fn scrub(&self, text: &str) -> String {
        let mut s = text.to_string();
        if let Some(home) = self.home.as_deref().filter(|h| h.len() >= 3) {
            s = scrub_home(&s, home);
        }
        s = scrub_user_dirs(&s);
        if let Some(user) = self.user.as_deref().map(str::trim).filter(|u| u.chars().count() >= 3) {
            s = replace_word_ci(&s, user, "[user]");
        }
        s = scrub_join_codes(&s);
        s = scrub_pins(&s);
        s = scrub_ipv4(&s);
        s = scrub_ipv6(&s);
        s = scrub_long_hex(&s);
        for name in &self.names {
            let name = name.trim();
            // "Host"/"Guest" are the app's own fallbacks, not anyone's name.
            if name.chars().count() < 2 || matches!(name.to_ascii_lowercase().as_str(), "host" | "guest") {
                continue;
            }
            s = replace_word_ci(&s, name, "[name]");
        }
        s
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn char_before(s: &str, i: usize) -> Option<char> {
    s[..i].chars().next_back()
}

fn char_at(s: &str, i: usize) -> Option<char> {
    s[i..].chars().next()
}

/// Replace whole-word, ASCII-case-insensitive occurrences of `needle`.
/// ASCII lowercasing keeps byte offsets, so indices map straight back.
fn replace_word_ci(hay: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return hay.to_string();
    }
    let lower = hay.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(hay.len());
    let mut last = 0;
    let mut from = 0;
    while let Some(pos) = lower[from..].find(&needle_lower) {
        let start = from + pos;
        let end = start + needle_lower.len();
        let bounded = !char_before(hay, start).is_some_and(is_word_char) && !char_at(hay, end).is_some_and(is_word_char);
        if bounded {
            out.push_str(&hay[last..start]);
            out.push_str(with);
            last = end;
            from = end;
        } else {
            from = start + char_at(hay, start).map_or(1, char::len_utf8);
        }
    }
    out.push_str(&hay[last..]);
    out
}

/// `C:\Users\Alex\Documents` → `~\Documents`, in every spelling a log line
/// may carry: either slash, doubled backslashes (JSON / Debug output), any case.
fn scrub_home(text: &str, home: &str) -> String {
    let trimmed = home.trim_end_matches(['\\', '/']);
    let mut variants = vec![trimmed.to_string(), trimmed.replace('\\', "/"), trimmed.replace('/', "\\"), trimmed.replace('\\', "\\\\")];
    variants.sort();
    variants.dedup();
    // Longest first: the doubled-backslash spelling contains the plain one.
    variants.sort_by_key(|v| std::cmp::Reverse(v.len()));
    let mut s = text.to_string();
    for v in variants {
        let lower = s.to_ascii_lowercase();
        let needle = v.to_ascii_lowercase();
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        let mut from = 0;
        while let Some(pos) = lower[from..].find(&needle) {
            let start = from + pos;
            let end = start + needle.len();
            // `C:\Users\Alex2` is someone else's folder, not under this home.
            if char_at(&s, end).is_some_and(is_word_char) {
                from = end;
                continue;
            }
            out.push_str(&s[last..start]);
            out.push('~');
            last = end;
            from = end;
        }
        out.push_str(&s[last..]);
        s = out;
    }
    s
}

/// Any other `\Users\<name>` or `/home/<name>` (another drive, a roaming
/// profile, a path from before the account was renamed): the folder name is
/// usually the Windows username.
fn scrub_user_dirs(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &lower[i..];
        let marker = ["\\\\users\\\\", "\\users\\", "/users/", "/home/"].into_iter().find(|m| rest.starts_with(m));
        let Some(marker) = marker else {
            i += char_at(text, i).map_or(1, char::len_utf8);
            continue;
        };
        let seg_start = i + marker.len();
        let tail = &text[seg_start..];
        let stop = tail
            .char_indices()
            .find(|(_, c)| matches!(c, '\\' | '/' | '"' | '\'' | ':' | ';' | ',' | ')' | ']' | '>' | '\n' | '\r'))
            .map(|(j, _)| j);
        let mut seg_len = stop.unwrap_or(tail.len());
        // A path at the end of a sentence: "...\Users\Alex failed" must keep "failed".
        if stop.is_none() {
            if let Some(ws) = tail.find(char::is_whitespace) {
                seg_len = ws;
            }
        }
        let seg = &tail[..seg_len];
        if seg.is_empty() || seg == "[user]" || seg == "~" {
            i = seg_start;
            continue;
        }
        out.push_str(&text[last..seg_start]);
        out.push_str("[user]");
        last = seg_start + seg_len;
        i = last;
    }
    out.push_str(&text[last..]);
    out
}

/// `SC-8M2K-0QRT-...` → `SC-[code]` (also inside invite links).
fn scrub_join_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    let bytes = text.as_bytes();
    while i + 3 <= bytes.len() {
        let is_prefix = bytes[i].eq_ignore_ascii_case(&b's') && bytes[i + 1].eq_ignore_ascii_case(&b'c') && bytes[i + 2] == b'-';
        if !is_prefix || char_before(text, i).is_some_and(|c| c.is_ascii_alphanumeric()) {
            i += char_at(text, i).map_or(1, char::len_utf8);
            continue;
        }
        let body = text[i + 3..].bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'-').count();
        let alnum = text[i + 3..i + 3 + body].bytes().filter(u8::is_ascii_alphanumeric).count();
        if alnum < 4 {
            i += 3;
            continue;
        }
        out.push_str(&text[last..i]);
        out.push_str("SC-[code]");
        last = i + 3 + body;
        i = last;
    }
    out.push_str(&text[last..]);
    out
}

/// Digits right after the word "PIN" ("PIN 48291", "pin: 48291", "PIN is 48291").
fn scrub_pins(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut from = 0;
    while let Some(pos) = lower[from..].find("pin") {
        let start = from + pos;
        let mut j = start + 3;
        from = j;
        if char_before(text, start).is_some_and(|c| c.is_alphabetic()) || char_at(text, j).is_some_and(|c| c.is_alphabetic()) {
            continue;
        }
        let mut skipped = 0;
        while skipped < 4 && j < text.len() && matches!(text.as_bytes()[j], b' ' | b':' | b'=' | b'#' | b'"' | b'\'') {
            j += 1;
            skipped += 1;
        }
        if lower[j..].starts_with("is ") {
            j += 3;
        }
        let digits = text[j..].bytes().take_while(u8::is_ascii_digit).count();
        if (3..=8).contains(&digits) && !char_at(text, j + digits).is_some_and(|c| c.is_ascii_alphanumeric()) {
            out.push_str(&text[last..j]);
            out.push_str("[hidden]");
            last = j + digits;
            from = last;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// IPv4 addresses → `[LAN IP]` / `[IP]`. Loopback and 0.0.0.0 stay: they
/// identify nobody and help (e.g. "bound to 0.0.0.0:9847").
fn scrub_ipv4(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        let starts = bytes[i].is_ascii_digit() && (i == 0 || !(bytes[i - 1].is_ascii_digit() || bytes[i - 1] == b'.'));
        if !starts {
            i += 1;
            continue;
        }
        let mut octets = [0u32; 4];
        let mut j = i;
        let mut ok = true;
        for (n, octet) in octets.iter_mut().enumerate() {
            let len = bytes[j..].iter().take_while(|b| b.is_ascii_digit()).count();
            if !(1..=3).contains(&len) {
                ok = false;
                break;
            }
            *octet = text[j..j + len].parse().unwrap_or(999);
            j += len;
            if n < 3 {
                if bytes.get(j) != Some(&b'.') {
                    ok = false;
                    break;
                }
                j += 1;
            }
        }
        // "1.2.3.4.5" is a version string, not an address.
        let trailing = bytes.get(j).is_some_and(|b| b.is_ascii_digit())
            || (bytes.get(j) == Some(&b'.') && bytes.get(j + 1).is_some_and(|b| b.is_ascii_digit()));
        if !ok || trailing || octets.iter().any(|o| *o > 255) {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            continue;
        }
        let keep = octets[0] == 127 || octets == [0, 0, 0, 0];
        if !keep {
            let private = octets[0] == 10
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 169 && octets[1] == 254)
                // Tailscale and other carrier-grade NAT VPNs.
                || (octets[0] == 100 && (64..=127).contains(&octets[1]));
            out.push_str(&text[last..i]);
            out.push_str(if private { "[LAN IP]" } else { "[IP]" });
            last = j;
        }
        i = j;
    }
    out.push_str(&text[last..]);
    out
}

/// IPv6 addresses (iroh logs carry them). Needs `::` or at least four colons
/// in an all-hex token, so times like 12:34:56 and Rust paths stay readable.
fn scrub_ipv6(text: &str) -> String {
    let is_tok = |c: char| c.is_ascii_hexdigit() || c == ':';
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    while i < text.len() {
        let c = char_at(text, i).unwrap_or(' ');
        if !is_tok(c) || char_before(text, i).is_some_and(|p| p.is_alphanumeric() || p == ':') {
            i += c.len_utf8();
            continue;
        }
        let len = text[i..].chars().take_while(|c| is_tok(*c)).map(char::len_utf8).sum::<usize>();
        let tok = &text[i..i + len];
        let end = i + len;
        let groups: Vec<&str> = tok.split(':').collect();
        let filled = groups.iter().filter(|g| !g.is_empty()).count();
        let colons = groups.len() - 1;
        let looks_v6 = groups.iter().all(|g| g.len() <= 4)
            && ((tok.contains("::") && filled >= 2) || (colons >= 4 && filled == groups.len()));
        if looks_v6 && !char_at(text, end).is_some_and(|c| c.is_alphanumeric()) {
            out.push_str(&text[last..i]);
            out.push_str("[IP]");
            last = end;
        }
        i = end.max(i + 1);
    }
    out.push_str(&text[last..]);
    out
}

/// Hex runs of 16+ characters: iroh endpoint ids (they let anyone reach this PC).
fn scrub_long_hex(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if !bytes[i].is_ascii_hexdigit() || (i > 0 && bytes[i - 1].is_ascii_alphanumeric()) {
            i += 1;
            continue;
        }
        let len = bytes[i..].iter().take_while(|b| b.is_ascii_hexdigit()).count();
        let end = i + len;
        if len >= 16 && !bytes.get(end).is_some_and(|b| b.is_ascii_alphanumeric()) {
            out.push_str(&text[last..i]);
            out.push_str("[id]");
            last = end;
        }
        i = end;
    }
    out.push_str(&text[last..]);
    out
}

fn on_off(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

fn yes_no(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}

fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec == 0 {
        return "unlimited".to_string();
    }
    let mb = bytes_per_sec as f64 / (1024.0 * 1024.0);
    if mb >= 1.0 {
        let s = format!("{mb:.1}");
        format!("{} MB/s", s.trim_end_matches(".0"))
    } else {
        format!("{} KB/s", (bytes_per_sec as f64 / 1024.0).round() as u64)
    }
}

fn format_time(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown time".to_string())
}

/// The report text. Pure: every value comes from `d`, every free-text value
/// goes through `scrub`.
pub fn format_report(d: &DiagnosticsInput, scrub: &Scrubber) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("SyncCrate diagnostics".to_string());
    lines.push(format!("Paste this into a GitHub issue: {ISSUE_URL}. It contains no files, codes or friends' names."));
    lines.push(String::new());

    lines.push(format!("App version:   {}", d.app_version));
    lines.push(format!("System:        {} ({})", d.os, d.arch));
    lines.push(format!("Game:          {}", if d.game_id.is_empty() { "none" } else { d.game_id.as_str() }));
    match &d.game_path {
        Some(p) => {
            lines.push(format!("Game folder:   {}", scrub.scrub(p)));
            lines.push(format!("Folder exists: {}", yes_no(d.game_path_exists)));
        }
        None => lines.push("Game folder:   not set".to_string()),
    }
    lines.push(String::new());

    let session = match d.session {
        SessionKind::Hosting => format!("hosting, {} connected", d.peers),
        SessionKind::Joined => format!("joined a host ({} connection{})", d.peers, if d.peers == 1 { "" } else { "s" }),
        SessionKind::None => "not in a session".to_string(),
    };
    lines.push(format!("Session:       {session}{}", if d.syncing { ", syncing now" } else { "" }));
    lines.push(format!("Listening port: {}", d.port));
    lines.push(format!("LAN discovery: {}", on_off(d.discovery_on)));
    lines.push(format!("Internet (join codes): {}", if d.internet_ready { "ready" } else { "not started yet" }));
    let firewall = match d.firewall {
        FirewallSummary::NotWindows => "not Windows",
        FirewallSummary::Allows => "allows SyncCrate",
        FirewallSummary::Blocking => "BLOCKING SyncCrate (a block rule exists)",
        FirewallSummary::NoAllowRule => "no allow rule for SyncCrate",
        FirewallSummary::Unknown => "couldn't check",
    };
    lines.push(format!("Windows Firewall: {firewall}"));
    lines.push(format!("Running as admin: {}", yes_no(d.elevated)));
    lines.push(format!(
        "Network adapters: {}{}",
        d.adapters,
        if d.virtual_adapters > 0 { format!(" ({} virtual or VPN)", d.virtual_adapters) } else { String::new() }
    ));
    lines.push(format!(
        "Last reachability test: {}",
        d.last_reach_test.as_deref().map(|t| scrub.scrub(t)).unwrap_or_else(|| "not run".to_string())
    ));
    lines.push(String::new());

    lines.push("Settings".to_string());
    lines.push(format!("  Max upload speed: {}", format_speed(d.upload_limit)));
    lines.push(format!("  Stay in sync:     {}", on_off(d.stay_in_sync)));
    lines.push(format!("  Discord status:   {}", on_off(d.discord)));
    lines.push(String::new());

    let problems: Vec<&DiagLogLine> = d
        .log
        .iter()
        .filter(|l| matches!(l.level.as_str(), "warning" | "error"))
        .collect();
    let shown = &problems[problems.len().saturating_sub(MAX_LOG_LINES)..];
    lines.push(format!("Recent warnings and errors ({} of {})", shown.len(), problems.len()));
    if shown.is_empty() {
        lines.push("  none".to_string());
    }
    for l in shown {
        let one_line = l.message.replace(['\r', '\n'], " ");
        let mut msg = scrub.scrub(&one_line);
        if msg.chars().count() > MAX_LOG_LINE_CHARS {
            msg = msg.chars().take(MAX_LOG_LINE_CHARS).collect::<String>() + "...";
        }
        lines.push(format!("  {} [{}] {}", format_time(l.timestamp), l.level, msg));
    }
    lines.join("\n") + "\n"
}

#[cfg(target_os = "windows")]
fn os_description() -> String {
    use windows_registry::LOCAL_MACHINE;
    let Ok(key) = LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion") else {
        return "Windows".to_string();
    };
    let build = key.get_string("CurrentBuildNumber").unwrap_or_default();
    // ProductName still says "Windows 10" on Windows 11; the build number tells.
    let name = if build.parse::<u32>().is_ok_and(|b| b >= 22000) { "Windows 11" } else { "Windows 10" };
    match key.get_string("DisplayVersion") {
        Ok(v) if !v.is_empty() => format!("{name} {v} (build {build})"),
        _ => format!("{name} (build {build})"),
    }
}

#[cfg(not(target_os = "windows"))]
fn os_description() -> String {
    std::env::consts::OS.to_string()
}

/// Gather the report. `log`, `stay_in_sync`, `discord` and `names` come from
/// the frontend, which keeps the activity log and those preferences; `names`
/// are display names it knows (last host, the user's own) to scrub.
#[tauri::command]
pub async fn diagnostics_report(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    log: Vec<DiagLogLine>,
    stay_in_sync: bool,
    discord: bool,
    names: Vec<String>,
) -> Result<String, String> {
    let (game_id, game_path, session, peers, syncing, port, mut known_names) = {
        let s = state.lock().await;
        let session = match s.session_type {
            SessionType::Host => SessionKind::Hosting,
            SessionType::Client => SessionKind::Joined,
            SessionType::None => SessionKind::None,
        };
        let mut known: Vec<String> = vec![s.local_display_name.clone(), s.session_name.clone()];
        known.extend(s.connections.values().map(|c| c.info.name.clone()));
        known.extend(s.discovered_peers.iter().map(|p| p.name.clone()));
        for crew in &s.crews.crews {
            known.push(crew.name.clone());
            known.extend(crew.members.iter().map(|m| m.name.clone()));
        }
        (
            s.active_game.clone(),
            s.game_paths.get(&s.active_game).cloned(),
            session,
            s.connections.len(),
            s.is_any_syncing(),
            s.session_port,
            known,
        )
    };
    known_names.extend(names);
    // Longest first, so "Alex Smith" goes before "Alex" leaves "[name] Smith".
    known_names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    known_names.dedup();

    let path_for_check = game_path.clone();
    let game_path_exists = tokio::task::spawn_blocking(move || path_for_check.is_some_and(|p| std::path::Path::new(&p).is_dir()))
        .await
        .unwrap_or(false);
    let interfaces = tokio::task::spawn_blocking(crate::network::netutil::local_ipv4_interfaces)
        .await
        .unwrap_or_default();
    let fw = super::system::get_firewall_status().await.unwrap_or_default();
    let firewall = if !fw.supported {
        FirewallSummary::NotWindows
    } else if !fw.checked {
        FirewallSummary::Unknown
    } else if fw.has_block_rule {
        FirewallSummary::Blocking
    } else if !fw.has_allow_rule {
        FirewallSummary::NoAllowRule
    } else {
        FirewallSummary::Allows
    };
    let os = tokio::task::spawn_blocking(os_description).await.unwrap_or_else(|_| std::env::consts::OS.to_string());

    let input = DiagnosticsInput {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os,
        arch: std::env::consts::ARCH.to_string(),
        game_id,
        game_path,
        game_path_exists,
        session,
        peers,
        syncing,
        port,
        discovery_on: crate::network::discovery::discovery_active(),
        internet_ready: crate::network::iroh_net::endpoint_ready(),
        firewall,
        elevated: super::system::is_elevated().await.unwrap_or(false),
        adapters: interfaces.iter().filter(|i| !i.is_virtual).count(),
        virtual_adapters: interfaces.iter().filter(|i| i.is_virtual).count(),
        last_reach_test: super::system::last_reach_test(),
        upload_limit: super::sync::read_sync_config().transfer_speed_limit,
        stay_in_sync,
        discord,
        log,
    };
    let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string());
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .or_else(|| dirs::home_dir().and_then(|h| h.file_name().map(|n| n.to_string_lossy().to_string())));
    let scrubber = Scrubber { home, user, names: known_names };
    Ok(format_report(&input, &scrubber))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber() -> Scrubber {
        Scrubber {
            home: Some(r"C:\Users\Zvonimir".to_string()),
            user: Some("Zvonimir".to_string()),
            names: vec!["Alex Smith".to_string(), "Jordan".to_string(), "Host".to_string()],
        }
    }

    #[test]
    fn join_codes_are_hidden() {
        let s = Scrubber::default();
        assert_eq!(s.scrub("Joining SC-8M2K-0QRT-4F7A-J9XC now"), "Joining SC-[code] now");
        assert_eq!(s.scrub("code sc-8m2k0qrt4f7a"), "code SC-[code]");
        assert_eq!(
            s.scrub("https://synccrate.app/open/#join/SC-8M2K-0QRT?game=sims4"),
            "https://synccrate.app/open/#join/SC-[code]?game=sims4"
        );
        // Not a code: too short, or part of a longer word.
        assert_eq!(s.scrub("DISC-ab is fine, SC-1 too"), "DISC-ab is fine, SC-1 too");
    }

    #[test]
    fn pins_are_hidden() {
        let s = Scrubber::default();
        assert_eq!(s.scrub("Hosting with PIN 48291"), "Hosting with PIN [hidden]");
        assert_eq!(s.scrub("pin: 48291, port 9847"), "pin: [hidden], port 9847");
        assert_eq!(s.scrub("The PIN is 4829."), "The PIN is [hidden].");
        assert_eq!(s.scrub("pin=\"12345\""), "pin=\"[hidden]\"");
        // The word alone, or other numbers, stay.
        assert_eq!(s.scrub("This host requires a PIN"), "This host requires a PIN");
        assert_eq!(s.scrub("Spinning 12345 files"), "Spinning 12345 files");
        assert_eq!(s.scrub("Wrong PIN. Enter the host's current PIN"), "Wrong PIN. Enter the host's current PIN");
    }

    #[test]
    fn home_paths_become_tilde() {
        let s = scrubber();
        assert_eq!(
            s.scrub(r"Scan failed: C:\Users\Zvonimir\Documents\Electronic Arts\The Sims 4\Mods"),
            r"Scan failed: ~\Documents\Electronic Arts\The Sims 4\Mods"
        );
        assert_eq!(s.scrub("c:/users/zvonimir/AppData/Roaming/.minecraft"), "~/AppData/Roaming/.minecraft");
        assert_eq!(s.scrub(r#"{"path":"C:\\Users\\Zvonimir\\Saves"}"#), r#"{"path":"~\\Saves"}"#);
    }

    #[test]
    fn other_user_folders_and_the_username_are_removed() {
        let s = scrubber();
        assert_eq!(s.scrub(r"D:\Users\Zvonimir\Games"), r"D:\Users\[user]\Games");
        assert_eq!(s.scrub(r"C:\Users\John Smith\OneDrive\Mods"), r"C:\Users\[user]\OneDrive\Mods");
        assert_eq!(s.scrub("/home/zvon/.local/share failed"), "/home/[user]/.local/share failed");
        assert_eq!(s.scrub(r"Path C:\Users\Ana failed"), r"Path C:\Users\[user] failed");
        assert_eq!(s.scrub("Hosting as Zvonimir"), "Hosting as [user]");
    }

    #[test]
    fn ip_addresses_are_hidden() {
        let s = Scrubber::default();
        assert_eq!(s.scrub("Could not reach 192.168.1.42:9847"), "Could not reach [LAN IP]:9847");
        assert_eq!(s.scrub("peer 8.8.4.4 and 100.64.1.5"), "peer [IP] and [LAN IP]");
        assert_eq!(s.scrub("bound to 0.0.0.0:9847 and 127.0.0.1"), "bound to 0.0.0.0:9847 and 127.0.0.1");
        assert_eq!(s.scrub("relay [2001:db8:85a3::8a2e:370:7334]:443"), "relay [[IP]]:443");
        assert_eq!(s.scrub("via fe80::1%12"), "via [IP]%12");
        // Versions, times and Rust paths are not addresses.
        assert_eq!(s.scrub("v0.8.5 at 12:34:56 in std::io"), "v0.8.5 at 12:34:56 in std::io");
        assert_eq!(s.scrub("version 1.2.3.4.5"), "version 1.2.3.4.5");
        assert_eq!(s.scrub("999.1.1.1"), "999.1.1.1");
    }

    #[test]
    fn endpoint_ids_and_names_are_hidden() {
        let s = scrubber();
        let id = "a".repeat(64);
        assert_eq!(s.scrub(&format!("node {id} offline")), "node [id] offline");
        assert_eq!(s.scrub("Alex Smith and jordan joined; Jordanian ok"), "[name] and [name] joined; Jordanian ok");
        assert_eq!(s.scrub("Disconnected from Host"), "Disconnected from Host");
    }

    fn input() -> DiagnosticsInput {
        DiagnosticsInput {
            app_version: "0.8.5".to_string(),
            os: "Windows 11 24H2 (build 26200)".to_string(),
            arch: "x86_64".to_string(),
            game_id: "sims4".to_string(),
            game_path: Some(r"C:\Users\Zvonimir\Documents\Electronic Arts\The Sims 4".to_string()),
            game_path_exists: true,
            session: SessionKind::Hosting,
            peers: 2,
            syncing: false,
            port: 9847,
            discovery_on: true,
            internet_ready: true,
            firewall: FirewallSummary::Blocking,
            elevated: false,
            adapters: 1,
            virtual_adapters: 1,
            last_reach_test: Some("192.168.1.20: no answer after 5 s".to_string()),
            upload_limit: 5 * 1024 * 1024,
            stay_in_sync: true,
            discord: false,
            log: Vec::new(),
        }
    }

    #[test]
    fn report_has_header_and_settings_and_scrubs_the_path() {
        let r = format_report(&input(), &scrubber());
        assert!(r.starts_with("SyncCrate diagnostics\nPaste this into a GitHub issue: https://github.com/stixez/SyncCrate/issues/new/choose."));
        assert!(r.contains("It contains no files, codes or friends' names."));
        assert!(r.contains("App version:   0.8.5"));
        assert!(r.contains(r"Game folder:   ~\Documents\Electronic Arts\The Sims 4"));
        assert!(r.contains("Session:       hosting, 2 connected"));
        assert!(r.contains("BLOCKING"));
        assert!(r.contains("Max upload speed: 5 MB/s"));
        assert!(r.contains("Stay in sync:     on"));
        assert!(r.contains("Discord status:   off"));
        assert!(r.contains("Last reachability test: [LAN IP]: no answer"));
        assert!(!r.contains("Zvonimir"));
        assert!(!r.contains('\u{2014}'), "no em dashes in user-facing text");
    }

    #[test]
    fn report_keeps_the_last_30_problems_scrubbed() {
        let mut d = input();
        for i in 0..40 {
            d.log.push(DiagLogLine { timestamp: 1_760_000_000_000 + i, level: "error".into(), message: format!("fail {i} SC-8M2K-0QRT PIN 12345") });
            d.log.push(DiagLogLine { timestamp: 1_760_000_000_000 + i, level: "info".into(), message: format!("info {i}") });
        }
        let r = format_report(&d, &Scrubber::default());
        assert!(r.contains("Recent warnings and errors (30 of 40)"));
        assert!(!r.contains("fail 9 "), "oldest lines dropped");
        assert!(r.contains("fail 10 SC-[code] PIN [hidden]"));
        assert!(r.contains("fail 39 "));
        assert!(!r.contains("info "));
        assert!(!r.contains("12345") && !r.contains("8M2K"));
    }

    #[test]
    fn speeds_read_plainly() {
        assert_eq!(format_speed(0), "unlimited");
        assert_eq!(format_speed(512 * 1024), "512 KB/s");
        assert_eq!(format_speed(1536 * 1024), "1.5 MB/s");
    }
}
