use chrono::{DateTime, Utc};
use garde::Validate;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::id::{NodeId, NodeLinkId};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NodeLink {
    pub id: NodeLinkId,
    pub node_id: NodeId,
    pub name: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct CreateNodeLinkRequest {
    #[garde(length(min = 1, max = 500))]
    pub name: String,
    #[garde(length(min = 1, max = 2048), custom(web_url))]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Validate)]
pub struct UpdateNodeLinkRequest {
    #[garde(length(min = 1, max = 500))]
    pub name: Option<String>,
    #[garde(length(min = 1, max = 2048), custom(web_url_if_set))]
    pub url: Option<String>,
}

/// Is `url` one the app may put in an `href`: `http://` or `https://`,
/// from its first character, with no control characters.
///
/// SECURITY: a stored link is shown to everyone who can see its node. The
/// UI adds `https://` to a bare address, but the API took anything, so an
/// editor could store `javascript:…` on a shared node; only the page's
/// CSP and `rel="noopener"` stood in the way (2026-11-10). Checked from
/// the first character on purpose: browsers skip leading whitespace, so
/// `" javascript:…"` must not pass.
#[must_use]
pub fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"));
    rest.is_some_and(|host| !host.is_empty()) && !url.chars().any(char::is_control)
}

/// `url` as typed, with `https://` in front when it names no web scheme.
/// What the link forms send, so a bare `example.com` is a link and the
/// API's [`is_web_url`] rule has something to accept.
#[must_use]
pub fn with_web_scheme(url: &str) -> String {
    let url = url.trim();
    if is_web_url(url) {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

const NOT_A_WEB_URL: &str = "must start with http:// or https://";

/// garde rule: [`is_web_url`].
pub fn web_url(url: &str, (): &()) -> garde::Result {
    if is_web_url(url) {
        Ok(())
    } else {
        Err(garde::Error::new(NOT_A_WEB_URL))
    }
}

/// garde rule for an update: an absent URL is left alone.
#[allow(clippy::ref_option)] // garde hands a custom rule `&Option<T>`
pub fn web_url_if_set(url: &Option<String>, ctx: &()) -> garde::Result {
    url.as_deref().map_or(Ok(()), |u| web_url(u, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_and_https_links_are_web_urls() {
        for ok in [
            "https://example.com",
            "http://example.com/a?b=c#d",
            "HTTPS://Example.com",
        ] {
            assert!(is_web_url(ok), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            " javascript:alert(1)",
            "\tjavascript:alert(1)",
            "data:text/html,<script>1</script>",
            "vbscript:x",
            "file:///etc/passwd",
            "example.com",
            "//example.com",
            "https://",
            "https://exa\nmple.com",
            " https://example.com",
            "",
        ] {
            assert!(!is_web_url(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_bare_address_gets_https_and_a_web_url_is_kept() {
        assert_eq!(with_web_scheme(" example.com/a "), "https://example.com/a");
        assert_eq!(with_web_scheme("http://example.com"), "http://example.com");
        // Another scheme typed in becomes part of an https address: the
        // browser asks a host for it and runs nothing.
        assert_eq!(
            with_web_scheme("javascript:alert(1)"),
            "https://javascript:alert(1)"
        );
        // A control character inside is still refused by the API.
        assert!(!is_web_url(&with_web_scheme("exa\nmple.com")));
    }

    #[test]
    fn link_requests_refuse_other_schemes() {
        let create = |url: &str| CreateNodeLinkRequest {
            name: "n".to_string(),
            url: url.to_string(),
        };
        assert!(create("https://example.com").validate().is_ok());
        assert!(create("javascript:alert(1)").validate().is_err());
        let update = |url: Option<&str>| UpdateNodeLinkRequest {
            name: None,
            url: url.map(str::to_string),
        };
        assert!(update(None).validate().is_ok());
        assert!(update(Some("https://example.com")).validate().is_ok());
        assert!(update(Some("javascript:alert(1)")).validate().is_err());
    }
}
