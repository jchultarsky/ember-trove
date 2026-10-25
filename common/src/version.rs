//! Release versions as the UI sees them (#129).

/// Whether a bundle stamped `bundle` should offer a reload, given the server
/// reports `server`. Only when both are known and they differ: an unknown
/// side (no stamp in a dev build, a failed health check) never nags.
pub fn update_available(bundle: &str, server: &str) -> bool {
    let (b, s) = (bundle.trim(), server.trim());
    !b.is_empty() && !s.is_empty() && b != s
}

#[cfg(test)]
mod tests {
    use super::update_available;

    #[test]
    fn offers_a_reload_only_for_a_known_different_server_version() {
        assert!(update_available("2.34.0", "2.35.0"));
        assert!(!update_available("2.34.0", "2.34.0"));
        assert!(!update_available("2.34.0", " 2.34.0 "));
        assert!(!update_available("", "2.35.0"), "unstamped (dev) bundle");
        assert!(!update_available("2.34.0", ""), "health check failed");
    }
}
