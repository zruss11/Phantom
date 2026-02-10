/**
 * Companion-style WebSocket bridge client for the chat log window.
 *
 * Connects to the Rust backend WS bridge (`/ws/browser/<taskId>`) and re-emits
 * protocol events as Phantom's existing `ChatLogStreaming` events so the UI can
 * reuse its current streaming/tool/permission rendering code.
 */
(function () {
  'use strict';

  function getTaskIdFromUrl() {
    try {
      const params = new URLSearchParams(window.location.search || '');
      return params.get('taskId');
    } catch (e) {
      return null;
    }
  }

  async function getBridgePort() {
    const tauri = window.__TAURI__ || null;
    const invoke = tauri && tauri.core && typeof tauri.core.invoke === 'function'
      ? tauri.core.invoke
      : null;
    if (!invoke) return null;
    try {
      const port = await invoke('get_ws_bridge_port');
      if (typeof port === 'number' && port > 0) return port;
    } catch (e) {
      console.warn('[ws-chat] get_ws_bridge_port failed:', e);
    }
    return null;
  }

  function buildWsUrl(port, taskId) {
    return 'ws://127.0.0.1:' + port + '/ws/browser/' + encodeURIComponent(taskId);
  }

  function emitStreaming(taskId, update) {
    if (typeof window.tauriEmitEvent !== 'function') return;
    window.tauriEmitEvent('ChatLogStreaming', null, taskId, update);
  }

  function toJsonString(value) {
    try {
      return JSON.stringify(value);
    } catch (e) {
      return '' + value;
    }
  }

  function handleStreamEvent(taskId, event) {
    if (!event || typeof event !== 'object') return;
    if (event.type !== 'content_block_delta') return;
    const delta = event.delta;
    if (!delta || typeof delta !== 'object') return;

    // Text streaming: { delta: { type: "text_delta", text: "..." } }
    if (delta.type === 'text_delta' && typeof delta.text === 'string') {
      emitStreaming(taskId, {
        type: 'streaming',
        message_type: 'text_chunk',
        content: delta.text,
        item_id: null,
      });
      return;
    }

    // Thinking streaming (best-effort; Anthropic formats can vary).
    if (delta.type === 'thinking_delta') {
      const thinking = typeof delta.thinking === 'string'
        ? delta.thinking
        : (typeof delta.text === 'string' ? delta.text : null);
      if (thinking) {
        emitStreaming(taskId, {
          type: 'streaming',
          message_type: 'reasoning_chunk',
          content: thinking,
        });
      }
    }
  }

  function handleAssistant(taskId, message) {
    const blocks = message && message.content && Array.isArray(message.content)
      ? message.content
      : [];

    blocks.forEach(function (block) {
      if (!block || typeof block !== 'object') return;
      if (block.type === 'tool_use') {
        const name = block.name || 'tool';
        const args = block.input ? toJsonString(block.input) : '{}';
        emitStreaming(taskId, {
          type: 'streaming',
          message_type: 'tool_call',
          name: name,
          arguments: args,
        });
      }

      if (block.type === 'tool_result') {
        const content = block.content !== undefined ? block.content : (block.output !== undefined ? block.output : '');
        const output = typeof content === 'string' ? content : toJsonString(content);
        emitStreaming(taskId, {
          type: 'streaming',
          message_type: 'tool_return',
          content: output,
        });
      }

      if (block.type === 'thinking') {
        const thinking = block.thinking;
        if (typeof thinking === 'string' && thinking.trim()) {
          emitStreaming(taskId, {
            type: 'streaming',
            message_type: 'reasoning_chunk',
            content: thinking,
          });
        }
      }
    });
  }

  function handlePermissionRequest(taskId, request) {
    if (!request) return;

    // ExitPlanMode is a special permission request — render it as a plan card
    // instead of a generic tool approval prompt.
    if (request.tool_name === 'ExitPlanMode') {
      var input = request.input || {};
      if (typeof input === 'string') {
        try { input = JSON.parse(input); } catch (e) { input = {}; }
      }
      var plan = typeof input.plan === 'string' ? input.plan : '';
      var allowedPrompts = Array.isArray(input.allowedPrompts) ? input.allowedPrompts : [];

      // Emit plan content so the chat log renders the markdown plan UI
      emitStreaming(taskId, {
        type: 'streaming',
        message_type: 'plan_content',
        content: plan,
        request_id: request.request_id || '',
        allowed_prompts: allowedPrompts,
      });
      return;
    }

    emitStreaming(taskId, {
      type: 'streaming',
      message_type: 'permission_request',
      request_id: request.request_id || '',
      tool_name: request.tool_name || 'tool',
      description: request.description || null,
      raw_input: request.input ? toJsonString(request.input) : null,
      // Options are generated on the Rust side for now (allow/deny).
      options: Array.isArray(request.options) ? request.options : (Array.isArray(request.permission_suggestions) ? request.permission_suggestions : []),
    });
  }

  async function connect() {
    const taskId = getTaskIdFromUrl();
    if (!taskId) return;
    const port = await getBridgePort();
    if (!port) return;

    let ws = null;
    let reconnectTimer = null;

    function scheduleReconnect() {
      if (reconnectTimer) return;
      reconnectTimer = setTimeout(function () {
        reconnectTimer = null;
        try {
          connect();
        } catch (e) {
          // ignore
        }
      }, 2000);
    }

    function cleanup() {
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      if (ws) {
        try { ws.close(); } catch (e) {}
        ws = null;
      }
    }

    ws = new WebSocket(buildWsUrl(port, taskId));

    ws.onopen = function () {
      // no-op
    };

    ws.onmessage = function (evt) {
      let data = null;
      try {
        data = JSON.parse(evt.data);
      } catch (e) {
        return;
      }
      if (!data || typeof data !== 'object') return;

      switch (data.type) {
        case 'stream_event':
          handleStreamEvent(taskId, data.event);
          break;
        case 'assistant':
          handleAssistant(taskId, data.message);
          break;
        case 'permission_request':
          handlePermissionRequest(taskId, data.request);
          break;
        default:
          break;
      }
    };

    ws.onclose = function () {
      cleanup();
      scheduleReconnect();
    };

    ws.onerror = function () {
      try { ws.close(); } catch (e) {}
    };
  }

  // Only useful inside the chat log window.
  if (document && document.body) {
    connect();
  }
})();

