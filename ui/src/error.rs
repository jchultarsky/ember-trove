use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum UiError {
    #[error("network error: {0}")]
    Network(String),

    #[error("server error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("failed to parse response: {0}")]
    Parse(String),
}

impl UiError {
    pub fn api(status: u16, message: impl Into<String>) -> Self {
        Self::Api {
            status,
            message: message.into(),
        }
    }

    /// A refusal the API explained, as a sentence for the person: its own
    /// message for a 4xx, and not "server error 403: …". `None` for
    /// anything else, which the caller words itself: a network failure, a
    /// 5xx, or a 4xx whose body is not the API's (the proxy's own error
    /// page is HTML).
    pub fn explained(&self) -> Option<String> {
        match self {
            Self::Api { status, message }
                if (400..500).contains(status)
                    && !message.trim().is_empty()
                    && message.len() <= 300
                    && !message.contains('<') =>
            {
                let mut chars = message.chars();
                let first = chars.next()?;
                Some(first.to_uppercase().chain(chars).collect())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_the_api_explained_is_said_in_its_own_words() {
        let refused = UiError::api(403, "only the owner of a task's Project can move it");
        assert_eq!(
            refused.explained().as_deref(),
            Some("Only the owner of a task's Project can move it")
        );
        // Not a refusal, nothing to say, or not the API's own words: the
        // caller keeps its own wording.
        assert_eq!(UiError::api(500, "boom").explained(), None);
        assert_eq!(UiError::api(403, " ").explained(), None);
        assert_eq!(UiError::Network("down".into()).explained(), None);
        let page = "<html><head><title>429 Too Many Requests</title></head></html>";
        assert_eq!(UiError::api(429, page).explained(), None);
        assert_eq!(UiError::api(422, "x".repeat(301)).explained(), None);
    }
}
