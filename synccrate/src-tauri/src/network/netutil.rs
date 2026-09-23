//! Local network interface helpers: enumerate usable LAN addresses and rank a
//! peer's advertised addresses so we try the one most likely to be reachable
//! first (same subnet > private LAN > VPN/CGNAT > IPv6), instead of whatever
//! mDNS happened to report first — often a Hyper-V / WSL / VirtualBox adapter.

use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};

#[derive(Debug, Clone, Serialize)]
pub struct LocalInterface {
    pub name: String,
    pub ip: String,
    #[serde(skip)]
    pub addr: Ipv4Addr,
    #[serde(skip)]
    pub netmask: Ipv4Addr,
    /// Likely a virtual adapter (Hyper-V, WSL, VirtualBox, VMware, Docker…).
    pub is_virtual: bool,
    /// The interface that carries the default route.
    pub is_primary: bool,
}

const VIRTUAL_ADAPTER_HINTS: &[&str] = &[
    "vethernet", "hyper-v", "wsl", "virtualbox", "vmware", "vmnet", "docker",
    "vbox", "loopback", "npcap", "tap-", "br-", "virbr", "veth",
];

fn looks_virtual(name: &str) -> bool {
    let lower = name.to_lowercase();
    VIRTUAL_ADAPTER_HINTS.iter().any(|h| lower.contains(h))
}

/// The local IPv4 address used for the default route (no packets are sent).
pub fn default_route_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// All usable (non-loopback, non-link-local) IPv4 interfaces, best first.
pub fn local_ipv4_interfaces() -> Vec<LocalInterface> {
    let primary = default_route_ipv4();
    let mut result: Vec<LocalInterface> = Vec::new();

    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if let if_addrs::IfAddr::V4(v4) = &iface.addr {
                let ip = v4.ip;
                if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
                    continue;
                }
                if result.iter().any(|r| r.addr == ip) {
                    continue;
                }
                result.push(LocalInterface {
                    name: iface.name.clone(),
                    ip: ip.to_string(),
                    addr: ip,
                    netmask: v4.netmask,
                    is_virtual: looks_virtual(&iface.name),
                    is_primary: Some(ip) == primary,
                });
            }
        }
    }

    // Fallback if interface enumeration failed but we have a default route.
    if result.is_empty() {
        if let Some(ip) = primary {
            result.push(LocalInterface {
                name: "default".to_string(),
                ip: ip.to_string(),
                addr: ip,
                netmask: Ipv4Addr::new(255, 255, 255, 0),
                is_virtual: false,
                is_primary: true,
            });
        }
    }

    result.sort_by_key(|i| (!i.is_primary, i.is_virtual, ipv4_class_rank(i.addr)));
    result
}

/// Directed broadcast addresses for each local interface (e.g. 192.168.1.255).
pub fn broadcast_addresses() -> Vec<Ipv4Addr> {
    let mut out = vec![Ipv4Addr::BROADCAST];
    for iface in local_ipv4_interfaces() {
        let ip = u32::from(iface.addr);
        let mask = u32::from(iface.netmask);
        if mask == 0 || mask == u32::MAX {
            continue;
        }
        let bcast = Ipv4Addr::from(ip | !mask);
        if !out.contains(&bcast) {
            out.push(bcast);
        }
    }
    out
}

/// Lower is better: private LAN ranges first, then CGNAT (Tailscale), then public.
fn ipv4_class_rank(ip: Ipv4Addr) -> u8 {
    let o = ip.octets();
    if o[0] == 192 && o[1] == 168 {
        0
    } else if o[0] == 10 {
        1
    } else if o[0] == 172 && (16..=31).contains(&o[1]) {
        2
    } else if o[0] == 100 && (64..=127).contains(&o[1]) {
        3
    } else {
        4
    }
}

fn same_subnet(a: Ipv4Addr, b: Ipv4Addr, mask: Ipv4Addr) -> bool {
    let m = u32::from(mask);
    (u32::from(a) & m) == (u32::from(b) & m)
}

/// Rank a peer's addresses given our local interfaces. Pure for testability.
pub fn rank_addresses_with(addrs: &[IpAddr], locals: &[LocalInterface]) -> Vec<IpAddr> {
    let score = |ip: &IpAddr| -> u32 {
        match ip {
            IpAddr::V4(v4) => {
                if v4.is_loopback() {
                    return 900;
                }
                if v4.is_link_local() {
                    return 800;
                }
                // Same subnet as one of our interfaces is the strongest signal;
                // prefer matches on real (non-virtual) adapters.
                if let Some(local) = locals.iter().find(|l| same_subnet(*v4, l.addr, l.netmask)) {
                    return if local.is_virtual { 50 } else { 0 } + ipv4_class_rank(*v4) as u32;
                }
                100 + ipv4_class_rank(*v4) as u32
            }
            IpAddr::V6(v6) => {
                if v6.is_loopback() {
                    return 950;
                }
                // fe80::/10 needs a scope id we don't have — effectively unusable.
                if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                    return 1000;
                }
                500
            }
        }
    };

    let mut unique: Vec<IpAddr> = Vec::new();
    for a in addrs {
        if !unique.contains(a) {
            unique.push(*a);
        }
    }
    unique.sort_by_key(|a| score(a));
    // Drop unusable link-local IPv6 unless it's all we have.
    if unique.iter().any(|a| score(a) < 1000) {
        unique.retain(|a| score(a) < 1000);
    }
    unique
}

pub fn rank_addresses(addrs: &[IpAddr]) -> Vec<IpAddr> {
    rank_addresses_with(addrs, &local_ipv4_interfaces())
}

/// Addresses to show the user on the host screen, best first. Virtual adapters
/// are left out unless nothing else is available.
pub fn host_display_ips() -> Vec<String> {
    let ifaces = local_ipv4_interfaces();
    let real: Vec<String> = ifaces.iter().filter(|i| !i.is_virtual).map(|i| i.ip.clone()).collect();
    if real.is_empty() {
        ifaces.into_iter().map(|i| i.ip).collect()
    } else {
        real
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(ip: [u8; 4], mask: [u8; 4], is_virtual: bool) -> LocalInterface {
        LocalInterface {
            name: "test".into(),
            ip: Ipv4Addr::from(ip).to_string(),
            addr: Ipv4Addr::from(ip),
            netmask: Ipv4Addr::from(mask),
            is_virtual,
            is_primary: false,
        }
    }

    #[test]
    fn prefers_same_subnet_over_virtual_adapter_address() {
        let locals = vec![
            iface([192, 168, 1, 20], [255, 255, 255, 0], false),
            iface([172, 20, 0, 1], [255, 255, 240, 0], true),
        ];
        let addrs: Vec<IpAddr> = vec![
            "172.20.5.9".parse().unwrap(),  // host's WSL adapter
            "fe80::1".parse().unwrap(),
            "192.168.1.35".parse().unwrap(), // host's real LAN IP
        ];
        let ranked = rank_addresses_with(&addrs, &locals);
        assert_eq!(ranked[0], "192.168.1.35".parse::<IpAddr>().unwrap());
        assert!(!ranked.contains(&"fe80::1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn keeps_link_local_v6_when_it_is_the_only_option() {
        let addrs: Vec<IpAddr> = vec!["fe80::1".parse().unwrap()];
        assert_eq!(rank_addresses_with(&addrs, &[]).len(), 1);
    }

    #[test]
    fn deduplicates_and_orders_private_before_public() {
        let addrs: Vec<IpAddr> = vec![
            "8.8.4.4".parse().unwrap(),
            "10.0.0.5".parse().unwrap(),
            "10.0.0.5".parse().unwrap(),
            "100.101.102.103".parse().unwrap(),
        ];
        let ranked = rank_addresses_with(&addrs, &[]);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0], "10.0.0.5".parse::<IpAddr>().unwrap());
        assert_eq!(ranked[2], "8.8.4.4".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn detects_virtual_adapter_names() {
        assert!(looks_virtual("vEthernet (WSL)"));
        assert!(looks_virtual("VirtualBox Host-Only Network"));
        assert!(!looks_virtual("Ethernet"));
        assert!(!looks_virtual("Wi-Fi"));
    }
}
