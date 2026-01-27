//! Local usage statistics parsing for Claude Code sessions.
//!
//! Scans `~/.claude/projects/**/*.jsonl` files to extract token usage,
//! model breakdown, and activity metrics for the analytics dashboard.
//! Based on ccusage (https://github.com/ryoppippi/ccusage) approach.

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// Daily usage statistics for a single day.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsageDay {
    pub day: String,
    pub input_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub total_cost: f64,
}

/// Aggregate totals and computed statistics.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsageTotals {
    pub last7_days_tokens: i64,
    pub last30_days_tokens: i64,
    pub average_daily_tokens: i64,
    pub cache_hit_rate_percent: f64,
    pub peak_day: Option<String>,
    pub peak_day_tokens: i64,
    pub total_cost: f64,
}

/// Token usage breakdown by model.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsageModel {
    pub model: String,
    pub tokens: i64,
    pub share_percent: f64,
    pub cost: f64,
}

/// Complete snapshot of Claude Code local usage statistics.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsageSnapshot {
    pub updated_at: i64,
    pub days: Vec<ClaudeUsageDay>,
    pub totals: ClaudeUsageTotals,
    #[serde(default)]
    pub top_models: Vec<ClaudeUsageModel>,
}

#[derive(Default, Clone)]
struct DailyTotals {
    input: i64,
    cache_creation: i64,
    cache_read: i64,
    output: i64,
    cost: f64,
}

#[derive(Default, Clone)]
struct ModelTotals {
    tokens: i64,
    cost: f64,
}

/// Pricing data for Claude models (per million tokens)
/// Based on https://www.anthropic.com/pricing
/// Returns (input_per_mtok, output_per_mtok, cache_write_per_mtok, cache_read_per_mtok)
pub fn get_model_pricing(model: &str) -> (f64, f64, f64, f64) {
    // Returns (input_per_mtok, output_per_mtok, cache_write_per_mtok, cache_read_per_mtok)
    let model_lower = model.to_lowercase();

    if model_lower.contains("opus") {
        // Claude Opus: $15/M input, $75/M output, cache write 1.25x, cache read 0.1x
        (15.0, 75.0, 18.75, 1.5)
    } else if model_lower.contains("sonnet") {
        // Claude Sonnet: $3/M input, $15/M output
        (3.0, 15.0, 3.75, 0.3)
    } else if model_lower.contains("haiku") {
        // Claude Haiku: $0.25/M input, $1.25/M output
        (0.25, 1.25, 0.3125, 0.025)
    } else {
        // Default to Sonnet pricing
        (3.0, 15.0, 3.75, 0.3)
    }
}

/// Calculate cost from token usage for a Claude model
pub fn calculate_cost(
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
) -> f64 {
    let (input_price, output_price, cache_write_price, cache_read_price) = get_model_pricing(model);

    let input_cost = (input_tokens as f64 / 1_000_000.0) * input_price;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * output_price;
    let cache_write_cost = (cache_creation_tokens as f64 / 1_000_000.0) * cache_write_price;
    let cache_read_cost = (cache_read_tokens as f64 / 1_000_000.0) * cache_read_price;

    input_cost + output_cost + cache_write_cost + cache_read_cost
}

/// Tauri command to fetch Claude Code local usage statistics.
#[tauri::command]
pub async fn claude_local_usage_snapshot(days: Option<u32>) -> Result<ClaudeUsageSnapshot, String> {
    let days = days.unwrap_or(30).clamp(1, 90);
    let snapshot = tokio::task::spawn_blocking(move || scan_claude_local_usage(days))
        .await
        .map_err(|err| err.to_string())??;
    Ok(snapshot)
}

fn scan_claude_local_usage(days: u32) -> Result<ClaudeUsageSnapshot, String> {
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let day_keys = make_day_keys(days);
    let mut daily: HashMap<String, DailyTotals> = day_keys
        .iter()
        .map(|key| (key.clone(), DailyTotals::default()))
        .collect();
    let mut model_totals: HashMap<String, ModelTotals> = HashMap::new();

    let claude_paths = resolve_claude_paths();
    if claude_paths.is_empty() {
        return Ok(build_snapshot(updated_at, day_keys, daily, HashMap::new()));
    }

    // Track processed message+request ID pairs for deduplication
    let mut processed_hashes: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Calculate date range for filtering
    let oldest_day = day_keys.first().cloned().unwrap_or_default();

    for claude_path in claude_paths {
        let projects_dir = claude_path.join("projects");
        if !projects_dir.exists() {
            continue;
        }

        // Walk through all JSONL files in projects directory
        for entry in WalkDir::new(&projects_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }

            scan_claude_file(
                path,
                &mut daily,
                &mut model_totals,
                &mut processed_hashes,
                &oldest_day,
            )?;
        }
    }

    Ok(build_snapshot(updated_at, day_keys, daily, model_totals))
}

fn build_snapshot(
    updated_at: i64,
    day_keys: Vec<String>,
    daily: HashMap<String, DailyTotals>,
    model_totals: HashMap<String, ModelTotals>,
) -> ClaudeUsageSnapshot {
    let mut days: Vec<ClaudeUsageDay> = Vec::with_capacity(day_keys.len());
    let mut total_tokens: i64 = 0;
    let mut total_cost: f64 = 0.0;

    for day_key in &day_keys {
        let totals = daily.get(day_key).cloned().unwrap_or_default();
        let total = totals.input + totals.output + totals.cache_creation + totals.cache_read;
        total_tokens += total;
        total_cost += totals.cost;
        days.push(ClaudeUsageDay {
            day: day_key.clone(),
            input_tokens: totals.input,
            cache_creation_tokens: totals.cache_creation,
            cache_read_tokens: totals.cache_read,
            output_tokens: totals.output,
            total_tokens: total,
            total_cost: totals.cost,
        });
    }

    let last7 = days.iter().rev().take(7).cloned().collect::<Vec<_>>();
    let last7_tokens: i64 = last7.iter().map(|day| day.total_tokens).sum();
    let last7_input: i64 = last7.iter().map(|day| day.input_tokens).sum();
    let last7_cache_read: i64 = last7.iter().map(|day| day.cache_read_tokens).sum();

    let average_daily_tokens = if last7.is_empty() {
        0
    } else {
        ((last7_tokens as f64) / (last7.len() as f64)).round() as i64
    };

    // Cache hit rate = cache_read / (input + cache_read)
    // Because input_tokens is AFTER cache hits are subtracted
    let total_input_attempted = last7_input + last7_cache_read;
    let cache_hit_rate_percent = if total_input_attempted > 0 {
        ((last7_cache_read as f64) / (total_input_attempted as f64) * 1000.0).round() / 10.0
    } else {
        0.0
    };

    let peak = days
        .iter()
        .max_by_key(|day| day.total_tokens)
        .filter(|day| day.total_tokens > 0);
    let peak_day = peak.map(|day| day.day.clone());
    let peak_day_tokens = peak.map(|day| day.total_tokens).unwrap_or(0);

    let mut top_models: Vec<ClaudeUsageModel> = model_totals
        .into_iter()
        .filter(|(model, totals)| model != "unknown" && totals.tokens > 0)
        .map(|(model, totals)| ClaudeUsageModel {
            model,
            tokens: totals.tokens,
            share_percent: if total_tokens > 0 {
                ((totals.tokens as f64) / (total_tokens as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            },
            cost: totals.cost,
        })
        .collect();
    top_models.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    top_models.truncate(4);

    ClaudeUsageSnapshot {
        updated_at,
        days,
        totals: ClaudeUsageTotals {
            last7_days_tokens: last7_tokens,
            last30_days_tokens: total_tokens,
            average_daily_tokens,
            cache_hit_rate_percent,
            peak_day,
            peak_day_tokens,
            total_cost,
        },
        top_models,
    }
}

fn scan_claude_file(
    path: &Path,
    daily: &mut HashMap<String, DailyTotals>,
    model_totals: &mut HashMap<String, ModelTotals>,
    processed_hashes: &mut std::collections::HashSet<String>,
    oldest_day: &str,
) -> Result<(), String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Ok(()),
    };
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };

        // Skip very large lines to prevent memory issues
        if line.len() > 512_000 {
            continue;
        }

        let value = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        // Only process assistant messages with usage data
        let msg_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if msg_type != "assistant" {
            continue;
        }

        // Extract timestamp
        let timestamp = match value.get("timestamp").and_then(|v| v.as_str()) {
            Some(ts) => ts,
            None => continue,
        };

        // Parse timestamp and get day key
        let day_key = match parse_timestamp_to_day(timestamp) {
            Some(key) => key,
            None => continue,
        };

        // Skip if before our date range
        if day_key.as_str() < oldest_day {
            continue;
        }

        // Check if we have this day in our map
        if !daily.contains_key(&day_key) {
            continue;
        }

        // Extract message data
        let message = match value.get("message").and_then(|v| v.as_object()) {
            Some(msg) => msg,
            None => continue,
        };

        // Extract usage data
        let usage = match message.get("usage").and_then(|v| v.as_object()) {
            Some(usage) => usage,
            None => continue,
        };

        // Create unique hash for deduplication
        let message_id = message.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let request_id = value
            .get("requestId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !message_id.is_empty() && !request_id.is_empty() {
            let hash = format!("{}:{}", message_id, request_id);
            if processed_hashes.contains(&hash) {
                continue;
            }
            processed_hashes.insert(hash);
        }

        // Extract token counts
        let input_tokens = usage
            .get("input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let output_tokens = usage
            .get("output_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_creation_tokens = usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cache_read_tokens = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        // Skip if no tokens
        if input_tokens == 0 && output_tokens == 0 {
            continue;
        }

        // Extract model
        let model = message
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Calculate cost
        let cost = calculate_cost(
            &model,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        );

        // Update daily totals
        if let Some(entry) = daily.get_mut(&day_key) {
            entry.input += input_tokens;
            entry.output += output_tokens;
            entry.cache_creation += cache_creation_tokens;
            entry.cache_read += cache_read_tokens;
            entry.cost += cost;
        }

        // Update model totals
        if model != "unknown" {
            let model_entry = model_totals
                .entry(model)
                .or_insert_with(ModelTotals::default);
            model_entry.tokens +=
                input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens;
            model_entry.cost += cost;
        }
    }

    Ok(())
}

fn parse_timestamp_to_day(timestamp: &str) -> Option<String> {
    // Parse ISO 8601 timestamp like "2026-01-02T22:20:42.398Z"
    let dt = DateTime::parse_from_rfc3339(timestamp).ok()?;
    let local = dt.with_timezone(&Local);
    Some(local.format("%Y-%m-%d").to_string())
}

fn make_day_keys(days: u32) -> Vec<String> {
    let today = Local::now().date_naive();
    (0..days)
        .rev()
        .map(|offset| {
            let day = today - Duration::days(offset as i64);
            day.format("%Y-%m-%d").to_string()
        })
        .collect()
}

fn resolve_claude_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Check environment variable first (like ccusage does)
    if let Ok(env_paths) = std::env::var("CLAUDE_CONFIG_DIR") {
        for env_path in env_paths
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
        {
            let path = PathBuf::from(env_path);
            if path.join("projects").exists() {
                paths.push(path);
            }
        }
        if !paths.is_empty() {
            return paths;
        }
    }

    // Check default paths
    if let Some(home) = dirs::home_dir() {
        // Check ~/.claude (primary on macOS)
        let claude_path = home.join(".claude");
        if claude_path.join("projects").exists() {
            paths.push(claude_path);
        }

        // Check ~/.config/claude (XDG standard)
        let config_claude_path = home.join(".config").join("claude");
        if config_claude_path.join("projects").exists() {
            paths.push(config_claude_path);
        }
    }

    paths
}
