/**
 * Task Scheduler (Create Tasks page)
 * Simple scheduling UI that creates cron-backed automations.
 *
 * Automations run as tasks in View Tasks and auto-start while Phantom is open.
 * We intentionally do not expose run history here.
 */
(function () {
  'use strict';

  var bridge = window.tauriBridge;
  var ipcRenderer = bridge && bridge.ipcRenderer ? bridge.ipcRenderer : null;
  if (!ipcRenderer) return;

  var state = {
    automations: [],
    selectedId: null,
    refreshing: false,
  };

  function byId(id) {
    return document.getElementById(id);
  }

  function escapeHtml(text) {
    if (typeof window.escapeHtml === 'function') return window.escapeHtml(text);
    var div = document.createElement('div');
    div.textContent = text || '';
    return div.innerHTML;
  }

  function notify(message, color) {
    if (typeof window.sendNotification === 'function') {
      window.sendNotification(message, color);
      return;
    }
    console.log('[TaskScheduler]', message);
  }

  function basename(path) {
    if (!path) return '';
    try {
      var parts = String(path).split(/[\\/]/).filter(Boolean);
      return parts.length ? parts[parts.length - 1] : String(path);
    } catch (e) {
      return String(path);
    }
  }

  function normalizeOptionalText(value) {
    var t = (value || '').toString().trim();
    return t ? t : null;
  }

  function getProjectPath() {
    var el = byId('projectPath');
    return normalizeOptionalText(el ? el.value : '');
  }

  function getBaseBranch() {
    try {
      if (window.baseBranchDropdown && typeof window.baseBranchDropdown.getValue === 'function') {
        var val = window.baseBranchDropdown.getValue();
        if (val && val !== 'default') return val;
      }
    } catch (e) {}
    return null;
  }

  function getPlanMode() {
    var el = byId('planModeToggle');
    return !!(el && el.checked);
  }

  function getUseWorktree() {
    var el = byId('useWorktreeToggle');
    return !!(el && el.checked);
  }

  function getClaudeRuntime(agentId) {
    if (agentId !== 'claude-code') return null;
    var el = byId('claudeDockerToggle');
    if (!el) return null;
    return el.checked ? 'docker' : 'native';
  }

  function getPrimaryAgentId() {
    return (window.activeAgentId || 'codex').toString();
  }

  function getAgentPretty(agentId) {
    var id = (agentId || '').toString();
    if (id === 'claude-code') return 'Claude Code';
    if (id === 'codex') return 'Codex';
    if (id === 'amp') return 'Amp';
    if (id === 'opencode') return 'OpenCode';
    if (id === 'factory-droid') return 'Factory Droid';
    if (id === 'droid') return 'Droid';
    return id || 'Agent';
  }

  function getPromptText() {
    try {
      if (typeof window.getPromptText === 'function') {
        return (window.getPromptText() || '').toString();
      }
    } catch (e) {}
    var el = byId('initialPrompt');
    var raw = el ? (el.innerText || el.textContent || '') : '';
    return (raw || '').toString();
  }

  function getExecModel() {
    try {
      if (window.execModelDropdown && typeof window.execModelDropdown.getValue === 'function') {
        return window.execModelDropdown.getValue() || 'default';
      }
    } catch (e) {}
    return 'default';
  }

  function getPermissionMode(agentId) {
    // Mirror create task behavior: agents with their own permissions always bypass.
    var agentsWithOwnPermissions = ['codex', 'claude-code', 'droid', 'factory-droid', 'amp', 'opencode'];
    if (agentsWithOwnPermissions.indexOf(agentId) >= 0) return 'bypassPermissions';
    try {
      if (window.permissionDropdown && typeof window.permissionDropdown.getValue === 'function') {
        return window.permissionDropdown.getValue() || 'default';
      }
    } catch (e) {}
    return 'default';
  }

  function getReasoningEffort(agentId) {
    if (agentId !== 'codex') return null;
    try {
      if (window.reasoningEffortDropdown && typeof window.reasoningEffortDropdown.getValue === 'function') {
        var val = window.reasoningEffortDropdown.getValue();
        if (val && val !== 'default') return val;
      }
    } catch (e) {}
    return null;
  }

  function getModeValues(agentId) {
    // Codex: agentModeDropdown represents codexMode choices (default/plan/execute/etc).
    // OpenCode: agentModeDropdown represents agent_mode (build/plan/general/explore).
    var out = { agentMode: null, codexMode: null };
    var planMode = getPlanMode();
    try {
      if (!window.agentModeDropdown || typeof window.agentModeDropdown.getValue !== 'function') {
        return out;
      }
      var val = window.agentModeDropdown.getValue() || 'default';
      if (agentId === 'codex') {
        if (planMode) {
          out.codexMode = 'plan';
        } else if (val !== 'default') {
          out.codexMode = val;
        }
      } else if (agentId === 'opencode') {
        out.agentMode = val || 'build';
      }
    } catch (e) {}
    return out;
  }

  function parseTime(value) {
    var v = (value || '').toString().trim();
    if (!v) v = '09:00';
    var parts = v.split(':');
    var h = parseInt(parts[0], 10);
    var m = parseInt(parts[1], 10);
    if (!Number.isFinite(h) || h < 0 || h > 23) h = 9;
    if (!Number.isFinite(m) || m < 0 || m > 59) m = 0;
    return { hour: h, minute: m };
  }

  function clampInt(value, min, max, fallback) {
    var n = parseInt(value, 10);
    if (!Number.isFinite(n)) return fallback;
    if (n < min) return min;
    if (n > max) return max;
    return n;
  }

  function cronFromForm() {
    var mode = (byId('taskSchedulerModeSelect') && byId('taskSchedulerModeSelect').value) || 'daily';
    var timeInput = byId('taskSchedulerTimeInput');
    var time = parseTime(timeInput ? timeInput.value : '09:00');
    var weekly = byId('taskSchedulerWeeklyDaySelect');
    var everyN = byId('taskSchedulerEveryNInput');

    if (mode === 'daily') {
      return time.minute + ' ' + time.hour + ' * * *';
    }
    if (mode === 'weekdays') {
      return time.minute + ' ' + time.hour + ' * * 1-5';
    }
    if (mode === 'weekly') {
      var dow = clampInt(weekly ? weekly.value : '1', 0, 6, 1);
      return time.minute + ' ' + time.hour + ' * * ' + dow;
    }
    if (mode === 'every_minutes') {
      var nMin = clampInt(everyN ? everyN.value : '15', 5, 720, 15);
      return '*/' + nMin + ' * * * *';
    }
    if (mode === 'every_hours') {
      var nHr = clampInt(everyN ? everyN.value : '1', 1, 24, 1);
      return time.minute + ' */' + nHr + ' * * *';
    }
    return time.minute + ' ' + time.hour + ' * * *';
  }

  function formatRelative(tsSeconds) {
    if (!tsSeconds) return '--';
    var now = Math.floor(Date.now() / 1000);
    var diff = tsSeconds - now;
    var abs = Math.abs(diff);
    if (abs < 60) return diff >= 0 ? 'in <1m' : 'just now';

    var units = [
      { s: 86400, label: 'd' },
      { s: 3600, label: 'h' },
      { s: 60, label: 'm' }
    ];

    var out = '1m';
    for (var i = 0; i < units.length; i++) {
      var u = units[i];
      if (abs >= u.s) {
        var n = diff >= 0 ? Math.ceil(abs / u.s) : Math.floor(abs / u.s);
        if (n < 1) n = 1;
        out = n + u.label;
        break;
      }
    }
    return diff >= 0 ? ('in ' + out) : (out + ' ago');
  }

  function setSelected(id) {
    state.selectedId = id || null;
    renderList();
    syncActionButtons();
  }

  function getSelectedAutomation() {
    if (!state.selectedId) return null;
    return (state.automations || []).find(function (a) { return a && a.id === state.selectedId; }) || null;
  }

  function setFormFromAutomation(a) {
    var name = byId('taskSchedulerNameInput');
    if (name) name.value = (a && a.name) ? a.name : '';

    var enabled = byId('taskSchedulerEnabledToggle');
    if (enabled) enabled.checked = a ? !!a.enabled : true;

    // Keep the builder simple: default to daily for edits, but preserve cron semantics
    // by showing a best-effort mapping for common patterns.
    var modeEl = byId('taskSchedulerModeSelect');
    var timeEl = byId('taskSchedulerTimeInput');
    var weeklyEl = byId('taskSchedulerWeeklyDaySelect');
    var nEl = byId('taskSchedulerEveryNInput');

    if (modeEl) modeEl.value = 'daily';
    if (timeEl) timeEl.value = '09:00';
    if (weeklyEl) weeklyEl.value = '1';
    if (nEl) nEl.value = '15';

    // Best-effort parse of cron formats we generate.
    var cron = (a && a.cron) ? String(a.cron) : '';
    var parts = cron.trim().split(/\s+/);
    if (parts.length === 5 && modeEl) {
      var minute = parts[0];
      var hour = parts[1];
      var dom = parts[2];
      var mon = parts[3];
      var dow = parts[4];

      // */N * * * *
      if (/^\*\/\d+$/.test(minute) && hour === '*' && dom === '*' && mon === '*' && dow === '*') {
        modeEl.value = 'every_minutes';
        if (nEl) nEl.value = minute.replace('*/', '');
      }
      // M */N * * *
      else if (/^\d+$/.test(minute) && /^\*\/\d+$/.test(hour) && dom === '*' && mon === '*' && dow === '*') {
        modeEl.value = 'every_hours';
        if (nEl) nEl.value = hour.replace('*/', '');
        if (timeEl) timeEl.value = '09:' + String(parseInt(minute, 10)).padStart(2, '0');
      }
      // M H * * 1-5
      else if (/^\d+$/.test(minute) && /^\d+$/.test(hour) && dom === '*' && mon === '*' && dow === '1-5') {
        modeEl.value = 'weekdays';
        if (timeEl) timeEl.value = String(parseInt(hour, 10)).padStart(2, '0') + ':' + String(parseInt(minute, 10)).padStart(2, '0');
      }
      // M H * * DOW
      else if (/^\d+$/.test(minute) && /^\d+$/.test(hour) && dom === '*' && mon === '*' && /^\d$/.test(dow)) {
        modeEl.value = 'weekly';
        if (weeklyEl) weeklyEl.value = dow;
        if (timeEl) timeEl.value = String(parseInt(hour, 10)).padStart(2, '0') + ':' + String(parseInt(minute, 10)).padStart(2, '0');
      }
      // M H * * *
      else if (/^\d+$/.test(minute) && /^\d+$/.test(hour) && dom === '*' && mon === '*' && dow === '*') {
        modeEl.value = 'daily';
        if (timeEl) timeEl.value = String(parseInt(hour, 10)).padStart(2, '0') + ':' + String(parseInt(minute, 10)).padStart(2, '0');
      }
    }

    syncFormVisibility();
    schedulePreviewRefresh();
    syncActionButtons();
  }

  function syncFormVisibility() {
    var mode = (byId('taskSchedulerModeSelect') && byId('taskSchedulerModeSelect').value) || 'daily';
    var weeklyRow = byId('taskSchedulerWeeklyRow');
    var everyNRow = byId('taskSchedulerEveryNRow');
    var timeGroup = byId('taskSchedulerTimeGroup');
    var unit = byId('taskSchedulerEveryNUnit');

    if (weeklyRow) weeklyRow.style.display = (mode === 'weekly') ? '' : 'none';
    if (everyNRow) everyNRow.style.display = (mode === 'every_minutes' || mode === 'every_hours') ? '' : 'none';
    if (timeGroup) timeGroup.style.display = (mode === 'every_minutes') ? 'none' : '';
    if (unit) unit.textContent = (mode === 'every_hours') ? 'hours' : 'minutes';
  }

  function syncActionButtons() {
    var hasSelected = !!state.selectedId;
    var runBtn = byId('taskSchedulerRunNowBtn');
    var toggleBtn = byId('taskSchedulerToggleBtn');
    var deleteBtn = byId('taskSchedulerDeleteBtn');
    if (runBtn) runBtn.disabled = !hasSelected;
    if (toggleBtn) toggleBtn.disabled = !hasSelected;
    if (deleteBtn) deleteBtn.disabled = !hasSelected;

    var a = getSelectedAutomation();
    if (toggleBtn) toggleBtn.textContent = (a && a.enabled) ? 'Disable' : 'Enable';
  }

  function buildDefaultName(agentId, promptText, projectPath) {
    var agent = getAgentPretty(agentId);
    var project = projectPath ? basename(projectPath) : 'No project';
    var p = (promptText || '').toString().replace(/\s+/g, ' ').trim();
    if (p.length > 42) p = p.slice(0, 42) + '...';
    var tail = p ? p : 'New schedule';
    return agent + ' • ' + project + ' • ' + tail;
  }

  function renderList() {
    var list = byId('taskSchedulerList');
    if (!list) return;
    list.innerHTML = '';

    var projectPath = getProjectPath();
    var items = (state.automations || []).slice();
    if (projectPath) {
      items = items.filter(function (a) {
        return a && (a.projectPath || null) === projectPath;
      });
    }

    // Enabled first, then next run soonest.
    items.sort(function (a, b) {
      var ae = a && a.enabled ? 0 : 1;
      var be = b && b.enabled ? 0 : 1;
      if (ae !== be) return ae - be;
      var an = (a && a.nextRunAt) ? a.nextRunAt : 0;
      var bn = (b && b.nextRunAt) ? b.nextRunAt : 0;
      if (an !== bn) return an - bn;
      return String(a && a.name || '').localeCompare(String(b && b.name || ''));
    });

    if (!items.length) {
      var empty = document.createElement('div');
      empty.className = 'text-muted';
      empty.style.fontSize = '12px';
      empty.textContent = projectPath
        ? 'No schedules for this project yet.'
        : 'No schedules yet.';
      list.appendChild(empty);
      return;
    }

    items.forEach(function (a) {
      if (!a) return;
      var div = document.createElement('div');
      div.className = 'task-scheduler-item' + (a.id === state.selectedId ? ' selected' : '');
      div.dataset.automationId = a.id;

      var whenText = a.enabled
        ? (a.nextRunAt ? ('Next ' + formatRelative(a.nextRunAt)) : 'Enabled')
        : 'Disabled';

      var metaParts = [];
      metaParts.push(getAgentPretty(a.agentId));
      if (a.execModel && a.execModel !== 'default') metaParts.push(a.execModel);
      if (!getProjectPath() && a.projectPath) metaParts.push(basename(a.projectPath));
      var meta = metaParts.join(' · ');

      div.innerHTML =
        '<div class="task-scheduler-item-top">' +
          '<div class="task-scheduler-item-name">' + escapeHtml(a.name || 'Schedule') + '</div>' +
          '<div class="task-scheduler-item-when">' + escapeHtml(whenText) + '</div>' +
        '</div>' +
        '<div class="task-scheduler-item-sub">' +
          '<div class="task-scheduler-item-meta">' + escapeHtml(meta) + '</div>' +
          '<div class="task-scheduler-pill ' + (a.enabled ? '' : 'off') + '">' + (a.enabled ? 'On' : 'Off') + '</div>' +
        '</div>' +
        (a.lastError ? ('<div class="task-scheduler-item-error">Last error: ' + escapeHtml(String(a.lastError)) + '</div>') : '');

      div.addEventListener('click', function () {
        setSelected(a.id);
        setFormFromAutomation(a);
      });

      list.appendChild(div);
    });
  }

  async function refreshAutomations() {
    if (state.refreshing) return;
    state.refreshing = true;
    try {
      var list = await ipcRenderer.invoke('loadAutomations');
      state.automations = Array.isArray(list) ? list : [];
    } catch (e) {
      state.automations = [];
    }
    state.refreshing = false;
    renderList();
    syncActionButtons();
  }

  var previewTimer = null;
  function schedulePreviewRefresh() {
    if (previewTimer) clearTimeout(previewTimer);
    previewTimer = setTimeout(refreshPreview, 220);
  }

  async function refreshPreview() {
    var el = byId('taskSchedulerNextRunPreview');
    if (!el) return;
    el.classList.remove('error');

    var enabledToggle = byId('taskSchedulerEnabledToggle');
    var enabled = enabledToggle ? !!enabledToggle.checked : true;
    if (!enabled) {
      el.textContent = 'Next run: Disabled';
      return;
    }

    var cron = '';
    try {
      cron = cronFromForm();
    } catch (e) {
      el.textContent = 'Next run: --';
      return;
    }

    try {
      var next = await ipcRenderer.invoke('previewAutomationNextRun', cron);
      if (next) {
        el.textContent = 'Next run: ' + formatRelative(next);
        return;
      }
    } catch (e) {
      el.classList.add('error');
      el.textContent = 'Next run: Invalid schedule';
      return;
    }

    el.textContent = 'Next run: --';
  }

  function clearForm() {
    setSelected(null);
    setFormFromAutomation(null);
    var enabled = byId('taskSchedulerEnabledToggle');
    if (enabled) enabled.checked = true;
    syncActionButtons();
    schedulePreviewRefresh();
  }

  async function saveSchedule() {
    var promptText = getPromptText();
    if (!promptText || !promptText.trim()) {
      notify('Add a prompt first (left side).', 'red');
      return;
    }

    var agentId = getPrimaryAgentId();
    var projectPath = getProjectPath();
    var baseBranch = getBaseBranch();
    var planMode = getPlanMode();
    var useWorktree = getUseWorktree();

    var nameEl = byId('taskSchedulerNameInput');
    var name = normalizeOptionalText(nameEl ? nameEl.value : '');
    if (!name) name = buildDefaultName(agentId, promptText, projectPath);

    var enabledToggle = byId('taskSchedulerEnabledToggle');
    var enabled = enabledToggle ? !!enabledToggle.checked : true;

    var cron = cronFromForm();
    var execModel = getExecModel();
    var permissionMode = getPermissionMode(agentId);
    var reasoningEffort = getReasoningEffort(agentId);
    var modes = getModeValues(agentId);
    var claudeRuntime = getClaudeRuntime(agentId);

    var payload = {
      name: name,
      enabled: enabled,
      agentId: agentId,
      execModel: execModel,
      prompt: promptText,
      projectPath: projectPath,
      baseBranch: baseBranch,
      planMode: planMode,
      thinking: true,
      useWorktree: useWorktree,
      permissionMode: permissionMode,
      reasoningEffort: reasoningEffort,
      agentMode: modes.agentMode,
      codexMode: modes.codexMode,
      claudeRuntime: claudeRuntime,
      cron: cron,
    };

    try {
      if (state.selectedId) {
        await ipcRenderer.invoke('updateAutomation', state.selectedId, payload);
        notify('Schedule updated', 'green');
      } else {
        var created = await ipcRenderer.invoke('createAutomation', payload);
        if (created && created.id) {
          state.selectedId = created.id;
        }
        notify('Schedule saved', 'green');
      }
    } catch (e) {
      console.error('[TaskScheduler] save failed', e);
      notify('Failed to save schedule', 'red');
      return;
    }

    await refreshAutomations();
    if (state.selectedId) {
      var a = getSelectedAutomation();
      setFormFromAutomation(a);
    }
  }

  async function runSelectedNow() {
    if (!state.selectedId) return;
    try {
      await ipcRenderer.invoke('runAutomationNow', state.selectedId);
      notify('Running now…', 'green');
    } catch (e) {
      notify('Run failed', 'red');
    }
    refreshAutomations();
  }

  async function toggleSelected() {
    var a = getSelectedAutomation();
    if (!a) return;
    var nextEnabled = !a.enabled;
    try {
      await ipcRenderer.invoke('updateAutomation', a.id, { enabled: nextEnabled });
      notify(nextEnabled ? 'Enabled' : 'Disabled', 'green');
    } catch (e) {
      notify('Failed to update', 'red');
      return;
    }
    refreshAutomations();
  }

  async function deleteSelected() {
    var a = getSelectedAutomation();
    if (!a) return;
    if (!confirm('Delete this schedule?')) return;
    try {
      await ipcRenderer.invoke('deleteAutomation', a.id);
      notify('Deleted', 'green');
    } catch (e) {
      notify('Failed to delete', 'red');
      return;
    }
    clearForm();
    refreshAutomations();
  }

  function bind() {
    var root = byId('taskScheduler');
    if (!root) return;

    var modeEl = byId('taskSchedulerModeSelect');
    var timeEl = byId('taskSchedulerTimeInput');
    var weeklyEl = byId('taskSchedulerWeeklyDaySelect');
    var nEl = byId('taskSchedulerEveryNInput');
    var enabledEl = byId('taskSchedulerEnabledToggle');

    var saveBtn = byId('taskSchedulerSaveBtn');
    var newBtn = byId('taskSchedulerNewBtn');
    var runBtn = byId('taskSchedulerRunNowBtn');
    var toggleBtn = byId('taskSchedulerToggleBtn');
    var deleteBtn = byId('taskSchedulerDeleteBtn');

    if (modeEl) {
      modeEl.addEventListener('change', function () {
        syncFormVisibility();
        schedulePreviewRefresh();
      });
    }
    if (timeEl) timeEl.addEventListener('change', schedulePreviewRefresh);
    if (weeklyEl) weeklyEl.addEventListener('change', schedulePreviewRefresh);
    if (nEl) nEl.addEventListener('input', schedulePreviewRefresh);
    if (enabledEl) enabledEl.addEventListener('change', schedulePreviewRefresh);

    if (saveBtn) saveBtn.addEventListener('click', saveSchedule);
    if (newBtn) newBtn.addEventListener('click', clearForm);
    if (runBtn) runBtn.addEventListener('click', runSelectedNow);
    if (toggleBtn) toggleBtn.addEventListener('click', toggleSelected);
    if (deleteBtn) deleteBtn.addEventListener('click', deleteSelected);

    // Refresh list on navigation or project changes.
    window.addEventListener('PhantomNavigate', function (e) {
      if (e && e.detail && e.detail.pageId === 'createTasksPage') {
        refreshAutomations();
        schedulePreviewRefresh();
      }
    });

    var projectEl = byId('projectPath');
    if (projectEl) projectEl.addEventListener('change', renderList);

    syncFormVisibility();
    schedulePreviewRefresh();
    refreshAutomations();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', bind);
  } else {
    bind();
  }
})();

