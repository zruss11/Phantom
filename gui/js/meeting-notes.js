(function () {
  var bridge = window.tauriBridge;
  var ipcRenderer = bridge ? bridge.ipcRenderer : null;

  // ─── State ───
  var state = {
    initialized: false,
    eventsBound: false,
    modelDownloaded: false,
    recording: false,
    paused: false,
    sessionId: null,
    timerInterval: null,
    timerSeconds: 0,
    segments: [],
    textNoteSuppressSave: false,
    textNoteSaveTimer: null,
    textNoteLastSavedText: '',
    sessions: [],
    selectedSessionId: null,
    visibilityObserver: null,

    // New for 3-panel layout
    currentView: 'default',
    searchQuery: '',
    activeFilter: 'mine',
    folders: [],
    selectedFolderId: null,
    sessionFolderMap: {},

    // Calendar (Coming up)
    upcomingEvents: [],
    upcomingLoading: false,
    upcomingError: null,
    upcomingRefreshTimer: null,
    calendarEnabled: true,
    calendarSelected: {},
    calendarList: [],
    calendarListLoading: false,
    calendarListError: null,
    notesSettingsTab: 'models',
    fullSettings: null,

    // Dictation (global transcription)
    // UX: default OFF so we don't trigger OS permission prompts on first launch.
    dictationEnabled: false,
    dictationActivation: 'fn_hold',
    dictationEngine: 'local',
    dictationShortcut: 'Option+Space',
    dictationFnWindowMs: 350,
    // UX: default OFF; turning this on will prompt for macOS Accessibility permission.
    dictationPasteIntoInputs: false,
    dictationClipboardFallback: true,
    dictationRestoreClipboard: true,
    dictationFlattenNewlinesInSingleLine: true,
    dictationCleanupEnabled: false,
    dictationCleanupRemoveLike: false,
    transcriptionAvailable: null,
    dictationStatus: null,

    // Chat sidebar
    chatAgentId: 'claude-code',
    chatTaskId: null,
    chatMessages: [],
    chatSessionActive: false,
    chatContextSent: false,
    chatStreamingEl: null,
    chatStreamingText: '',
    templates: [],
    selectedTemplateId: null,
    templatesModalOpen: false,
    chatThreadsSessionId: null,
    chatThreads: [],
    chatThreadId: null,

    // Sparkle summary (separate from chat sidebar)
    summariesAgentId: null,
    summaryTaskId: null,
    summarySessionId: null,
    summaryGenerating: false,
    summaryStreamingText: '',

    // Cmd+K palette
    paletteOpen: false,
    paletteQuery: '',
    paletteItemsFlat: [],
    paletteActiveIndex: 0,
    paletteLastFocus: null,

    // Local ASR models (Whisper + Parakeet)
    whisperModels: [],
    whisperDownloaded: {},
    whisperSizeBytes: {},
    activeModelId: null,
    selectedModelId: null,
    modelDownloading: false,
    whisperModelStatus: null,
  };

  var notesChatAgentDropdown = null;
  var notesChatThreadDropdown = null;
  var notesTranscriptFolderDropdown = null;

  function $(id) {
    return document.getElementById(id);
  }

  function escapeHtml(text) {
    var div = document.createElement('div');
    div.textContent = text || '';
    return div.innerHTML;
  }

  function moveSessionToFolder(sessionId, folderId) {
    if (!sessionId) return;
    if (!folderId) {
      delete state.sessionFolderMap[sessionId];
    } else {
      state.sessionFolderMap[sessionId] = folderId;
    }
    saveFolders();
    renderFolderList();
    renderDateGroupedSessions(state.sessions);
    refreshTranscriptFolderDropdown();
    if (state.selectedSessionId === sessionId) {
      // Refresh tags
      selectSession(sessionId);
    }
  }

  function uuid() {
    try {
      if (window.crypto && typeof window.crypto.randomUUID === 'function') {
        return window.crypto.randomUUID();
      }
    } catch (e) {}
    return 'id_' + Date.now().toString(36) + '_' + Math.random().toString(36).slice(2, 10);
  }

  function isTextNoteSession(session) {
    return !!(session && session.status === 'text');
  }

  function isNotesVisible() {
    var page = $('notesPage');
    return !!(page && !page.hasAttribute('hidden'));
  }

  // ─── Templates (Prompt Presets) ───
  function defaultNotesTemplates() {
    return [
      {
        id: 'summarize',
        name: 'Summarize',
        pinned: true,
        command: null,
        prompt:
          'Your job is to summarize this meeting transcript.\n' +
          '- Keep it concise and high-signal.\n' +
          '- Do not invent details not present.\n' +
          '- Use Markdown.\n' +
          '\n' +
          'Output format:\n' +
          '## TL;DR\n' +
          '- ...\n' +
          '\n' +
          '## Key Points\n' +
          '- ...\n' +
          '\n' +
          '## Action Items\n' +
          '- [ ] Owner: task\n',
      },
      {
        id: 'action_items',
        name: 'Action items',
        pinned: true,
        command: null,
        prompt:
          'Your job is to extract action items from this meeting transcript.\n' +
          '- Do not invent action items.\n' +
          '- Use Markdown.\n' +
          '- Prefer checklists.\n' +
          '\n' +
          'Output format:\n' +
          '## Action Items\n' +
          '- [ ] Owner: task (if unknown, leave Owner blank)\n',
      },
      {
        id: 'followup_email',
        name: 'Follow-up email',
        pinned: true,
        command: null,
        prompt:
          'Write a follow-up email based on this meeting transcript.\n' +
          '- Keep it professional and concise.\n' +
          '- Do not invent facts not in the transcript.\n' +
          '- Use a clear subject line.\n',
      },
      {
        id: 'tldr',
        name: 'Write TLDR',
        pinned: true,
        command: null,
        prompt:
          'Write a brief TLDR of this meeting transcript.\n' +
          '- 3 to 6 bullets.\n' +
          '- Concrete and specific.\n' +
          '- Do not invent details not present.\n',
      },
      {
        id: 'create_linear_ticket',
        name: 'Create Linear Ticket',
        pinned: false,
        command: '/create-linear-ticket',
        prompt:
          "Your job is to help me create a Linear ticket from this meeting transcript (or from explicit '/create-linear-ticket' commands).\n" +
          '- Suggest what ticket should be created, confirm with me, then generate a valid markdown link that opens a pre-filled Linear issue.\n' +
          "- If I invoke '/create-linear-ticket' and provide context, bypass transcript analysis and use my input as the source of truth.\n" +
          '\n' +
          '<URL Schemas>\n' +
          '<Linear>\n' +
          'Base URL: https://linear.new\n' +
          'Parameters:\n' +
          '- title -> issue title (URL-encoded, use + for spaces)\n' +
          '- description -> issue description (markdown supported; URL-encoded; use %0A for line breaks)\n' +
          '- assignee -> UUID, display name, or assignee=me\n' +
          '- priority -> Urgent, High, Medium, Low\n' +
          '- status -> name or UUID of workflow status\n' +
          '- estimate -> point value (e.g. 2, 4, 8)\n' +
          '- labels -> comma-separated labels (URL-encoded if multiple)\n' +
          '- project -> project name or UUID\n' +
          '- cycle -> cycle name, number, or UUID\n' +
          '- links -> URL encoded comma-delimited list of links, with optional titles in format url|title\n' +
          '</Linear>\n' +
          '</URL Schemas>\n' +
          '\n' +
          '<Instructions>\n' +
          'Based on the discussion in this meeting, I need you to help me create a Linear ticket for a customer-reported issue.\n' +
          '\n' +
          '1. Start by suggesting what ticket(s) should be created.\n' +
          '- Usually suggest just one ticket.\n' +
          '- Keep the suggestion short: a one-line title and one sentence of context.\n' +
          '- Ask me if that sounds right.\n' +
          '\n' +
          '2. If I give you feedback, incorporate it and update the suggestion.\n' +
          '\n' +
          "3. Once confirmed, generate a clickable markdown link called 'Create Linear Ticket'.\n" +
          '- Use the Linear URL schema with properly URL-encoded parameters.\n' +
          '- Always include at least a title.\n' +
          '- Include description, labels, priority, assignee, project if obvious from the transcript.\n' +
          '- Leave blank if not clear.\n' +
          '\n' +
          '4. When building the description field, pull in details from the transcript only if they are explicitly present. Use structured sections when possible:\n' +
          '- Customer Impact -> if the transcript names affected customers, number of users, or account value.\n' +
          '- Steps to Reproduce -> if specific steps are mentioned (numbered list).\n' +
          '- Expected vs Actual -> if both outcomes are described.\n' +
          '- Environment -> if browser, device, OS, or version is mentioned.\n' +
          '- Business Impact -> if there is mention of lost revenue, blocked workflow, or severity.\n' +
          '- Workaround -> if a temporary fix is described.\n' +
          '- Links -> if there is a support ticket, Slack thread, or external doc mentioned.\n' +
          '\n' +
          "If these are not in the transcript, do not invent them. Omit the section.\n" +
          '\n' +
          '5. Always return the link as valid markdown.\n' +
          '6. If creating a placeholder issue, mark that clearly in the title.\n' +
          "7. Never use quotation marks, always use ' in their place.\n" +
          '</Instructions>\n',
      },
    ];
  }

  function normalizeNotesTemplates(value) {
    var list = Array.isArray(value) ? value : [];
    var out = [];
    list.forEach(function (t) {
      if (!t) return;
      var id = (t.id || '').toString().trim();
      var name = (t.name || '').toString().trim();
      var prompt = (t.prompt || '').toString();
      if (!id || !name || !prompt) return;
      out.push({
        id: id,
        name: name,
        prompt: prompt,
        command: t.command ? (t.command || '').toString() : null,
        pinned: !!t.pinned,
      });
    });
    return out;
  }

  function ensureTemplatesLoaded() {
    var list = normalizeNotesTemplates(state.templates);
    if (!list.length) list = defaultNotesTemplates();
    state.templates = list;
    if (!state.selectedTemplateId || !state.templates.some(function (t) { return t.id === state.selectedTemplateId; })) {
      state.selectedTemplateId = state.templates[0] ? state.templates[0].id : null;
    }
  }

  function getTemplateById(id) {
    if (!id) return null;
    return (state.templates || []).find(function (t) { return t.id === id; }) || null;
  }

  function getTemplateForCommand(cmd) {
    if (!cmd) return null;
    var c = cmd.trim().toLowerCase();
    return (state.templates || []).find(function (t) {
      return t.command && t.command.toLowerCase() === c;
    }) || null;
  }

  function buildTemplateInitialPrompt(template, opts) {
    opts = opts || {};
    var transcript = (opts.transcript || '').toString();
    var includeTranscript = !!opts.includeTranscript;
    var userContext = (opts.userContext || '').toString().trim();

    var base = (template && template.prompt) ? template.prompt : '';

    if (userContext) {
      base += '\n\nUser-provided context:\n' + userContext + '\n';
    }

    if (!includeTranscript) return base;

    if (base.indexOf('{{transcript}}') !== -1) {
      return base.split('{{transcript}}').join(transcript);
    }

    return base + '\n\nTranscript:\n' + transcript + '\n';
  }

  function renderTemplateChips() {
    var wrap = $('notesTemplateChips');
    if (!wrap) return;

    ensureTemplatesLoaded();

    wrap.innerHTML = '';
    var pinned = (state.templates || []).filter(function (t) { return !!t.pinned; });

    if (!pinned.length) {
      var empty = document.createElement('div');
      empty.className = 'notes-empty-upcoming';
      empty.textContent = 'No pinned templates. Add one in Templates.';
      wrap.appendChild(empty);
      return;
    }

    pinned.forEach(function (t) {
      var btn = document.createElement('button');
      btn.className = 'notes-skill-chip';
      btn.type = 'button';
      btn.dataset.templateId = t.id;
      btn.innerHTML = '<i class=\"fal fa-bolt\"></i> ' + escapeHtml(t.name);
      btn.addEventListener('click', function () {
        runTemplate(t.id);
      });
      wrap.appendChild(btn);
    });
  }

  function openTemplatesModal() {
    ensureTemplatesLoaded();
    state.templatesModalOpen = true;
    var modal = $('notesTemplatesModal');
    if (modal) modal.style.display = 'flex';
    renderTemplatesModalList();
    loadSelectedTemplateIntoEditor();
  }

  function closeTemplatesModal() {
    state.templatesModalOpen = false;
    var modal = $('notesTemplatesModal');
    if (modal) modal.style.display = 'none';
  }

  function renderTemplatesModalList() {
    var list = $('notesTemplatesList');
    if (!list) return;
    ensureTemplatesLoaded();
    list.innerHTML = '';

    state.templates.forEach(function (t) {
      var row = document.createElement('div');
      row.className = 'notes-template-row' + (t.id === state.selectedTemplateId ? ' active' : '');
      row.dataset.templateId = t.id;

      var left = document.createElement('div');
      left.style.minWidth = '0';

      var title = document.createElement('div');
      title.className = 'notes-template-row-title';
      title.textContent = t.name;

      var meta = document.createElement('div');
      meta.className = 'notes-template-row-meta';
      meta.textContent = (t.command ? t.command : (t.pinned ? 'Pinned' : ''));

      left.appendChild(title);
      left.appendChild(meta);

      row.appendChild(left);
      row.addEventListener('click', function () {
        state.selectedTemplateId = t.id;
        renderTemplatesModalList();
        loadSelectedTemplateIntoEditor();
      });

      list.appendChild(row);
    });
  }

  function loadSelectedTemplateIntoEditor() {
    var t = getTemplateById(state.selectedTemplateId);
    var name = $('notesTemplateName');
    var cmd = $('notesTemplateCommand');
    var pinned = $('notesTemplatePinned');
    var prompt = $('notesTemplatePrompt');
    var delBtn = $('notesTemplatesDeleteBtn');

    if (!t) {
      if (name) name.value = '';
      if (cmd) cmd.value = '';
      if (pinned) pinned.checked = false;
      if (prompt) prompt.value = '';
      if (delBtn) delBtn.disabled = true;
      return;
    }

    if (name) name.value = t.name || '';
    if (cmd) cmd.value = t.command || '';
    if (pinned) pinned.checked = !!t.pinned;
    if (prompt) prompt.value = t.prompt || '';
    if (delBtn) delBtn.disabled = false;
  }

  function readTemplateFromEditor() {
    var t = getTemplateById(state.selectedTemplateId);
    if (!t) return null;

    var name = $('notesTemplateName');
    var cmd = $('notesTemplateCommand');
    var pinned = $('notesTemplatePinned');
    var prompt = $('notesTemplatePrompt');

    var next = Object.assign({}, t);
    next.name = (name && name.value ? name.value : '').trim();
    next.command = (cmd && cmd.value ? cmd.value : '').trim() || null;
    next.pinned = !!(pinned && pinned.checked);
    next.prompt = (prompt && prompt.value ? prompt.value : '');

    return next;
  }

  function makeTemplateIdFromName(name) {
    var base = (name || 'template').toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '');
    if (!base) base = 'template';
    var id = base;
    var i = 2;
    while ((state.templates || []).some(function (t) { return t.id === id; })) {
      id = base + '_' + i;
      i += 1;
    }
    return id;
  }

  async function saveTemplatesToSettings() {
    ensureTemplatesLoaded();
    await saveSettingsPatch({ notesTemplates: state.templates });
  }

  async function handleTemplatesSave() {
    var updated = readTemplateFromEditor();
    if (!updated) return;
    if (!updated.name || !updated.prompt) {
      alert('Template requires a name and a prompt.');
      return;
    }

    var idx = state.templates.findIndex(function (t) { return t.id === updated.id; });
    if (idx === -1) return;
    state.templates[idx] = updated;
    await saveTemplatesToSettings();
    renderTemplateChips();
    renderTemplatesModalList();
    loadSelectedTemplateIntoEditor();
  }

  async function handleTemplatesNew() {
    ensureTemplatesLoaded();
    var name = 'New template';
    var id = makeTemplateIdFromName(name);
    state.templates.unshift({
      id: id,
      name: name,
      prompt: 'Describe what you want the agent to do.\n\n{{transcript}}',
      command: null,
      pinned: true,
    });
    state.selectedTemplateId = id;
    await saveTemplatesToSettings();
    renderTemplateChips();
    renderTemplatesModalList();
    loadSelectedTemplateIntoEditor();
  }

  async function handleTemplatesDelete() {
    var t = getTemplateById(state.selectedTemplateId);
    if (!t) return;
    if (!confirm("Delete template '" + t.name + "'?")) return;

    state.templates = (state.templates || []).filter(function (x) { return x.id !== t.id; });
    ensureTemplatesLoaded();
    await saveTemplatesToSettings();
    renderTemplateChips();
    renderTemplatesModalList();
    loadSelectedTemplateIntoEditor();
  }

  async function handleTemplatesReset() {
    if (!confirm('Reset templates back to defaults?')) return;
    state.templates = defaultNotesTemplates();
    state.selectedTemplateId = state.templates[0] ? state.templates[0].id : null;
    await saveTemplatesToSettings();
    renderTemplateChips();
    renderTemplatesModalList();
    loadSelectedTemplateIntoEditor();
  }

  // ─── Calendar (Coming Up) ───
  function monthDayBadge(ts) {
    if (!ts) return '';
    var d = new Date(ts);
    var mon = d.toLocaleString('en-US', { month: 'short' }).toUpperCase();
    return mon + ' ' + d.getDate();
  }

  function formatUpcomingSub(ev) {
    if (!ev) return '';
    if (ev.all_day) {
      return formatDateGroup(ev.startMs) + ' · All day';
    }
    return formatDateGroup(ev.startMs) + ' · ' + formatTimeOfDay(ev.startMs);
  }

  function renderUpcomingEvents() {
    var list = $('notesUpcomingEvents');
    if (!list) return;

    list.innerHTML = '';

    if (!state.calendarEnabled) {
      var off = document.createElement('div');
      off.className = 'notes-empty-upcoming';
      off.textContent = 'Calendar is disabled (open Notes settings to enable).';
      list.appendChild(off);
      return;
    }

    if (state.upcomingLoading) {
      var loading = document.createElement('div');
      loading.className = 'notes-empty-upcoming';
      loading.textContent = 'Loading calendar…';
      list.appendChild(loading);
      return;
    }

    if (state.upcomingError) {
      var row = document.createElement('div');
      row.className = 'notes-upcoming-row notes-upcoming-row-error';
      row.innerHTML =
        '<div class="notes-upcoming-text">' +
          '<div class="notes-upcoming-title">Calendar access needed</div>' +
          '<div class="notes-upcoming-sub">Enable Calendar permissions to show upcoming meetings.</div>' +
          '<button class="notes-upcoming-cta" type="button">Open Settings</button>' +
        '</div>';

      var btn = row.querySelector('.notes-upcoming-cta');
      if (btn) {
        btn.addEventListener('click', function (e) {
          e.preventDefault();
          e.stopPropagation();
          if (!ipcRenderer) return;
          ipcRenderer.invoke('open_external_url', {
            url: 'x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars',
          });
        });
      }

      list.appendChild(row);
      return;
    }

    var events = state.upcomingEvents || [];
    if (!events.length) {
      var empty = document.createElement('div');
      empty.className = 'notes-empty-upcoming';
      empty.textContent = 'No upcoming meetings';
      list.appendChild(empty);
      return;
    }

    events.forEach(function (ev) {
      var row = document.createElement('div');
      row.className = 'notes-upcoming-row';

      var date = document.createElement('div');
      date.className = 'notes-upcoming-date';
      date.textContent = monthDayBadge(ev.startMs);

      var text = document.createElement('div');
      text.className = 'notes-upcoming-text';

      var title = document.createElement('div');
      title.className = 'notes-upcoming-title';
      title.textContent = ev.title || 'Untitled';

      var sub = document.createElement('div');
      sub.className = 'notes-upcoming-sub';
      sub.textContent = formatUpcomingSub(ev);

      text.appendChild(title);
      text.appendChild(sub);

      row.appendChild(date);
      row.appendChild(text);

      if (ev.meeting_url) {
        var join = document.createElement('button');
        join.className = 'notes-upcoming-join';
        join.type = 'button';
        join.title = 'Join meeting';
        join.innerHTML = '<i class="fal fa-external-link"></i>';
        join.addEventListener('click', function (e) {
          e.preventDefault();
          e.stopPropagation();
          if (!ipcRenderer) return;
          ipcRenderer.invoke('open_external_url', { url: ev.meeting_url });
        });
        row.appendChild(join);
      }

      row.addEventListener('click', function () {
        var titleInput = $('notesTitleInput');
        if (titleInput) {
          titleInput.value = ev.title || '';
          try { titleInput.focus(); } catch (e) {}
        }
      });

      list.appendChild(row);
    });
  }

  async function loadUpcomingEvents() {
    if (!ipcRenderer) return;
    if (!state.calendarEnabled) {
      state.upcomingLoading = false;
      state.upcomingError = null;
      state.upcomingEvents = [];
      renderUpcomingEvents();
      return;
    }
    state.upcomingLoading = true;
    state.upcomingError = null;
    renderUpcomingEvents();

    try {
      var selectedIds = [];
      var hasSelectionMap = false;
      try {
        var keys = Object.keys(state.calendarSelected || {});
        hasSelectionMap = keys.length > 0;
        keys.forEach(function (k) {
          if (state.calendarSelected[k]) selectedIds.push(k);
        });
      } catch (e) {}

      var payload = { limit: 10, days: 7 };
      // If the user has explicitly configured a selection, pass it through.
      // Empty array means "no calendars selected" (show nothing).
      if (hasSelectionMap) payload.calendarIds = selectedIds;

      var events = await ipcRenderer.invoke('calendar_get_upcoming_events', payload);
      state.upcomingEvents = Array.isArray(events) ? events : [];
      state.upcomingError = null;
    } catch (err) {
      console.error('[MeetingNotes] calendar_get_upcoming_events failed:', err);
      state.upcomingEvents = [];
      state.upcomingError = err || true;
    } finally {
      state.upcomingLoading = false;
      renderUpcomingEvents();
    }
  }

  function startUpcomingRefresh() {
    if (state.upcomingRefreshTimer) return;
    state.upcomingRefreshTimer = setInterval(function () {
      if (!isNotesVisible()) return;
      loadUpcomingEvents();
    }, 2 * 60 * 1000);
  }

  function stopUpcomingRefresh() {
    if (state.upcomingRefreshTimer) {
      clearInterval(state.upcomingRefreshTimer);
      state.upcomingRefreshTimer = null;
    }
  }

  // ─── Timer ───
  function formatTime(seconds) {
    var h = Math.floor(seconds / 3600);
    var m = Math.floor((seconds % 3600) / 60);
    var s = seconds % 60;
    return String(h).padStart(2, '0') + ':' + String(m).padStart(2, '0') + ':' + String(s).padStart(2, '0');
  }

  function startTimer() {
    stopTimer();
    state.timerSeconds = 0;
    var display = $('notesTimer');
    if (display) display.textContent = formatTime(0);
    state.timerInterval = setInterval(function () {
      state.timerSeconds += 1;
      if (display) display.textContent = formatTime(state.timerSeconds);
    }, 1000);
  }

  function stopTimer() {
    if (state.timerInterval) {
      clearInterval(state.timerInterval);
      state.timerInterval = null;
    }
    state.timerSeconds = 0;
    var display = $('notesTimer');
    if (display) display.textContent = formatTime(0);
  }

  // ─── Model Management ───
  var notesModelDropdown = null;

  function openModelModal() {
    var modal = $('notesModelModal');
    if (modal) modal.style.display = 'flex';
    // Load/refresh the model catalog when opening the modal so the dropdown is populated.
    checkModelStatus();
    // Also fetch the current download/status so the progress UI can attach mid-download.
    fetchWhisperModelStatus();
    syncModalStatus();
    setNotesSettingsTab(state.notesSettingsTab || 'models');
    syncCalendarSettingsUI();
    syncDictationSettingsUI();
  }

  function closeModelModal() {
    var modal = $('notesModelModal');
    if (modal) modal.style.display = 'none';
  }

  // ─── Permission Modal (used to pre-explain scary macOS dialogs) ───
  var permissionModalResolve = null;

  function openPermissionModal(opts) {
    opts = opts || {};
    var modal = $('notesPermissionModal');
    var title = $('notesPermissionTitle');
    var body = $('notesPermissionBody');
    var continueBtn = $('notesPermissionContinueBtn');

    if (title) title.textContent = (opts.title || 'Permission Required').toString();
    if (body) body.textContent = (opts.body || '').toString();
    if (continueBtn) continueBtn.textContent = (opts.continueLabel || 'Continue').toString();

    if (modal) modal.style.display = 'flex';

    // Ensure only one outstanding resolver at a time.
    if (permissionModalResolve) {
      try { permissionModalResolve(false); } catch (e) {}
      permissionModalResolve = null;
    }

    return new Promise(function (resolve) {
      permissionModalResolve = resolve;
    });
  }

  function closePermissionModal(result) {
    var modal = $('notesPermissionModal');
    if (modal) modal.style.display = 'none';
    if (permissionModalResolve) {
      var resolve = permissionModalResolve;
      permissionModalResolve = null;
      try { resolve(!!result); } catch (e) {}
    }
  }

  function setNotesSettingsTab(tab) {
    state.notesSettingsTab = (tab === 'calendar') ? 'calendar' : (tab === 'dictation' ? 'dictation' : 'models');

    var btnModels = $('notesModalTabModels');
    var btnCal = $('notesModalTabCalendar');
    var btnDict = $('notesModalTabDictation');
    var panelModels = $('notesSettingsPanelModels');
    var panelCal = $('notesSettingsPanelCalendar');
    var panelDict = $('notesSettingsPanelDictation');

    if (btnModels) {
      btnModels.classList.toggle('active', state.notesSettingsTab === 'models');
      btnModels.setAttribute('aria-selected', state.notesSettingsTab === 'models' ? 'true' : 'false');
    }
    if (btnCal) {
      btnCal.classList.toggle('active', state.notesSettingsTab === 'calendar');
      btnCal.setAttribute('aria-selected', state.notesSettingsTab === 'calendar' ? 'true' : 'false');
    }
    if (btnDict) {
      btnDict.classList.toggle('active', state.notesSettingsTab === 'dictation');
      btnDict.setAttribute('aria-selected', state.notesSettingsTab === 'dictation' ? 'true' : 'false');
    }
    if (panelModels) panelModels.hidden = state.notesSettingsTab !== 'models';
    if (panelCal) panelCal.hidden = state.notesSettingsTab !== 'calendar';
    if (panelDict) panelDict.hidden = state.notesSettingsTab !== 'dictation';

    if (state.notesSettingsTab === 'calendar') {
      syncCalendarSettingsUI();
      if (state.calendarEnabled) loadCalendarList();
    }
    if (state.notesSettingsTab === 'dictation') {
      syncDictationSettingsUI();
      refreshDictationStatus();
    }
  }

  function syncCalendarSettingsUI() {
    var toggle = $('notesCalendarEnabledToggle');
    var status = $('notesCalendarStatus');
    var picker = $('notesCalendarPicker');

    if (toggle) toggle.checked = !!state.calendarEnabled;
    if (picker) picker.style.display = state.calendarEnabled ? '' : 'none';

    if (!status) return;

    if (!state.calendarEnabled) {
      status.textContent = 'Calendar is disabled.';
      status.className = 'notes-modal-status missing';
      return;
    }
    if (state.calendarListLoading) {
      status.textContent = 'Loading calendars…';
      status.className = 'notes-modal-status downloading';
      return;
    }
    if (state.calendarListError) {
      status.textContent = 'Calendar error. Check permissions.';
      status.className = 'notes-modal-status missing';
      return;
    }

    var selectedCount = 0;
    var keys = Object.keys(state.calendarSelected || {});
    keys.forEach(function (k) {
      if (state.calendarSelected[k]) selectedCount += 1;
    });
    if (!keys.length) {
      status.textContent = 'Enabled · All calendars';
    } else if (!selectedCount) {
      status.textContent = 'Enabled · No calendars selected';
    } else {
      status.textContent = 'Enabled · ' + selectedCount + ' calendars';
    }
    status.className = 'notes-modal-status ready';
  }

  // ─── Dictation Settings ───
  function syncDictationSettingsUI() {
    var enabled = $('notesDictationEnabledToggle');
    if (enabled) enabled.checked = !!state.dictationEnabled;

    var activation = $('notesDictationActivationSelect');
    if (activation) activation.value = state.dictationActivation || 'fn_hold';

    var engine = $('notesDictationEngineSelect');
    if (engine) engine.value = state.dictationEngine || 'local';

    var engineHelp = $('notesDictationEngineHelp');
    if (engineHelp) {
      if (state.dictationEngine === 'chatgpt') {
        if (state.transcriptionAvailable === false) {
          engineHelp.textContent = 'Cloud transcription unavailable. Login to Codex first (Create Tasks mic button uses the same auth).';
        } else {
          engineHelp.textContent = 'Cloud transcription uses ChatGPT (requires Codex login). Audio is sent to ChatGPT for transcription.';
        }
      } else {
        engineHelp.textContent = 'Local runs fully on-device using the selected local model (Whisper or Parakeet). Cloud uses ChatGPT transcription (requires Codex login).';
      }
    }

    var shortcutRow = $('notesDictationShortcutRow');
    if (shortcutRow) shortcutRow.style.display = (state.dictationActivation === 'global_shortcut') ? '' : 'none';

    var shortcutInput = $('notesDictationShortcutInput');
    if (shortcutInput) shortcutInput.value = state.dictationShortcut || 'Option+Space';

    var pasteToggle = $('notesDictationPasteToggle');
    if (pasteToggle) pasteToggle.checked = !!state.dictationPasteIntoInputs;

    var clipToggle = $('notesDictationClipboardFallbackToggle');
    if (clipToggle) clipToggle.checked = !!state.dictationClipboardFallback;

    var restoreToggle = $('notesDictationRestoreClipboardToggle');
    if (restoreToggle) restoreToggle.checked = !!state.dictationRestoreClipboard;

    var flattenToggle = $('notesDictationFlattenToggle');
    if (flattenToggle) flattenToggle.checked = !!state.dictationFlattenNewlinesInSingleLine;

    var cleanupToggle = $('notesDictationCleanupToggle');
    if (cleanupToggle) cleanupToggle.checked = !!state.dictationCleanupEnabled;

    var likeToggle = $('notesDictationCleanupLikeToggle');
    if (likeToggle) {
      likeToggle.checked = !!state.dictationCleanupRemoveLike;
      likeToggle.disabled = !state.dictationCleanupEnabled;
    }

    renderDictationStatus();
  }

  function renderDictationStatus() {
    var el = $('notesDictationStatus');
    if (!el) return;

    var st = state.dictationStatus || null;
    if (!st) {
      el.textContent = 'Dictation status: unknown';
      el.className = 'notes-modal-status downloading';
      return;
    }

    if (!state.dictationEnabled) {
      el.textContent = 'Dictation: Disabled (enable it to use global dictation)';
      el.className = 'notes-modal-status missing';
      var last0 = $('notesDictationLastTranscript');
      if (last0) last0.value = (st.last_transcript || '').toString();
      return;
    }

    var parts = [];
    var mode = (state.dictationActivation === 'global_shortcut')
      ? ('Shortcut: ' + (state.dictationShortcut || 'Option+Space'))
      : (state.dictationActivation === 'fn_double_press' ? 'Fn double-press' : 'Hold Fn');
    parts.push(mode);

    var engineText = (state.dictationEngine === 'chatgpt') ? 'Engine: Cloud' : 'Engine: Local';
    parts.push(engineText);
    if (state.dictationEngine === 'chatgpt' && state.transcriptionAvailable === false) {
      parts.push('Codex login needed');
    }

    if (st.accessibilityTrusted === false) parts.push('Accessibility: needed');
    if (st.fnListenerActive === false && state.dictationActivation !== 'global_shortcut') parts.push('Input Monitoring: needed');

    if (st.state === 'listening') parts.push('Listening...');
    else if (st.state === 'transcribing') parts.push('Transcribing...');
    else if (st.state === 'error') parts.push('Error');
    else parts.push('Ready');

    if (st.error) parts.push(st.error);

    el.textContent = 'Dictation: ' + parts.join(' · ');
    el.className = (st.state === 'error' || st.error) ? 'notes-modal-status missing' : 'notes-modal-status ready';

    var last = $('notesDictationLastTranscript');
    if (last) last.value = (st.last_transcript || '').toString();
  }

  async function refreshDictationStatus() {
    if (!ipcRenderer) return;
    try {
      var st = await ipcRenderer.invoke('dictation_get_status');
      state.dictationStatus = st || null;
    } catch (e) {
      state.dictationStatus = { state: 'error', error: 'Failed to load dictation status' };
    }
    // Keep Cloud engine availability fresh when the modal is open.
    refreshTranscriptionAvailability();
    renderDictationStatus();
  }

  async function refreshTranscriptionAvailability() {
    if (!ipcRenderer) return;
    try {
      state.transcriptionAvailable = await ipcRenderer.invoke('check_transcription_available');
    } catch (e) {
      state.transcriptionAvailable = false;
    }
  }

  async function startDictationNow() {
    if (!ipcRenderer) return;
    try {
      await ipcRenderer.invoke('dictation_start');
    } catch (e) {
      console.error('[MeetingNotes] dictation_start failed:', e);
    }
    refreshDictationStatus();
  }

  async function stopDictationNow() {
    if (!ipcRenderer) return;
    try {
      await ipcRenderer.invoke('dictation_stop');
    } catch (e) {
      console.error('[MeetingNotes] dictation_stop failed:', e);
    }
    refreshDictationStatus();
  }

  async function copyLastDictationTranscript() {
    var st = state.dictationStatus || {};
    var text = (st.last_transcript || '').toString();
    if (!text) return;

    try {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
        await navigator.clipboard.writeText(text);
        return;
      }
    } catch (e) {}

    // Fallback
    var ta = $('notesDictationLastTranscript');
    if (!ta) return;
    try {
      ta.focus();
      ta.select();
      document.execCommand('copy');
    } catch (e) {}
  }

  function onDictationStatus(data) {
    state.dictationStatus = data || null;
    renderDictationStatus();
  }

  function onDictationTranscript(data) {
    // data: { text, outcome, error }
    if (!data) return;
    if (!state.dictationStatus) state.dictationStatus = {};
    state.dictationStatus.last_transcript = data.text;
    if (data.error) state.dictationStatus.error = data.error;
    renderDictationStatus();
  }

  function onDictationOpenSettings(data) {
    // Open the Notes Settings modal on the Dictation tab.
    state.notesSettingsTab = 'dictation';
    openModelModal();
    setNotesSettingsTab('dictation');
    refreshDictationStatus();
  }

  function renderCalendarList() {
    var list = $('notesCalendarList');
    if (!list) return;
    list.innerHTML = '';

    var calendars = state.calendarList || [];
    if (!calendars.length) {
      var empty = document.createElement('div');
      empty.className = 'notes-empty-upcoming';
      empty.textContent = state.calendarEnabled ? 'No calendars found.' : 'Enable calendar to configure.';
      list.appendChild(empty);
      return;
    }

    calendars.forEach(function (c) {
      var row = document.createElement('label');
      row.className = 'notes-calendar-item';

      var left = document.createElement('div');
      left.className = 'notes-calendar-item-left';

      var title = document.createElement('div');
      title.className = 'notes-calendar-item-title';
      title.textContent = c.title || 'Untitled calendar';

      var sub = document.createElement('div');
      sub.className = 'notes-calendar-item-sub';
      sub.textContent = (c.source ? (c.source + ' · ') : '') + (c.allows_modifications ? 'Editable' : 'Read-only');

      left.appendChild(title);
      left.appendChild(sub);

      var cb = document.createElement('input');
      cb.type = 'checkbox';
      cb.checked = !!(state.calendarSelected && state.calendarSelected[c.id]);
      cb.addEventListener('change', function () {
        if (!state.calendarSelected) state.calendarSelected = {};
        state.calendarSelected[c.id] = !!cb.checked;
        saveSettingsPatch({ appleCalendarsSelected: state.calendarSelected }).then(function () {
          syncCalendarSettingsUI();
          loadUpcomingEvents();
        });
      });

      row.appendChild(left);
      row.appendChild(cb);
      list.appendChild(row);
    });
  }

  async function loadCalendarList() {
    if (!ipcRenderer || !state.calendarEnabled) return;
    state.calendarListLoading = true;
    state.calendarListError = null;
    syncCalendarSettingsUI();
    renderCalendarList();

    try {
      var calendars = await ipcRenderer.invoke('calendar_list_calendars');
      state.calendarList = Array.isArray(calendars) ? calendars : [];

      // If no saved selection exists, default to selecting all calendars.
      if (!state.calendarSelected || !Object.keys(state.calendarSelected).length) {
        state.calendarSelected = {};
        state.calendarList.forEach(function (c) { state.calendarSelected[c.id] = true; });
        await saveSettingsPatch({ appleCalendarsSelected: state.calendarSelected });
      }
    } catch (err) {
      console.error('[MeetingNotes] calendar_list_calendars failed:', err);
      state.calendarList = [];
      state.calendarListError = err || true;
    } finally {
      state.calendarListLoading = false;
      syncCalendarSettingsUI();
      renderCalendarList();
    }
  }

  function syncModalStatus() {
    var modalStatus = $('notesModalModelStatus');
    if (modalStatus) {
      if (state.whisperModelStatus && state.whisperModelStatus.state === 'downloading') {
        modalStatus.textContent = 'Downloading…';
        modalStatus.className = 'notes-modal-status downloading';
      } else if (state.whisperModelStatus && state.whisperModelStatus.state === 'error') {
        modalStatus.textContent = 'Download failed: ' + (state.whisperModelStatus.error || 'Unknown error');
        modalStatus.className = 'notes-modal-status missing';
      } else if (state.modelDownloaded) {
        var active = getModelSpec(state.activeModelId);
        var label = active ? active.label : 'Model ready';
        modalStatus.textContent = 'Ready: ' + label;
        modalStatus.className = 'notes-modal-status ready';
      } else {
        modalStatus.textContent = 'No model downloaded';
        modalStatus.className = 'notes-modal-status missing';
      }
    }

    renderModelSelect();
    renderModelDetails();

    var dlBtn = $('notesModelDownloadBtn');
    var delBtn = $('notesModelDeleteBtn');
    var setActiveBtn = $('notesModelSetActiveBtn');
    var cancelBtn = $('notesModelCancelBtn');
    var progressWrap = $('notesModelProgress');

    var selectedId = state.selectedModelId || state.activeModelId;
    var selectedDownloaded = !!(selectedId && state.whisperDownloaded && state.whisperDownloaded[selectedId]);

    if (dlBtn) dlBtn.disabled = state.modelDownloading || selectedDownloaded;
    if (delBtn) delBtn.disabled = state.modelDownloading || !selectedDownloaded;
    if (setActiveBtn) setActiveBtn.disabled = state.modelDownloading || !selectedDownloaded || selectedId === state.activeModelId;
    if (cancelBtn) cancelBtn.disabled = !state.modelDownloading;
    if (cancelBtn) cancelBtn.style.display = state.modelDownloading ? 'inline-flex' : 'none';
    if (progressWrap) progressWrap.style.display = state.modelDownloading ? 'flex' : 'none';
  }

  function getModelSpec(modelId) {
    if (!modelId) return null;
    return (state.whisperModels || []).find(function (m) { return m.id === modelId; }) || null;
  }

  function bytesToHuman(bytes) {
    if (!bytes || bytes <= 0) return '';
    var mb = bytes / (1024 * 1024);
    if (mb < 1024) return mb.toFixed(0) + ' MB';
    return (mb / 1024).toFixed(1) + ' GB';
  }

  function getModelDropdownItems() {
    return (state.whisperModels || []).map(function (m) {
      var downloaded = !!(state.whisperDownloaded && state.whisperDownloaded[m.id]);
      var sizeBytes = (state.whisperSizeBytes && state.whisperSizeBytes[m.id]) || 0;
      var sizeText = sizeBytes ? bytesToHuman(sizeBytes) : (m.approx_size_mb ? (m.approx_size_mb + ' MB') : '');
      var descParts = [];
      if (m.language) descParts.push(m.language);
      if (sizeText) descParts.push(sizeText);
      return {
        value: m.id,
        name: (downloaded ? 'Downloaded · ' : '') + m.label,
        description: descParts.join(' · '),
      };
    });
  }

  function initModelDropdown() {
    if (notesModelDropdown) return;
    var container = $('notesModelDropdown');
    if (!container || !window.CustomDropdown) return;

    notesModelDropdown = new window.CustomDropdown({
      container: container,
      items: getModelDropdownItems(),
      placeholder: 'Select a model…',
      defaultValue: state.selectedModelId || state.activeModelId || '',
      searchable: false,
      portal: true,
      panelClassName: 'notes-model-dropdown-panel',
      onChange: function (value) {
        state.selectedModelId = value;
        syncModalStatus();
        fetchWhisperModelStatus();
      },
    });
  }

  function refreshModelDropdown() {
    if (!notesModelDropdown) return;
    notesModelDropdown.setOptions(getModelDropdownItems());
    var value =
      state.selectedModelId ||
      state.activeModelId ||
      (state.whisperModels && state.whisperModels[0] && state.whisperModels[0].id) ||
      '';
    if (value) {
      notesModelDropdown.setValue(value);
      state.selectedModelId = value;
    }
  }

  function renderModelSelect() {
    if (window.CustomDropdown) {
      initModelDropdown();
      refreshModelDropdown();
      return;
    }

    // Fallback for older builds: use the native <select> if present.
    var select = $('notesModelSelect');
    if (!select) return;

    select.innerHTML = '';
    (state.whisperModels || []).forEach(function (m) {
      var opt = document.createElement('option');
      opt.value = m.id;
      var downloaded = !!(state.whisperDownloaded && state.whisperDownloaded[m.id]);
      opt.textContent = (downloaded ? 'Downloaded · ' : '') + m.label;
      select.appendChild(opt);
    });

    var value = state.selectedModelId || state.activeModelId || (state.whisperModels[0] && state.whisperModels[0].id);
    if (value) {
      select.value = value;
      state.selectedModelId = value;
    }
  }

  function renderModelDetails() {
    var details = $('notesModelDetails');
    if (!details) return;

    var id = state.selectedModelId || state.activeModelId;
    var spec = getModelSpec(id);
    if (!spec) {
      details.textContent = '';
      return;
    }

    var downloaded = !!(state.whisperDownloaded && state.whisperDownloaded[spec.id]);
    var sizeBytes = (state.whisperSizeBytes && state.whisperSizeBytes[spec.id]) || 0;
    var sizeText = sizeBytes ? bytesToHuman(sizeBytes) : (spec.approx_size_mb ? (spec.approx_size_mb + ' MB') : '');

    details.innerHTML =
      '<div class="notes-model-detail-row">' +
        '<div class="notes-model-pill ' + (downloaded ? 'ready' : 'missing') + '">' +
          (downloaded ? 'Downloaded' : 'Not downloaded') +
        '</div>' +
        (spec.language ? '<div class="notes-model-pill">' + escapeHtml(spec.language) + '</div>' : '') +
        (sizeText ? '<div class="notes-model-pill">' + escapeHtml(sizeText) + '</div>' : '') +
        (state.activeModelId === spec.id ? '<div class="notes-model-pill active">Active</div>' : '') +
      '</div>' +
      '<div class="notes-model-detail-sub">' +
        'Models are downloaded from HuggingFace and used locally on-device (Whisper via whisper-rs, Parakeet via ONNX Runtime).' +
      '</div>';

    // Update button label to match selection.
    var dlBtn = $('notesModelDownloadBtn');
    if (dlBtn) {
      dlBtn.innerHTML = '<i class="fal fa-download"></i> Download ' + escapeHtml(spec.label);
    }
  }

  async function checkModelStatus() {
    var badge = $('notesModelStatus');

    if (!ipcRenderer) {
      if (badge) badge.textContent = 'No bridge';
      return;
    }

    try {
      var result = await ipcRenderer.invoke('check_local_asr_model');
      state.whisperModels = (result && result.models) || [];
      state.whisperDownloaded = (result && result.downloaded) || {};
      state.whisperSizeBytes = (result && result.size_bytes) || {};
      state.activeModelId = (result && result.active_model_id) || null;

      if (!state.selectedModelId) state.selectedModelId = state.activeModelId;

      // Consider "model downloaded" true when the active model exists on disk.
      state.modelDownloaded = !!(state.activeModelId && state.whisperDownloaded && state.whisperDownloaded[state.activeModelId]);

      if (badge) {
        var isDownloading = !!(state.whisperModelStatus && state.whisperModelStatus.state === 'downloading');
        if (isDownloading) {
          var pctText = '';
          if (state.whisperModelStatus.progress && state.whisperModelStatus.progress.totalBytes) {
            var pct = Math.round((state.whisperModelStatus.progress.downloadedBytes / state.whisperModelStatus.progress.totalBytes) * 100);
            pctText = ' · ' + pct + '%';
          }
          badge.textContent = 'Downloading' + pctText;
        } else if (state.modelDownloaded) {
          var active = getModelSpec(state.activeModelId);
          badge.textContent = active ? active.label : 'Model ready';
        } else {
          badge.textContent = 'No model';
        }
        badge.classList.toggle('downloading', isDownloading);
        badge.classList.toggle('ready', state.modelDownloaded && !isDownloading);
        badge.classList.toggle('missing', !state.modelDownloaded && !isDownloading);
      }

      syncModalStatus();
      updateControlsUI();
    } catch (err) {
      console.error('[MeetingNotes] check_local_asr_model failed:', err);
      if (badge) badge.textContent = 'Error';
    }
  }

  async function fetchWhisperModelStatus() {
    if (!ipcRenderer) return;
    try {
      var modelId = state.selectedModelId || state.activeModelId;
      var status = await ipcRenderer.invoke('local_asr_model_status', { modelId: modelId });
      onWhisperModelStatus(status);
    } catch (err) {
      // Non-fatal; the older builds won't have this command.
      // Keep console noise low since we also have progress events.
    }
  }

  async function downloadModel() {
    if (!ipcRenderer) return;

    var progressWrap = $('notesModelProgress');
    var progressBar = $('notesModelProgressBar');
    var dlBtn = $('notesModelDownloadBtn');

    if (dlBtn) dlBtn.disabled = true;
    if (progressWrap) progressWrap.style.display = 'flex';
    if (progressBar) {
      progressBar.style.width = '0%';
    }

    try {
      state.modelDownloading = true;
      var modelId = state.selectedModelId || state.activeModelId;
      await ipcRenderer.invoke('download_local_asr_model', { modelId: modelId });
      // Background download will stream status/progress events; we keep the UI open.
      syncModalStatus();
    } catch (err) {
      console.error('[MeetingNotes] download_local_asr_model failed:', err);
      state.modelDownloading = false;
      syncModalStatus();
      if (dlBtn) dlBtn.disabled = false;
    }
  }

  async function cancelModelDownload() {
    if (!ipcRenderer) return;
    try {
      await ipcRenderer.invoke('cancel_local_asr_download');
    } catch (err) {
      console.error('[MeetingNotes] cancel_local_asr_download failed:', err);
    }
  }

  async function setActiveModel() {
    if (!ipcRenderer) return;
    var modelId = state.selectedModelId;
    if (!modelId) return;

    try {
      await ipcRenderer.invoke('set_active_local_asr_model', { modelId: modelId });
      await checkModelStatus();
    } catch (err) {
      console.error('[MeetingNotes] set_active_local_asr_model failed:', err);
    }
  }

  async function deleteModel() {
    if (!ipcRenderer) return;

    try {
      var modelId = state.selectedModelId || state.activeModelId;
      await ipcRenderer.invoke('delete_local_asr_model', { modelId: modelId });
      await checkModelStatus();
    } catch (err) {
      console.error('[MeetingNotes] delete_local_asr_model failed:', err);
    }
  }

  // ─── Recording Controls ───
  async function startRecording() {
    if (!ipcRenderer) return;
    if (!state.modelDownloaded) {
      openModelModal();
      return;
    }

    var titleInput = $('notesTitleInput');
    var micCheckbox = $('notesCaptureMic');
    var systemCheckbox = $('notesCaptureSystem');
    var title = titleInput ? titleInput.value.trim() : '';
    var captureMic = micCheckbox ? micCheckbox.checked : true;
    var captureSystem = systemCheckbox ? systemCheckbox.checked : false;

    try {
      var result = await ipcRenderer.invoke('meeting_start', {
        title: title,
        captureMic: captureMic,
        captureSystem: captureSystem,
      });
      state.recording = true;
      state.paused = false;
      state.sessionId = (typeof result === 'string') ? result : ((result && result.session_id) || null);
      state.segments = [];
      clearTranscript();

      // If the user has a folder selected, auto-file this recording there.
      if (state.sessionId && state.selectedFolderId) {
        state.sessionFolderMap[state.sessionId] = state.selectedFolderId;
        saveFolders();
        renderFolderList();
      }

      // Switch to transcript view when recording starts
      switchView('transcript', null);
      // Allow editing title immediately for the active session.
      state.selectedSessionId = state.sessionId;
      setTranscriptTitleInput(title || '');
      setSummaryPanelVisible(false);
      setSummaryStatus('Ready', 'ready');
      setSummaryContent('', false);

      startTimer();
      updateControlsUI();
      refreshTranscriptFolderDropdown();
    } catch (err) {
      console.error('[MeetingNotes] meeting_start failed:', err);
    }
  }

  async function pauseRecording() {
    if (!ipcRenderer || !state.recording) return;

    try {
      await ipcRenderer.invoke('meeting_pause');
      state.paused = true;
      if (state.timerInterval) {
        clearInterval(state.timerInterval);
        state.timerInterval = null;
      }
      updateControlsUI();
    } catch (err) {
      console.error('[MeetingNotes] meeting_pause failed:', err);
    }
  }

  async function resumeRecording() {
    if (!ipcRenderer || !state.recording || !state.paused) return;

    try {
      await ipcRenderer.invoke('meeting_resume');
      state.paused = false;
      var display = $('notesTimer');
      state.timerInterval = setInterval(function () {
        state.timerSeconds += 1;
        if (display) display.textContent = formatTime(state.timerSeconds);
      }, 1000);
      updateControlsUI();
    } catch (err) {
      console.error('[MeetingNotes] meeting_resume failed:', err);
    }
  }

  async function stopRecording() {
    if (!ipcRenderer || !state.recording) return;

    try {
      await ipcRenderer.invoke('meeting_stop');
      state.recording = false;
      state.paused = false;
      var stoppedSessionId = state.sessionId;
      state.sessionId = null;
      stopTimer();
      updateControlsUI();
      await loadSessions();

      // Stay on transcript if we have a session, otherwise go back to default
      if (stoppedSessionId) {
        switchView('transcript', stoppedSessionId);
      } else {
        switchView('default');
      }
    } catch (err) {
      console.error('[MeetingNotes] meeting_stop failed:', err);
    }
  }

  function updateControlsUI() {
    var startBtn = $('notesStartBtn');
    var pauseBtn = $('notesPauseBtn');
    var resumeBtn = $('notesResumeBtn');
    var stopBtn = $('notesStopBtn');
    var indicator = $('notesRecordingIndicator');
    var titleInput = $('notesTitleInput');
    var micCheckbox = $('notesCaptureMic');
    var systemCheckbox = $('notesCaptureSystem');

    var canStart = !state.recording;
    var isRecording = state.recording && !state.paused;
    var isPaused = state.recording && state.paused;

    if (startBtn) {
      startBtn.disabled = !canStart;
      // While recording, keep the UI focused on Pause/Stop. The recording state
      // is shown on the right (REC + timer), so we avoid a large center pill.
      startBtn.classList.toggle('hidden', !!state.recording);

      if (!state.recording) {
        if (state.modelDownloaded) {
          startBtn.title = 'Start Recording';
          startBtn.setAttribute('aria-label', 'Start recording');
          startBtn.innerHTML = '<span class="notes-record-btn-dot" aria-hidden="true"></span>';
          startBtn.classList.remove('needs-model');
        } else {
          startBtn.title = 'Download a local model to start';
          startBtn.setAttribute('aria-label', 'Download model to enable recording');
          startBtn.innerHTML = '<i class="fal fa-download notes-record-btn-aux-icon" aria-hidden="true"></i>';
          startBtn.classList.add('needs-model');
        }
      }
    }

    if (pauseBtn) {
      pauseBtn.disabled = !isRecording;
      pauseBtn.classList.toggle('hidden', !isRecording);
    }
    if (resumeBtn) {
      resumeBtn.disabled = !isPaused;
      resumeBtn.classList.toggle('hidden', !isPaused);
    }
    if (stopBtn) {
      stopBtn.disabled = !state.recording;
      stopBtn.classList.toggle('hidden', !state.recording);
    }

    if (titleInput) titleInput.disabled = state.recording;
    if (micCheckbox) micCheckbox.disabled = state.recording;
    if (systemCheckbox) systemCheckbox.disabled = state.recording;

    if (indicator) {
      indicator.classList.toggle('recording', isRecording);
      indicator.classList.toggle('paused', isPaused);
      indicator.classList.toggle('hidden', !state.recording);
    }

    // Export buttons depend on a selected session
    var hasSession = !!state.selectedSessionId;
    var exportTxt = $('notesExportTxt');
    var exportMd = $('notesExportMd');
    var exportJson = $('notesExportJson');
    var copyBtn = $('notesCopyBtn');
    var deleteBtn = $('notesDeleteBtn');
    if (exportTxt) exportTxt.disabled = !hasSession;
    if (exportMd) exportMd.disabled = !hasSession;
    if (exportJson) exportJson.disabled = !hasSession;
    if (copyBtn) copyBtn.disabled = !hasSession;
    if (deleteBtn) deleteBtn.disabled = !hasSession;
  }

  // ─── View Switching ───
  function switchView(view, sessionId) {
    // Avoid losing edits when switching away from a text note.
    flushTextNoteSaveNow();

    state.currentView = view;
    var defaultView = $('notesCenterDefault');
    var transcriptView = $('notesCenterTranscript');

    if (view === 'transcript') {
      if (defaultView) defaultView.style.display = 'none';
      if (transcriptView) transcriptView.style.display = 'flex';

      if (sessionId) {
        selectSession(sessionId);
      }
    } else {
      if (defaultView) defaultView.style.display = '';
      if (transcriptView) transcriptView.style.display = 'none';
      state.selectedSessionId = null;
      state.chatContextSent = false;
      loadChatThreadsForSession(null);
      updateControlsUI();
    }
  }

  // ─── Live Transcript ───
  function renderSegment(segment) {
    var ts = segment.timestamp;
    if (ts === undefined || ts === null) {
      if (segment.start_ms !== undefined && segment.start_ms !== null) {
        ts = (segment.start_ms || 0) / 1000.0;
      } else {
        ts = 0;
      }
    }
    var totalSeconds = Math.floor(ts || 0);
    var minutes = Math.floor(totalSeconds / 60);
    var seconds = totalSeconds % 60;
    var timeStr = String(minutes).padStart(2, '0') + ':' + String(seconds).padStart(2, '0');

    var el = document.createElement('div');
    el.className = 'notes-segment';

    var timeSpan = document.createElement('span');
    timeSpan.className = 'notes-segment-time';
    timeSpan.textContent = '[' + timeStr + ']';

    var textSpan = document.createElement('span');
    textSpan.className = 'notes-segment-text';
    textSpan.textContent = segment.text || '';

    el.appendChild(timeSpan);
    el.appendChild(textSpan);
    return el;
  }

  function clearTranscript() {
    var area = $('notesTranscript');
    if (area) area.innerHTML = '';
    state.segments = [];
  }

  function appendSegment(segment) {
    state.segments.push(segment);
    var area = $('notesTranscript');
    if (!area) return;

    // Remove empty state placeholder if present
    var empty = area.querySelector('.notes-transcript-empty');
    if (empty) empty.remove();

    area.appendChild(renderSegment(segment));
    area.scrollTop = area.scrollHeight;
  }

  // ─── Date-Grouped Session Rendering ───
  function formatDateGroup(timestamp) {
    if (!timestamp) return 'Unknown';
    var date = new Date(timestamp);
    var now = new Date();
    var today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    var yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    var sessionDate = new Date(date.getFullYear(), date.getMonth(), date.getDate());

    if (sessionDate.getTime() === today.getTime()) return 'Today';
    if (sessionDate.getTime() === yesterday.getTime()) return 'Yesterday';

    var days = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
    var months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
    return days[date.getDay()] + ', ' + months[date.getMonth()] + ' ' + date.getDate();
  }

  function formatTimeOfDay(timestamp) {
    if (!timestamp) return '';
    var d = new Date(timestamp);
    var h = d.getHours();
    var m = d.getMinutes();
    var ampm = h >= 12 ? 'PM' : 'AM';
    h = h % 12 || 12;
    return h + ':' + String(m).padStart(2, '0') + ' ' + ampm;
  }

  function formatDuration(totalSeconds) {
    if (!totalSeconds || totalSeconds <= 0) return '';
    var m = Math.floor(totalSeconds / 60);
    var s = totalSeconds % 60;
    if (m > 0 && s > 0) return m + 'm ' + s + 's';
    if (m > 0) return m + 'm';
    return s + 's';
  }

  function renderDateGroupedSessions(sessions) {
    var container = $('notesPastMeetings');
    if (!container) return;
    container.innerHTML = '';

    var filtered = filterSessionsList(sessions);

    if (!filtered.length) {
      var empty = document.createElement('div');
      empty.className = 'notes-empty-sessions';
      empty.innerHTML = '<i class="fal fa-sticky-note"></i><p>No notes yet</p><div class="notes-empty-sub">Click + to create a text note, or start recording.</div>';
      container.appendChild(empty);
      return;
    }

    // Group by date
    var groups = {};
    var groupOrder = [];
    filtered.forEach(function (session) {
      var key = formatDateGroup(session.created_at);
      if (!groups[key]) {
        groups[key] = [];
        groupOrder.push(key);
      }
      groups[key].push(session);
    });

    groupOrder.forEach(function (key) {
      var group = document.createElement('div');
      group.className = 'notes-date-group';

      var heading = document.createElement('h6');
      heading.className = 'notes-date-group-heading';
      heading.textContent = key;
      group.appendChild(heading);

      groups[key].forEach(function (session) {
        group.appendChild(createMeetingRow(session));
      });

      container.appendChild(group);
    });
  }

  function createMeetingRow(session) {
    var row = document.createElement('div');
    row.className = 'notes-meeting-row';
    row.dataset.sessionId = session.id;

    var icon = document.createElement('div');
    icon.className = 'notes-meeting-icon';
    icon.innerHTML = isTextNoteSession(session)
      ? '<i class="fal fa-sticky-note"></i>'
      : '<i class="fal fa-microphone-alt"></i>';

    var info = document.createElement('div');
    info.className = 'notes-meeting-info';

    var title = document.createElement('div');
    title.className = 'notes-meeting-title';
    title.textContent = session.title || 'Untitled';

    var meta = document.createElement('div');
    meta.className = 'notes-meeting-meta';
    var duration = formatDuration(session.duration || 0);
    if (duration) {
      var durationSpan = document.createElement('span');
      durationSpan.textContent = duration;
      meta.appendChild(durationSpan);
    }
    if (isTextNoteSession(session)) {
      var kindSpan = document.createElement('span');
      kindSpan.textContent = 'Text note';
      meta.appendChild(kindSpan);
    }
    var folderName = getFolderForSession(session.id);
    if (folderName) {
      var folderSpan = document.createElement('span');
      folderSpan.textContent = folderName;
      meta.appendChild(folderSpan);
    }

    info.appendChild(title);
    info.appendChild(meta);

    var time = document.createElement('span');
    time.className = 'notes-meeting-time';
    time.textContent = formatTimeOfDay(session.created_at);

    var deleteBtn = document.createElement('button');
    deleteBtn.className = 'notes-meeting-delete';
    deleteBtn.title = isTextNoteSession(session) ? 'Delete note' : 'Delete session';
    deleteBtn.innerHTML = '<i class="fal fa-trash-alt"></i>';
    deleteBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      deleteSession(session.id);
    });

    var right = document.createElement('div');
    right.className = 'notes-meeting-right';
    right.appendChild(time);
    right.appendChild(deleteBtn);

    row.addEventListener('contextmenu', function (e) {
      e.preventDefault();
      openMoveToFolderMenu(session.id, e.clientX, e.clientY);
    });

    row.appendChild(icon);
    row.appendChild(info);
    row.appendChild(right);

    row.addEventListener('click', function () {
      switchView('transcript', session.id);
    });

    return row;
  }

  // ─── Session Loading ───
  async function loadSessions() {
    if (!ipcRenderer) return;

    try {
      var result = await ipcRenderer.invoke('meeting_list_sessions');
      state.sessions = Array.isArray(result) ? result : [];
      renderDateGroupedSessions(state.sessions);
    } catch (err) {
      console.error('[MeetingNotes] meeting_list_sessions failed:', err);
      state.sessions = [];
      renderDateGroupedSessions([]);
    }
  }

  async function selectSession(sessionId) {
    state.selectedSessionId = sessionId;
    state.chatContextSent = false;
    updateControlsUI();
    loadChatThreadsForSession(sessionId);
    refreshTranscriptFolderDropdown();
    var session = (state.sessions || []).find(function (s) { return s.id === sessionId; }) || null;

    // Update transcript meta
    var metaEl = $('notesTranscriptMeta');
    if (metaEl) {
      if (session) {
        metaEl.innerHTML = '';

        setTranscriptTitleInput(session.title || '');

        if (session.created_at) {
          var dateTag = document.createElement('span');
          dateTag.className = 'notes-meta-tag';
          dateTag.innerHTML = '<i class="fal fa-calendar"></i> ' + escapeHtml(formatDateGroup(session.created_at) + ' ' + formatTimeOfDay(session.created_at));
          metaEl.appendChild(dateTag);
        }
        if (isTextNoteSession(session)) {
          var kindTag = document.createElement('span');
          kindTag.className = 'notes-meta-tag';
          kindTag.innerHTML = '<i class="fal fa-sticky-note"></i> Text note';
          metaEl.appendChild(kindTag);
        }
        if (session.duration) {
          var durTag = document.createElement('span');
          durTag.className = 'notes-meta-tag';
          durTag.innerHTML = '<i class="fal fa-clock"></i> ' + escapeHtml(formatDuration(session.duration));
          metaEl.appendChild(durTag);
        }

        var folderName = getFolderForSession(sessionId);
        if (folderName) {
          var folderTag = document.createElement('span');
          folderTag.className = 'notes-meta-tag';
          folderTag.innerHTML = '<i class="fal fa-folder"></i> ' + escapeHtml(folderName);
          metaEl.appendChild(folderTag);
        }
      }
      if (!session) {
        metaEl.innerHTML = '';
        setTranscriptTitleInput('');
      }
    }

    // Reset summary UI for the newly selected session (show cached if present).
    if (state.summarySessionId && state.summarySessionId !== sessionId) {
      // Stop listening for the previous summary task; it can keep running quietly.
      state.summaryTaskId = null;
      state.summaryGenerating = false;
      state.summaryStreamingText = '';
    }
    state.summarySessionId = sessionId;
    var cached = loadCachedSummary(sessionId);
    if (cached && cached.trim()) {
      setSummaryPanelVisible(true);
      setSummaryStatus('Ready', 'ready');
      setSummaryContent(cached, true);
    } else {
      setSummaryPanelVisible(false);
      setSummaryStatus('Ready', 'ready');
      setSummaryContent('', false);
    }

    if (!ipcRenderer || !sessionId) return;

    try {
      var result = await ipcRenderer.invoke('meeting_get_transcript', { sessionId: sessionId });
      var segments = [];
      if (Array.isArray(result)) {
        segments = result;
      } else if (result && Array.isArray(result.segments)) {
        segments = result.segments;
      }
      renderSavedTranscript(segments, session);
    } catch (err) {
      console.error('[MeetingNotes] meeting_get_transcript failed:', err);
    }
  }

  function renderSavedTranscript(segments, session) {
    var area = $('notesTranscript');
    var editor = $('notesTextNoteEditor');
    var isText = isTextNoteSession(session || getSelectedSession());

    if (editor) editor.style.display = isText ? 'block' : 'none';
    if (area) area.style.display = isText ? 'none' : '';

    if (isText) {
      var list = Array.isArray(segments) ? segments : [];
      state.segments = list;
      if (editor) {
        state.textNoteSuppressSave = true;
        editor.value = list.map(function (s) { return (s && s.text) ? s.text : ''; }).join('\n');
        state.textNoteLastSavedText = editor.value;
        setTimeout(function () { state.textNoteSuppressSave = false; }, 0);
      }
      return;
    }

    if (!area) return;
    area.innerHTML = '';

    if (!segments.length) {
      var empty = document.createElement('div');
      empty.className = 'notes-transcript-empty';
      empty.innerHTML = '<i class="fal fa-microphone-alt"></i><p>No transcript data for this session.</p>';
      area.appendChild(empty);
      return;
    }

    state.segments = segments;
    segments.forEach(function (segment) {
      area.appendChild(renderSegment(segment));
    });
  }

  async function deleteSession(sessionId) {
    if (!ipcRenderer || !sessionId) return;
    if (!confirm('Delete this session? This cannot be undone.')) return;

    try {
      await ipcRenderer.invoke('meeting_delete_session', { sessionId: sessionId });
      try {
        localStorage.removeItem(getChatThreadsKey(sessionId));
        localStorage.removeItem(getChatActiveThreadKey(sessionId));
      } catch (e) {}
      delete state.sessionFolderMap[sessionId];
      saveFolders();
      renderFolderList();
      if (state.selectedSessionId === sessionId) {
        state.selectedSessionId = null;
        switchView('default');
      }
      await loadSessions();
    } catch (err) {
      console.error('[MeetingNotes] meeting_delete_session failed:', err);
    }
  }

  // ─── Search & Folder Filtering ───
  function filterSessionsList(sessions) {
    var query = state.searchQuery.toLowerCase();
    var folderId = state.selectedFolderId;

    return sessions.filter(function (s) {
      // Search filter
      if (query) {
        var title = (s.title || '').toLowerCase();
        if (title.indexOf(query) === -1) return false;
      }
      // Folder filter
      if (folderId) {
        var mapped = state.sessionFolderMap[s.id];
        if (mapped !== folderId) return false;
      }
      return true;
    });
  }

  function onSearchInput() {
    var input = $('notesSearchInput');
    state.searchQuery = input ? input.value : '';
    renderDateGroupedSessions(state.sessions);
  }

  // ─── Cmd+K Search Palette ───
  function openSearchPalette(initialQuery) {
    if (!isNotesVisible()) return;

    var overlay = $('notesSearchPalette');
    var input = $('notesPaletteInput');
    if (!overlay || !input) return;

    state.paletteLastFocus = document.activeElement;
    state.paletteOpen = true;
    overlay.hidden = false;

    var query = typeof initialQuery === 'string' ? initialQuery : '';
    if (!query) {
      var sidebarInput = $('notesSearchInput');
      if (sidebarInput && sidebarInput.value) query = sidebarInput.value;
    }

    state.paletteQuery = query;
    input.value = query;
    state.paletteActiveIndex = 0;

    renderPaletteResults();

    // Focus after paint so the input reliably receives caret.
    setTimeout(function () {
      try { input.focus(); } catch (e) {}
      input.setSelectionRange(input.value.length, input.value.length);
    }, 0);
  }

  function closeSearchPalette() {
    var overlay = $('notesSearchPalette');
    if (overlay) overlay.hidden = true;

    state.paletteOpen = false;
    state.paletteItemsFlat = [];
    state.paletteActiveIndex = 0;

    // Restore focus if we can.
    var last = state.paletteLastFocus;
    state.paletteLastFocus = null;
    if (last && typeof last.focus === 'function') {
      try { last.focus(); } catch (e) {}
    }
  }

  function paletteNormalize(str) {
    return (str || '').toString().trim().toLowerCase();
  }

  function paletteMakeGroups() {
    var q = paletteNormalize(state.paletteQuery);

    var folders = (state.folders || []).filter(function (f) {
      if (!q) return true;
      return paletteNormalize(f.name).indexOf(q) !== -1;
    }).slice(0, q ? 12 : 6);

    var sessions = (state.sessions || []).slice().sort(function (a, b) {
      return (b.created_at || 0) - (a.created_at || 0);
    }).filter(function (s) {
      if (!q) return true;
      var title = paletteNormalize(s.title);
      if (title.indexOf(q) !== -1) return true;
      var folderName = paletteNormalize(getFolderForSession(s.id));
      if (folderName && folderName.indexOf(q) !== -1) return true;
      return false;
    }).slice(0, q ? 20 : 8);

    var groups = [];
    if (folders.length) groups.push({ title: 'Folders', kind: 'folder', items: folders });
    if (sessions.length) groups.push({ title: 'Notes', kind: 'session', items: sessions });
    return groups;
  }

  function setPaletteActive(index) {
    var results = $('notesPaletteResults');
    if (!results) return;

    var items = results.querySelectorAll('.notes-palette-item[data-index]');
    if (!items.length) {
      state.paletteActiveIndex = 0;
      return;
    }

    var idx = index;
    if (idx < 0) idx = items.length - 1;
    if (idx >= items.length) idx = 0;
    state.paletteActiveIndex = idx;

    items.forEach(function (el) { el.classList.remove('active'); });
    var active = items[idx];
    if (active) {
      active.classList.add('active');
      try { active.scrollIntoView({ block: 'nearest' }); } catch (e) {}
    }
  }

  function activatePaletteSelection() {
    var item = state.paletteItemsFlat[state.paletteActiveIndex];
    if (!item) return;

    if (item.type === 'folder') {
      state.selectedFolderId = item.id;
      renderFolderList();
      renderDateGroupedSessions(state.sessions);
      closeSearchPalette();
      return;
    }

    if (item.type === 'session') {
      closeSearchPalette();
      switchView('transcript', item.id);
      return;
    }
  }

  function renderPaletteResults() {
    var results = $('notesPaletteResults');
    if (!results) return;

    var groups = paletteMakeGroups();
    state.paletteItemsFlat = [];
    results.innerHTML = '';

    if (!groups.length) {
      var empty = document.createElement('div');
      empty.className = 'notes-palette-empty';
      empty.innerHTML = '<div class="notes-palette-empty-title">No results</div><div class="notes-palette-empty-sub">Try a different search.</div>';
      results.appendChild(empty);
      return;
    }

    var flatIndex = 0;
    groups.forEach(function (g) {
      var groupEl = document.createElement('div');
      groupEl.className = 'notes-palette-group';

      var heading = document.createElement('div');
      heading.className = 'notes-palette-group-title';
      heading.textContent = g.title;
      groupEl.appendChild(heading);

      g.items.forEach(function (it) {
        var row = document.createElement('div');
        row.className = 'notes-palette-item';
        row.setAttribute('role', 'option');
        row.dataset.index = String(flatIndex);

        if (g.kind === 'folder') {
          state.paletteItemsFlat.push({ type: 'folder', id: it.id });
          row.innerHTML =
            '<div class="notes-palette-item-icon"><i class="fal fa-folder"></i></div>' +
            '<div class="notes-palette-item-text">' +
              '<div class="notes-palette-item-title">' + escapeHtml(it.name) + '</div>' +
              '<div class="notes-palette-item-sub">Private</div>' +
            '</div>' +
            '<div class="notes-palette-item-meta">Open</div>';
        } else {
          state.paletteItemsFlat.push({ type: 'session', id: it.id });
          var subBits = [];
          if (it.created_at) subBits.push(formatDateGroup(it.created_at) + ' ' + formatTimeOfDay(it.created_at));
          var folder = getFolderForSession(it.id);
          if (folder) subBits.push(folder);
          var sub = subBits.join(' · ');

          row.innerHTML =
            '<div class="notes-palette-item-icon meeting"><i class="fal fa-microphone-alt"></i></div>' +
            '<div class="notes-palette-item-text">' +
              '<div class="notes-palette-item-title">' + escapeHtml(it.title || 'Untitled') + '</div>' +
              '<div class="notes-palette-item-sub">' + escapeHtml(sub) + '</div>' +
            '</div>' +
            '<div class="notes-palette-item-meta">' + escapeHtml(formatDuration(it.duration || 0) || '') + '</div>';
        }

        row.addEventListener('mouseenter', function () {
          setPaletteActive(Number(row.dataset.index || 0));
        });
        row.addEventListener('click', function () {
          setPaletteActive(Number(row.dataset.index || 0));
          activatePaletteSelection();
        });

        groupEl.appendChild(row);
        flatIndex += 1;
      });

      results.appendChild(groupEl);
    });

    setPaletteActive(state.paletteActiveIndex);
  }

  // ─── Folders (localStorage) ───
  function loadFolders() {
    try {
      var data = localStorage.getItem('phantom-notes-folders');
      state.folders = data ? JSON.parse(data) : [];
    } catch (e) {
      state.folders = [];
    }
    try {
      var map = localStorage.getItem('phantom-notes-session-folders');
      state.sessionFolderMap = map ? JSON.parse(map) : {};
    } catch (e) {
      state.sessionFolderMap = {};
    }
  }

  function saveFolders() {
    localStorage.setItem('phantom-notes-folders', JSON.stringify(state.folders));
    localStorage.setItem('phantom-notes-session-folders', JSON.stringify(state.sessionFolderMap));
  }

  function createFolder(name) {
    if (!name) return;
    var id = 'folder-' + Date.now();
    state.folders.push({ id: id, name: name });
    saveFolders();
    renderFolderList();
    refreshTranscriptFolderDropdown();
  }

  function getFolderForSession(sessionId) {
    var folderId = state.sessionFolderMap[sessionId];
    if (!folderId) return null;
    var folder = state.folders.find(function (f) { return f.id === folderId; });
    return folder ? folder.name : null;
  }

  function renderFolderList() {
    var list = $('notesFolderList');
    if (!list) return;
    list.innerHTML = '';

    state.folders.forEach(function (folder) {
      var item = document.createElement('div');
      item.className = 'notes-folder-item';
      if (folder.id === state.selectedFolderId) item.classList.add('active');
      item.innerHTML = '<i class="fal fa-folder"></i> ' + escapeHtml(folder.name);
      item.addEventListener('click', function () {
        if (state.selectedFolderId === folder.id) {
          state.selectedFolderId = null;
        } else {
          state.selectedFolderId = folder.id;
        }
        renderFolderList();
        renderDateGroupedSessions(state.sessions);
      });

      list.appendChild(item);
    });
  }

  function closeInlineFolderCreator() {
    var list = $('notesFolderList');
    if (!list) return;
    var existing = list.querySelector('.notes-folder-create-row');
    if (existing) existing.remove();
  }

  function openInlineFolderCreator() {
    var list = $('notesFolderList');
    if (!list) return;

    var existing = list.querySelector('.notes-folder-create-row');
    if (existing) {
      var input = existing.querySelector('input');
      if (input) input.focus();
      return;
    }

    var row = document.createElement('div');
    row.className = 'notes-folder-create-row';
    row.innerHTML =
      '<i class="fal fa-folder-plus notes-folder-create-icon"></i>' +
      '<input class="notes-folder-create-input" type="text" placeholder="New folder name" maxlength="64" />' +
      '<button class="notes-folder-create-btn ok" type="button" title="Create folder"><i class="fal fa-check"></i></button>' +
      '<button class="notes-folder-create-btn cancel" type="button" title="Cancel"><i class="fal fa-times"></i></button>';

    list.insertBefore(row, list.firstChild);

    var input = row.querySelector('.notes-folder-create-input');
    var okBtn = row.querySelector('.notes-folder-create-btn.ok');
    var cancelBtn = row.querySelector('.notes-folder-create-btn.cancel');

    function submit() {
      var name = (input && input.value ? input.value : '').trim();
      if (!name) return;
      closeInlineFolderCreator();
      createFolder(name);
    }

    if (okBtn) okBtn.addEventListener('click', submit);
    if (cancelBtn) cancelBtn.addEventListener('click', closeInlineFolderCreator);
    if (input) {
      input.addEventListener('keydown', function (e) {
        if (e.key === 'Enter') {
          e.preventDefault();
          submit();
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          closeInlineFolderCreator();
        }
      });
      setTimeout(function () { try { input.focus(); } catch (e) {} }, 0);
    }

    // Click outside closes the inline creator.
    setTimeout(function () {
      function onDocDown(e) {
        if (!row.contains(e.target)) {
          closeInlineFolderCreator();
          document.removeEventListener('mousedown', onDocDown, true);
        }
      }
      document.addEventListener('mousedown', onDocDown, true);
    }, 0);
  }

  // ─── Folder Context Menu (Move To...) ───
  var notesMoveMenuEl = null;
  var notesMoveMenuBackdrop = null;

  function closeMoveToFolderMenu() {
    if (notesMoveMenuEl && notesMoveMenuEl.parentNode) notesMoveMenuEl.parentNode.removeChild(notesMoveMenuEl);
    if (notesMoveMenuBackdrop && notesMoveMenuBackdrop.parentNode) notesMoveMenuBackdrop.parentNode.removeChild(notesMoveMenuBackdrop);
    notesMoveMenuEl = null;
    notesMoveMenuBackdrop = null;
  }

  function openMoveToFolderMenu(sessionId, x, y) {
    closeMoveToFolderMenu();

    var backdrop = document.createElement('div');
    backdrop.className = 'notes-context-backdrop';
    backdrop.addEventListener('click', closeMoveToFolderMenu);
    document.body.appendChild(backdrop);
    notesMoveMenuBackdrop = backdrop;

    var menu = document.createElement('div');
    menu.className = 'notes-context-menu';

    var title = document.createElement('div');
    title.className = 'notes-context-title';
    title.textContent = 'Move to folder';
    menu.appendChild(title);

    var currentFolderId = state.sessionFolderMap[sessionId] || '';

    function addItem(label, folderId) {
      var btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'notes-context-item' + ((folderId || '') === (currentFolderId || '') ? ' active' : '');
      btn.innerHTML = '<span class="notes-context-label">' + escapeHtml(label) + '</span>' +
        (((folderId || '') === (currentFolderId || '')) ? '<i class="fal fa-check"></i>' : '');
      btn.addEventListener('click', function () {
        moveSessionToFolder(sessionId, folderId || null);
        closeMoveToFolderMenu();
      });
      menu.appendChild(btn);
    }

    addItem('No folder', '');
    (state.folders || []).forEach(function (f) {
      addItem(f.name, f.id);
    });

    document.body.appendChild(menu);
    notesMoveMenuEl = menu;

    // Position within viewport
    var vw = window.innerWidth || document.documentElement.clientWidth;
    var vh = window.innerHeight || document.documentElement.clientHeight;
    var rect = menu.getBoundingClientRect();
    var left = Math.min(x, vw - rect.width - 12);
    var top = Math.min(y, vh - rect.height - 12);
    left = Math.max(12, left);
    top = Math.max(12, top);

    menu.style.left = left + 'px';
    menu.style.top = top + 'px';

    // Escape closes
    setTimeout(function () {
      function onKey(e) {
        if (e.key === 'Escape') {
          e.preventDefault();
          closeMoveToFolderMenu();
          document.removeEventListener('keydown', onKey, true);
        }
      }
      document.addEventListener('keydown', onKey, true);
    }, 0);
  }

  function getFolderDropdownItems() {
    var items = [{ value: '', name: 'No folder', description: '' }];
    (state.folders || []).forEach(function (f) {
      items.push({ value: f.id, name: f.name, description: '' });
    });
    return items;
  }

  function initTranscriptFolderDropdown() {
    if (notesTranscriptFolderDropdown) return;
    var container = $('notesTranscriptFolderDropdown');
    if (!container || !window.CustomDropdown) return;

    notesTranscriptFolderDropdown = new window.CustomDropdown({
      container: container,
      items: getFolderDropdownItems(),
      placeholder: 'Folder',
      defaultValue: '',
      searchable: true,
      searchPlaceholder: 'Search folders…',
      portal: true,
      panelClassName: 'notes-transcript-folder-dropdown-panel',
      onChange: function (value) {
        var sessionId = state.selectedSessionId || state.sessionId;
        if (!sessionId) return;

        if (!value) {
          delete state.sessionFolderMap[sessionId];
        } else {
          state.sessionFolderMap[sessionId] = value;
        }
        saveFolders();
        renderDateGroupedSessions(state.sessions);
        // Update meta tags to show the new folder immediately.
        selectSession(sessionId);
      },
    });
  }

  function refreshTranscriptFolderDropdown() {
    if (!notesTranscriptFolderDropdown) return;
    notesTranscriptFolderDropdown.setOptions(getFolderDropdownItems());
    var sessionId = state.selectedSessionId || state.sessionId;
    var folderId = sessionId ? (state.sessionFolderMap[sessionId] || '') : '';
    notesTranscriptFolderDropdown.setValue(folderId || '');

    var wrap = $('notesTranscriptFolderDropdown');
    if (wrap) {
      var enabled = !!sessionId;
      wrap.style.opacity = enabled ? '1' : '0.55';
      wrap.style.pointerEvents = enabled ? '' : 'none';
    }
  }

  // ─── Export ───
  async function exportTranscript(format) {
    if (!ipcRenderer || !state.selectedSessionId) return;

    try {
      // Backend expects one of: txt, md, json. "Copy" is a UI action.
      var backendFormat = (format === 'copy') ? 'txt' : format;
      var alwaysCopy = isTextNoteSession(getSelectedSession()) && format !== 'copy';
      var result = await ipcRenderer.invoke('meeting_export_transcript', {
        sessionId: state.selectedSessionId,
        format: backendFormat,
      });

      var text = typeof result === 'string' ? result : (result && result.text) || '';

      if (format === 'copy' || alwaysCopy || !text) {
        await copyToClipboard(text);
      } else {
        var area = $('notesTranscript');
        if (area) {
          area.innerHTML = '';
          var pre = document.createElement('pre');
          pre.className = 'notes-export-preview';
          pre.textContent = text;
          area.appendChild(pre);
        }
      }
    } catch (err) {
      console.error('[MeetingNotes] meeting_export_transcript failed:', err);
    }
  }

  async function copyToClipboard(text) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch (err) {
      console.error('[MeetingNotes] clipboard write failed:', err);
    }
  }

  // ─── Chat Sidebar ───
  function getTranscriptText() {
    var session = getSelectedSession();
    if (isTextNoteSession(session)) {
      var ta = $('notesTextNoteEditor');
      return (ta && typeof ta.value === 'string') ? ta.value : '';
    }
    return state.segments.map(function (s) {
      return '[' + Math.floor((s.timestamp || 0) / 60) + ':' + String(Math.floor((s.timestamp || 0) % 60)).padStart(2, '0') + '] ' + (s.text || '');
    }).join('\n');
  }

  function getChatThreadsKey(sessionId) {
    return 'phantom-notes-chat-threads:' + sessionId;
  }

  function getChatActiveThreadKey(sessionId) {
    return 'phantom-notes-chat-active:' + sessionId;
  }

  function loadChatThreadsFromStorage(sessionId) {
    if (!sessionId) return [];
    try {
      var raw = localStorage.getItem(getChatThreadsKey(sessionId));
      if (!raw) return [];
      var parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : [];
    } catch (e) {
      return [];
    }
  }

  function saveChatThreadsToStorage(sessionId, threads) {
    if (!sessionId) return;
    try {
      localStorage.setItem(getChatThreadsKey(sessionId), JSON.stringify(threads || []));
    } catch (e) {}
  }

  function loadActiveThreadIdFromStorage(sessionId) {
    if (!sessionId) return null;
    try {
      return localStorage.getItem(getChatActiveThreadKey(sessionId));
    } catch (e) {
      return null;
    }
  }

  function saveActiveThreadIdToStorage(sessionId, threadId) {
    if (!sessionId) return;
    try {
      if (threadId) {
        localStorage.setItem(getChatActiveThreadKey(sessionId), threadId);
      } else {
        localStorage.removeItem(getChatActiveThreadKey(sessionId));
      }
    } catch (e) {}
  }

  function normalizeChatThreads(list) {
    var threads = Array.isArray(list) ? list : [];
    var out = [];
    threads.forEach(function (t) {
      if (!t) return;
      var id = (t.id || '').toString().trim();
      var title = (t.title || '').toString().trim();
      if (!id) return;
      out.push({
        id: id,
        title: title || 'New chat',
        agentId: (t.agentId || t.agent_id || '').toString().trim() || (state.chatAgentId || 'claude-code'),
        taskId: (t.taskId || t.task_id || '').toString().trim() || null,
        createdAt: t.createdAt || t.created_at || Date.now(),
        updatedAt: t.updatedAt || t.updated_at || (t.createdAt || t.created_at || Date.now()),
      });
    });
    // Newest first
    out.sort(function (a, b) { return (b.updatedAt || 0) - (a.updatedAt || 0); });
    return out;
  }

  function getCurrentChatThreadsSessionId() {
    return state.selectedSessionId || state.sessionId || null;
  }

  function setChatInputEnabled(enabled) {
    var input = $('notesChatInput');
    var sendBtn = $('notesChatSendBtn');
    if (input) input.disabled = !enabled;
    if (sendBtn) sendBtn.disabled = !enabled;
    if (input) input.placeholder = enabled ? 'Ask anything...' : 'Select a meeting to chat...';
  }

  function clearChatMessagesUI(emptyText) {
    state.chatMessages = [];
    state.chatStreamingEl = null;
    state.chatStreamingText = '';

    var container = $('notesChatMessages');
    if (container) {
      container.innerHTML = '<div class="notes-chat-empty"><i class="fal fa-comment-alt-dots"></i><p>' +
        escapeHtml(emptyText || 'Ask anything about this note...') + '</p></div>';
    }
  }

  function resetChat(opts) {
    opts = opts || {};

    if (!opts.keepTaskId) {
      state.chatTaskId = null;
      state.chatSessionActive = false;
    }
    state.chatContextSent = false;
    state.chatStreamingEl = null;
    state.chatStreamingText = '';
    clearChatMessagesUI(opts.emptyText);
    clearChatInput();
  }

  function setThreadDropdownDisabled(disabled) {
    var wrap = $('notesChatThreadDropdown');
    if (!wrap) return;
    wrap.classList.toggle('disabled', !!disabled);
  }

  function buildChatThreadDropdownItems(threads) {
    var items = [];
    (threads || []).forEach(function (t) {
      var agentLabel = (t.agentId || '').toString();
      var agentPretty = agentLabel;
      if (agentLabel === 'claude-code') agentPretty = 'Claude Code';
      if (agentLabel === 'codex') agentPretty = 'Codex';
      if (agentLabel === 'factory-droid') agentPretty = 'Factory Droid';
      if (agentLabel === 'droid') agentPretty = 'Droid';

      var desc = agentPretty ? agentPretty : '';
      items.push({
        value: t.id,
        name: t.title || 'New chat',
        description: desc,
      });
    });
    return items;
  }

  function ensureDefaultChatThread(sessionId) {
    if (!sessionId) return;
    var threads = normalizeChatThreads(loadChatThreadsFromStorage(sessionId));
    if (!threads.length) {
      var thread = {
        id: uuid(),
        title: 'Meeting chat',
        agentId: state.chatAgentId || 'claude-code',
        taskId: null,
        createdAt: Date.now(),
        updatedAt: Date.now(),
      };
      threads = [thread];
      saveChatThreadsToStorage(sessionId, threads);
      saveActiveThreadIdToStorage(sessionId, thread.id);
    }
  }

  function loadChatThreadsForSession(sessionId) {
    state.chatThreadsSessionId = sessionId || null;
    state.chatThreads = [];
    state.chatThreadId = null;

    if (!sessionId) {
      setThreadDropdownDisabled(true);
      setChatInputEnabled(false);
      if (notesChatThreadDropdown) {
        notesChatThreadDropdown.setOptions([]);
      }
      resetChat({ keepTaskId: false, emptyText: 'Select a meeting to chat...' });
      return;
    }

    ensureDefaultChatThread(sessionId);
    var threads = normalizeChatThreads(loadChatThreadsFromStorage(sessionId));
    state.chatThreads = threads;

    var activeId = loadActiveThreadIdFromStorage(sessionId);
    if (!activeId || !threads.some(function (t) { return t.id === activeId; })) {
      activeId = threads[0] ? threads[0].id : null;
      saveActiveThreadIdToStorage(sessionId, activeId);
    }

    setThreadDropdownDisabled(false);
    setChatInputEnabled(true);
    refreshChatThreadDropdown();
    if (activeId) {
      setActiveChatThread(sessionId, activeId);
    }
  }

  function refreshChatThreadDropdown() {
    if (!notesChatThreadDropdown) return;
    var sessionId = state.chatThreadsSessionId;
    if (!sessionId) {
      notesChatThreadDropdown.setOptions([]);
      return;
    }

    var items = buildChatThreadDropdownItems(state.chatThreads);
    notesChatThreadDropdown.setOptions(items);
    if (state.chatThreadId) notesChatThreadDropdown.setValue(state.chatThreadId);
  }

  function updateChatThread(sessionId, threadId, patch) {
    if (!sessionId || !threadId) return;
    var threads = normalizeChatThreads(loadChatThreadsFromStorage(sessionId));
    var idx = threads.findIndex(function (t) { return t.id === threadId; });
    if (idx === -1) return;
    threads[idx] = Object.assign({}, threads[idx], patch || {}, { updatedAt: Date.now() });
    saveChatThreadsToStorage(sessionId, threads);
    state.chatThreads = threads;
    refreshChatThreadDropdown();
  }

  function createNewChatThread(sessionId, opts) {
    opts = opts || {};
    if (!sessionId) return null;

    var threads = normalizeChatThreads(loadChatThreadsFromStorage(sessionId));
    var t = {
      id: uuid(),
      title: opts.title || 'New chat',
      agentId: opts.agentId || (state.chatAgentId || 'claude-code'),
      taskId: null,
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    threads.unshift(t);
    saveChatThreadsToStorage(sessionId, threads);
    saveActiveThreadIdToStorage(sessionId, t.id);
    state.chatThreads = threads;
    state.chatThreadId = t.id;
    refreshChatThreadDropdown();
    return t;
  }

  function setActiveChatThread(sessionId, threadId) {
    if (!sessionId || !threadId) return;
    var t = (state.chatThreads || []).find(function (x) { return x.id === threadId; }) || null;
    state.chatThreadId = threadId;
    saveActiveThreadIdToStorage(sessionId, threadId);

    // Sync agent selector to this thread (affects new chats + initial prompt).
    if (t && t.agentId) {
      state.chatAgentId = t.agentId;
      if (notesChatAgentDropdown) notesChatAgentDropdown.setValue(state.chatAgentId);
    }

    // Clear UI then hydrate from history (if task exists).
    state.chatTaskId = t ? (t.taskId || null) : null;
    state.chatSessionActive = !!state.chatTaskId;
    resetChat({ keepTaskId: true });

    if (notesChatThreadDropdown) notesChatThreadDropdown.setValue(threadId);

    if (state.chatTaskId && ipcRenderer) {
      clearChatMessagesUI('Loading chat…');
      ipcRenderer.send('GetTaskInfo', state.chatTaskId);
    } else {
      clearChatMessagesUI('Ask anything about this note…');
    }
  }

  function getSummaryCacheKey(sessionId) {
    return 'phantom-notes-summary:' + sessionId;
  }

  function getSelectedSession() {
    var id = state.selectedSessionId;
    if (!id) return null;
    return (state.sessions || []).find(function (s) { return s.id === id; }) || null;
  }

  function setTranscriptTitleInput(value) {
    var el = $('notesTranscriptTitleInput');
    if (!el) return;
    el.value = value || '';
  }

  function setSummaryPanelVisible(visible) {
    var wrap = $('notesTranscriptSummary');
    if (!wrap) return;
    wrap.style.display = visible ? '' : 'none';
  }

  function setSummaryStatus(text, kind) {
    var status = $('notesSummaryStatus');
    if (!status) return;
    status.textContent = text || '';
    status.classList.remove('busy', 'error', 'ready');
    if (kind) status.classList.add(kind);
  }

  function setSummaryContent(text, renderMarkdown) {
    var body = $('notesSummaryContent');
    if (!body) return;

    var t = text || '';
    if (renderMarkdown && window.marked) {
      body.innerHTML = window.marked.parse(t);
    } else {
      body.textContent = t;
    }
  }

  function loadCachedSummary(sessionId) {
    if (!sessionId) return null;
    try {
      return localStorage.getItem(getSummaryCacheKey(sessionId));
    } catch (e) {
      return null;
    }
  }

  function saveCachedSummary(sessionId, summaryText) {
    if (!sessionId) return;
    try {
      localStorage.setItem(getSummaryCacheKey(sessionId), summaryText || '');
    } catch (e) {}
  }

  async function loadNotesSettings() {
    if (!ipcRenderer) return;
    try {
      var settings = await ipcRenderer.invoke('get_settings');
      state.fullSettings = settings || null;
      state.summariesAgentId = (settings && settings.summariesAgent) ? settings.summariesAgent : null;
      // Default to enabled unless explicitly disabled.
      state.calendarEnabled = (settings && settings.appleCalendarEnabled !== undefined && settings.appleCalendarEnabled !== null)
        ? !!settings.appleCalendarEnabled
        : true;
      state.calendarSelected = (settings && settings.appleCalendarsSelected) ? settings.appleCalendarsSelected : {};
      state.templates = normalizeNotesTemplates(settings && settings.notesTemplates);

      // Dictation settings (default off; request permissions only when user opts in).
      state.dictationEnabled = (settings && settings.notesDictationEnabled !== undefined && settings.notesDictationEnabled !== null)
        ? !!settings.notesDictationEnabled
        : false;
      state.dictationActivation = (settings && settings.notesDictationActivation)
        ? settings.notesDictationActivation
        : 'fn_hold';
      state.dictationEngine = (settings && settings.notesDictationEngine)
        ? settings.notesDictationEngine
        : 'local';
      state.dictationShortcut = (settings && settings.notesDictationShortcut)
        ? settings.notesDictationShortcut
        : 'Option+Space';
      state.dictationFnWindowMs = (settings && settings.notesDictationFnWindowMs)
        ? settings.notesDictationFnWindowMs
        : 350;
      state.dictationPasteIntoInputs = (settings && settings.notesDictationPasteIntoInputs !== undefined && settings.notesDictationPasteIntoInputs !== null)
        ? !!settings.notesDictationPasteIntoInputs
        : false;
      state.dictationClipboardFallback = (settings && settings.notesDictationClipboardFallback !== undefined && settings.notesDictationClipboardFallback !== null)
        ? !!settings.notesDictationClipboardFallback
        : true;
      state.dictationRestoreClipboard = (settings && settings.notesDictationRestoreClipboard !== undefined && settings.notesDictationRestoreClipboard !== null)
        ? !!settings.notesDictationRestoreClipboard
        : true;
      state.dictationFlattenNewlinesInSingleLine = (settings && settings.notesDictationFlattenNewlinesInSingleLine !== undefined && settings.notesDictationFlattenNewlinesInSingleLine !== null)
        ? !!settings.notesDictationFlattenNewlinesInSingleLine
        : true;
      state.dictationCleanupEnabled = (settings && settings.notesDictationCleanupEnabled !== undefined && settings.notesDictationCleanupEnabled !== null)
        ? !!settings.notesDictationCleanupEnabled
        : false;
      state.dictationCleanupRemoveLike = (settings && settings.notesDictationCleanupRemoveLike !== undefined && settings.notesDictationCleanupRemoveLike !== null)
        ? !!settings.notesDictationCleanupRemoveLike
        : false;

      ensureTemplatesLoaded();
    } catch (err) {
      // Non-fatal. Notes can still fall back to the selected chat agent.
      state.summariesAgentId = null;
      state.fullSettings = null;
      state.calendarEnabled = true;
      state.calendarSelected = {};
      state.templates = defaultNotesTemplates();
      state.dictationEnabled = false;
      state.dictationActivation = 'fn_hold';
      state.dictationEngine = 'local';
      state.dictationShortcut = 'Option+Space';
      state.dictationFnWindowMs = 350;
      state.dictationPasteIntoInputs = false;
      state.dictationClipboardFallback = true;
      state.dictationRestoreClipboard = true;
      state.dictationFlattenNewlinesInSingleLine = true;
      state.dictationCleanupEnabled = false;
      state.dictationCleanupRemoveLike = false;
      ensureTemplatesLoaded();
    }
  }

  async function saveSettingsPatch(patch) {
    if (!ipcRenderer) return null;
    try {
      var current = await ipcRenderer.invoke('get_settings');
      var next = Object.assign({}, current || {}, patch || {});
      // Use the snake_case Tauri command directly; this avoids any mismatch between
      // `bridge.invoke('saveSettings', ...)` vs `ipcRenderer.invoke('save_settings', ...)`.
      await ipcRenderer.invoke('save_settings', { settings: next });
      state.fullSettings = next;
      return next;
    } catch (err) {
      console.error('[MeetingNotes] saveSettingsPatch failed:', err);
      return null;
    }
  }

  function getEffectiveSummariesAgentId() {
    // If user has a dedicated summaries agent configured, use it unless set to "auto".
    if (state.summariesAgentId && state.summariesAgentId !== 'auto') return state.summariesAgentId;
    return state.chatAgentId || 'claude-code';
  }

  function buildNotesSummaryPrompt(transcriptText) {
    return (
      'You are an assistant that summarizes meeting transcripts.\n' +
      '\n' +
      'Write a concise, high-signal summary in Markdown.\n' +
      'Rules:\n' +
      '- Do not invent details that are not present.\n' +
      '- Prefer concrete bullets over paragraphs.\n' +
      '- If something is unclear, write it as an open question.\n' +
      '\n' +
      'Output format:\n' +
      '## TL;DR\n' +
      '- ...\n' +
      '\n' +
      '## Key Points\n' +
      '- ...\n' +
      '\n' +
      '## Action Items\n' +
      '- [ ] Owner: task (if unknown, leave Owner blank)\n' +
      '\n' +
      '## Open Questions\n' +
      '- ...\n' +
      '\n' +
      'Transcript:\n' +
      transcriptText
    );
  }

  function truncateTranscriptForPrompt(text) {
    // Keep this as a simple character cap to avoid blowing up the backend/model.
    // (We can improve to token-based later.)
    var max = 16000;
    var t = (text || '').toString();
    if (t.length <= max) return { text: t, truncated: false };
    return { text: t.slice(0, max) + '\n\n[... transcript truncated ...]', truncated: true };
  }

  function getNotesChatProjectPath() {
    try {
      if (typeof window.getProjectPath === 'function') {
        var p = window.getProjectPath();
        return p || null;
      }
    } catch (e) {}
    return null;
  }

  function buildNotesChatCreatePayload(prompt) {
    var agentId = state.chatAgentId || 'claude-code';
    var agentsWithOwnPermissions = ['codex', 'claude-code', 'droid', 'factory-droid', 'amp', 'opencode'];

    return {
      agentId: agentId,
      prompt: prompt || '',
      projectPath: getNotesChatProjectPath(),
      baseBranch: null,
      planMode: false,
      thinking: true,
      useWorktree: false,
      permissionMode: agentsWithOwnPermissions.indexOf(agentId) !== -1 ? 'bypassPermissions' : 'default',
      execModel: 'default',
      reasoningEffort: null,
      agentMode: null,
      codexMode: null,
      claudeRuntime: null,
      multiCreate: false,
      suppressNotifications: true,
      attachments: [],
    };
  }

  function buildNotesSummaryCreatePayload(prompt) {
    var agentId = getEffectiveSummariesAgentId();
    var agentsWithOwnPermissions = ['codex', 'claude-code', 'droid', 'factory-droid', 'amp', 'opencode'];

    return {
      agentId: agentId,
      prompt: prompt || '',
      projectPath: getNotesChatProjectPath(),
      baseBranch: null,
      planMode: false,
      thinking: true,
      useWorktree: false,
      permissionMode: agentsWithOwnPermissions.indexOf(agentId) !== -1 ? 'bypassPermissions' : 'default',
      execModel: 'default',
      reasoningEffort: null,
      agentMode: null,
      codexMode: null,
      claudeRuntime: null,
      multiCreate: false,
      suppressNotifications: true,
      attachments: [],
    };
  }

  async function createNotesChatTask(initialPrompt, overrideAgentId) {
    if (!ipcRenderer) return null;
    var prev = state.chatAgentId;
    if (overrideAgentId) state.chatAgentId = overrideAgentId;
    var payload = buildNotesChatCreatePayload(initialPrompt);
    state.chatAgentId = prev;
    var result = await ipcRenderer.invoke('create_agent_session', { payload: payload });
    var taskId = (result && (result.task_id || result.taskId)) || null;
    return taskId;
  }

  async function runTemplate(templateId, userContext) {
    ensureTemplatesLoaded();
    var t = getTemplateById(templateId);
    if (!t || !ipcRenderer) return;

    var ctx = (userContext || '').toString().trim();
    var includeTranscript = !ctx;

    var sessionId = state.chatThreadsSessionId;
    if (!sessionId) {
      appendChatMessage('assistant', 'Select a note first, then run a template.');
      return;
    }

    var transcriptText = getTranscriptText();
    if (includeTranscript && (!transcriptText || !transcriptText.trim())) {
      appendChatMessage('assistant', 'No transcript yet. Record or load a transcript, then try again.');
      return;
    }

    // Templates feel best as their own thread so you can come back later.
    var thread = createNewChatThread(sessionId, { title: t.name || 'Template', agentId: state.chatAgentId });
    if (thread) {
      setActiveChatThread(sessionId, thread.id);
    } else {
      resetChat();
    }

    appendChatMessage('user', ctx ? (t.command + ' ' + ctx) : ('Template: ' + t.name));

    // Template runs own their context (transcript injected or bypassed). Do not
    // auto-prepend transcript again on the next user message.
    state.chatContextSent = true;

    var trunc = truncateTranscriptForPrompt(transcriptText);
    var initialPrompt = buildTemplateInitialPrompt(t, {
      transcript: trunc.text,
      includeTranscript: includeTranscript,
      userContext: ctx,
    });

    try {
      var agentIdForThread = thread ? thread.agentId : (state.chatAgentId || 'claude-code');
      var createdTaskId = await createNotesChatTask(initialPrompt, agentIdForThread);
      if (!createdTaskId) throw new Error('No task id returned');
      state.chatTaskId = createdTaskId;
      state.chatSessionActive = true;

      if (thread) {
        updateChatThread(sessionId, thread.id, { taskId: createdTaskId, agentId: agentIdForThread, title: t.name || thread.title });
      }

      // Show a placeholder assistant bubble immediately for responsiveness.
      ensureStreamingAssistantEl();

      ipcRenderer.send('StartPendingSession', state.chatTaskId);
    } catch (err) {
      console.error('[MeetingNotes] template create_agent_session failed:', err);
      appendChatMessage('assistant', 'Unable to start template. Please try again.');
    }
  }

  async function sendChatMessage(text) {
    if (!text || !ipcRenderer) return;

    var sessionId = state.chatThreadsSessionId;
    if (!sessionId) {
      appendChatMessage('assistant', 'Select a note first, then start a chat.');
      return;
    }

    // Ensure a thread exists/selected.
    if (!state.chatThreadId) {
      var t = createNewChatThread(sessionId, { title: 'New chat', agentId: state.chatAgentId });
      state.chatThreadId = t ? t.id : null;
      refreshChatThreadDropdown();
    }

    // Template command flow: /command <optional context>
    if (text.trim().startsWith('/')) {
      var trimmed = text.trim();
      var parts = trimmed.split(/\s+/);
      var cmd = parts[0];
      var t = getTemplateForCommand(cmd);
      if (t) {
        var ctx = trimmed.slice(cmd.length).trim();
        await runTemplate(t.id, ctx);
        return;
      }
    }

    // If we have transcript context and haven't sent it yet, prepend it
    var message = text;
    var transcript = getTranscriptText();
    if (!state.chatContextSent && transcript && transcript.trim()) {
      var label = isTextNoteSession(getSelectedSession()) ? 'note' : 'meeting transcript';
      message = 'Here is the ' + label + ' for context:\n\n' + transcript + '\n\nUser question: ' + text;
      state.chatContextSent = true;
    }

    appendChatMessage('user', text);
    clearChatInput();

    // If we don't have a task yet, create one using the first message as the
    // initial prompt, then start it (same as the rest of the app's "pending prompt" flow).
    if (!state.chatTaskId) {
      try {
        var currentThread = (state.chatThreads || []).find(function (x) { return x.id === state.chatThreadId; }) || null;
        var agentIdForThread = currentThread ? currentThread.agentId : (state.chatAgentId || 'claude-code');
        var createdTaskId = await createNotesChatTask(message, agentIdForThread);
        if (!createdTaskId) throw new Error('No task id returned');
        state.chatTaskId = createdTaskId;
        state.chatSessionActive = true;

        // Persist this task id onto the active thread so it can be reopened later.
        updateChatThread(sessionId, state.chatThreadId, {
          taskId: createdTaskId,
          agentId: agentIdForThread,
          title: (currentThread && currentThread.title && currentThread.title !== 'New chat')
            ? currentThread.title
            : (text.length > 48 ? (text.slice(0, 48) + '…') : text),
        });

        // Kick off the pending prompt immediately.
        ipcRenderer.send('StartPendingSession', state.chatTaskId);
        return;
      } catch (err) {
        console.error('[MeetingNotes] create_agent_session for notes chat failed:', err);
        appendChatMessage('assistant', 'Unable to connect to agent. Please try again.');
        return;
      }
    }

    ipcRenderer.send('SendChatMessage', state.chatTaskId, message);
  }

  async function saveSessionTitle(sessionId, title) {
    if (!ipcRenderer || !sessionId) return;
    try {
      var normalized = (title || '').trim();
      await ipcRenderer.invoke('meeting_update_title', {
        sessionId: sessionId,
        title: normalized ? normalized : null,
      });

      // Update local copy so left list reflects the change immediately.
      var idx = (state.sessions || []).findIndex(function (s) { return s.id === sessionId; });
      if (idx !== -1) {
        state.sessions[idx].title = normalized ? normalized : null;
        renderDateGroupedSessions(state.sessions);
      }
    } catch (err) {
      console.error('[MeetingNotes] meeting_update_title failed:', err);
    }
  }

  async function createTextNote() {
    if (!ipcRenderer) return;
    try {
      var result = await ipcRenderer.invoke('meeting_create_text_note', {
        title: null,
        content: null,
      });

      // Ensure the newly created note is in our local list before selecting it.
      await loadSessions();

      var id = result && result.id ? result.id : null;
      if (!id && state.sessions && state.sessions.length) id = state.sessions[0].id;
      if (!id) return;

      // If the user is currently filtering by a folder, create into that folder.
      if (state.selectedFolderId) {
        moveSessionToFolder(id, state.selectedFolderId);
      }

      switchView('transcript', id);
      setTimeout(function () {
        var editor = $('notesTextNoteEditor');
        if (editor) {
          try { editor.focus(); } catch (e) {}
        }
      }, 0);
    } catch (err) {
      console.error('[MeetingNotes] meeting_create_text_note failed:', err);
    }
  }

  function scheduleTextNoteSave() {
    if (!ipcRenderer) return;
    clearTimeout(state.textNoteSaveTimer);
    state.textNoteSaveTimer = setTimeout(flushTextNoteSaveNow, 600);
  }

  function flushTextNoteSaveNow() {
    clearTimeout(state.textNoteSaveTimer);
    state.textNoteSaveTimer = null;

    if (!ipcRenderer) return;
    if (!state.selectedSessionId) return;

    var session = getSelectedSession();
    if (!isTextNoteSession(session)) return;

    var editor = $('notesTextNoteEditor');
    if (!editor) return;
    if (state.textNoteSuppressSave) return;

    var text = (editor.value || '');
    if (text === state.textNoteLastSavedText) return;

    // Fire-and-forget; keep typing latency low.
    ipcRenderer
      .invoke('meeting_update_text_note', { sessionId: state.selectedSessionId, content: text })
      .then(function () { state.textNoteLastSavedText = text; })
      .catch(function (err) {
        console.error('[MeetingNotes] meeting_update_text_note failed:', err);
      });
  }

  async function generateSummaryForSelectedSession() {
    if (!ipcRenderer) return;
    var sessionId = state.selectedSessionId || state.sessionId;
    if (!sessionId) return;

    if (state.summaryGenerating) return;

    var transcriptText = getTranscriptText();
    if (!transcriptText || !transcriptText.trim()) {
      setSummaryPanelVisible(true);
      setSummaryStatus('No transcript yet', 'error');
      setSummaryContent('Record or load a transcript, then try again.', false);
      return;
    }

    var trunc = truncateTranscriptForPrompt(transcriptText);
    var prompt = buildNotesSummaryPrompt(trunc.text);

    setSummaryPanelVisible(true);
    setSummaryStatus(trunc.truncated ? 'Summarizing (truncated)…' : 'Summarizing…', 'busy');
    setSummaryContent('', false);

    var btn = $('notesTranscriptSummarizeBtn');
    if (btn) btn.disabled = true;

    state.summaryGenerating = true;
    state.summaryStreamingText = '';
    state.summarySessionId = sessionId;

    try {
      var payload = buildNotesSummaryCreatePayload(prompt);
      var result = await ipcRenderer.invoke('create_agent_session', { payload: payload });
      var taskId = (result && (result.task_id || result.taskId)) || null;
      if (!taskId) throw new Error('No task id returned');

      state.summaryTaskId = taskId;
      ipcRenderer.send('StartPendingSession', taskId);
    } catch (err) {
      console.error('[MeetingNotes] create_agent_session for summary failed:', err);
      state.summaryGenerating = false;
      if (btn) btn.disabled = false;
      setSummaryStatus('Failed', 'error');
      setSummaryContent('Unable to start summarization. Please try again.', false);
    }
  }

  function appendChatMessage(role, content) {
    var container = $('notesChatMessages');
    if (!container) return;

    // Remove empty state
    var empty = container.querySelector('.notes-chat-empty');
    if (empty) empty.remove();

    var msg = document.createElement('div');
    msg.className = 'notes-chat-msg ' + role;

    if (role === 'assistant' && window.marked) {
      msg.innerHTML = window.marked.parse(content);
    } else {
      msg.textContent = content;
    }

    state.chatMessages.push({ role: role, content: content });
    container.appendChild(msg);
    container.scrollTop = container.scrollHeight;
  }

  function clearChatInput() {
    var input = $('notesChatInput');
    if (input) {
      input.value = '';
      input.style.height = 'auto';
    }
  }

  // resetChat is defined earlier in the Chat Sidebar section (thread-aware).

  function initNotesChatThreadDropdown() {
    if (notesChatThreadDropdown) return;
    var container = $('notesChatThreadDropdown');
    if (!container || !window.CustomDropdown) return;

    notesChatThreadDropdown = new window.CustomDropdown({
      container: container,
      items: [],
      placeholder: 'Conversation',
      defaultValue: '',
      searchable: true,
      searchPlaceholder: 'Search chats…',
      portal: true,
      panelClassName: 'notes-chat-thread-dropdown-panel',
      onChange: function (value) {
        var sessionId = state.chatThreadsSessionId;
        if (!sessionId) return;
        setActiveChatThread(sessionId, value);
      },
    });
  }

  function initNotesChatAgentDropdown() {
    if (notesChatAgentDropdown) return;
    var container = $('notesChatAgentDropdown');
    if (!container || !window.CustomDropdown) return;

    notesChatAgentDropdown = new window.CustomDropdown({
      container: container,
      items: [
        { value: 'claude-code', name: 'Claude Code', description: '' },
        { value: 'codex', name: 'Codex', description: '' },
        { value: 'factory-droid', name: 'Factory Droid', description: '' },
      ],
      placeholder: 'Agent',
      defaultValue: state.chatAgentId || 'claude-code',
      searchable: false,
      // Notes right sidebar uses backdrop-filter + overflow hidden, which can
      // create a containing block that clips/mis-positions fixed descendants.
      // Rendering the panel in a portal keeps the dropdown usable and consistent.
      portal: true,
      panelClassName: 'notes-chat-agent-dropdown-panel',
      onChange: function (value) {
        state.chatAgentId = value;
        // If the current thread has not started yet, update its agent preference.
        // Otherwise, keep the existing chat session stable; the new agent will
        // apply to the next chat you create.
        var sessionId = state.chatThreadsSessionId;
        var threadId = state.chatThreadId;
        var t = (state.chatThreads || []).find(function (x) { return x.id === threadId; }) || null;
        if (sessionId && threadId && t && !t.taskId) {
          updateChatThread(sessionId, threadId, { agentId: value });
        }
      },
    });
  }

  // ─── Tauri Event Handlers ───
  function payloadFromArgs(a, b) {
    return (b !== undefined) ? b : a;
  }

  function onTranscriptionSegment(a, b) {
    var data = payloadFromArgs(a, b);
    if (!state.recording) return;
    appendSegment(data || {});
  }

  function onTranscriptionStatus(a, b) {
    var data = payloadFromArgs(a, b);
    if (!data) return;

    if (data.recording !== undefined) state.recording = !!data.recording;
    if (data.paused !== undefined) state.paused = !!data.paused;
    if (data.session_id !== undefined) state.sessionId = data.session_id;
    if (data.state) {
      // Back-compat: some payloads used a string state.
      state.recording = data.state === 'recording' || data.state === 'paused';
      state.paused = data.state === 'paused';
    }

    updateControlsUI();

    if (!state.recording && state.timerInterval) {
      stopTimer();
      loadSessions();
    }
  }

  function onWhisperModelProgress(a, b) {
    var data = payloadFromArgs(a, b);
    var progressBar = $('notesModelProgressBar');
    var progressText = $('notesModelProgressText');
    if (!data) return;

    // Back-compat: older backend only emitted progress events. Treat any progress
    // event as "download in progress" so the UI still works.
    if (!state.modelDownloading) {
      state.modelDownloading = true;
      syncModalStatus();
    }

    var pct = 0;
    if (data.progress !== undefined) {
      pct = Math.round(data.progress || 0);
    } else if (data.total) {
      pct = Math.round(((data.downloaded || 0) / data.total) * 100);
    }
    if (progressBar) progressBar.style.width = pct + '%';
    if (progressText) progressText.textContent = pct + '%';
  }

  function onWhisperModelStatus(a, b) {
    var data = payloadFromArgs(a, b);
    if (!data) return;
    state.whisperModelStatus = data;

    var statusState = data.state;
    state.modelDownloading = statusState === 'downloading';

    // Update the top-right badge without hammering `check_whisper_model` during downloads.
    var badge = $('notesModelStatus');
    if (badge) {
      if (statusState === 'downloading') {
        var pctText = '';
        if (data.progress && data.progress.totalBytes) {
          var pctBadge = Math.round((data.progress.downloadedBytes / data.progress.totalBytes) * 100);
          pctText = ' · ' + pctBadge + '%';
        }
        badge.textContent = 'Downloading' + pctText;
        badge.classList.add('downloading');
        badge.classList.remove('ready');
        badge.classList.remove('missing');
      } else if (statusState === 'error') {
        badge.textContent = 'Model error';
        badge.classList.remove('downloading');
        badge.classList.remove('ready');
        badge.classList.add('missing');
      }
    }

    var progressWrap = $('notesModelProgress');
    var progressBar = $('notesModelProgressBar');
    var progressText = $('notesModelProgressText');

    if (statusState === 'downloading') {
      if (progressWrap) progressWrap.style.display = 'flex';
      var pct = 0;
      if (data.progress && data.progress.totalBytes) {
        pct = Math.round((data.progress.downloadedBytes / data.progress.totalBytes) * 100);
      }
      if (progressBar) progressBar.style.width = pct + '%';
      if (progressText) {
        var bytes = data.progress ? (data.progress.downloadedBytes || 0) : 0;
        var total = data.progress ? (data.progress.totalBytes || 0) : 0;
        var extra = total ? (' · ' + bytesToHuman(bytes) + ' / ' + bytesToHuman(total)) : '';
        progressText.textContent = pct + '%' + extra;
      }
    } else {
      if (progressWrap) progressWrap.style.display = 'none';
    }

    // Sync buttons + badge.
    syncModalStatus();

    // When download finishes, refresh catalog so the dropdown shows "Downloaded".
    if (statusState === 'ready' || statusState === 'missing' || statusState === 'error') {
      checkModelStatus();
    }
  }

  function onAddTask(event, task) {
    // Capture the task ID for chat if it matches our agent
    if (task && task.ID && state.chatSessionActive && !state.chatTaskId) {
      state.chatTaskId = task.ID;
    }
  }

  function onChatLogBatch(event, taskId, messages) {
    if (taskId !== state.chatTaskId) return;
    if (!messages || !messages.length) return;

    // Replace current UI with the persisted history.
    clearChatMessagesUI();
    messages.forEach(function (msg) {
      if (!msg || !msg.content) return;
      var role = msg.role || null;
      var mt = msg.message_type || null;
      if (mt === 'user_message' || role === 'user') {
        appendChatMessage('user', msg.content);
      } else if (mt === 'assistant_message' || role === 'assistant') {
        appendChatMessage('assistant', msg.content);
      }
    });
  }

  function ensureStreamingAssistantEl() {
    var container = $('notesChatMessages');
    if (!container) return null;

    // Remove empty state
    var empty = container.querySelector('.notes-chat-empty');
    if (empty) empty.remove();

    if (state.chatStreamingEl && state.chatStreamingEl.parentNode === container) {
      return state.chatStreamingEl;
    }

    var el = document.createElement('div');
    el.className = 'notes-chat-msg assistant';
    el.dataset.streaming = 'true';
    el.textContent = '';
    container.appendChild(el);
    container.scrollTop = container.scrollHeight;
    state.chatStreamingEl = el;
    state.chatStreamingText = '';
    return el;
  }

  function clearStreamingAssistantEl() {
    if (state.chatStreamingEl && state.chatStreamingEl.parentNode) {
      state.chatStreamingEl.parentNode.removeChild(state.chatStreamingEl);
    }
    state.chatStreamingEl = null;
    state.chatStreamingText = '';
  }

  function onChatLogUpdate(event, taskId, message) {
    var isChat = taskId === state.chatTaskId;
    var isSummary = taskId === state.summaryTaskId;
    if (!isChat && !isSummary) return;
    if (!message) return;

    // We already render user messages locally in the sidebar UI.
    if (message.message_type === 'user_message') return;

    // Replace any in-progress streaming bubble with the final message.
    if (message.message_type === 'assistant_message') {
      if (isChat) {
        clearStreamingAssistantEl();
        if (message.content) {
          appendChatMessage('assistant', message.content);
        }
      }
      if (isSummary) {
        state.summaryGenerating = false;
        var btn = $('notesTranscriptSummarizeBtn');
        if (btn) btn.disabled = false;

        var content = message.content || state.summaryStreamingText || '';
        setSummaryPanelVisible(true);
        setSummaryStatus('Ready', 'ready');
        setSummaryContent(content, true);
        if (state.summarySessionId) saveCachedSummary(state.summarySessionId, content);
      }
    }
  }

  function onChatLogStreaming(event, taskId, update) {
    var isChat = taskId === state.chatTaskId;
    var isSummary = taskId === state.summaryTaskId;
    if (!isChat && !isSummary) return;
    if (!update || update.type !== 'streaming') return;

    if (update.message_type !== 'assistant_message') return;
    var chunk = update.content || '';
    if (!chunk) return;

    if (isChat) {
      var el = ensureStreamingAssistantEl();
      if (!el) return;

      state.chatStreamingText += chunk;
      el.textContent = state.chatStreamingText;
      return;
    }

    if (isSummary) {
      state.summaryStreamingText += chunk;
      setSummaryPanelVisible(true);
      setSummaryStatus('Summarizing…', 'busy');
      setSummaryContent(state.summaryStreamingText, false);
    }
  }

  function onChatLogStatus(event, taskId, status, statusState) {
    var isChat = taskId === state.chatTaskId;
    var isSummary = taskId === state.summaryTaskId;
    if (!isChat && !isSummary) return;

    if (statusState === 'idle' || statusState === 'completed' || statusState === 'error') {
      if (isChat) {
        // When the backend reports completion/idle, the final ChatLogUpdate should arrive.
        // If it doesn't (edge cases), clear the streaming bubble so the UI doesn't get stuck.
        setTimeout(function () {
          if (state.chatStreamingEl) clearStreamingAssistantEl();
        }, 300);
      }

      if (isSummary && statusState === 'error') {
        state.summaryGenerating = false;
        var btn = $('notesTranscriptSummarizeBtn');
        if (btn) btn.disabled = false;
        setSummaryPanelVisible(true);
        setSummaryStatus('Failed', 'error');
      }
    }
  }

  // ─── Event Binding ───
  function bindEvents() {
    if (state.eventsBound) return;
    state.eventsBound = true;

    // Model modal
    var settingsBtn = $('notesSettingsBtn');
    if (settingsBtn) settingsBtn.addEventListener('click', openModelModal);

    // Templates modal
    var templatesBtn = $('notesTemplatesBtn');
    if (templatesBtn) templatesBtn.addEventListener('click', openTemplatesModal);

    var templatesCloseBtn = $('notesTemplatesCloseBtn');
    if (templatesCloseBtn) templatesCloseBtn.addEventListener('click', closeTemplatesModal);

    var templatesSaveBtn = $('notesTemplatesSaveBtn');
    if (templatesSaveBtn) templatesSaveBtn.addEventListener('click', handleTemplatesSave);

    var templatesNewBtn = $('notesTemplatesNewBtn');
    if (templatesNewBtn) templatesNewBtn.addEventListener('click', handleTemplatesNew);

    var templatesDeleteBtn = $('notesTemplatesDeleteBtn');
    if (templatesDeleteBtn) templatesDeleteBtn.addEventListener('click', handleTemplatesDelete);

    var templatesResetBtn = $('notesTemplatesResetBtn');
    if (templatesResetBtn) templatesResetBtn.addEventListener('click', handleTemplatesReset);

    var templatesModal = $('notesTemplatesModal');
    if (templatesModal) {
      templatesModal.addEventListener('click', function (e) {
        if (e.target === templatesModal) closeTemplatesModal();
      });
    }

    var modelStatus = $('notesModelStatus');
    if (modelStatus) modelStatus.addEventListener('click', openModelModal);

    var modalCloseBtn = $('notesModalCloseBtn');
    if (modalCloseBtn) modalCloseBtn.addEventListener('click', closeModelModal);

    // Permission modal (Accessibility prompt preflight)
    var permCloseBtn = $('notesPermissionCloseBtn');
    if (permCloseBtn) permCloseBtn.addEventListener('click', function () { closePermissionModal(false); });

    var permCancelBtn = $('notesPermissionCancelBtn');
    if (permCancelBtn) permCancelBtn.addEventListener('click', function () { closePermissionModal(false); });

    var permContinueBtn = $('notesPermissionContinueBtn');
    if (permContinueBtn) permContinueBtn.addEventListener('click', function () { closePermissionModal(true); });

    var permModal = $('notesPermissionModal');
    if (permModal) {
      permModal.addEventListener('click', function (e) {
        if (e.target === permModal) closePermissionModal(false);
      });
    }

    var tabModels = $('notesModalTabModels');
    if (tabModels) tabModels.addEventListener('click', function () { setNotesSettingsTab('models'); });

    var tabCalendar = $('notesModalTabCalendar');
    if (tabCalendar) tabCalendar.addEventListener('click', function () { setNotesSettingsTab('calendar'); });

    var tabDictation = $('notesModalTabDictation');
    if (tabDictation) tabDictation.addEventListener('click', function () { setNotesSettingsTab('dictation'); });

    initModelDropdown();

    var modelDownloadBtn = $('notesModelDownloadBtn');
    if (modelDownloadBtn) modelDownloadBtn.addEventListener('click', downloadModel);

    var modelCancelBtn = $('notesModelCancelBtn');
    if (modelCancelBtn) modelCancelBtn.addEventListener('click', cancelModelDownload);

    var modelSetActiveBtn = $('notesModelSetActiveBtn');
    if (modelSetActiveBtn) modelSetActiveBtn.addEventListener('click', setActiveModel);

    var modelDeleteBtn = $('notesModelDeleteBtn');
    if (modelDeleteBtn) modelDeleteBtn.addEventListener('click', deleteModel);

    var calToggle = $('notesCalendarEnabledToggle');
    if (calToggle) {
      calToggle.addEventListener('change', function () {
        state.calendarEnabled = !!calToggle.checked;
        saveSettingsPatch({ appleCalendarEnabled: state.calendarEnabled }).then(function () {
          syncCalendarSettingsUI();
          loadUpcomingEvents();
          if (state.calendarEnabled) loadCalendarList();
        });
      });
    }

    var calAll = $('notesCalendarSelectAllBtn');
    if (calAll) calAll.addEventListener('click', function () {
      if (!state.calendarSelected) state.calendarSelected = {};
      (state.calendarList || []).forEach(function (c) { state.calendarSelected[c.id] = true; });
      saveSettingsPatch({ appleCalendarsSelected: state.calendarSelected }).then(function () {
        renderCalendarList();
        syncCalendarSettingsUI();
        loadUpcomingEvents();
      });
    });

    var calNone = $('notesCalendarClearBtn');
    if (calNone) calNone.addEventListener('click', function () {
      if (!state.calendarSelected) state.calendarSelected = {};
      (state.calendarList || []).forEach(function (c) { state.calendarSelected[c.id] = false; });
      saveSettingsPatch({ appleCalendarsSelected: state.calendarSelected }).then(function () {
        renderCalendarList();
        syncCalendarSettingsUI();
        loadUpcomingEvents();
      });
    });

    var calRefresh = $('notesCalendarRefreshBtn');
    if (calRefresh) calRefresh.addEventListener('click', function () {
      loadCalendarList().then(loadUpcomingEvents);
    });

    // Dictation controls/settings
    var dictEnabled = $('notesDictationEnabledToggle');
    if (dictEnabled) {
      var onDictEnabledChanged = function () {
        state.dictationEnabled = !!dictEnabled.checked;
        saveSettingsPatch({ notesDictationEnabled: state.dictationEnabled }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictEnabled.addEventListener('change', onDictEnabledChanged);
      // Some WebView/platform combos feel more responsive with input listeners as well.
      dictEnabled.addEventListener('input', onDictEnabledChanged);
    }

    var dictActivation = $('notesDictationActivationSelect');
    if (dictActivation) {
      var onDictActivationChanged = function () {
        state.dictationActivation = dictActivation.value || 'fn_hold';
        saveSettingsPatch({ notesDictationActivation: state.dictationActivation }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictActivation.addEventListener('change', onDictActivationChanged);
      dictActivation.addEventListener('input', onDictActivationChanged);
    }

    var dictEngine = $('notesDictationEngineSelect');
    if (dictEngine) {
      var onDictEngineChanged = function () {
      state.dictationEngine = dictEngine.value || 'local';
        // If switching to Cloud, refresh availability so we can show guidance immediately.
        if (state.dictationEngine === 'chatgpt') refreshTranscriptionAvailability();
        saveSettingsPatch({ notesDictationEngine: state.dictationEngine }).then(function (next) {
          // Even if save fails, keep UI consistent and show status guidance.
          refreshTranscriptionAvailability().then(syncDictationSettingsUI);
          if (!next) refreshDictationStatus();
        });
      };
      dictEngine.addEventListener('change', onDictEngineChanged);
      // On some platforms, <select> only commits on blur; input provides immediate persistence.
      dictEngine.addEventListener('input', onDictEngineChanged);
    }

    var dictShortcut = $('notesDictationShortcutInput');
    if (dictShortcut) {
      var dictShortcutSaveTimer = null;
      var saveDictShortcutNow = function () {
        state.dictationShortcut = (dictShortcut.value || '').trim() || 'Option+Space';
        saveSettingsPatch({ notesDictationShortcut: state.dictationShortcut }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictShortcut.addEventListener('change', saveDictShortcutNow);
      dictShortcut.addEventListener('blur', saveDictShortcutNow);
      dictShortcut.addEventListener('input', function () {
        if (dictShortcutSaveTimer) clearTimeout(dictShortcutSaveTimer);
        dictShortcutSaveTimer = setTimeout(saveDictShortcutNow, 450);
      });
    }

    var dictPaste = $('notesDictationPasteToggle');
    if (dictPaste) {
      var onDictPasteChanged = function () {
        var want = !!dictPaste.checked;

        // If toggling ON, pre-explain the macOS dialog and only then trigger it.
        if (want && !state.dictationPasteIntoInputs) {
          dictPaste.checked = false;

          openPermissionModal({
            title: 'Enable Paste Into Other Apps',
            body: 'To paste transcripts directly into other apps, macOS will show an Accessibility permissions prompt for Phantom. Phantom only uses this to paste your transcript (Cmd+V). You can keep using clipboard-only mode if you prefer.',
            continueLabel: 'Enable Paste'
          }).then(function (confirmed) {
            if (!confirmed) {
              // Leave setting off.
              syncDictationSettingsUI();
              return;
            }
            if (!ipcRenderer) {
              state.dictationPasteIntoInputs = true;
              saveSettingsPatch({ notesDictationPasteIntoInputs: true }).then(function () {
                syncDictationSettingsUI();
              });
              return;
            }

            ipcRenderer.invoke('dictation_request_accessibility').then(function (trusted) {
              if (trusted === true) {
                state.dictationPasteIntoInputs = true;
                dictPaste.checked = true;
                saveSettingsPatch({ notesDictationPasteIntoInputs: true }).then(function (next) {
                  syncDictationSettingsUI();
                  if (!next) refreshDictationStatus();
                });
              } else {
                state.dictationPasteIntoInputs = false;
                dictPaste.checked = false;
                // Best-effort: show a gentle hint; dictation will still copy to clipboard.
                if (typeof sendNotification === 'function') {
                  sendNotification('Accessibility not granted. Dictation will copy to clipboard instead of pasting.', 'yellow');
                }
                refreshDictationStatus();
                syncDictationSettingsUI();
              }
            }).catch(function () {
              // Non-macOS (or command unavailable): allow the toggle without prompting.
              state.dictationPasteIntoInputs = true;
              dictPaste.checked = true;
              saveSettingsPatch({ notesDictationPasteIntoInputs: true }).then(function (next) {
                syncDictationSettingsUI();
                if (!next) refreshDictationStatus();
              });
            });
          });
          return;
        }

        // Toggling OFF (or unchanged): just persist.
        state.dictationPasteIntoInputs = want;
        saveSettingsPatch({ notesDictationPasteIntoInputs: state.dictationPasteIntoInputs }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictPaste.addEventListener('change', onDictPasteChanged);
      dictPaste.addEventListener('input', onDictPasteChanged);
    }

    var dictClipFallback = $('notesDictationClipboardFallbackToggle');
    if (dictClipFallback) {
      var onDictClipFallbackChanged = function () {
        state.dictationClipboardFallback = !!dictClipFallback.checked;
        saveSettingsPatch({ notesDictationClipboardFallback: state.dictationClipboardFallback }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictClipFallback.addEventListener('change', onDictClipFallbackChanged);
      dictClipFallback.addEventListener('input', onDictClipFallbackChanged);
    }

    var dictRestore = $('notesDictationRestoreClipboardToggle');
    if (dictRestore) {
      var onDictRestoreChanged = function () {
        state.dictationRestoreClipboard = !!dictRestore.checked;
        saveSettingsPatch({ notesDictationRestoreClipboard: state.dictationRestoreClipboard }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictRestore.addEventListener('change', onDictRestoreChanged);
      dictRestore.addEventListener('input', onDictRestoreChanged);
    }

    var dictFlatten = $('notesDictationFlattenToggle');
    if (dictFlatten) {
      var onDictFlattenChanged = function () {
        state.dictationFlattenNewlinesInSingleLine = !!dictFlatten.checked;
        saveSettingsPatch({ notesDictationFlattenNewlinesInSingleLine: state.dictationFlattenNewlinesInSingleLine }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictFlatten.addEventListener('change', onDictFlattenChanged);
      dictFlatten.addEventListener('input', onDictFlattenChanged);
    }

    var dictCleanup = $('notesDictationCleanupToggle');
    if (dictCleanup) {
      var onDictCleanupChanged = function () {
        state.dictationCleanupEnabled = !!dictCleanup.checked;
        saveSettingsPatch({ notesDictationCleanupEnabled: state.dictationCleanupEnabled }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictCleanup.addEventListener('change', onDictCleanupChanged);
      dictCleanup.addEventListener('input', onDictCleanupChanged);
    }

    var dictCleanupLike = $('notesDictationCleanupLikeToggle');
    if (dictCleanupLike) {
      var onDictCleanupLikeChanged = function () {
        state.dictationCleanupRemoveLike = !!dictCleanupLike.checked;
        saveSettingsPatch({ notesDictationCleanupRemoveLike: state.dictationCleanupRemoveLike }).then(function (next) {
          syncDictationSettingsUI();
          if (!next) refreshDictationStatus();
        });
      };
      dictCleanupLike.addEventListener('change', onDictCleanupLikeChanged);
      dictCleanupLike.addEventListener('input', onDictCleanupLikeChanged);
    }

    var dictRefresh = $('notesDictationRefreshBtn');
    if (dictRefresh) dictRefresh.addEventListener('click', refreshDictationStatus);

    var dictStart = $('notesDictationStartBtn');
    if (dictStart) dictStart.addEventListener('click', startDictationNow);

    var dictStop = $('notesDictationStopBtn');
    if (dictStop) dictStop.addEventListener('click', stopDictationNow);

    var dictCopy = $('notesDictationCopyBtn');
    if (dictCopy) dictCopy.addEventListener('click', copyLastDictationTranscript);

    // Click overlay to close modal
    var modelModal = $('notesModelModal');
    if (modelModal) {
      modelModal.addEventListener('click', function (e) {
        if (e.target === modelModal) closeModelModal();
      });
    }

    // Recording controls
    var startBtn = $('notesStartBtn');
    if (startBtn) startBtn.addEventListener('click', startRecording);

    var pauseBtn = $('notesPauseBtn');
    if (pauseBtn) pauseBtn.addEventListener('click', pauseRecording);

    var resumeBtn = $('notesResumeBtn');
    if (resumeBtn) resumeBtn.addEventListener('click', resumeRecording);

    var stopBtn = $('notesStopBtn');
    if (stopBtn) stopBtn.addEventListener('click', stopRecording);

    // Transcript view
    var backBtn = $('notesBackBtn');
    if (backBtn) {
      backBtn.addEventListener('click', function () {
        switchView('default');
      });
    }

    initTranscriptFolderDropdown();
    refreshTranscriptFolderDropdown();

    var titleEdit = $('notesTranscriptTitleInput');
    if (titleEdit) {
      titleEdit.addEventListener('keydown', function (e) {
        if (e.key === 'Enter') {
          e.preventDefault();
          try { titleEdit.blur(); } catch (err) {}
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          var session = getSelectedSession();
          setTranscriptTitleInput(session ? (session.title || '') : '');
          try { titleEdit.blur(); } catch (err) {}
        }
      });

      titleEdit.addEventListener('blur', function () {
        var sessionId = state.selectedSessionId || state.sessionId;
        if (!sessionId) return;
        saveSessionTitle(sessionId, titleEdit.value);
      });
    }

    var summarizeBtn = $('notesTranscriptSummarizeBtn');
    if (summarizeBtn) summarizeBtn.addEventListener('click', generateSummaryForSelectedSession);

    // Export
    var exportTxt = $('notesExportTxt');
    if (exportTxt) exportTxt.addEventListener('click', function () { exportTranscript('txt'); });

    var exportMd = $('notesExportMd');
    if (exportMd) exportMd.addEventListener('click', function () { exportTranscript('md'); });

    var exportJson = $('notesExportJson');
    if (exportJson) exportJson.addEventListener('click', function () { exportTranscript('json'); });

    var copyBtn = $('notesCopyBtn');
    if (copyBtn) copyBtn.addEventListener('click', function () { exportTranscript('copy'); });

    var deleteBtn = $('notesDeleteBtn');
    if (deleteBtn) {
      deleteBtn.addEventListener('click', function () {
        if (!state.selectedSessionId) return;
        deleteSession(state.selectedSessionId);
      });
    }

    // Left sidebar nav (currently only "My notes")
    document.querySelectorAll('.notes-nav-item[data-filter]').forEach(function (el) {
      el.addEventListener('click', function () {
        document.querySelectorAll('.notes-nav-item').forEach(function (n) { n.classList.remove('active'); });
        el.classList.add('active');
        state.activeFilter = 'mine';
        renderDateGroupedSessions(state.sessions);
      });
    });

    // New text note (scratch pad)
    var newNoteBtn = $('notesNewTextNoteBtn');
    if (newNoteBtn) newNoteBtn.addEventListener('click', createTextNote);

    var textEditor = $('notesTextNoteEditor');
    if (textEditor) {
      textEditor.addEventListener('input', function () {
        if (state.textNoteSuppressSave) return;
        var session = getSelectedSession();
        if (!isTextNoteSession(session)) return;
        scheduleTextNoteSave();
      });
      textEditor.addEventListener('blur', flushTextNoteSaveNow);
    }

    // Search
    var searchInput = $('notesSearchInput');
    var searchTimeout = null;
    if (searchInput) {
      searchInput.addEventListener('input', function () {
        clearTimeout(searchTimeout);
        searchTimeout = setTimeout(onSearchInput, 250);
      });
    }

    // Palette
    var paletteOverlay = $('notesSearchPalette');
    if (paletteOverlay) {
      paletteOverlay.addEventListener('click', function (e) {
        if (e.target === paletteOverlay) closeSearchPalette();
      });
    }

    var paletteInput = $('notesPaletteInput');
    var paletteTimeout = null;
    if (paletteInput) {
      paletteInput.addEventListener('input', function () {
        clearTimeout(paletteTimeout);
        paletteTimeout = setTimeout(function () {
          state.paletteQuery = paletteInput.value || '';
          // Keep sidebar search in sync so it "remembers" the last query.
          var sidebarInput = $('notesSearchInput');
          if (sidebarInput) sidebarInput.value = state.paletteQuery;
          state.searchQuery = state.paletteQuery;
          renderDateGroupedSessions(state.sessions);
          renderPaletteResults();
        }, 60);
      });
    }

    // Add folder
    var addFolderBtn = $('notesAddFolderBtn');
    if (addFolderBtn) {
      addFolderBtn.addEventListener('click', function () {
        openInlineFolderCreator();
      });
    }

    // Chat sidebar
    initNotesChatThreadDropdown();
    initNotesChatAgentDropdown();

    var newChatBtn = $('notesNewChatBtn');
    if (newChatBtn) {
      newChatBtn.addEventListener('click', function () {
        var sessionId = getCurrentChatThreadsSessionId();
        if (!sessionId) {
          appendChatMessage('assistant', 'Select a note first, then start a chat.');
          return;
        }
        createNewChatThread(sessionId, { title: 'New chat', agentId: state.chatAgentId });
        refreshChatThreadDropdown();
        setActiveChatThread(sessionId, state.chatThreadId);
      });
    }

    // Chat input
    var chatInput = $('notesChatInput');
    var chatSendBtn = $('notesChatSendBtn');

    if (chatInput) {
      chatInput.addEventListener('keydown', function (e) {
        if (e.key === 'Enter' && !e.shiftKey) {
          e.preventDefault();
          var text = chatInput.value.trim();
          if (text) sendChatMessage(text);
        }
      });

      // Auto-resize textarea
      chatInput.addEventListener('input', function () {
        chatInput.style.height = 'auto';
        chatInput.style.height = Math.min(chatInput.scrollHeight, 80) + 'px';
      });
    }

    if (chatSendBtn) {
      chatSendBtn.addEventListener('click', function () {
        var text = chatInput ? chatInput.value.trim() : '';
        if (text) sendChatMessage(text);
      });
    }

    // Templates (chips are rendered dynamically)
    renderTemplateChips();

    // Tauri event listeners
    if (ipcRenderer) {
      ipcRenderer.on('TranscriptionSegment', onTranscriptionSegment);
      ipcRenderer.on('TranscriptionStatus', onTranscriptionStatus);
      ipcRenderer.on('LocalAsrModelProgress', onWhisperModelProgress);
      ipcRenderer.on('LocalAsrModelStatus', onWhisperModelStatus);
      ipcRenderer.on('DictationStatus', onDictationStatus);
      ipcRenderer.on('DictationTranscript', onDictationTranscript);
      ipcRenderer.on('DictationOpenSettings', onDictationOpenSettings);
      ipcRenderer.on('AddTask', onAddTask);
      ipcRenderer.on('ChatLogBatch', onChatLogBatch);
      ipcRenderer.on('ChatLogUpdate', onChatLogUpdate);
      ipcRenderer.on('ChatLogStreaming', onChatLogStreaming);
      ipcRenderer.on('ChatLogStatus', onChatLogStatus);
    }

    // Navigation event
    window.addEventListener('PhantomNavigate', function (event) {
      var pageId = event && event.detail && event.detail.pageId;
      if (pageId !== 'notesPage') return;
      init();
    });

    // Keyboard shortcut: Cmd+K for search focus
    document.addEventListener('keydown', function (e) {
      if (!isNotesVisible()) return;

      var perm = $('notesPermissionModal');
      if (perm && perm.style.display !== 'none' && e.key === 'Escape') {
        e.preventDefault();
        closePermissionModal(false);
        return;
      }

      if (state.templatesModalOpen && e.key === 'Escape') {
        e.preventDefault();
        closeTemplatesModal();
        return;
      }

      // When palette is open, it owns the keyboard.
      if (state.paletteOpen) {
        if (e.key === 'Escape') {
          e.preventDefault();
          closeSearchPalette();
          return;
        }
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setPaletteActive(state.paletteActiveIndex + 1);
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          setPaletteActive(state.paletteActiveIndex - 1);
          return;
        }
        if (e.key === 'Enter') {
          e.preventDefault();
          activatePaletteSelection();
          return;
        }
      }

      // Cmd/Ctrl+K opens palette.
      if ((e.metaKey || e.ctrlKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        openSearchPalette();
        return;
      }
    });

    watchNotesVisibility();
  }

  function watchNotesVisibility() {
    var page = $('notesPage');
    if (!page || state.visibilityObserver) return;

    state.visibilityObserver = new MutationObserver(function () {
      if (!page.hasAttribute('hidden')) {
        init();
        startUpcomingRefresh();
      } else {
        stopUpcomingRefresh();
      }
    });

    state.visibilityObserver.observe(page, {
      attributes: true,
      attributeFilter: ['hidden'],
    });

    if (!page.hasAttribute('hidden')) {
      init();
      startUpcomingRefresh();
    }
  }

  // ─── Initialization ───
  async function init() {
    if (state.initialized) {
      await loadNotesSettings();
      renderTemplateChips();
      loadChatThreadsForSession(state.selectedSessionId);
      loadUpcomingEvents();
      await loadSessions();
      return;
    }
    state.initialized = true;

    loadFolders();
    renderFolderList();
    await loadNotesSettings();
    renderTemplateChips();
    loadChatThreadsForSession(state.selectedSessionId);
    await checkModelStatus();
    await loadUpcomingEvents();
    await loadSessions();
    updateControlsUI();

    // Check if there is an active meeting in progress
    if (ipcRenderer) {
      try {
        var meetingState = await ipcRenderer.invoke('meeting_state');
        if (meetingState) {
          state.recording = !!meetingState.recording;
          state.paused = !!meetingState.paused;
          state.sessionId = meetingState.session_id || null;

          if (state.recording) {
            switchView('transcript', null);
            if (state.sessionId) {
              state.selectedSessionId = state.sessionId;
              loadChatThreadsForSession(state.sessionId);
            }

            if (meetingState.elapsed_seconds) {
              state.timerSeconds = meetingState.elapsed_seconds;
              var display = $('notesTimer');
              if (display) display.textContent = formatTime(state.timerSeconds);
              if (!state.paused) {
                state.timerInterval = setInterval(function () {
                  state.timerSeconds += 1;
                  if (display) display.textContent = formatTime(state.timerSeconds);
                }, 1000);
              }
            }
          }

          updateControlsUI();
        }
      } catch (err) {
        console.error('[MeetingNotes] meeting_state failed:', err);
      }
    }
  }

  // Boot
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', function () { bindEvents(); });
  } else {
    bindEvents();
  }

  window.initMeetingNotes = init;
})();
