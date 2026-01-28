use phantom_harness_backend::cli::AgentProcessClient;

// Smoke test for Claude Code stream-json adapter.
//
// Run manually:
//   cd src-tauri
//   CLAUDE_SMOKE=1 cargo test -p phantom_harness_backend --test claude_smoke -- --nocapture
//
// Requires `claude` installed + authenticated.
#[tokio::test]
async fn claude_stream_smoke() {
    if std::env::var("CLAUDE_SMOKE").ok().as_deref() != Some("1") {
        eprintln!("skipping (set CLAUDE_SMOKE=1)");
        return;
    }

    let cwd = std::env::current_dir().expect("cwd");
    let client = AgentProcessClient::spawn(
        "claude",
        &vec!["--output-format".to_string(), "stream-json".to_string()],
        &cwd,
        &[],
    )
    .await
    .expect("spawn");

    client
        .session_set_mode("", "bypassPermissions")
        .await
        .expect("set mode");

    // Keep it tiny. Add a hard timeout so CI/dev doesn't hang if Claude blocks.
    let res = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        client.session_prompt_streaming("", "reply with exactly OK", |_u| {}),
    )
    .await
    .expect("timeout")
    .expect("prompt");

    let text = res
        .messages
        .iter()
        .filter(|m| m.message_type == "assistant_message")
        .filter_map(|m| m.content.as_ref())
        .fold(String::new(), |mut acc, s| {
            acc.push_str(s);
            acc
        });

    assert!(text.contains("OK"), "expected OK, got: {text:?}");
}
