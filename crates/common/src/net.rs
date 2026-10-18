//! Reaching addresses users give us (stream server stats URLs) without being
//! led into our own network: the API's "Test connection" and obs-link's polling
//! both refuse private, loopback and link-local addresses.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Why a host can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    #[error("couldn't find that address")]
    NotFound,
    #[error("that address is on a private network")]
    Private,
}

/// Whether `ip` is reachable from the internet, i.e. not private, loopback,
/// link-local, shared, reserved or multicast.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        // Carrier-grade NAT, 100.64.0.0/10.
        || (a == 100 && (64..128).contains(&b))
        // Benchmarking, 198.18.0.0/15.
        || (a == 198 && (18..20).contains(&b))
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Unique local, fc00::/7.
        || (first & 0xfe00) == 0xfc00
        // Link local, fe80::/10.
        || (first & 0xffc0) == 0xfe80)
}

/// Looks `host` up, refusing it when any of its addresses isn't public (unless
/// `allow_private`). Connect to the addresses returned, not to the name: a second
/// lookup could answer differently.
pub async fn resolve(
    host: &str,
    port: u16,
    allow_private: bool,
) -> Result<Vec<SocketAddr>, ResolveError> {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| ResolveError::NotFound)?
        .collect();
    if addresses.is_empty() {
        return Err(ResolveError::NotFound);
    }
    if !allow_private && addresses.iter().any(|address| !is_public(address.ip())) {
        return Err(ResolveError::Private);
    }
    Ok(addresses)
}

/// Whether `url`'s host is an IP address that isn't public. Names are not
/// checked here: they're only known by looking them up ([`resolve`]).
pub fn is_private_literal(host: &str) -> bool {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|ip| !is_public(ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_refused() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(ip.parse::<IpAddr>().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700::1111"] {
            assert!(is_public(ip.parse::<IpAddr>().unwrap()), "{ip}");
        }
    }

    #[test]
    fn literal_hosts() {
        assert!(is_private_literal("169.254.169.254"));
        assert!(is_private_literal("[::1]"));
        assert!(!is_private_literal("1.1.1.1"));
        assert!(!is_private_literal("example.com"));
    }

    #[tokio::test]
    async fn names_that_resolve_to_private_addresses_are_refused() {
        assert_eq!(
            resolve("localhost", 80, false).await,
            Err(ResolveError::Private)
        );
        assert!(resolve("localhost", 80, true).await.is_ok());
        assert_eq!(
            resolve("127.0.0.1", 80, false).await,
            Err(ResolveError::Private)
        );
    }
}
