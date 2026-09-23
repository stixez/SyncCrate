//! Join codes: a short, typo-tolerant string (e.g. `SC-8M2K-0QRT-...`) that
//! carries everything a friend needs to connect — the host's addresses, port
//! and optional PIN — so nobody has to read out IPs and port numbers.
//!
//! Layout (before base32): version, flags (bit 0 = PIN present), port (u16 BE),
//! [pin (u16 BE)], address count, IPv4 addresses (4 bytes each), checksum.
//! Encoded with Crockford base32 (no I/L/O/U; decoding maps I/L→1, O→0).

use std::net::Ipv4Addr;

const VERSION: u8 = 1;
const PREFIX: &str = "SC";
const MAX_ADDRESSES: usize = 6;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinInfo {
    pub addresses: Vec<Ipv4Addr>,
    pub port: u16,
    pub pin: Option<String>,
}

fn checksum(bytes: &[u8]) -> u8 {
    // Position-weighted so swapped characters are caught, not just wrong ones.
    bytes
        .iter()
        .enumerate()
        .fold(0x5Au8, |acc, (i, b)| acc.wrapping_add(b.wrapping_mul(i as u8 * 2 + 1)))
}

fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in s.chars() {
        let c = match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        };
        let v = ALPHABET.iter().position(|&a| a as char == c)? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

pub fn encode(info: &JoinInfo) -> Result<String, String> {
    if info.addresses.is_empty() {
        return Err("No network address to put in the join code".to_string());
    }
    let mut bytes = vec![VERSION];
    let pin: Option<u16> = match &info.pin {
        Some(p) => Some(p.parse().map_err(|_| "PIN must be numeric".to_string())?),
        None => None,
    };
    bytes.push(if pin.is_some() { 1 } else { 0 });
    bytes.extend_from_slice(&info.port.to_be_bytes());
    if let Some(p) = pin {
        bytes.extend_from_slice(&p.to_be_bytes());
    }
    let addrs: Vec<&Ipv4Addr> = info.addresses.iter().take(MAX_ADDRESSES).collect();
    bytes.push(addrs.len() as u8);
    for a in addrs {
        bytes.extend_from_slice(&a.octets());
    }
    bytes.push(checksum(&bytes));

    let body = base32_encode(&bytes);
    let groups: Vec<String> = body
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).to_string())
        .collect();
    Ok(format!("{}-{}", PREFIX, groups.join("-")))
}

pub fn decode(code: &str) -> Result<JoinInfo, String> {
    let invalid = || "That join code isn't valid — check it was copied completely.".to_string();
    let cleaned: String = code
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    let body = cleaned.strip_prefix(PREFIX).unwrap_or(&cleaned);
    let bytes = base32_decode(body).ok_or_else(invalid)?;

    if bytes.len() < 6 {
        return Err(invalid());
    }
    // (Padding bits never add a byte: decoding yields exactly the encoded bytes.)
    let (payload, sum) = bytes.split_at(bytes.len() - 1);
    if checksum(payload) != sum[0] {
        return Err(invalid());
    }

    if payload[0] != VERSION {
        return Err("This join code is from a different SyncCrate version — update both apps.".to_string());
    }
    let has_pin = payload[1] & 1 == 1;
    let mut i = 2;
    let take = |i: &mut usize, n: usize| -> Result<&[u8], String> {
        let s = payload.get(*i..*i + n).ok_or_else(invalid)?;
        *i += n;
        Ok(s)
    };
    let port = u16::from_be_bytes(take(&mut i, 2)?.try_into().map_err(|_| invalid())?);
    let pin = if has_pin {
        let p = u16::from_be_bytes(take(&mut i, 2)?.try_into().map_err(|_| invalid())?);
        Some(format!("{:04}", p))
    } else {
        None
    };
    let count = take(&mut i, 1)?[0] as usize;
    if count == 0 || count > MAX_ADDRESSES {
        return Err(invalid());
    }
    let mut addresses = Vec::new();
    for _ in 0..count {
        let o = take(&mut i, 4)?;
        addresses.push(Ipv4Addr::new(o[0], o[1], o[2], o[3]));
    }
    if port < 1024 {
        return Err(invalid());
    }
    Ok(JoinInfo { addresses, port, pin })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(pin: Option<&str>) -> JoinInfo {
        JoinInfo {
            addresses: vec![Ipv4Addr::new(192, 168, 1, 20), Ipv4Addr::new(100, 101, 102, 103)],
            port: 9847,
            pin: pin.map(|p| p.to_string()),
        }
    }

    #[test]
    fn roundtrip_with_and_without_pin() {
        for info in [sample(None), sample(Some("0427")), sample(Some("9999"))] {
            let code = encode(&info).unwrap();
            assert!(code.starts_with("SC-"));
            assert_eq!(decode(&code).unwrap(), info);
        }
    }

    #[test]
    fn decode_is_forgiving_about_formatting() {
        let code = encode(&sample(Some("1234"))).unwrap();
        let messy = format!("  {}  ", code.to_lowercase().replace('-', " "));
        assert_eq!(decode(&messy).unwrap(), sample(Some("1234")));
        // Without the prefix / dashes.
        let bare: String = code.trim_start_matches("SC-").chars().filter(|c| *c != '-').collect();
        assert_eq!(decode(&bare).unwrap(), sample(Some("1234")));
    }

    #[test]
    fn detects_typos() {
        let code = encode(&sample(None)).unwrap();
        let mut chars: Vec<char> = code.chars().collect();
        let idx = chars.len() - 3;
        chars[idx] = if chars[idx] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert!(decode(&typo).is_err());
        assert!(decode("SC-HELLO").is_err());
        assert!(decode("").is_err());
    }

    #[test]
    fn single_address_code_is_short() {
        let info = JoinInfo { addresses: vec![Ipv4Addr::new(10, 0, 0, 5)], port: 9847, pin: None };
        let code = encode(&info).unwrap();
        assert!(code.len() <= 26, "code too long: {}", code);
        assert_eq!(decode(&code).unwrap(), info);
    }
}
