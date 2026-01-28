use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};

use chrono::Utc;
use serenity::all::{ButtonStyle, ComponentInteractionDataKind, Interaction};
use serenity::async_trait;
use serenity::builder::{
    CreateActionRow, CreateButton, CreateCommand, CreateCommandOption, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, CreateMessage,
    CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, CreateThread,
};
use serenity::http::Http;
use serenity::model::application::{CommandDataOptionValue, CommandOptionType};
use serenity::model::channel::{Channel, Message};
use serenity::model::id::{ChannelId, UserId};
use serenity::prelude::*;
use uuid::Uuid;

use crate::db;
use crate::{AppState, PendingDiscordTask, PendingUserInput, Settings};
use tauri::{AppHandle, Manager};

use serenity::all::ShardManager;

#[derive(Clone)]
pub struct DiscordBotHandle {
    http: Arc<Http>,
    channel_id: ChannelId,
    bot_user_id: Arc<StdMutex<Option<UserId>>>,
    shard_manager: Arc<ShardManager>,
}

impl DiscordBotHandle {
    pub async fn shutdown(&self) {
        self.shard_manager.shutdown_all().await;
    }

    pub fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    pub fn bot_user_id(&self) -> Option<UserId> {
        self.bot_user_id.lock().ok().and_then(|g| *g)
    }

    pub async fn send_channel_message(&self, content: &str) -> Result<Message, String> {
        self.channel_id
            .send_message(&self.http, CreateMessage::new().content(content))
            .await
            .map_err(|e| format!("Discord send_message failed: {e}"))
    }

    pub async fn send_thread_message(
        &self,
        thread_id: ChannelId,
        content: &str,
    ) -> Result<(), String> {
        thread_id
            .send_message(&self.http, CreateMessage::new().content(content))
            .await
            .map_err(|e| format!("Discord thread send_message failed: {e}"))?;
        Ok(())
    }

    pub async fn send_thread_message_with_components(
        &self,
        thread_id: ChannelId,
        content: &str,
        components: Vec<CreateActionRow>,
    ) -> Result<(), String> {
        thread_id
            .send_message(
                &self.http,
                CreateMessage::new().content(content).components(components),
            )
            .await
            .map_err(|e| format!("Discord thread send_message failed: {e}"))?;
        Ok(())
    }
}

struct DiscordEventHandler {
    app: AppHandle,
    channel_id: ChannelId,
    bot_user_id: Arc<StdMutex<Option<UserId>>>,
}

#[async_trait]
impl EventHandler for DiscordEventHandler {
    async fn ready(&self, ctx: Context, ready: serenity::model::gateway::Ready) {
        if let Ok(mut guard) = self.bot_user_id.lock() {
            *guard = Some(ready.user.id);
        }
        println!("[Discord] Bot ready: {}", ready.user.name);
        // Warm cache by fetching channel info
        if let Ok(channel) = self.channel_id.to_channel(&ctx.http).await {
            if let Channel::Guild(guild_channel) = channel {
                let guild_id = guild_channel.guild_id;
                let command = CreateCommand::new("task")
                    .description("Create a Phantom task")
                    .add_option(
                        CreateCommandOption::new(
                            CommandOptionType::String,
                            "prompt",
                            "Task prompt",
                        )
                        .required(true),
                    )
                    .add_option(
                        CreateCommandOption::new(
                            CommandOptionType::String,
                            "project",
                            "Project name or path",
                        )
                        .required(true),
                    )
                    .add_option(
                        CreateCommandOption::new(CommandOptionType::String, "agent", "Agent id")
                            .required(false),
                    )
                    .add_option(
                        CreateCommandOption::new(CommandOptionType::String, "model", "Model id")
                            .required(false),
                    );
                if let Err(err) = guild_id.create_command(&ctx.http, command).await {
                    println!("[Discord] Failed to register /task: {err}");
                }
            }
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        let state = self.app.state::<crate::AppState>().inner().clone();
        let app = self.app.clone();

        if msg.channel_id == self.channel_id {
            let settings = state.settings.lock().await.clone();
            let allowlist = project_allowlist(&settings);
            let bot_user_id = self.bot_user_id.lock().ok().and_then(|g| *g);
            if let Some(bot_user_id) = bot_user_id {
                let mentioned = msg.mentions.iter().any(|user| user.id == bot_user_id);
                if mentioned {
                    let prompt = strip_bot_mentions(&msg.content, bot_user_id);
                    if prompt.trim().is_empty() {
                        let _ = msg
                            .channel_id
                            .send_message(
                                &ctx.http,
                                CreateMessage::new().content(
                                    "Mention the bot with your task prompt. Example: @Phantom fix tests",
                                ),
                            )
                            .await;
                        return;
                    }

                    if allowlist.is_empty() {
                        let _ = msg
                            .channel_id
                            .send_message(
                                &ctx.http,
                                CreateMessage::new().content(
                                    "No project allowlist configured. Update Settings > Task Projects.",
                                ),
                            )
                            .await;
                        return;
                    }

                    let pending_id = Uuid::new_v4().to_string();
                    let pending = PendingDiscordTask {
                        prompt,
                        requester_id: msg.author.id.get(),
                        channel_id: msg.channel_id.get(),
                        project_path: None,
                        agent_id: None,
                        model: None,
                        created_at: Utc::now().timestamp(),
                        ephemeral: false,
                    };
                    let (components, truncated) =
                        build_project_action_rows(&allowlist, &pending_id);
                    {
                        let mut pending_guard = state.pending_discord_tasks.lock().await;
                        prune_pending_discord_tasks(&mut pending_guard);
                        pending_guard.insert(pending_id.clone(), pending);
                    }

                    let mut content = "Pick a project for this task:".to_string();
                    if truncated {
                        content.push_str(
                            " (Too many projects; reply with `project: <name>` to use another.)",
                        );
                    }
                    let _ = msg
                        .channel_id
                        .send_message(
                            &ctx.http,
                            CreateMessage::new().content(content).components(components),
                        )
                        .await;
                    return;
                }
            }

            if let Some((key, value)) = parse_task_override(&msg.content) {
                let pending_id_opt = {
                    let pending_guard = state.pending_discord_tasks.lock().await;
                    latest_pending_for_user(
                        &pending_guard,
                        msg.author.id.get(),
                        msg.channel_id.get(),
                    )
                    .map(|(id, _)| id)
                };

                if let Some(pending_id) = pending_id_opt {
                    let mut pending_guard = state.pending_discord_tasks.lock().await;
                    prune_pending_discord_tasks(&mut pending_guard);
                    let pending = match pending_guard.get_mut(&pending_id) {
                        Some(pending) => pending,
                        None => return,
                    };

                    if key == "project" {
                        match resolve_project_match(&allowlist, &value) {
                            Ok(path) => {
                                pending.project_path = Some(path);
                            }
                            Err(err) => {
                                let _ = msg
                                    .channel_id
                                    .send_message(&ctx.http, CreateMessage::new().content(err))
                                    .await;
                                return;
                            }
                        }
                    } else if key == "agent" {
                        if !agent_exists(&state, &value) {
                            let _ = msg
                                .channel_id
                                .send_message(
                                    &ctx.http,
                                    CreateMessage::new().content(
                                        "Unknown agent. Use the buttons to pick an agent.",
                                    ),
                                )
                                .await;
                            return;
                        }
                        pending.agent_id = Some(value);
                    } else if key == "model" {
                        pending.model = Some(value);
                    }

                    let pending_snapshot = pending.clone();
                    drop(pending_guard);

                    if pending_snapshot.project_path.is_none() {
                        let (components, truncated) =
                            build_project_action_rows(&allowlist, &pending_id);
                        let mut content = "Pick a project for this task:".to_string();
                        if truncated {
                            content.push_str(
                                " (Too many projects; reply with `project: <name>` to use another.)",
                            );
                        }
                        let _ = msg
                            .channel_id
                            .send_message(
                                &ctx.http,
                                CreateMessage::new().content(content).components(components),
                            )
                            .await;
                        return;
                    }

                    if pending_snapshot.agent_id.is_none() {
                        let components = build_agent_action_rows(&state, &pending_id);
                        let _ = msg
                            .channel_id
                            .send_message(
                                &ctx.http,
                                CreateMessage::new()
                                    .content("Pick an agent for this task:")
                                    .components(components),
                            )
                            .await;
                        return;
                    }

                    if pending_snapshot.model.is_none() {
                        let agent_id = pending_snapshot
                            .agent_id
                            .clone()
                            .unwrap_or_else(|| "default".to_string());
                        let (components, truncated) =
                            build_model_action_rows(&state, &pending_id, &agent_id);
                        let mut content = format!("Select a model for `{}`:", agent_id);
                        if truncated {
                            content.push_str(
                                " (Too many models; reply with `model: <id>` to use another.)",
                            );
                        }
                        let _ = msg
                            .channel_id
                            .send_message(
                                &ctx.http,
                                CreateMessage::new().content(content).components(components),
                            )
                            .await;
                        return;
                    }

                    let agent_id = pending_snapshot
                        .agent_id
                        .clone()
                        .unwrap_or_else(|| "default".to_string());
                    let project_path = pending_snapshot.project_path.clone().unwrap_or_default();
                    let model = pending_snapshot
                        .model
                        .clone()
                        .unwrap_or_else(|| "default".to_string());
                    {
                        let mut pending_guard = state.pending_discord_tasks.lock().await;
                        pending_guard.remove(&pending_id);
                    }
                    let _ = msg
                        .channel_id
                        .send_message(&ctx.http, CreateMessage::new().content("Creating task..."))
                        .await;
                    match crate::create_task_from_discord(
                        app.clone(),
                        &state,
                        pending_snapshot.prompt,
                        agent_id,
                        project_path,
                        model,
                    )
                    .await
                    {
                        Ok(task_id) => {
                            let _ = msg
                                .channel_id
                                .send_message(
                                    &ctx.http,
                                    CreateMessage::new().content(format!(
                                        "Task started: `{}`. A thread will appear shortly.",
                                        task_id
                                    )),
                                )
                                .await;
                        }
                        Err(err) => {
                            let _ = msg
                                .channel_id
                                .send_message(
                                    &ctx.http,
                                    CreateMessage::new()
                                        .content(format!("Failed to create task: {}", err)),
                                )
                                .await;
                        }
                    }
                }
            }

            return;
        }

        let thread_id = msg.channel_id.get();
        let task_id_opt = {
            let conn = match state.db.lock() {
                Ok(c) => c,
                Err(_) => return,
            };
            db::get_task_id_for_discord_thread(&conn, thread_id)
                .ok()
                .flatten()
        };

        let Some(task_id) = task_id_opt else {
            return;
        };

        let content = msg.content.trim();
        if content.is_empty() {
            return;
        }

        // Check for pending user input request.
        let pending = {
            let guard = state.pending_user_inputs.lock().await;
            guard.get(&task_id).cloned()
        };

        if let Some(pending_req) = pending {
            let answers = parse_user_input_answers(&pending_req, content, &pending_req.answers);
            if let Err(err) = crate::respond_to_user_input_internal(
                task_id.clone(),
                pending_req.request_id.clone(),
                answers,
                &state,
                app,
            )
            .await
            {
                println!("[Discord] Failed to respond to user input: {err}");
            }
            return;
        }

        if let Err(err) = crate::send_chat_message_internal(
            task_id,
            content.to_string(),
            &state,
            app,
            crate::MessageOrigin::Discord,
        )
        .await
        {
            println!("[Discord] Failed to send chat message: {err}");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                if command.data.name != "task" {
                    return;
                }

                if command.channel_id != self.channel_id {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Use /task in the configured Discord channel.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                }

                let state = self.app.state::<crate::AppState>().inner().clone();
                let settings = state.settings.lock().await.clone();
                let allowlist = project_allowlist(&settings);

                let mut prompt = None;
                let mut project = None;
                let mut agent_id = None;
                let mut model = None;
                for option in &command.data.options {
                    if let CommandDataOptionValue::String(value) = &option.value {
                        match option.name.as_str() {
                            "prompt" => prompt = Some(value.clone()),
                            "project" => project = Some(value.clone()),
                            "agent" => agent_id = Some(value.clone()),
                            "model" => model = Some(value.clone()),
                            _ => {}
                        }
                    }
                }

                let prompt = match prompt {
                    Some(prompt) if !prompt.trim().is_empty() => prompt,
                    _ => {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Provide a task prompt.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }
                };

                let project = match project {
                    Some(project) if !project.trim().is_empty() => project,
                    _ => {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Provide a project from the allowlist.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }
                };

                let project_path = match resolve_project_match(&allowlist, &project) {
                    Ok(path) => path,
                    Err(err) => {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(err)
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }
                };

                if let Some(ref agent_id) = agent_id {
                    if !agent_exists(&state, agent_id) {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Unknown agent id.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }
                }

                let pending_id = Uuid::new_v4().to_string();
                let pending = PendingDiscordTask {
                    prompt,
                    requester_id: command.user.id.get(),
                    channel_id: command.channel_id.get(),
                    project_path: Some(project_path.clone()),
                    agent_id: agent_id.clone(),
                    model: model.clone(),
                    created_at: Utc::now().timestamp(),
                    ephemeral: true,
                };

                if let (Some(agent_id), Some(model)) = (agent_id.clone(), model.clone()) {
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(
                                CreateInteractionResponseMessage::new().ephemeral(true),
                            ),
                        )
                        .await;
                    let state = state.clone();
                    let app = self.app.clone();
                    let http = ctx.http.clone();
                    let command = command.clone();
                    let project_path = project_path.clone();
                    tauri::async_runtime::spawn(async move {
                        let result = crate::create_task_from_discord(
                            app,
                            &state,
                            pending.prompt,
                            agent_id,
                            project_path,
                            model,
                        )
                        .await;
                        let content = match result {
                            Ok(task_id) => format!(
                                "Task started: `{}`. A thread will appear shortly.",
                                task_id
                            ),
                            Err(err) => format!("Failed to create task: {}", err),
                        };
                        let _ = command
                            .create_followup(
                                &http,
                                CreateInteractionResponseFollowup::new()
                                    .content(content)
                                    .ephemeral(true),
                            )
                            .await;
                    });
                    return;
                }

                {
                    let mut pending_guard = state.pending_discord_tasks.lock().await;
                    prune_pending_discord_tasks(&mut pending_guard);
                    pending_guard.insert(pending_id.clone(), pending);
                }

                if agent_id.is_none() {
                    let components = build_agent_action_rows(&state, &pending_id);
                    let _ = command
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Pick an agent for this task:")
                                    .components(components)
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                }

                let agent_id = agent_id.unwrap_or_else(|| "default".to_string());
                let (components, truncated) =
                    build_model_action_rows(&state, &pending_id, &agent_id);
                let mut content = format!("Select a model for `{}`:", agent_id);
                if truncated {
                    content
                        .push_str(" (Too many models; reply with `model: <id>` to use another.)");
                }
                let _ = command
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(content)
                                .components(components)
                                .ephemeral(true),
                        ),
                    )
                    .await;
            }
            Interaction::Component(component) => {
                let custom_id = component.data.custom_id.as_str();
                if let Some(action) = parse_task_create_action(custom_id) {
                    let state = self.app.state::<crate::AppState>().inner().clone();
                    let settings = state.settings.lock().await.clone();
                    let allowlist = project_allowlist(&settings);
                    let selected_value =
                        action.value.clone().or_else(|| match &component.data.kind {
                            ComponentInteractionDataKind::StringSelect { values } => {
                                values.get(0).cloned()
                            }
                            _ => None,
                        });

                    let selected_value = match selected_value {
                        Some(value) if !value.trim().is_empty() => value,
                        _ => {
                            let _ = component
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("Selection missing.")
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    let mut pending_guard = state.pending_discord_tasks.lock().await;
                    prune_pending_discord_tasks(&mut pending_guard);
                    let pending = match pending_guard.get_mut(&action.pending_id) {
                        Some(pending) => pending,
                        None => {
                            let _ = component
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("This task request has expired.")
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    if pending.requester_id != component.user.id.get() {
                        let _ = component
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Only the requester can select this.")
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }

                    match action.kind {
                        TaskCreateKind::Project => {
                            match resolve_project_match(&allowlist, &selected_value) {
                                Ok(path) => {
                                    pending.project_path = Some(path);
                                }
                                Err(err) => {
                                    let _ = component
                                        .create_response(
                                            &ctx.http,
                                            CreateInteractionResponse::Message(
                                                CreateInteractionResponseMessage::new()
                                                    .content(err)
                                                    .ephemeral(true),
                                            ),
                                        )
                                        .await;
                                    return;
                                }
                            }
                        }
                        TaskCreateKind::Agent => {
                            if !agent_exists(&state, &selected_value) {
                                let _ = component
                                    .create_response(
                                        &ctx.http,
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content("Unknown agent id.")
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await;
                                return;
                            }
                            pending.agent_id = Some(selected_value);
                        }
                        TaskCreateKind::Model => {
                            pending.model = Some(selected_value);
                        }
                    }

                    let pending_snapshot = pending.clone();
                    drop(pending_guard);

                    if pending_snapshot.project_path.is_none() {
                        if allowlist.is_empty() {
                            let _ = component
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content(
                                                "No project allowlist configured. Update Settings > Task Projects.",
                                            )
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                            return;
                        }
                        let (components, truncated) =
                            build_project_action_rows(&allowlist, &action.pending_id);
                        let mut content = "Pick a project for this task:".to_string();
                        if truncated {
                            content.push_str(
                                " (Too many projects; reply with `project: <name>` to use another.)",
                            );
                        }
                        let _ = component
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::UpdateMessage(
                                    CreateInteractionResponseMessage::new()
                                        .content(content)
                                        .components(components),
                                ),
                            )
                            .await;
                        return;
                    }

                    let Some(agent_id) = pending_snapshot.agent_id.clone() else {
                        let components = build_agent_action_rows(&state, &action.pending_id);
                        let _ = component
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::UpdateMessage(
                                    CreateInteractionResponseMessage::new()
                                        .content("Pick an agent for this task:")
                                        .components(components),
                                ),
                            )
                            .await;
                        return;
                    };

                    if pending_snapshot.model.is_none() {
                        let (components, truncated) =
                            build_model_action_rows(&state, &action.pending_id, &agent_id);
                        let mut content = format!("Select a model for `{}`:", agent_id);
                        if truncated {
                            content.push_str(
                                " (Too many models; reply with `model: <id>` to use another.)",
                            );
                        }
                        let _ = component
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::UpdateMessage(
                                    CreateInteractionResponseMessage::new()
                                        .content(content)
                                        .components(components),
                                ),
                            )
                            .await;
                        return;
                    }

                    let model = pending_snapshot
                        .model
                        .clone()
                        .unwrap_or_else(|| "default".to_string());
                    let project_path = pending_snapshot.project_path.clone().unwrap_or_default();
                    {
                        let mut pending_guard = state.pending_discord_tasks.lock().await;
                        pending_guard.remove(&action.pending_id);
                    }

                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .content("Creating task...")
                                    .components(Vec::new()),
                            ),
                        )
                        .await;

                    let component = component.clone();
                    let http = ctx.http.clone();
                    let app = self.app.clone();
                    tauri::async_runtime::spawn(async move {
                        let result = crate::create_task_from_discord(
                            app,
                            &state,
                            pending_snapshot.prompt,
                            agent_id,
                            project_path,
                            model,
                        )
                        .await;
                        let content = match result {
                            Ok(task_id) => format!(
                                "Task started: `{}`. A thread will appear shortly.",
                                task_id
                            ),
                            Err(err) => format!("Failed to create task: {}", err),
                        };
                        let _ = component
                            .create_followup(
                                &http,
                                CreateInteractionResponseFollowup::new()
                                    .content(content)
                                    .ephemeral(pending_snapshot.ephemeral),
                            )
                            .await;
                    });
                    return;
                }

                if !custom_id.starts_with("user_input:") {
                    return;
                }

                let thread_id = component.channel_id.get();

                let state = self.app.state::<crate::AppState>().inner().clone();
                let task_id_opt = {
                    let conn = match state.db.lock() {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    db::get_task_id_for_discord_thread(&conn, thread_id)
                        .ok()
                        .flatten()
                };
                let Some(task_id) = task_id_opt else {
                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("No task bound to this thread.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                };

                let Some((request_id, question_id, option_idx)) =
                    parse_user_input_custom_id(custom_id)
                else {
                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Unsupported input action.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                };

                let mut pending_guard = state.pending_user_inputs.lock().await;
                let Some(pending) = pending_guard.get_mut(&task_id) else {
                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("No pending input for this task.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                };

                if pending.request_id != request_id {
                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("This input request has expired.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                }

                let Some(question) = pending.questions.iter().find(|q| q.id == question_id) else {
                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Unknown question.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                };

                let Some(options) = question.options.as_ref() else {
                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("This question needs a typed response.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                };

                let Some(option) = options.get(option_idx) else {
                    let _ = component
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Option not found.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                    return;
                };

                pending
                    .answers
                    .insert(question.id.clone(), option.label.clone());
                let ready = pending_complete(pending);
                let answers_payload = if ready {
                    build_answers_payload(&pending.answers)
                } else {
                    serde_json::Value::Null
                };
                let request_id = pending.request_id.clone();
                drop(pending_guard);

                let ack_message = if ready {
                    "Answer recorded. Submitting your responses..."
                } else {
                    "Answer recorded. Please answer the remaining questions."
                };
                let _ = component
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(ack_message)
                                .ephemeral(true),
                        ),
                    )
                    .await;

                if ready {
                    if let Err(err) = crate::respond_to_user_input_internal(
                        task_id,
                        request_id,
                        answers_payload,
                        &state,
                        self.app.clone(),
                    )
                    .await
                    {
                        println!("[Discord] Failed to respond to user input: {err}");
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse_user_input_answers(
    pending: &PendingUserInput,
    content: &str,
    seed: &HashMap<String, String>,
) -> serde_json::Value {
    let mut answers: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    let lines: Vec<&str> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();

    for (qid, value) in seed {
        answers.insert(qid.clone(), serde_json::json!({ "answers": [value] }));
    }

    if pending.questions.len() == 1 {
        let q = &pending.questions[0];
        let value = normalize_answer_value(q, content.trim());
        answers.insert(q.id.clone(), serde_json::json!({ "answers": [value] }));
        return serde_json::Value::Object(answers);
    }

    for line in lines {
        let mut parts = line.splitn(2, ':');
        let key = parts.next().unwrap_or("").trim();
        let value_raw = parts.next().unwrap_or("").trim();
        if key.is_empty() || value_raw.is_empty() {
            continue;
        }
        if let Some(q) = pending.questions.iter().find(|q| q.id == key) {
            let value = normalize_answer_value(q, value_raw);
            answers.insert(q.id.clone(), serde_json::json!({ "answers": [value] }));
        }
    }

    if answers.is_empty() {
        // Fallback: map the entire message to the first question.
        if let Some(q) = pending.questions.first() {
            let value = normalize_answer_value(q, content.trim());
            answers.insert(q.id.clone(), serde_json::json!({ "answers": [value] }));
        }
    }

    serde_json::Value::Object(answers)
}

fn normalize_answer_value(
    question: &phantom_harness_backend::cli::UserInputQuestion,
    raw: &str,
) -> String {
    if let Some(options) = question.options.as_ref() {
        if let Some(opt) = options
            .iter()
            .find(|opt| opt.label.eq_ignore_ascii_case(raw))
        {
            return opt.label.clone();
        }
    }
    raw.to_string()
}

fn parse_user_input_custom_id(custom_id: &str) -> Option<(String, String, usize)> {
    let mut parts = custom_id.split(':');
    let prefix = parts.next()?;
    if prefix != "user_input" {
        return None;
    }
    let request_id = parts.next()?.to_string();
    let question_id = parts.next()?.to_string();
    let idx = parts.next()?.parse::<usize>().ok()?;
    Some((request_id, question_id, idx))
}

fn build_answers_payload(answers: &HashMap<String, String>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (qid, value) in answers {
        map.insert(qid.clone(), serde_json::json!({ "answers": [value] }));
    }
    serde_json::Value::Object(map)
}

fn pending_complete(pending: &PendingUserInput) -> bool {
    for q in &pending.questions {
        if q.options.is_none() && !pending.answers.contains_key(&q.id) {
            return false;
        }
        if q.options.is_some() && !pending.answers.contains_key(&q.id) {
            return false;
        }
    }
    true
}

#[derive(Debug, Clone)]
enum TaskCreateKind {
    Project,
    Agent,
    Model,
}

#[derive(Debug, Clone)]
struct TaskCreateAction {
    kind: TaskCreateKind,
    pending_id: String,
    value: Option<String>,
}

fn strip_bot_mentions(content: &str, bot_user_id: UserId) -> String {
    let mention = format!("<@{}>", bot_user_id.get());
    let mention_nick = format!("<@!{}>", bot_user_id.get());
    content
        .replace(&mention, "")
        .replace(&mention_nick, "")
        .trim()
        .to_string()
}

fn parse_task_override(content: &str) -> Option<(String, String)> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(2, ':');
        let key = parts.next().unwrap_or("").trim().to_lowercase();
        let value = parts.next().unwrap_or("").trim();
        if value.is_empty() {
            continue;
        }
        if key == "agent" || key == "model" || key == "project" {
            return Some((key, value.to_string()));
        }
    }
    None
}

fn parse_task_create_action(custom_id: &str) -> Option<TaskCreateAction> {
    let mut parts = custom_id.splitn(4, ':');
    let prefix = parts.next()?;
    if prefix != "task_create" {
        return None;
    }
    let kind = match parts.next()? {
        "project" => TaskCreateKind::Project,
        "agent" => TaskCreateKind::Agent,
        "model" => TaskCreateKind::Model,
        _ => return None,
    };
    let pending_id = parts.next()?.to_string();
    let value = parts.next().map(|value| value.to_string());
    Some(TaskCreateAction {
        kind,
        pending_id,
        value,
    })
}

fn project_allowlist(settings: &Settings) -> Vec<String> {
    settings
        .task_project_allowlist
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn normalize_project_token(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn project_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_string()
}

fn compact_path(path: &str, max_len: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max_len {
        return path.to_string();
    }
    let tail_len = max_len.saturating_sub(4);
    let tail = chars[chars.len().saturating_sub(tail_len)..]
        .iter()
        .collect::<String>();
    format!("…{}", tail)
}

fn resolve_project_match(allowlist: &[String], query: &str) -> Result<String, String> {
    if allowlist.is_empty() {
        return Err(
            "No project allowlist configured. Update Settings > Task Projects.".to_string(),
        );
    }
    let normalized_query = normalize_project_token(query.trim());
    if normalized_query.is_empty() {
        return Err("Provide a project name from the allowlist.".to_string());
    }

    let mut matches: Vec<(i32, String, String)> = Vec::new();
    for path in allowlist {
        let base = project_label(path);
        let base_norm = normalize_project_token(&base);
        let path_norm = normalize_project_token(path);

        let mut score = 0;
        if !base_norm.is_empty() {
            if base_norm == normalized_query {
                score = score.max(4);
            } else if base_norm.starts_with(&normalized_query) {
                score = score.max(3);
            } else if base_norm.contains(&normalized_query) {
                score = score.max(2);
            }
        }
        if !path_norm.is_empty() {
            if path_norm == normalized_query {
                score = score.max(3);
            } else if path_norm.contains(&normalized_query) {
                score = score.max(2);
            }
        }

        if score > 0 {
            matches.push((score, path.clone(), base));
        }
    }

    if matches.is_empty() {
        let names = allowlist
            .iter()
            .map(|path| project_label(path))
            .take(6)
            .collect::<Vec<_>>();
        let hint = if names.is_empty() {
            "".to_string()
        } else {
            format!(" Available: {}", names.join(", "))
        };
        return Err(format!("No project matches `{}`.{}", query.trim(), hint));
    }

    matches.sort_by(|a, b| b.0.cmp(&a.0));
    let best_score = matches[0].0;
    let best_matches: Vec<(String, String)> = matches
        .into_iter()
        .filter(|(score, _, _)| *score == best_score)
        .map(|(_, path, base)| (path, base))
        .collect();

    if best_matches.len() > 1 {
        let names = best_matches
            .iter()
            .map(|(_, base)| base.clone())
            .take(6)
            .collect::<Vec<_>>();
        return Err(format!(
            "Multiple projects match `{}`: {}. Be more specific.",
            query.trim(),
            names.join(", ")
        ));
    }

    Ok(best_matches[0].0.clone())
}

fn latest_pending_for_user(
    pending: &HashMap<String, PendingDiscordTask>,
    requester_id: u64,
    channel_id: u64,
) -> Option<(String, PendingDiscordTask)> {
    pending
        .iter()
        .filter(|(_, task)| task.requester_id == requester_id && task.channel_id == channel_id)
        .max_by_key(|(_, task)| task.created_at)
        .map(|(id, task)| (id.clone(), task.clone()))
}

fn prune_pending_discord_tasks(pending: &mut HashMap<String, PendingDiscordTask>) {
    const TTL_SECONDS: i64 = 15 * 60;
    let cutoff = Utc::now().timestamp() - TTL_SECONDS;
    pending.retain(|_, task| task.created_at >= cutoff);
}

fn agent_exists(state: &AppState, agent_id: &str) -> bool {
    state.config.agents.iter().any(|agent| agent.id == agent_id)
}

fn build_project_action_rows(
    allowlist: &[String],
    pending_id: &str,
) -> (Vec<CreateActionRow>, bool) {
    let mut seen = HashSet::new();
    let mut options: Vec<CreateSelectMenuOption> = Vec::new();
    for path in allowlist {
        let trimmed = path.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        let label = project_label(trimmed);
        let description = compact_path(trimmed, 90);
        let mut option = CreateSelectMenuOption::new(label, trimmed.to_string());
        if !description.is_empty() {
            option = option.description(description);
        }
        options.push(option);
    }

    let truncated = options.len() > 25;
    let options = options.into_iter().take(25).collect::<Vec<_>>();

    let menu = CreateSelectMenu::new(
        format!("task_create:project:{}", pending_id),
        CreateSelectMenuKind::String { options },
    )
    .placeholder("Select a project")
    .min_values(1)
    .max_values(1);

    (vec![CreateActionRow::SelectMenu(menu)], truncated)
}

fn build_agent_action_rows(state: &AppState, pending_id: &str) -> Vec<CreateActionRow> {
    let mut rows: Vec<CreateActionRow> = Vec::new();
    let mut current_row: Vec<CreateButton> = Vec::new();

    for agent in state.config.agents.iter().take(25) {
        let label = agent
            .display_name
            .clone()
            .unwrap_or_else(|| agent.id.clone());
        let custom_id = format!("task_create:agent:{}:{}", pending_id, agent.id);
        let button = CreateButton::new(custom_id)
            .label(label)
            .style(ButtonStyle::Primary);
        current_row.push(button);
        if current_row.len() == 5 {
            rows.push(CreateActionRow::Buttons(current_row));
            current_row = Vec::new();
        }
    }

    if !current_row.is_empty() {
        rows.push(CreateActionRow::Buttons(current_row));
    }

    rows
}

fn build_model_action_rows(
    state: &AppState,
    pending_id: &str,
    agent_id: &str,
) -> (Vec<CreateActionRow>, bool) {
    let options = model_options_for_agent(state, agent_id);
    let truncated = options.len() > 25;
    let mut select_options: Vec<CreateSelectMenuOption> = Vec::new();
    for option in options.into_iter().take(25) {
        let mut entry = CreateSelectMenuOption::new(option.label, option.value);
        if let Some(description) = option.description {
            entry = entry.description(description);
        }
        select_options.push(entry);
    }

    if select_options.is_empty() {
        select_options.push(CreateSelectMenuOption::new("default", "default"));
    }

    let menu = CreateSelectMenu::new(
        format!("task_create:model:{}", pending_id),
        CreateSelectMenuKind::String {
            options: select_options,
        },
    )
    .placeholder("Select a model")
    .min_values(1)
    .max_values(1);

    (vec![CreateActionRow::SelectMenu(menu)], truncated)
}

struct ModelOption {
    value: String,
    label: String,
    description: Option<String>,
}

fn model_options_for_agent(state: &AppState, agent_id: &str) -> Vec<ModelOption> {
    let cached_models = {
        let conn = match state.db.lock() {
            Ok(conn) => conn,
            Err(_) => {
                return vec![ModelOption {
                    value: "default".to_string(),
                    label: "default".to_string(),
                    description: None,
                }]
            }
        };
        db::get_cached_models(&conn, agent_id).unwrap_or_default()
    };

    let mut options: Vec<ModelOption> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut push_option = |value: String, label: String, description: Option<String>| {
        if seen.insert(value.clone()) {
            options.push(ModelOption {
                value,
                label,
                description,
            });
        }
    };

    push_option("default".to_string(), "default".to_string(), None);

    if !cached_models.is_empty() {
        for model in cached_models {
            let label = model.name.clone().unwrap_or_else(|| model.value.clone());
            push_option(model.value, label, model.description);
        }
        return options;
    }

    if let Some(agent) = state
        .config
        .agents
        .iter()
        .find(|agent| agent.id == agent_id)
    {
        if let Some(default_exec) = agent.default_exec_model.clone() {
            push_option(default_exec.clone(), default_exec, None);
        }
        if let Some(default_plan) = agent.default_plan_model.clone() {
            push_option(default_plan.clone(), default_plan, None);
        }
        for model in &agent.models {
            push_option(model.clone(), model.clone(), None);
        }
    }

    options
}

pub async fn start_discord_bot(
    app: AppHandle,
    settings: &Settings,
) -> Result<DiscordBotHandle, String> {
    let token = settings
        .discord_bot_token
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Discord bot token is missing".to_string())?
        .to_string();
    let channel_id_raw = settings
        .discord_channel_id
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Discord channel ID is missing".to_string())?
        .to_string();

    let channel_id = channel_id_raw
        .trim()
        .parse::<u64>()
        .map_err(|_| "Discord channel ID must be numeric".to_string())?;

    let intents =
        GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILDS | GatewayIntents::MESSAGE_CONTENT;

    let bot_user_id: Arc<StdMutex<Option<UserId>>> = Arc::new(StdMutex::new(None));

    let handler = DiscordEventHandler {
        app,
        channel_id: ChannelId::new(channel_id),
        bot_user_id: bot_user_id.clone(),
    };

    let mut client = Client::builder(token, intents)
        .event_handler(handler)
        .await
        .map_err(|e| format!("Discord client init failed: {e}"))?;

    let shard_manager = client.shard_manager.clone();
    let http = client.http.clone();

    tauri::async_runtime::spawn(async move {
        if let Err(e) = client.start().await {
            println!("[Discord] Client error: {e}");
        }
    });

    Ok(DiscordBotHandle {
        http,
        channel_id: ChannelId::new(channel_id),
        bot_user_id,
        shard_manager,
    })
}

pub async fn ensure_thread_for_task(
    handle: &DiscordBotHandle,
    db_conn: Arc<StdMutex<rusqlite::Connection>>,
    task_id: &str,
    thread_name: &str,
    intro_message: &str,
) -> Result<ChannelId, String> {
    if let Some(thread_id) = {
        let conn = db_conn.lock().map_err(|e| e.to_string())?;
        db::get_discord_thread_id(&conn, task_id).map_err(|e| e.to_string())?
    } {
        return Ok(ChannelId::new(thread_id));
    }

    let message = handle.send_channel_message(intro_message).await?;

    let thread = message
        .channel_id
        .create_thread_from_message(&handle.http, message.id, CreateThread::new(thread_name))
        .await
        .map_err(|e| format!("Discord create_thread failed: {e}"))?;

    {
        let conn = db_conn.lock().map_err(|e| e.to_string())?;
        db::save_discord_thread(&conn, task_id, thread.id.get(), handle.channel_id().get())
            .map_err(|e| e.to_string())?;
    }
    Ok(thread.id)
}

pub async fn post_task_notification(
    handle: &DiscordBotHandle,
    db_conn: Arc<StdMutex<rusqlite::Connection>>,
    task_id: &str,
    thread_name: &str,
    content: &str,
) -> Result<ChannelId, String> {
    let thread_id = ensure_thread_for_task(handle, db_conn, task_id, thread_name, content).await?;
    handle.send_thread_message(thread_id, content).await?;
    Ok(thread_id)
}

pub async fn post_to_thread(
    handle: &DiscordBotHandle,
    db_conn: Arc<StdMutex<rusqlite::Connection>>,
    task_id: &str,
    content: &str,
) -> Result<(), String> {
    let thread_id = {
        let conn = db_conn.lock().map_err(|e| e.to_string())?;
        db::get_discord_thread_id(&conn, task_id).map_err(|e| e.to_string())?
    };
    let thread_id = match thread_id {
        Some(id) => id,
        None => return Ok(()),
    };
    handle
        .send_thread_message(ChannelId::new(thread_id), content)
        .await
}

pub async fn post_user_input_question(
    handle: &DiscordBotHandle,
    db_conn: Arc<StdMutex<rusqlite::Connection>>,
    task_id: &str,
    request_id: &str,
    question: &phantom_harness_backend::cli::UserInputQuestion,
) -> Result<(), String> {
    let thread_id = {
        let conn = db_conn.lock().map_err(|e| e.to_string())?;
        db::get_discord_thread_id(&conn, task_id).map_err(|e| e.to_string())?
    };
    let thread_id = match thread_id {
        Some(id) => id,
        None => return Ok(()),
    };

    let mut rows: Vec<CreateActionRow> = Vec::new();
    let mut current_row: Vec<CreateButton> = Vec::new();

    let Some(options) = question.options.as_ref() else {
        return Ok(());
    };

    for (idx, opt) in options.iter().enumerate() {
        let custom_id = format!("user_input:{}:{}:{}", request_id, question.id, idx);
        let button = CreateButton::new(custom_id)
            .label(opt.label.clone())
            .style(ButtonStyle::Primary);
        current_row.push(button);
        if current_row.len() == 5 {
            rows.push(CreateActionRow::Buttons(current_row));
            current_row = Vec::new();
        }
        if rows.len() == 5 {
            break;
        }
    }
    if !current_row.is_empty() && rows.len() < 5 {
        rows.push(CreateActionRow::Buttons(current_row));
    }

    let content = format!(
        "**{}** (`{}`)\n{}",
        question.header, question.id, question.question
    );
    handle
        .send_thread_message_with_components(ChannelId::new(thread_id), &content, rows)
        .await
}
