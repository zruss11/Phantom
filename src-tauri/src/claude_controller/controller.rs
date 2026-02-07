use std::collections::HashMap;
use std::sync::Arc;
use std::process::Command;

use tokio::sync::{broadcast, Mutex};

use super::inbox;
use super::process::{ProcessManager, SpawnAgentOptions};
use super::team::{self, TeamMember};
use super::types::{InboxMessage, ParsedMessage, StructuredMessage};

#[derive(Clone)]
pub struct ClaudeTeamsController {
    team_name: String,
    controller_name: String,
    lead_session_id: String,
    cwd: String,
    claude_binary: String,
    default_env: Vec<(String, String)>,
    processes: ProcessManager,
    channels: Arc<Mutex<HashMap<String, broadcast::Sender<InboxMessage>>>>,
    poller_started: Arc<Mutex<bool>>,
}

impl ClaudeTeamsController {
    pub fn team_name(&self) -> &str {
        &self.team_name
    }

    pub async fn init(
        team_name: String,
        cwd: String,
        claude_binary: String,
        default_env: Vec<(String, String)>,
    ) -> Result<Self, String> {
        verify_claude_teammate_support(&claude_binary).await?;

        let lead_session_id = uuid::Uuid::new_v4().to_string();
        team::ensure_team(&team_name, &cwd, &lead_session_id)?;

        let controller = Self {
            team_name,
            controller_name: "controller".to_string(),
            lead_session_id,
            cwd,
            claude_binary,
            default_env,
            processes: ProcessManager::default(),
            channels: Arc::new(Mutex::new(HashMap::new())),
            poller_started: Arc::new(Mutex::new(false)),
        };

        controller.start_poller().await?;
        Ok(controller)
    }

    async fn start_poller(&self) -> Result<(), String> {
        let mut started = self.poller_started.lock().await;
        if *started {
            return Ok(());
        }
        *started = true;

        let team_name = self.team_name.clone();
        let controller_name = self.controller_name.clone();
        let channels = self.channels.clone();
        tokio::spawn(async move {
            loop {
                let events = tokio::task::spawn_blocking({
                    let team_name = team_name.clone();
                    let controller_name = controller_name.clone();
                    move || inbox::read_unread_and_mark_read(&team_name, &controller_name)
                })
                .await
                .ok()
                .and_then(|r| r.ok())
                .unwrap_or_default();

                for ev in events {
                    let from = ev.raw.from.clone();

                    // Auto-approve plan/permission requests for v1.
                    match &ev.parsed {
                        ParsedMessage::Structured(StructuredMessage::PlanApprovalRequest {
                            request_id,
                            timestamp,
                            ..
                        }) => {
                            let msg = serde_json::json!({
                                "type": "plan_approval_response",
                                "requestId": request_id,
                                "from": "controller",
                                "approved": true,
                                "timestamp": timestamp,
                            })
                            .to_string();
                            let _ = tokio::task::spawn_blocking({
                                let team_name = team_name.clone();
                                let from = from.clone();
                                move || {
                                    inbox::write_inbox(
                                        &team_name,
                                        &from,
                                        InboxMessage {
                                            from: "controller".to_string(),
                                            text: msg,
                                            timestamp: chrono::Utc::now().to_rfc3339(),
                                            color: None,
                                            read: false,
                                            summary: Some("auto-approved plan".to_string()),
                                        },
                                    )
                                }
                            })
                            .await;
                            continue;
                        }
                        ParsedMessage::Structured(StructuredMessage::PermissionRequest {
                            request_id,
                            timestamp,
                            ..
                        }) => {
                            let msg = serde_json::json!({
                                "type": "permission_response",
                                "requestId": request_id,
                                "from": "controller",
                                "approved": true,
                                "timestamp": timestamp,
                            })
                            .to_string();
                            let _ = tokio::task::spawn_blocking({
                                let team_name = team_name.clone();
                                let from = from.clone();
                                move || {
                                    inbox::write_inbox(
                                        &team_name,
                                        &from,
                                        InboxMessage {
                                            from: "controller".to_string(),
                                            text: msg,
                                            timestamp: chrono::Utc::now().to_rfc3339(),
                                            color: None,
                                            read: false,
                                            summary: Some("auto-approved permission".to_string()),
                                        },
                                    )
                                }
                            })
                            .await;
                            continue;
                        }
                        _ => {}
                    }

                    // Forward message to per-agent channel.
                    let sender = {
                        let mut map = channels.lock().await;
                        map.entry(from.clone()).or_insert_with(|| {
                            let (tx, _rx) = broadcast::channel(1024);
                            tx
                        });
                        map.get(&from).cloned()
                    };
                    if let Some(tx) = sender {
                        let _ = tx.send(ev.raw.clone());
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });

        Ok(())
    }

    pub async fn subscribe(&self, agent_name: &str) -> broadcast::Receiver<InboxMessage> {
        let mut map = self.channels.lock().await;
        let tx = map.entry(agent_name.to_string()).or_insert_with(|| {
            let (tx, _rx) = broadcast::channel(1024);
            tx
        });
        tx.subscribe()
    }

    pub async fn spawn_agent(
        &self,
        agent_name: String,
        agent_type: Option<String>,
        model: Option<String>,
        cwd: Option<String>,
        permission_mode: Option<String>,
        allowed_tools: Vec<String>,
        env: Vec<(String, String)>,
    ) -> Result<i32, String> {
        let cwd = cwd.unwrap_or_else(|| self.cwd.clone());
        let agent_id = format!("{agent_name}@{}", self.team_name);

        // Ensure agent inbox exists (empty file) before the process starts.
        tokio::task::spawn_blocking({
            let team_name = self.team_name.clone();
            let agent_name = agent_name.clone();
            move || inbox::ensure_inbox_file(&team_name, &agent_name)
        })
        .await
        .ok();

        let member = TeamMember {
            agent_id: agent_id.clone(),
            name: agent_name.clone(),
            agent_type: agent_type.clone().unwrap_or_else(|| "general-purpose".to_string()),
            model: model.clone(),
            joined_at: chrono::Utc::now().timestamp_millis(),
            tmux_pane_id: Some(String::new()),
            cwd: cwd.clone(),
            subscriptions: Some(Vec::new()),
        };

        tokio::task::spawn_blocking({
            let team_name = self.team_name.clone();
            move || team::add_member(&team_name, member)
        })
        .await
        .map_err(|e| format!("join team: {e}"))?
        .map_err(|e| format!("join team: {e}"))?;

        let mut merged_env = self.default_env.clone();
        merged_env.extend(env);
        let pid = tokio::task::spawn_blocking({
            let pm = self.processes.clone();
            let opts = SpawnAgentOptions {
                team_name: self.team_name.clone(),
                agent_name: agent_name.clone(),
                agent_id,
                agent_type,
                model,
                cwd,
                parent_session_id: Some(self.lead_session_id.clone()),
                color: None,
                claude_binary: self.claude_binary.clone(),
                permission_mode,
                allowed_tools,
                env: merged_env,
            };
            move || pm.spawn(opts)
        })
        .await
        .map_err(|e| format!("spawn: {e}"))??;

        Ok(pid)
    }

    pub async fn send(
        &self,
        agent_name: &str,
        message: &str,
        summary: Option<String>,
    ) -> Result<(), String> {
        let msg = InboxMessage {
            from: self.controller_name.clone(),
            text: message.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            color: None,
            read: false,
            summary,
        };
        tokio::task::spawn_blocking({
            let team_name = self.team_name.clone();
            let agent_name = agent_name.to_string();
            move || inbox::write_inbox(&team_name, &agent_name, msg)
        })
        .await
        .map_err(|e| format!("send: {e}"))?
    }

    pub async fn shutdown_agent(&self, agent_name: &str, reason: &str) -> Result<(), String> {
        let request_id = format!("shutdown-{}@{}", chrono::Utc::now().timestamp_millis(), agent_name);
        let msg = serde_json::json!({
            "type": "shutdown_request",
            "requestId": request_id,
            "from": "controller",
            "reason": reason,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })
        .to_string();
        self.send(agent_name, &msg, Some("shutdown request".to_string()))
            .await
    }

    pub async fn kill_agent(&self, agent_name: &str) -> Result<(), String> {
        let name = agent_name.to_string();
        let pm = self.processes.clone();
        tokio::task::spawn_blocking(move || pm.kill(&name))
            .await
            .map_err(|e| format!("kill: {e}"))?;
        tokio::task::spawn_blocking({
            let team_name = self.team_name.clone();
            let name = agent_name.to_string();
            move || team::remove_member(&team_name, &name)
        })
        .await
        .ok();
        Ok(())
    }

    pub async fn is_agent_running(&self, agent_name: &str) -> bool {
        self.processes.is_running(agent_name)
    }

    pub async fn list_agents(&self) -> Result<Vec<(String, String, Option<String>, bool)>, String> {
        let config = tokio::task::spawn_blocking({
            let team_name = self.team_name.clone();
            move || team::read_config(&team_name)
        })
        .await
        .map_err(|e| format!("read config: {e}"))??;

        let mut out = Vec::new();
        for m in config.members {
            if m.name == self.controller_name {
                continue;
            }
            let running = self.processes.is_running(&m.name);
            out.push((m.name, m.agent_type, m.model, running));
        }
        Ok(out)
    }
}

async fn verify_claude_teammate_support(claude_binary: &str) -> Result<(), String> {
    let claude_binary = claude_binary.to_string();
    tokio::task::spawn_blocking(move || {
        let version = Command::new(&claude_binary)
            .arg("--version")
            .output()
            .map_err(|e| format!("Failed to execute claude --version: {e}"))?;
        if !version.status.success() {
            let stderr = String::from_utf8_lossy(&version.stderr);
            return Err(format!(
                "claude --version failed (status={:?}): {}",
                version.status.code(),
                stderr.trim()
            ));
        }

        let help = Command::new(&claude_binary)
            .arg("--help")
            .output()
            .map_err(|e| format!("Failed to execute claude --help: {e}"))?;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&help.stdout),
            String::from_utf8_lossy(&help.stderr)
        );
        let has_team = text.contains("--team-name");
        let has_mode = text.contains("--teammate-mode");
        if has_team && has_mode {
            Ok(())
        } else {
            Err("Claude CLI does not appear to support teammate agent teams (missing --team-name/--teammate-mode in --help). Upgrade claude-code or disable teammate mode.".to_string())
        }
    })
    .await
    .map_err(|e| format!("verify claude: {e}"))?
}
