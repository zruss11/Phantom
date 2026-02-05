/**
 * Command Center Module
 * Manages the Command Center dashboard for GitHub issues, Linear issues,
 * Sentry errors, and CI/CD status.
 */
(function() {
  'use strict';

  // ============================================================================
  // Module State
  // ============================================================================
  var state = {
    data: null,           // CommandCenterData from backend
    config: {
      refreshInterval: 15 // minutes, default
    },
    refreshTimer: null,
    isVisible: false,
    selectedAgent: 'claude-code',
    popupContext: null    // { type: 'error'|'issue', item: {...} }
  };

  // ============================================================================
  // Utility Functions
  // ============================================================================

  /**
   * Escape HTML to prevent XSS
   */
  function escapeHtml(text) {
    if (!text) return '';
    var div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
  }

  /**
   * Format a count with optional suffix
   */
  function formatCount(count, singular, plural) {
    plural = plural || singular + 's';
    return count + ' ' + (count === 1 ? singular : plural);
  }

  /**
   * Format relative time (e.g., "2h ago", "3d ago")
   */
  function formatTimeAgo(dateString) {
    if (!dateString) return '';
    try {
      var date = new Date(dateString);
      var now = new Date();
      var diffMs = now - date;
      var diffSec = Math.floor(diffMs / 1000);
      var diffMin = Math.floor(diffSec / 60);
      var diffHour = Math.floor(diffMin / 60);
      var diffDay = Math.floor(diffHour / 24);

      if (diffMin < 1) return 'just now';
      if (diffMin < 60) return diffMin + 'm ago';
      if (diffHour < 24) return diffHour + 'h ago';
      if (diffDay < 7) return diffDay + 'd ago';
      return date.toLocaleDateString();
    } catch (e) {
      return dateString;
    }
  }

  /**
   * Format duration in seconds to human readable
   */
  function formatDuration(seconds) {
    if (!seconds && seconds !== 0) return '';
    var min = Math.floor(seconds / 60);
    var sec = seconds % 60;
    return min + 'm ' + sec + 's';
  }

  /**
   * Get priority class for Linear issues
   */
  function getPriorityClass(priority) {
    switch (priority) {
      case 1: return 'priority-urgent';
      case 2: return 'priority-high';
      case 3: return 'priority-medium';
      case 4: return 'priority-low';
      default: return 'priority-none';
    }
  }

  /**
   * Get label class based on label name
   */
  function getLabelClass(labelName) {
    var name = (labelName || '').toLowerCase();
    if (name.includes('bug')) return 'label-bug';
    if (name.includes('feature')) return 'label-feature';
    if (name.includes('improvement') || name.includes('enhancement')) return 'label-improvement';
    return 'label-default';
  }

  /**
   * Get CI status class
   */
  function getCiStatusClass(status, conclusion) {
    if (status === 'in_progress' || status === 'queued') return 'running';
    if (conclusion === 'success') return 'success';
    if (conclusion === 'failure') return 'failed';
    return 'pending';
  }

  /**
   * Check if error is critical (high count or recent)
   */
  function isCriticalError(error) {
    return error.count > 100 || error.level === 'error';
  }

  /**
   * Invoke Tauri command via bridge
   */
  function tauriInvoke(command, args) {
    var tauri = window.__TAURI__;
    if (tauri && tauri.core && tauri.core.invoke) {
      return tauri.core.invoke(command, args || {});
    }
    // Fallback: use bridge if available
    if (window.tauriBridge && window.tauriBridge.ipcRenderer) {
      return window.tauriBridge.ipcRenderer.invoke(command, args);
    }
    return Promise.reject(new Error('Tauri not available'));
  }

  /**
   * Open external URL
   */
  function openExternal(url) {
    var tauri = window.__TAURI__;
    if (tauri && tauri.shell && tauri.shell.open) {
      tauri.shell.open(url);
      return;
    }
    if (window.tauriBridge && window.tauriBridge.shell) {
      window.tauriBridge.shell.openExternal(url);
      return;
    }
    window.open(url, '_blank');
  }

  // ============================================================================
  // Data Fetching
  // ============================================================================

  /**
   * Fetch all command center data from backend
   */
  function fetchData() {
    console.log('[CommandCenter] Fetching data...');
    return tauriInvoke('fetch_command_center_data')
      .then(function(data) {
        console.log('[CommandCenter] Data received:', data);
        state.data = data;
        render();
        return data;
      })
      .catch(function(err) {
        console.error('[CommandCenter] Failed to fetch data:', err);
        // Show error state in UI
        showFetchError(err);
        return null;
      });
  }

  /**
   * Show fetch error in UI
   */
  function showFetchError(err) {
    var message = err && err.message ? err.message : String(err);
    // Update panel badges to show error state
    var badges = ['linearIssueCount', 'sentryErrorCount', 'ciStatusCount'];
    badges.forEach(function(id) {
      var el = document.getElementById(id);
      if (el) {
        el.textContent = 'Error';
        el.className = 'command-panel-badge command-badge-error';
      }
    });
  }

  // ============================================================================
  // Rendering
  // ============================================================================

  /**
   * Main render function - updates all panels
   */
  function render() {
    if (!state.data) {
      renderEmptyState();
      return;
    }

    renderLinearIssues();
    renderSentryErrors();
    renderCiStatus();
    renderQuickActions();
    updateLastUpdated();
  }

  /**
   * Render empty state for all panels
   */
  function renderEmptyState() {
    // Linear
    var linearList = document.getElementById('linearIssuesList');
    if (linearList) {
      linearList.innerHTML = '<div class="command-empty-state">' +
        '<i class="fal fa-ticket"></i>' +
        '<p>No issues assigned</p>' +
        '<small>Connect Linear to see your issues</small>' +
        '</div>';
    }
    var linearCount = document.getElementById('linearIssueCount');
    if (linearCount) linearCount.textContent = '0 active';

    // Sentry
    var sentryList = document.getElementById('sentryErrorsList');
    if (sentryList) {
      sentryList.innerHTML = '<div class="command-empty-state">' +
        '<i class="fal fa-bug"></i>' +
        '<p>No unresolved errors</p>' +
        '<small>Connect Sentry to see errors</small>' +
        '</div>';
    }
    var sentryCount = document.getElementById('sentryErrorCount');
    if (sentryCount) sentryCount.textContent = '0 unresolved';

    // CI
    var ciList = document.getElementById('ciStatusList');
    if (ciList) {
      ciList.innerHTML = '<div class="command-empty-state">' +
        '<i class="fal fa-code-branch"></i>' +
        '<p>No recent workflows</p>' +
        '<small>Connect GitHub to see CI status</small>' +
        '</div>';
    }
    var ciCount = document.getElementById('ciStatusCount');
    if (ciCount) ciCount.textContent = '0 running';
  }

  /**
   * Render Linear issues panel
   */
  function renderLinearIssues() {
    var container = document.getElementById('linearIssuesList');
    var countEl = document.getElementById('linearIssueCount');
    if (!container) return;

    var issues = state.data.linearIssues || [];

    // Update count badge
    if (countEl) {
      countEl.textContent = formatCount(issues.length, 'active', 'active');
    }

    if (issues.length === 0) {
      container.innerHTML = '<div class="command-empty-state">' +
        '<i class="fal fa-ticket"></i>' +
        '<p>No issues assigned</p>' +
        '<small>Connect Linear to see your issues</small>' +
        '</div>';
      return;
    }

    var html = issues.map(function(issue) {
      var priorityClass = getPriorityClass(issue.priority);
      var labels = (issue.labels || []).map(function(label) {
        var labelClass = getLabelClass(label.name);
        return '<span class="command-issue-label ' + labelClass + '">' + escapeHtml(label.name) + '</span>';
      }).join('');

      return '<div class="command-issue-item" data-issue-id="' + escapeHtml(issue.id) + '" data-url="' + escapeHtml(issue.url) + '">' +
        '<div class="command-issue-header">' +
          '<span class="command-issue-priority ' + priorityClass + '"></span>' +
          '<span class="command-issue-id">' + escapeHtml(issue.identifier) + '</span>' +
          labels +
        '</div>' +
        '<div class="command-issue-title">' + escapeHtml(issue.title) + '</div>' +
        '<div class="command-issue-meta">' +
          (issue.assignee ? '<span><i class="fal fa-user"></i> ' + escapeHtml(issue.assignee) + '</span>' : '') +
          '<span><i class="fal fa-clock"></i> ' + formatTimeAgo(issue.updatedAt) + '</span>' +
        '</div>' +
      '</div>';
    }).join('');

    container.innerHTML = html;

    // Bind click events
    container.querySelectorAll('.command-issue-item').forEach(function(el) {
      el.addEventListener('click', function() {
        var url = el.getAttribute('data-url');
        if (url) openExternal(url);
      });
    });
  }

  /**
   * Render Sentry errors panel
   */
  function renderSentryErrors() {
    var container = document.getElementById('sentryErrorsList');
    var countEl = document.getElementById('sentryErrorCount');
    if (!container) return;

    var errors = state.data.sentryErrors || [];

    // Update count badge
    if (countEl) {
      countEl.textContent = formatCount(errors.length, 'unresolved', 'unresolved');
    }

    if (errors.length === 0) {
      container.innerHTML = '<div class="command-empty-state">' +
        '<i class="fal fa-bug"></i>' +
        '<p>No unresolved errors</p>' +
        '<small>Connect Sentry to see errors</small>' +
        '</div>';
      return;
    }

    var html = errors.map(function(error) {
      var isCritical = isCriticalError(error);
      var errorType = error.metadata && error.metadata.errorType ? error.metadata.errorType : 'Error';
      var errorValue = error.metadata && error.metadata.value ? error.metadata.value : error.title;
      var filename = error.metadata && error.metadata.filename ? error.metadata.filename : error.culprit;

      return '<div class="command-error-item' + (isCritical ? ' critical' : '') + '" data-error-id="' + escapeHtml(error.id) + '">' +
        '<div class="command-error-header">' +
          '<span class="command-error-type">' + escapeHtml(errorType) + '</span>' +
          '<span class="command-error-count">' + error.count + ' events</span>' +
        '</div>' +
        '<div class="command-error-message">' + escapeHtml(errorValue) + '</div>' +
        '<div class="command-error-meta">' +
          '<span class="command-error-location">' + escapeHtml(filename) + '</span>' +
          '<span class="command-error-time">' + formatTimeAgo(error.lastSeen) + '</span>' +
        '</div>' +
        '<div class="command-error-actions">' +
          '<button class="command-error-action-btn view-btn" data-url="' + escapeHtml(error.permalink) + '">' +
            '<i class="fal fa-external-link"></i> View' +
          '</button>' +
          '<button class="command-error-action-btn resolve-btn" data-error-id="' + escapeHtml(error.id) + '">' +
            '<i class="fal fa-check"></i> Resolve' +
          '</button>' +
          '<button class="command-error-action-btn fix command-fix-btn" data-error-id="' + escapeHtml(error.id) + '">' +
            '<i class="fal fa-robot"></i> Fix with Agent' +
          '</button>' +
        '</div>' +
      '</div>';
    }).join('');

    container.innerHTML = html;

    // Bind click events
    container.querySelectorAll('.view-btn').forEach(function(btn) {
      btn.addEventListener('click', function(e) {
        e.stopPropagation();
        var url = btn.getAttribute('data-url');
        if (url) openExternal(url);
      });
    });

    container.querySelectorAll('.resolve-btn').forEach(function(btn) {
      btn.addEventListener('click', function(e) {
        e.stopPropagation();
        var errorId = btn.getAttribute('data-error-id');
        resolveError(errorId);
      });
    });

    container.querySelectorAll('.command-fix-btn').forEach(function(btn) {
      btn.addEventListener('click', function(e) {
        e.stopPropagation();
        var errorId = btn.getAttribute('data-error-id');
        var error = errors.find(function(err) { return err.id === errorId; });
        if (error) {
          openAgentPopup('error', error);
        }
      });
    });
  }

  /**
   * Render CI/CD status panel
   */
  function renderCiStatus() {
    var container = document.getElementById('ciStatusList');
    var countEl = document.getElementById('ciStatusCount');
    if (!container) return;

    var workflows = state.data.githubWorkflows || [];

    // Count running workflows
    var runningCount = workflows.filter(function(w) {
      return w.status === 'in_progress' || w.status === 'queued';
    }).length;

    // Update count badge
    if (countEl) {
      countEl.textContent = formatCount(runningCount, 'running', 'running');
    }

    if (workflows.length === 0) {
      container.innerHTML = '<div class="command-empty-state">' +
        '<i class="fal fa-code-branch"></i>' +
        '<p>No recent workflows</p>' +
        '<small>Connect GitHub to see CI status</small>' +
        '</div>';
      return;
    }

    var html = workflows.map(function(workflow) {
      var statusClass = getCiStatusClass(workflow.status, workflow.conclusion);
      var showActions = workflow.conclusion === 'failure';

      return '<div class="command-ci-item" data-workflow-id="' + workflow.id + '" data-url="' + escapeHtml(workflow.htmlUrl) + '">' +
        '<div class="command-ci-header">' +
          '<span class="command-ci-status ' + statusClass + '"></span>' +
          '<span class="command-ci-branch">' + escapeHtml(workflow.branch) + '</span>' +
        '</div>' +
        '<div class="command-ci-workflow">' + escapeHtml(workflow.name) + '</div>' +
        '<div class="command-ci-meta">' +
          '<span class="command-ci-duration"><i class="fal fa-clock"></i> ' + formatDuration(workflow.durationSeconds) + '</span>' +
          '<span>' + formatTimeAgo(workflow.createdAt) + '</span>' +
        '</div>' +
        (showActions ? '<div class="command-ci-actions">' +
          '<button class="command-ci-action-btn logs-btn" data-url="' + escapeHtml(workflow.htmlUrl) + '">' +
            '<i class="fal fa-file-alt"></i> Logs' +
          '</button>' +
          '<button class="command-ci-action-btn rerun-btn" data-workflow-id="' + workflow.id + '" data-repo="' + escapeHtml(workflow.repo) + '">' +
            '<i class="fal fa-redo"></i> Re-run' +
          '</button>' +
        '</div>' : '') +
      '</div>';
    }).join('');

    container.innerHTML = html;

    // Bind click events
    container.querySelectorAll('.command-ci-item').forEach(function(el) {
      el.addEventListener('click', function(e) {
        // Don't navigate if clicking on action buttons
        if (e.target.closest('.command-ci-actions')) return;
        var url = el.getAttribute('data-url');
        if (url) openExternal(url);
      });
    });

    container.querySelectorAll('.logs-btn').forEach(function(btn) {
      btn.addEventListener('click', function(e) {
        e.stopPropagation();
        var url = btn.getAttribute('data-url');
        if (url) openExternal(url);
      });
    });

    container.querySelectorAll('.rerun-btn').forEach(function(btn) {
      btn.addEventListener('click', function(e) {
        e.stopPropagation();
        var workflowId = btn.getAttribute('data-workflow-id');
        var repo = btn.getAttribute('data-repo');
        rerunWorkflow(repo, workflowId);
      });
    });
  }

  /**
   * Render quick action buttons
   */
  function renderQuickActions() {
    // Fix Top Error
    var fixDesc = document.getElementById('quickFixTopErrorDesc');
    var fixBtn = document.getElementById('quickFixTopError');
    if (fixDesc && state.data.sentryErrors && state.data.sentryErrors.length > 0) {
      var topError = state.data.sentryErrors[0];
      var errorType = topError.metadata && topError.metadata.errorType ? topError.metadata.errorType : 'Error';
      fixDesc.textContent = errorType + ' (' + topError.count + ' events)';
      if (fixBtn) fixBtn.classList.remove('disabled');
    } else if (fixDesc) {
      fixDesc.textContent = 'No errors detected';
      if (fixBtn) fixBtn.classList.add('disabled');
    }

    // Start Next Issue
    var issueDesc = document.getElementById('quickStartNextIssueDesc');
    var issueBtn = document.getElementById('quickStartNextIssue');
    if (issueDesc && state.data.linearIssues && state.data.linearIssues.length > 0) {
      var topIssue = state.data.linearIssues[0];
      issueDesc.textContent = topIssue.identifier + ': ' + (topIssue.title.length > 30 ? topIssue.title.substring(0, 30) + '...' : topIssue.title);
      if (issueBtn) issueBtn.classList.remove('disabled');
    } else if (issueDesc) {
      issueDesc.textContent = 'No issues assigned';
      if (issueBtn) issueBtn.classList.add('disabled');
    }

    // Re-run Failed CI
    var ciDesc = document.getElementById('quickRerunFailedCIDesc');
    var ciBtn = document.getElementById('quickRerunFailedCI');
    var failedWorkflows = (state.data.githubWorkflows || []).filter(function(w) {
      return w.conclusion === 'failure';
    });
    if (ciDesc && failedWorkflows.length > 0) {
      var topFailed = failedWorkflows[0];
      ciDesc.textContent = topFailed.branch + ' (' + failedWorkflows.length + ' failed)';
      if (ciBtn) ciBtn.classList.remove('disabled');
    } else if (ciDesc) {
      ciDesc.textContent = 'All builds passing';
      if (ciBtn) ciBtn.classList.add('disabled');
    }
  }

  /**
   * Update last updated timestamp
   */
  function updateLastUpdated() {
    if (state.data && state.data.lastUpdated) {
      console.log('[CommandCenter] Last updated:', state.data.lastUpdated);
    }
  }

  // ============================================================================
  // Actions
  // ============================================================================

  /**
   * Resolve a Sentry error
   */
  function resolveError(errorId) {
    console.log('[CommandCenter] Resolving error:', errorId);
    tauriInvoke('cc_resolve_sentry_issue', { issueId: errorId })
      .then(function() {
        console.log('[CommandCenter] Error resolved');
        // Refresh data
        fetchData();
        if (typeof sendNotification === 'function') {
          sendNotification('Error resolved', 'green');
        }
      })
      .catch(function(err) {
        console.error('[CommandCenter] Failed to resolve error:', err);
        if (typeof sendNotification === 'function') {
          sendNotification('Failed to resolve error: ' + err, 'red');
        }
      });
  }

  /**
   * Re-run a failed workflow
   */
  function rerunWorkflow(repo, runId) {
    console.log('[CommandCenter] Re-running workflow:', repo, runId);
    tauriInvoke('cc_rerun_github_workflow', { repo: repo, runId: parseInt(runId, 10) })
      .then(function() {
        console.log('[CommandCenter] Workflow re-run triggered');
        // Refresh data after a delay to allow GitHub to update
        setTimeout(fetchData, 2000);
        if (typeof sendNotification === 'function') {
          sendNotification('Workflow re-run triggered', 'green');
        }
      })
      .catch(function(err) {
        console.error('[CommandCenter] Failed to re-run workflow:', err);
        if (typeof sendNotification === 'function') {
          sendNotification('Failed to re-run workflow: ' + err, 'red');
        }
      });
  }

  /**
   * Quick action: Fix top error
   */
  function fixTopError() {
    if (!state.data || !state.data.sentryErrors || state.data.sentryErrors.length === 0) {
      return;
    }
    openAgentPopup('error', state.data.sentryErrors[0]);
  }

  /**
   * Quick action: Start next issue
   */
  function startNextIssue() {
    if (!state.data || !state.data.linearIssues || state.data.linearIssues.length === 0) {
      return;
    }
    openAgentPopup('issue', state.data.linearIssues[0]);
  }

  /**
   * Quick action: Re-run failed CI
   */
  function rerunFailedCI() {
    var failedWorkflows = (state.data && state.data.githubWorkflows || []).filter(function(w) {
      return w.conclusion === 'failure';
    });
    if (failedWorkflows.length === 0) return;

    var topFailed = failedWorkflows[0];
    rerunWorkflow(topFailed.repo, topFailed.id);
  }

  // ============================================================================
  // Agent Popup
  // ============================================================================

  /**
   * Open the agent popup for creating a task
   */
  function openAgentPopup(type, item) {
    state.popupContext = { type: type, item: item };

    var popup = document.getElementById('commandAgentPopup');
    var titleEl = document.getElementById('commandAgentErrorTitle');
    var fileEl = document.getElementById('commandAgentErrorFile');
    var promptInput = document.getElementById('commandAgentPromptInput');

    if (!popup) return;

    // Update preview
    if (type === 'error' && item) {
      var errorType = item.metadata && item.metadata.errorType ? item.metadata.errorType : 'Error';
      var errorValue = item.metadata && item.metadata.value ? item.metadata.value : item.title;
      var filename = item.metadata && item.metadata.filename ? item.metadata.filename : item.culprit;

      if (titleEl) titleEl.textContent = errorType + ': ' + errorValue;
      if (fileEl) fileEl.textContent = filename + ' · ' + item.count + ' events';
      if (promptInput) {
        promptInput.value = 'Fix the ' + errorType + ' in ' + filename + '. ' +
          'The error message is: "' + errorValue + '". ' +
          'Add proper error handling and ensure the fix prevents this error from recurring.';
      }
    } else if (type === 'issue' && item) {
      if (titleEl) titleEl.textContent = item.identifier + ': ' + item.title;
      if (fileEl) fileEl.textContent = 'Priority: ' + item.priority + ' · ' + (item.state ? item.state.name : 'Open');
      if (promptInput) {
        promptInput.value = 'Implement ' + item.identifier + ': ' + item.title + '\n\n' +
          'Please implement this feature/fix according to the issue description.';
      }
    }

    // Show popup and backdrop
    var backdrop = document.getElementById('commandAgentPopupBackdrop');
    if (backdrop) backdrop.removeAttribute('hidden');
    popup.removeAttribute('hidden');
  }

  /**
   * Close the agent popup
   */
  function closeAgentPopup() {
    var popup = document.getElementById('commandAgentPopup');
    var backdrop = document.getElementById('commandAgentPopupBackdrop');
    if (popup) {
      popup.setAttribute('hidden', '');
    }
    if (backdrop) {
      backdrop.setAttribute('hidden', '');
    }
    state.popupContext = null;
  }

  /**
   * Create a task from the popup
   */
  function createTaskFromPopup() {
    var promptInput = document.getElementById('commandAgentPromptInput');
    var prompt = promptInput ? promptInput.value : '';
    var agent = state.selectedAgent;

    if (!prompt.trim()) {
      if (typeof sendNotification === 'function') {
        sendNotification('Please enter a prompt', 'red');
      }
      return;
    }

    // Use the tauri bridge to create a session
    if (window.tauriBridge && window.tauriBridge.ipcRenderer) {
      var payload = {
        agentId: agent,
        prompt: prompt,
        projectPath: null, // Will use current project
        planMode: false,
        thinking: false,
        useWorktree: false,
        permissionMode: 'default',
        execModel: 'default'
      };

      window.tauriBridge.ipcRenderer.send('CreateAgentSession', payload);
      closeAgentPopup();

      if (typeof sendNotification === 'function') {
        sendNotification('Creating task with ' + agent + '...', 'green');
      }
    } else {
      console.error('[CommandCenter] Tauri bridge not available');
      if (typeof sendNotification === 'function') {
        sendNotification('Failed to create task: Tauri bridge not available', 'red');
      }
    }
  }

  // ============================================================================
  // Auto-refresh
  // ============================================================================

  /**
   * Start auto-refresh timer
   */
  function startAutoRefresh() {
    stopAutoRefresh();
    var intervalMs = state.config.refreshInterval * 60 * 1000;
    console.log('[CommandCenter] Starting auto-refresh, interval:', intervalMs, 'ms');
    state.refreshTimer = setInterval(function() {
      if (state.isVisible) {
        fetchData();
      }
    }, intervalMs);
  }

  /**
   * Stop auto-refresh timer
   */
  function stopAutoRefresh() {
    if (state.refreshTimer) {
      clearInterval(state.refreshTimer);
      state.refreshTimer = null;
    }
  }

  // ============================================================================
  // Visibility Handling
  // ============================================================================

  /**
   * Called when Command Center page becomes visible
   */
  function onPageVisible() {
    console.log('[CommandCenter] Page visible');
    state.isVisible = true;
    fetchData();
    startAutoRefresh();
  }

  /**
   * Called when Command Center page becomes hidden
   */
  function onPageHidden() {
    console.log('[CommandCenter] Page hidden');
    state.isVisible = false;
    stopAutoRefresh();
  }

  // ============================================================================
  // Configuration
  // ============================================================================

  /**
   * Open configuration modal (delegates to CommandCenterConfig if available)
   */
  function openConfigModal() {
    if (window.CommandCenterConfig && typeof window.CommandCenterConfig.open === 'function') {
      window.CommandCenterConfig.open();
    } else {
      console.log('[CommandCenter] Config modal not available');
      if (typeof sendNotification === 'function') {
        sendNotification('Configuration coming soon', 'blue');
      }
    }
  }

  /**
   * Update configuration
   */
  function updateConfig(newConfig) {
    Object.assign(state.config, newConfig);
    // Restart auto-refresh with new interval
    if (state.isVisible) {
      startAutoRefresh();
    }
  }

  // ============================================================================
  // Event Binding
  // ============================================================================

  /**
   * Bind all event listeners
   */
  function bindEvents() {
    // Refresh button
    var refreshBtn = document.getElementById('commandRefreshBtn');
    if (refreshBtn) {
      refreshBtn.addEventListener('click', function() {
        refreshBtn.classList.add('spinning');
        fetchData().finally(function() {
          setTimeout(function() {
            refreshBtn.classList.remove('spinning');
          }, 500);
        });
      });
    }

    // Configure button
    var configBtn = document.getElementById('commandConfigureBtn');
    if (configBtn) {
      configBtn.addEventListener('click', openConfigModal);
    }

    // Quick actions
    var fixTopErrorBtn = document.getElementById('quickFixTopError');
    if (fixTopErrorBtn) {
      fixTopErrorBtn.addEventListener('click', fixTopError);
    }

    var startNextIssueBtn = document.getElementById('quickStartNextIssue');
    if (startNextIssueBtn) {
      startNextIssueBtn.addEventListener('click', startNextIssue);
    }

    var rerunFailedCIBtn = document.getElementById('quickRerunFailedCI');
    if (rerunFailedCIBtn) {
      rerunFailedCIBtn.addEventListener('click', rerunFailedCI);
    }

    // Agent popup
    var popupClose = document.getElementById('commandAgentPopupClose');
    if (popupClose) {
      popupClose.addEventListener('click', closeAgentPopup);
    }

    var cancelBtn = document.getElementById('commandAgentCancelBtn');
    if (cancelBtn) {
      cancelBtn.addEventListener('click', closeAgentPopup);
    }

    // Clicking backdrop closes popup
    var popupBackdrop = document.getElementById('commandAgentPopupBackdrop');
    if (popupBackdrop) {
      popupBackdrop.addEventListener('click', closeAgentPopup);
    }

    var createBtn = document.getElementById('commandAgentCreateBtn');
    if (createBtn) {
      createBtn.addEventListener('click', createTaskFromPopup);
    }

    // Agent selection
    document.querySelectorAll('.command-agent-option').forEach(function(option) {
      option.addEventListener('click', function() {
        document.querySelectorAll('.command-agent-option').forEach(function(opt) {
          opt.classList.remove('selected');
        });
        option.classList.add('selected');
        state.selectedAgent = option.getAttribute('data-agent');
      });
    });

    // Listen for navigation events
    window.addEventListener('PhantomNavigate', function(e) {
      if (e.detail && e.detail.pageId === 'commandPage') {
        onPageVisible();
      } else if (state.isVisible) {
        onPageHidden();
      }
    });
  }

  // ============================================================================
  // Initialization
  // ============================================================================

  /**
   * Initialize the Command Center module
   */
  function init() {
    console.log('[CommandCenter] Initializing...');

    // Bind events
    bindEvents();

    // Render empty state
    renderEmptyState();

    // Check if we're already on the command page
    var commandPage = document.getElementById('commandPage');
    if (commandPage && !commandPage.hasAttribute('hidden')) {
      onPageVisible();
    }

    console.log('[CommandCenter] Initialized');
  }

  // Initialize on DOMContentLoaded
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  // ============================================================================
  // Public API
  // ============================================================================

  window.CommandCenter = {
    init: init,
    refresh: fetchData,
    render: render,
    onPageVisible: onPageVisible,
    onPageHidden: onPageHidden,
    openAgentPopup: openAgentPopup,
    closeAgentPopup: closeAgentPopup,
    openConfigModal: openConfigModal,
    updateConfig: updateConfig,
    getState: function() { return state; }
  };

})();
