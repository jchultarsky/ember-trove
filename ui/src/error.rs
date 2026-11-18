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

    /// The error in words for the person: the API's own message for a
    /// refusal it explained (a 4xx), as a sentence, and not "server error
    /// 403: …". Anything else reads as it always did.
    pub fn said(&self) -> String {
        match self {
            Self::Api { status, message } if (400..500).contains(status) && !message.is_empty() => {
                let mut chars = message.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
            other => other.to_string(),
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
            refused.said(),
            "Only the owner of a task's Project can move it"
        );
        // Not a refusal, or nothing to say: as before.
        assert_eq!(UiError::api(500, "boom").said(), "server error 500: boom");
        assert_eq!(UiError::api(403, "").said(), "server error 403: ");
        assert_eq!(
            UiError::Network("down".into()).said(),
            "network error: down"
        );
    }
}
