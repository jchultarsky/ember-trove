//! SSRF guards shared by webhook validation (create/update time, in
//! `routes/webhooks.rs`) and webhook delivery (dispatch time, in
//! `webhook_dispatch.rs`).
//!
//! [`is_blocked_ip`] is the single definition of "an address a
//! server-initiated request must never touch". [`vet_url_for_dispatch`]
//! re-resolves a webhook URL's host immediately before delivery and returns
//! the vetted socket addresses so the HTTP client can be pinned to them
//! (`reqwest::ClientBuilder::resolve_to_addrs`) — closing the DNS-rebinding
//! TOCTOU window between create-time validation and the actual send.

use std::net::{IpAddr, SocketAddr};

/// True if `ip` is loopback / private / link-local / IMDS / unspecified — i.e.
/// a target a server-initiated request must never reach (SSRF).
pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
                || (v4.octets()[0] == 169 && v4.octets()[1] == 254)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 (ULA)
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 (link-local)
        }
    }
}

/// Outcome of dispatch-time vetting for a webhook URL.
pub enum DispatchTarget {
    /// Host is a literal, non-blocked IP — no DNS involved, nothing to pin.
    LiteralIp,
    /// Hostname resolved to these non-blocked addresses; pin the client to
    /// them so the connection cannot be re-resolved to something else.
    Pinned {
        host: String,
        addrs: Vec<SocketAddr>,
    },
}

/// Re-validate `url` immediately before dispatch.
///
/// Returns `Err(reason)` if the URL is malformed or hostless, its host is a
/// blocked literal IP, the host does not resolve, or ANY resolved address is
/// blocked — the same strictness as create-time `validate_webhook_dns`.
pub async fn vet_url_for_dispatch(url: &str) -> Result<DispatchTarget, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "invalid URL".to_string())?;
    let Some(host) = parsed.host_str() else {
        return Err("URL has no host".to_string());
    };

    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_blocked_ip(ip) {
            Err(format!("literal IP {ip} is blocked"))
        } else {
            Ok(DispatchTarget::LiteralIp)
        };
    }

    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "host does not resolve".to_string())?
        .collect();
    if addrs.is_empty() {
        return Err("host resolved to no addresses".to_string());
    }
    if let Some(bad) = addrs.iter().find(|a| is_blocked_ip(a.ip())) {
        return Err(format!("host resolves to blocked address {}", bad.ip()));
    }
    Ok(DispatchTarget::Pinned {
        host: host.to_string(),
        addrs,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_blocked_ip_flags_private_and_imds() {
        for ip in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.1",
            "172.16.0.1",
            "169.254.169.254", // AWS IMDS
            "0.0.0.0",
        ] {
            assert!(
                is_blocked_ip(ip.parse::<IpAddr>().unwrap()),
                "{ip} should be blocked"
            );
        }
        for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            assert!(
                !is_blocked_ip(ip.parse::<IpAddr>().unwrap()),
                "{ip} should be allowed"
            );
        }
    }

    #[tokio::test]
    async fn vet_rejects_blocked_literal_ip() {
        assert!(
            vet_url_for_dispatch("https://169.254.169.254/hook")
                .await
                .is_err()
        );
        assert!(vet_url_for_dispatch("https://10.0.0.5/hook").await.is_err());
    }

    #[tokio::test]
    async fn vet_accepts_public_literal_ip_without_pinning() {
        match vet_url_for_dispatch("https://93.184.216.34/hook").await {
            Ok(DispatchTarget::LiteralIp) => {}
            _ => panic!("public literal IP should be accepted with no pin"),
        }
    }

    #[tokio::test]
    async fn vet_rejects_hostname_resolving_to_loopback() {
        // `localhost` resolves via the hosts file — no external DNS needed.
        assert!(
            vet_url_for_dispatch("https://localhost/hook")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn vet_rejects_malformed_and_hostless_urls() {
        assert!(vet_url_for_dispatch("not a url").await.is_err());
        assert!(vet_url_for_dispatch("file:///etc/passwd").await.is_err());
    }
}
