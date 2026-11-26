//! Destination checks for remote asset downloads: DNS resolution and public-address filtering.

use super::*;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait RemoteAssetResolver {
    fn resolve(&self, url: &reqwest::Url) -> Result<Vec<SocketAddr>, String>;
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct PublicRemoteAssetResolver;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn parse_url_ip_literal(host: &str) -> Option<IpAddr> {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    host.parse().ok()
}

#[cfg(not(target_arch = "wasm32"))]
impl RemoteAssetResolver for PublicRemoteAssetResolver {
    fn resolve(&self, url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
        let host = url
            .host_str()
            .ok_or_else(|| "remote asset URL has no hostname".to_string())?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| "remote asset URL has no valid port".to_string())?;
        let addresses = if let Some(ip) = parse_url_ip_literal(host) {
            vec![SocketAddr::new(ip, port)]
        } else {
            (host, port)
                .to_socket_addrs()
                .map_err(|error| format!("failed to resolve remote asset hostname: {error}"))?
                .collect()
        };
        validate_public_addresses(&addresses)?;
        Ok(addresses)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<F> RemoteAssetResolver for F
where
    F: Fn(&reqwest::Url) -> Result<Vec<SocketAddr>, String>,
{
    fn resolve(&self, url: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
        self(url)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn has_http_scheme(value: &str) -> bool {
    value
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn validate_remote_url(url: &reqwest::Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("remote asset URL must use HTTP or HTTPS".into());
    }
    if url.host_str().is_none() {
        return Err("remote asset URL has no hostname".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("remote asset URL must not contain credentials".into());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn validate_public_addresses(addresses: &[SocketAddr]) -> Result<(), String> {
    if addresses.is_empty() {
        return Err("remote asset hostname resolved to no addresses".into());
    }
    if addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err("remote asset destination is not a public IP address".into());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let value = u32::from(ip);
            let in_range = |network: u32, prefix: u32| {
                let mask = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix)
                };
                value & mask == network & mask
            };
            let blocked = [
                (0x0000_0000, 8),  // This network
                (0x0a00_0000, 8),  // Private use
                (0x6440_0000, 10), // Shared address space
                (0x7f00_0000, 8),  // Loopback
                (0xa9fe_0000, 16), // Link local
                (0xac10_0000, 12), // Private use
                (0xc000_0000, 24), // IETF protocol assignments
                (0xc000_0200, 24), // Documentation
                (0xc058_6300, 24), // Deprecated 6to4 relay anycast
                (0xc0a8_0000, 16), // Private use
                (0xc612_0000, 15), // Benchmarking
                (0xc633_6400, 24), // Documentation
                (0xcb00_7100, 24), // Documentation
                (0xe000_0000, 4),  // Multicast
                (0xf000_0000, 4),  // Reserved and broadcast
            ];
            !blocked
                .iter()
                .any(|(network, prefix)| in_range(*network, *prefix))
                && value != u32::from(std::net::Ipv4Addr::new(168, 63, 129, 16))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            let global_unicast = segments[0] >> 13 == 0b001;
            let value = u128::from(ip);
            // This allowlist follows the assigned prefixes in IANA's IPv6
            // Global Unicast Address Space registry; unlisted space is reserved.
            let assigned_global_unicast = [
                (0x2001_0200_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0400_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0600_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0800_0000_0000_0000_0000_0000_0000, 22),
                (0x2001_0c00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_0e00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1200_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1400_0000_0000_0000_0000_0000_0000, 22),
                (0x2001_1800_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1a00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_1c00_0000_0000_0000_0000_0000_0000, 22),
                (0x2001_2000_0000_0000_0000_0000_0000_0000, 19),
                (0x2001_4000_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4200_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4400_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4600_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4800_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4a00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_4c00_0000_0000_0000_0000_0000_0000, 23),
                (0x2001_5000_0000_0000_0000_0000_0000_0000, 20),
                (0x2001_8000_0000_0000_0000_0000_0000_0000, 19),
                (0x2001_a000_0000_0000_0000_0000_0000_0000, 20),
                (0x2001_b000_0000_0000_0000_0000_0000_0000, 20),
                (0x2003_0000_0000_0000_0000_0000_0000_0000, 18),
                (0x2400_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2410_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2600_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2610_0000_0000_0000_0000_0000_0000_0000, 23),
                (0x2620_0000_0000_0000_0000_0000_0000_0000, 23),
                (0x2630_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2800_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2a00_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2a10_0000_0000_0000_0000_0000_0000_0000, 12),
                (0x2c00_0000_0000_0000_0000_0000_0000_0000, 12),
            ];
            let in_prefix = |network: u128, prefix: u32| {
                let mask = u128::MAX << (128 - prefix);
                value & mask == network
            };
            let globally_reachable_special = [
                (0x2001_0001_0000_0000_0000_0000_0000_0001, 128),
                (0x2001_0001_0000_0000_0000_0000_0000_0002, 128),
                (0x2001_0001_0000_0000_0000_0000_0000_0003, 128),
                (0x2001_0003_0000_0000_0000_0000_0000_0000, 32),
                (0x2001_0004_0112_0000_0000_0000_0000_0000, 48),
                (0x2001_0020_0000_0000_0000_0000_0000_0000, 28),
                (0x2001_0030_0000_0000_0000_0000_0000_0000, 28),
            ];
            let allowed_special = globally_reachable_special
                .iter()
                .any(|(network, prefix)| in_prefix(*network, *prefix));
            let assigned = assigned_global_unicast
                .iter()
                .any(|(network, prefix)| in_prefix(*network, *prefix))
                || allowed_special;
            let special_use = (in_prefix(0x2001_0000_0000_0000_0000_0000_0000_0000, 23)
                && !allowed_special)
                || in_prefix(0x2001_0db8_0000_0000_0000_0000_0000_0000, 32)
                || in_prefix(0x2002_0000_0000_0000_0000_0000_0000_0000, 16);
            global_unicast && assigned && !special_use
        }
    }
}
