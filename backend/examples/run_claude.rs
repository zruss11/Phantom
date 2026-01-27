use phantom_harness_backend::cli::{AgentProcessClient, StreamingUpdate};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "reply with exactly OK".to_string());

    let cwd = std::env::current_dir()?;
    let mut client = AgentProcessClient::spawn(
        "claude",
        &vec!["--output-format".to_string(), "stream-json".to_string()],
        &cwd,
        &[],
    )
    .await?;

    client.session_set_mode("", "bypassPermissions").await?;

    let res = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        client.session_prompt_streaming("", &prompt, |u| match u {
            StreamingUpdate::Status { message } => println!("[status] {message}"),
            StreamingUpdate::TextChunk { text, .. } => print!("{text}"),
            StreamingUpdate::ReasoningChunk { .. } => {}
            StreamingUpdate::ToolCall { name, .. } => println!("\n[tool] {name}"),
            StreamingUpdate::ToolReturn { .. } => {}
            StreamingUpdate::PermissionRequest { tool_name, .. } => {
                println!("\n[perm] {tool_name}")
            }
            StreamingUpdate::AvailableCommands { .. } => {},
            StreamingUpdate::UserInputRequest { .. } => {}
            StreamingUpdate::PlanUpdate { .. } => {}
        }),
    )
    .await??;

    println!("\n\n=== messages ===");
    for m in &res.messages {
        println!("{}: {:?}", m.message_type, m.content);
    }

    Ok(())
}
