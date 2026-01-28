/**
 * Tauri bridge for the Phantom GUI.
 * Provides IPC/event shims and small API helpers without Electron.
 */
(function() {
  'use strict';

  var tauri = window.__TAURI__ || null;
  // Tauri v2 API structure - invoke is under core, window API varies by config
  var tauriCore = tauri && tauri.core ? tauri.core : null;
  var tauriInvoke = tauriCore && tauriCore.invoke ? tauriCore.invoke : null;
  var tauriEvents = tauri && tauri.event ? tauri.event : null;
  var tauriShell = tauri && tauri.shell ? tauri.shell : null;
  var tauriClipboard = tauri && tauri.clipboard ? tauri.clipboard : null;
  // Window API: withGlobalTauri uses tauri.window, otherwise tauri.webviewWindow
  var tauriWindow = tauri && (tauri.window || tauri.webviewWindow) ? (tauri.window || tauri.webviewWindow) : null;
  var tauriApp = tauri && tauri.app ? tauri.app : null;

  var mockData = {
    accountLists: [],
    tasks: [],
    settings: {
      discordEnabled: false,
      discordBotToken: '',
      discordChannelId: '',
      retryDelay: 1000,
      errorDelay: 2000,
      openaiApiKey: '',
      anthropicApiKey: '',
      codexAuthMethod: '',
      claudeAuthMethod: '',
      taskProjectAllowlist: []
    },
    projectPath: '~',
    taskIdCounter: 1
  };

  var eventListeners = {};

  // Module-scoped flag/counter for double-click protection (more reliable than DOM state)
  var createSessionInProgress = false;
  var createSessionInFlight = 0;

  function emitEvent(channel) {
    var args = Array.prototype.slice.call(arguments, 1);
    if (eventListeners[channel]) {
      eventListeners[channel].forEach(function(callback) {
        callback.apply(null, args);
      });
    }
  }

  var ipcRenderer = {
    on: function(channel, callback) {
      if (tauriEvents && typeof tauriEvents.listen === 'function') {
        tauriEvents.listen(channel, function(event) {
          var payload = event && event.payload;
          if (Array.isArray(payload)) {
            callback.apply(null, [null].concat(payload));
          } else if (payload !== undefined) {
            callback(null, payload);
          } else {
            callback(null);
          }
        });
      }
      if (!eventListeners[channel]) {
        eventListeners[channel] = [];
      }
      eventListeners[channel].push(callback);
      console.log('[Tauri Bridge] Registered listener for: ' + channel);
    },

    send: function(channel) {
      var args = Array.prototype.slice.call(arguments, 1);
      console.log('[Tauri Bridge] Send: ' + channel, args);
      if (tauriInvoke) {
        if (channel === 'CreateAgentSession') {
          var payload = args[0] || {};
          var allowConcurrent = !!payload.multiCreate;
          // Double-click protection using module-scoped flag (more reliable than DOM state)
          if (createSessionInProgress && !allowConcurrent) {
            console.log('[Tauri Bridge] CreateAgentSession already in progress, ignoring');
            return;
          }
          createSessionInProgress = true;
          createSessionInFlight += 1;

          // Also update button for visual feedback
          var btn = document.getElementById('createAgentButton');
          if (btn) {
            btn.disabled = true;
            btn.textContent = 'Creating...';
          }

          tauriInvoke('create_agent_session', { payload: payload })
            .then(function(result) {
              console.log('[Tauri Bridge] CreateAgentSession result:', result);
              var agentTask = {
                ID: result.task_id,
                agent: payload.agentId || 'codex',
                model: payload.execModel || 'default',
                Status: 'Ready',
                statusState: 'idle',
                cost: 0,
                worktreePath: result.worktreePath || null,
                projectPath: payload.projectPath || null
              };

              // Emit AddTask event - handler will append to DOM
              emitEvent('AddTask', null, result.task_id, agentTask);

              // Wait for DOM to settle before navigating (double RAF pattern)
              // First RAF waits for paint, second RAF ensures DOM mutations are complete
              requestAnimationFrame(function() {
                requestAnimationFrame(function() {
                  if (typeof switchToPage === 'function') {
                    switchToPage('viewTasksPage');
                  } else {
                    // Fallback: click the nav element
                    var navEl = document.querySelector('[data-page="viewTasksPage"]');
                    if (navEl) navEl.click();
                  }
                });
              });
            })
            .catch(function(err) {
              console.error('[Tauri Bridge] create_agent_session error:', err);
              // Show notification if available
              if (typeof sendNotification === 'function') {
                sendNotification('Failed to create task: ' + (err.message || err), 'red');
              }
            })
            .finally(function() {
              // Reset module-scoped flag and re-enable button when all in-flight creates finish
              createSessionInFlight = Math.max(0, createSessionInFlight - 1);
              if (createSessionInFlight === 0) {
                createSessionInProgress = false;
                if (btn) {
                  btn.disabled = false;
                  btn.textContent = 'Create Task';
                }
              }
            });
          return;
        }
      }

      switch (channel) {
        case 'CreateTask': {
          var taskId = mockData.taskIdCounter++;
          var task = Object.assign({
            ID: taskId,
            Status: 'Ready'
          }, args[0]);
          mockData.tasks.push(task);
          setTimeout(function() {
            emitEvent('AddTask', null, taskId, task);
          }, 100);
          break;
        }
        case 'CreateAgentSession': {
          console.log('[Tauri Bridge] CreateAgentSession:', args[0]);
          var sessionId = 'session-' + Date.now() + '-' + Math.random().toString(36).substr(2, 9);
          var payload = args[0] || {};
          var agentTask = {
            ID: sessionId,
            agent: payload.agentId || 'codex',
            model: payload.execModel || 'default',
            Status: 'Initializing...',
            statusState: 'running',
            cost: 0,
            worktreePath: null
          };
          mockData.tasks.push(agentTask);
          setTimeout(function() {
            emitEvent('AddTask', null, sessionId, agentTask);
          }, 100);
          // Simulate status updates for demo
          setTimeout(function() {
            emitEvent('StatusUpdate', null, sessionId, 'Planning task...', 'yellow', 'running');
          }, 1500);
          setTimeout(function() {
            emitEvent('CostUpdate', null, sessionId, 0.0012);
          }, 2000);
          setTimeout(function() {
            emitEvent('StatusUpdate', null, sessionId, 'Executing step 1/3...', 'yellow', 'running');
          }, 3000);
          setTimeout(function() {
            emitEvent('CostUpdate', null, sessionId, 0.0045);
          }, 4000);
          setTimeout(function() {
            emitEvent('StatusUpdate', null, sessionId, 'Task completed successfully', '#04d885', 'completed');
          }, 6000);
          setTimeout(function() {
            emitEvent('CostUpdate', null, sessionId, 0.0078);
          }, 6500);
          break;
        }
        case 'StartTask':
          if (tauriInvoke) {
            tauriInvoke('start_task', { taskId: args[0] })
              .catch(function(err) {
                console.error('[Tauri Bridge] start_task error:', err);
                // Emit error status on failure
                emitEvent('StatusUpdate', null, args[0], 'Error: ' + err, 'red', 'error');
              });
            return;
          }
          // Fallback to mock for browser testing
          setTimeout(function() {
            emitEvent('StatusUpdate', null, args[0], 'Starting...', 'yellow', 'running');
          }, 100);
          break;
        case 'StartPendingSession':
          // Start a pending prompt from the chat log window
          console.log('[Tauri Bridge] StartPendingSession:', args[0]);
          if (tauriInvoke) {
            tauriInvoke('start_pending_prompt', { taskId: args[0] })
              .catch(function(err) {
                console.error('[Tauri Bridge] start_pending_prompt error:', err);
                emitEvent('StatusUpdate', null, args[0], 'Error: ' + err, 'red', 'error');
              });
            return;
          }
          // Fallback to mock for browser testing
          setTimeout(function() {
            emitEvent('StatusUpdate', null, args[0], 'Starting session...', 'yellow', 'running');
          }, 100);
          break;
        case 'StopTask':
          if (tauriInvoke) {
            tauriInvoke('stop_task', { taskId: args[0] })
              .catch(function(err) {
                console.error('[Tauri Bridge] stop_task error:', err);
                emitEvent('StatusUpdate', null, args[0], 'Error: ' + err, 'red', 'error');
              });
            return;
          }
          setTimeout(function() {
            emitEvent('StatusUpdate', null, args[0], 'Stopped', 'red', 'idle');
          }, 100);
          break;
        case 'StopGeneration':
          if (tauriInvoke) {
            tauriInvoke('stop_task', { taskId: args[0] })
              .catch(function(err) {
                console.error('[Tauri Bridge] stop_task error:', err);
                emitEvent('ChatLogStatus', null, args[0], 'Error: ' + err, 'error');
              });
            return;
          }
          setTimeout(function() {
            emitEvent('ChatLogStatus', null, args[0], 'Stopped', 'idle');
          }, 100);
          break;
        case 'DeleteTask':
          if (tauriInvoke) {
            tauriInvoke('delete_task', { taskId: args[0] })
              .catch(function(err) {
                console.error('[Tauri Bridge] delete_task error:', err);
                if (typeof sendNotification === 'function') {
                  sendNotification('Failed to delete task/worktree: ' + (err.message || err), 'red');
                }
              });
          }
          mockData.tasks = mockData.tasks.filter(function(t) { return t.ID !== args[0]; });
          break;
        case 'OpenAgentChatLog':
          console.log('[Tauri Bridge] OpenAgentChatLog:', args[0]);
          console.log('[Tauri Bridge] tauriInvoke available:', !!tauriInvoke);
          if (tauriInvoke) {
            console.log('[Tauri Bridge] Calling tauriInvoke open_chat_window...');
            tauriInvoke('open_chat_window', { taskId: args[0] })
              .then(function(result) {
                console.log('[Tauri Bridge] open_chat_window success:', result);
              })
              .catch(function(err) {
                console.error('[Tauri Bridge] open_chat_window error:', err);
              });
          } else {
            // Browser fallback - open in new window
            console.log('[Tauri Bridge] Using browser fallback window.open');
            window.open(
              'agent_chat_log.html?taskId=' + encodeURIComponent(args[0]),
              'chat-' + args[0],
              'width=650,height=750'
            );
          }
          break;
        case 'SendChatMessage':
          console.log('[Tauri Bridge] SendChatMessage:', args[0], args[1]);
          if (tauriInvoke) {
            tauriInvoke('send_chat_message', { taskId: args[0], message: args[1] })
              .catch(function(err) {
                console.error('[Tauri Bridge] send_chat_message error:', err);
              });
          }
          break;
        case 'RespondToPermission':
          console.log('[Tauri Bridge] RespondToPermission:', args[0], args[1], args[2]);
          if (tauriInvoke) {
            tauriInvoke('respond_to_permission', {
              taskId: args[0],
              requestId: args[1],
              responseId: args[2]
            })
              .then(function() {
                console.log('[Tauri Bridge] respond_to_permission success');
              })
              .catch(function(err) {
                console.error('[Tauri Bridge] respond_to_permission error:', err);
              });
          }
          break;
        case 'RespondToUserInput':
          console.log('[Tauri Bridge] RespondToUserInput:', args[0], args[1], args[2]);
          if (tauriInvoke) {
            tauriInvoke('respond_to_user_input', {
              taskId: args[0],
              requestId: args[1],
              answers: args[2]
            })
              .then(function() {
                console.log('[Tauri Bridge] respond_to_user_input success');
              })
              .catch(function(err) {
                console.error('[Tauri Bridge] respond_to_user_input error:', err);
              });
          }
          break;
        case 'OpenTaskDirectory':
          console.log('[Tauri Bridge] OpenTaskDirectory path:', args[0], 'target:', args[1]);
          if (tauriInvoke) {
            tauriInvoke('open_task_directory', {
              path: args[0],
              target: args[1]
            })
              .catch(function(err) {
                console.error('[Tauri Bridge] open_task_directory error:', err);
                // Show error to user via alert or notification
                if (typeof window !== 'undefined' && window.alert) {
                  window.alert('Failed to open: ' + err);
                }
              });
          }
          break;
        case 'GetTaskInfo':
          console.log('[Tauri Bridge] GetTaskInfo:', args[0]);
          if (tauriInvoke) {
            tauriInvoke('get_task_history', { taskId: args[0] })
              .then(function(result) {
                console.log('[Tauri Bridge] get_task_history result:', result);
                // Emit TaskInfo to the caller (now includes pending_prompt, status_state, title_summary, and paths)
                if (result && eventListeners['TaskInfo']) {
                  eventListeners['TaskInfo'].forEach(function(cb) {
                    cb(null, {
                      id: result.task_id,
                      agent: result.agent_id,
                      pending_prompt: result.pending_prompt,
                      status_state: result.status_state,
                      title_summary: result.title_summary,
                      worktree_path: result.worktree_path,
                      project_path: result.project_path
                    });
                  });
                }
                // Emit ChatLogBatch with messages
                if (result && result.messages && eventListeners['ChatLogBatch']) {
                  eventListeners['ChatLogBatch'].forEach(function(cb) {
                    cb(null, result.task_id, result.messages);
                  });
                }
              })
              .catch(function(err) {
                console.error('[Tauri Bridge] get_task_history error:', err);
              });
          }
          break;
        case 'settingsSaveButton':
          Object.assign(mockData.settings, args[0]);
          console.log('[Tauri Bridge] Settings saved:', mockData.settings);
          break;
        case 'testDiscord':
          if (tauriInvoke) {
            tauriInvoke('test_discord')
              .then(function(result) {
                if (typeof sendNotification === 'function') {
                  sendNotification(result, 'green');
                }
              })
              .catch(function(err) {
                if (typeof sendNotification === 'function') {
                  sendNotification('Discord test failed: ' + (err.message || err), 'red');
                }
              });
          } else {
            if (typeof sendNotification === 'function') {
              sendNotification('Discord test not available in browser mode', 'red');
            }
          }
          break;
      }
    },

    invoke: function(channel) {
      var args = Array.prototype.slice.call(arguments, 1);
      console.log('[Tauri Bridge] Invoke: ' + channel, args);
      console.log('[Tauri Bridge] tauriInvoke available:', !!tauriInvoke);
      if (tauriInvoke) {
        if (channel === 'getAgentModels') {
          console.log('[Tauri Bridge] Calling tauriInvoke for get_agent_models with agentId:', args[0]);
          return tauriInvoke('get_agent_models', { agentId: args[0] }).then(function(result) {
            console.log('[Tauri Bridge] get_agent_models result:', result);
            return result;
          }).catch(function(err) {
            console.error('[Tauri Bridge] get_agent_models error:', err);
            throw err;
          });
        }
        if (channel === 'pickProjectPath') {
          return tauriInvoke('pick_project_path');
        }
        if (channel === 'getRepoBranches') {
          return tauriInvoke('get_repo_branches', { projectPath: args[0] || null });
        }
        if (channel === 'getPrReadyState') {
          return tauriInvoke('get_pr_ready_state', { projectPath: args[0] || null });
        }
        if (channel === 'getSettings') {
          return tauriInvoke('get_settings');
        }
        if (channel === 'saveSettings') {
          return tauriInvoke('save_settings', { settings: args[0] });
        }
        if (channel === 'getAgentSkills') {
          return tauriInvoke('get_agent_skills', {
            agentId: args[0],
            projectPath: args[1] || null
          });
        }
        if (channel === 'toggleSkill') {
          return tauriInvoke('toggle_skill', {
            agentId: args[0],
            skillName: args[1],
            enabled: args[2]
          });
        }
        if (channel === 'getAgentAvailability') {
          return tauriInvoke('get_agent_availability');
        }
        if (channel === 'refreshAgentAvailability') {
          return tauriInvoke('refresh_agent_availability');
        }
        if (channel === 'codexLogin') {
          console.log('[Tauri Bridge] Invoking codex_login...');
          return tauriInvoke('codex_login').then(function(result) {
            console.log('[Tauri Bridge] codex_login result:', result);
            return result;
          }).catch(function(err) {
            console.error('[Tauri Bridge] codex_login error:', err);
            throw err;
          });
        }
        if (channel === 'codexLogout') {
          return tauriInvoke('codex_logout');
        }
        if (channel === 'checkCodexAuth') {
          return tauriInvoke('check_codex_auth');
        }
        if (channel === 'claudeLogin') {
          return tauriInvoke('claude_login');
        }
        if (channel === 'claudeLogout') {
          return tauriInvoke('claude_logout');
        }
        if (channel === 'checkClaudeAuth') {
          return tauriInvoke('check_claude_auth');
        }
        if (channel === 'codexRateLimits') {
          return tauriInvoke('codex_rate_limits');
        }
        if (channel === 'claudeRateLimits') {
          return tauriInvoke('claude_rate_limits');
        }
        if (channel === 'loadTasks') {
          return tauriInvoke('load_tasks');
        }
        if (channel === 'getCachedModels') {
          return tauriInvoke('get_cached_models', { agentId: args[0] });
        }
        if (channel === 'getAllCachedModels') {
          return tauriInvoke('get_all_cached_models');
        }
        if (channel === 'refreshAgentModels') {
          return tauriInvoke('refresh_agent_models', { agentId: args[0] });
        }
        if (channel === 'getEnrichedModels') {
          console.log('[Tauri Bridge] Calling get_enriched_models for agentId:', args[0]);
          return tauriInvoke('get_enriched_models', { agentId: args[0] }).then(function(result) {
            console.log('[Tauri Bridge] get_enriched_models result:', result);
            return result;
          }).catch(function(err) {
            console.error('[Tauri Bridge] get_enriched_models error:', err);
            throw err;
          });
        }
          // Mode commands
        if (channel === 'getAgentModes') {
          return tauriInvoke('get_agent_modes', { agentId: args[0] });
        }
        if (channel === 'getCachedModes') {
          return tauriInvoke('get_cached_modes', { agentId: args[0] });
        }
        if (channel === 'getAllCachedModes') {
          return tauriInvoke('get_all_cached_modes_cmd');
        }
        if (channel === 'refreshAgentModes') {
          return tauriInvoke('refresh_agent_modes', { agentId: args[0] });
        }
        if (channel === 'localUsageSnapshot') {
          return tauriInvoke('local_usage_snapshot', { days: args[0] || 30 });
        }
        if (channel === 'claudeLocalUsageSnapshot') {
          return tauriInvoke('claude_local_usage_snapshot', { days: args[0] || 30 });
        }
        if (channel === 'getCachedAnalytics') {
          return tauriInvoke('get_cached_analytics', { agentType: args[0] });
        }
        if (channel === 'getAllCachedAnalytics') {
          return tauriInvoke('get_all_cached_analytics');
        }
        if (channel === 'saveAnalyticsCache') {
          return tauriInvoke('save_analytics_cache', { agentType: args[0], snapshot: args[1] });
        }
        if (channel === 'save_attachment') {
          return tauriInvoke('save_attachment', { payload: args[0] });
        }
      }

      return new Promise(function(resolve) {
        switch (channel) {
          case 'getAccountLists':
            resolve(mockData.accountLists);
            break;
          case 'saveAccountList':
            resolve(true);
            break;
          case 'deleteAccountList':
            mockData.accountLists = mockData.accountLists.filter(function(l) { return l.name !== args[0]; });
            resolve(true);
            break;
          case 'getSettings':
            resolve(mockData.settings);
            break;
          case 'saveSettings':
            mockData.settings = Object.assign({}, mockData.settings, args[0] || {});
            resolve(true);
            break;
          case 'getAgentSkills':
            // Mock skills for testing
            resolve([
              { name: 'mock-skill', description: 'A mock skill for testing', source: 'personal', enabled: true, path: '/mock/path', can_toggle: true },
              { name: 'disabled-skill', description: 'A disabled mock skill', source: 'personal', enabled: false, path: '/mock/disabled', can_toggle: true },
              { name: 'project-skill', description: 'A project skill (read-only)', source: 'project', enabled: true, path: '/project/skill', can_toggle: false }
            ]);
            break;
          case 'toggleSkill':
            // Mock toggle - just resolve successfully
            console.log('[Tauri Bridge Mock] Toggle skill:', args[0], args[1], args[2]);
            resolve();
            break;
          case 'getRunningTasks':
            // Mock: return empty array (no running tasks)
            console.log('[Tauri Bridge Mock] Get running tasks');
            resolve([]);
            break;
          case 'restartAllAgents':
            // Mock restart - just resolve successfully with empty array
            console.log('[Tauri Bridge Mock] Restart all agents');
            resolve([]);
            break;
          case 'loadTasks':
            resolve(mockData.tasks);
            break;
          case 'getAgentModels': {
            var agentId = args[0];
            var modelMap = {
              'codex': [
                { value: 'gpt-5.1', name: 'GPT-5.1', description: 'Latest GPT model' },
                { value: 'gpt-5.1-mini', name: 'GPT-5.1 Mini', description: 'Faster, lighter model' },
                { value: 'gpt-5.1/high', name: 'GPT-5.1 (High Effort)', description: 'Extended reasoning' },
                { value: 'gpt-4o', name: 'GPT-4o', description: 'Multimodal GPT-4' },
                { value: 'gpt-4o-mini', name: 'GPT-4o Mini', description: 'Lighter multimodal model' }
              ],
              'claude-code': [
                { value: 'claude-3-haiku-20240307', name: 'Claude 3 Haiku', description: 'Fast and affordable' },
                { value: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4', description: 'Balanced performance' },
                { value: 'claude-opus-4-20250514', name: 'Claude Opus 4', description: 'Most capable model' }
              ],
              'factory-droid': [{ value: 'default', name: 'Default', description: 'Factory default model' }]
            };
            resolve(modelMap[agentId] || []);
            break;
          }
          case 'pickProjectPath':
            resolve(mockData.projectPath || '~');
            break;
          case 'getRepoBranches':
            resolve({
              branches: [],
              defaultBranch: null,
              currentBranch: null,
              source: 'mock',
              error: 'Branches unavailable in mock mode'
            });
            break;
          case 'getCachedModels':
            // Mock: return empty array (no cached models in mock mode)
            resolve([]);
            break;
          case 'getAllCachedModels':
            // Mock: return empty object (no cached models in mock mode)
            resolve({});
            break;
          case 'refreshAgentModels': {
            // Mock: use same mock models as getAgentModels
            var mockAgentId = args[0];
            var mockModelMap = {
              'codex': [
                { value: 'gpt-5.1', name: 'GPT-5.1', description: 'Latest GPT model' },
                { value: 'gpt-5.1-mini', name: 'GPT-5.1 Mini', description: 'Faster, lighter model' }
              ],
              'claude-code': [
                { value: 'claude-3-haiku-20240307', name: 'Claude 3 Haiku', description: 'Fast and affordable' },
                { value: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4', description: 'Balanced performance' }
              ],
              'factory-droid': [{ value: 'default', name: 'Default', description: 'Factory default model' }]
            };
            resolve(mockModelMap[mockAgentId] || []);
            break;
          }
          case 'getEnrichedModels': {
            // Mock: return enriched models with reasoning efforts (only for Codex)
            var enrichedAgentId = args[0];
            if (enrichedAgentId === 'codex') {
              resolve([
                {
                  value: 'gpt-5.2-codex',
                  name: 'GPT-5.2 Codex',
                  description: 'Latest Codex model',
                  supportedReasoningEfforts: [
                    { value: 'low', description: 'Fast responses' },
                    { value: 'medium', description: 'Balanced' },
                    { value: 'high', description: 'Thorough reasoning' },
                    { value: 'extra_high', description: 'Maximum reasoning' }
                  ],
                  defaultReasoningEffort: 'medium',
                  isDefault: true
                },
                {
                  value: 'gpt-5.1',
                  name: 'GPT-5.1',
                  description: 'Previous Codex model',
                  supportedReasoningEfforts: [
                    { value: 'low', description: 'Fast responses' },
                    { value: 'medium', description: 'Balanced' },
                    { value: 'high', description: 'Thorough reasoning' }
                  ],
                  defaultReasoningEffort: 'medium',
                  isDefault: false
                }
              ]);
            } else {
              resolve([]);
            }
            break;
          }
          // Mode mocks
          case 'getAgentModes':
          case 'refreshAgentModes': {
            var modeAgentId = args[0];
            resolve([]);
            break;
          }
          case 'getCachedModes':
            resolve([]);
            break;
          case 'getAllCachedModes':
            resolve({});
            break;
          case 'checkCodexAuth':
            resolve({ authenticated: false, method: null, expires_at: null });
            break;
          case 'codexLogout':
            mockData.settings.codexAuthMethod = null;
            resolve();
            break;
          case 'checkClaudeAuth':
            resolve({ authenticated: false, method: null, expires_at: null, email: null });
            break;
          case 'claudeLogout':
            mockData.settings.claudeAuthMethod = null;
            resolve();
            break;
          case 'claudeRateLimits':
            // Mock: Claude usage not available in browser testing
            resolve({ notAvailable: true, errorMessage: 'Mock mode - Claude usage not available' });
            break;
          case 'codexRateLimits':
            // Mock: Codex usage not available in browser testing
            resolve({ notAvailable: true, errorMessage: 'Mock mode - Codex usage not available' });
            break;
          case 'localUsageSnapshot': {
            // Mock analytics data for browser testing
            var now = Date.now();
            var mockDays = [];
            var today = new Date();
            for (var i = 29; i >= 0; i--) {
              var date = new Date(today);
              date.setDate(date.getDate() - i);
              var dayKey = date.toISOString().split('T')[0];
              var baseTokens = Math.floor(Math.random() * 50000000) + 5000000;
              var inputTokens = Math.floor(baseTokens * 0.6);
              var cachedTokens = Math.floor(inputTokens * 0.35);
              var outputTokens = baseTokens - inputTokens;
              mockDays.push({
                day: dayKey,
                inputTokens: inputTokens,
                cachedInputTokens: cachedTokens,
                outputTokens: outputTokens,
                totalTokens: baseTokens,
                agentTimeMs: Math.floor(Math.random() * 3600000) + 600000,
                agentRuns: Math.floor(Math.random() * 50) + 5
              });
            }
            var last7 = mockDays.slice(-7);
            var last7Tokens = last7.reduce(function(sum, d) { return sum + d.totalTokens; }, 0);
            var last7Input = last7.reduce(function(sum, d) { return sum + d.inputTokens; }, 0);
            var last7Cached = last7.reduce(function(sum, d) { return sum + d.cachedInputTokens; }, 0);
            var totalTokens = mockDays.reduce(function(sum, d) { return sum + d.totalTokens; }, 0);
            var peakDay = mockDays.reduce(function(max, d) { return d.totalTokens > max.totalTokens ? d : max; }, mockDays[0]);
            resolve({
              updatedAt: now,
              days: mockDays,
              totals: {
                last7DaysTokens: last7Tokens,
                last30DaysTokens: totalTokens,
                averageDailyTokens: Math.floor(last7Tokens / 7),
                cacheHitRatePercent: last7Input > 0 ? Math.round((last7Cached / last7Input) * 1000) / 10 : 0,
                peakDay: peakDay.day,
                peakDayTokens: peakDay.totalTokens
              },
              topModels: [
                { model: 'gpt-5.1-codex', tokens: Math.floor(totalTokens * 0.65), sharePercent: 65.0 },
                { model: 'gpt-4o', tokens: Math.floor(totalTokens * 0.20), sharePercent: 20.0 },
                { model: 'gpt-5.1-mini', tokens: Math.floor(totalTokens * 0.10), sharePercent: 10.0 },
                { model: 'o3-mini', tokens: Math.floor(totalTokens * 0.05), sharePercent: 5.0 }
              ]
            });
            break;
          }
          case 'claudeLocalUsageSnapshot': {
            // Mock Claude Code analytics data for browser testing
            var nowClaude = Date.now();
            var mockClaudeDays = [];
            var todayClaude = new Date();
            for (var ci = 29; ci >= 0; ci--) {
              var cdate = new Date(todayClaude);
              cdate.setDate(cdate.getDate() - ci);
              var cdayKey = cdate.toISOString().split('T')[0];
              var cbaseTokens = Math.floor(Math.random() * 30000000) + 2000000;
              var cinputTokens = Math.floor(cbaseTokens * 0.65);
              var ccacheCreation = Math.floor(cinputTokens * 0.3);
              var ccacheRead = Math.floor(cinputTokens * 0.25);
              var coutputTokens = cbaseTokens - cinputTokens;
              var ccost = (cinputTokens / 1000000 * 3) + (coutputTokens / 1000000 * 15) + (ccacheCreation / 1000000 * 3.75) + (ccacheRead / 1000000 * 0.3);
              mockClaudeDays.push({
                day: cdayKey,
                inputTokens: cinputTokens,
                cacheCreationTokens: ccacheCreation,
                cacheReadTokens: ccacheRead,
                outputTokens: coutputTokens,
                totalTokens: cbaseTokens,
                totalCost: ccost
              });
            }
            var clast7 = mockClaudeDays.slice(-7);
            var clast7Tokens = clast7.reduce(function(sum, d) { return sum + d.totalTokens; }, 0);
            var clast7Input = clast7.reduce(function(sum, d) { return sum + d.inputTokens; }, 0);
            var clast7CacheRead = clast7.reduce(function(sum, d) { return sum + d.cacheReadTokens; }, 0);
            var ctotalTokens = mockClaudeDays.reduce(function(sum, d) { return sum + d.totalTokens; }, 0);
            var ctotalCost = mockClaudeDays.reduce(function(sum, d) { return sum + d.totalCost; }, 0);
            var cpeakDay = mockClaudeDays.reduce(function(max, d) { return d.totalTokens > max.totalTokens ? d : max; }, mockClaudeDays[0]);
            resolve({
              updatedAt: nowClaude,
              days: mockClaudeDays,
              totals: {
                last7DaysTokens: clast7Tokens,
                last30DaysTokens: ctotalTokens,
                averageDailyTokens: Math.floor(clast7Tokens / 7),
                cacheHitRatePercent: (clast7Input + clast7CacheRead) > 0 ? Math.round((clast7CacheRead / (clast7Input + clast7CacheRead)) * 1000) / 10 : 0,
                peakDay: cpeakDay.day,
                peakDayTokens: cpeakDay.totalTokens,
                totalCost: ctotalCost
              },
              topModels: [
                { model: 'claude-sonnet-4-20250514', tokens: Math.floor(ctotalTokens * 0.55), sharePercent: 55.0, cost: ctotalCost * 0.4 },
                { model: 'claude-opus-4-5-20251101', tokens: Math.floor(ctotalTokens * 0.25), sharePercent: 25.0, cost: ctotalCost * 0.45 },
                { model: 'claude-3-haiku-20240307', tokens: Math.floor(ctotalTokens * 0.15), sharePercent: 15.0, cost: ctotalCost * 0.1 },
                { model: 'claude-3-5-sonnet-20241022', tokens: Math.floor(ctotalTokens * 0.05), sharePercent: 5.0, cost: ctotalCost * 0.05 }
              ]
            });
            break;
          }
          default:
            console.log('[Tauri Bridge] Unhandled invoke: ' + channel);
            resolve(null);
        }
      });
    }
  };

  var bridgeGlobals = window.__PHANTOM_GLOBALS__ || {
    user_id: 'mock-user-123',
    app_version: '1.0.0',
    machine_id: 'mock-device-456',
    clientExpiry: '12/31/2025',
    as_store: {
      get: function() { return null; }
    },
    rcs_store: {
      get: function() { return null; }
    }
  };

  // Helper to get current Tauri window (handles both API styles)
  function getCurrentTauriWindow() {
    if (tauriWindow) {
      // withGlobalTauri style: window.getCurrentWindow()
      if (typeof tauriWindow.getCurrentWindow === 'function') {
        return tauriWindow.getCurrentWindow();
      }
      // Module style: window.getCurrent()
      if (typeof tauriWindow.getCurrent === 'function') {
        return tauriWindow.getCurrent();
      }
    }
    return null;
  }

  var bridgeWindow = {
    getCurrent: function() {
      return {
        close: function() {
          var win = getCurrentTauriWindow();
          if (win) {
            return win.close();
          }
          console.log('[Tauri Bridge] Window close requested - no Tauri window');
          return Promise.resolve();
        },
        minimize: function() {
          var win = getCurrentTauriWindow();
          if (win) {
            return win.minimize();
          }
          console.log('[Tauri Bridge] Window minimize requested - no Tauri window');
          return Promise.resolve();
        },
        startDragging: function() {
          var win = getCurrentTauriWindow();
          if (win) {
            return win.startDragging();
          }
          console.log('[Tauri Bridge] Window startDragging requested - no Tauri window');
          return Promise.resolve();
        }
      };
    }
  };

  var remote = {
    app: {
      getVersion: function() {
        return bridgeGlobals.app_version || '1.0.0';
      }
    },
    getGlobal: function(name) {
      return bridgeGlobals[name];
    },
    getCurrentWindow: function() {
      return bridgeWindow.getCurrent();
    },
    BrowserWindow: {
      getFocusedWindow: function() {
        return bridgeWindow.getCurrent();
      }
    }
  };

  var webFrame = {
    setVisualZoomLevelLimits: function() {},
    setLayoutZoomLevelLimits: function() {}
  };

  var shell = {
    openExternal: function(url) {
      if (tauriShell && typeof tauriShell.open === 'function') {
        tauriShell.open(url);
        return;
      }
      window.open(url, '_blank');
    }
  };

  var clipboard = {
    readText: function() {
      if (tauriClipboard && typeof tauriClipboard.readText === 'function') {
        return tauriClipboard.readText();
      }
      return Promise.resolve('');
    },
    writeText: function(text) {
      if (tauriClipboard && typeof tauriClipboard.writeText === 'function') {
        return tauriClipboard.writeText(text);
      }
      return Promise.resolve(text);
    }
  };

  var app = {
    getVersion: function() {
      if (tauriApp && typeof tauriApp.getVersion === 'function') {
        return tauriApp.getVersion();
      }
      return Promise.resolve(remote.app.getVersion());
    }
  };

  window.tauriBridge = {
    ipcRenderer: ipcRenderer,
    remote: remote,
    shell: shell,
    webFrame: webFrame,
    clipboard: clipboard,
    app: app,
    window: bridgeWindow
  };

  window.tauriEmitEvent = emitEvent;
  window.tauriMockData = mockData;

  console.log('[Phantom Harness] Tauri bridge initialized');
})();
