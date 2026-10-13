use anyhow::{anyhow, bail, Result};
use axum::http::HeaderMap;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
};

/// Which peers are allowed to supply `CF-Connecting-IP` / `X-Real-IP` /
/// `X-Forwarded-For`. Configured through `TRUST_PROXY`.
#[derive(Debug, Clone, Default)]
pub struct TrustedProxies {
    all: bool,
    rules: Vec<ProxyRule>,
}

#[derive(Debug, Clone)]
enum ProxyRule {
    Exact(IpAddr),
    V4Subnet { network: u32, mask: u32 },
}

impl TrustedProxies {
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        let lowered = value.to_ascii_lowercase();

        if value.is_empty() || matches!(lowered.as_str(), "0" | "false" | "no" | "off") {
            return Ok(Self::default());
        }
        if matches!(lowered.as_str(), "1" | "true" | "yes" | "on" | "all") {
            return Ok(Self {
                all: true,
                rules: Vec::new(),
            });
        }

        let rules = value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(ProxyRule::parse)
            .collect::<Result<Vec<_>>>()?;

        if rules.is_empty() {
            bail!("TRUST_PROXY 未包含有效的代理地址");
        }

        Ok(Self { all: false, rules })
    }

    pub fn trusts(&self, peer: IpAddr) -> bool {
        self.all || self.rules.iter().any(|rule| rule.matches(peer))
    }

    pub fn is_enabled(&self) -> bool {
        self.all || !self.rules.is_empty()
    }
}

impl ProxyRule {
    fn parse(entry: &str) -> Result<Self> {
        let Some((address, prefix)) = entry.split_once('/') else {
            return IpAddr::from_str(entry)
                .map(Self::Exact)
                .map_err(|_| anyhow!("TRUST_PROXY 中的地址无效: {entry}"));
        };

        let network: Ipv4Addr = address
            .trim()
            .parse()
            .map_err(|_| anyhow!("TRUST_PROXY 只支持 IPv4 网段: {entry}"))?;
        let prefix: u8 = prefix
            .trim()
            .parse()
            .map_err(|_| anyhow!("TRUST_PROXY 中的网段前缀无效: {entry}"))?;
        if prefix > 32 {
            bail!("TRUST_PROXY 中的网段前缀必须在 0-32 之间: {entry}");
        }

        let mask = if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - u32::from(prefix))
        };

        Ok(Self::V4Subnet {
            network: u32::from(network) & mask,
            mask,
        })
    }

    fn matches(&self, peer: IpAddr) -> bool {
        match (self, peer) {
            (Self::Exact(expected), actual) => *expected == actual,
            (Self::V4Subnet { network, mask }, IpAddr::V4(actual)) => {
                u32::from(actual) & mask == *network
            }
            (Self::V4Subnet { .. }, IpAddr::V6(_)) => false,
        }
    }
}

/// Resolves the client address for audit logging. Forwarded headers are only
/// considered when the TCP peer is a configured trusted proxy.
pub fn client_ip(headers: &HeaderMap, peer: SocketAddr, trusted: &TrustedProxies) -> IpAddr {
    if trusted.trusts(peer.ip()) {
        for header in ["cf-connecting-ip", "x-real-ip", "x-forwarded-for"] {
            let Some(value) = headers.get(header).and_then(|value| value.to_str().ok()) else {
                continue;
            };

            // X-Forwarded-For is a comma separated chain; the left-most entry is the client.
            if let Some(ip) = value.split(',').find_map(parse_ip) {
                return ip;
            }
        }
    }

    peer.ip()
}

pub fn parse_ip(value: &str) -> Option<IpAddr> {
    value.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn peer(value: &str) -> SocketAddr {
        SocketAddr::new(value.parse().unwrap(), 5000)
    }

    #[test]
    fn disabled_by_default_and_for_false_values() {
        for value in ["", "0", "false", "no", "off"] {
            let trusted = TrustedProxies::parse(value).unwrap();
            assert!(!trusted.is_enabled(), "value was: {value}");
            assert!(!trusted.trusts("10.0.0.1".parse().unwrap()));
        }
    }

    #[test]
    fn true_trusts_every_peer() {
        let trusted = TrustedProxies::parse("true").unwrap();
        assert!(trusted.is_enabled());
        assert!(trusted.trusts("203.0.113.7".parse().unwrap()));
    }

    #[test]
    fn matches_exact_addresses_and_ipv4_subnets() {
        let trusted = TrustedProxies::parse("10.0.0.1, 172.16.0.0/12").unwrap();
        assert!(trusted.trusts("10.0.0.1".parse().unwrap()));
        assert!(!trusted.trusts("10.0.0.2".parse().unwrap()));
        assert!(trusted.trusts("172.16.5.5".parse().unwrap()));
        assert!(!trusted.trusts("172.32.0.1".parse().unwrap()));
        assert!(!trusted.trusts("::1".parse().unwrap()));
    }

    #[test]
    fn rejects_invalid_entries() {
        assert!(TrustedProxies::parse("not-an-ip").is_err());
        assert!(TrustedProxies::parse("10.0.0.0/33").is_err());
        assert!(TrustedProxies::parse("10.0.0.0/").is_err());
    }

    #[test]
    fn ignores_forwarded_headers_from_untrusted_peers() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", HeaderValue::from_static("203.0.113.7"));

        let peer = peer("198.51.100.9");
        assert_eq!(
            client_ip(&headers, peer, &TrustedProxies::parse("false").unwrap()),
            "198.51.100.9".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            client_ip(
                &headers,
                peer,
                &TrustedProxies::parse("198.51.100.9").unwrap()
            ),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn uses_leftmost_forwarded_for_entry() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.7, 10.0.0.1"),
        );

        let trusted = TrustedProxies::parse("true").unwrap();
        assert_eq!(
            client_ip(&headers, peer("10.0.0.1"), &trusted),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
    }
}
