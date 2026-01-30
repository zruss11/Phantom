# Plan: Dedicated Summaries Agent Setting

## Overview

Add a setting in the AI Summaries tab that allows users to select a dedicated agent for ALL AI-powered name generation, regardless of which agent is running the actual task. This affects:

1. **Task title summaries** - 15-30 char titles generated on task creation
2. **Status summaries** - 40 char status summaries on task completion
3. **Worktree branch naming** - LLM-generated branch names (e.g., `fix/login-bug`)

## Current State

- **Location**: AI Summaries settings in `gui/menu.html:1194-1215`
- **Summarize module**: `src-tauri/src/summarize.rs` - routes summaries to the task's agent
- **Namegen module**: `src-tauri/src/namegen.rs` - generates branch names using task's agent
- **Settings struct**: `src-tauri/src/main.rs:436-437` - only has `ai_summaries_enabled` boolean
- **Agent availability**: `build_agent_availability()` in main.rs checks which agent CLIs are installed

## Current Routing Logic

### Title/Status Summaries (summarize.rs:51-56)
```rust
match agent_id {
    "codex" => call_codex_api(&full_prompt).await,
    "amp" => call_amp_cli(&full_prompt).await,
    _ => call_claude_api(&full_prompt).await,
}
```

### Worktree Branch Naming (namegen.rs:68-75)
```rust
match agent_id {
    "codex" => generate_with_codex_backend(&full_prompt).await?,
    "claude-code" => generate_with_claude_oauth(&full_prompt).await?,
    _ => return Ok(generate_fallback(&truncated_prompt)),
}
```

All three use the task's `agent_id` passed from `main.rs`.

## Proposed Changes

### 1. Settings Struct (main.rs)

Add new field to `Settings` struct (~line 437):

```rust
#[serde(rename = "aiSummariesEnabled")]
ai_summaries_enabled: Option<bool>,
#[serde(rename = "summariesAgent")]
summaries_agent: Option<String>,  // NEW: "auto", "amp", "codex", "claude-code"
```

### 2. Frontend UI (menu.html)

Add a dropdown selector below the AI Summaries toggle (~line 1215):

```html
<div class="form-group mb-3" id="summariesAgentGroup">
  <label class="settings-label">Summaries Agent</label>
  <select class="form-select" id="summariesAgent">
    <option value="auto">Auto (use task's agent)</option>
    <option value="amp">Amp (free)</option>
    <option value="codex">Codex (GPT-5.1-codex-mini)</option>
    <option value="claude-code">Claude (Haiku)</option>
  </select>
  <small class="text-muted d-block mt-1">
    Select which agent generates task summaries. Amp is free and recommended if installed.
  </small>
</div>
```

### 3. Frontend JavaScript (application.js)

**A. Add to `saveSettingsFromUi()` (~line 341):**
```javascript
summariesAgent: $("#summariesAgent").val(),
```

**B. Add to settings load (~line 1857):**
```javascript
if (settingsPayload.summariesAgent) {
  $("#summariesAgent").val(settingsPayload.summariesAgent);
} else {
  // Default to amp if installed, otherwise auto
  const ampAvailable = window.agentAvailability?.amp?.available;
  $("#summariesAgent").val(ampAvailable ? "amp" : "auto");
}
```

**C. Add change handler (~line 361):**
```javascript
$("#summariesAgent").on("change", saveSettingsFromUi);
```

**D. Update visibility based on summaries toggle:**
```javascript
// Show/hide summaries agent dropdown based on toggle
function updateSummariesAgentVisibility() {
  const enabled = $("#aiSummariesEnabled").is(":checked");
  $("#summariesAgentGroup").toggle(enabled);
}
$("#aiSummariesEnabled").on("change", updateSummariesAgentVisibility);
```

### 4. Summarize Module (summarize.rs)

**A. Add helper function to resolve the summaries agent:**

```rust
/// Resolve which agent to use for summarization.
/// Returns the configured agent, or falls back to the task's agent if "auto" or None.
fn resolve_summaries_agent<'a>(task_agent: &'a str, configured: Option<&'a str>) -> &'a str {
    match configured {
        Some("auto") | None => task_agent,
        Some(agent) => agent,
    }
}
```

**B. Add `_with_override` variants of the public functions:**

```rust
/// Generate title using the configured summaries agent (or fallback to task agent)
pub async fn summarize_title_with_override(
    prompt: &str,
    task_agent_id: &str,
    summaries_agent: Option<&str>,
) -> String {
    let agent_id = resolve_summaries_agent(task_agent_id, summaries_agent);
    summarize_title(prompt, agent_id).await
}

/// Generate status using the configured summaries agent (or fallback to task agent)
pub async fn summarize_status_with_override(
    response: &str,
    task_agent_id: &str,
    summaries_agent: Option<&str>,
) -> String {
    let agent_id = resolve_summaries_agent(task_agent_id, summaries_agent);
    summarize_status(response, agent_id).await
}
```

### 5. Namegen Module (namegen.rs)

**A. Add the same helper function:**

```rust
fn resolve_summaries_agent<'a>(task_agent: &'a str, configured: Option<&'a str>) -> &'a str {
    match configured {
        Some("auto") | None => task_agent,
        Some(agent) => agent,
    }
}
```

**B. Add `_with_override` variant:**

```rust
/// Generate run metadata using the configured summaries agent (or fallback to task agent)
pub async fn generate_run_metadata_with_override(
    prompt: &str,
    task_agent_id: &str,
    summaries_agent: Option<&str>,
    api_key: Option<&str>,
) -> Result<RunMetadata, String> {
    let agent_id = resolve_summaries_agent(task_agent_id, summaries_agent);
    generate_run_metadata(prompt, agent_id, api_key).await
}

/// Timeout wrapper for the override version
pub async fn generate_run_metadata_with_timeout_and_override(
    prompt: &str,
    task_agent_id: &str,
    summaries_agent: Option<&str>,
    api_key: Option<&str>,
    timeout_secs: u64,
) -> RunMetadata {
    let agent_id = resolve_summaries_agent(task_agent_id, summaries_agent);
    generate_run_metadata_with_timeout(prompt, agent_id, api_key, timeout_secs).await
}
```

**C. Add Amp support to namegen routing (currently missing!):**

```rust
// namegen.rs:68-75 - UPDATE to include amp
match agent_id {
    "codex" => generate_with_codex_backend(&full_prompt).await?,
    "amp" => generate_with_amp_cli(&full_prompt).await?,  // NEW
    "claude-code" => generate_with_claude_oauth(&full_prompt).await?,
    _ => generate_with_claude_oauth(&full_prompt).await?,  // Default to Claude
}
```

**D. Add `generate_with_amp_cli` function (copy pattern from summarize.rs):**

```rust
/// Generate using Amp CLI (same as summarize.rs)
async fn generate_with_amp_cli(prompt: &str) -> Result<String, String> {
    // Same implementation as call_amp_cli in summarize.rs
    // Parse NDJSON output, look for assistant/result events
}
```

### 6. Main.rs Integration

**A. Update title generation call (~line 3046):**

```rust
// Get summaries agent setting
let summaries_agent = settings.summaries_agent.clone();

tauri::async_runtime::spawn(async move {
    let title = summarize::summarize_title_with_override(
        &prompt_clone,
        &agent_clone,
        summaries_agent.as_deref(),
    ).await;
    // ... rest unchanged
});
```

**B. Update status summary call (~line 7175):**

```rust
async fn summarize_status_for_notifications(
    state: &AppState,
    task_agent_id: &str,
    full_text: &str,
    fallback: &str,
) -> String {
    let settings = state.settings.lock().await.clone();
    if !settings.ai_summaries_enabled.unwrap_or(true) {
        return fallback.to_string();
    }

    let summary = summarize::summarize_status_with_override(
        full_text,
        task_agent_id,
        settings.summaries_agent.as_deref(),
    ).await;
    // ... rest unchanged
}
```

**C. Update worktree branch naming call (~line 3085):**

```rust
// Get summaries agent setting for branch naming
let summaries_agent = settings.summaries_agent.clone();

tauri::async_runtime::spawn(async move {
    // Generate proper branch name via LLM (using configured summaries agent)
    let metadata = namegen::generate_run_metadata_with_timeout_and_override(
        &prompt_clone,
        &agent_clone,
        summaries_agent.as_deref(),
        api_key.as_deref(),
        5,
    )
    .await;
    // ... rest unchanged
});
```

### 7. Default to Amp if Installed

In the frontend, when loading settings:

```javascript
// After loading agent availability
ipcRenderer.invoke("getAgentAvailability").then((avail) => {
  window.agentAvailability = avail;

  // If no summaries agent set and amp is available, default to amp
  if (!currentSettings.summariesAgent && avail.amp?.available) {
    $("#summariesAgent").val("amp");
    saveSettingsFromUi();
  }
});
```

## File Changes Summary

| File | Changes |
|------|---------|
| `src-tauri/src/main.rs` | Add `summaries_agent` to Settings struct, update 3 call sites |
| `src-tauri/src/summarize.rs` | Add `_with_override` variants, add `resolve_summaries_agent` helper |
| `src-tauri/src/namegen.rs` | Add `_with_override` variants, add Amp support, add `resolve_summaries_agent` |
| `gui/menu.html` | Add summaries agent dropdown UI |
| `gui/js/application.js` | Add settings save/load for dropdown, add auto-default logic |

## Testing Plan

1. **No agent selected (auto)**: Verify all three (titles, status, branch names) use the task's agent
2. **Amp selected**: Verify Claude Code task uses Amp for titles, status, AND branch names
3. **Codex selected**: Verify Amp task uses Codex for all summaries
4. **Claude selected**: Verify Codex task uses Claude for all summaries
5. **Amp not installed**: Verify dropdown shows but amp option is visually indicated as unavailable
6. **Toggle off**: Verify dropdown hides when AI Summaries disabled
7. **Default behavior**: Verify amp is auto-selected on first run if installed
8. **Worktree branch naming**: Verify branch names are generated by the configured agent, not the task agent

## Edge Cases

1. **Selected agent not installed**: Show error in notification, fall back to task agent
2. **Auth expired for selected agent**: Show error, fall back to task agent
3. **Migration**: Existing users with no setting get "auto" behavior (backward compatible)
4. **Namegen fallback**: If amp fails for branch naming, use simple fallback (first 5 words)

## Considerations

- **Why default to Amp?**: Amp is free (no API costs), making it ideal for summary generation
- **Why keep "auto"?**: Some users may prefer summaries from the same agent for consistency
- **Auth handling**: Codex and Claude require OAuth tokens; Amp uses CLI (no auth needed)
- **Code deduplication**: Consider extracting `call_amp_cli` to a shared utility module since both summarize.rs and namegen.rs need it
