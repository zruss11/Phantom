/**
 * Command Center Configuration Modal
 * Manages settings for GitHub, Linear, Sentry integrations and general preferences.
 */
(function() {
  'use strict';

  // ============================================================================
  // Module State
  // ============================================================================
  var state = {
    settings: null,
    ghCliAvailable: false,
    linearProjects: [],
    sentryOrganizations: [],
    sentryProjects: [],
    githubRepos: []
  };

  // ============================================================================
  // Utility Functions
  // ============================================================================

  /**
   * Invoke Tauri command
   */
  function tauriInvoke(command, args) {
    var tauri = window.__TAURI__;
    if (tauri && tauri.core && tauri.core.invoke) {
      return tauri.core.invoke(command, args || {});
    }
    if (window.tauriBridge && window.tauriBridge.ipcRenderer) {
      return window.tauriBridge.ipcRenderer.invoke(command, args);
    }
    return Promise.reject(new Error('Tauri not available'));
  }

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
   * Show notification
   */
  function notify(message, type) {
    if (typeof window.sendNotification === 'function') {
      window.sendNotification(message, type || 'blue');
    } else {
      console.log('[CommandCenterConfig]', message);
    }
  }

  // ============================================================================
  // Modal Management
  // ============================================================================

  /**
   * Open the configuration modal
   */
  function open() {
    console.log('[CommandCenterConfig] Opening modal...');

    // Load settings first
    loadSettings()
      .then(function() {
        // Show the modal
        var modal = document.getElementById('commandCenterConfigModal');
        if (modal) {
          $(modal).modal('show');
        }

        // Check gh CLI status
        checkGhCli();
      })
      .catch(function(err) {
        console.error('[CommandCenterConfig] Failed to load settings:', err);
        notify('Failed to load settings: ' + err, 'red');
      });
  }

  /**
   * Close the configuration modal
   */
  function close() {
    var modal = document.getElementById('commandCenterConfigModal');
    if (modal) {
      $(modal).modal('hide');
    }
  }

  // ============================================================================
  // Settings Management
  // ============================================================================

  /**
   * Load current settings from backend
   */
  function loadSettings() {
    return tauriInvoke('get_settings')
      .then(function(settings) {
        console.log('[CommandCenterConfig] Settings loaded:', settings);
        state.settings = settings || {};
        state.githubRepos = settings.githubWatchedRepos || [];
        populateForm();
        return settings;
      });
  }

  /**
   * Save settings to backend
   */
  function save() {
    var settings = collectFormData();
    console.log('[CommandCenterConfig] Saving settings:', settings);

    // Show saving indicator
    var saveBtn = document.getElementById('ccConfigSaveBtn');
    if (saveBtn) {
      saveBtn.disabled = true;
      saveBtn.innerHTML = '<i class="fal fa-spinner fa-spin"></i> Saving...';
    }

    return tauriInvoke('save_settings', { settings: settings })
      .then(function() {
        console.log('[CommandCenterConfig] Settings saved');
        notify('Settings saved', 'green');

        // Trigger Command Center refresh
        if (window.CommandCenter && typeof window.CommandCenter.refresh === 'function') {
          window.CommandCenter.refresh();
        }
        if (window.CommandCenter && typeof window.CommandCenter.updateConfig === 'function') {
          window.CommandCenter.updateConfig({
            refreshInterval: settings.commandCenterRefreshInterval || 15
          });
        }

        close();
      })
      .catch(function(err) {
        console.error('[CommandCenterConfig] Failed to save settings:', err);
        notify('Failed to save settings: ' + err, 'red');
      })
      .finally(function() {
        if (saveBtn) {
          saveBtn.disabled = false;
          saveBtn.innerHTML = '<i class="fal fa-save"></i> Save';
        }
      });
  }

  /**
   * Collect form data into settings object
   */
  function collectFormData() {
    var settings = Object.assign({}, state.settings);

    // General
    settings.commandCenterEnabled = document.getElementById('ccEnabledToggle')?.checked ?? true;
    var refreshSlider = document.getElementById('ccRefreshInterval');
    settings.commandCenterRefreshInterval = refreshSlider ? parseInt(refreshSlider.value, 10) : 15;

    // GitHub
    var ghAuthMethod = document.querySelector('input[name="ghAuthMethod"]:checked');
    settings.githubAuthMethod = ghAuthMethod ? ghAuthMethod.value : 'gh-cli';
    settings.githubToken = document.getElementById('ccGithubToken')?.value || '';
    settings.githubWatchedRepos = state.githubRepos.slice();

    // Linear
    settings.linearToken = document.getElementById('ccLinearToken')?.value || '';
    settings.linearWatchedProjects = getCheckedValues('ccLinearProjectsList');

    // Sentry
    settings.sentryToken = document.getElementById('ccSentryToken')?.value || '';
    var sentryOrgSelect = document.getElementById('ccSentryOrg');
    settings.sentryOrganization = sentryOrgSelect ? sentryOrgSelect.value : '';
    settings.sentryWatchedProjects = getCheckedValues('ccSentryProjectsList');

    return settings;
  }

  /**
   * Get checked checkbox values from a container
   */
  function getCheckedValues(containerId) {
    var container = document.getElementById(containerId);
    if (!container) return [];

    var values = [];
    container.querySelectorAll('input[type="checkbox"]:checked').forEach(function(cb) {
      values.push(cb.value);
    });
    return values;
  }

  /**
   * Populate form with current settings
   */
  function populateForm() {
    var s = state.settings || {};

    // General
    var enabledToggle = document.getElementById('ccEnabledToggle');
    if (enabledToggle) {
      enabledToggle.checked = s.commandCenterEnabled !== false;
    }

    var refreshSlider = document.getElementById('ccRefreshInterval');
    var refreshValue = document.getElementById('ccRefreshIntervalValue');
    if (refreshSlider) {
      refreshSlider.value = s.commandCenterRefreshInterval || 15;
    }
    if (refreshValue) {
      refreshValue.textContent = (s.commandCenterRefreshInterval || 15) + ' min';
    }

    // GitHub
    var ghMethod = s.githubAuthMethod || 'gh-cli';
    var ghRadio = document.querySelector('input[name="ghAuthMethod"][value="' + ghMethod + '"]');
    if (ghRadio) ghRadio.checked = true;
    updateGithubAuthUI(ghMethod);

    var ghToken = document.getElementById('ccGithubToken');
    if (ghToken) ghToken.value = s.githubToken || '';

    renderGithubReposList();

    // Linear
    var linearToken = document.getElementById('ccLinearToken');
    if (linearToken) linearToken.value = s.linearToken || '';

    // Sentry
    var sentryToken = document.getElementById('ccSentryToken');
    if (sentryToken) sentryToken.value = s.sentryToken || '';
  }

  // ============================================================================
  // GitHub Integration
  // ============================================================================

  /**
   * Check if gh CLI is authenticated
   */
  function checkGhCli() {
    var statusEl = document.getElementById('ccGhCliStatus');
    if (statusEl) {
      statusEl.innerHTML = '<i class="fal fa-spinner fa-spin"></i> Checking...';
    }

    return tauriInvoke('check_gh_cli_auth')
      .then(function(result) {
        console.log('[CommandCenterConfig] gh CLI auth status:', result);
        state.ghCliAvailable = result && result.authenticated;

        if (statusEl) {
          if (state.ghCliAvailable) {
            statusEl.innerHTML = '<i class="fal fa-check-circle text-success"></i> Authenticated as ' + escapeHtml(result.username || 'user');
          } else {
            statusEl.innerHTML = '<i class="fal fa-exclamation-circle text-warning"></i> Not authenticated';
          }
        }
      })
      .catch(function(err) {
        console.error('[CommandCenterConfig] Failed to check gh CLI:', err);
        state.ghCliAvailable = false;
        if (statusEl) {
          statusEl.innerHTML = '<i class="fal fa-times-circle text-danger"></i> gh CLI not found';
        }
      });
  }

  /**
   * Update GitHub auth UI based on selected method
   */
  function updateGithubAuthUI(method) {
    var ghCliSection = document.getElementById('ccGhCliSection');
    var tokenSection = document.getElementById('ccGhTokenSection');

    if (method === 'gh-cli') {
      if (ghCliSection) ghCliSection.style.display = 'block';
      if (tokenSection) tokenSection.style.display = 'none';
    } else {
      if (ghCliSection) ghCliSection.style.display = 'none';
      if (tokenSection) tokenSection.style.display = 'block';
    }
  }

  /**
   * Add a GitHub repo to watch list
   */
  function addGithubRepo() {
    var input = document.getElementById('ccGithubRepoInput');
    if (!input) return;

    var repo = input.value.trim();
    if (!repo) {
      notify('Please enter a repository (owner/repo)', 'red');
      return;
    }

    // Validate format
    if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) {
      notify('Invalid format. Use owner/repo', 'red');
      return;
    }

    // Check for duplicates
    if (state.githubRepos.includes(repo)) {
      notify('Repository already in list', 'red');
      return;
    }

    state.githubRepos.push(repo);
    input.value = '';
    renderGithubReposList();
  }

  /**
   * Remove a GitHub repo from watch list
   */
  function removeGithubRepo(repo) {
    var idx = state.githubRepos.indexOf(repo);
    if (idx !== -1) {
      state.githubRepos.splice(idx, 1);
      renderGithubReposList();
    }
  }

  /**
   * Render the GitHub repos list
   */
  function renderGithubReposList() {
    var container = document.getElementById('ccGithubReposList');
    if (!container) return;

    if (state.githubRepos.length === 0) {
      container.innerHTML = '<div class="cc-empty-list">No repositories added</div>';
      return;
    }

    var html = state.githubRepos.map(function(repo) {
      return '<div class="cc-repo-item">' +
        '<span class="cc-repo-name"><i class="fab fa-github"></i> ' + escapeHtml(repo) + '</span>' +
        '<button type="button" class="cc-remove-btn" data-repo="' + escapeHtml(repo) + '">' +
          '<i class="fal fa-times"></i>' +
        '</button>' +
      '</div>';
    }).join('');

    container.innerHTML = html;

    // Bind remove buttons
    container.querySelectorAll('.cc-remove-btn').forEach(function(btn) {
      btn.addEventListener('click', function() {
        var repo = btn.getAttribute('data-repo');
        removeGithubRepo(repo);
      });
    });
  }

  // ============================================================================
  // Linear Integration
  // ============================================================================

  /**
   * Load Linear projects
   */
  function loadLinearProjects() {
    var token = document.getElementById('ccLinearToken')?.value;
    if (!token) {
      notify('Please enter a Linear API key first', 'red');
      return;
    }

    var btn = document.getElementById('ccLoadLinearProjectsBtn');
    var container = document.getElementById('ccLinearProjectsList');

    if (btn) {
      btn.disabled = true;
      btn.innerHTML = '<i class="fal fa-spinner fa-spin"></i> Loading...';
    }

    tauriInvoke('cc_fetch_linear_projects', { token: token })
      .then(function(projects) {
        console.log('[CommandCenterConfig] Linear projects loaded:', projects);
        state.linearProjects = projects || [];
        renderLinearProjects();
      })
      .catch(function(err) {
        console.error('[CommandCenterConfig] Failed to load Linear projects:', err);
        notify('Failed to load Linear projects: ' + err, 'red');
        if (container) {
          container.innerHTML = '<div class="cc-empty-list text-danger">Failed to load projects</div>';
        }
      })
      .finally(function() {
        if (btn) {
          btn.disabled = false;
          btn.innerHTML = '<i class="fal fa-sync-alt"></i> Load Projects';
        }
      });
  }

  /**
   * Render Linear projects checkboxes
   */
  function renderLinearProjects() {
    var container = document.getElementById('ccLinearProjectsList');
    if (!container) return;

    if (state.linearProjects.length === 0) {
      container.innerHTML = '<div class="cc-empty-list">No projects found</div>';
      return;
    }

    var watchedProjects = state.settings?.linearWatchedProjects || [];

    var html = state.linearProjects.map(function(project) {
      var isChecked = watchedProjects.includes(project.id);
      return '<div class="cc-checkbox-item">' +
        '<label class="cc-checkbox-label">' +
          '<input type="checkbox" value="' + escapeHtml(project.id) + '"' + (isChecked ? ' checked' : '') + '>' +
          '<span class="cc-checkbox-text">' + escapeHtml(project.name) + '</span>' +
        '</label>' +
      '</div>';
    }).join('');

    container.innerHTML = html;
  }

  // ============================================================================
  // Sentry Integration
  // ============================================================================

  /**
   * Load Sentry organizations
   */
  function loadSentryOrgs() {
    var token = document.getElementById('ccSentryToken')?.value;
    if (!token) {
      notify('Please enter a Sentry auth token first', 'red');
      return;
    }

    var btn = document.getElementById('ccLoadSentryOrgsBtn');
    var select = document.getElementById('ccSentryOrg');

    if (btn) {
      btn.disabled = true;
      btn.innerHTML = '<i class="fal fa-spinner fa-spin"></i> Loading...';
    }

    tauriInvoke('cc_fetch_sentry_organizations', { token: token })
      .then(function(orgs) {
        console.log('[CommandCenterConfig] Sentry orgs loaded:', orgs);
        state.sentryOrganizations = orgs || [];
        renderSentryOrgs();
      })
      .catch(function(err) {
        console.error('[CommandCenterConfig] Failed to load Sentry orgs:', err);
        notify('Failed to load Sentry organizations: ' + err, 'red');
      })
      .finally(function() {
        if (btn) {
          btn.disabled = false;
          btn.innerHTML = '<i class="fal fa-sync-alt"></i> Load Organizations';
        }
      });
  }

  /**
   * Render Sentry organizations dropdown
   */
  function renderSentryOrgs() {
    var select = document.getElementById('ccSentryOrg');
    if (!select) return;

    var currentOrg = state.settings?.sentryOrganization || '';

    var html = '<option value="">Select an organization...</option>';
    html += state.sentryOrganizations.map(function(org) {
      var isSelected = org.slug === currentOrg;
      return '<option value="' + escapeHtml(org.slug) + '"' + (isSelected ? ' selected' : '') + '>' +
        escapeHtml(org.name) +
      '</option>';
    }).join('');

    select.innerHTML = html;

    // Enable project loading if org is selected
    if (currentOrg) {
      loadSentryProjects();
    }
  }

  /**
   * Load Sentry projects for selected organization
   */
  function loadSentryProjects() {
    var token = document.getElementById('ccSentryToken')?.value;
    var org = document.getElementById('ccSentryOrg')?.value;

    if (!token || !org) {
      return;
    }

    var container = document.getElementById('ccSentryProjectsList');
    if (container) {
      container.innerHTML = '<div class="cc-loading"><i class="fal fa-spinner fa-spin"></i> Loading projects...</div>';
    }

    tauriInvoke('cc_fetch_sentry_projects', { token: token, org: org })
      .then(function(projects) {
        console.log('[CommandCenterConfig] Sentry projects loaded:', projects);
        state.sentryProjects = projects || [];
        renderSentryProjects();
      })
      .catch(function(err) {
        console.error('[CommandCenterConfig] Failed to load Sentry projects:', err);
        if (container) {
          container.innerHTML = '<div class="cc-empty-list text-danger">Failed to load projects</div>';
        }
      });
  }

  /**
   * Render Sentry projects checkboxes
   */
  function renderSentryProjects() {
    var container = document.getElementById('ccSentryProjectsList');
    if (!container) return;

    if (state.sentryProjects.length === 0) {
      container.innerHTML = '<div class="cc-empty-list">No projects found</div>';
      return;
    }

    var watchedProjects = state.settings?.sentryWatchedProjects || [];

    var html = state.sentryProjects.map(function(project) {
      var isChecked = watchedProjects.includes(project.slug);
      return '<div class="cc-checkbox-item">' +
        '<label class="cc-checkbox-label">' +
          '<input type="checkbox" value="' + escapeHtml(project.slug) + '"' + (isChecked ? ' checked' : '') + '>' +
          '<span class="cc-checkbox-text">' + escapeHtml(project.name) + '</span>' +
        '</label>' +
      '</div>';
    }).join('');

    container.innerHTML = html;
  }

  // ============================================================================
  // Event Binding
  // ============================================================================

  /**
   * Bind all event handlers
   */
  function bindEvents() {
    // Save button
    var saveBtn = document.getElementById('ccConfigSaveBtn');
    if (saveBtn) {
      saveBtn.addEventListener('click', save);
    }

    // Cancel button
    var cancelBtn = document.getElementById('ccConfigCancelBtn');
    if (cancelBtn) {
      cancelBtn.addEventListener('click', close);
    }

    // GitHub auth method toggle
    document.querySelectorAll('input[name="ghAuthMethod"]').forEach(function(radio) {
      radio.addEventListener('change', function() {
        updateGithubAuthUI(radio.value);
      });
    });

    // Add GitHub repo
    var addRepoBtn = document.getElementById('ccAddGithubRepoBtn');
    if (addRepoBtn) {
      addRepoBtn.addEventListener('click', addGithubRepo);
    }

    var repoInput = document.getElementById('ccGithubRepoInput');
    if (repoInput) {
      repoInput.addEventListener('keypress', function(e) {
        if (e.key === 'Enter') {
          e.preventDefault();
          addGithubRepo();
        }
      });
    }

    // Load Linear projects
    var loadLinearBtn = document.getElementById('ccLoadLinearProjectsBtn');
    if (loadLinearBtn) {
      loadLinearBtn.addEventListener('click', loadLinearProjects);
    }

    // Load Sentry organizations
    var loadSentryOrgsBtn = document.getElementById('ccLoadSentryOrgsBtn');
    if (loadSentryOrgsBtn) {
      loadSentryOrgsBtn.addEventListener('click', loadSentryOrgs);
    }

    // Sentry org change - load projects
    var sentryOrgSelect = document.getElementById('ccSentryOrg');
    if (sentryOrgSelect) {
      sentryOrgSelect.addEventListener('change', loadSentryProjects);
    }

    // Refresh interval slider
    var refreshSlider = document.getElementById('ccRefreshInterval');
    var refreshValue = document.getElementById('ccRefreshIntervalValue');
    if (refreshSlider && refreshValue) {
      refreshSlider.addEventListener('input', function() {
        refreshValue.textContent = refreshSlider.value + ' min';
      });
    }
  }

  // ============================================================================
  // Initialization
  // ============================================================================

  /**
   * Initialize the config module
   */
  function init() {
    console.log('[CommandCenterConfig] Initializing...');
    bindEvents();
    console.log('[CommandCenterConfig] Initialized');
  }

  // Initialize when DOM is ready
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }

  // ============================================================================
  // Public API
  // ============================================================================

  window.CommandCenterConfig = {
    open: open,
    close: close,
    save: save,
    loadSettings: loadSettings,
    checkGhCli: checkGhCli,
    loadLinearProjects: loadLinearProjects,
    loadSentryOrgs: loadSentryOrgs,
    loadSentryProjects: loadSentryProjects,
    addGithubRepo: addGithubRepo,
    removeGithubRepo: removeGithubRepo
  };

})();
