//! Discord webhook notifications for Phantom Harness
//!
//! Ported from the original Phantom sneaker bot's webhook system.
//! Uses Discord's Slack-compatible `/slack` endpoint format.

use crate::utils::truncate_str;
use serde::Serialize;

/// Phantom Twitter avatar - the original ghost icon
const AVATAR_URL: &str =
    "https://pbs.twimg.com/profile_images/1306353753197555713/V0eKwilY_400x400.jpg";

/// Colors used in webhooks (decimal format for Discord)
const COLOR_MAROON: &str = "#5b2738"; // Test/nostalgic checkout
const COLOR_GREEN: &str = "#00FF7F"; // Success/agent notification

/// Slack-compatible payload structure for Discord webhooks
#[derive(Debug, Clone, Serialize)]
pub struct SlackPayload {
    pub username: String,
    pub icon_url: String,
    pub attachments: Vec<SlackAttachment>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlackAttachment {
    pub fallback: String,
    pub color: String,
    pub author_name: String,
    pub author_icon: String,
    pub fields: Vec<SlackField>,
    pub footer: String,
    pub footer_icon: String,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SlackField {
    pub title: String,
    pub value: String,
    pub short: bool,
}

/// Send a webhook payload to Discord (via /slack endpoint)
///
/// Handles rate limiting with retry logic (429 status + Retry-After header)
pub async fn send_webhook(url: &str, payload: &SlackPayload) -> Result<(), String> {
    // Append /slack to the webhook URL if not already present
    let slack_url = if url.ends_with("/slack") {
        url.to_string()
    } else {
        format!("{}/slack", url.trim_end_matches('/'))
    };

    let client = reqwest::Client::new();
    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 3;

    loop {
        attempts += 1;
        let response = client
            .post(&slack_url)
            .json(payload)
            .send()
            .await
            .map_err(|e| format!("Failed to send webhook: {}", e))?;

        let status = response.status();

        if status.is_success() {
            return Ok(());
        }

        // Handle rate limiting
        if status.as_u16() == 429 {
            if attempts >= MAX_ATTEMPTS {
                return Err("Rate limited after maximum retry attempts".to_string());
            }

            // Get retry delay from header or default to 1 second
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(1);

            println!(
                "[Webhook] Rate limited, waiting {} seconds before retry (attempt {}/{})",
                retry_after, attempts, MAX_ATTEMPTS
            );

            tokio::time::sleep(tokio::time::Duration::from_secs(retry_after)).await;
            continue;
        }

        // Other errors - don't retry
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!(
            "Webhook failed with status {}: {}",
            status, error_text
        ));
    }
}

/// Build the nostalgic YEEZY BOOST 350 V2 test checkout payload
///
/// This is the iconic Phantom bot checkout notification format
pub fn build_test_payload() -> SlackPayload {
    let now = chrono::Utc::now().timestamp();

    SlackPayload {
        username: "Phantom Harness".to_string(),
        icon_url: AVATAR_URL.to_string(),
        attachments: vec![SlackAttachment {
            fallback: "Phantom Harness Test Checkout".to_string(),
            color: COLOR_MAROON.to_string(),
            author_name: "Successful Checkout".to_string(),
            author_icon: AVATAR_URL.to_string(),
            fields: vec![
                SlackField {
                    title: "Item".to_string(),
                    value: "YEEZY BOOST 350 V2".to_string(),
                    short: true,
                },
                SlackField {
                    title: "Size".to_string(),
                    value: "10".to_string(),
                    short: true,
                },
                SlackField {
                    title: "Profile".to_string(),
                    value: "Test Profile".to_string(),
                    short: true,
                },
                SlackField {
                    title: "Order Number".to_string(),
                    value: "||PH-TEST-12345||".to_string(), // Discord spoiler tags
                    short: true,
                },
                SlackField {
                    title: "Site".to_string(),
                    value: "Phantom Harness".to_string(),
                    short: true,
                },
            ],
            footer: "Phantom Harness".to_string(),
            footer_icon: AVATAR_URL.to_string(),
            ts: now,
        }],
    }
}

/// Build an agent notification payload for when tasks are ready for input
pub fn build_agent_notification_payload(
    agent_display_name: &str,
    task_title: &str,
    project_path: &str,
    preview: &str,
) -> SlackPayload {
    let now = chrono::Utc::now().timestamp();

    // Truncate preview to 200 chars (safe for UTF-8)
    let preview_truncated = truncate_str(preview, 200);

    SlackPayload {
        username: "Phantom Harness".to_string(),
        icon_url: AVATAR_URL.to_string(),
        attachments: vec![SlackAttachment {
            fallback: format!("{} - Ready for Input", agent_display_name),
            color: COLOR_GREEN.to_string(),
            author_name: "Agent Ready for Input".to_string(),
            author_icon: AVATAR_URL.to_string(),
            fields: vec![
                SlackField {
                    title: "Agent".to_string(),
                    value: agent_display_name.to_string(),
                    short: true,
                },
                SlackField {
                    title: "Task".to_string(),
                    value: task_title.to_string(),
                    short: true,
                },
                SlackField {
                    title: "Status".to_string(),
                    value: "Ready for Input".to_string(),
                    short: true,
                },
                SlackField {
                    title: "Project".to_string(),
                    value: project_path.to_string(),
                    short: true,
                },
                SlackField {
                    title: "Preview".to_string(),
                    value: preview_truncated,
                    short: false,
                },
            ],
            footer: "Phantom Harness".to_string(),
            footer_icon: AVATAR_URL.to_string(),
            ts: now,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_test_payload() {
        let payload = build_test_payload();
        assert_eq!(payload.username, "Phantom Harness");
        assert_eq!(payload.attachments.len(), 1);
        assert_eq!(payload.attachments[0].fields.len(), 5);
        assert_eq!(payload.attachments[0].fields[0].value, "YEEZY BOOST 350 V2");
    }

    #[test]
    fn test_build_agent_notification_payload() {
        let payload = build_agent_notification_payload(
            "Claude Code",
            "Test Task",
            "/path/to/project",
            "This is a preview message",
        );
        assert_eq!(payload.username, "Phantom Harness");
        assert_eq!(payload.attachments[0].fields[0].value, "Claude Code");
        assert_eq!(payload.attachments[0].fields[1].value, "Test Task");
    }

    #[test]
    fn test_preview_truncation() {
        let long_preview = "a".repeat(300);
        let payload = build_agent_notification_payload("Agent", "Task", "/path", &long_preview);
        let preview_field = &payload.attachments[0].fields[4];
        assert!(preview_field.value.len() <= 203); // 200 + "..."
        assert!(preview_field.value.ends_with("..."));
    }
}
