//! Audio transcription using ChatGPT's internal transcription API.
//!
//! Uses the same OAuth token from Codex auth (~/.codex/auth.json) to call
//! ChatGPT's backend transcription endpoint. This is the same API that
//! Codex Desktop uses for dictation.

use reqwest::multipart;
use std::path::Path;

/// Transcribe audio file using ChatGPT's transcription API
///
/// # Arguments
/// * `audio_path` - Path to the audio file (supports webm, mp3, wav, m4a, etc.)
/// * `language` - Optional language hint (e.g., "en", "es", "fr")
///
/// # Returns
/// Transcribed text on success
pub async fn transcribe_audio(audio_path: &str, language: Option<&str>) -> Result<String, String> {
    let (token, account_id) = get_codex_auth()?;
    let account_id = account_id.ok_or("Codex account_id required for transcription")?;

    // Read audio file
    let path = Path::new(audio_path);
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.webm")
        .to_string();

    let audio_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read audio file: {}", e))?;

    // Detect content type from extension
    let content_type = match path.extension().and_then(|e| e.to_str()) {
        Some("webm") => "audio/webm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/m4a",
        Some("ogg") => "audio/ogg",
        Some("flac") => "audio/flac",
        _ => "audio/webm", // Default
    };

    transcribe_bytes(&audio_bytes, &filename, content_type, language, &token, &account_id).await
}

/// Transcribe raw audio bytes using ChatGPT's transcription API
///
/// # Arguments
/// * `audio_data` - Raw audio bytes
/// * `filename` - Filename for the audio (used for content-type detection)
/// * `content_type` - MIME type of the audio
/// * `language` - Optional language hint
/// * `token` - ChatGPT OAuth token
/// * `account_id` - ChatGPT account ID
pub async fn transcribe_bytes(
    audio_data: &[u8],
    filename: &str,
    content_type: &str,
    language: Option<&str>,
    token: &str,
    account_id: &str,
) -> Result<String, String> {
    println!(
        "[Transcribe] Starting transcription, file: {}, size: {} bytes",
        filename,
        audio_data.len()
    );

    // Build multipart form
    let file_part = multipart::Part::bytes(audio_data.to_vec())
        .file_name(filename.to_string())
        .mime_str(content_type)
        .map_err(|e| format!("Failed to create file part: {}", e))?;

    let mut form = multipart::Form::new().part("file", file_part);

    // Add language if specified
    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }

    let client = reqwest::Client::new();
    let response = client
        .post("https://chatgpt.com/backend-api/transcribe")
        .header("Authorization", format!("Bearer {}", token))
        .header("chatgpt-account-id", account_id)
        .header("originator", "Phantom Harness")
        .header(
            "User-Agent",
            format!("Phantom-Harness/1.0 ({})", std::env::consts::OS),
        )
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Transcription request failed: {}", e))?;

    let status = response.status();

    if status == 401 {
        return Err("Codex token expired or invalid. Please re-authenticate.".to_string());
    }

    if status == 403 {
        return Err("Access forbidden. Ensure you have a valid ChatGPT Plus subscription.".to_string());
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Transcription failed ({}): {}", status, body));
    }

    // Parse response - expects { "text": "transcribed text" }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse transcription response: {}", e))?;

    let text = json["text"]
        .as_str()
        .ok_or("No text field in transcription response")?
        .to_string();

    println!("[Transcribe] Success: {} chars", text.len());
    Ok(text)
}

/// Get Codex OAuth token and account ID from ~/.codex/auth.json
///
/// This is the same auth used by Codex Desktop and provides access to
/// ChatGPT's backend APIs.
pub fn get_codex_auth() -> Result<(String, Option<String>), String> {
    let auth_path = dirs::home_dir()
        .ok_or("No home directory found")?
        .join(".codex")
        .join("auth.json");

    if !auth_path.exists() {
        return Err(
            "Codex auth not found. Please login with Codex Desktop first (~/.codex/auth.json)"
                .to_string(),
        );
    }

    let content = std::fs::read_to_string(&auth_path)
        .map_err(|e| format!("Cannot read ~/.codex/auth.json: {}", e))?;

    // Security: Size limit
    if content.len() > 1_000_000 {
        return Err("Auth file exceeds size limit".to_string());
    }

    let auth: serde_json::Value =
        serde_json::from_str(&content).map_err(|_| "Invalid auth.json format")?;

    let account_id = auth["tokens"]["account_id"]
        .as_str()
        .map(|s| s.to_string());

    let token = auth["tokens"]["access_token"]
        .as_str()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .ok_or("No access_token found in auth.json")?;

    Ok((token, account_id))
}

/// Check if Codex auth is available for transcription
pub fn is_transcription_available() -> bool {
    get_codex_auth().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_parsing() {
        // This test just verifies the function doesn't panic
        // Actual auth testing requires the auth file to exist
        let result = get_codex_auth();
        // Either Ok or specific error message
        if let Err(e) = &result {
            assert!(
                e.contains("auth not found") || e.contains("Cannot read") || e.contains("No access_token"),
                "Unexpected error: {}", e
            );
        }
    }
}
