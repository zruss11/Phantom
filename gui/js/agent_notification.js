(function() {
  'use strict';

  const bridge = window.tauriBridge;
  const ipcRenderer = bridge ? bridge.ipcRenderer : null;
  const remote = bridge ? bridge.remote : null;

  const AGENT_LOGOS = {
    'codex': '<svg class="agent-icon" role="img" aria-label="Codex"><use href="images/chatgpt-sprites-core.svg#55180d"></use></svg>',
    'claude-code': '<img src="images/claude-color.png" alt="Claude Code">',

    // Provider-style agents (Tauri wiring)
    'amp': '<img src="images/ampcode.png" alt="Amp">',
    'droid': '<img src="images/factorydroid.png" alt="Droid">',
    'opencode': '<img src="images/opencode.png" alt="OpenCode">',

    // Back-compat
    'factory-droid': '<img src="factoryy-ai.svg" alt="Factory">'
  };

  const AGENT_NAMES = {
    'codex': 'Codex',
    'claude-code': 'Claude Code',
    'amp': 'Amp',
    'droid': 'Droid',
    'opencode': 'OpenCode',
    'factory-droid': 'Factory'
  };

  function getParam(key) {
    const params = new URLSearchParams(window.location.search);
    return params.get(key);
  }

  function decodeParam(value) {
    if (!value) return '';
    try {
      return decodeURIComponent(value.replace(/\+/g, ' '));
    } catch (err) {
      return value;
    }
  }

  function closeWindow() {
    const tauri = window.__TAURI__ || null;
    
    // Tauri v2 with withGlobalTauri: true
    // The close() method returns a Promise, so we need to handle it properly
    if (tauri) {
      // Try window.getCurrentWindow() first (Tauri v2 preferred method)
      if (tauri.window && typeof tauri.window.getCurrentWindow === 'function') {
        try {
          const win = tauri.window.getCurrentWindow();
          if (win && typeof win.close === 'function') {
            win.close().catch(function(err) {
              console.warn('[Notification] window.close() failed:', err);
            });
            return;
          }
        } catch (err) {
          console.warn('[Notification] getCurrentWindow failed:', err);
        }
      }
      
      // Try webviewWindow.getCurrentWebviewWindow() (alternate Tauri v2 API)
      if (tauri.webviewWindow && typeof tauri.webviewWindow.getCurrentWebviewWindow === 'function') {
        try {
          const win = tauri.webviewWindow.getCurrentWebviewWindow();
          if (win && typeof win.close === 'function') {
            win.close().catch(function(err) {
              console.warn('[Notification] webviewWindow.close() failed:', err);
            });
            return;
          }
        } catch (err) {
          console.warn('[Notification] getCurrentWebviewWindow failed:', err);
        }
      }
    }
    
    // Fallback via bridge
    try {
      if (bridge && bridge.window && typeof bridge.window.getCurrent === 'function') {
        const win = bridge.window.getCurrent();
        if (win && typeof win.close === 'function') {
          const result = win.close();
          if (result && typeof result.catch === 'function') {
            result.catch(function(err) {
              console.warn('[Notification] bridge close failed:', err);
            });
          }
          return;
        }
      }
    } catch (err) {
      console.warn('[Notification] bridge close failed:', err);
    }
    
    // Last resort
    window.close();
  }

  function openChatLog(taskId) {
    if (!taskId) return;
    if (ipcRenderer) {
      ipcRenderer.send('OpenAgentChatLog', taskId);
    }
  }

  function playSound() {
    try {
      const audio = new Audio('checkout.mp3');
      audio.volume = 0.75;
      const playPromise = audio.play();
      if (playPromise && typeof playPromise.catch === 'function') {
        playPromise.catch(function() {
          // Ignore autoplay errors
        });
      }
    } catch (err) {
      // Ignore audio errors
    }
  }

  function init() {
    const taskId = getParam('taskId');
    const agentId = getParam('agent') || 'codex';
    const preview = decodeParam(getParam('preview')) || 'Agent finished and is waiting on your reply.';

    const logo = AGENT_LOGOS[agentId] || AGENT_LOGOS['codex'];
    const name = AGENT_NAMES[agentId] || 'Agent';

    const logoEl = document.getElementById('agentLogo');
    const subtitleEl = document.getElementById('notificationSubtitle');
    const previewEl = document.getElementById('notificationPreview');

    if (logoEl) logoEl.innerHTML = logo;
    if (subtitleEl) subtitleEl.textContent = name + ' finished a turn';
    if (previewEl) previewEl.textContent = preview;

    const shell = document.getElementById('notificationShell');
    const viewButton = document.getElementById('viewPanel');
    const dismissButton = document.getElementById('dismiss');

    function handleOpen() {
      openChatLog(taskId);
      closeWindow();
    }

    if (shell) {
      shell.addEventListener('click', function(event) {
        if (event.target && event.target.id === 'dismiss') return;
        handleOpen();
      });
      shell.addEventListener('keydown', function(event) {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          handleOpen();
        }
      });
    }

    if (viewButton) {
      viewButton.addEventListener('click', function(event) {
        event.stopPropagation();
        handleOpen();
      });
    }

    if (dismissButton) {
      dismissButton.addEventListener('click', function(event) {
        event.stopPropagation();
        closeWindow();
      });
    }

    document.addEventListener('keydown', function(event) {
      if (event.key === 'Escape') {
        closeWindow();
      }
    });

    playSound();
  }

  init();
})();
