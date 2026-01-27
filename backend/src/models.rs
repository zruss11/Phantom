use crate::cli::{
    AgentProcessClient, AvailableMode, CodexModeInfo, ConfigOption, ConfigOptionValue, ModelInfo,
    NewSessionResult,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOption {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Reasoning effort option for models that support thinking levels
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEffort {
    pub value: String,
    pub description: Option<String>,
}

/// Enriched model option with reasoning effort support (for Codex models)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichedModelOption {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub supported_reasoning_efforts: Vec<ReasoningEffort>,
    pub default_reasoning_effort: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

impl From<&ModelInfo> for ModelOption {
    fn from(info: &ModelInfo) -> Self {
        ModelOption {
            value: info.id.clone(),
            name: info.label.clone(),
            description: info.description.clone(),
        }
    }
}

/// Mode option for agent mode selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeOption {
    pub value: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl From<&AvailableMode> for ModeOption {
    fn from(mode: &AvailableMode) -> Self {
        ModeOption {
            value: mode.id.clone(),
            name: Some(mode.name.clone()),
            description: mode.description.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentLaunchConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: String,
}

pub async fn get_agent_models(config: AgentLaunchConfig) -> Result<Vec<ModelOption>> {
    let mut client = AgentProcessClient::spawn(
        &config.command,
        &config.args,
        Path::new(&config.cwd),
        &config.env,
    )
    .await?;

    client.initialize("Phantom Harness", "0.1.0").await?;
    let session = client.session_new(&config.cwd).await?;
    let models = extract_model_options(&session)?;
    client.shutdown().await?;
    Ok(models)
}

/// Fetch available models from Codex app-server using the model/list API.
/// This provides dynamic model information including reasoning effort options.
pub async fn get_codex_models(config: AgentLaunchConfig) -> Result<Vec<ModelOption>> {
    let mut client = AgentProcessClient::spawn(
        &config.command,
        &["app-server".to_string()], // Override args to use app-server
        Path::new(&config.cwd),
        &config.env,
    )
    .await?;

    let codex_models = client.fetch_codex_models().await?;
    client.shutdown().await?;

    // Convert CodexModelInfo to ModelOption
    Ok(codex_models
        .into_iter()
        .map(|m| ModelOption {
            value: m.model.unwrap_or(m.id.clone()),
            name: m.display_name.or(Some(m.id)),
            description: m.description,
        })
        .collect())
}

/// Fetch enriched models from Codex app-server including reasoning effort info.
pub async fn get_codex_models_enriched(config: AgentLaunchConfig) -> Result<Vec<EnrichedModelOption>> {
    let mut client = AgentProcessClient::spawn(
        &config.command,
        &["app-server".to_string()],
        Path::new(&config.cwd),
        &config.env,
    )
    .await?;

    let codex_models = client.fetch_codex_models().await?;
    client.shutdown().await?;

    // Convert CodexModelInfo to EnrichedModelOption with reasoning efforts
    Ok(codex_models
        .into_iter()
        .map(|m| EnrichedModelOption {
            value: m.model.clone().unwrap_or(m.id.clone()),
            name: m.display_name.or(Some(m.id)),
            description: m.description,
            supported_reasoning_efforts: m
                .supported_reasoning_efforts
                .into_iter()
                .map(|e| ReasoningEffort {
                    value: e.reasoning_effort,
                    description: e.description,
                })
                .collect(),
            default_reasoning_effort: m.default_reasoning_effort,
            is_default: m.is_default,
        })
        .collect())
}

pub async fn set_session_model(
    mut client: AgentProcessClient,
    session_id: &str,
    config_id: &str,
    model_value: &str,
) -> Result<Vec<ConfigOption>> {
    client
        .set_config_option(session_id, config_id, model_value)
        .await
}

/// Apply model selection to a session.
/// Tries the native session/set_model API first, falls back to config options.
pub async fn apply_model_selection(
    client: &mut AgentProcessClient,
    session: &NewSessionResult,
    model_value: &str,
) -> Result<Vec<ConfigOption>> {
    if client
        .set_session_model(&session.session_id, model_value)
        .await
        .is_ok()
    {
        return Ok(Vec::new());
    }

    // Fall back to old configOptions API
    let config_id =
        find_model_config_id(&session.config_options).context("model config id missing")?;
    client
        .set_config_option(&session.session_id, &config_id, model_value)
        .await
}

pub fn find_model_config_id(options: &[ConfigOption]) -> Option<String> {
    options
        .iter()
        .find(|option| option.category.as_deref() == Some("model"))
        .or_else(|| options.iter().find(|option| option.id == "model"))
        .map(|option| option.id.clone())
}

fn extract_model_options(session: &NewSessionResult) -> Result<Vec<ModelOption>> {
    // Try new format first: session.models.available_models
    if let Some(ref models_state) = session.models {
        if !models_state.available_models.is_empty() {
            return Ok(models_state
                .available_models
                .iter()
                .map(ModelOption::from)
                .collect());
        }
    }

    // Fall back to old configOptions format
    let model_option = session
        .config_options
        .iter()
        .find(|option| option.category.as_deref() == Some("model"))
        .or_else(|| {
            session
                .config_options
                .iter()
                .find(|option| option.id == "model")
        })
        .context("no model config option returned by agent")?;
    Ok(map_model_values(&model_option.options))
}

fn map_model_values(options: &[ConfigOptionValue]) -> Vec<ModelOption> {
    options
        .iter()
        .map(|option| ModelOption {
            value: option.value.clone(),
            name: option.name.clone(),
            description: option.description.clone(),
        })
        .collect()
}

/// Extract available modes from a session
pub fn extract_mode_options(session: &NewSessionResult) -> Vec<ModeOption> {
    session
        .modes
        .as_ref()
        .map(|m| m.available_modes.iter().map(ModeOption::from).collect())
        .unwrap_or_default()
}

/// Fetch available modes by spawning a CLI client and reading session/new response
pub async fn get_agent_modes(config: AgentLaunchConfig) -> Result<Vec<ModeOption>> {
    let mut client = AgentProcessClient::spawn(
        &config.command,
        &config.args,
        Path::new(&config.cwd),
        &config.env,
    )
    .await?;

    client.initialize("Phantom Harness", "0.1.0").await?;
    let session = client.session_new(&config.cwd).await?;
    let modes = extract_mode_options(&session);
    client.shutdown().await?;
    Ok(modes)
}

/// Fetch available modes from Codex app-server using the mode/list API.
/// Returns hardcoded fallback modes if the endpoint is not supported.
pub async fn get_codex_modes(config: AgentLaunchConfig) -> Result<Vec<ModeOption>> {
    let mut client = AgentProcessClient::spawn(
        &config.command,
        &["app-server".to_string()], // Override args to use app-server
        Path::new(&config.cwd),
        &config.env,
    )
    .await?;

    let codex_modes = client.fetch_codex_modes().await?;
    client.shutdown().await?;

    // Convert CodexModeInfo to ModeOption
    let mut modes: Vec<ModeOption> = codex_modes
        .into_iter()
        .map(|m| ModeOption {
            value: m.id,
            name: m.name,
            description: m.description,
        })
        .collect();

    let filtered: Vec<ModeOption> = modes
        .iter()
        .cloned()
        .filter(|m| m.value == "default" || m.value == "plan")
        .collect();
    if !filtered.is_empty() {
        modes = filtered;
    }

    Ok(modes)
}

impl From<&CodexModeInfo> for ModeOption {
    fn from(mode: &CodexModeInfo) -> Self {
        ModeOption {
            value: mode.id.clone(),
            name: mode.name.clone(),
            description: mode.description.clone(),
        }
    }
}

/// Custom model entry from Factory's ~/.factory/settings.json
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FactoryCustomModel {
    model: String,
    id: Option<String>,
    display_name: Option<String>,
    #[allow(dead_code)]
    base_url: Option<String>,
    #[allow(dead_code)]
    api_key: Option<String>,
    #[allow(dead_code)]
    provider: Option<String>,
}

/// Factory settings.json structure
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FactorySettings {
    #[serde(default)]
    custom_models: Vec<FactoryCustomModel>,
}

/// Read custom BYOK models from Factory's ~/.factory/settings.json
/// Returns models with "Custom Model (BYOK)" description for tooltip display
pub fn get_factory_custom_models() -> Vec<ModelOption> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    let settings_path = std::path::PathBuf::from(home)
        .join(".factory")
        .join("settings.json");

    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let settings: FactorySettings = match serde_json::from_str(&content) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[Harness] Failed to parse ~/.factory/settings.json: {}", e);
            return Vec::new();
        }
    };

    settings
        .custom_models
        .into_iter()
        .map(|m| {
            // Use the custom id (e.g., "custom:CC:-Opus-4.5-(High)-0") as the value
            // This is what droid expects when selecting a custom model
            let value = m.id.unwrap_or_else(|| m.model.clone());
            let name = m.display_name.or_else(|| Some(m.model.clone()));
            ModelOption {
                value,
                name,
                description: Some("Custom Model (BYOK)".to_string()),
            }
        })
        .collect()
}
