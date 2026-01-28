use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use serenity::all::{ButtonStyle, Interaction};
use serenity::async_trait;
use serenity::builder::{
    CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateMessage, CreateThread,
};
use serenity::http::Http;
use serenity::model::channel::Message;
use serenity::model::id::{ChannelId, UserId};
use serenity::prelude::*;

use crate::db;
use crate::{PendingUserInput, Settings};
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
        let _ = self.channel_id.to_channel(&ctx.http).await;
    }

    async fn message(&self, _ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }
        if msg.channel_id == self.channel_id {
            // Ignore direct channel chatter; we only bind threads to tasks.
            return;
        }

        let state = self.app.state::<crate::AppState>().inner().clone();
        let app = self.app.clone();
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
        let Interaction::Component(component) = interaction else {
            return;
        };

        let custom_id = component.data.custom_id.as_str();
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

        let Some((request_id, question_id, option_idx)) = parse_user_input_custom_id(custom_id)
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
