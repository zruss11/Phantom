/**
 * Automations UI
 * Cron-backed schedules that create + auto-start tasks while the app is open.
 */
(function () {
  'use strict';

  var bridge = window.tauriBridge;
  var ipcRenderer = bridge && bridge.ipcRenderer ? bridge.ipcRenderer : null;
  if (!ipcRenderer) return;

  var automations = [];
  var runs = [];
  var selectedAutomationId = null;

  var agentDropdown = null;
  var modelDropdown = null;
  var scheduleMode = 'simple';
  var editingAutomationId = null;
  var nextRunPreviewTimer = null;
  var relativeTimeTicker = null;
  var autoRefreshTimer = null;

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
    console.log('[Automations]', message);
  }

  function basename(path) {
    if (!path) return '-';
    try {
      var parts = String(path).split(/[\\/]/).filter(Boolean);
      return parts.length ? parts[parts.length - 1] : path;
    } catch (e) {
      return path;
    }
  }

  function formatRelative(tsSeconds) {
    if (!tsSeconds) return '--';
    var now = Math.floor(Date.now() / 1000);
    var diff = tsSeconds - now;
    var abs = Math.abs(diff);

    // Note: we intentionally avoid seconds in the UI, but keep it "live" and monotonic.
    if (abs < 60) {
      return diff >= 0 ? 'in <1m' : 'just now';
    }

    var units = [
      { s: 86400, label: 'd' },
      { s: 3600, label: 'h' },
      { s: 60, label: 'm' }
    ];

    var out = '1m';
    for (var i = 0; i < units.length; i++) {
      var u = units[i];
      if (abs >= u.s) {
        // For future times, count down (ceil). For past times, count up (floor).
        var n = diff >= 0 ? Math.ceil(abs / u.s) : Math.floor(abs / u.s);
        if (n < 1) n = 1;
        out = n + u.label;
        break;
      }
    }

    if (diff >= 0) return 'in ' + out;
    return out + ' ago';
  }

  function computeAutomationWhenText(a) {
    if (!a) return '--';
    if (!a.enabled) return 'Disabled';
    if (a.nextRunAt) return 'Starts ' + formatRelative(a.nextRunAt);
    return 'Enabled';
  }

  function computeAutomationMetaText(a) {
    if (!a) return '--';
    var enabledText = a.enabled ? 'Enabled' : 'Disabled';
    var next = a.enabled && a.nextRunAt ? ('Next ' + formatRelative(a.nextRunAt)) : 'No next run';
    var meta = enabledText + ' - ' + next;
    if (a.lastError) {
      // Keep it short so the header doesn't turn into a novel.
      var msg = String(a.lastError);
      if (msg.length > 90) msg = msg.slice(0, 90) + '...';
      meta += ' - Last error: ' + msg;
    }
    return meta;
  }

  function tickRelativeTimes() {
    // Scheduled list: update "Starts in ..." as time passes without rebuilding the whole list.
    document.querySelectorAll('.automation-item-when[data-automation-id]').forEach(function (el) {
      var id = el.dataset.automationId;
      if (!id) return;
      var a = getAutomationById(id);
      var when = computeAutomationWhenText(a);
      if (el.textContent !== when) el.textContent = when;
    });

    // Runs list: update "Xm ago" in place.
    document.querySelectorAll('.automation-run-when[data-run-ts]').forEach(function (el) {
      var ts = parseInt(el.dataset.runTs, 10);
      if (!Number.isFinite(ts)) return;
      var when = formatRelative(ts);
      if (el.textContent !== when) el.textContent = when;
    });

    // Detail panel: update the meta + timestamps in place.
    if (!selectedAutomationId) return;
    var a = getAutomationById(selectedAutomationId);
    if (!a) return;

    var meta = byId('automationDetailMeta');
    if (meta) {
      var metaText = computeAutomationMetaText(a);
      if (meta.textContent !== metaText) meta.textContent = metaText;
    }

    var nextRun = byId('automationDetailNextRun');
    if (nextRun) {
      var nextText = a.nextRunAt ? formatRelative(a.nextRunAt) : '--';
      if (nextRun.textContent !== nextText) nextRun.textContent = nextText;
    }

    var lastRun = byId('automationDetailLastRun');
    if (lastRun) {
      var lastText = a.lastRunAt ? formatRelative(a.lastRunAt) : '--';
      if (lastRun.textContent !== lastText) lastRun.textContent = lastText;
    }
  }

  function startRelativeTimeTicker() {
    if (relativeTimeTicker) return;
    // Fast enough to feel "live", slow enough to stay cheap.
    relativeTimeTicker = setInterval(tickRelativeTimes, 15000);
    setTimeout(tickRelativeTimes, 250);
    window.addEventListener('focus', tickRelativeTimes);
  }

  function isAutomationsPageActive() {
    var page = byId('automationsPage');
    return !!(page && !page.hidden);
  }

  function startAutoRefresh() {
    if (autoRefreshTimer) return;
    // This keeps scheduler-created runs/errors visible without the user having to re-open the page.
    autoRefreshTimer = setInterval(function () {
      if (!isAutomationsPageActive()) return;
      refreshAll();
    }, 30000);
    window.addEventListener('focus', function () {
      if (!isAutomationsPageActive()) return;
      refreshAll();
    });
  }

  function setDetailVisible(visible) {
    var empty = byId('automationsEmpty');
    var detail = byId('automationDetail');
    if (empty) empty.hidden = !!visible;
    if (detail) detail.hidden = !visible;
  }

  function getAutomationById(id) {
    return automations.find(function (a) { return a.id === id; }) || null;
  }

  function renderScheduledList() {
    var list = byId('automationsScheduledList');
    if (!list) return;

    if (!automations.length) {
      list.innerHTML =
        '<div class="automations-placeholder">No scheduled automations</div>';
      return;
    }

    var html = '';
    automations.forEach(function (a) {
      var selected = a.id === selectedAutomationId ? ' selected' : '';
      var disabled = !a.enabled ? ' disabled' : '';
      var when = computeAutomationWhenText(a);
      var project = a.projectPath ? basename(a.projectPath) : '-';
      var pill = a.cron ? escapeHtml(a.cron) : '--';

      html +=
        '<button type="button" class="automation-item' + selected + disabled + '" data-automation-id="' + escapeHtml(a.id) + '">' +
          '<div class="automation-item-top">' +
            '<div class="automation-item-name">' + escapeHtml(a.name) + '</div>' +
            '<div class="automation-item-when" data-automation-id="' + escapeHtml(a.id) + '">' + escapeHtml(when) + '</div>' +
          '</div>' +
          '<div class="automation-item-sub">' +
            '<span class="automation-item-project">' + escapeHtml(project) + '</span>' +
            '<span class="automation-pill">' + pill + '</span>' +
          '</div>' +
        '</button>';
    });

    list.innerHTML = html;
  }

  function renderRunsList() {
    var list = byId('automationsRunsList');
    if (!list) return;

    if (!runs.length) {
      list.innerHTML =
        '<div class="automations-placeholder">No runs yet</div>';
      return;
    }

    var nameById = {};
    automations.forEach(function (a) {
      nameById[a.id] = a.name;
    });

    var html = '';
    runs.forEach(function (r) {
      var title = nameById[r.automationId] || 'Automation';
      var ts = r.createdAt || r.scheduledFor;
      var when = formatRelative(ts);
      var status = r.error ? 'Failed' : 'Ran';
      var taskId = r.taskId || '';
      var cls = 'automation-run-item' + (r.error ? ' failed' : '');

      html +=
        '<button type="button" class="' + cls + '" data-task-id="' + escapeHtml(taskId) + '">' +
          '<div class="automation-run-top">' +
            '<div class="automation-run-title">' + escapeHtml(title) + '</div>' +
            '<div class="automation-run-when" data-run-ts="' + escapeHtml(String(ts || '')) + '">' + escapeHtml(when) + '</div>' +
          '</div>' +
          '<div class="automation-run-sub">' +
            '<span class="automation-run-status">' + escapeHtml(status) + '</span>' +
            (r.error ? ('<span class="automation-run-error" title="' + escapeHtml(r.error) + '">' + escapeHtml(r.error) + '</span>') : '') +
          '</div>' +
        '</button>';
    });

    list.innerHTML = html;
  }

  function renderDetail() {
    if (!selectedAutomationId) {
      setDetailVisible(false);
      return;
    }

    var a = getAutomationById(selectedAutomationId);
    if (!a) {
      selectedAutomationId = null;
      setDetailVisible(false);
      return;
    }

    setDetailVisible(true);

    var title = byId('automationDetailTitle');
    var meta = byId('automationDetailMeta');
    var runNowBtn = byId('automationRunNowBtn');
    var editBtn = byId('automationEditBtn');
    var toggleBtn = byId('automationToggleBtn');
    var deleteBtn = byId('automationDeleteBtn');

    if (title) title.textContent = a.name || '--';
    if (meta) {
      meta.textContent = computeAutomationMetaText(a);
    }

    if (runNowBtn) runNowBtn.dataset.automationId = a.id;
    if (editBtn) editBtn.dataset.automationId = a.id;
    if (toggleBtn) {
      toggleBtn.dataset.automationId = a.id;
      toggleBtn.textContent = a.enabled ? 'Disable' : 'Enable';
    }
    if (deleteBtn) deleteBtn.dataset.automationId = a.id;

    var agent = byId('automationDetailAgent');
    var model = byId('automationDetailModel');
    var project = byId('automationDetailProject');
    var cron = byId('automationDetailCron');
    var nextRun = byId('automationDetailNextRun');
    var lastRun = byId('automationDetailLastRun');
    var prompt = byId('automationDetailPrompt');

    if (agent) agent.textContent = a.agentId || '--';
    if (model) model.textContent = a.execModel || 'default';
    if (project) project.textContent = a.projectPath || '--';
    if (cron) cron.textContent = a.cron || '--';
    if (nextRun) nextRun.textContent = a.nextRunAt ? formatRelative(a.nextRunAt) : '--';
    if (lastRun) lastRun.textContent = a.lastRunAt ? formatRelative(a.lastRunAt) : '--';
    if (prompt) prompt.textContent = a.prompt || '--';
  }

  async function refreshAll() {
    try {
      var list = await ipcRenderer.invoke('loadAutomations');
      automations = Array.isArray(list) ? list : [];
    } catch (err) {
      console.warn('[Automations] loadAutomations failed:', err);
      automations = [];
    }

    try {
      var runList = await ipcRenderer.invoke('loadAutomationRuns', 25);
      runs = Array.isArray(runList) ? runList : [];
    } catch (err2) {
      console.warn('[Automations] loadAutomationRuns failed:', err2);
      runs = [];
    }

    renderScheduledList();
    renderRunsList();
    renderDetail();

    // If there are no automations, keep the empty state visible.
    if (!automations.length) {
      selectedAutomationId = null;
      setDetailVisible(false);
    }
  }

  function getBuilderCron() {
    var timeInput = byId('automationTimeInput');
    var pills = byId('automationDowPills');
    var timeValue = timeInput && timeInput.value ? timeInput.value : '09:00';
    var parts = timeValue.split(':');
    var hour = parseInt(parts[0], 10);
    var minute = parseInt(parts[1], 10);
    if (!Number.isFinite(hour)) hour = 9;
    if (!Number.isFinite(minute)) minute = 0;

    var selected = [];
    if (pills) {
      pills.querySelectorAll('.dow-pill.selected').forEach(function (btn) {
        var dow = parseInt(btn.dataset.dow, 10);
        if (Number.isFinite(dow)) selected.push(dow);
      });
    }
    selected.sort(function (a, b) { return a - b; });

    var dowField = '*';
    if (selected.length === 0) {
      dowField = '*';
    } else if (selected.length === 7) {
      dowField = '*';
    } else {
      dowField = selected.join(',');
    }

    return minute + ' ' + hour + ' * * ' + dowField;
  }

  function setScheduleMode(mode) {
    scheduleMode = mode === 'cron' ? 'cron' : 'simple';
    var simpleEl = byId('automationScheduleSimple');
    var cronEl = byId('automationScheduleCron');
    if (simpleEl) simpleEl.hidden = scheduleMode !== 'simple';
    if (cronEl) cronEl.hidden = scheduleMode !== 'cron';

    document.querySelectorAll('.schedule-mode-btn').forEach(function (btn) {
      btn.classList.toggle('selected', btn.dataset.scheduleMode === scheduleMode);
    });

    var cronPreview = byId('automationCronPreview');
    var cronInput = byId('automationCronInput');
    var cronValue = scheduleMode === 'simple' ? getBuilderCron() : (cronInput ? cronInput.value : '');
    if (cronPreview) cronPreview.textContent = cronValue || '--';
    if (scheduleMode === 'cron' && cronInput && !cronInput.value) {
      cronInput.value = getBuilderCron();
    }
  }

  function setAutomationPromptText(text) {
    var el = byId('automationPrompt');
    if (!el) return;
    el.innerText = text || '';
    updateAutomationPromptPlaceholder();
  }

  function updateAutomationPromptPlaceholder() {
    var el = byId('automationPrompt');
    if (!el) return;
    var text = (el.innerText || el.textContent || '')
      .replace(/\u200B/g, '')
      .replace(/\u00A0/g, ' ')
      .trim();
    el.classList.toggle('is-empty', text.length === 0);
  }

  async function loadModelsForAgent(agentId, preferredValue) {
    if (!modelDropdown) return;

    // Seed with cached models first (instant), then refresh.
    try {
      var cached = await ipcRenderer.invoke('getCachedModels', agentId);
      if (Array.isArray(cached) && cached.length) {
        var cachedItems = [{ value: 'default', name: 'Use agent default', description: '' }];
        cached.forEach(function (m) {
          var value = m.value || m.id || m.modelId;
          if (!value) return;
          cachedItems.push({
            value: value,
            name: (m.name || value),
            description: (m.description || '')
          });
        });
        modelDropdown.setOptions(cachedItems);
      }
    } catch (e) {
      // ignore
    }

    try {
      var fresh = await ipcRenderer.invoke('getAgentModels', agentId);
      var items = [{ value: 'default', name: 'Use agent default', description: '' }];
      if (Array.isArray(fresh)) {
        fresh.forEach(function (m) {
          if (typeof m === 'string') {
            items.push({ value: m, name: m, description: '' });
            return;
          }
          if (!m || typeof m !== 'object') return;
          var value = m.value || m.id || m.modelId || '';
          if (!value) return;
          items.push({
            value: value,
            name: m.name || m.label || value,
            description: m.description || ''
          });
        });
      }
      modelDropdown.setOptions(items);

      if (preferredValue && items.some(function (it) { return it.value === preferredValue; })) {
        modelDropdown.setValue(preferredValue);
        return;
      }
      if (!modelDropdown.getValue()) {
        modelDropdown.setValue('default');
      }
    } catch (err) {
      console.warn('[Automations] getAgentModels failed:', err);
    }
  }

  function getModalCronValue() {
    if (scheduleMode === 'cron') {
      var cronInput = byId('automationCronInput');
      return cronInput ? String(cronInput.value || '').trim() : '';
    }
    return getBuilderCron();
  }

  function computeNextRunAtFromBuilder() {
    var timeInput = byId('automationTimeInput');
    var pills = byId('automationDowPills');
    var timeValue = timeInput && timeInput.value ? timeInput.value : '09:00';
    var parts = timeValue.split(':');
    var hour = parseInt(parts[0], 10);
    var minute = parseInt(parts[1], 10);
    if (!Number.isFinite(hour)) hour = 9;
    if (!Number.isFinite(minute)) minute = 0;

    var selected = [];
    if (pills) {
      pills.querySelectorAll('.dow-pill.selected').forEach(function (btn) {
        var dow = parseInt(btn.dataset.dow, 10);
        if (Number.isFinite(dow)) selected.push(dow);
      });
    }

    if (!selected.length || selected.length === 7) {
      selected = [0, 1, 2, 3, 4, 5, 6];
    }

    var now = new Date();
    for (var offset = 0; offset <= 7; offset++) {
      var candidate = new Date(now.getTime());
      candidate.setDate(now.getDate() + offset);
      candidate.setHours(hour, minute, 0, 0);

      // If we're scheduling "today", ensure we pick a time in the future.
      if (offset === 0 && candidate.getTime() <= now.getTime()) continue;

      if (selected.indexOf(candidate.getDay()) >= 0) {
        return Math.floor(candidate.getTime() / 1000);
      }
    }

    return null;
  }

  function scheduleNextRunPreview() {
    if (nextRunPreviewTimer) clearTimeout(nextRunPreviewTimer);
    nextRunPreviewTimer = setTimeout(updateNextRunPreview, 220);
  }

  async function updateNextRunPreview() {
    var el = byId('automationNextRunPreview');
    if (!el) return;

    var enabledToggle = byId('automationEnabledToggle');
    var enabled = enabledToggle ? !!enabledToggle.checked : true;
    if (!enabled) {
      el.textContent = 'Next run: Disabled';
      return;
    }

    var cron = getModalCronValue();
    if (!cron) {
      el.textContent = 'Next run: --';
      return;
    }

    // Prefer the backend preview (matches actual scheduler behavior).
    try {
      var next = await ipcRenderer.invoke('previewAutomationNextRun', cron);
      if (next) {
        el.textContent = 'Next run: ' + formatRelative(next);
        return;
      }
    } catch (e) {
      // Fall back to simple builder preview (browser mode or older backend).
    }

    if (scheduleMode === 'simple') {
      var nextFromBuilder = computeNextRunAtFromBuilder();
      if (nextFromBuilder) {
        el.textContent = 'Next run: ' + formatRelative(nextFromBuilder);
        return;
      }
    }

    el.textContent = 'Next run: --';
  }

  function populateModalFromAutomation(a) {
    if (!a) return;
    byId('automationModalLabel').textContent = 'Edit automation';
    byId('automationSaveBtn').textContent = 'Save';

    byId('automationNameInput').value = a.name || '';
    byId('automationProjectPath').value = a.projectPath || '';
    if (agentDropdown) agentDropdown.setValue(a.agentId || 'codex');

    // Prompt
    setAutomationPromptText(a.prompt || '');

    // Enabled toggle
    var enabledToggle = byId('automationEnabledToggle');
    if (enabledToggle) {
      enabledToggle.checked = !!a.enabled;
      if (typeof window.syncToggleButtonsFromCheckbox === 'function') {
        window.syncToggleButtonsFromCheckbox(enabledToggle);
      }
    }

    // Schedule mode heuristic: builder supports only "m h * * dow".
    var cron = String(a.cron || '').trim();
    var fields = cron.split(/\s+/).filter(Boolean);
    var builderDowOk = false;
    if (fields.length === 5) {
      builderDowOk =
        fields[4] === '*' ||
        fields[4] === '?' ||
        /^[0-7](,[0-7])*$/.test(fields[4]);
    }
    var builderCompatible =
      fields.length === 5 &&
      /^\d+$/.test(fields[0]) &&
      /^\d+$/.test(fields[1]) &&
      fields[2] === '*' &&
      fields[3] === '*' &&
      builderDowOk;

    if (builderCompatible) {
      var minute = parseInt(fields[0], 10);
      var hour = parseInt(fields[1], 10);
      if (!Number.isFinite(minute)) minute = 0;
      if (!Number.isFinite(hour)) hour = 9;
      var hh = String(hour).padStart(2, '0');
      var mm = String(minute).padStart(2, '0');
      byId('automationTimeInput').value = hh + ':' + mm;

      var dowField = fields[4];
      var selected = null;
      if (dowField === '*' || dowField === '?') {
        selected = [0, 1, 2, 3, 4, 5, 6];
      } else {
        selected = dowField
          .split(',')
          .map(function (x) {
            var n = parseInt(x, 10);
            if (n === 7) return 0;
            return n;
          })
          .filter(function (n) { return Number.isFinite(n) && n >= 0 && n <= 6; });
        if (!selected.length) selected = [0, 1, 2, 3, 4, 5, 6];
      }

      var pills = byId('automationDowPills');
      if (pills) {
        pills.querySelectorAll('.dow-pill').forEach(function (btn) {
          var dow = parseInt(btn.dataset.dow, 10);
          btn.classList.toggle('selected', selected.indexOf(dow) >= 0);
        });
      }
      setScheduleMode('simple');
    } else {
      byId('automationCronInput').value = cron;
      setScheduleMode('cron');
    }

    if (agentDropdown) {
      loadModelsForAgent(agentDropdown.getValue(), a.execModel || 'default');
    }

    scheduleNextRunPreview();
  }

  function resetModalForCreate() {
    byId('automationModalLabel').textContent = 'Create automation';
    byId('automationSaveBtn').textContent = 'Create';

    byId('automationNameInput').value = '';
    byId('automationProjectPath').value = (byId('projectPath') && byId('projectPath').value) ? byId('projectPath').value : '';

    var defaultAgent = window.activeAgentId || 'codex';
    if (agentDropdown) agentDropdown.setValue(defaultAgent);
    if (modelDropdown) modelDropdown.setValue('default');
    setAutomationPromptText('');

    // Default schedule: 09:00 daily
    byId('automationTimeInput').value = '09:00';
    var pills = byId('automationDowPills');
    if (pills) {
      pills.querySelectorAll('.dow-pill').forEach(function (btn) {
        btn.classList.add('selected');
      });
    }
    byId('automationCronInput').value = '';
    setScheduleMode('simple');

    var enabledToggle = byId('automationEnabledToggle');
    if (enabledToggle) {
      enabledToggle.checked = true;
      if (typeof window.syncToggleButtonsFromCheckbox === 'function') {
        window.syncToggleButtonsFromCheckbox(enabledToggle);
      }
    }

    if (agentDropdown) loadModelsForAgent(agentDropdown.getValue(), 'default');
    scheduleNextRunPreview();
  }

  function openAutomationModal(editAutomation) {
    editingAutomationId = editAutomation ? editAutomation.id : null;

    if (editAutomation) {
      populateModalFromAutomation(editAutomation);
    } else {
      resetModalForCreate();
    }

    if (window.promptSlashCommands && window.promptSlashCommands.suppressBlur !== undefined) {
      // nothing
    }

    try {
      $('#automationModal').modal('show');
    } catch (e) {
      // Bootstrap/jQuery should exist, but keep it safe.
      console.warn('[Automations] Failed to show modal:', e);
    }
  }

  async function onSaveAutomation() {
    var isEdit = !!editingAutomationId;
    var saveBtn = byId('automationSaveBtn');
    if (saveBtn) {
      saveBtn.disabled = true;
      saveBtn.textContent = isEdit ? 'Saving...' : 'Creating...';
    }

    try {
      var name = String(byId('automationNameInput').value || '').trim();
      var projectPath = String(byId('automationProjectPath').value || '').trim();
      var agentId = agentDropdown ? agentDropdown.getValue() : 'codex';
      var execModel = modelDropdown ? modelDropdown.getValue() : 'default';
      var promptEl = byId('automationPrompt');
      var promptText = promptEl ? String(promptEl.innerText || promptEl.textContent || '').trim() : '';
      var cron = getModalCronValue();
      var enabledToggle = byId('automationEnabledToggle');
      var enabled = enabledToggle ? !!enabledToggle.checked : true;

      if (!name) throw new Error('Name is required');
      if (!promptText) throw new Error('Prompt is required');
      if (!cron) throw new Error('Cron is required');

      var payload = {
        name: name,
        agentId: agentId,
        execModel: execModel,
        prompt: promptText,
        projectPath: projectPath,
        baseBranch: null,
        planMode: false,
        thinking: true,
        useWorktree: true,
        permissionMode: null,
        reasoningEffort: null,
        agentMode: null,
        codexMode: null,
        claudeRuntime: null,
        cron: cron,
        enabled: enabled
      };

      var result = null;
      if (isEdit) {
        result = await ipcRenderer.invoke('updateAutomation', editingAutomationId, payload);
      } else {
        result = await ipcRenderer.invoke('createAutomation', payload);
      }

      if (result && result.id) {
        selectedAutomationId = result.id;
      }

      try {
        $('#automationModal').modal('hide');
      } catch (e2) {
        // ignore
      }

      editingAutomationId = null;
      notify(isEdit ? 'Automation updated' : 'Automation created', 'green');
      await refreshAll();
    } catch (err) {
      console.warn('[Automations] save failed:', err);
      notify('Automation save failed: ' + (err && err.message ? err.message : err), 'red');
    } finally {
      if (saveBtn) {
        saveBtn.disabled = false;
        saveBtn.textContent = isEdit ? 'Save' : 'Create';
      }
    }
  }

  async function onRunNow(automationId) {
    if (!automationId) return;
    try {
      var taskId = await ipcRenderer.invoke('runAutomationNow', automationId);
      notify('Automation started', 'green');
      if (typeof switchToPage === 'function') {
        switchToPage('viewTasksPage');
      }
      if (taskId && typeof window.tauriBridge !== 'undefined') {
        console.log('[Automations] runAutomationNow task:', taskId);
      }
      await refreshAll();
    } catch (err) {
      console.warn('[Automations] runAutomationNow failed:', err);
      notify('Run failed: ' + (err && err.message ? err.message : err), 'red');
    }
  }

  async function onToggleEnabled(automationId) {
    var a = getAutomationById(automationId);
    if (!a) return;
    try {
      await ipcRenderer.invoke('updateAutomation', automationId, { enabled: !a.enabled });
      await refreshAll();
      notify(!a.enabled ? 'Automation enabled' : 'Automation disabled', 'yellow');
    } catch (err) {
      console.warn('[Automations] toggle failed:', err);
      notify('Toggle failed: ' + (err && err.message ? err.message : err), 'red');
    }
  }

  async function onDeleteAutomation(automationId) {
    var a = getAutomationById(automationId);
    if (!a) return;
    var ok = window.confirm('Delete automation "' + a.name + '"?');
    if (!ok) return;
    try {
      await ipcRenderer.invoke('deleteAutomation', automationId);
      if (selectedAutomationId === automationId) {
        selectedAutomationId = null;
      }
      await refreshAll();
      notify('Automation deleted', 'yellow');
    } catch (err) {
      console.warn('[Automations] delete failed:', err);
      notify('Delete failed: ' + (err && err.message ? err.message : err), 'red');
    }
  }

  function wireEvents() {
    var createBtn = byId('createAutomationBtn');
    var createFirstBtn = byId('createFirstAutomationBtn');
    if (createBtn) {
      createBtn.addEventListener('click', function () {
        openAutomationModal(null);
      });
    }
    if (createFirstBtn) {
      createFirstBtn.addEventListener('click', function () {
        openAutomationModal(null);
      });
    }

    var scheduledList = byId('automationsScheduledList');
    if (scheduledList) {
      scheduledList.addEventListener('click', function (e) {
        var btn = e.target.closest('.automation-item');
        if (!btn) return;
        selectedAutomationId = btn.dataset.automationId || null;
        renderScheduledList();
        renderDetail();
      });
    }

    var runsList = byId('automationsRunsList');
    if (runsList) {
      runsList.addEventListener('click', function (e) {
        var btn = e.target.closest('.automation-run-item');
        if (!btn) return;
        var taskId = btn.dataset.taskId;
        if (taskId) {
          ipcRenderer.send('OpenAgentChatLog', taskId);
        }
      });
    }

    var runNowBtn = byId('automationRunNowBtn');
    if (runNowBtn) {
      runNowBtn.addEventListener('click', function () {
        onRunNow(runNowBtn.dataset.automationId);
      });
    }
    var editBtn = byId('automationEditBtn');
    if (editBtn) {
      editBtn.addEventListener('click', function () {
        var a = getAutomationById(editBtn.dataset.automationId);
        if (a) openAutomationModal(a);
      });
    }
    var toggleBtn = byId('automationToggleBtn');
    if (toggleBtn) {
      toggleBtn.addEventListener('click', function () {
        onToggleEnabled(toggleBtn.dataset.automationId);
      });
    }
    var deleteBtn = byId('automationDeleteBtn');
    if (deleteBtn) {
      deleteBtn.addEventListener('click', function () {
        onDeleteAutomation(deleteBtn.dataset.automationId);
      });
    }

    var saveBtn = byId('automationSaveBtn');
    if (saveBtn) {
      saveBtn.addEventListener('click', function () {
        onSaveAutomation();
      });
    }

    var pickBtn = byId('automationPickProjectPath');
    if (pickBtn) {
      pickBtn.addEventListener('click', function () {
        if (typeof window.openFileBrowser === 'function') {
          window.openFileBrowser(function (picked) {
            if (picked) byId('automationProjectPath').value = picked;
          });
          return;
        }
        ipcRenderer.invoke('pickProjectPath').then(function (picked) {
          if (picked) byId('automationProjectPath').value = picked;
        });
      });
    }

    document.querySelectorAll('.schedule-mode-btn').forEach(function (btn) {
      btn.addEventListener('click', function () {
        setScheduleMode(btn.dataset.scheduleMode);
        scheduleNextRunPreview();
      });
    });

    var timeInput = byId('automationTimeInput');
    if (timeInput) {
      timeInput.addEventListener('change', function () {
        if (scheduleMode !== 'simple') return;
        setScheduleMode('simple');
        scheduleNextRunPreview();
      });
    }

    var pills = byId('automationDowPills');
    if (pills) {
      pills.addEventListener('click', function (e) {
        var pill = e.target.closest('.dow-pill');
        if (!pill) return;
        pill.classList.toggle('selected');
        if (scheduleMode !== 'simple') return;
        setScheduleMode('simple');
        scheduleNextRunPreview();
      });
    }

    var cronInput = byId('automationCronInput');
    if (cronInput) {
      cronInput.addEventListener('input', function () {
        if (scheduleMode !== 'cron') return;
        var cronPreview = byId('automationCronPreview');
        if (cronPreview) cronPreview.textContent = String(cronInput.value || '').trim() || '--';
        scheduleNextRunPreview();
      });
    }

    var promptEl = byId('automationPrompt');
    if (promptEl) {
      updateAutomationPromptPlaceholder();
      promptEl.addEventListener('input', updateAutomationPromptPlaceholder);
      promptEl.addEventListener('blur', updateAutomationPromptPlaceholder);
      promptEl.addEventListener('focus', updateAutomationPromptPlaceholder);
    }

    var enabledToggle = byId('automationEnabledToggle');
    if (enabledToggle) {
      enabledToggle.addEventListener('change', scheduleNextRunPreview);
    }

    try {
      $('#automationModal').on('hidden.bs.modal', function () {
        editingAutomationId = null;
      });
    } catch (e2) {
      // ignore
    }
  }

  function initDropdowns() {
    if (!window.CustomDropdown) return;

    var agentItems = [
      { value: 'codex', name: 'Codex', description: 'OpenAI (OAuth or API key)' },
      { value: 'claude-code', name: 'Claude Code', description: 'Anthropic (CLI or OAuth)' },
      { value: 'amp', name: 'Amp', description: 'Amp CLI' },
      { value: 'droid', name: 'Droid', description: 'Factory Droid CLI' },
      { value: 'opencode', name: 'OpenCode', description: 'OpenCode CLI' }
    ];

    var agentContainer = byId('automationAgentDropdown');
    if (agentContainer) {
      agentDropdown = new window.CustomDropdown({
        container: agentContainer,
        items: agentItems,
        placeholder: 'Agent',
        defaultValue: window.activeAgentId || 'codex',
        searchable: true,
        searchPlaceholder: 'Search agents...',
        onChange: function (value) {
          loadModelsForAgent(value, 'default');
          if (automationSlash) automationSlash.setAgent(value);
        }
      });
    }

    var modelContainer = byId('automationModelDropdown');
    if (modelContainer) {
      modelDropdown = new window.CustomDropdown({
        container: modelContainer,
        items: [{ value: 'default', name: 'Use agent default', description: '' }],
        placeholder: 'Model',
        defaultValue: 'default',
        searchable: true,
        searchPlaceholder: 'Search models...',
        onChange: function () {}
      });
    }
  }

  var automationSlash = null;
  function initSlashAutocomplete() {
    var promptEl = byId('automationPrompt');
    if (!promptEl || !window.SlashCommandAutocomplete) return;
    automationSlash = new window.SlashCommandAutocomplete(
      promptEl,
      (agentDropdown && agentDropdown.getValue()) || window.activeAgentId || 'codex'
    );
  }

  function bindNavigationRefresh() {
    window.addEventListener('PhantomNavigate', function (e) {
      if (!e || !e.detail) return;
      if (e.detail.pageId === 'automationsPage') {
        refreshAll();
      }
    });
  }

  function init() {
    initDropdowns();
    initSlashAutocomplete();
    wireEvents();
    bindNavigationRefresh();
    startRelativeTimeTicker();
    startAutoRefresh();

    // Initial load (don't wait for user navigation in case Automations is first click).
    setTimeout(refreshAll, 600);
  }

  init();
})();
