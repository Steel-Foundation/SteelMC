//! Player info forwarding from a proxy in front of this server.
//!
//! - [`velocity`]: Velocity modern forwarding, an HMAC-signed `velocity:player_info` login query.
//! - [`bungee`]: `BungeeCord` legacy forwarding in the handshake hostname, optionally checked
//!   with a `BungeeGuard` token.
//!
//! Formats and disconnect messages follow Paper/Spigot so existing proxy setups work unchanged.

pub(crate) mod bungee;
pub(crate) mod velocity;

use std::net::IpAddr;

/// Parses a forwarded client IP, the way the proxy writes it with Java's
/// `InetAddress.getHostAddress`/`getHostString`.
///
/// Unlike Spigot this never resolves a hostname: anything that is not a literal IP is rejected.
/// IPv6 brackets and zone ids (`%eth0`) are dropped because `IpAddr` cannot carry them.
fn parse_forwarded_ip(address: &str) -> Option<IpAddr> {
    let address = address
        .strip_prefix('[')
        .and_then(|address| address.strip_suffix(']'))
        .unwrap_or(address);
    let address = address.split_once('%').map_or(address, |(ip, _zone)| ip);
    address.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::parse_forwarded_ip;

    #[test]
    fn forwarded_ip_accepts_only_literal_addresses() {
        assert_eq!(
            parse_forwarded_ip("203.0.113.7"),
            Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)))
        );
        assert_eq!(
            parse_forwarded_ip("fe80::1%eth0"),
            Some(IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)))
        );
        assert_eq!(
            parse_forwarded_ip("[::1]"),
            Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
        // A hostname would make Spigot resolve DNS; Steel refuses it.
        assert_eq!(parse_forwarded_ip("localhost"), None);
        assert_eq!(parse_forwarded_ip(""), None);
    }
}
