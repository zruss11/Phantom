const bridge = window.tauriBridge;
const ipcRenderer = bridge.ipcRenderer;
const remote = bridge.remote;
const webFrame = bridge.webFrame;
const shell = bridge.shell;

// Permission mode options with descriptions for custom dropdown
const PERMISSION_MODE_OPTIONS = [
  { value: 'default', name: 'Default', description: 'Standard behavior, prompts for dangerous operations' },
  { value: 'acceptEdits', name: 'Accept Edits', description: 'Auto-accept file edit operations' },
  { value: 'plan', name: 'Plan Mode', description: 'Planning mode, no actual tool execution' },
  { value: 'dontAsk', name: "Don't Ask", description: "Don't prompt for permissions, deny if not pre-approved" },
  { value: 'bypassPermissions', name: 'Bypass Permissions', description: 'Bypass all permission checks (dangerous)' }
];

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text || "";
  return div.innerHTML;
}

// Custom dropdown instances (initialized in initCustomDropdowns)
let permissionDropdown = null;
let execModelDropdown = null;
let reasoningEffortDropdown = null;
let agentModeDropdown = null;
let baseBranchDropdown = null;

// Flag to prevent saving during restoration (avoids overwriting saved preferences)
let isRestoringSettings = false;
let pendingBaseBranchValue = null;

// Cache for enriched models with reasoning effort data (for Codex)
let enrichedModelCache = {};

function syncToggleButtonsFromCheckbox(checkbox) {
  if (!checkbox) return;
  const container = checkbox.closest('.toggle-buttons');
  if (!container) return;
  const buttons = container.querySelectorAll('.toggle-button');
  const isChecked = checkbox.checked;
  container.classList.toggle('is-on', isChecked);
  buttons.forEach(btn => {
    const btnValue = btn.dataset.value === 'true';
    btn.classList.toggle('is-active', btnValue === isChecked);
  });
}

function setToggleState(toggleId, enabled) {
  const checkbox = document.getElementById(toggleId);
  if (!checkbox) return;
  checkbox.checked = enabled;
  syncToggleButtonsFromCheckbox(checkbox);
  checkbox.dispatchEvent(new Event('change', { bubbles: true }));
}

// Initialize custom dropdowns
function initCustomDropdowns() {
  const permContainer = document.getElementById('permissionModeDropdown');
  const execContainer = document.getElementById('execModelDropdown');
  const reasoningContainer = document.getElementById('reasoningEffortDropdown');
  const modeContainer = document.getElementById('agentModeDropdown');
  const baseBranchContainer = document.getElementById('baseBranchDropdown');

  if (permContainer && window.CustomDropdown) {
    permissionDropdown = new window.CustomDropdown({
      container: permContainer,
      items: PERMISSION_MODE_OPTIONS,
      placeholder: 'Permission Mode',
      defaultValue: 'default',
      onChange: function(value) {
        console.log('[Harness] Permission mode changed:', value);
        saveTaskSettings();
      }
    });
    window.permissionDropdown = permissionDropdown;
  }

  if (execContainer && window.CustomDropdown) {
    execModelDropdown = new window.CustomDropdown({
      container: execContainer,
      items: [{ value: 'default', name: 'Use agent default', description: '' }],
      placeholder: 'Model',
      defaultValue: 'default',
      onChange: function(value) {
        console.log('[Harness] Exec model changed:', value);
        updateReasoningEffortDropdown(value);
        saveTaskSettings();
      }
    });
    window.execModelDropdown = execModelDropdown;
  }

  if (reasoningContainer && window.CustomDropdown) {
    reasoningEffortDropdown = new window.CustomDropdown({
      container: reasoningContainer,
      items: [{ value: 'default', name: 'Default', description: '' }],
      placeholder: 'Thinking Level',
      defaultValue: 'default',
      onChange: function(value) {
        console.log('[Harness] Reasoning effort changed:', value);
        // Don't save during restoration to avoid overwriting saved preferences
        if (!isRestoringSettings) {
          saveTaskSettings();
        }
      }
    });
    window.reasoningEffortDropdown = reasoningEffortDropdown;
  }

  if (modeContainer && window.CustomDropdown) {
    agentModeDropdown = new window.CustomDropdown({
      container: modeContainer,
      items: [{ value: 'default', name: 'Use default', description: '' }],
      placeholder: 'Agent Mode',
      defaultValue: 'default',
      onChange: function(value) {
        console.log('[Harness] Agent mode changed:', value);
        // Don't save during restoration to avoid overwriting saved preferences
        if (!isRestoringSettings) {
          saveTaskSettings();
        }
      }
    });
    window.agentModeDropdown = agentModeDropdown;
  }

  if (baseBranchContainer && window.CustomDropdown) {
    baseBranchDropdown = new window.CustomDropdown({
      container: baseBranchContainer,
      items: [{ value: 'default', name: 'Select base branch', description: '' }],
      placeholder: 'Base Branch',
      defaultValue: 'default',
      searchable: true,
      searchPlaceholder: 'Search branches...',
      onChange: function(value) {
        console.log('[Harness] Base branch changed:', value);
        if (!isRestoringSettings) {
          saveTaskSettings();
        }
      }
    });
    window.baseBranchDropdown = baseBranchDropdown;
  }

  console.log('[Harness] Custom dropdowns initialized');
}

// Project path helpers - display folder name, store full path
function setProjectPath(fullPath) {
  const $input = $("#projectPath");
  if (!fullPath) {
    $input.val("").attr("data-full-path", "").attr("title", "");
    return;
  }
  const folderName = fullPath.split("/").pop() || fullPath;
  $input
    .val(folderName)
    .attr("data-full-path", fullPath)
    .attr("title", fullPath);
}

function getProjectPath() {
  return (
    $("#projectPath").attr("data-full-path") || $("#projectPath").val() || null
  );
}

async function refreshBaseBranchOptions() {
  if (!baseBranchDropdown) return;
  const projectPath = getProjectPath();

  baseBranchDropdown.setOptions([
    { value: 'default', name: 'Loading branches...', description: '' }
  ]);

  try {
    const result = await ipcRenderer.invoke("getRepoBranches", projectPath);
    const branches = Array.isArray(result?.branches) ? result.branches : [];
    if (!branches.length) {
      baseBranchDropdown.setOptions([
        {
          value: 'default',
          name: 'No branches found',
          description: result?.error || 'Select a git project to load branches'
        }
      ]);
      baseBranchDropdown.setValue('default');
      return;
    }

    const items = branches.map((branch) => {
      let description = '';
      if (result?.defaultBranch && branch === result.defaultBranch) {
        description = 'Default branch';
      } else if (result?.currentBranch && branch === result.currentBranch) {
        description = 'Current branch';
      }
      return { value: branch, name: branch, description };
    });
    baseBranchDropdown.setOptions(items);

    const preferred =
      (pendingBaseBranchValue && branches.includes(pendingBaseBranchValue) && pendingBaseBranchValue) ||
      (currentSettings?.taskBaseBranch && branches.includes(currentSettings.taskBaseBranch) && currentSettings.taskBaseBranch) ||
      (result?.defaultBranch && branches.includes(result.defaultBranch) && result.defaultBranch) ||
      (result?.currentBranch && branches.includes(result.currentBranch) && result.currentBranch) ||
      branches[0];

    baseBranchDropdown.setValue(preferred || 'default');
  } catch (err) {
    console.warn('[Harness] Failed to load base branches:', err);
    baseBranchDropdown.setOptions([
      { value: 'default', name: 'Branches unavailable', description: 'Check gh auth or repo' }
    ]);
    baseBranchDropdown.setValue('default');
  } finally {
    pendingBaseBranchValue = null;
  }
}
// webFrame.setVisualZoomLevelLimits(1, 1)
// webFrame.setLayoutZoomLevelLimits(0, 0);
let currentSettings = {};
let recentProjectPaths = [];

function collectAuthInputs() {
  const auth = {};
  document.querySelectorAll("[data-auth-key]").forEach((input) => {
    const key = input.dataset.authKey;
    if (!key) return;
    const value = input.value.trim();
    if (value) {
      auth[key] = value;
    }
  });
  return auth;
}

function getProjectAllowlist() {
  return Array.isArray(currentSettings.taskProjectAllowlist)
    ? currentSettings.taskProjectAllowlist
    : [];
}

function addRecentProjectPath(path) {
  const trimmed = (path || "").trim();
  if (!trimmed) return;
  recentProjectPaths = recentProjectPaths.filter((entry) => entry !== trimmed);
  recentProjectPaths.unshift(trimmed);
  recentProjectPaths = recentProjectPaths.slice(0, 12);
}

function projectPathLabel(path) {
  return path.split(/[\\/]/).pop() || path;
}

function renderProjectAllowlist() {
  const container = $("#taskProjectAllowlistList");
  if (!container.length) return;

  const allowlist = getProjectAllowlist()
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
  const allowSet = new Set(allowlist);
  const entries = [];

  recentProjectPaths.forEach((path) => {
    if (!path) return;
    entries.push({ path: path, starred: allowSet.has(path) });
  });

  allowlist.forEach((path) => {
    if (!entries.find((entry) => entry.path === path)) {
      entries.push({ path: path, starred: true });
    }
  });

  container.empty();

  if (!entries.length) {
    container.append(
      '<div class="text-muted small project-allowlist-empty">No recent projects yet.</div>',
    );
    return;
  }

  entries.forEach((entry) => {
    const name = projectPathLabel(entry.path);
    const starClass = entry.starred ? "fas" : "fal";
    const item = $(
      '<div class="list-group-item project-allowlist-item d-flex align-items-center justify-content-between"></div>',
    );
    const label = $("<div></div>")
      .addClass("text-truncate")
      .text(name)
      .attr("title", entry.path);
    const button = $(
      '<button type="button" class="btn btn-sm btn-outline-secondary project-allowlist-star"></button>',
    );
    button.data("path", entry.path);
    button.append(`<i class="${starClass} fa-star"></i>`);
    item.append(label, button);
    container.append(item);
  });
}

function updateProjectAllowlist(nextAllowlist) {
  currentSettings.taskProjectAllowlist = nextAllowlist;
  saveSettingsFromUi();
  renderProjectAllowlist();
}

function getCreateTaskButtonLabel() {
  return $("#containerIsolationEnabled").is(":checked")
    ? "Create Contained Task"
    : "Create Task";
}

function updateCreateTaskButtonLabel() {
  const btn = document.getElementById("createAgentButton");
  if (!btn) return;
  if (btn.disabled && btn.textContent === "Creating...") {
    return;
  }
  btn.textContent = getCreateTaskButtonLabel();
}

window.updateCreateTaskButtonLabel = updateCreateTaskButtonLabel;

function setCreateTaskMessage(message, tone) {
  const el = document.getElementById("createTaskError");
  if (!el) return;
  el.classList.remove("text-danger", "text-warning");
  if (message && message.trim()) {
    if (tone === "warning") {
      el.classList.add("text-warning");
    } else {
      el.classList.add("text-danger");
    }
    el.textContent = message;
    el.style.display = "block";
  } else {
    el.textContent = "";
    el.style.display = "none";
  }
}

function setCreateTaskError(message) {
  setCreateTaskMessage(message, "error");
}

function setCreateTaskWarning(message) {
  setCreateTaskMessage(message, "warning");
}

function clearCreateTaskError() {
  setCreateTaskError("");
  const btn = document.getElementById("fixWithAgentButton");
  if (btn) {
    btn.style.display = "none";
    btn.disabled = false;
    btn.textContent = "Fix with Agent";
  }
  pendingFixTask = null;
}

let pendingFixTask = null;
let lastCreateAgentId = null;

function updateFixTaskButton(worktreePath, agentId) {
  pendingFixTask = {
    worktreePath: worktreePath || null,
    agentId: agentId || lastCreateAgentId || primaryAgentId || activeAgentId || null,
  };
  const btn = document.getElementById("fixWithAgentButton");
  if (btn) {
    if (pendingFixTask.worktreePath) {
      btn.style.display = "inline-block";
      btn.disabled = false;
    } else {
      btn.style.display = "none";
    }
  }
}

function showCreateTaskWarning(message, worktreePath, agentId) {
  setCreateTaskWarning(message);
  updateFixTaskButton(worktreePath, agentId);
}

function showCreateTaskConflict(message, worktreePath, agentId) {
  setCreateTaskError(message);
  updateFixTaskButton(worktreePath, agentId);
}

window.setCreateTaskError = setCreateTaskError;
window.clearCreateTaskError = clearCreateTaskError;
window.setCreateTaskWarning = setCreateTaskWarning;
window.showCreateTaskWarning = showCreateTaskWarning;
window.showCreateTaskConflict = showCreateTaskConflict;

function buildConflictFixPrompt(worktreePath) {
  return [
    "You are in a git worktree with conflicts from applying local changes.",
    "",
    `Worktree path: ${worktreePath}`,
    "",
    "Please:",
    "1) Resolve all merge conflicts.",
    "2) Run git status to confirm clean.",
    "3) Summarize what was changed.",
    "4) Do NOT start the original task; only fix the conflicts.",
    "",
    "If you need to inspect files, do so directly in the worktree.",
  ].join("\n");
}

function createFixTaskFromWarning() {
  if (!pendingFixTask || !pendingFixTask.worktreePath) return;
  const agentId =
    pendingFixTask.agentId || primaryAgentId || activeAgentId || "codex";
  const agentModels = (currentSettings && currentSettings.taskAgentModels) || {};
  const prefs = agentModels[agentId] || {};
  const execModel = prefs.execModel || "default";
  const reasoningEffort = agentId === "codex" ? (prefs.reasoningEffort || "default") : null;
  const agentMode = agentId === "opencode" ? (prefs.agentMode || "build") : null;
  const codexMode = agentId === "codex" ? (prefs.agentMode || "default") : null;

  const payload = {
    agentId: agentId,
    prompt: buildConflictFixPrompt(pendingFixTask.worktreePath),
    projectPath: pendingFixTask.worktreePath,
    baseBranch: null,
    planMode: false,
    thinking: true,
    useWorktree: false,
    permissionMode: "bypassPermissions",
    execModel: execModel,
    reasoningEffort: reasoningEffort !== "default" ? reasoningEffort : null,
    agentMode: agentMode,
    codexMode: codexMode !== "default" ? codexMode : null,
    attachments: [],
    multiCreate: false,
  };

  const btn = document.getElementById("fixWithAgentButton");
  if (btn) {
    btn.disabled = true;
    btn.textContent = "Creating...";
  }
  ipcRenderer.send("CreateAgentSession", payload);
}

ipcRenderer.on("CreateTaskError", (e, message) => {
  if (typeof window.setCreateTaskError === "function") {
    window.setCreateTaskError(message || "Failed to create task.");
  }
});

ipcRenderer.on("CreateTaskWarning", (e, message) => {
  if (typeof window.showCreateTaskWarning === "function") {
    window.showCreateTaskWarning(message || "Created task with warnings.", null, lastCreateAgentId);
  }
});

async function saveSettingsFromUi() {
  let discordBotToken = $("#discordBotToken").val();
  let discordChannelId = $("#discordChannelId").val();
  let retryDelay = $("#retryDelay").val();
  let errorDelay = $("#errorDelay").val();
  let taskProjectAllowlist = getProjectAllowlist();
  let pl = Object.assign(
    {},
    currentSettings,
    {
      discordEnabled: $("#discordEnabled").is(":checked"),
      discordBotToken: (discordBotToken || "").toString().trim(),
      discordChannelId: (discordChannelId || "").toString().trim(),
      retryDelay: retryDelay,
      errorDelay: errorDelay,
      ignoreDeclines: $("#ignoreDeclines").is(":checked"),
      agentNotificationsEnabled: $("#agentNotificationsEnabled").is(":checked"),
      agentNotificationStack: $("#agentNotificationStack").is(":checked"),
      aiSummariesEnabled: $("#aiSummariesEnabled").is(":checked"),
      containerIsolationEnabled: $("#containerIsolationEnabled").is(":checked"),
      taskProjectAllowlist: taskProjectAllowlist,
    },
    collectAuthInputs(),
  );
  currentSettings = pl;
  try {
    await ipcRenderer.invoke("saveSettings", pl);
    sendNotification("Settings saved", "green");
  } catch (err) {
    console.warn("[Harness] settings save failed", err);
    sendNotification("Settings save failed", "red");
  }
}

// Auto-save settings on any change (inputs and toggles)
$("#discordBotToken, #discordChannelId, #retryDelay, #errorDelay").on("change", saveSettingsFromUi);
$("#discordEnabled, #agentNotificationsEnabled, #agentNotificationStack, #aiSummariesEnabled, #containerIsolationEnabled").on("change", saveSettingsFromUi);
$("#containerIsolationEnabled").on("change", updateCreateTaskButtonLabel);

$(document).on("click", ".project-allowlist-star", function () {
  const path = $(this).data("path");
  if (!path) return;
  const allowlist = getProjectAllowlist();
  const nextAllowlist = allowlist.includes(path)
    ? allowlist.filter((entry) => entry !== path)
    : allowlist.concat(path);
  updateProjectAllowlist(nextAllowlist);
});

document.querySelectorAll("[data-auth-save]").forEach((button) => {
  button.addEventListener("click", (event) => {
    event.preventDefault();
    saveSettingsFromUi();
  });
});

// Simplified auth state management (per research insights)
let codexAuthState = { authenticated: false, method: null };
let claudeAuthState = { authenticated: false, method: null, email: null };

// Agent availability tracking
let agentAvailability = {};

// Listen for backend availability updates
ipcRenderer.on("AgentAvailabilityUpdate", function (e, agentId, available, errorMessage) {
  console.log("[Harness] AgentAvailabilityUpdate:", agentId, available, errorMessage);
  agentAvailability[agentId] = { available, errorMessage };
  updateAgentCardAvailability(agentId, available, errorMessage);
});

// Update an agent card's availability status in the UI
function updateAgentCardAvailability(agentId, available, errorMessage) {
  const card = document.querySelector(
    `.agent-card[data-agent-id="${agentId}"]`
  );
  if (!card) return;

  // Don't modify "Coming Soon" cards (like Factory Droid)
  if (card.classList.contains("coming-soon")) return;

  if (available) {
    // Agent is available - remove unavailable state
    card.classList.remove("unavailable");
    card.disabled = false;
    card.removeAttribute("data-unavailable-reason");
    // Remove unavailable badge if present
    const badge = card.querySelector(".unavailable-badge");
    if (badge) badge.remove();
  } else {
    // Agent is unavailable - show error state
    card.classList.add("unavailable");
    card.disabled = true;
    // Truncate error message for tooltip (max 150 chars)
    const truncatedError = errorMessage
      ? errorMessage.length > 150
        ? errorMessage.substring(0, 150) + "..."
        : errorMessage
      : "Initialization failed";
    card.setAttribute("data-unavailable-reason", truncatedError);

    // Add unavailable badge if not already present
    if (!card.querySelector(".unavailable-badge")) {
      const badge = document.createElement("span");
      badge.className = "unavailable-badge";
      badge.textContent = "Unavailable";
      card.appendChild(badge);
    }

    console.warn("[Harness] Agent unavailable:", agentId, errorMessage);
  }
}

// Fetch agent availability from backend on page load
async function fetchAgentAvailability() {
  try {
    const availability = await ipcRenderer.invoke("getAgentAvailability");
    if (availability) {
      agentAvailability = availability;
      for (const [agentId, status] of Object.entries(availability)) {
        updateAgentCardAvailability(
          agentId,
          status.available,
          status.error_message
        );
      }
    }
  } catch (err) {
    console.warn("[Harness] Failed to fetch agent availability:", err);
  }
}

// Helper to redact email for privacy
function redactEmail(email) {
  if (!email) return "";
  const [local, domain] = email.split("@");
  if (!domain) return "••••••••";
  const redactedLocal =
    local.length > 2
      ? local[0] +
        "•".repeat(Math.min(local.length - 2, 6)) +
        local[local.length - 1]
      : "••";
  return redactedLocal + "@" + domain;
}

// Toggle email visibility
function toggleEmailVisibility(provider) {
  document
    .querySelectorAll(`[data-auth-email="${provider}"]`)
    .forEach((emailNode) => {
      const toggleBtn = emailNode
        .closest(".auth-email-wrapper")
        ?.querySelector(`[data-toggle-email="${provider}"]`);
      const isRedacted = emailNode.classList.contains("redacted");

      if (isRedacted) {
        // Show actual email
        emailNode.textContent = emailNode.dataset.email || "";
        emailNode.classList.remove("redacted");
        if (toggleBtn) {
          toggleBtn.dataset.visible = "true";
          toggleBtn.title = "Hide email";
        }
      } else {
        // Hide email (show redacted)
        emailNode.textContent = "";
        emailNode.classList.add("redacted");
        if (toggleBtn) {
          toggleBtn.dataset.visible = "false";
          toggleBtn.title = "Show email";
        }
      }
    });
}

// Setup email toggle handlers
document.addEventListener("click", (e) => {
  const toggleBtn = e.target.closest("[data-toggle-email]");
  if (toggleBtn) {
    const provider = toggleBtn.dataset.toggleEmail;
    toggleEmailVisibility(provider);
  }
});

async function checkCodexAuth() {
  try {
    const status = await ipcRenderer.invoke("checkCodexAuth");
    codexAuthState = status || { authenticated: false, method: null };
    updateCodexAuthUI();
    return codexAuthState;
  } catch (err) {
    console.warn("[Harness] checkCodexAuth failed", err);
    return { authenticated: false, method: null };
  }
}

function updateCodexAuthUI() {
  const state = codexAuthState;

  // Update status text nodes
  document.querySelectorAll('[data-auth-status="codex"]').forEach((node) => {
    if (state.authenticated) {
      node.textContent =
        state.method === "chatgpt"
          ? "ChatGPT subscription linked"
          : "API key configured";
      node.classList.add("connected");
    } else {
      node.textContent = "Not connected";
      node.classList.remove("connected");
    }
  });

  // Update email display (with redaction)
  document
    .querySelectorAll('[data-auth-email-wrapper="codex"]')
    .forEach((wrapper) => {
      const emailNode = wrapper.querySelector('[data-auth-email="codex"]');
      if (state.authenticated && state.email) {
        emailNode.dataset.email = state.email;
        emailNode.dataset.redacted = redactEmail(state.email);
        emailNode.classList.add("redacted");
        wrapper.hidden = false;
      } else {
        emailNode.dataset.email = "";
        emailNode.dataset.redacted = "";
        emailNode.textContent = "";
        wrapper.hidden = true;
      }
    });

  // Update login buttons
  document
    .querySelectorAll('[data-auth-action="codex-login"]')
    .forEach((btn) => {
      btn.textContent = state.authenticated
        ? "Sign out"
        : "Sign in with ChatGPT";
      btn.dataset.authState = state.authenticated
        ? "connected"
        : "disconnected";
      btn.disabled = false;
      btn.classList.remove("auth-pending");
    });

  // Start/stop usage polling based on auth state
  console.log(
    "[Harness] Auth state update, authenticated:",
    state.authenticated,
  );
  if (state.authenticated) {
    console.log("[Harness] Starting usage polling...");
    startUsagePolling();
  } else {
    console.log("[Harness] Stopping usage polling...");
    stopUsagePolling();
    // Hide the usage tracker when not authenticated
    const tracker = document.querySelector('[data-usage-tracker="codex"]');
    if (tracker) tracker.hidden = true;
    updateAgentUsageWarning("codex", null, null);
  }
}

async function checkClaudeAuth() {
  try {
    const status = await ipcRenderer.invoke("checkClaudeAuth");
    claudeAuthState = status || {
      authenticated: false,
      method: null,
      email: null,
    };
    updateClaudeAuthUI();
    return claudeAuthState;
  } catch (err) {
    console.warn("[Harness] checkClaudeAuth failed", err);
    return { authenticated: false, method: null, email: null };
  }
}

function updateClaudeAuthUI() {
  const state = claudeAuthState;

  // Update status text nodes
  document.querySelectorAll('[data-auth-status="claude"]').forEach((node) => {
    if (state.authenticated) {
      node.textContent =
        state.method === "oauth"
          ? "Claude Code login linked"
          : "API key configured";
      node.classList.add("connected");
    } else {
      node.textContent = "Not connected";
      node.classList.remove("connected");
    }
  });

  // Update email display (with redaction)
  document
    .querySelectorAll('[data-auth-email-wrapper="claude"]')
    .forEach((wrapper) => {
      const emailNode = wrapper.querySelector('[data-auth-email="claude"]');
      if (state.authenticated && state.email) {
        emailNode.dataset.email = state.email;
        emailNode.dataset.redacted = redactEmail(state.email);
        emailNode.classList.add("redacted");
        wrapper.hidden = false;
      } else {
        emailNode.dataset.email = "";
        emailNode.dataset.redacted = "";
        emailNode.textContent = "";
        wrapper.hidden = true;
      }
    });

  // Update login buttons
  document
    .querySelectorAll('[data-auth-action="claude-login"]')
    .forEach((btn) => {
      btn.textContent = state.authenticated
        ? "Sign out"
        : "Sign in with Claude";
      btn.dataset.authState = state.authenticated
        ? "connected"
        : "disconnected";
      btn.disabled = false;
      btn.classList.remove("auth-pending");
    });

  if (!state.authenticated) {
    updateAgentUsageWarning("claude-code", null, null);
  }
}

async function initAuthState() {
  await checkCodexAuth();
  await checkClaudeAuth();
  // Try to enable Claude usage tracking on page load (non-blocking)
  tryEnableClaudeUsage();
  // Fetch agent availability status (for detecting broken agents)
  fetchAgentAvailability();
}

// Usage tracker state and polling
let usagePollingInterval = null;
const USAGE_POLL_INTERVAL = 300000; // 5 minutes
const USAGE_WARNING_THRESHOLD = 95;

async function fetchCodexUsage() {
  try {
    console.log("[Harness] Fetching codex usage...");
    const rateLimits = await ipcRenderer.invoke("codexRateLimits");
    console.log("[Harness] Got rate limits:", rateLimits);
    updateUsageDisplay(rateLimits);
  } catch (err) {
    console.warn("[Harness] Failed to fetch usage:", err);
    // Hide tracker on error
    const tracker = document.querySelector('[data-usage-tracker="codex"]');
    if (tracker) tracker.hidden = true;
  }
}

function formatResetTimeCompact(resetsAt, windowMins) {
  if (resetsAt) {
    const resetDate = new Date(resetsAt);
    const now = new Date();
    const diffMs = resetDate - now;
    if (diffMs > 0) {
      const hours = Math.floor(diffMs / (1000 * 60 * 60));
      const days = Math.floor(hours / 24);
      if (days > 0) return `${days}d ${hours % 24}h`;
      if (hours > 0) return `${hours}h`;
      const mins = Math.floor(diffMs / (1000 * 60));
      return `${mins}m`;
    }
  }
  // Fallback to window duration
  if (windowMins) {
    const hours = Math.floor(windowMins / 60);
    const days = Math.floor(hours / 24);
    if (days > 0) return `${days}d window`;
    if (hours > 0) return `${hours}h window`;
  }
  return "--";
}

function getUsageLevel(percent) {
  if (percent >= 80) return "high";
  if (percent >= 50) return "medium";
  return "low";
}

function updateAgentUsageWarning(agentId, sessionPercent, weeklyPercent) {
  const card = document.querySelector(`.agent-card[data-agent-id="${agentId}"]`);
  if (!card) return;
  if (card.classList.contains("unavailable") || card.classList.contains("coming-soon")) {
    card.classList.remove("usage-warning");
    card.removeAttribute("title");
    return;
  }

  const percents = [sessionPercent, weeklyPercent].filter(
    (value) => typeof value === "number" && !Number.isNaN(value),
  );
  const shouldWarn = percents.some((value) => value >= USAGE_WARNING_THRESHOLD);

  card.classList.toggle("usage-warning", shouldWarn);
  if (shouldWarn) {
    const parts = [];
    if (typeof sessionPercent === "number") parts.push(`Session ${sessionPercent}%`);
    if (typeof weeklyPercent === "number") parts.push(`Weekly ${weeklyPercent}%`);
    card.setAttribute("title", parts.join(" • "));
  } else {
    card.removeAttribute("title");
  }
}

function updateUsageDisplay(rateLimits) {
  const tracker = document.querySelector('[data-usage-tracker="codex"]');
  if (!tracker) return;

  // Handle null/undefined response
  if (!rateLimits) {
    console.warn("[Harness] rateLimits is null/undefined");
    tracker.hidden = true;
    updateAgentUsageWarning("codex", null, null);
    return;
  }

  // Handle "not available" response
  if (rateLimits.notAvailable) {
    console.log(
      "[Harness] Rate limits not available:",
      rateLimits.errorMessage,
    );
    tracker.hidden = true;
    updateAgentUsageWarning("codex", null, null);
    return;
  }

  // Update session meter
  let sessionPercent = null;
  if (rateLimits.primary) {
    sessionPercent = Math.round(rateLimits.primary.usedPercent || 0);
    const sessionMeter = document.querySelector('[data-usage-meter="session"]');
    const percentEl = document.querySelector('[data-usage-percent="session"]');
    const fillEl = document.querySelector('[data-usage-fill="session"]');
    const resetEl = document.querySelector('[data-usage-reset="session"]');

    if (percentEl) percentEl.textContent = `${sessionPercent}%`;
    if (fillEl) fillEl.style.width = `${sessionPercent}%`;
    if (resetEl)
      resetEl.textContent = formatResetTimeCompact(
        rateLimits.primary.resetsAt,
        rateLimits.primary.windowDurationMins,
      );
    if (sessionMeter)
      sessionMeter.dataset.usageLevel = getUsageLevel(sessionPercent);
  }

  // Update weekly meter
  let weeklyPercent = null;
  if (rateLimits.secondary) {
    weeklyPercent = Math.round(rateLimits.secondary.usedPercent || 0);
    const weeklyMeter = document.querySelector('[data-usage-meter="weekly"]');
    const percentEl = document.querySelector('[data-usage-percent="weekly"]');
    const fillEl = document.querySelector('[data-usage-fill="weekly"]');
    const resetEl = document.querySelector('[data-usage-reset="weekly"]');

    if (percentEl) percentEl.textContent = `${weeklyPercent}%`;
    if (fillEl) fillEl.style.width = `${weeklyPercent}%`;
    if (resetEl)
      resetEl.textContent = formatResetTimeCompact(
        rateLimits.secondary.resetsAt,
        rateLimits.secondary.windowDurationMins,
      );
    if (weeklyMeter)
      weeklyMeter.dataset.usageLevel = getUsageLevel(weeklyPercent);
  }

  updateAgentUsageWarning("codex", sessionPercent, weeklyPercent);
  tracker.hidden = false;
}

function startUsagePolling() {
  if (usagePollingInterval) return;
  fetchCodexUsage(); // Immediate fetch
  usagePollingInterval = setInterval(fetchCodexUsage, USAGE_POLL_INTERVAL);
}

function stopUsagePolling() {
  if (usagePollingInterval) {
    clearInterval(usagePollingInterval);
    usagePollingInterval = null;
  }
}

// Claude Usage Tracking
let claudeUsagePollingInterval = null;
let claudeUsageEnabled = false;

async function fetchClaudeUsage() {
  try {
    console.log("[Harness] Fetching Claude usage...");
    const rateLimits = await ipcRenderer.invoke("claudeRateLimits");
    console.log("[Harness] Got Claude rate limits:", rateLimits);
    updateClaudeUsageDisplay(rateLimits);
    return rateLimits;
  } catch (err) {
    console.warn("[Harness] Failed to fetch Claude usage:", err);
    const tracker = document.querySelector('[data-usage-tracker="claude"]');
    if (tracker) tracker.hidden = true;
    return null;
  }
}

function updateClaudeUsageDisplay(rateLimits) {
  const tracker = document.querySelector('[data-usage-tracker="claude"]');
  const keychainPrompt = document.querySelector('[data-usage-keychain="claude"]');
  
  if (!tracker) return;

  // Handle null/undefined response
  if (!rateLimits) {
    console.warn("[Harness] Claude rateLimits is null/undefined");
    tracker.hidden = true;
    if (keychainPrompt) keychainPrompt.hidden = false;
    updateAgentUsageWarning("claude-code", null, null);
    return;
  }

  // Handle "not available" response
  if (rateLimits.notAvailable) {
    console.log(
      "[Harness] Claude rate limits not available:",
      rateLimits.errorMessage,
    );
    tracker.hidden = true;
    if (keychainPrompt) {
      keychainPrompt.hidden = false;
      // Update hint with error message if available
      const hint = keychainPrompt.querySelector('.usage-keychain-hint');
      if (hint && rateLimits.errorMessage) {
        hint.textContent = rateLimits.errorMessage;
      }
    }
    updateAgentUsageWarning("claude-code", null, null);
    return;
  }

  // Hide keychain prompt, show tracker
  if (keychainPrompt) keychainPrompt.hidden = true;
  claudeUsageEnabled = true;

  // Update session meter
  let sessionPercent = null;
  if (rateLimits.primary) {
    sessionPercent = Math.round(rateLimits.primary.usedPercent || 0);
    const sessionMeter = document.querySelector('[data-usage-meter-claude="session"]');
    const percentEl = document.querySelector('[data-usage-percent-claude="session"]');
    const fillEl = document.querySelector('[data-usage-fill-claude="session"]');
    const resetEl = document.querySelector('[data-usage-reset-claude="session"]');

    if (percentEl) percentEl.textContent = `${sessionPercent}%`;
    if (fillEl) fillEl.style.width = `${sessionPercent}%`;
    if (resetEl)
      resetEl.textContent = formatResetTimeCompact(
        rateLimits.primary.resetsAt,
        rateLimits.primary.windowDurationMins,
      );
    if (sessionMeter)
      sessionMeter.dataset.usageLevel = getUsageLevel(sessionPercent);
  }

  // Update weekly meter
  let weeklyPercent = null;
  if (rateLimits.secondary) {
    weeklyPercent = Math.round(rateLimits.secondary.usedPercent || 0);
    const weeklyMeter = document.querySelector('[data-usage-meter-claude="weekly"]');
    const percentEl = document.querySelector('[data-usage-percent-claude="weekly"]');
    const fillEl = document.querySelector('[data-usage-fill-claude="weekly"]');
    const resetEl = document.querySelector('[data-usage-reset-claude="weekly"]');

    if (percentEl) percentEl.textContent = `${weeklyPercent}%`;
    if (fillEl) fillEl.style.width = `${weeklyPercent}%`;
    if (resetEl)
      resetEl.textContent = formatResetTimeCompact(
        rateLimits.secondary.resetsAt,
        rateLimits.secondary.windowDurationMins,
      );
    if (weeklyMeter)
      weeklyMeter.dataset.usageLevel = getUsageLevel(weeklyPercent);
  }

  updateAgentUsageWarning("claude-code", sessionPercent, weeklyPercent);
  tracker.hidden = false;
}

function startClaudeUsagePolling() {
  if (claudeUsagePollingInterval) return;
  fetchClaudeUsage(); // Immediate fetch
  claudeUsagePollingInterval = setInterval(fetchClaudeUsage, USAGE_POLL_INTERVAL);
}

function stopClaudeUsagePolling() {
  if (claudeUsagePollingInterval) {
    clearInterval(claudeUsagePollingInterval);
    claudeUsagePollingInterval = null;
  }
}

// Try to enable Claude usage tracking on auth
async function tryEnableClaudeUsage() {
  const result = await fetchClaudeUsage();
  if (result && !result.notAvailable) {
    startClaudeUsagePolling();
    return true;
  }
  return false;
}

document.querySelectorAll("[data-auth-action]").forEach((button) => {
  button.addEventListener("click", async (event) => {
    event.preventDefault();
    const action = button.dataset.authAction;
    try {
      if (action === "codex-login") {
        if (button.dataset.authState === "connected") {
          // Logout
          await ipcRenderer.invoke("codexLogout");
          codexAuthState = { authenticated: false, method: null };
          updateCodexAuthUI();
          sendNotification("Signed out of Codex", "green");
        } else {
          // Login - show progress state
          button.textContent = "Signing in...";
          button.disabled = true;
          button.classList.add("auth-pending");

          const status = await ipcRenderer.invoke("codexLogin");
          codexAuthState = status || { authenticated: false, method: null };
          updateCodexAuthUI();

          if (codexAuthState.authenticated) {
            sendNotification("Signed in to Codex", "green");
          } else {
            sendNotification("Login was not completed", "yellow");
          }
        }
      } else if (action === "claude-login") {
        if (button.dataset.authState === "connected") {
          // Logout
          await ipcRenderer.invoke("claudeLogout");
          claudeAuthState = { authenticated: false, method: null, email: null };
          updateClaudeAuthUI();
          sendNotification("Signed out of Claude", "green");
        } else {
          // Login - show progress state
          button.textContent = "Signing in...";
          button.disabled = true;
          button.classList.add("auth-pending");

          const status = await ipcRenderer.invoke("claudeLogin");
          claudeAuthState = status || {
            authenticated: false,
            method: null,
            email: null,
          };
          updateClaudeAuthUI();

          if (claudeAuthState.authenticated) {
            sendNotification("Signed in to Claude", "green");
            // Try to enable usage tracking after successful login
            tryEnableClaudeUsage();
          } else {
            sendNotification("Login was not completed", "yellow");
          }
        }
      } else if (action === "claude-keychain") {
        // Keychain permission request for Claude usage tracking
        button.textContent = "Checking...";
        button.disabled = true;
        
        const success = await tryEnableClaudeUsage();
        
        if (success) {
          sendNotification("Claude usage tracking enabled", "green");
        } else {
          button.innerHTML = '<i class="fal fa-key"></i> Enable Usage Tracking';
          button.disabled = false;
          sendNotification("Could not access Claude usage data", "yellow");
        }
      }
    } catch (err) {
      console.warn("[Harness] auth action failed", err);
      // Reset button state on error
      if (action === "codex-login") {
        button.classList.remove("auth-pending");
        button.disabled = false;
        button.textContent = "Sign in with ChatGPT";
      } else if (action === "claude-login") {
        button.classList.remove("auth-pending");
        button.disabled = false;
        button.textContent = "Sign in with Claude";
      } else if (action === "claude-keychain") {
        button.innerHTML = '<i class="fal fa-key"></i> Enable Usage Tracking';
        button.disabled = false;
      }
      sendNotification("Auth action failed: " + (err.message || err), "red");
    }
  });
});

function getSizes(s, e, sizeArray) {
  let sizes = [];
  let si;
  let ei;
  for (let i = 0; i < sizeArray.length; i++) {
    if (s === sizeArray[i]) {
      si = i;
    }
    if (e === sizeArray[i]) {
      ei = i;
    }
  }
  if (si > ei) {
    return null;
  } else {
    for (let i = si; i <= ei; i++) {
      sizes.push(sizeArray[i]);
    }
    return sizes;
  }
}

let tasksOnPage = [];
let taskDataMap = {}; // Store full task data for sorting
let startingTasks = {}; // Guard against rapid Start clicks per task
let pendingDeleteTaskId = null; // Task ID awaiting deletion confirmation
let displayIdCounter = 1; // Sequential display IDs
let currentSortState = { column: null, direction: "asc" };
let pendingStatusUpdates = {}; // Queue status updates for tasks not yet in DOM
const PENDING_STATUS_TTL_MS = 30000; // Clean up orphaned entries after 30 seconds

// Periodic cleanup of orphaned pending status entries (prevents memory leak)
setInterval(function () {
  const now = Date.now();
  for (const id in pendingStatusUpdates) {
    if (
      pendingStatusUpdates[id].timestamp &&
      now - pendingStatusUpdates[id].timestamp > PENDING_STATUS_TTL_MS
    ) {
      console.warn("[Harness] Dropping stale pending status for task:", id);
      delete pendingStatusUpdates[id];
    }
  }
}, 10000);

// Agent logo mapping
const AGENT_LOGOS = {
  codex:
    '<svg class="agent-icon" role="img" aria-label="Codex"><use href="images/chatgpt-sprites-core.svg#55180d"></use></svg>',
  "claude-code": '<img src="images/claude-color.png" alt="Claude Code">',

  // Provider-style agents (Tauri wiring)
  amp: '<img src="images/ampcode.png" alt="Amp">',
  droid: '<img src="images/factorydroid.png" alt="Droid">',
  opencode: '<img src="images/opencode.png" alt="OpenCode">',

  // Back-compat
  "factory-droid": '<img src="factoryy-ai.svg" alt="Factory">',
};

// Get animation class based on status state
function getAnimationClass(statusState) {
  switch (statusState) {
    case "running":
      return "running";
    case "completed":
      return "completed";
    case "error":
      return "error";
    default:
      return "idle";
  }
}

// Format cost for display
function formatCost(cost) {
  if (cost === undefined || cost === null || cost === 0) {
    return "-";
  }
  return "$" + cost.toFixed(4);
}

ipcRenderer.on("AddTask", (e, ID, Task) => {
  const displayId = displayIdCounter++;
  const agent = Task.agent || "codex";
  const model = Task.model || "default";
  const status = Task.Status || "Ready";
  const statusState = Task.statusState || "idle";
  const cost = Task.cost || 0;
  const worktreePath = Task.worktreePath || Task.worktree_path || null;
  const projectPath = Task.projectPath || Task.project_path || null;
  const branch = Task.branch || null; // Git branch name (may differ from folder after async rename)
  const totalTokens = Task.totalTokens || null;
  const contextWindow = Task.contextWindow || null;
  const agentLogo = AGENT_LOGOS[agent] || AGENT_LOGOS["codex"];
  const animationClass = getAnimationClass(statusState);

  if (projectPath) {
    addRecentProjectPath(projectPath);
    renderProjectAllowlist();
  }

  // Store task data for sorting
  taskDataMap[ID] = {
    id: ID,
    displayId: displayId,
    agent: agent,
    model: model,
    status: status,
    statusState: statusState,
    cost: cost,
    worktree: worktreePath || "",
    branch: branch,
    totalTokens: totalTokens,
    contextWindow: contextWindow,
  };

  // Calculate context ring state from persisted data
  let contextRingClass = "no-data";
  let contextTooltip = "No usage data";
  let contextFreePercent = 100;
  if (totalTokens && totalTokens > 0) {
    contextRingClass = "";
    if (contextWindow && contextWindow > 0) {
      contextFreePercent = Math.max(0, 100 - (totalTokens / contextWindow) * 100);
      contextTooltip = `${Math.round(contextFreePercent)}% free\n${formatContextTokens(totalTokens)} / ${formatContextTokens(contextWindow)}`;
    } else {
      contextTooltip = `${formatContextTokens(totalTokens)} tokens used`;
    }
  }

  const thinkingClass = status === "Thinking..." ? "status-thinking" : "";
  const completedClass = statusState === "completed" ? "status-completed" : "";
  // Prefer the actual git branch name over the folder name (animal slug)
  const worktreeLabel = branch
    ? branch
    : (worktreePath ? worktreePath.split(/[\\/]/).pop() || worktreePath : "-");
  const worktreeTitle = worktreePath ? escapeHtml(worktreePath) : "";
  let taskElement = `<tr id="task-${ID}" data-display-id="${displayId}" data-task-id="${ID}">
        <th scope="row">${displayId}</th>
        <td class="agent-cell">
          <span class="agent-logo ${animationClass}" id="task-${ID}-Logo" data-agent="${agent}">${agentLogo}</span>
        </td>
        <td class="model-cell" id="task-${ID}-Model">${model}</td>
        <td class="worktree-cell" id="task-${ID}-Worktree" title="${worktreeTitle}">${escapeHtml(worktreeLabel)}</td>
        <td class="status-cell ${thinkingClass} ${completedClass}" id="task-${ID}-Status" title="${status}">${status}</td>
        <td class="context-cell" id="task-${ID}-Context">
          <div class="context-ring ${contextRingClass}" style="--context-free: ${contextFreePercent}" data-tooltip="${contextTooltip}"></div>
        </td>
        <td class="cost-cell" id="task-${ID}-Cost">${formatCost(cost)}</td>
        <td class="actions-cell">
            <a class="play green-text" data-action="start" data-task-id="${ID}"><i class="far fa-play"></i></a>
            <a class="stop yellow-text" data-action="stop" data-task-id="${ID}"><i class="far fa-stop"></i></a>
            <a class="view-log" data-action="view-log" data-task-id="${ID}"><i class="far fa-terminal"></i></a>
            <a class="delete red-text" data-action="delete" data-task-id="${ID}"><i class="far fa-trash-alt"></i></a>
        </td>
      </tr>`;
  taskElement = $.parseHTML(taskElement, false);
  $("#tasks-table").append(taskElement);
  tasksOnPage.push(ID);

  // Apply any pending status updates that arrived before task was in DOM
  if (pendingStatusUpdates[ID]) {
    const pending = pendingStatusUpdates[ID];
    console.log("[Harness] Applying pending status update for task:", ID);
    setTimeout(function () {
      applyStatusUpdate(
        ID,
        pending.message,
        pending.color,
        pending.statusState,
      );
      delete pendingStatusUpdates[ID];
    }, 0);
  }
});

ipcRenderer.on("UpdateEmail", (e, ID, email) => {
  $(`task-${ID}-Email`).text(email);
});

// Apply status update to task row - extracted for reuse by pending queue
function applyStatusUpdate(id, message, color, statusState) {
  const statusEl = $(`#task-${id}-Status`);

  // Toggle thinking animation class based on status message
  // Triggers shimmer for: "Thinking...", "OpenCode is working...", etc.
  const isThinking = message === "Thinking..." ||
                     message.startsWith("OpenCode is working");
  statusEl.toggleClass("status-thinking", isThinking);

  // Toggle completed class based on status state (green text for finished tasks)
  const isCompleted = statusState === "completed";
  statusEl.toggleClass("status-completed", isCompleted);

  // Only apply color if NOT thinking (thinking uses gradient) and NOT completed (uses CSS class)
  if (!isThinking && !isCompleted) {
    statusEl.animate({ color: color }, 0);
  }

  statusEl.text(message);
  statusEl.attr("title", message);

  // Update task data map
  if (taskDataMap[id]) {
    taskDataMap[id].status = message;
    if (statusState) {
      taskDataMap[id].statusState = statusState;
    }
  }

  // Update animation class if statusState provided
  if (statusState) {
    const logoEl = $(`#task-${id}-Logo`);
    logoEl.removeClass("idle running completed error");
    logoEl.addClass(getAnimationClass(statusState));

    // Re-enable Start when the task transitions out of running
    if (statusState !== "running") {
      startingTasks[id] = false;
      const playEl = $(`#task-${id} a.play`);
      playEl.removeClass("disabled");
      playEl.css("pointer-events", "auto");
    }
  }
}

ipcRenderer.on(
  "StatusUpdate",
  (e, id, message, color = "white", statusState = null) => {
    const statusEl = $(`#task-${id}-Status`);

    // If task not in DOM yet, queue the update for later (with timestamp for TTL cleanup)
    if (statusEl.length === 0) {
      console.log("[Harness] Queued status for pending task:", id);
      pendingStatusUpdates[id] = {
        message,
        color,
        statusState,
        timestamp: Date.now(),
      };
      return;
    }

    applyStatusUpdate(id, message, color, statusState);
  },
);

// Cost update handler
ipcRenderer.on("CostUpdate", (e, id, cost) => {
  $(`#task-${id}-Cost`).text(formatCost(cost));

  // Update task data map
  if (taskDataMap[id]) {
    taskDataMap[id].cost = cost;
  }
});

// Branch update handler (deferred branch naming after worktree creation)
// This updates the worktree column to show the LLM-generated branch name
// instead of the initial animal folder name
ipcRenderer.on("BranchUpdate", (e, id, branchName) => {
  console.log("[Harness] BranchUpdate:", id, branchName);

  // Update task data map with the branch name
  if (taskDataMap[id]) {
    taskDataMap[id].branch = branchName;
  }

  // Update the worktree cell in the task list to show the branch name
  const worktreeCell = $(`#task-${id}-Worktree`);
  if (worktreeCell.length > 0) {
    worktreeCell.text(escapeHtml(branchName));
    worktreeCell.attr("title", branchName);
  }
});

// Token usage update handler for context indicator
ipcRenderer.on("TokenUsageUpdate", (e, id, usage) => {
  const ring = $(`#task-${id}-Context .context-ring`);
  if (ring.length === 0) return;

  const lastTotalTokens = usage?.lastTokenUsage?.totalTokens || usage?.last_token_usage?.total_tokens || 0;
  const totalTokens = usage?.totalTokenUsage?.totalTokens || usage?.total_token_usage?.total_tokens || 0;
  const contextWindow = usage?.modelContextWindow || usage?.model_context_window || null;
  const usedTokens = lastTotalTokens > 0 ? lastTotalTokens : totalTokens;

  if (contextWindow && contextWindow > 0 && usedTokens > 0) {
    const freePercent = Math.max(0, 100 - (usedTokens / contextWindow) * 100);
    ring.removeClass("no-data");
    ring[0].style.setProperty("--context-free", freePercent);
    ring.attr(
      "data-tooltip",
      `${Math.round(freePercent)}% free\n${formatContextTokens(usedTokens)} / ${formatContextTokens(contextWindow)}`
    );
  } else if (usedTokens > 0) {
    // Have token usage but no context window info - show green ring with token count
    ring.removeClass("no-data");
    ring[0].style.setProperty("--context-free", 100);
    ring.attr("data-tooltip", `${formatContextTokens(usedTokens)} tokens used`);
  }

  // Store in task data map
  if (taskDataMap[id]) {
    taskDataMap[id].tokenUsage = usage;
  }
});

// Format token counts for display (e.g., 150000 -> "150K")
function formatContextTokens(n) {
  if (n == null || isNaN(n)) return "--";
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1).replace(/\.0$/, "") + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(0) + "K";
  return n.toString();
}

ipcRenderer.on("ProductUpdate", (e, id, message) => {
  $(`#task-${id}-Product`).attr("title", message);
  $(`#task-${id}-Product`).text(message);
});
$(function () {
  $('[data-toggle="tooltip"]').tooltip();
});

ipcRenderer.on("SizeUpdate", (e, id, size) => {
  $(`#task-${id}-Size`).text(size);
});
function StartTask(id) {
  const task = taskDataMap[id];
  if (task && task.statusState === "completed") {
    return;
  }
  // Prevent double-start when the user clicks rapidly.
  if (startingTasks[id]) {
    console.log("[Harness] StartTask ignored (already starting):", id);
    return;
  }
  startingTasks[id] = true;

  // Disable the play button immediately to prevent repeat clicks
  const playEl = $(`#task-${id} a.play`);
  playEl.addClass("disabled");
  playEl.css("pointer-events", "none");

  ipcRenderer.send("StartTask", id);

  // Update animation to running
  const logoEl = $(`#task-${id}-Logo`);
  logoEl.removeClass("idle running completed error");
  logoEl.addClass("running");
  if (taskDataMap[id]) {
    taskDataMap[id].statusState = "running";
  }
}

function StopTask(id) {
  ipcRenderer.send("StopTask", id);
  // Update animation to idle
  const logoEl = $(`#task-${id}-Logo`);
  logoEl.removeClass("idle running completed error");
  logoEl.addClass("idle");
  if (taskDataMap[id]) {
    taskDataMap[id].statusState = "idle";
  }
}

async function DeleteTask(id) {
  try {
    const result = await ipcRenderer.invoke('checkTaskUncommittedChanges', id);

    if (result && result.has_changes) {
      pendingDeleteTaskId = id;
      const pathEl = document.getElementById('deleteTaskWorktreePath');
      if (pathEl) pathEl.textContent = result.worktree_path || '';
      $('#deleteTaskWarningModal').modal('show');
    } else {
      performTaskDeletion(id);
    }
  } catch (err) {
    console.error('[Harness] Error checking uncommitted changes:', err);
    performTaskDeletion(id); // Fail-open: allow deletion if check fails
  }
}

function performTaskDeletion(id) {
  ipcRenderer.send("DeleteTask", id);
  $(`#task-${id}`).remove();
  let index = tasksOnPage.indexOf(id);
  if (index > -1) {
    tasksOnPage.splice(index, 1);
  }
  // Clean up task data map
  delete taskDataMap[id];
}

function ViewTaskLog(id) {
  console.log("[Harness] ViewTaskLog:", id);
  ipcRenderer.send("OpenAgentChatLog", id);
}

// Task action handlers (avoid inline onclick for CSP/release builds)
$("#tasks-table").on("click", "a.play, a.stop, a.view-log, a.delete", function (event) {
  event.preventDefault();
  const action = this.dataset.action;
  const taskId = this.dataset.taskId || $(this).closest("tr").data("task-id");
  if (!taskId) return;
  if (action === "start") {
    StartTask(taskId);
  } else if (action === "stop") {
    StopTask(taskId);
  } else if (action === "view-log") {
    ViewTaskLog(taskId);
  } else if (action === "delete") {
    DeleteTask(taskId);
  }
});

// Delete task confirmation modal handler
$('#confirmDeleteTask').on('click', function() {
  $('#deleteTaskWarningModal').modal('hide');
  if (pendingDeleteTaskId) {
    performTaskDeletion(pendingDeleteTaskId);
    pendingDeleteTaskId = null;
  }
});

// Table sorting functionality
function sortTasks(column) {
  if (currentSortState.column === column) {
    currentSortState.direction =
      currentSortState.direction === "asc" ? "desc" : "asc";
  } else {
    currentSortState.column = column;
    currentSortState.direction = "asc";
  }

  // Update header visual indicators
  $(".sortable-table thead th.sortable").removeClass("sort-asc sort-desc");
  $(`.sortable-table thead th.sortable[data-sort="${column}"]`).addClass(
    currentSortState.direction === "asc" ? "sort-asc" : "sort-desc",
  );

  // Get task rows and sort
  const rows = $("#tasks-table tr").toArray();

  rows.sort((a, b) => {
    const idA = $(a).data("task-id");
    const idB = $(b).data("task-id");
    const taskA = taskDataMap[idA];
    const taskB = taskDataMap[idB];

    if (!taskA || !taskB) return 0;

    let valA, valB;

    switch (column) {
      case "id":
        valA = taskA.displayId;
        valB = taskB.displayId;
        break;
      case "agent":
        valA = taskA.agent || "";
        valB = taskB.agent || "";
        break;
      case "model":
        valA = taskA.model || "";
        valB = taskB.model || "";
        break;
      case "status":
        valA = taskA.status || "";
        valB = taskB.status || "";
        break;
      case "worktree":
        valA = taskA.worktree || "";
        valB = taskB.worktree || "";
        break;
      case "cost":
        valA = taskA.cost || 0;
        valB = taskB.cost || 0;
        break;
      default:
        return 0;
    }

    // Numeric comparison for id and cost
    if (column === "id" || column === "cost") {
      return currentSortState.direction === "asc" ? valA - valB : valB - valA;
    }

    // String comparison for others
    valA = String(valA).toLowerCase();
    valB = String(valB).toLowerCase();
    if (valA < valB) return currentSortState.direction === "asc" ? -1 : 1;
    if (valA > valB) return currentSortState.direction === "asc" ? 1 : -1;
    return 0;
  });

  // Re-append sorted rows
  const tbody = $("#tasks-table");
  rows.forEach((row) => tbody.append(row));
}

// Setup sorting click handlers
$(document).on("click", ".sortable-table thead th.sortable", function () {
  const column = $(this).data("sort");
  if (column) {
    sortTasks(column);
  }
});
$("#globalRestartTasks").click(() => {
  ipcRenderer.send("RestartAll");
});
$("#globalStopTasks").click(() => {
  tasksOnPage.forEach((taskID) => {
    ipcRenderer.send("StopTask", taskID);
  });
});
$("#globalDeleteTasks").click(() => {
  for (let e = tasksOnPage.length - 1; e >= 0; e--) {
    const element = tasksOnPage[e];
    DeleteTask(element);
  }
});
$("#globalStartTasks").click(() => {
  tasksOnPage.forEach((taskID) => {
    const task = taskDataMap[taskID];
    if (!task) return;
    const isReady = task.status === "Ready" && task.statusState === "idle";
    if (!isReady) return;
    ipcRenderer.send("StartTask", taskID);
  });
});
$("#globalFocusMode").click(() => {
  focusMode();
});
// Export/Import removed - tasks are now agent sessions

$("#checkUpdates").click(() => {
  $("#statusMessage").removeClass("green-text");
  $("#statusMessage").text("Checking For Updates");
  ipcRenderer.send("CheckUpdates");
});

ipcRenderer.on("NoUpdates", (e) => {
  $("#statusMessage").addClass("blue-text");
  $("#statusMessage").text("Up To Date");
  setTimeout(setNormal, 999);
});
ipcRenderer.on("update_prog", (e, update_text) => {
  $("#statusMessage").addClass("blue-text");
  $("#statusMessage").text(update_text);
});

function sendNotification(message, color) {
  $("#statusMessage").removeClass("green-text");
  if (color) {
    $("#statusMessage").text(message);
    $("#statusMessage").addClass(`${color}-text`);
  } else {
    $("#statusMessage").text(message);
  }
  setTimeout(() => {
    setNormal(color ? color : null);
  }, 5000);
}
function setNormal(color) {
  if (color) {
    $("#statusMessage").removeClass(`${color}-text`);
    $("#statusMessage").addClass("green-text");
    $("#statusMessage").text("Connected");
  } else {
    $("#statusMessage").removeClass("blue-text");
    $("#statusMessage").addClass("green-text");
    $("#statusMessage").text("Connected");
  }
}

ipcRenderer.on("StartAllTB", (e) => {
  tasksOnPage.forEach((taskID) => {
    const task = taskDataMap[taskID];
    if (!task) return;
    const isReady = task.status === "Ready" && task.statusState === "idle";
    if (!isReady) return;
    ipcRenderer.send("StartTask", taskID);
  });
});
ipcRenderer.on("StopAllTB", (e) => {
  tasksOnPage.forEach((taskID) => {
    ipcRenderer.send("StopTask", taskID);
  });
});
ipcRenderer.on("RestartAllTB", (e) => {
  ipcRenderer.send("RestartAll");
});
ipcRenderer.on("DeleteAllTB", (e) => {
  for (let e = tasksOnPage.length - 1; e >= 0; e--) {
    const element = tasksOnPage[e];
    DeleteTask(element);
  }
});

$("#testDiscord").click(() => {
  ipcRenderer.send("testDiscord");
});

$("#refreshAgentsBtn").click(async () => {
  const btn = $("#refreshAgentsBtn");
  btn.prop("disabled", true);
  const originalText = btn.text();
  btn.text("Refreshing...");
  try {
    await ipcRenderer.invoke("refreshAgentAvailability");
    sendNotification("Agent availability refreshed", "green");
  } catch (err) {
    console.warn("[Harness] refreshAgentAvailability failed:", err);
    sendNotification("Failed to refresh agent availability", "red");
  } finally {
    btn.text(originalText);
    btn.prop("disabled", false);
  }
});

ipcRenderer.on("holiday_mode", holidayMode);
function holidayMode() {
  //var audio = new Audio('iag.mp3');
  //audio.play();
  document.title = "Jolly Phantom";
  snowStorm.start();
  $(
    "#top-titlebar > nav.navbar.navbar-expand-sm.navbar-dark.pink.draggable",
  ).css("background", "#96c4f9");
  bridge.app.getVersion().then((appVersion) => {
    $("#note").text(`Jolly Phantom Version ${appVersion}`);
  });
  $(".btn-brand").css("background-color", "#96c4f9");
  $(".slider:before").css("background-color", "#96c4f9");
  $(".slider").css("border", "2px solid #96c4f9");
  $("#removeBadButton").css("background-color", "#96c4f9");
  $("#accountOpenSaveLocationButton").css("background-color", "#96c4f9");
  //$('#tasks-table').css('background', '')
}
let gifs = {
  first: [
    "https://media.giphy.com/media/3ofT5ztPo7htHZyM8M/giphy.gif",
    "https://media.giphy.com/media/F9hQLAVhWnL56/giphy.gif",
    "https://media.giphy.com/media/3o7budMRwZvNGJ3pyE/giphy.gif",
  ],
  second: [
    "https://media.giphy.com/media/xdkCzArnlCw24/giphy.gif",
    "https://data.whicdn.com/images/286420291/original.gif",
    "https://media.giphy.com/media/kkYbDLFmNvO4E/giphy.gif",
  ],
  third: [
    "https://media.giphy.com/media/J6VR03Q4GRO12/giphy.gif",
    "https://media.giphy.com/media/7UL2HdkAtRH8Y/giphy.gif",
    "https://media.giphy.com/media/l4FGrYKtP0pBGpBAY/giphy.gif",
  ],
};

function focusMode() {
  ipcRenderer.send("LaunchHelper");
  let things = [
    "#globalStartTasks",
    "#globalStopTasks",
    "#globalDeleteTasks",
    "#globalImport",
    "a[data-page=createTasksPage]",
    "a[data-page=settingsPage]",
  ];
  things.forEach((thing) => {
    //$(thing).addClass('disabled')
    $(thing).fadeOut();
  });
  $("#statusMessage").text(`Focus Mode Enabled`);
  $("#statusMessage").addClass("text-info");
}

/* HANDLE USER RESUBS */
$("#openRCLogin").click(() => {
  ipcRenderer.send("showRCLogin");
});

// RCM (Captcha Manager) removed - not used for agent sessions
ipcRenderer.on("user-resub-complete", (e, resubData) => {
  $("span#userResubExpireDate").text(resubData.expiration);
});

ipcRenderer.on("user-resub-started", (e) => {
  $("#userResub")
    .text("Resubbing...")
    .removeClass("btn-success")
    .addClass("btn-warning")
    .attr("disabled", true);
});

ipcRenderer.on("user-resub-closed", (e) => {
  $("#userResub")
    .text("Renew Subscription (6 Months)")
    .removeClass("btn-warning")
    .addClass("btn-success")
    .attr("disabled", false);
});

$("#userResub").click(() => {
  ipcRenderer.send(`user-resub-start`);
});
ipcRenderer.on("expiration-update", (e, d) => {});

$("#userInvite").click(() => {
  ipcRenderer.send(`inviteRequest`);
});

ipcRenderer.on("inviteResponse", (e, b) => {
  $("#singleUseInstanceCode").val(b);
});
// Captcha Harvester removed - not used for agent sessions

$("#youtubeLogin").click(() => {
  ipcRenderer.send("youtubeLogin");
});

ipcRenderer.on("user_config", (e, d, key) => {
  if (d.expires == "01/01/2070") {
    $("#userResub").addClass("disabled");
    return $("span#userResubExpireDate").text("Never");
  }
  $("span#userResubExpireDate").text(d.expires);
});
ipcRenderer.on("socket_disconnect", () => {
  $("#statusMessage").text("Socket Disconnected");
  $("#statusMessage").removeClass("green-text");
  $("#statusMessage").addClass("red-text");
});

ipcRenderer.on("socket_error", () => {
  $("#statusMessage").text("Socket Connection Error");
  $("#statusMessage").removeClass("green-text");
  $("#statusMessage").addClass("red-text");
});

ipcRenderer.on("socket_connected", () => {
  setTimeout(() => {
    $("#statusMessage").removeClass("red-text");
    $("#statusMessage").addClass("green-text");
    $("#statusMessage").text("Connected");
  }, 500);
});
ipcRenderer.on("socket_event", (e, d) => {
  $("#statusMessage").text(d);
  $("#statusMessage").removeClass("green-text");
  $("#statusMessage").addClass("red-text");
});

let footerStatusSticky = false;

function setFooterStatus(message, color) {
  $("#statusMessage").removeClass("green-text blue-text red-text");
  if (color) {
    $("#statusMessage").addClass(`${color}-text`);
  }
  $("#statusMessage").text(message);
}

window.setFooterStatus = setFooterStatus;

ipcRenderer.on("FooterStatus", (e, message, color, sticky) => {
  footerStatusSticky = !!sticky;
  setFooterStatus(message, color || "blue");
});

ipcRenderer.on("FooterStatusClear", () => {
  footerStatusSticky = false;
  $("#statusMessage").removeClass("green-text blue-text red-text");
  $("#statusMessage").addClass("green-text");
  $("#statusMessage").text("Connected");
});

window.ghost_uid = remote.getGlobal("user_id");
window.app_version = remote.getGlobal("app_version");
window.device_id = remote.getGlobal("machine_id");

$("#note").text(`Phantom v${window.app_version}`);

function logEvent(event) {
  return;
}

async function getSettings() {
  let settingsPayload = await ipcRenderer.invoke("getSettings");

  if (!settingsPayload) {
    return;
  }
  currentSettings = settingsPayload;
  if (settingsPayload.discordBotToken !== undefined) {
    $("#discordBotToken").val(settingsPayload.discordBotToken || "");
  }
  if (settingsPayload.discordChannelId !== undefined) {
    $("#discordChannelId").val(settingsPayload.discordChannelId || "");
  }
  if (settingsPayload.discordEnabled !== undefined) {
    $("#discordEnabled").prop("checked", !!settingsPayload.discordEnabled);
  } else {
    $("#discordEnabled").prop("checked", false);
  }
  if (settingsPayload.retryDelay !== undefined) {
    $("#retryDelay").val(settingsPayload.retryDelay);
  }
  if (settingsPayload.errorDelay !== undefined) {
    $("#errorDelay").val(settingsPayload.errorDelay);
  }
  if (settingsPayload.ignoreDeclines !== undefined) {
    $("#ignoreDeclines").prop("checked", !!settingsPayload.ignoreDeclines);
  }
  if (settingsPayload.agentNotificationsEnabled !== undefined) {
    $("#agentNotificationsEnabled").prop(
      "checked",
      !!settingsPayload.agentNotificationsEnabled,
    );
  } else {
    $("#agentNotificationsEnabled").prop("checked", true);
  }
  if (settingsPayload.agentNotificationStack !== undefined) {
    $("#agentNotificationStack").prop(
      "checked",
      !!settingsPayload.agentNotificationStack,
    );
  } else {
    $("#agentNotificationStack").prop("checked", true);
  }

  // AI Summaries setting (default enabled)
  if (settingsPayload.aiSummariesEnabled !== undefined) {
    $("#aiSummariesEnabled").prop("checked", !!settingsPayload.aiSummariesEnabled);
  } else {
    $("#aiSummariesEnabled").prop("checked", true);
  }

  if (settingsPayload.containerIsolationEnabled !== undefined) {
    $("#containerIsolationEnabled").prop(
      "checked",
      !!settingsPayload.containerIsolationEnabled,
    );
  } else {
    $("#containerIsolationEnabled").prop("checked", false);
  }

  if (settingsPayload.taskProjectAllowlist !== undefined) {
    currentSettings.taskProjectAllowlist = Array.isArray(
      settingsPayload.taskProjectAllowlist,
    )
      ? settingsPayload.taskProjectAllowlist
      : [];
  } else {
    currentSettings.taskProjectAllowlist = [];
  }
  renderProjectAllowlist();

  updateCreateTaskButtonLabel();


  document.querySelectorAll("[data-auth-key]").forEach((input) => {
    const key = input.dataset.authKey;
    if (!key) return;
    const value = settingsPayload[key];
    if (value !== undefined) {
      input.value = value || "";
    }
  });

  document.querySelectorAll("[data-auth-status]").forEach((node) => {
    const key = node.dataset.authStatus;
    if (key === "codex" && settingsPayload.codexAuthMethod) {
      node.textContent =
        settingsPayload.codexAuthMethod === "chatgpt"
          ? "ChatGPT subscription linked"
          : "API key configured";
    }
    if (key === "claude" && settingsPayload.claudeAuthMethod) {
      node.textContent =
        settingsPayload.claudeAuthMethod === "cli"
          ? "Claude Code login linked"
          : "API key configured";
    }
  });

  // Restore task creation settings
  restoreTaskSettings(settingsPayload);
  
  // Initialize settings page toggle buttons
  initSettingsToggles();
}

// Initialize and sync settings page toggle buttons with their checkboxes
function initSettingsToggles() {
  document.querySelectorAll('.toggle-buttons').forEach(container => {
    if (container.dataset.toggleInit === "true") return;
    container.dataset.toggleInit = "true";

    const checkbox = container.querySelector('input[type="checkbox"]');
    const buttons = container.querySelectorAll('.toggle-button');

    if (!checkbox || buttons.length < 2) return;
    
    // Sync button state with checkbox
    const syncButtons = () => {
      const isChecked = checkbox.checked;
      // Toggle is-on class on container for sliding animation
      container.classList.toggle('is-on', isChecked);
      buttons.forEach(btn => {
        const btnValue = btn.dataset.value === 'true';
        btn.classList.toggle('is-active', btnValue === isChecked);
      });
    };
    
    // Initial sync
    syncButtons();
    
    // Handle button clicks
    buttons.forEach(btn => {
      btn.addEventListener('click', () => {
        if (checkbox.disabled) return;
        const newValue = btn.dataset.value === 'true';
        checkbox.checked = newValue;
        syncButtons();
        // Trigger change event for save handlers
        checkbox.dispatchEvent(new Event('change', { bubbles: true }));
      });
    });
    
    // Sync when checkbox changes programmatically
    checkbox.addEventListener('change', syncButtons);
  });
}

// Restore task creation form settings from saved settings
function restoreTaskSettings(settings) {
  if (!settings) return;

  // Reset multi-agent selection to a single agent on load
  // Note: selectedAgentIds is scoped to initAgentCreateUI IIFE, use window exposure
  if (window.selectedAgentIds) {
    window.selectedAgentIds.clear();
  }

  // Restore project path
  if (settings.taskProjectPath) {
    setProjectPath(settings.taskProjectPath);
  }

  // Restore toggles
  if (settings.taskPlanMode !== undefined) {
    $("#planModeToggle").prop("checked", settings.taskPlanMode);
  }
  if (settings.taskUseWorktree !== undefined) {
    $("#useWorktreeToggle").prop("checked", settings.taskUseWorktree);
  }
  if (settings.taskBaseBranch !== undefined) {
    pendingBaseBranchValue = settings.taskBaseBranch;
  }

  // Select last used agent if available
  if (settings.taskLastAgent) {
    const agentCard = document.querySelector(
      `.agent-card[data-agent-id="${settings.taskLastAgent}"]`,
    );
    if (agentCard && window.selectAgentById) {
      window.selectAgentById(settings.taskLastAgent);
    }
  }

  refreshBaseBranchOptions();
}

// Save task creation settings - core logic
async function saveTaskSettingsCore(agentIdOverride) {
  const agentId = agentIdOverride || window.activeAgentId || "codex";
  const agentModels = currentSettings.taskAgentModels || {};

  const permMode = permissionDropdown ? permissionDropdown.getValue() : "default";
  const execVal = execModelDropdown ? execModelDropdown.getValue() : "default";
  const reasoningEffortVal = reasoningEffortDropdown ? reasoningEffortDropdown.getValue() : "default";
  const agentModeVal = agentModeDropdown ? agentModeDropdown.getValue() : "default";
  console.log(
    "[Harness] Saving settings for",
    agentId,
    "- permission:",
    permMode,
    "exec:",
    execVal,
    "reasoningEffort:",
    reasoningEffortVal,
    "agentMode:",
    agentModeVal,
  );

  // Save current agent's settings (permission mode + model + reasoning effort + agent mode)
  agentModels[agentId] = {
    permissionMode: permMode,
    execModel: execVal,
    reasoningEffort: reasoningEffortVal,
    agentMode: agentModeVal,
  };

  const baseBranchValue = baseBranchDropdown ? baseBranchDropdown.getValue() : "default";
  const baseBranch =
    baseBranchValue && baseBranchValue !== "default" ? baseBranchValue : null;

  const taskSettings = {
    taskProjectPath: getProjectPath(),
    taskPlanMode: $("#planModeToggle").is(":checked"),
    taskUseWorktree: $("#useWorktreeToggle").is(":checked"),
    taskBaseBranch: baseBranch,
    taskLastAgent: agentId,
    taskAgentModels: agentModels,
  };

  const updated = Object.assign({}, currentSettings, taskSettings);
  currentSettings = updated;

  try {
    await ipcRenderer.invoke("saveSettings", updated);
    console.log("[Harness] Task settings saved for agent:", agentId);
  } catch (err) {
    console.warn("[Harness] Failed to save task settings", err);
  }
}

// Save immediately (for agent switching)
async function saveTaskSettingsImmediate(agentId) {
  if (taskSettingsSaveTimeout) {
    clearTimeout(taskSettingsSaveTimeout);
    taskSettingsSaveTimeout = null;
  }
  await saveTaskSettingsCore(agentId);
}

// Save task creation settings (debounced to avoid excessive writes)
let taskSettingsSaveTimeout = null;
function saveTaskSettings() {
  if (taskSettingsSaveTimeout) {
    clearTimeout(taskSettingsSaveTimeout);
  }
  taskSettingsSaveTimeout = setTimeout(() => {
    saveTaskSettingsCore();
  }, 500); // Debounce 500ms
}

// Restore settings (permission mode + model + agent mode) for a specific agent
function restoreAgentModelSelections(agentId) {
  const agentModels = currentSettings.taskAgentModels || {};
  const prefs = agentModels[agentId];
  console.log("[Harness] Restoring settings for", agentId);
  console.log("[Harness] All agentModels:", JSON.stringify(agentModels));
  console.log("[Harness] Prefs for", agentId, ":", JSON.stringify(prefs));
  if (prefs) {
    // Log available model options
    const execOptions = execModelDropdown ? execModelDropdown.items.map(item => item.value) : [];
    console.log("[Harness] Available exec options:", execOptions);

    // Restore permission mode
    if (prefs.permissionMode && permissionDropdown) {
      permissionDropdown.setValue(prefs.permissionMode);
      console.log("[Harness] Restored permissionMode:", prefs.permissionMode);
    }

    // Restore model
    if (prefs.execModel && prefs.execModel !== "default" && execModelDropdown) {
      const hasOption = execOptions.includes(prefs.execModel);
      console.log(
        "[Harness] Restore execModel:",
        prefs.execModel,
        "exists:",
        hasOption,
      );
      if (hasOption) {
        execModelDropdown.setValue(prefs.execModel);
        console.log("[Harness] Set execModelDropdown to:", execModelDropdown.getValue());
      }
    }

    // Restore reasoning effort (for Codex)
    if (agentId === "codex" && prefs.reasoningEffort && reasoningEffortDropdown) {
      const reasoningOptions = reasoningEffortDropdown.items.map(item => item.value);
      const hasReasoningOption = reasoningOptions.includes(prefs.reasoningEffort);
      console.log(
        "[Harness] Restore reasoningEffort:",
        prefs.reasoningEffort,
        "exists:",
        hasReasoningOption,
      );
      if (hasReasoningOption) {
        reasoningEffortDropdown.setValue(prefs.reasoningEffort);
        console.log("[Harness] Set reasoningEffortDropdown to:", reasoningEffortDropdown.getValue());
      }
    }

    // Restore agent mode (for Codex and OpenCode)
    const agentsWithModes = ["codex", "opencode"];
    if (agentsWithModes.includes(agentId) && prefs.agentMode && agentModeDropdown) {
      const modeOptions = agentModeDropdown.items.map(item => item.value);
      const hasModeOption = modeOptions.includes(prefs.agentMode);
      console.log(
        "[Harness] Restore agentMode:",
        prefs.agentMode,
        "exists:",
        hasModeOption,
      );
      if (hasModeOption) {
        agentModeDropdown.setValue(prefs.agentMode);
        console.log("[Harness] Set agentModeDropdown to:", agentModeDropdown.getValue());
      }
    }
  } else {
    console.log("[Harness] No saved prefs for", agentId);
  }
}

// Separate function to restore reasoning effort AFTER updateReasoningEffortDropdown populates options
function restoreReasoningEffort(agentId) {
  if (agentId !== 'codex' || !reasoningEffortDropdown) return;

  const agentModels = currentSettings.taskAgentModels || {};
  const prefs = agentModels[agentId];

  if (prefs && prefs.reasoningEffort) {
    const reasoningOptions = reasoningEffortDropdown.items.map(item => item.value);
    const hasOption = reasoningOptions.includes(prefs.reasoningEffort);
    console.log(
      "[Harness] Restore reasoningEffort:",
      prefs.reasoningEffort,
      "available options:",
      reasoningOptions,
      "exists:",
      hasOption
    );
    if (hasOption) {
      reasoningEffortDropdown.setValue(prefs.reasoningEffort);
      console.log("[Harness] Restored reasoningEffort to:", reasoningEffortDropdown.getValue());
    }
  }
}

function init() {
  initCustomDropdowns(); // Initialize custom dropdowns first
  getSettings();
  initAuthState(); // Check Codex auth on startup
  // Load persisted tasks from database
  setTimeout(() => {
    ipcRenderer
      .invoke("loadTasks")
      .then(function (tasks) {
        if (Array.isArray(tasks)) {
          console.log("[Harness] Loading", tasks.length, "persisted tasks");
          tasks.forEach(function (task) {
            if (task.project_path) {
              addRecentProjectPath(task.project_path);
            }
            // Emit AddTask for each persisted task
            window.tauriEmitEvent("AddTask", null, task.id, {
              ID: task.id,
              agent: task.agent_id,
              model: task.model,
              Status: task.status,
              statusState: task.status_state,
              cost: task.cost,
              worktreePath: task.worktreePath || null,
              totalTokens: task.totalTokens,
              contextWindow: task.contextWindow,
            });
          });
          renderProjectAllowlist();
        }
      })
      .catch(function (err) {
        console.warn("[Harness] loadTasks failed:", err);
      });
  }, 500);

  // Native Tauri event listeners for events emitted from Rust backend
  if (window.__TAURI__ && window.__TAURI__.event) {
    window.__TAURI__.event.listen("StatusUpdate", function (event) {
      var payload = event.payload;
      if (Array.isArray(payload) && payload.length >= 2) {
        var id = payload[0];
        var message = payload[1];
        var color = payload[2] || "white";
        var statusState = payload[3] || null;

        // Update status cell
        var statusEl = $("#task-" + id + "-Status");
        var isThinking = message === "Thinking..." ||
                         message.indexOf("OpenCode is working") === 0;
        var isCompleted = statusState === "completed";

        // Toggle status classes
        statusEl.toggleClass("status-thinking", isThinking);
        statusEl.toggleClass("status-completed", isCompleted);

        // Only apply color if NOT thinking and NOT completed (they use CSS classes)
        if (!isThinking && !isCompleted) {
          statusEl.animate({ color: color }, 0);
        }

        statusEl.text(message).attr("title", message);

        // Update task data and animation
        if (taskDataMap[id]) {
          taskDataMap[id].status = message;
          if (statusState) {
            taskDataMap[id].statusState = statusState;
          }
        }

        if (statusState) {
          var logoEl = $("#task-" + id + "-Logo");
          logoEl
            .removeClass("idle running completed error")
            .addClass(getAnimationClass(statusState));
        }
      }
    });

    window.__TAURI__.event.listen("CostUpdate", function (event) {
      var payload = event.payload;
      if (Array.isArray(payload) && payload.length >= 2) {
        var id = payload[0];
        var cost = payload[1];
        $("#task-" + id + "-Cost").text(formatCost(cost));
        if (taskDataMap[id]) {
          taskDataMap[id].cost = cost;
        }
      }
    });

    window.__TAURI__.event.listen("TokenUsageUpdate", function (event) {
      var payload = event.payload;
      if (Array.isArray(payload) && payload.length >= 2) {
        var id = payload[0];
        var usage = payload[1];
        var ring = $("#task-" + id + "-Context .context-ring");
        if (ring.length === 0) return;

        var lastTotalTokens = (usage && usage.lastTokenUsage && usage.lastTokenUsage.totalTokens) ||
                              (usage && usage.last_token_usage && usage.last_token_usage.total_tokens) || 0;
        var totalTokens = (usage && usage.totalTokenUsage && usage.totalTokenUsage.totalTokens) ||
                          (usage && usage.total_token_usage && usage.total_token_usage.total_tokens) || 0;
        var contextWindow = (usage && usage.modelContextWindow) || (usage && usage.model_context_window) || null;
        var usedTokens = lastTotalTokens > 0 ? lastTotalTokens : totalTokens;

        if (contextWindow && contextWindow > 0 && usedTokens > 0) {
          var freePercent = Math.max(0, 100 - (usedTokens / contextWindow) * 100);
          ring.removeClass("no-data");
          ring[0].style.setProperty("--context-free", freePercent);
          ring.attr("data-tooltip", Math.round(freePercent) + "% free\n" + formatContextTokens(usedTokens) + " / " + formatContextTokens(contextWindow));
        } else if (usedTokens > 0) {
          ring.removeClass("no-data");
          ring[0].style.setProperty("--context-free", 100);
          ring.attr("data-tooltip", formatContextTokens(usedTokens) + " tokens used");
        }

        if (taskDataMap[id]) {
          taskDataMap[id].tokenUsage = usage;
        }
      }
    });
  }
}

init();

// Agent create UI (ACP harness)
(function initAgentCreateUI() {
  const agentCards = document.querySelectorAll(".agent-card");
  if (!agentCards.length) {
    return;
  }

  let activeAgentId = null;
  let primaryAgentId = null;
  const selectedAgentIds = new Set();
  window.selectedAgentIds = selectedAgentIds; // Expose for restoreTaskSettings
  let multiSelectModifierActive = false;

  document.addEventListener("keydown", (event) => {
    if (!event) return;
    if (event.key === "Shift" || event.key === "Meta" || event.key === "Control") {
      multiSelectModifierActive = true;
    }
  });

  document.addEventListener("keyup", (event) => {
    if (!event) return;
    if (event.key === "Shift" || event.key === "Meta" || event.key === "Control") {
      multiSelectModifierActive = false;
    }
  });

  window.addEventListener("blur", () => {
    multiSelectModifierActive = false;
  });

  function markSelected() {
    agentCards.forEach((card) => {
      card.classList.toggle("selected", selectedAgentIds.has(card.dataset.agentId));
    });
    activeAgentId = primaryAgentId;
    window.activeAgentId = primaryAgentId; // Expose for saveTaskSettings

    // Update slash commands for the new agent
    if (primaryAgentId && window.promptSlashCommands) {
      window.promptSlashCommands.setAgent(primaryAgentId);
    }
  }

  function getSelectedAgents() {
    if (selectedAgentIds.size === 0 && primaryAgentId) {
      selectedAgentIds.add(primaryAgentId);
    }
    return Array.from(selectedAgentIds);
  }

  function setPrimaryAgent(agentId) {
    primaryAgentId = agentId;
    if (agentId) {
      selectedAgentIds.add(agentId);
    }
    markSelected();
    updateWorktreeLock();
    updateMultiAgentUiVisibility();
  }

  function selectSingleAgent(agentId) {
    selectedAgentIds.clear();
    selectedAgentIds.add(agentId);
    primaryAgentId = agentId;
    markSelected();
    updateWorktreeLock();
    updateMultiAgentUiVisibility();
  }

  function updateWorktreeLock() {
    const useWorktreeToggle = document.getElementById("useWorktreeToggle");
    const worktreeHint = document.getElementById("worktreeMultiHint");
    const toggleContainer = useWorktreeToggle ? useWorktreeToggle.closest(".toggle-buttons") : null;
    const multi = selectedAgentIds.size > 1;

    if (!useWorktreeToggle) return;

    if (multi) {
      setToggleState("useWorktreeToggle", true);
      useWorktreeToggle.disabled = true;
      if (toggleContainer) {
        toggleContainer.classList.add("is-disabled");
      }
      if (worktreeHint) {
        worktreeHint.style.display = "block";
      }
    } else {
      useWorktreeToggle.disabled = false;
      if (toggleContainer) {
        toggleContainer.classList.remove("is-disabled");
      }
      if (worktreeHint) {
        worktreeHint.style.display = "none";
      }
    }
    updateMultiAgentUiVisibility();
  }

  function updateMultiAgentUiVisibility() {
    const multi = selectedAgentIds.size > 1;
    const permissionModeGroup = document.getElementById("permissionModeGroup");
    const agentModeGroup = document.getElementById("agentModeGroup");
    const reasoningEffortGroup = document.getElementById("reasoningEffortGroup");
    const modelGroup = execModelDropdown && execModelDropdown.container
      ? execModelDropdown.container.closest(".form-group")
      : null;

    if (permissionModeGroup) {
      permissionModeGroup.classList.toggle("multi-agent-hidden", multi);
    }
    if (agentModeGroup) {
      agentModeGroup.classList.toggle("multi-agent-hidden", multi);
    }
    if (reasoningEffortGroup) {
      reasoningEffortGroup.classList.toggle("multi-agent-hidden", multi);
    }
    if (modelGroup) {
      modelGroup.classList.toggle("multi-agent-hidden", multi);
    }
  }

  // Set options on custom dropdown for models
  function setSelectOptions(dropdown, models) {
    if (!dropdown) {
      console.warn("[Harness] setSelectOptions called with null dropdown");
      return;
    }
    const normalized = normalizeModels(models);
    console.log(
      "[Harness] setSelectOptions called with",
      normalized.length,
      "models",
    );
    const currentValue = dropdown.getValue();

    // Build items array for CustomDropdown
    const items = [{ value: 'default', name: 'Use agent default', description: '' }];
    let addedCount = 0;

    normalized.forEach((model) => {
      let value, name, description;

      if (typeof model === "string") {
        value = model;
        name = model;
        description = '';
        console.log("[Harness] Adding string model:", model);
      } else if (model && typeof model === "object") {
        value = model.value || model.modelId || model.id || "";
        name = model.name || model.label || value;
        description = model.description || '';
        // Capitalize first letter of name for consistency (e.g., "opus" -> "Opus")
        if (name && typeof name === "string" && name.length > 0) {
          name = name.charAt(0).toUpperCase() + name.slice(1);
        }
        console.log("[Harness] Adding object model:", value, "->", name);
      }

      if (value) {
        items.push({ value, name, description });
        addedCount++;
      } else {
        console.warn("[Harness] Skipping model with no value:", model);
      }
    });

    console.log("[Harness] Added", addedCount, "options to dropdown");
    dropdown.setOptions(items);

    // Restore previous value if it still exists
    if (currentValue && items.some(item => item.value === currentValue)) {
      dropdown.setValue(currentValue);
    }
  }

  function normalizeModels(models) {
    if (!models) return [];
    if (Array.isArray(models)) return models;
    if (models.availableModels && Array.isArray(models.availableModels)) {
      return models.availableModels;
    }
    if (models.models && Array.isArray(models.models)) {
      return models.models;
    }
    return [];
  }

  // Model cache for instant switching (populated from SQLite on startup)
  const modelCache = {};

  // Mode cache for agents that expose modes
  const modeCache = {};

  // Load all cached modes from SQLite on startup (instant)
  async function loadCachedModesFromDb() {
    console.log("[Harness] loadCachedModesFromDb starting...");
    try {
      const allCached = await ipcRenderer.invoke("getAllCachedModes");
      console.log("[Harness] getAllCachedModes returned:", allCached);
      if (allCached && typeof allCached === "object") {
        for (const [agentId, modes] of Object.entries(allCached)) {
          if (Array.isArray(modes) && modes.length > 0) {
            modeCache[agentId] = modes;
            console.log(
              "[Harness] Loaded",
              modes.length,
              "cached modes for",
              agentId,
            );
          }
        }
      }
    } catch (err) {
      console.error("[Harness] Failed to load cached modes from DB:", err);
    }
    console.log(
      "[Harness] loadCachedModesFromDb done. Cache:",
      Object.keys(modeCache),
    );
  }

  // Refresh modes from agent in background (slow, updates cache)
  async function refreshModesInBackground(agentId) {
    try {
      console.log("[Harness] Background mode refresh for", agentId);
      const freshModes = await ipcRenderer.invoke("refreshAgentModes", agentId);
      if (freshModes && freshModes.length > 0) {
        const oldModes = modeCache[agentId] || [];
        const changed = JSON.stringify(freshModes) !== JSON.stringify(oldModes);

        modeCache[agentId] = freshModes;
        console.log(
          "[Harness] Background mode refresh got",
          freshModes.length,
          "modes for",
          agentId,
          "changed:",
          changed,
        );

        // If this is the active agent and modes changed, update UI
        if (changed && activeAgentId === agentId) {
          console.log("[Harness] Updating UI with fresh modes for", agentId);
          if (agentModeDropdown) {
            setModeOptions(agentModeDropdown, freshModes);
          }
        }
      }
    } catch (err) {
      console.warn("[Harness] Background mode refresh failed for", agentId, err);
    }
  }

  // Load modes for an agent (from cache or fetch)
  async function loadModes(agentId) {
    console.log(
      "[Harness] loadModes called for:",
      agentId,
      "cache has:",
      Object.keys(modeCache),
    );
    // Return cached immediately if available (instant UX)
    if (modeCache[agentId] && modeCache[agentId].length > 0) {
      console.log(
        "[Harness] Using cached modes for:",
        agentId,
        "(",
        modeCache[agentId].length,
        "modes)",
      );
      // Trigger background refresh for freshness
      setTimeout(() => refreshModesInBackground(agentId), 0);
      return modeCache[agentId];
    }

    // No cache - must wait for fresh fetch
    console.log(
      "[Harness] No cached modes for",
      agentId,
      "- calling refreshAgentModes...",
    );
    try {
      const modes = await ipcRenderer.invoke("refreshAgentModes", agentId);
      console.log("[Harness] refreshAgentModes returned:", modes?.length || 0, "modes");
      if (modes && modes.length > 0) {
        modeCache[agentId] = modes;
      }
      return modes || [];
    } catch (err) {
      console.error("[Harness] refreshAgentModes failed for", agentId, ":", err);
      return [];
    }
  }

  // Set mode options on a custom dropdown
  function setModeOptions(dropdown, modes) {
    if (!dropdown) {
      console.warn("[Harness] setModeOptions called with null dropdown");
      return;
    }
    const currentValue = dropdown.getValue();

    // Build items array for CustomDropdown
    const items = [{ value: 'default', name: 'Use default', description: '' }];

    if (Array.isArray(modes)) {
      modes.forEach((mode) => {
        items.push({
          value: mode.value,
          name: mode.name || mode.value,
          description: mode.description || ''
        });
      });
    }

    console.log("[Harness] Setting", items.length - 1, "mode options");
    dropdown.setOptions(items);

    // Restore previous value if it still exists
    if (currentValue && items.some(item => item.value === currentValue)) {
      dropdown.setValue(currentValue);
    }
  }

  // Fetch enriched models (with reasoning effort data) for Codex
  async function loadEnrichedModels(agentId) {
    if (agentId !== 'codex') {
      return null;
    }

    // Check cache first
    if (enrichedModelCache[agentId]) {
      console.log('[Harness] Using cached enriched models for', agentId);
      return enrichedModelCache[agentId];
    }

    try {
      console.log('[Harness] Fetching enriched models for', agentId);
      const enrichedModels = await ipcRenderer.invoke('getEnrichedModels', agentId);
      if (enrichedModels && enrichedModels.length > 0) {
        enrichedModelCache[agentId] = enrichedModels;
        console.log('[Harness] Got', enrichedModels.length, 'enriched models for', agentId);
        return enrichedModels;
      }
    } catch (err) {
      console.error('[Harness] Failed to fetch enriched models:', err);
    }
    return null;
  }

  // Update reasoning effort dropdown based on selected model
  function updateReasoningEffortDropdown(selectedModelValue) {
    const reasoningGroup = document.getElementById('reasoningEffortGroup');

    // Only show for Codex
    if (activeAgentId !== 'codex') {
      if (reasoningGroup) reasoningGroup.style.display = 'none';
      return;
    }

    // Get enriched model data from cache
    const enrichedModels = enrichedModelCache['codex'];
    if (!enrichedModels) {
      if (reasoningGroup) reasoningGroup.style.display = 'none';
      return;
    }

    // Find the selected model's reasoning efforts
    let modelData = null;
    if (selectedModelValue === 'default') {
      // Find the default model
      modelData = enrichedModels.find(m => m.isDefault);
    } else {
      modelData = enrichedModels.find(m => m.value === selectedModelValue);
    }

    if (!modelData || !modelData.supportedReasoningEfforts || modelData.supportedReasoningEfforts.length === 0) {
      // No reasoning efforts for this model, hide dropdown
      if (reasoningGroup) reasoningGroup.style.display = 'none';
      return;
    }

    // Show dropdown and populate options
    if (reasoningGroup) reasoningGroup.style.display = '';

    if (reasoningEffortDropdown) {
      const items = [{ value: 'default', name: 'Default', description: `Uses model default: ${modelData.defaultReasoningEffort || 'medium'}` }];

      modelData.supportedReasoningEfforts.forEach(effort => {
        items.push({
          value: effort.value,
          name: capitalizeFirst(effort.value),
          description: effort.description || ''
        });
      });

      console.log('[Harness] Setting reasoning effort options:', items.length - 1, 'options');
      reasoningEffortDropdown.setOptions(items);
      reasoningEffortDropdown.setValue('default');
    }
  }

  // Capitalize first letter helper
  function capitalizeFirst(str) {
    if (!str) return str;
    return str.charAt(0).toUpperCase() + str.slice(1);
  }

  // Load all cached models from SQLite on startup (instant)
  async function loadCachedModelsFromDb() {
    console.log("[Harness] loadCachedModelsFromDb starting...");
    try {
      console.log("[Harness] Calling getAllCachedModels...");
      const allCached = await ipcRenderer.invoke("getAllCachedModels");
      console.log("[Harness] getAllCachedModels returned:", allCached);
      if (allCached && typeof allCached === "object") {
        for (const [agentId, models] of Object.entries(allCached)) {
          if (Array.isArray(models) && models.length > 0) {
            modelCache[agentId] = models;
            console.log(
              "[Harness] Loaded",
              models.length,
              "cached models for",
              agentId,
            );
          }
        }
      } else {
        console.warn(
          "[Harness] getAllCachedModels returned unexpected value:",
          typeof allCached,
        );
      }
    } catch (err) {
      console.error("[Harness] Failed to load cached models from DB:", err);
    }
    console.log(
      "[Harness] loadCachedModelsFromDb done. Cache:",
      Object.keys(modelCache),
    );
    ensureExecModelOptions();
  }

  // Refresh models from agent in background (slow, updates cache)
  async function refreshModelsInBackground(agentId) {
    try {
      console.log("[Harness] Background refresh for", agentId);
      const freshModels = await ipcRenderer.invoke(
        "refreshAgentModels",
        agentId,
      );
      const normalized = normalizeModels(freshModels);
      if (normalized.length > 0) {
        // Check if models changed
        const oldModels = modelCache[agentId] || [];
        const changed =
          JSON.stringify(normalized) !== JSON.stringify(oldModels);

        modelCache[agentId] = normalized;
        console.log(
          "[Harness] Background refresh got",
          normalized.length,
          "models for",
          agentId,
          "changed:",
          changed,
        );

        // If this is the active agent and models changed, update UI
        if (changed && activeAgentId === agentId) {
          console.log("[Harness] Updating UI with fresh models for", agentId);
          if (execModelDropdown) {
            setSelectOptions(execModelDropdown, freshModels);
            // Set flag to prevent onChange from saving during restoration
            isRestoringSettings = true;
            restoreAgentModelSelections(agentId);
            // For Codex, load enriched models first, then update and restore reasoning effort
            if (agentId === 'codex') {
              await loadEnrichedModels(agentId);
              updateReasoningEffortDropdown(execModelDropdown.getValue());
              restoreReasoningEffort(agentId);
            }
            isRestoringSettings = false;
          }
        }
      }
    } catch (err) {
      console.warn("[Harness] Background refresh failed for", agentId, err);
    }
  }

  // Preload/refresh models for all agents in background
  function preloadAllModels() {
    const agents = ["codex", "claude-code", "amp", "droid", "opencode", "factory-droid"];
    agents.forEach((agentId, index) => {
      // Stagger refreshes to avoid overwhelming
      setTimeout(() => refreshModelsInBackground(agentId), index * 500);
    });
  }

  async function loadModels(agentId) {
    console.log(
      "[Harness] loadModels called for:",
      agentId,
      "cache has:",
      Object.keys(modelCache),
    );
    // Return cached immediately if available (instant UX)
    if (modelCache[agentId] && modelCache[agentId].length > 0) {
      console.log(
        "[Harness] Using cached models for:",
        agentId,
        "(",
        modelCache[agentId].length,
        "models)",
      );
      // Trigger background refresh for freshness
      setTimeout(() => refreshModelsInBackground(agentId), 0);
      return modelCache[agentId];
    }

    // No cache - must wait for fresh fetch
    console.log(
      "[Harness] No cached models for",
      agentId,
      "- calling refreshAgentModels...",
    );
    try {
      const models = await ipcRenderer.invoke("refreshAgentModels", agentId);
      const normalized = normalizeModels(models);
      console.log(
        "[Harness] refreshAgentModels returned:",
        normalized.length,
        "models",
      );
      if (normalized.length > 0) {
        modelCache[agentId] = normalized;
      }
      return normalized;
    } catch (err) {
      console.error(
        "[Harness] refreshAgentModels failed for",
        agentId,
        ":",
        err,
      );
      return [];
    }
  }

  async function onAgentSelected(agentId, previousAgentId) {
    console.log("[Harness] onAgentSelected:", agentId);
    // Save previous agent's model selections immediately before switching
    const prevAgent = previousAgentId || activeAgentId;
    if (prevAgent && prevAgent !== agentId) {
      await saveTaskSettingsImmediate(prevAgent);
    }
    setPrimaryAgent(agentId);
    hydrateModelsFromCache(agentId);

    // Handle permission mode dropdown
    // Hide for agents that use their own permission mechanisms:
    // - Codex/Claude Code: always bypass mode
    // - Droid: uses --auto high or --skip-permissions-unsafe
    // - Amp: uses --dangerously-allow-all
    // - OpenCode: no CLI permission flags
    const permissionModeGroup = document.getElementById("permissionModeGroup");
    const hidePermissionDropdown = ["codex", "claude-code", "droid", "factory-droid", "amp", "opencode"].includes(agentId);
    if (hidePermissionDropdown) {
      if (permissionModeGroup) {
        permissionModeGroup.style.display = "none";
      }
    } else {
      // Show permission dropdown for other agents
      if (permissionModeGroup) {
        permissionModeGroup.style.display = "";
      }
      // Reset permission dropdown to default
      if (permissionDropdown) {
        permissionDropdown.setValue("default");
      }
    }

    // Reset exec model dropdown to default
    if (execModelDropdown) {
      execModelDropdown.setValue("default");
      if (execModelDropdown.items.length <= 1) {
        setSelectOptions(execModelDropdown, []);
      }
    }

    // Handle agent mode dropdown (visible for Codex and OpenCode)
    const agentModeGroup = document.getElementById("agentModeGroup");
    const agentsWithModes = ["codex", "opencode"];
    if (agentsWithModes.includes(agentId)) {
      // Show the agent mode dropdown
      if (agentModeGroup) {
        agentModeGroup.style.display = "";
      }

      if (agentId === "opencode") {
        // OpenCode has static agent modes: build, plan, general, explore
        const opencodeModes = [
          { value: "build", name: "Build", description: "Default agent with full tool access" },
          { value: "plan", name: "Plan", description: "Planning and analysis without modifications" },
          { value: "general", name: "General", description: "Multipurpose for complex tasks" },
          { value: "explore", name: "Explore", description: "Fast read-only codebase exploration" }
        ];
        if (agentModeDropdown) {
          setModeOptions(agentModeDropdown, opencodeModes);
          console.log("[Harness] Populated agent mode dropdown with OpenCode agents");
        }
      } else {
        // Load modes dynamically for Codex
        const modes = await loadModes(agentId);
        if (agentModeDropdown && modes.length > 0) {
          setModeOptions(agentModeDropdown, modes);
          console.log("[Harness] Populated agent mode dropdown with", modes.length, "modes for", agentId);
        }
      }
    } else {
      // Hide the agent mode dropdown for other agents
      if (agentModeGroup) {
        agentModeGroup.style.display = "none";
      }
      if (agentModeDropdown) {
        agentModeDropdown.setValue("default");
      }
    }

    // Handle reasoning effort dropdown (only visible for Codex)
    const reasoningEffortGroup = document.getElementById("reasoningEffortGroup");
    if (agentId === "codex") {
      // Fetch enriched models (with reasoning effort data) for Codex
      await loadEnrichedModels(agentId);
      // Will update visibility once model is selected
    } else {
      // Hide reasoning effort dropdown for non-Codex agents
      if (reasoningEffortGroup) {
        reasoningEffortGroup.style.display = "none";
      }
      // Reset dropdown without triggering save
      isRestoringSettings = true;
      if (reasoningEffortDropdown) {
        reasoningEffortDropdown.setValue("default");
      }
      isRestoringSettings = false;
    }

    const models = await loadModels(agentId);
    if (activeAgentId !== agentId) {
      console.log("[Harness] Skipping stale model load for", agentId);
      return;
    }
    console.log("[Harness] Got models for UI:", models);
    if (execModelDropdown) {
      console.log(
        "[Harness] Populating dropdown with",
        models.length,
        "models",
      );
      setSelectOptions(execModelDropdown, models);
      console.log("[Harness] execModelDropdown items:", execModelDropdown.items.length);
      // Restore saved settings for this agent (model, permission mode)
      // Set flag to prevent onChange from saving during restoration
      isRestoringSettings = true;
      restoreAgentModelSelections(agentId);
      // Update reasoning effort dropdown based on selected model, THEN restore saved effort
      if (agentId === 'codex') {
        updateReasoningEffortDropdown(execModelDropdown.getValue());
        // Now restore reasoning effort after options are populated
        restoreReasoningEffort(agentId);
      }
      isRestoringSettings = false;
    } else {
      console.warn("[Harness] Could not find execModelDropdown!");
    }
    // Save the agent switch
    saveTaskSettings();
  }

  async function hydrateModelsFromCache(forceAgentId) {
    if (!execModelDropdown) return;
    const agentId =
      forceAgentId ||
      primaryAgentId ||
      activeAgentId ||
      document.querySelector(".agent-card.selected")?.dataset.agentId;
    if (!agentId) return;
    const cached = modelCache[agentId];
    if (cached && cached.length > 0 && execModelDropdown.items.length <= 1) {
      console.log("[Harness] Hydrating execModelDropdown from cache for", agentId);
      setSelectOptions(execModelDropdown, cached);
      // Set flag to prevent onChange from saving during restoration
      isRestoringSettings = true;
      restoreAgentModelSelections(agentId);
      // For Codex, load enriched models first, then populate and restore reasoning effort
      if (agentId === 'codex') {
        await loadEnrichedModels(agentId);
        updateReasoningEffortDropdown(execModelDropdown.getValue());
        restoreReasoningEffort(agentId);
      }
      isRestoringSettings = false;
    }
  }

  window.hydrateModelsFromCache = hydrateModelsFromCache;

  function ensureExecModelOptions() {
    const maxAttempts = 6;
    let attempts = 0;
    const timer = setInterval(() => {
      attempts++;
      hydrateModelsFromCache();
      if (!execModelDropdown || execModelDropdown.items.length > 1 || attempts >= maxAttempts) {
        clearInterval(timer);
      }
    }, 400);
  }

  // Expose for external use (e.g., restoring last agent)
  window.selectAgentById = function (agentId) {
    const card = document.querySelector(
      `.agent-card[data-agent-id="${agentId}"]`,
    );
    // Don't select unavailable or coming-soon agents
    if (card && !card.classList.contains("unavailable") && !card.classList.contains("coming-soon")) {
      const prevActive = activeAgentId;
      selectSingleAgent(agentId);
      onAgentSelected(agentId, prevActive);
    }
  };

  // Expose updateWorktreeLock for external use
  window.updateWorktreeLock = updateWorktreeLock;

  // Initialize worktree toggle state on load
  updateWorktreeLock();

  agentCards.forEach((card) => {
    card.addEventListener("click", async (event) => {
      // Don't allow selection of unavailable or coming-soon agents
      if (card.classList.contains("unavailable") || card.classList.contains("coming-soon")) {
        return;
      }
      const agentId = card.dataset.agentId;
      const prevActive = activeAgentId;

      const isMultiToggle = (event && (event.shiftKey || event.metaKey || event.ctrlKey)) || multiSelectModifierActive;
      if (isMultiToggle) {
        if (selectedAgentIds.has(agentId) && selectedAgentIds.size > 1) {
          selectedAgentIds.delete(agentId);
          if (primaryAgentId === agentId) {
            const remaining = Array.from(selectedAgentIds);
            const nextPrimary = remaining.length > 0 ? remaining[0] : agentId;
            primaryAgentId = nextPrimary;
            markSelected();
            updateWorktreeLock();
            updateMultiAgentUiVisibility();
            if (nextPrimary) {
              await onAgentSelected(nextPrimary, prevActive);
            }
          } else {
            markSelected();
            updateWorktreeLock();
            updateMultiAgentUiVisibility();
          }
          return;
        }
        // Add to selection and set as primary
        selectedAgentIds.add(agentId);
        primaryAgentId = agentId;
        markSelected();
        updateWorktreeLock();
        updateMultiAgentUiVisibility();
        await onAgentSelected(agentId, prevActive);
        return;
      }

      selectSingleAgent(agentId);
      await onAgentSelected(agentId, prevActive);
    });
  });

  // Initial agent selection - wait for settings AND cached models/modes to load first
  window.initAgentSelection = async function () {
    // Load cached models and modes from SQLite first (instant)
    await Promise.all([loadCachedModelsFromDb(), loadCachedModesFromDb()]);
    console.log(
      "[Harness] Model cache populated from SQLite:",
      Object.keys(modelCache),
    );
    console.log(
      "[Harness] Mode cache populated from SQLite:",
      Object.keys(modeCache),
    );

    // ALWAYS ensure settings are fully loaded (fixes race condition with init())
    // Check specifically for taskAgentModels which contains the saved selections
    if (!currentSettings || !currentSettings.taskAgentModels) {
      console.log("[Harness] Settings not ready, fetching...");
      try {
        const settings = await ipcRenderer.invoke("getSettings");
        if (settings) {
          currentSettings = settings;
          console.log("[Harness] Settings loaded:", Object.keys(currentSettings));
        }
      } catch (e) {
        console.warn("[Harness] Failed to load settings:", e);
      }
    }

    const lastAgent = currentSettings && currentSettings.taskLastAgent;
    const cachedFallback =
      Object.keys(modelCache).length > 0 ? Object.keys(modelCache)[0] : null;
    const initialAgentId =
      lastAgent ||
      document.querySelector(".agent-card.selected")?.dataset.agentId ||
      cachedFallback;
    // Track whether we did a full restoration from cache (to avoid double-init)
    let didCacheRestore = false;

    if (initialAgentId && execModelDropdown && modelCache[initialAgentId]) {
      console.log("[Harness] Restoring from cache for agent:", initialAgentId);
      selectSingleAgent(initialAgentId);
      setSelectOptions(execModelDropdown, modelCache[initialAgentId]);
      // Set flag to prevent onChange from saving during restoration
      isRestoringSettings = true;
      restoreAgentModelSelections(initialAgentId);
      // For Codex, load enriched models first, then populate and restore reasoning effort
      if (initialAgentId === 'codex') {
        await loadEnrichedModels(initialAgentId);
        updateReasoningEffortDropdown(execModelDropdown.getValue());
        restoreReasoningEffort(initialAgentId);
        // Also restore agent mode for Codex
        const modes = await loadModes('codex');
        if (agentModeDropdown && modes.length > 0) {
          setModeOptions(agentModeDropdown, modes);
          // Restore saved agent mode
          const prefs = (currentSettings.taskAgentModels || {})['codex'];
          if (prefs && prefs.agentMode) {
            const modeOptions = agentModeDropdown.items.map(item => item.value);
            if (modeOptions.includes(prefs.agentMode)) {
              agentModeDropdown.setValue(prefs.agentMode);
            }
          }
        }
      }
      isRestoringSettings = false;
      didCacheRestore = true;
    }

    const defaultCard = lastAgent
      ? document.querySelector(`.agent-card[data-agent-id="${lastAgent}"]`)
      : document.querySelector(".agent-card.selected") || agentCards[0];

    // Only call onAgentSelected if we didn't already restore from cache
    // OR if the card is different from what we restored (rare edge case)
    if (defaultCard && (!didCacheRestore || defaultCard.dataset.agentId !== initialAgentId)) {
      await onAgentSelected(defaultCard.dataset.agentId);
    } else if (didCacheRestore) {
      // We restored from cache, just ensure UI visibility is correct
      console.log("[Harness] Skipping onAgentSelected (already restored from cache)");
      // Still need to handle agent-specific UI visibility
      const permissionModeGroup = document.getElementById("permissionModeGroup");
      const agentModeGroup = document.getElementById("agentModeGroup");
      const reasoningEffortGroup = document.getElementById("reasoningEffortGroup");

      // Hide permission dropdown for agents that use their own mechanisms
      const hidePermissionDropdown = ["codex", "claude-code", "droid", "factory-droid", "amp", "opencode"].includes(initialAgentId);
      const showAgentModeDropdown = ["codex", "opencode"].includes(initialAgentId);
      if (hidePermissionDropdown) {
        if (permissionModeGroup) permissionModeGroup.style.display = "none";
        // Agent mode visible for Codex and OpenCode
        if (agentModeGroup) agentModeGroup.style.display = showAgentModeDropdown ? "" : "none";
        // reasoningEffortGroup visibility handled by updateReasoningEffortDropdown
      } else {
        if (permissionModeGroup) permissionModeGroup.style.display = "";
        if (agentModeGroup) agentModeGroup.style.display = "none";
        if (reasoningEffortGroup) reasoningEffortGroup.style.display = "none";
      }
    }
    hydrateModelsFromCache();
    ensureExecModelOptions();
  };

  // Trigger initial selection (will wait for settings and cached models)
  window.initAgentSelection();

  // Refresh models for all agents in background (staggered to avoid overwhelming)
  setTimeout(preloadAllModels, 500);

  $("#planModeToggle").on("change", saveTaskSettings);

  // Save settings when toggles change
  $("#useWorktreeToggle").on("change", saveTaskSettings);

  // Note: Permission mode, model, and agent mode dropdown changes are handled
  // by the onChange callbacks in initCustomDropdowns()

  // Save settings when project path changes
  $("#projectPath").on("change", () => {
    saveTaskSettings();
    refreshBaseBranchOptions();
    addRecentProjectPath(getProjectPath());
    renderProjectAllowlist();
  });

  $("#pickProjectPath").on("click", async () => {
    try {
      const picked = await ipcRenderer.invoke("pickProjectPath");
      if (picked) {
        setProjectPath(picked);
        saveTaskSettings();
        refreshBaseBranchOptions();
        addRecentProjectPath(picked);
        renderProjectAllowlist();
      }
    } catch (err) {
      console.log("[Harness] Project picker unavailable");
    }
  });

  $("#createAgentButton").on("click", async () => {
    clearCreateTaskError();
    // Get pending attachments if any
    const attachments = window.getPendingAttachments
      ? window.getPendingAttachments()
      : [];

    // Get text from contenteditable div (excludes inline images)
    const promptText = window.getPromptText ? window.getPromptText() : $("#initialPrompt").text();

    const selectedAgents = getSelectedAgents();
    const primaryAgent = primaryAgentId || activeAgentId || selectedAgents[0] || "codex";
    lastCreateAgentId = primaryAgent;
    const multiCreate = selectedAgents.length > 1;

    // Ensure current primary agent selections are saved before using prefs
    if (primaryAgent) {
      await saveTaskSettingsImmediate(primaryAgent);
    }

    const planMode = $("#planModeToggle").is(":checked");
    const baseBranch = baseBranchDropdown && baseBranchDropdown.getValue() !== "default"
      ? baseBranchDropdown.getValue()
      : null;
    const forceWorktree = multiCreate ? true : $("#useWorktreeToggle").is(":checked");

    // Agents with their own permission mechanisms always use bypass:
    // - Codex/Claude Code: always bypass
    // - Droid: CLI uses --auto high or --skip-permissions-unsafe
    // - Amp: CLI uses --dangerously-allow-all
    // - OpenCode: CLI handles permissions internally
    const agentsWithOwnPermissions = ["codex", "claude-code", "droid", "factory-droid", "amp", "opencode"];

    const agentModels = (currentSettings && currentSettings.taskAgentModels) || {};

    selectedAgents.forEach((agentId) => {
      const prefs = agentModels[agentId] || {};
      const execModel = prefs.execModel || "default";
      const reasoningEffort = agentId === "codex" ? (prefs.reasoningEffort || "default") : null;
      const agentMode = agentId === "opencode" ? (prefs.agentMode || "build") : null;
      const codexMode = agentId === "codex"
        ? (planMode ? "plan" : (prefs.agentMode || "default"))
        : null;
      const permissionMode = agentsWithOwnPermissions.includes(agentId)
        ? "bypassPermissions"
        : (prefs.permissionMode || "default");

      const payload = {
        agentId: agentId,
        prompt: promptText,
        projectPath: getProjectPath(),
        baseBranch: baseBranch,
        planMode: planMode,
        thinking: true,
        useWorktree: forceWorktree,
        permissionMode: permissionMode,
        execModel: execModel,
        reasoningEffort: reasoningEffort !== "default" ? reasoningEffort : null,
        agentMode: agentMode,
        codexMode: codexMode !== "default" ? codexMode : null,
        attachments: attachments.map((a) => ({
          id: a.id,
          relativePath: a.relativePath,
          mimeType: a.mimeType,
        })),
        multiCreate: multiCreate
      };
      ipcRenderer.send("CreateAgentSession", payload);
      console.log("[Harness] CreateAgentSession", payload);
    });

    // Clear prompt and attachments after task creation
    const promptEl = document.getElementById("initialPrompt");
    if (promptEl) {
      promptEl.innerHTML = "";
      if (window.updatePromptPlaceholder) {
        window.updatePromptPlaceholder();
      }
    }
    if (window.clearPendingAttachments) {
      window.clearPendingAttachments();
    }
  });

  $("#fixWithAgentButton").on("click", () => {
    createFixTaskFromWarning();
  });

  // Cmd+Enter in prompt contenteditable triggers create task
  $("#initialPrompt").on("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      $("#createAgentButton").click();
    }
  });

  // Initialize slash command autocomplete for the prompt contenteditable
  var promptTextarea = document.getElementById("initialPrompt");
  if (promptTextarea && window.SlashCommandAutocomplete) {
    window.promptSlashCommands = new SlashCommandAutocomplete(
      promptTextarea,
      activeAgentId || "codex",
    );
    console.log("[Harness] Slash command autocomplete initialized");
  }

  // Auto-focus the prompt textarea on initial page load (Notion-style UX)
  // Navigation focus is handled in gui.js
  requestAnimationFrame(() => {
    const prompt = document.getElementById("initialPrompt");
    if (prompt) {
      prompt.focus();
    }
  });

  // Listen for dynamic slash commands from ACP
  if (ipcRenderer) {
    ipcRenderer.on("AvailableCommands", function (e, taskId, commands) {
      console.log(
        "[Harness] Received",
        commands.length,
        "available commands from ACP",
      );
      // Update the slash commands for the current agent
      if (window.promptSlashCommands && Array.isArray(commands)) {
        window.promptSlashCommands.updateCommands(commands);
      }
    });
  }

  // ============================================================================
  // Image Attachment Handling (Inline Images at Cursor Position)
  // ============================================================================

  // Store pending attachments for the current task creation
  window.pendingAttachments = [];

  // Generate a temporary task ID for attachments before actual task creation
  function getTempTaskId() {
    if (!window.tempTaskId) {
      window.tempTaskId = "temp-" + Date.now() + "-" + Math.random().toString(36).substr(2, 9);
    }
    return window.tempTaskId;
  }

  // Reset temp task ID after task creation
  function resetTempTaskId() {
    window.tempTaskId = null;
    window.pendingAttachments = [];
  }

  function updatePromptPlaceholder() {
    const promptEl = document.getElementById("initialPrompt");
    if (!promptEl) return;
    const text = promptEl.innerText || promptEl.textContent || "";
    const normalized = text
      .replace(/\u200B/g, "")
      .replace(/\u00A0/g, " ")
      .trim();
    const hasText = normalized.length > 0;
    const hasImages = !!promptEl.querySelector(
      ".inline-image-wrapper, img.inline-image",
    );
    promptEl.classList.toggle("is-empty", !hasText && !hasImages);
  }

  window.updatePromptPlaceholder = updatePromptPlaceholder;

  // Save current selection/cursor position in the contenteditable
  let savedSelection = null;
  function saveSelection() {
    const sel = window.getSelection();
    if (sel.rangeCount > 0) {
      savedSelection = sel.getRangeAt(0).cloneRange();
    }
  }

  // Restore saved selection/cursor position
  function restoreSelection() {
    if (savedSelection) {
      const sel = window.getSelection();
      sel.removeAllRanges();
      sel.addRange(savedSelection);
    }
  }

  // Insert an element at the current cursor position in contenteditable
  function insertAtCursor(element) {
    const promptEl = document.getElementById("initialPrompt");
    if (!promptEl) return;

    // Focus the element first
    promptEl.focus();

    // Restore saved selection if available
    restoreSelection();

    const sel = window.getSelection();
    if (sel.rangeCount > 0) {
      const range = sel.getRangeAt(0);
      range.deleteContents();
      range.insertNode(element);
      // Move cursor after the inserted element
      range.setStartAfter(element);
      range.setEndAfter(element);
      sel.removeAllRanges();
      sel.addRange(range);
    } else {
      // Fallback: append to end
      promptEl.appendChild(element);
    }

    // Clear saved selection
    savedSelection = null;

    updatePromptPlaceholder();
  }

  // Insert an inline image at cursor position
  function insertInlineImage(attachment, dataUrl) {
    const wrapper = document.createElement("span");
    wrapper.className = "inline-image-wrapper";
    wrapper.dataset.attachmentId = attachment.id;
    wrapper.contentEditable = "false"; // Prevent editing the wrapper

    const img = document.createElement("img");
    img.className = "inline-image";
    img.alt = attachment.fileName || "attachment";
    img.title = attachment.fileName || "Attached image";

    // Handle image load success
    img.onload = () => {
      console.log("[Harness] Inline image loaded successfully:", attachment.id, {
        naturalWidth: img.naturalWidth,
        naturalHeight: img.naturalHeight
      });
    };

    // Handle image load error
    img.onerror = (e) => {
      console.error("[Harness] Failed to load inline image:", attachment.id, e);
      img.style.background = "rgba(255, 100, 100, 0.3)";
      img.alt = "Failed to load";
    };

    // Log for debugging before setting src
    console.log("[Harness] Inserting inline image:", {
      id: attachment.id,
      dataUrlLength: dataUrl ? dataUrl.length : 0,
      dataUrlStart: dataUrl ? dataUrl.substring(0, 50) : "empty"
    });

    // Set src after setting up handlers
    img.src = dataUrl;

    // Use span instead of button to avoid Bootstrap/browser button styling issues
    const removeBtn = document.createElement("span");
    removeBtn.className = "remove-inline-image";
    // Empty - we'll use CSS ::before for the X to avoid font issues
    removeBtn.title = "Remove image";
    removeBtn.setAttribute("role", "button");
    removeBtn.setAttribute("tabindex", "0");
    // Use mousedown instead of click - fires before contenteditable can steal focus
    removeBtn.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();
      console.log("[Harness] Remove button clicked for:", attachment.id);
      removeInlineAttachment(attachment.id);
    }, { capture: true });

    wrapper.appendChild(img);
    wrapper.appendChild(removeBtn);

    insertAtCursor(wrapper);
  }

  // Remove inline attachment
  async function removeInlineAttachment(attachmentId) {
    const taskId = getTempTaskId();
    try {
      await ipcRenderer.invoke("delete_attachment", attachmentId, taskId);
      window.pendingAttachments = window.pendingAttachments.filter(
        (a) => a.id !== attachmentId
      );
      // Remove from contenteditable
      const wrapper = document.querySelector(
        `.inline-image-wrapper[data-attachment-id="${attachmentId}"]`
      );
      if (wrapper) wrapper.remove();
      updatePromptPlaceholder();
      console.log("[Harness] Removed inline attachment:", attachmentId);
    } catch (err) {
      console.error("[Harness] Failed to remove attachment:", err);
    }
  }

  // Process and upload a file, then insert inline at cursor
  async function processFile(file) {
    // Validate file type
    const validTypes = ["image/jpeg", "image/png", "image/gif", "image/webp"];
    if (!validTypes.includes(file.type)) {
      console.warn("[Harness] Invalid file type:", file.type);
      return;
    }

    // Validate file size (max 5MB)
    if (file.size > 5 * 1024 * 1024) {
      alert("Image exceeds 5MB size limit. Please use a smaller image.");
      return;
    }

    // Read file as base64 data URL for both storage and display
    // Note: We use data URLs instead of blob URLs because Tauri's WebView
    // with useHttpsScheme can have origin issues with blob URLs
    const reader = new FileReader();
    reader.onload = async (e) => {
      const dataUrl = e.target.result;
      const base64 = dataUrl.split(",")[1]; // Remove data:image/xxx;base64, prefix

      try {
        const attachment = await ipcRenderer.invoke("save_attachment", {
          taskId: getTempTaskId(),
          fileName: file.name,
          mimeType: file.type,
          data: base64,
        });

        // Store the data URL with the attachment for display
        attachment.dataUrl = dataUrl;
        window.pendingAttachments.push(attachment);
        // Use data URL for display (works reliably in Tauri WebView)
        insertInlineImage(attachment, dataUrl);
        console.log("[Harness] Uploaded inline attachment:", attachment.id);
      } catch (err) {
        console.error("[Harness] Failed to upload attachment:", err);
        alert("Failed to upload image: " + err);
      }
    };
    reader.readAsDataURL(file);
  }

  // File input change handler
  const attachmentInput = document.getElementById("attachmentInput");
  if (attachmentInput) {
    attachmentInput.addEventListener("change", (e) => {
      const files = e.target.files;
      for (const file of files) {
        processFile(file);
      }
      // Reset input so the same file can be selected again
      e.target.value = "";
    });
  }

  // Attach button click handler - save selection before opening file picker
  const attachImageBtn = document.getElementById("attachImageBtn");
  if (attachImageBtn) {
    attachImageBtn.addEventListener("mousedown", (e) => {
      // Save selection before the button click steals focus
      saveSelection();
    });
    attachImageBtn.addEventListener("click", () => {
      attachmentInput?.click();
    });
  }

  // Paste handler for images - contenteditable version
  const promptEl = document.getElementById("initialPrompt");
  if (promptEl) {
    updatePromptPlaceholder();
    promptEl.addEventListener("input", updatePromptPlaceholder);
    promptEl.addEventListener("blur", updatePromptPlaceholder);
    promptEl.addEventListener("focus", updatePromptPlaceholder);

    promptEl.addEventListener("paste", (e) => {
      const items = e.clipboardData?.items;
      if (!items) return;

      for (const item of items) {
        if (item.type.startsWith("image/")) {
          e.preventDefault();
          // Save selection before async processing
          saveSelection();
          const file = item.getAsFile();
          if (file) processFile(file);
          return; // Only process one image per paste
        }
      }
    });

    // Handle Enter key to create proper line breaks
    promptEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
        // Allow default behavior for plain Enter in contenteditable
        // This creates a new line naturally
      }
    });

    // Drag and drop handlers
    promptEl.addEventListener("dragover", (e) => {
      e.preventDefault();
      e.stopPropagation();
      promptEl.classList.add("drag-over");
    });

    promptEl.addEventListener("dragleave", (e) => {
      e.preventDefault();
      e.stopPropagation();
      promptEl.classList.remove("drag-over");
    });

    promptEl.addEventListener("drop", (e) => {
      e.preventDefault();
      e.stopPropagation();
      promptEl.classList.remove("drag-over");

      // Save the drop position
      if (document.caretRangeFromPoint) {
        const range = document.caretRangeFromPoint(e.clientX, e.clientY);
        if (range) {
          const sel = window.getSelection();
          sel.removeAllRanges();
          sel.addRange(range);
          saveSelection();
        }
      }

      const files = e.dataTransfer?.files;
      if (files) {
        for (const file of files) {
          if (file.type.startsWith("image/")) {
            processFile(file);
          }
        }
      }
    });
  }

  // Get text content from contenteditable (excludes inline images)
  window.getPromptText = () => {
    const el = document.getElementById("initialPrompt");
    if (!el) return "";
    // Get text content, which excludes images
    return el.innerText || el.textContent || "";
  };

  // Expose for use in task creation
  window.getPendingAttachments = () => window.pendingAttachments;
  window.clearPendingAttachments = () => {
    const promptEl = document.getElementById("initialPrompt");
    if (promptEl) {
      // Remove inline images
      const inlineImages = promptEl.querySelectorAll(".inline-image-wrapper");
      inlineImages.forEach((img) => img.remove());
    }
    resetTempTaskId();
    updatePromptPlaceholder();
  };
})();

/* ============================================
   Analytics Dashboard Module
   ============================================ */
(function () {
  "use strict";

  const ipcRenderer = window.tauriBridge.ipcRenderer;

  // Format large token numbers (1,445,985,730 → "1.4b", 683,000,000 → "683m")
  function formatTokens(n) {
    if (n == null || isNaN(n)) return "--";
    if (n >= 1_000_000_000) {
      return (n / 1_000_000_000).toFixed(1).replace(/\.0$/, "") + "b";
    }
    if (n >= 1_000_000) {
      return (n / 1_000_000).toFixed(0) + "m";
    }
    if (n >= 1_000) {
      return (n / 1_000).toFixed(0) + "k";
    }
    return n.toString();
  }

  // Format day label from YYYY-MM-DD
  function formatDayLabel(dayKey) {
    if (!dayKey) return "--";
    const parts = dayKey.split("-");
    if (parts.length !== 3) return dayKey;
    const months = [
      "Jan",
      "Feb",
      "Mar",
      "Apr",
      "May",
      "Jun",
      "Jul",
      "Aug",
      "Sep",
      "Oct",
      "Nov",
      "Dec",
    ];
    const month = months[parseInt(parts[1], 10) - 1] || parts[1];
    const day = parseInt(parts[2], 10);
    return month + " " + day;
  }

  let codexSnapshot = null;

  function getAnalyticsRangeDays() {
    const days = window.analyticsRangeDays;
    return Number.isFinite(days) && days > 0 ? days : 7;
  }

  function computeCodexRangeStats(snapshot, rangeDays) {
    const days = (snapshot?.days || []).slice(-rangeDays);
    let totalTokens = 0;
    let outputTokens = 0;
    let inputTokens = 0;
    let cachedTokens = 0;
    let totalCost = 0;
    let peakDay = null;
    let peakTokens = 0;

    days.forEach((day) => {
      const dayTotal = day.totalTokens || 0;
      totalTokens += dayTotal;
      outputTokens += day.outputTokens || 0;
      inputTokens += day.inputTokens || 0;
      cachedTokens += day.cachedInputTokens || 0;
      totalCost += day.totalCost || 0;
      if (dayTotal > peakTokens) {
        peakTokens = dayTotal;
        peakDay = day.day;
      }
    });

    const averageDailyTokens = days.length
      ? Math.round(totalTokens / days.length)
      : 0;
    const cacheHitRatePercent = inputTokens > 0
      ? (cachedTokens / inputTokens) * 100
      : 0;

    return {
      totalTokens,
      outputTokens,
      averageDailyTokens,
      cacheHitRatePercent,
      totalCost,
      peakDay,
      peakTokens,
    };
  }

  // Render summary stat cards
  function renderSummaryCards(snapshot) {
    const grid = document.getElementById("codexSummaryGrid");
    if (!grid || !snapshot) return;

    const rangeDays = getAnalyticsRangeDays();
    const stats = computeCodexRangeStats(snapshot, rangeDays);

    // Total tokens (range)
    const last7El = grid.querySelector('[data-stat="last7"]');
    if (last7El) last7El.textContent = formatTokens(stats.totalTokens);

    // Estimated cost (range)
    const costEl = grid.querySelector('[data-stat="totalCost"]');
    if (costEl) costEl.textContent = formatCost(stats.totalCost);

    // Output tokens (range)
    const last30El = grid.querySelector('[data-stat="last30"]');
    if (last30El) last30El.textContent = formatTokens(stats.outputTokens);

    // Cache hit rate
    const cacheEl = grid.querySelector('[data-stat="cacheRate"]');
    if (cacheEl) cacheEl.textContent = stats.cacheHitRatePercent.toFixed(1) + "%";

    // Peak day
    const peakTokensEl = grid.querySelector('[data-stat="peakTokens"]');
    if (peakTokensEl)
      peakTokensEl.textContent = formatTokens(stats.peakTokens);

    const peakDateEl = grid.closest(".phantom-agent-card")?.querySelector('[data-stat="peakDate"]');
    if (peakDateEl) {
      peakDateEl.textContent = stats.peakDay
        ? "Peak: " + formatDayLabel(stats.peakDay)
        : "Peak: --";
    }
  }

  // Render bar chart for last 7 days
  function renderBarChart(days) {
    const chartEl = document.getElementById("codexBarChart");
    if (!chartEl || !days) return;

    // Get last 7 days
    const last7 = days.slice(-7);
    if (last7.length === 0) return;

    // Find max for scaling
    const maxTokens = Math.max(...last7.map((d) => d.totalTokens || 0), 1);

    const containers = chartEl.querySelectorAll(".bar-container");
    containers.forEach((container, idx) => {
      const bar = container.querySelector(".bar");
      const label = container.querySelector(".bar-label");
      const dayData = last7[idx];

      if (!dayData) {
        if (bar) bar.style.setProperty("--bar-height", "0%");
        if (label) label.textContent = "--";
        return;
      }

      const heightPct = Math.max((dayData.totalTokens / maxTokens) * 100, 2);
      if (bar) {
        bar.style.setProperty("--bar-height", heightPct + "%");
        bar.setAttribute("data-tokens", formatTokens(dayData.totalTokens));
      }

      if (label) {
        // Check if today
        const today = new Date().toISOString().split("T")[0];
        if (dayData.day === today) {
          label.textContent = "Today";
          container.classList.add("today");
        } else {
          label.textContent = formatDayLabel(dayData.day);
          container.classList.remove("today");
        }
      }
    });
  }

  // Render model breakdown
  function renderModelBreakdown(topModels) {
    const container = document.getElementById("codexModelBreakdown");
    if (!container) return;

    if (!topModels || topModels.length === 0) {
      container.innerHTML = `
        <div class="breakdown-item">
          <div class="breakdown-header">
            <span class="breakdown-model">No data</span>
            <span class="breakdown-percent">--</span>
          </div>
          <div class="breakdown-bar">
            <div class="breakdown-fill" style="width: 0%"></div>
          </div>
        </div>
      `;
      return;
    }

    // Limit to top 4
    const models = topModels.slice(0, 4);
    const maxShare = Math.max(...models.map((m) => m.sharePercent || 0), 1);

    container.innerHTML = models
      .map((model, idx) => {
        const barWidth = (model.sharePercent / maxShare) * 100;
        return `
        <div class="breakdown-item">
          <div class="breakdown-header">
            <span class="breakdown-model">${escapeHtml(model.model)}</span>
            <span class="breakdown-percent">${(model.sharePercent || 0).toFixed(1)}%</span>
          </div>
          <div class="breakdown-bar">
            <div class="breakdown-fill" style="width: ${barWidth}%"></div>
          </div>
        </div>
      `;
      })
      .join("");
  }

  function escapeHtml(str) {
    if (!str) return "";
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  // Render snapshot data (used by both cached and fresh data)
  function renderCodexSnapshot(snapshot) {
    if (!snapshot) return;
    codexSnapshot = snapshot;
    renderSummaryCards(snapshot);
    renderBarChart(snapshot.days);
    renderModelBreakdown(snapshot.topModels);

    // Dispatch event for comparison module
    window.dispatchEvent(new CustomEvent("codexSnapshotUpdated", { detail: snapshot }));
  }

  // Fetch and render all Codex analytics (with caching)
  async function fetchCodexAnalytics(skipCache = false) {
    console.log("[Analytics] Fetching Codex analytics...", skipCache ? "(force refresh)" : "");
    try {
      const snapshot = await ipcRenderer.invoke("localUsageSnapshot", 30);
      console.log("[Analytics] Snapshot received:", snapshot);

      if (!snapshot) {
        console.log("[Analytics] No snapshot data");
        return;
      }

      renderCodexSnapshot(snapshot);

      // Save to cache for instant loading next time
      try {
        await ipcRenderer.invoke("saveAnalyticsCache", "codex", snapshot);
        console.log("[Analytics] Saved to cache");
      } catch (cacheErr) {
        console.warn("[Analytics] Cache save failed:", cacheErr);
      }

      console.log("[Analytics] Render complete");
    } catch (err) {
      console.error("[Analytics] Fetch failed:", err);
    }
  }

  // Load cached analytics on startup (instant)
  async function loadCachedCodexAnalytics() {
    try {
      const cached = await ipcRenderer.invoke("getCachedAnalytics", "codex");
      if (cached) {
        console.log("[Analytics] Loading cached Codex analytics");
        renderCodexSnapshot(cached);
        return true;
      }
    } catch (err) {
      console.warn("[Analytics] Cache load failed:", err);
    }
    return false;
  }

  // Refresh button handler
  const refreshBtn = document.getElementById("refreshCodexAnalytics");
  if (refreshBtn) {
    refreshBtn.addEventListener("click", () => {
      refreshBtn.disabled = true;
      refreshBtn.querySelector("i")?.classList.add("fa-spin");

      fetchCodexAnalytics(true).finally(() => {
        setTimeout(() => {
          refreshBtn.disabled = false;
          refreshBtn.querySelector("i")?.classList.remove("fa-spin");
        }, 500);
      });
    });
  }

  // Auto-refresh interval (5 minutes)
  const ANALYTICS_REFRESH_INTERVAL = 5 * 60 * 1000;
  let analyticsRefreshInterval = null;

  function startAnalyticsRefresh() {
    if (analyticsRefreshInterval) return;
    // First load cached, then fetch fresh in background
    loadCachedCodexAnalytics().then(() => {
      fetchCodexAnalytics();
    });
    analyticsRefreshInterval = setInterval(fetchCodexAnalytics, ANALYTICS_REFRESH_INTERVAL);
  }

  function stopAnalyticsRefresh() {
    if (analyticsRefreshInterval) {
      clearInterval(analyticsRefreshInterval);
      analyticsRefreshInterval = null;
    }
  }

  // Observe page visibility
  const analyticsPage = document.getElementById("analyticsPage");
  if (analyticsPage) {
    const observer = new MutationObserver(() => {
      if (!analyticsPage.hidden) {
        startAnalyticsRefresh();
      } else {
        stopAnalyticsRefresh();
      }
    });
    observer.observe(analyticsPage, {
      attributes: true,
      attributeFilter: ["hidden"],
    });

    // Initial check
    if (!analyticsPage.hidden) {
      startAnalyticsRefresh();
    }
  }

  // Load cached analytics on app startup (non-blocking)
  loadCachedCodexAnalytics();

  // Expose for external use
  window.fetchCodexAnalytics = fetchCodexAnalytics;
  window.loadCachedCodexAnalytics = loadCachedCodexAnalytics;

  window.addEventListener("analyticsRangeChanged", () => {
    if (codexSnapshot) {
      renderSummaryCards(codexSnapshot);
    }
  });

  console.log("[Analytics] Module initialized");
})();

/* ============================================
   Claude Code Analytics Dashboard Module
   ============================================ */
(function () {
  "use strict";

  const ipcRenderer = window.tauriBridge.ipcRenderer;

  // Format large token numbers (1,445,985,730 → "1.4b", 683,000,000 → "683m")
  function formatTokens(n) {
    if (n == null || isNaN(n)) return "--";
    if (n >= 1_000_000_000) {
      return (n / 1_000_000_000).toFixed(1).replace(/\.0$/, "") + "b";
    }
    if (n >= 1_000_000) {
      return (n / 1_000_000).toFixed(0) + "m";
    }
    if (n >= 1_000) {
      return (n / 1_000).toFixed(0) + "k";
    }
    return n.toString();
  }

  // Format cost as USD
  function formatCost(cost) {
    if (cost == null || isNaN(cost)) return "$--";
    if (cost >= 1000) {
      return "$" + (cost / 1000).toFixed(1) + "k";
    }
    if (cost >= 1) {
      return "$" + cost.toFixed(2);
    }
    return "$" + cost.toFixed(4);
  }

  // Format day label from YYYY-MM-DD
  function formatDayLabel(dayKey) {
    if (!dayKey) return "--";
    const parts = dayKey.split("-");
    if (parts.length !== 3) return dayKey;
    const months = [
      "Jan",
      "Feb",
      "Mar",
      "Apr",
      "May",
      "Jun",
      "Jul",
      "Aug",
      "Sep",
      "Oct",
      "Nov",
      "Dec",
    ];
    const month = months[parseInt(parts[1], 10) - 1] || parts[1];
    const day = parseInt(parts[2], 10);
    return month + " " + day;
  }

  let claudeSnapshot = null;

  function getAnalyticsRangeDays() {
    const days = window.analyticsRangeDays;
    return Number.isFinite(days) && days > 0 ? days : 7;
  }

  function computeClaudeRangeStats(snapshot, rangeDays) {
    const days = (snapshot?.days || []).slice(-rangeDays);
    let totalTokens = 0;
    let outputTokens = 0;
    let inputTokens = 0;
    let cacheReadTokens = 0;
    let cacheCreationTokens = 0;
    let totalCost = 0;
    let peakDay = null;
    let peakTokens = 0;

    days.forEach((day) => {
      const dayTotal = day.totalTokens || 0;
      totalTokens += dayTotal;
      outputTokens += day.outputTokens || 0;
      inputTokens += day.inputTokens || 0;
      cacheReadTokens += day.cacheReadTokens || 0;
      cacheCreationTokens += day.cacheCreationTokens || 0;
      totalCost += day.totalCost || 0;
      if (dayTotal > peakTokens) {
        peakTokens = dayTotal;
        peakDay = day.day;
      }
    });

    const averageDailyTokens = days.length
      ? Math.round(totalTokens / days.length)
      : 0;
    const totalInputAttempted = inputTokens + cacheReadTokens;
    const cacheHitRatePercent = totalInputAttempted > 0
      ? (cacheReadTokens / totalInputAttempted) * 100
      : 0;

    return {
      totalTokens,
      outputTokens,
      averageDailyTokens,
      cacheHitRatePercent,
      totalCost,
      peakDay,
      peakTokens,
      cacheReadTokens,
      cacheCreationTokens,
    };
  }

  // Render Claude summary stat cards
  function renderClaudeSummaryCards(snapshot) {
    const grid = document.getElementById("claudeSummaryGrid");
    if (!grid || !snapshot) return;

    const rangeDays = getAnalyticsRangeDays();
    const stats = computeClaudeRangeStats(snapshot, rangeDays);

    // Total tokens (range)
    const last7El = grid.querySelector('[data-stat="last7"]');
    if (last7El) last7El.textContent = formatTokens(stats.totalTokens);

    // Average daily
    const avgEl = grid.querySelector('[data-stat="avgDaily"]');
    if (avgEl) avgEl.textContent = formatTokens(stats.averageDailyTokens);

    // Output tokens (range)
    const last30El = grid.querySelector('[data-stat="last30"]');
    if (last30El) last30El.textContent = formatTokens(stats.outputTokens);

    // Cache hit rate
    const cacheEl = grid.querySelector('[data-stat="cacheRate"]');
    if (cacheEl) cacheEl.textContent = stats.cacheHitRatePercent.toFixed(1) + "%";

    // Total cost (range)
    const costEl = grid.querySelector('[data-stat="totalCost"]');
    if (costEl) costEl.textContent = formatCost(stats.totalCost);

    // Peak date
    const peakDateEl = grid.closest(".phantom-agent-card")?.querySelector('[data-stat="peakDate"]');
    if (peakDateEl) {
      peakDateEl.textContent = stats.peakDay
        ? "Peak: " + formatDayLabel(stats.peakDay)
        : "Peak: --";
    }
  }

  // Render Claude bar chart for last 7 days
  function renderClaudeBarChart(days) {
    const chartEl = document.getElementById("claudeBarChart");
    if (!chartEl || !days) return;

    // Get last 7 days
    const last7 = days.slice(-7);
    if (last7.length === 0) return;

    // Find max for scaling
    const maxTokens = Math.max(...last7.map((d) => d.totalTokens || 0), 1);

    const containers = chartEl.querySelectorAll(".bar-container");
    containers.forEach((container, idx) => {
      const bar = container.querySelector(".bar");
      const label = container.querySelector(".bar-label");
      const dayData = last7[idx];

      if (!dayData) {
        if (bar) bar.style.setProperty("--bar-height", "0%");
        if (label) label.textContent = "--";
        return;
      }

      const heightPct = Math.max((dayData.totalTokens / maxTokens) * 100, 2);
      if (bar) {
        bar.style.setProperty("--bar-height", heightPct + "%");
        bar.setAttribute("data-tokens", formatTokens(dayData.totalTokens));
      }

      if (label) {
        // Check if today
        const today = new Date().toISOString().split("T")[0];
        if (dayData.day === today) {
          label.textContent = "Today";
          container.classList.add("today");
        } else {
          label.textContent = formatDayLabel(dayData.day);
          container.classList.remove("today");
        }
      }
    });
  }

  // Render Claude model breakdown
  function renderClaudeModelBreakdown(topModels) {
    const container = document.getElementById("claudeModelBreakdown");
    if (!container) return;

    if (!topModels || topModels.length === 0) {
      container.innerHTML = `
        <div class="breakdown-item">
          <div class="breakdown-header">
            <span class="breakdown-model">No data</span>
            <span class="breakdown-percent">--</span>
          </div>
          <div class="breakdown-bar">
            <div class="breakdown-fill claude" style="width: 0%"></div>
          </div>
        </div>
      `;
      return;
    }

    // Limit to top 4
    const models = topModels.slice(0, 4);
    const maxShare = Math.max(...models.map((m) => m.sharePercent || 0), 1);

    container.innerHTML = models
      .map((model, idx) => {
        const barWidth = (model.sharePercent / maxShare) * 100;
        // Format model name to be more readable
        const displayName = model.model
          .replace(/^claude-/i, "")
          .replace(/-\d{8}$/, "")
          .replace(/-/g, " ")
          .replace(/\b(\w)/g, (c) => c.toUpperCase());
        return `
        <div class="breakdown-item">
          <div class="breakdown-header">
            <span class="breakdown-model">${escapeHtml(displayName)}</span>
            <span class="breakdown-percent">${(model.sharePercent || 0).toFixed(1)}%</span>
          </div>
          <div class="breakdown-bar">
            <div class="breakdown-fill claude" style="width: ${barWidth}%"></div>
          </div>
        </div>
      `;
      })
      .join("");
  }

  function escapeHtml(str) {
    if (!str) return "";
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  // Render snapshot data (used by both cached and fresh data)
  function renderClaudeSnapshot(snapshot) {
    if (!snapshot) return;
    claudeSnapshot = snapshot;
    renderClaudeSummaryCards(snapshot);
    renderClaudeBarChart(snapshot.days);
    renderClaudeModelBreakdown(snapshot.topModels);

    // Dispatch event for comparison module
    window.dispatchEvent(new CustomEvent("claudeSnapshotUpdated", { detail: snapshot }));
  }

  // Fetch and render all Claude Code analytics (with caching)
  async function fetchClaudeAnalytics(skipCache = false) {
    console.log("[Claude Analytics] Fetching Claude Code analytics...", skipCache ? "(force refresh)" : "");
    try {
      const snapshot = await ipcRenderer.invoke("claudeLocalUsageSnapshot", 30);
      console.log("[Claude Analytics] Snapshot received:", snapshot);

      if (!snapshot) {
        console.log("[Claude Analytics] No snapshot data");
        return;
      }

      renderClaudeSnapshot(snapshot);

      // Save to cache for instant loading next time
      try {
        await ipcRenderer.invoke("saveAnalyticsCache", "claude", snapshot);
        console.log("[Claude Analytics] Saved to cache");
      } catch (cacheErr) {
        console.warn("[Claude Analytics] Cache save failed:", cacheErr);
      }

      console.log("[Claude Analytics] Render complete");
    } catch (err) {
      console.error("[Claude Analytics] Fetch failed:", err);
    }
  }

  // Load cached analytics on startup (instant)
  async function loadCachedClaudeAnalytics() {
    try {
      const cached = await ipcRenderer.invoke("getCachedAnalytics", "claude");
      if (cached) {
        console.log("[Claude Analytics] Loading cached Claude analytics");
        renderClaudeSnapshot(cached);
        return true;
      }
    } catch (err) {
      console.warn("[Claude Analytics] Cache load failed:", err);
    }
    return false;
  }

  // Refresh button handler for Claude
  const refreshClaudeBtn = document.getElementById("refreshClaudeAnalytics");
  if (refreshClaudeBtn) {
    refreshClaudeBtn.addEventListener("click", () => {
      refreshClaudeBtn.disabled = true;
      refreshClaudeBtn.querySelector("i")?.classList.add("fa-spin");

      fetchClaudeAnalytics(true).finally(() => {
        setTimeout(() => {
          refreshClaudeBtn.disabled = false;
          refreshClaudeBtn.querySelector("i")?.classList.remove("fa-spin");
        }, 500);
      });
    });
  }

  // Auto-refresh interval (5 minutes)
  const CLAUDE_ANALYTICS_REFRESH_INTERVAL = 5 * 60 * 1000;
  let claudeAnalyticsRefreshInterval = null;

  function startClaudeAnalyticsRefresh() {
    if (claudeAnalyticsRefreshInterval) return;
    // First load cached, then fetch fresh in background
    loadCachedClaudeAnalytics().then(() => {
      fetchClaudeAnalytics();
    });
    claudeAnalyticsRefreshInterval = setInterval(fetchClaudeAnalytics, CLAUDE_ANALYTICS_REFRESH_INTERVAL);
  }

  function stopClaudeAnalyticsRefresh() {
    if (claudeAnalyticsRefreshInterval) {
      clearInterval(claudeAnalyticsRefreshInterval);
      claudeAnalyticsRefreshInterval = null;
    }
  }

  // Observe page visibility for Claude analytics
  const analyticsPage = document.getElementById("analyticsPage");
  if (analyticsPage) {
    const observer = new MutationObserver(() => {
      if (!analyticsPage.hidden) {
        startClaudeAnalyticsRefresh();
      } else {
        stopClaudeAnalyticsRefresh();
      }
    });
    observer.observe(analyticsPage, {
      attributes: true,
      attributeFilter: ["hidden"],
    });

    // Initial check
    if (!analyticsPage.hidden) {
      startClaudeAnalyticsRefresh();
    }
  }

  // Load cached analytics on app startup (non-blocking)
  loadCachedClaudeAnalytics();

  // Expose for external use
  window.fetchClaudeAnalytics = fetchClaudeAnalytics;
  window.loadCachedClaudeAnalytics = loadCachedClaudeAnalytics;

  window.addEventListener("analyticsRangeChanged", () => {
    if (claudeSnapshot) {
      renderClaudeSummaryCards(claudeSnapshot);
    }
  });

  console.log("[Claude Analytics] Module initialized");
})();

/* ============================================
   Phantom Analytics Center - Comparison Analytics
   ============================================ */
(function () {
  "use strict";

  // Store snapshots for comparison
  let codexSnapshot = null;
  let claudeSnapshot = null;

  // Format large token numbers
  function formatTokens(n) {
    if (n == null || isNaN(n)) return "--";
    if (n >= 1_000_000_000) return (n / 1_000_000_000).toFixed(1).replace(/\.0$/, "") + "b";
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(0) + "m";
    if (n >= 1_000) return (n / 1_000).toFixed(0) + "k";
    return n.toString();
  }

  // Listen for snapshot updates from the individual analytics modules
  window.addEventListener("codexSnapshotUpdated", (e) => {
    codexSnapshot = e.detail;
    renderComparisonMetrics();
  });

  window.addEventListener("claudeSnapshotUpdated", (e) => {
    claudeSnapshot = e.detail;
    renderComparisonMetrics();
  });

  // Render head-to-head comparison metrics
  function renderComparisonMetrics() {
    const rangeDays = Number.isFinite(window.analyticsRangeDays)
      ? window.analyticsRangeDays
      : 7;

    const codexDays = (codexSnapshot?.days || []).slice(-rangeDays);
    const claudeDays = (claudeSnapshot?.days || []).slice(-rangeDays);

    const codexTotals = codexDays.reduce(
      (acc, day) => {
        acc.totalTokens += day.totalTokens || 0;
        acc.outputTokens += day.outputTokens || 0;
        acc.inputTokens += day.inputTokens || 0;
        acc.cachedTokens += day.cachedInputTokens || 0;
        return acc;
      },
      { totalTokens: 0, outputTokens: 0, inputTokens: 0, cachedTokens: 0 }
    );

    const claudeTotals = claudeDays.reduce(
      (acc, day) => {
        acc.totalTokens += day.totalTokens || 0;
        acc.outputTokens += day.outputTokens || 0;
        acc.inputTokens += day.inputTokens || 0;
        acc.cacheReadTokens += day.cacheReadTokens || 0;
        return acc;
      },
      { totalTokens: 0, outputTokens: 0, inputTokens: 0, cacheReadTokens: 0 }
    );

    const codexCache = codexTotals.inputTokens > 0
      ? (codexTotals.cachedTokens / codexTotals.inputTokens) * 100
      : 0;
    const claudeInputAttempted = claudeTotals.inputTokens + claudeTotals.cacheReadTokens;
    const claudeCache = claudeInputAttempted > 0
      ? (claudeTotals.cacheReadTokens / claudeInputAttempted) * 100
      : 0;

    const codexAvg = codexDays.length
      ? Math.round(codexTotals.totalTokens / codexDays.length)
      : 0;
    const claudeAvg = claudeDays.length
      ? Math.round(claudeTotals.totalTokens / claudeDays.length)
      : 0;

    updateMetricBar("codex7dBar", "claude7dBar", codexTotals.totalTokens, claudeTotals.totalTokens);
    updateMetricValue("codex7dVal", formatTokens(codexTotals.totalTokens));
    updateMetricValue("claude7dVal", formatTokens(claudeTotals.totalTokens));

    updateMetricBar("codexCacheBar", "claudeCacheBar", codexCache, claudeCache);
    updateMetricValue("codexCacheVal", codexCache.toFixed(1) + "%");
    updateMetricValue("claudeCacheVal", claudeCache.toFixed(1) + "%");

    updateMetricBar("codexAvgBar", "claudeAvgBar", codexAvg, claudeAvg);
    updateMetricValue("codexAvgVal", formatTokens(codexAvg));
    updateMetricValue("claudeAvgVal", formatTokens(claudeAvg));

    // Winner badge
    const winnerName = document.getElementById("winnerName");
    if (winnerName) {
      if (codexTotals.totalTokens > claudeTotals.totalTokens) {
        winnerName.textContent = "Codex";
        winnerName.style.color = "#89a9b8";
      } else if (claudeTotals.totalTokens > codexTotals.totalTokens) {
        winnerName.textContent = "Claude";
        winnerName.style.color = "#c68a7a";
      } else {
        winnerName.textContent = "Tie!";
        winnerName.style.color = "#f78a97";
      }
    }
  }

  function updateMetricBar(codexId, claudeId, codexVal, claudeVal) {
    const total = codexVal + claudeVal || 1;
    const codexBar = document.getElementById(codexId);
    const claudeBar = document.getElementById(claudeId);

    if (codexBar) codexBar.style.width = (codexVal / total * 100) + "%";
    if (claudeBar) claudeBar.style.width = (claudeVal / total * 100) + "%";
  }

  function updateMetricValue(id, value) {
    const el = document.getElementById(id);
    if (el) el.textContent = value;
  }

  // Refresh All button handler
  const refreshAllBtn = document.getElementById("refreshAllAnalytics");
  if (refreshAllBtn) {
    refreshAllBtn.addEventListener("click", () => {
      refreshAllBtn.disabled = true;
      refreshAllBtn.querySelector("i")?.classList.add("fa-spin");

      Promise.all([
        window.fetchCodexAnalytics?.(true),
        window.fetchClaudeAnalytics?.(true)
      ]).finally(() => {
        setTimeout(() => {
          refreshAllBtn.disabled = false;
          refreshAllBtn.querySelector("i")?.classList.remove("fa-spin");
        }, 500);
      });
    });
  }

  function updateRangeLabels(days) {
    const totalLabels = document.querySelectorAll('[data-stat-label="totalTokens"]');
    totalLabels.forEach((label) => {
      label.textContent = `Total Tokens (${days}D)`;
    });
    const outputLabels = document.querySelectorAll('[data-stat-label="outputTokens"]');
    outputLabels.forEach((label) => {
      label.textContent = `Output Tokens (${days}D)`;
    });
    const costLabels = document.querySelectorAll('[data-stat-label="totalCost"]');
    costLabels.forEach((label) => {
      label.textContent = `Est. Cost (${days}D)`;
    });
    const metricLabels = document.querySelectorAll('[data-metric-label="totalTokens"]');
    metricLabels.forEach((label) => {
      label.textContent = `Total Tokens (${days}D)`;
    });
  }

  function setAnalyticsRangeDays(days) {
    window.analyticsRangeDays = days;
    updateRangeLabels(days);
    window.dispatchEvent(new CustomEvent("analyticsRangeChanged", { detail: { days } }));
    renderComparisonMetrics();
  }

  const activeRange = document.querySelector(".phantom-range-btn.active");
  const initialRange = activeRange ? parseInt(activeRange.dataset.range, 10) : 7;
  setAnalyticsRangeDays(Number.isFinite(initialRange) ? initialRange : 7);

  // Time range buttons
  document.querySelectorAll(".phantom-range-btn").forEach((btn) => {
    btn.addEventListener("click", () => {
      document.querySelectorAll(".phantom-range-btn").forEach((b) => b.classList.remove("active"));
      btn.classList.add("active");
      const rangeDays = parseInt(btn.dataset.range, 10);
      setAnalyticsRangeDays(Number.isFinite(rangeDays) ? rangeDays : 7);
    });
  });

  console.log("[Phantom Analytics Center] Comparison module initialized");
})();

// Skills Page Module
(function initSkillsPage() {
  const skillsTabs = document.querySelectorAll(".skills-tab");
  const skillsPanels = document.querySelectorAll(".skills-panel");
  const refreshBtn = document.getElementById("refreshSkillsBtn");
  const applyChangesBtn = document.getElementById("applySkillChangesBtn");

  if (!skillsTabs.length) return;

  // Map frontend agent IDs to backend identifiers
  const agentIdMap = {
    "claude-code": "claude",
    "codex": "codex"
  };

  // Friendly names for display
  const agentDisplayNames = {
    "claude-code": "Claude Code",
    "codex": "Codex"
  };

  // Cache for loaded skills
  const skillsCache = {};
  let currentAgent = "codex";
  let skillsChanged = false;
  
  function escapeHtml(text) {
    const div = document.createElement("div");
    div.textContent = text || "";
    return div.innerHTML;
  }
  
  function renderSkillCard(skill) {
    const toggleDisabled = !skill.can_toggle ? 'disabled' : '';
    const toggleChecked = skill.enabled ? 'checked' : '';
    const disabledClass = !skill.enabled ? 'skill-card-disabled' : '';
    const toggleTitle = !skill.can_toggle
      ? 'Project skills cannot be toggled'
      : (skill.enabled ? 'Click to disable this skill' : 'Click to enable this skill');

    return `<div class="skill-card ${disabledClass}" data-skill-name="${escapeHtml(skill.name)}" data-skill-path="${escapeHtml(skill.path || '')}">
      <div class="skill-card-header">
        <div class="skill-card-name">${escapeHtml(skill.name)}</div>
        <label class="skill-toggle" title="${toggleTitle}">
          <input type="checkbox" ${toggleChecked} ${toggleDisabled} />
          <span class="skill-toggle-slider"></span>
        </label>
      </div>
      <div class="skill-card-description">${escapeHtml(skill.description)}</div>
    </div>`;
  }
  
  function renderEmptyState(message) {
    return `<div class="skills-empty-state">
      <i class="fal fa-magic"></i>
      <p>${message}</p>
    </div>`;
  }
  
  function updateSkillsPanel(agentId, skills) {
    const frontendId = Object.keys(agentIdMap).find(k => agentIdMap[k] === agentId) || agentId;

    // Map frontend IDs to the data-skills attribute prefix
    const skillsPrefix = frontendId === "claude-code" ? "claude" : frontendId;

    // Query all grids directly (works with multi-card layout)
    const personalGrid = document.querySelector(`[data-skills="${skillsPrefix}-personal"]`);
    const projectGrid = document.querySelector(`[data-skills="${skillsPrefix}-project"]`);

    const personalSkills = skills.filter(s => s.source === "personal");
    const projectSkills = skills.filter(s => s.source === "project");

    if (personalGrid) {
      personalGrid.innerHTML = personalSkills.length > 0
        ? personalSkills.map(renderSkillCard).join("")
        : renderEmptyState("No personal skills found");
    }

    if (projectGrid) {
      projectGrid.innerHTML = projectSkills.length > 0
        ? projectSkills.map(renderSkillCard).join("")
        : renderEmptyState("No project skills found. Select a project path in Create Tasks.");
    }

    // Update tab count
    const tabCount = document.querySelector(`[data-count="${frontendId}"]`);
    if (tabCount) {
      tabCount.textContent = skills.length;
    }
  }
  
  async function loadSkillsForAgent(agentId) {
    const backendId = agentIdMap[agentId] || agentId;
    
    // Get project path from Create Tasks page
    const projectPath = typeof getProjectPath === "function" ? getProjectPath() : null;
    
    try {
      const skills = await ipcRenderer.invoke("getAgentSkills", backendId, projectPath);
      skillsCache[agentId] = skills || [];
      updateSkillsPanel(backendId, skills || []);
    } catch (err) {
      console.warn("[Skills] Failed to load skills for", agentId, err);
      skillsCache[agentId] = [];
      updateSkillsPanel(backendId, []);
    }
  }
  
  async function loadAllSkills() {
    const agents = Object.keys(agentIdMap);
    await Promise.all(agents.map(loadSkillsForAgent));
  }
  
  function switchTab(agentId) {
    currentAgent = agentId;
    
    // Update tab styles
    skillsTabs.forEach(tab => {
      tab.classList.toggle("active", tab.dataset.agent === agentId);
    });
    
    // Show/hide panels
    skillsPanels.forEach(panel => {
      panel.hidden = panel.dataset.panel !== agentId;
    });
    
    // Load skills if not cached
    if (!skillsCache[agentId]) {
      loadSkillsForAgent(agentId);
    }
  }
  
  // Tab click handlers
  skillsTabs.forEach(tab => {
    tab.addEventListener("click", () => switchTab(tab.dataset.agent));
  });
  
  // Refresh button
  if (refreshBtn) {
    refreshBtn.addEventListener("click", () => {
      skillsCache[currentAgent] = null;
      loadSkillsForAgent(currentAgent);
    });
  }
  
  // Load skills when page becomes visible
  const skillsPage = document.getElementById("skillsPage");
  if (skillsPage) {
    const observer = new MutationObserver((mutations) => {
      mutations.forEach((mutation) => {
        if (mutation.attributeName === "hidden" && !skillsPage.hidden) {
          loadAllSkills();
        }
      });
    });
    observer.observe(skillsPage, { attributes: true });

    // Toggle event handler for skill cards (event delegation)
    skillsPage.addEventListener("change", async (event) => {
      const toggle = event.target.closest(".skill-toggle input[type='checkbox']");
      if (!toggle) return;

      const skillCard = toggle.closest(".skill-card");
      if (!skillCard) return;

      const skillName = skillCard.dataset.skillName;
      const enabled = toggle.checked;
      const backendId = agentIdMap[currentAgent] || currentAgent;

      // Prevent further changes while processing
      toggle.disabled = true;

      try {
        await ipcRenderer.invoke("toggleSkill", backendId, skillName, enabled);

        // Update card visual state
        if (enabled) {
          skillCard.classList.remove("skill-card-disabled");
        } else {
          skillCard.classList.add("skill-card-disabled");
        }

        // Show success toast
        if (typeof sendNotification === "function") {
          sendNotification(
            `Skill "${skillName}" ${enabled ? "enabled" : "disabled"}. Click "Apply Changes" to restart agents.`,
            enabled ? "green" : "yellow"
          );
        }

        // Clear cache so next refresh shows correct state
        skillsCache[currentAgent] = null;

        // Show the Apply Changes button
        skillsChanged = true;
        if (applyChangesBtn) {
          applyChangesBtn.style.display = "inline-flex";
        }

      } catch (err) {
        console.error("[Skills] Toggle failed:", err);
        // Revert toggle state
        toggle.checked = !enabled;
        if (typeof sendNotification === "function") {
          sendNotification(`Failed to toggle skill: ${err}`, "red");
        }
      } finally {
        // Re-enable toggle (unless it's a project skill)
        const skillData = skillsCache[currentAgent]?.find(s => s.name === skillName);
        if (!skillData || skillData.can_toggle) {
          toggle.disabled = false;
        }
      }
    });
  }

  // Apply Changes button click handler
  if (applyChangesBtn) {
    applyChangesBtn.addEventListener("click", async () => {
      console.log("[Skills] Apply Changes clicked");

      // Check for running tasks first
      try {
        const runningTasks = await ipcRenderer.invoke("getRunningTasks");
        console.log("[Skills] Running tasks:", runningTasks);

        if (runningTasks && runningTasks.length > 0) {
          // Populate the modal with running agents
          const runningList = document.getElementById("runningAgentsList");
          if (runningList) {
            runningList.innerHTML = runningTasks.map(task => {
              const displayName = agentDisplayNames[task.agent_id] || task.agent_id;
              const title = task.title_summary || task.prompt?.slice(0, 50) || "Untitled task";
              return `<li><i class="fal fa-circle text-success"></i> <strong>${displayName}</strong>: ${title}</li>`;
            }).join("");
          }

          // Show the confirmation modal
          $("#applySkillChangesModal").modal("show");
        } else {
          // No running tasks, proceed directly
          await performRestart();
        }
      } catch (err) {
        console.error("[Skills] Error checking running tasks:", err);
        // If we can't check, assume no running tasks and proceed
        await performRestart();
      }
    });
  }

  // Confirmation modal "Restart Anyway" button
  const confirmBtn = document.getElementById("confirmApplySkillChanges");
  if (confirmBtn) {
    confirmBtn.addEventListener("click", async () => {
      $("#applySkillChangesModal").modal("hide");
      await performRestart();
    });
  }

  // Perform the actual restart
  async function performRestart() {
    console.log("[Skills] Restarting all agents...");

    if (applyChangesBtn) {
      applyChangesBtn.disabled = true;
      applyChangesBtn.innerHTML = '<i class="fal fa-spinner fa-spin"></i> Restarting...';
    }

    try {
      const restartedIds = await ipcRenderer.invoke("restartAllAgents");
      console.log("[Skills] Restarted agents for tasks:", restartedIds);

      // Reset state
      skillsChanged = false;

      // Hide the button with a small delay for visual feedback
      setTimeout(() => {
        if (applyChangesBtn) {
          applyChangesBtn.style.display = "none";
          applyChangesBtn.disabled = false;
          applyChangesBtn.innerHTML = '<i class="fal fa-bolt"></i> Apply Changes';
        }
      }, 500);

      // Show success notification
      if (typeof sendNotification === "function") {
        const count = restartedIds?.length || 0;
        if (count > 0) {
          sendNotification(`Restarted ${count} agent session${count !== 1 ? "s" : ""}. Skill changes are now active.`, "green");
        } else {
          sendNotification("Skill changes saved. They will apply to new agent sessions.", "green");
        }
      }
    } catch (err) {
      console.error("[Skills] Restart failed:", err);
      if (applyChangesBtn) {
        applyChangesBtn.disabled = false;
        applyChangesBtn.innerHTML = '<i class="fal fa-bolt"></i> Apply Changes';
      }
      if (typeof sendNotification === "function") {
        sendNotification(`Failed to restart agents: ${err}`, "red");
      }
    }
  }

  console.log("[Skills] Page module initialized");
})();

// Keybinds Module
(function initKeybinds() {
  // Default keybinds
  const DEFAULT_KEYBINDS = {
    // Navigation
    "nav.createTasks": { mod: true, key: "1" },
    "nav.viewTasks": { mod: true, key: "2" },
    "nav.accounts": { mod: true, key: "3" },
    "nav.skills": { mod: true, key: "4" },
    "nav.analytics": { mod: true, key: "5" },
    "nav.settings": { mod: true, key: "6" },
    // Agent selection
    "agent.codex": { mod: true, shift: true, key: "1" },
    "agent.claude": { mod: true, shift: true, key: "2" },
    "agent.amp": { mod: true, shift: true, key: "3" },
    "agent.droid": { mod: true, shift: true, key: "4" },
    "agent.opencode": { mod: true, shift: true, key: "5" },
    "agent.factoryDroid": { mod: true, shift: true, key: "6" },
    // Actions
    "action.createTask": { mod: true, key: "Enter" },
    "action.focusPrompt": { mod: true, key: "l" },
    "action.pickProject": { mod: true, shift: true, key: "o" }
  };

  // Action handlers
  const ACTION_HANDLERS = {
    "nav.createTasks": () => switchToPage("createTasksPage"),
    "nav.viewTasks": () => switchToPage("viewTasksPage"),
    "nav.accounts": () => switchToPage("accountsPage"),
    "nav.skills": () => switchToPage("skillsPage"),
    "nav.analytics": () => switchToPage("analyticsPage"),
    "nav.settings": () => switchToPage("settingsPage"),
    "agent.codex": () => window.selectAgentById && window.selectAgentById("codex"),
    "agent.claude": () => window.selectAgentById && window.selectAgentById("claude-code"),
    "agent.amp": () => window.selectAgentById && window.selectAgentById("amp"),
    "agent.droid": () => window.selectAgentById && window.selectAgentById("droid"),
    "agent.opencode": () => window.selectAgentById && window.selectAgentById("opencode"),
    "agent.factoryDroid": () => window.selectAgentById && window.selectAgentById("factory-droid"),
    "action.createTask": () => $("#createAgentButton").click(),
    "action.focusPrompt": () => $("#initialPrompt").focus(),
    "action.pickProject": () => $("#pickProjectPath").click()
  };

  // Current keybinds (loaded from settings or defaults)
  let keybinds = {};
  let recordingAction = null;

  // Load keybinds from settings
  function loadKeybinds() {
    const saved = localStorage.getItem("phantom-keybinds");
    if (saved) {
      try {
        keybinds = JSON.parse(saved);
        // Ensure all default actions exist
        for (const action in DEFAULT_KEYBINDS) {
          if (!keybinds[action]) {
            keybinds[action] = { ...DEFAULT_KEYBINDS[action] };
          }
        }
      } catch (e) {
        keybinds = JSON.parse(JSON.stringify(DEFAULT_KEYBINDS));
      }
    } else {
      keybinds = JSON.parse(JSON.stringify(DEFAULT_KEYBINDS));
    }
  }

  // Save keybinds to settings
  function saveKeybinds() {
    localStorage.setItem("phantom-keybinds", JSON.stringify(keybinds));
  }

  // Format keybind for display
  function formatKeybind(kb) {
    if (!kb || !kb.key) return "Not set";
    const parts = [];
    const isMac = navigator.platform.includes("Mac");
    if (kb.mod) parts.push(isMac ? "Cmd" : "Ctrl");
    if (kb.shift) parts.push("Shift");
    if (kb.alt) parts.push(isMac ? "Opt" : "Alt");
    
    // Format key name
    let keyName = kb.key;
    if (keyName === "Enter") keyName = "Enter";
    else if (keyName === " ") keyName = "Space";
    else if (keyName.length === 1) keyName = keyName.toUpperCase();
    
    parts.push(keyName);
    return parts;
  }

  // Render keybind display
  function renderKeybindDisplay(kb) {
    const parts = formatKeybind(kb);
    if (typeof parts === "string") return parts;
    return parts.map(p => `<span class="keybind-key">${p}</span>`).join('<span class="keybind-plus">+</span>');
  }

  // Update all keybind displays
  function updateKeybindDisplays() {
    document.querySelectorAll(".keybind-input").forEach(btn => {
      const action = btn.dataset.keybind;
      const kb = keybinds[action];
      const keysEl = btn.querySelector(".keybind-keys");
      if (keysEl) {
        keysEl.innerHTML = renderKeybindDisplay(kb);
      }
    });
  }

  // Check for conflicts
  function findConflict(action, kb) {
    const kbStr = serializeKeybind(kb);
    for (const [otherAction, otherKb] of Object.entries(keybinds)) {
      if (otherAction !== action && serializeKeybind(otherKb) === kbStr) {
        return otherAction;
      }
    }
    return null;
  }

  // Serialize keybind for comparison
  function serializeKeybind(kb) {
    if (!kb || !kb.key) return "";
    return `${kb.mod ? "mod+" : ""}${kb.shift ? "shift+" : ""}${kb.alt ? "alt+" : ""}${kb.key.toLowerCase()}`;
  }

  // Match keydown event to keybind
  function matchKeybind(e, kb) {
    if (!kb || !kb.key) return false;
    const modMatch = kb.mod ? (e.metaKey || e.ctrlKey) : !(e.metaKey || e.ctrlKey);
    const shiftMatch = kb.shift ? e.shiftKey : !e.shiftKey;
    const altMatch = kb.alt ? e.altKey : !e.altKey;
    const keyMatch = e.key.toLowerCase() === kb.key.toLowerCase();
    return modMatch && shiftMatch && altMatch && keyMatch;
  }

  // Get action label
  function getActionLabel(action) {
    const row = document.querySelector(`[data-action="${action}"] .keybind-label`);
    return row ? row.textContent : action;
  }

  // Handle keyboard events
  function handleKeydown(e) {
    // If recording a new keybind
    if (recordingAction) {
      e.preventDefault();
      e.stopPropagation();
      
      // Escape to cancel
      if (e.key === "Escape") {
        stopRecording();
        return;
      }
      
      // Ignore modifier-only presses
      if (["Control", "Meta", "Shift", "Alt"].includes(e.key)) {
        return;
      }
      
      const newKb = {
        mod: e.metaKey || e.ctrlKey,
        shift: e.shiftKey,
        alt: e.altKey,
        key: e.key
      };
      
      // Check for conflicts
      const conflict = findConflict(recordingAction, newKb);
      const warningEl = document.getElementById("keybindConflictWarning");
      const warningText = document.getElementById("keybindConflictText");
      
      if (conflict) {
        warningEl.style.display = "flex";
        warningText.textContent = `This shortcut is already used by "${getActionLabel(conflict)}"`;
        document.querySelector(`[data-keybind="${recordingAction}"]`)?.classList.add("conflict");
        return;
      }
      
      // Save the new keybind
      keybinds[recordingAction] = newKb;
      saveKeybinds();
      updateKeybindDisplays();
      stopRecording();
      warningEl.style.display = "none";
      return;
    }
    
    // Normal keybind handling
    for (const [action, kb] of Object.entries(keybinds)) {
      if (matchKeybind(e, kb)) {
        const handler = ACTION_HANDLERS[action];
        if (handler) {
          e.preventDefault();
          handler();
          return;
        }
      }
    }
  }

  // Start recording a new keybind
  function startRecording(action) {
    recordingAction = action;
    const btn = document.querySelector(`[data-keybind="${action}"]`);
    if (btn) {
      btn.classList.add("recording");
      btn.querySelector(".keybind-keys").innerHTML = "Press keys...";
    }
    document.getElementById("keybindConflictWarning").style.display = "none";
  }

  // Stop recording
  function stopRecording() {
    if (recordingAction) {
      const btn = document.querySelector(`[data-keybind="${recordingAction}"]`);
      if (btn) {
        btn.classList.remove("recording", "conflict");
      }
    }
    recordingAction = null;
    updateKeybindDisplays();
  }

  // Reset to defaults
  function resetToDefaults() {
    keybinds = JSON.parse(JSON.stringify(DEFAULT_KEYBINDS));
    saveKeybinds();
    updateKeybindDisplays();
    document.getElementById("keybindConflictWarning").style.display = "none";
  }

  // Initialize
  function init() {
    loadKeybinds();
    
    // Set up keybind input click handlers
    document.querySelectorAll(".keybind-input").forEach(btn => {
      btn.addEventListener("click", (e) => {
        e.preventDefault();
        if (recordingAction) {
          stopRecording();
        }
        startRecording(btn.dataset.keybind);
      });
    });

    // Reset button
    const resetBtn = document.getElementById("resetKeybindsBtn");
    if (resetBtn) {
      resetBtn.addEventListener("click", resetToDefaults);
    }

    // Global keydown handler
    document.addEventListener("keydown", handleKeydown);

    // Click outside to cancel recording
    document.addEventListener("click", (e) => {
      if (recordingAction && !e.target.closest(".keybind-input")) {
        stopRecording();
      }
    });

    // Initial display update
    updateKeybindDisplays();
    
    console.log("[Keybinds] Module initialized");
  }

  // Export for external use
  window.keybindsModule = {
    getKeybinds: () => keybinds,
    resetToDefaults,
    updateKeybindDisplays
  };

  // Initialize when DOM is ready
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();

// =============================================================================
// AUTO-UPDATE HANDLING
// =============================================================================
(function() {
  'use strict';

  var tauri = window.__TAURI__ || null;
  var tauriCore = tauri && tauri.core ? tauri.core : null;
  var tauriInvoke = tauriCore && tauriCore.invoke ? tauriCore.invoke : null;
  var tauriEvents = tauri && tauri.event ? tauri.event : null;

  var updateAvailable = null;

  // Check for updates on app startup (after a short delay)
  setTimeout(async function() {
    if (!tauriInvoke) {
      console.log('[Auto-Update] Not available in browser mode');
      return;
    }
    try {
      console.log('[Auto-Update] Checking for updates...');
      var result = await tauriInvoke('check_for_updates');
      if (result) {
        console.log('[Auto-Update] Update available:', result.version);
        showUpdateToast(result.version, result.notes);
      } else {
        console.log('[Auto-Update] No updates available');
      }
    } catch (err) {
      console.log('[Auto-Update] Check failed:', err);
    }
  }, 3000); // Check 3 seconds after startup

  // Listen for update-available event from backend
  if (tauriEvents && typeof tauriEvents.listen === 'function') {
    tauriEvents.listen('update-available', function(event) {
      var payload = event.payload || {};
      console.log('[Auto-Update] update-available event:', payload);
      showUpdateToast(payload.version, payload.notes);
    });

    // Listen for download progress
    tauriEvents.listen('update-progress', function(event) {
      var progress = event.payload;
      console.log('[Auto-Update] Download progress:', progress);
      updateProgressBar(progress);
    });
  }

  function showUpdateToast(version, notes) {
    updateAvailable = { version: version, notes: notes };

    var versionEl = document.getElementById('update-version');
    var toastEl = document.getElementById('update-toast');
    var progressBarEl = document.getElementById('update-progress-bar');
    var installBtn = document.getElementById('update-install-btn');

    if (versionEl) versionEl.textContent = version || 'unknown';
    if (toastEl) toastEl.classList.remove('hidden');
    if (progressBarEl) progressBarEl.classList.add('hidden');
    if (installBtn) {
      installBtn.disabled = false;
      installBtn.textContent = 'Install';
    }
  }

  function hideUpdateToast() {
    var toastEl = document.getElementById('update-toast');
    if (toastEl) toastEl.classList.add('hidden');
  }

  function updateProgressBar(percent) {
    var fillEl = document.getElementById('update-progress-fill');
    if (fillEl) fillEl.style.width = percent + '%';
  }

  // Install button click handler
  document.addEventListener('DOMContentLoaded', function() {
    var installBtn = document.getElementById('update-install-btn');
    var dismissBtn = document.getElementById('update-dismiss-btn');

    if (installBtn) {
      installBtn.addEventListener('click', async function() {
        if (!tauriInvoke) return;

        installBtn.disabled = true;
        installBtn.textContent = 'Installing...';

        // Show progress bar
        var progressBarEl = document.getElementById('update-progress-bar');
        if (progressBarEl) progressBarEl.classList.remove('hidden');
        updateProgressBar(0);

        try {
          await tauriInvoke('install_update');
          // App will restart automatically after install
        } catch (err) {
          console.error('[Auto-Update] Install failed:', err);
          installBtn.disabled = false;
          installBtn.textContent = 'Retry';
          if (typeof sendNotification === 'function') {
            sendNotification('Update failed: ' + err, 'red');
          }
        }
      });
    }

    if (dismissBtn) {
      dismissBtn.addEventListener('click', function() {
        hideUpdateToast();
      });
    }
  });

  console.log('[Auto-Update] Module initialized');
})();
