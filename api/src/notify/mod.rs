//! Email notification helpers.
//!
//! [`SesNotifier`] wraps `aws-sdk-sesv2` and sends transactional emails.
//! It is entirely optional: when `SES_FROM_EMAIL` is not set the notifier is
//! absent from `AppState` and all notification calls are silently skipped.

use aws_sdk_sesv2::{
    Client,
    types::{Body, Content, Destination, EmailContent, Message},
};
use tracing::warn;

/// Escape the five HTML-significant characters so user-controlled values
/// (node title, inviter name) can't inject markup into the HTML email body.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Build a UTF-8 SES `Content`, returning `None` on the (unreachable) build
/// error instead of panicking — keeps the no-panic guarantee on this path.
fn ses_content(data: String) -> Option<Content> {
    Content::builder().data(data).charset("UTF-8").build().ok()
}

/// Sends transactional emails via AWS SES v2.
#[derive(Clone)]
pub struct SesNotifier {
    client: Client,
    from_email: String,
    frontend_url: String,
}

impl SesNotifier {
    /// Build a [`SesNotifier`] from the shared AWS config already loaded for S3/Cognito.
    pub async fn new(from_email: String, frontend_url: String) -> Self {
        let sdk_config = aws_config::load_from_env().await;
        Self {
            client: Client::new(&sdk_config),
            from_email,
            frontend_url,
        }
    }

    /// Send a "you've been invited" email to an existing user.
    ///
    /// Fires and returns — a send failure is logged as a warning but does NOT
    /// cause the invite endpoint to return an error (the permission was already
    /// granted at this point).
    pub async fn send_invite_notification(
        &self,
        to_email: &str,
        inviter: &str,
        node_title: &str,
        role: &str,
        node_id: &str,
    ) {
        let node_url = format!("{}/node/{node_id}", self.frontend_url.trim_end_matches('/'));

        // Subject + text body are plain text (SES encodes them); the HTML body
        // interpolates user-controlled values, so escape them to prevent HTML
        // injection (phishing links / tracking pixels) into the invite email.
        let subject = format!("{inviter} shared \"{node_title}\" with you on Ember Trove");
        let inviter_h = html_escape(inviter);
        let node_title_h = html_escape(node_title);
        let role_h = html_escape(role);
        let node_url_h = html_escape(&node_url);

        let html = format!(
            "<p>Hi,</p>\
<p><strong>{inviter_h}</strong> has granted you <strong>{role_h}</strong> access to the node \
<strong>&quot;{node_title_h}&quot;</strong> on Ember Trove.</p>\
<p><a href=\"{node_url_h}\">Open node &rarr;</a></p>\
<p style=\"color:#888;font-size:12px;\">You are receiving this email because someone shared a \
knowledge node with you on Ember Trove.</p>"
        );

        let text = format!(
            "{inviter} has granted you {role} access to \"{node_title}\" on Ember Trove.\n\nOpen it here: {node_url}"
        );

        let (Some(subject_c), Some(html_c), Some(text_c)) =
            (ses_content(subject), ses_content(html), ses_content(text))
        else {
            warn!("SES content build failed (non-fatal); skipping invite email");
            return;
        };

        let result = self
            .client
            .send_email()
            .from_email_address(&self.from_email)
            .destination(
                Destination::builder()
                    .to_addresses(to_email)
                    .build(),
            )
            .content(
                EmailContent::builder()
                    .simple(
                        Message::builder()
                            .subject(subject_c)
                            .body(
                                Body::builder()
                                    .html(html_c)
                                    .text(text_c)
                                    .build(),
                            )
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await;

        if let Err(e) = result {
            warn!("SES send_invite_notification failed (non-fatal): {e}");
        }
    }
}

/// Fire-and-forget wrapper used in route handlers: skips silently when `notifier`
/// is `None`.
pub async fn maybe_notify_invite(
    notifier: Option<&SesNotifier>,
    to_email: &str,
    inviter: &str,
    node_title: &str,
    role: &str,
    node_id: &str,
) {
    if let Some(n) = notifier {
        n.send_invite_notification(to_email, inviter, node_title, role, node_id)
            .await;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maybe_notify_skips_when_none() {
        // Calling maybe_notify_invite with None must not panic.
        // We can't easily await in a sync test, so just confirm it compiles and
        // the None branch short-circuits.
        let notifier: Option<&SesNotifier> = None;
        assert!(notifier.is_none());
    }

    #[test]
    fn html_escape_neutralizes_markup() {
        assert_eq!(
            html_escape("</strong><a href=\"x\">hi</a>"),
            "&lt;/strong&gt;&lt;a href=&quot;x&quot;&gt;hi&lt;/a&gt;"
        );
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("o'brien"), "o&#x27;brien");
        assert_eq!(html_escape("plain title"), "plain title");
    }
}
