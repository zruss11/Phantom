/**
 * Agent Chat Log Viewer
 * Displays real-time chat log from agent sessions with message input
 */
(function () {
  "use strict";

  const bridge = window.tauriBridge;
  const ipcRenderer = bridge ? bridge.ipcRenderer : null;

  // Configure marked for safe rendering
  function initMarkdown() {
    if (typeof marked !== 'undefined') {
      marked.setOptions({
        highlight: function(code, lang) {
          if (typeof hljs !== 'undefined' && lang && hljs.getLanguage(lang)) {
            return hljs.highlight(code, { language: lang }).value;
          }
          return code;
        },
        breaks: true,
        gfm: true
      });
    }
  }

  // Render markdown content safely
  function renderMarkdown(content) {
    if (!content) return '';
    if (typeof marked === 'undefined') {
      return escapeHtml(content); // Fallback
    }
    try {
      const html = marked.parse(content);
      return rewriteMarkdownLinks(html);
    } catch (e) {
      console.error('[ChatLog] Markdown error:', e);
      return escapeHtml(content);
    }
  }

  function rewriteMarkdownLinks(html) {
    if (!html || typeof DOMParser === 'undefined') {
      return html;
    }
    try {
      const parser = new DOMParser();
      const doc = parser.parseFromString(html, 'text/html');
      const anchors = doc.querySelectorAll('a[href]');
      anchors.forEach((a) => {
        a.setAttribute('target', '_blank');
        a.setAttribute('rel', 'noopener noreferrer');
        a.setAttribute('data-external', 'true');
      });
      return doc.body.innerHTML;
    } catch (e) {
      console.error('[ChatLog] Link rewrite error:', e);
      return html;
    }
  }

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

  const AGENT_NAMES = {
    codex: "Codex",
    "claude-code": "Claude Code",
    amp: "Amp",
    droid: "Droid",
    opencode: "OpenCode",
  };

  const AGENT_REVIEW_ICONS = {
    codex: 'images/codex.png',
    'claude-code': 'images/claude-color.png',
    amp: 'images/ampcode.png',
    droid: 'images/factorydroid.png',
    opencode: 'images/opencode.png',
    'factory-droid': 'images/factorydroid.png',
  };

  // Image lightbox functionality
  function showImageLightbox(src, alt) {
    let lightbox = document.getElementById("imageLightbox");
    if (!lightbox) {
      // Create lightbox if it doesn't exist
      lightbox = document.createElement("div");
      lightbox.id = "imageLightbox";
      lightbox.className = "image-lightbox";
      lightbox.innerHTML = `
        <button class="image-lightbox-close" onclick="hideImageLightbox()">
          <i class="fal fa-times"></i>
        </button>
        <img src="" alt="">
      `;
      lightbox.onclick = (e) => {
        if (e.target === lightbox) hideImageLightbox();
      };
      document.body.appendChild(lightbox);
    }

    const img = lightbox.querySelector("img");
    img.src = src;
    img.alt = alt || "Image";
    lightbox.classList.add("active");

    // Close on escape key
    document.addEventListener("keydown", handleLightboxEscape);
  }

  function hideImageLightbox() {
    const lightbox = document.getElementById("imageLightbox");
    if (lightbox) {
      lightbox.classList.remove("active");
    }
    document.removeEventListener("keydown", handleLightboxEscape);
  }

  function handleLightboxEscape(e) {
    if (e.key === "Escape") {
      hideImageLightbox();
    }
  }

  // Expose lightbox functions globally for onclick handlers
  window.showImageLightbox = showImageLightbox;
  window.hideImageLightbox = hideImageLightbox;

  // Ghost SVG icon for unknown tools
  const GHOST_SVG =
    '<svg class="ghost-icon" width="14" height="14" viewBox="0 0 36 36" fill="currentColor" xmlns="http://www.w3.org/2000/svg"><path fill-rule="evenodd" clip-rule="evenodd" d="M34.8888 33.9334C32.6622 32.7094 30.7428 31.0101 29.2718 28.9604C28.2094 27.4306 27.7961 25.5533 28.1199 23.7283C28.4437 21.9033 29.479 20.2747 31.0053 19.1892L32.6594 18.0175C33.3441 17.5877 33.8366 16.9174 34.0366 16.143C34.2366 15.3687 34.1291 14.5485 33.7359 13.8493C33.5397 13.4953 33.2724 13.1841 32.9503 12.9346C32.6282 12.6851 32.258 12.5025 31.862 12.3978C31.4774 12.2954 31.0756 12.271 30.6811 12.3261C30.2866 12.3812 29.9076 12.5147 29.5672 12.7185L28.3724 13.4246C28.3172 13.458 28.2537 13.4757 28.1889 13.4759C28.124 13.4761 28.0604 13.4588 28.005 13.4257C27.9529 13.3987 27.9093 13.3581 27.8789 13.3085C27.8485 13.2588 27.8324 13.2019 27.8325 13.144L27.834 13.1036C27.9001 11.3349 27.6064 9.57104 26.9702 7.91548C26.334 6.25992 25.3682 4.74598 24.1293 3.46243C23.0978 2.38214 21.8546 1.51843 20.4747 0.923368C19.0947 0.328306 17.6066 0.014207 16.0999 0C9.86269 0 5.15898 5.62316 5.15898 13.0795C5.15898 13.234 5.16053 13.4151 5.16325 13.6126C5.16495 13.6746 5.14965 13.7359 5.11895 13.7901C5.08826 13.8443 5.04331 13.8894 4.98881 13.9207C4.93588 13.9531 4.87496 13.9708 4.8126 13.9717C4.75024 13.9726 4.6888 13.9568 4.63489 13.926L4.5678 13.8898C4.22733 13.6862 3.84834 13.5528 3.45384 13.4976C3.05935 13.4425 2.65758 13.4668 2.27293 13.5691C1.87693 13.6738 1.50666 13.8564 1.18456 14.106C0.862453 14.3556 0.595215 14.6669 0.399044 15.021C0.00286865 15.7253 -0.103195 16.5523 0.102764 17.3312C0.308727 18.1102 0.810974 18.7816 1.50578 19.2068L4.69883 21.0941C5.3548 21.475 5.86448 22.0583 6.14862 22.7535C8.52337 28.7598 14.3466 35.9199 28.6035 35.9199C30.6011 35.9199 32.0448 35.9537 33.1017 35.9785L33.1262 35.979L33.1492 35.9796L33.1502 35.9796C33.6243 35.9908 34.0155 36 34.3399 36C35.4924 36 35.8017 35.8852 35.9606 35.3411C36.156 34.6747 35.5968 34.3476 34.889 33.9336L34.8888 33.9334ZM15.963 12.3796C15.0378 11.9925 14.4827 10.9906 14.6214 9.92043C14.6908 9.30564 14.9221 8.59976 15.4773 8.1899C16.6801 7.2791 22.2547 6.00398 23.1568 8.75915C24.0589 11.4916 18.9701 13.6092 15.963 12.3796ZM8.29434 8.85085C7.99363 9.35179 7.9705 9.98935 8.13242 10.5586C8.47938 11.7654 9.24271 12.3802 10.538 12.4485C12.0184 12.4713 12.9437 12.2664 12.8743 9.60226C12.828 7.98559 11.417 7.75789 10.6537 7.75789C9.28897 7.73512 8.61817 8.30437 8.29434 8.85085Z"/></svg>';

  // Tool icon mapping for visual differentiation
  const TOOL_ICONS = {
    Read: "fa-file-alt",
    Write: "fa-file-edit",
    Edit: "fa-edit",
    MultiEdit: "fa-layer-group",
    Bash: "fa-terminal",
    Grep: "fa-search",
    Glob: null,
    WebFetch: "fa-globe",
    WebSearch: "fa-search-plus",
    Task: "fa-tasks",
    LS: "fa-list",
    List: "fa-list",
    NotebookEdit: "fa-book",
    AskUserQuestion: "fa-question-circle",
    default: null, // null means use GHOST_SVG
  };

  let currentTaskId = null;
  let currentTaskPath = null; // Resolved path for "Open in..." functionality
  let codeReviewLoading = false;
  let autoScroll = true;
  let elapsedTimer = null;
  let startTime = null;

  // Pending prompt state
  let pendingPrompt = null;
  let taskStatusState = "idle";

  // Diff line counter (tracks additions and deletions)
  let diffAdditions = 0;
  let diffDeletions = 0;

  function updateDiffCounter() {
    const counter = document.getElementById("diffCounter");
    const countEl = document.getElementById("diffCount");
    if (counter && countEl) {
      const hasChanges = diffAdditions > 0 || diffDeletions > 0;
      countEl.innerHTML = `<span class="diff-counter-add">+${diffAdditions}</span> / <span class="diff-counter-del">-${diffDeletions}</span>`;
      counter.style.display = hasChanges ? "flex" : "none";
    }
  }

  function addDiffLines(additions, deletions) {
    diffAdditions += additions;
    diffDeletions += deletions || 0;
    updateDiffCounter();
  }

  function resetDiffCount() {
    diffAdditions = 0;
    diffDeletions = 0;
    updateDiffCounter();
  }

  // Parse task ID from URL query params
  function getTaskIdFromUrl() {
    const params = new URLSearchParams(window.location.search);
    return params.get("taskId");
  }

  // Initialize the viewer
  function init() {
    // Initialize markdown renderer
    initMarkdown();

    currentTaskId = getTaskIdFromUrl();

    if (currentTaskId) {
      $("#taskId .task-id-text").text("Task ID: " + currentTaskId);
    }

    // Request task info from main process
    if (ipcRenderer && currentTaskId) {
      ipcRenderer.send("GetTaskInfo", currentTaskId);
    }

    setupEventListeners();
    populateCodeReviewMenu();
    setupIPCListeners();
    initTodoSidebar();

    // Initialize slash command autocomplete for the chat input
    var chatInput = document.getElementById("chatInput");
    if (chatInput && window.SlashCommandAutocomplete) {
      window.chatSlashCommands = new SlashCommandAutocomplete(
        chatInput,
        "codex",
      );
      console.log("[ChatLog] Slash command autocomplete initialized");
    }
  }

  // Set up UI event listeners
  function setupEventListeners() {
    $("#closeWindow").on("click", function () {
      if (bridge && bridge.remote) {
        bridge.remote.getCurrentWindow().close();
      }
    });

    $("#minimizeWindow").on("click", function () {
      if (bridge && bridge.remote) {
        bridge.remote.getCurrentWindow().minimize();
      }
    });

    $("#clearLog").on("click", function () {
      clearMessages();
    });

    $("#taskIdCopy").on("click", function (e) {
      e.preventDefault();
      e.stopPropagation();
      if (!currentTaskId) return;
      copyToClipboard(currentTaskId, this);
    });

    const logSwitcherWrap = $("#logSwitcherWrap");
    const logSwitcher = $("#logSwitcher");
    const logSwitcherLabel = $("#logSwitcherLabel");
    const logSwitcherIcon = $("#logSwitcherIcon");
    var codeReviewWrap = $('#codeReviewWrap');
    var codeReviewBtn = $('#codeReviewBtn');

    logSwitcher.on("click", function (e) {
      e.stopPropagation();
      logSwitcherWrap.toggleClass("open");
      logSwitcher.attr("aria-expanded", logSwitcherWrap.hasClass("open"));
    });

    logSwitcher.on("keydown", function (e) {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        logSwitcher.trigger("click");
      }
    });

    $("#logSwitcherMenu").on("click", ".log-switcher-item", function (e) {
      e.stopPropagation();
      const item = $(this);
      const target = item.data("target");
      const icon = item.data("icon");
      const label = item.text().trim();
      $(".log-switcher-item").removeClass("is-active");
      item.addClass("is-active");
      if (logSwitcherLabel.length) {
        logSwitcherLabel.text(label);
      }
      if (logSwitcherIcon.length && icon) {
        logSwitcherIcon.attr("src", icon);
      }
      if (logSwitcher.length) {
        logSwitcher.attr("aria-label", "Open in " + label);
        logSwitcher.attr("title", "Open in " + label);
      }
      if (ipcRenderer && currentTaskPath && target) {
        ipcRenderer.send("OpenTaskDirectory", currentTaskPath, target);
      } else if (!currentTaskPath) {
        console.warn("[ChatLog] No path available for task - cannot open in", target);
        window.alert("No project path available for this task");
      }
      logSwitcherWrap.removeClass("open");
      logSwitcher.attr("aria-expanded", false);
    });

    // --- Code Review Dropdown ---
    codeReviewBtn.on('click', function(e) {
      e.stopPropagation();
      // Close the other dropdown if open
      logSwitcherWrap.removeClass('open');
      logSwitcher.attr('aria-expanded', false);
      codeReviewWrap.toggleClass('open');
      codeReviewBtn.attr('aria-expanded', codeReviewWrap.hasClass('open'));
    });

    codeReviewBtn.on('keydown', function(e) {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        codeReviewBtn.trigger('click');
      }
    });

    $('#codeReviewMenu').on('click', '.log-switcher-item', function(e) {
      e.stopPropagation();
      if (codeReviewLoading) return;
      var reviewAgentId = $(this).data('agent');
      if (!reviewAgentId) return;
      codeReviewWrap.removeClass('open');
      codeReviewBtn.attr('aria-expanded', false);
      initiateCodeReview(reviewAgentId);
    });

    $(document).on("click", function () {
      if (logSwitcherWrap.hasClass("open")) {
        logSwitcherWrap.removeClass("open");
        logSwitcher.attr("aria-expanded", false);
      }
      if (codeReviewWrap.hasClass("open")) {
        codeReviewWrap.removeClass("open");
        codeReviewBtn.attr("aria-expanded", false);
      }
    });

    $(document).on("keydown", function (e) {
      if (e.key === "Escape") {
        if (logSwitcherWrap.hasClass("open")) {
          logSwitcherWrap.removeClass("open");
          logSwitcher.attr("aria-expanded", false);
        }
        if (codeReviewWrap.hasClass("open")) {
          codeReviewWrap.removeClass("open");
          codeReviewBtn.attr("aria-expanded", false);
        }
      }
    });

    // Window dragging for custom titlebar
    $(".chat-header").on("mousedown", function (e) {
      if ($(e.target).closest("button, a, input").length) {
        return;
      }
      if (bridge && bridge.remote) {
        bridge.remote.getCurrentWindow().startDragging();
      }
    });

    // Track scroll position for auto-scroll
    $("#chatContainer").on("scroll", function () {
      const container = this;
      const atBottom =
        container.scrollHeight - container.scrollTop <=
        container.clientHeight + 50;
      autoScroll = atBottom;
    });

    // Send button click
    $("#sendButton").on("click", function () {
      sendMessage();
    });

    // Stop button click
    $("#stopButton").on("click", function () {
      stopGeneration();
    });

    // Start session button click (for pending prompts)
    $("#startSessionBtn").on("click", function () {
      const btn = $(this);
      btn
        .prop("disabled", true)
        .html('<i class="fal fa-spinner fa-spin"></i> Starting...');

      // Convert pending message to sent visually
      const msg = document.getElementById("pendingPromptMessage");
      if (msg) {
        msg.classList.remove("pending");
        const draftLabel = msg.querySelector(".draft-label");
        if (draftLabel) draftLabel.remove();
      }

      // Hide pending actions and enable chat input
      $("#pendingActions").hide();
      $("#chatInput")
        .prop("disabled", false)
        .attr("placeholder", "Send a message to the agent...");
      $("#sendButton").prop("disabled", false);

      // Send start request to backend
      if (ipcRenderer && currentTaskId) {
        ipcRenderer.send("StartPendingSession", currentTaskId);
      }

      // Update local state
      pendingPrompt = null;
      taskStatusState = "running";
    });

    // Enter key to send (Shift+Enter for newline)
    $("#chatInput").on("keydown", function (e) {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        sendMessage();
      }
    });

    // Paste handler for images in textarea
    $("#chatInput").on("paste", function (e) {
      const items =
        e.originalEvent && e.originalEvent.clipboardData
          ? e.originalEvent.clipboardData.items
          : null;
      if (!items) return;
      for (const item of items) {
        if (item.type && item.type.startsWith("image/")) {
          const file = item.getAsFile();
          if (file) {
            processChatImageFile(file);
          }
        }
      }
    });

    // Auto-resize textarea
    $("#chatInput").on("input", function () {
      this.style.height = "auto";
      this.style.height = Math.min(this.scrollHeight, 120) + "px";
    });

    // Reasoning message collapse toggle (delegated)
    $("#chatContainer").on("click", ".reasoning-header", function () {
      const header = $(this);
      const content = header.siblings(".reasoning-content");
      header.toggleClass("collapsed");
      content.toggleClass("hidden");
      // Rotate chevron: down when collapsed, up when expanded
      const chevron = header.find(".reasoning-toggle");
      if (header.hasClass("collapsed")) {
        chevron.removeClass("fa-chevron-up").addClass("fa-chevron-down");
      } else {
        chevron.removeClass("fa-chevron-down").addClass("fa-chevron-up");
      }
    });

    // Tool card collapse toggle (delegated)
    $("#chatContainer").on("click", ".tool-card-header", function () {
      const header = $(this);
      const body = header.siblings(".tool-card-body");
      header.toggleClass("collapsed");
      body.toggleClass("hidden");
      // Rotate chevron: down when collapsed, up when expanded
      const chevron = header.find(".tool-card-toggle");
      if (header.hasClass("collapsed")) {
        chevron.removeClass("fa-chevron-up").addClass("fa-chevron-down");
      } else {
        chevron.removeClass("fa-chevron-down").addClass("fa-chevron-up");
      }
    });

    // Tool section collapse toggle (delegated)
    $("#chatContainer").on("click", ".tool-section-header", function () {
      const header = $(this);
      const content = header.siblings(".tool-section-content");
      header.toggleClass("collapsed");
      content.toggleClass("hidden");
    });

    // Permission request: raw input toggle (delegated)
    $("#chatContainer").on("click", ".permission-raw-toggle", function () {
      const toggle = $(this);
      const content = toggle.next(".permission-raw-content");
      toggle.toggleClass("expanded");
      content.toggleClass("visible");
    });

    // Permission request: action button click (delegated)
    $("#chatContainer").on("click", ".permission-btn", function () {
      const btn = $(this);
      if (btn.hasClass("permission-menu-toggle")) {
        return;
      }
      const card = btn.closest(".permission-request");
      const requestId = card.attr("data-request-id");
      // User input submission uses a different handler
      if (btn.attr("data-action") === "submit_user_input") {
        if (!requestId) {
          console.error("[ChatLog] Missing request ID for user input");
          return;
        }
        submitUserInputResponse(requestId, card);
        return;
      }

      const responseId =
        btn.attr("data-response-id") ||
        btn.closest(".permission-action-group").attr("data-response-id");

      if (!requestId || !responseId) {
        console.error(
          "[ChatLog] Missing request or response ID for permission",
        );
        return;
      }

      sendPermissionResponse(requestId, responseId, card);
    });

    // AskUserQuestion: submit answer (delegated)
    $("#chatContainer").on("click", ".ask-question-submit", function () {
      const btn = $(this);
      const card = btn.closest(".ask-user-question");
      if (!card.length) return;

      const questionBlocks = card.find(".user-input-question");
      if (questionBlocks.length > 1) {
        const answers = [];
        let missing = false;
        questionBlocks.each(function (idx, el) {
          const block = $(el);
          const header = block
            .find(".permission-action-label")
            .clone()
            .children()
            .remove()
            .end()
            .text()
            .trim();
          const label =
            header ||
            block.find(".permission-reason").first().text().trim() ||
            `Question ${idx + 1}`;
          const choices = block.find(".user-input-choices").first();

          let answerText = "";
          if (choices.length) {
            const selected = choices.find(".user-input-option.is-selected");
            if (selected.length) {
              if (choices.attr("data-multiselect") === "true") {
                const labels = selected
                  .map((_, item) => $(item).attr("data-value") || "")
                  .get()
                  .filter(Boolean);
                answerText = labels.join(", ");
              } else {
                answerText = selected.first().attr("data-value") || "";
              }
            }
          }

          if (!answerText) {
            const input = block.find(".user-input-freeform").first();
            if (input.length) {
              answerText = (input.val() || "").toString().trim();
            }
          }

          if (!answerText) {
            missing = true;
            return false;
          }

          answers.push(`${label}: ${answerText}`);
          return true;
        });

        if (missing || answers.length === 0) return;

        card.find(".permission-btn").prop("disabled", true);
        card.addClass("responded");
        const statusEl = card.find(".permission-status");
        statusEl.removeClass("denied").addClass("confirmed");
        statusEl.find("span").text("Answered");

        sendMessageWithText(answers.join("\n"));
        return;
      }

      let answer = "";
      const selected = card.find(".user-input-option.is-selected").first();
      if (selected.length) {
        answer = selected.attr("data-value") || "";
      }
      if (!answer) {
        const input = card.find(".user-input-freeform").first();
        if (input.length) {
          answer = (input.val() || "").toString().trim();
        }
      }
      if (!answer) return;

      card.find(".permission-btn").prop("disabled", true);
      card.addClass("responded");
      const statusEl = card.find(".permission-status");
      statusEl.removeClass("denied").addClass("confirmed");
      statusEl.find("span").text("Answered");

      sendMessageWithText(answer);
    });

    $("#chatContainer").on("click", ".permission-menu-toggle", function (e) {
      e.stopPropagation();
      const group = $(this).closest(".permission-action-group");
      $(".permission-action-group").not(group).removeClass("open");
      group.toggleClass("open");
    });

    $("#chatContainer").on("click", ".permission-menu-item", function (e) {
      e.stopPropagation();
      const item = $(this);
      const group = item.closest(".permission-action-group");
      const responseId = item.attr("data-response-id");
      const label = item.attr("data-label") || item.text().trim();
      const trigger = group.find(".permission-action-trigger");

      if (responseId) {
        group.attr("data-response-id", responseId);
        trigger.attr("data-response-id", responseId);
      }

      if (label) {
        trigger.find(".btn-label").text(label);
      }

      group.removeClass("open");
    });

    $(document).on("click", function (e) {
      if (!$(e.target).closest(".permission-action-group").length) {
        $(".permission-action-group").removeClass("open");
      }
    });

    // User input: option selection (delegated)
    $("#chatContainer").on("click", ".user-input-option", function () {
      const btn = $(this);
      const qid = btn.attr("data-qid");
      if (!qid) return;
      const card = btn.closest(".user-input-request");
      const question = card.find(`.user-input-choices[data-qid="${qid}"]`);
      card.find(".user-input-choices").removeClass("is-active");
      card.find(".user-input-question").removeClass("is-active-question");
      question.addClass("is-active");
      card.find(`.user-input-question[data-qid="${qid}"]`).addClass("is-active-question");
      if (question.attr("data-multiselect") === "true") {
        btn.toggleClass("is-selected");
      } else {
        question.find(".user-input-option").removeClass("is-selected");
        btn.addClass("is-selected");
      }
      card.attr("data-active-qid", qid);
    });

    // Permission request: keyboard shortcuts
    $(document).on("keydown", function (e) {
      // Only handle if a permission request is pending
      const pendingCard = $(".permission-request").not(".responded").not(".user-input-request").first();
      if (!pendingCard.length) return;

      const requestId = pendingCard.attr("data-request-id");
      if (!requestId) return;

      // ⌥⌘Y - Auto-accept (primary action)
      if (e.metaKey && e.altKey && e.key.toLowerCase() === "y") {
        e.preventDefault();
        const btn = pendingCard.find(
          '.permission-btn.primary[data-response-id="auto_accept"]',
        );
        if (btn.length) {
          sendPermissionResponse(requestId, "auto_accept", pendingCard);
        }
        return;
      }

      // ⌘Y - Manual approve (secondary action)
      if (e.metaKey && !e.altKey && e.key.toLowerCase() === "y") {
        e.preventDefault();
        const btn = pendingCard.find(
          '.permission-btn.secondary[data-response-id="manual"]',
        );
        if (btn.length) {
          sendPermissionResponse(requestId, "manual", pendingCard);
        }
        return;
      }

      // ⌥⌘Z - Deny (danger action)
      if (e.metaKey && e.altKey && e.key.toLowerCase() === "z") {
        e.preventDefault();
        const btn = pendingCard.find(
          '.permission-btn.danger[data-response-id="deny"]',
        );
        if (btn.length) {
          sendPermissionResponse(requestId, "deny", pendingCard);
        }
        return;
      }
    });

    // User input: keyboard navigation
    $(document).on("keydown", function (e) {
      const pendingCard = $(".user-input-request").not(".responded").first();
      if (!pendingCard.length) return;

      const target = e.target;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable)
      ) {
        return;
      }

      const requestId = pendingCard.attr("data-request-id");
      if (!requestId) return;

      const questions = pendingCard.find(".user-input-choices");
      if (!questions.length) return;

      let activeQid = pendingCard.attr("data-active-qid");
      if (!activeQid || !pendingCard.find(`.user-input-choices[data-qid="${activeQid}"]`).length) {
        activeQid = $(questions[0]).attr("data-qid") || "";
        if (activeQid) pendingCard.attr("data-active-qid", activeQid);
      }

      const activeChoices = pendingCard.find(`.user-input-choices[data-qid="${activeQid}"]`);
      pendingCard.find(".user-input-choices").removeClass("is-active");
      pendingCard.find(".user-input-question").removeClass("is-active-question");
      activeChoices.addClass("is-active");
      pendingCard.find(`.user-input-question[data-qid="${activeQid}"]`).addClass("is-active-question");
      const options = activeChoices.find(".user-input-option");
      if (!options.length) return;

      const selected = options.filter(".is-selected");
      const currentIndex = selected.length ? options.index(selected.first()) : 0;

      if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        e.preventDefault();
        const delta = e.key === "ArrowUp" ? -1 : 1;
        let nextIndex = currentIndex + delta;
        if (nextIndex < 0) nextIndex = options.length - 1;
        if (nextIndex >= options.length) nextIndex = 0;
        options.removeClass("is-selected");
        $(options.get(nextIndex)).addClass("is-selected");
        return;
      }

      if (/^[1-9]$/.test(e.key)) {
        const index = parseInt(e.key, 10) - 1;
        if (index >= 0 && index < options.length) {
          e.preventDefault();
          options.removeClass("is-selected");
          $(options.get(index)).addClass("is-selected");
        }
        return;
      }

      if (e.key === "Enter") {
        e.preventDefault();
        submitUserInputResponse(requestId, pendingCard);
        return;
      }

      if (e.key === "Escape") {
        e.preventDefault();
        options.removeClass("is-selected");
        return;
      }
    });
  }

  // Send permission response to backend
  function sendPermissionResponse(requestId, responseId, cardElement) {
    if (!ipcRenderer || !currentTaskId) {
      console.error(
        "[ChatLog] Cannot send permission response: no IPC or task ID",
      );
      return;
    }

    // Disable all buttons in the card
    const card = $(cardElement);
    card.find(".permission-btn").prop("disabled", true);
    card.addClass("responded");

    // Update status based on response
    const statusEl = card.find(".permission-status");
    const statusText = statusEl.find("span");

    if (responseId === "deny") {
      statusEl.removeClass("confirmed").addClass("denied");
      statusText.text("Denied");
    } else {
      statusEl.removeClass("denied").addClass("confirmed");
      statusText.text("Confirmed");
    }

    // Send to backend
    console.log(
      "[ChatLog] Sending permission response:",
      currentTaskId,
      requestId,
      responseId,
    );
    ipcRenderer.send(
      "RespondToPermission",
      currentTaskId,
      requestId,
      responseId,
    );
  }

  // Send user input answers (Codex request_user_input)
  function submitUserInputResponse(requestId, cardElement) {
    if (!ipcRenderer || !currentTaskId) {
      console.error("[ChatLog] Cannot send user input: no IPC or task ID");
      return;
    }

    const card = $(cardElement);
    const answers = {};

    // Collect selected option answers
    card.find(".user-input-choices").each(function () {
      const choices = $(this);
      const qid = choices.attr("data-qid") || "";
      if (!qid) return;
      let selected = choices.find(".user-input-option.is-selected").first();
      if (!selected.length) {
        selected = choices.find(".user-input-option").first();
      }
      const val = selected.attr("data-value");
      if (val != null) {
        answers[qid] = { answers: [val.toString()] };
      }
    });

    // Collect freeform answers
    card.find('input.user-input-freeform').each(function () {
      const input = $(this);
      const qid = input.attr('data-qid');
      const val = (input.val() || '').toString().trim();
      if (qid && val) {
        answers[qid] = { answers: [val] };
      }
    });

    // Disable the card UI
    card.find('.permission-btn').prop('disabled', true);
    card.addClass('responded');

    const statusEl = card.find('.permission-status');
    statusEl.removeClass('denied').addClass('confirmed');
    statusEl.find('span').text('Answered');

    console.log('[ChatLog] Sending user input response:', currentTaskId, requestId, answers);
    ipcRenderer.send('RespondToUserInput', currentTaskId, requestId, answers);
  }

  function sendMessageWithText(text) {
    const input = $("#chatInput");
    const message = (text || "").toString().trim();
    if (!message) return;

    // Disable input while sending
    input.prop("disabled", true);
    $("#sendButton").hide();
    $("#stopButton").show();
    isGenerating = true;

    const outgoing = {
      type: "user",
      content: message,
      timestamp: new Date().toISOString(),
    };
    addMessage(outgoing);

    if (ipcRenderer && currentTaskId) {
      ipcRenderer.send("SendChatMessage", currentTaskId, message);
      updateStatus("Sending...", "running");
    }

    setTimeout(function () {
      input.prop("disabled", false);
      input.focus();
    }, 100);
  }

  // Set up IPC listeners for chat messages
  function setupIPCListeners() {
    if (!ipcRenderer) {
      console.log("[ChatLog] No IPC renderer available");
      return;
    }

    // Receive task info
    ipcRenderer.on("TaskInfo", function (e, taskInfo) {
      if (taskInfo && taskInfo.id === currentTaskId) {
        updateHeader(taskInfo);
        // Update slash commands for the task's agent
        if (window.chatSlashCommands && taskInfo.agent) {
          window.chatSlashCommands.setAgent(taskInfo.agent);
        }
        // Extract pending prompt state for draft message display
        pendingPrompt = taskInfo.pending_prompt || null;
        taskStatusState = taskInfo.status_state || "idle";
        // Store resolved path for "Open in..." functionality (worktree_path preferred, project_path fallback)
        currentTaskPath = taskInfo.worktree_path || taskInfo.project_path || null;
        updatePendingUI();
      }
    });

    // Receive chat log updates
    ipcRenderer.on("ChatLogUpdate", function (e, taskId, message) {
      if (taskId === currentTaskId) {
        addMessage(message);
      }
    });

    // Receive batch of messages (initial load)
    ipcRenderer.on("ChatLogBatch", function (e, taskId, messages) {
      if (taskId === currentTaskId && Array.isArray(messages)) {
        clearMessages();
        messages.forEach(function (msg) {
          addMessage(msg, false);
        });
        scrollToBottom();
      }
    });

    // Handle window close notification
    ipcRenderer.on("ChatLogClosed", function (e, taskId) {
      if (taskId === currentTaskId) {
        addSystemMessage("Session ended");
        updateStatus("Session ended", "idle");
      }
    });

    // Handle status updates
    ipcRenderer.on("ChatLogStatus", function (e, taskId, status, statusState) {
      if (taskId === currentTaskId) {
        if (statusState === "completed" || statusState === "idle") {
          finalizeStreamingMessage();
        }
        updateStatus(status, statusState);
      }
    });

    // Handle streaming updates (real-time token-by-token)
    ipcRenderer.on("ChatLogStreaming", function (e, taskId, update) {
      if (taskId === currentTaskId) {
        handleStreamingUpdate(update);
      }
    });

    // Handle available commands update from ACP (for slash command autocomplete)
    ipcRenderer.on("AvailableCommands", function (e, taskId, commands) {
      if (
        taskId === currentTaskId &&
        window.chatSlashCommands &&
        Array.isArray(commands)
      ) {
        console.log(
          "[ChatLog] Received",
          commands.length,
          "available commands from ACP",
        );
        window.chatSlashCommands.updateCommands(commands);
      }
    });

    // Handle AI-generated title updates
    ipcRenderer.on("TitleUpdate", function (e, taskId, title) {
      if (taskId === currentTaskId && title) {
        console.log("[ChatLog] Received title update:", title);
        $("#agentName").text(title);
      }
    });

    // Handle cost updates
    ipcRenderer.on("CostUpdate", function (e, taskId, cost) {
      if (taskId === currentTaskId && typeof cost === "number" && cost > 0) {
        const formatted = cost < 0.01 ? "<$0.01" : "$" + cost.toFixed(2);
        $("#sessionCost").text(formatted);
      }
    });

    // Open markdown links in the user's browser, not inside the webview
    $(document).on("click", "a[data-external='true']", function (e) {
      const href = $(this).attr("href");
      if (!href) return;
      e.preventDefault();
      if (bridge && bridge.shell && typeof bridge.shell.openExternal === "function") {
        bridge.shell.openExternal(href);
      } else {
        window.open(href, "_blank");
      }
    });
  }

  // Streaming state management
  let streamingElement = null;
  let streamingContent = "";
  let streamingType = null;
  let streamingItemId = null; // Track by itemId for Codex app-server
  let lastFinalizedContent = ""; // Track last finalized content to prevent duplicates

  // Handle a streaming update from the agent
  function handleStreamingUpdate(update) {
    const messageType = update.message_type;
    const content = update.content || "";

    // Handle different streaming message types
    switch (messageType) {
      case "text_chunk":
        appendToStreamingMessage("assistant", content, update.item_id);
        updateStatus("Responding...", "running");
        break;

      case "reasoning_chunk":
        appendToStreamingMessage("reasoning", content);
        updateStatus("Thinking...", "running");
        break;

      case "tool_call":
        // Finalize any existing streaming message first
        finalizeStreamingMessage();
        // Add the tool call as a regular message
        addMessage({
          type: "tool_call",
          name: update.name,
          arguments: update.arguments,
        });
        updateStatus("Running: " + (update.name || "tool"), "running");
        break;

      case "tool_return":
        // Instead of adding a separate message, merge result into the last tool call
        mergeToolResult(content);
        break;

      case "status":
        updateStatus(content, "running");
        break;

      case "permission_request":
        // Finalize any existing streaming message first
        finalizeStreamingMessage();
        // Add the permission request card
        addMessage({
          type: "permission_request",
          request_id: update.request_id,
          tool_name: update.tool_name,
          description: update.description,
          raw_input: update.raw_input,
          options: update.options,
        });
        updateStatus("Waiting for permission...", "running");
        break;

      case "user_input_request":
        finalizeStreamingMessage();
        addMessage({
          type: "user_input_request",
          request_id: update.request_id,
          questions: update.questions,
        });
        updateStatus("Waiting for input...", "running");
        break;

      case "plan_update":
        finalizeStreamingMessage();
        upsertPlanPanel(update);
        updateStatus("Plan updated", "running");
        break;

      case "plan_content":
        finalizeStreamingMessage();
        addMessage({
          type: "plan_content",
          file_path: update.file_path,
          content: update.content,
        });
        updateStatus("Plan content", "running");
        break;
    }
  }

  // Merge streaming text, handling potential overlap (borrowed from CodexMonitor)
  function mergeStreamingText(existing, delta) {
    if (!delta) return existing;
    if (!existing) return delta;
    if (delta === existing) return existing;
    if (delta.startsWith(existing)) return delta;
    if (existing.startsWith(delta)) return existing;

    // Check for overlap at the end of existing and start of delta
    const maxOverlap = Math.min(existing.length, delta.length);
    for (let length = maxOverlap; length > 0; length--) {
      if (existing.endsWith(delta.slice(0, length))) {
        return existing + delta.slice(length);
      }
    }

    return existing + delta;
  }

  function normalizePlanStatus(status) {
    if (!status) return "pending";
    const compact = status.toString().toLowerCase().replace(/[\s_-]/g, "");
    if (compact === "inprogress") return "inProgress";
    if (compact === "completed" || compact === "done") return "completed";
    return "pending";
  }

  function normalizePlanPayload(payload) {
    const explanation = (payload?.explanation || "").toString().trim() || null;
    const rawSteps = Array.isArray(payload?.plan) ? payload.plan : [];
    const steps = rawSteps
      .map((step) => ({
        step: (step?.step || "").toString().trim(),
        status: normalizePlanStatus(step?.status),
      }))
      .filter((step) => step.step.length > 0);
    return { explanation, steps };
  }

  // Render todo steps HTML for sidebar
  function renderTodoStepsHtml(payload) {
    const normalized = normalizePlanPayload(payload);
    const steps = normalized.steps;

    if (steps.length === 0) {
      return '<div class="todo-empty">No plan yet</div>';
    }

    const stepsHtml = steps
      .map((s) => {
        const statusClass =
          s.status === "completed"
            ? "completed"
            : s.status === "inProgress"
              ? "in-progress"
              : "pending";
        const icon =
          s.status === "completed"
            ? "fa-check-circle"
            : s.status === "inProgress"
              ? "fa-spinner fa-spin"
              : "fa-circle";
        return `
          <li class="todo-step ${statusClass}">
            <i class="fal ${icon}"></i>
            <span class="todo-step-text">${escapeHtml(s.step)}</span>
          </li>
        `;
      })
      .join("");

    let html = "";
    if (normalized.explanation) {
      html += `<div class="todo-explanation">${escapeHtml(normalized.explanation)}</div>`;
    }
    html += `<ul class="todo-steps">${stepsHtml}</ul>`;

    return html;
  }

  // Update the floating progress pill with plan data
  function updateProgressPill(payload) {
    const pill = document.getElementById("progressPill");
    const badge = document.getElementById("progressPillBadge");
    const stepText = document.getElementById("progressPillStep");
    const content = document.getElementById("progressPillContent");

    if (!pill) return;

    const normalized = normalizePlanPayload(payload);
    const steps = normalized.steps;
    const total = steps.length;
    const completed = steps.filter((s) => s.status === "completed").length;

    // Hide pill if no steps
    if (total === 0) {
      pill.classList.remove("visible");
      return;
    }

    // Show pill
    pill.classList.add("visible");

    // Update badge
    if (badge) {
      badge.textContent = `${completed}/${total}`;
      badge.classList.toggle("complete", completed === total);
    }

    // Find current step (first in-progress, or last completed, or first pending)
    const inProgress = steps.find((s) => s.status === "inProgress");
    const currentStep = inProgress || steps.find((s) => s.status === "pending") || steps[steps.length - 1];

    // Update step text
    if (stepText && currentStep) {
      stepText.textContent = currentStep.step;
      stepText.classList.toggle("in-progress", currentStep.status === "inProgress");
    }

    // Update dropdown content
    if (content) {
      content.innerHTML = renderTodoStepsHtml(payload);
    }
  }

  // Initialize progress pill toggle
  function initProgressPill() {
    const pill = document.getElementById("progressPill");
    const summary = document.getElementById("progressPillSummary");

    if (!pill || !summary) return;

    summary.addEventListener("click", function () {
      pill.classList.toggle("expanded");
    });

    // Close dropdown when clicking outside
    document.addEventListener("click", function (e) {
      if (!pill.contains(e.target)) {
        pill.classList.remove("expanded");
      }
    });
  }

  // Reset progress pill for new task
  function resetProgressPill() {
    const pill = document.getElementById("progressPill");
    const badge = document.getElementById("progressPillBadge");
    const stepText = document.getElementById("progressPillStep");
    const content = document.getElementById("progressPillContent");

    if (pill) {
      pill.classList.remove("visible", "expanded");
    }
    if (badge) {
      badge.textContent = "0/0";
      badge.classList.remove("complete");
    }
    if (stepText) {
      stepText.textContent = "No plan yet";
      stepText.classList.remove("in-progress");
    }
    if (content) {
      content.innerHTML = '<div class="todo-empty">No plan yet</div>';
    }
  }

  // Backward compatible aliases
  function updateTodoSidebar(payload) {
    updateProgressPill(payload);
  }

  function initTodoSidebar() {
    initProgressPill();
  }

  function resetTodoSidebar() {
    resetProgressPill();
  }

  // Legacy function for backward compatibility
  function upsertPlanPanel(payload) {
    updateProgressPill(payload);
  }

  // Append content to the current streaming message, creating one if needed
  // Now with itemId tracking for Codex app-server streaming
  function appendToStreamingMessage(type, content, itemId) {
    const container = $("#chatContainer");

    // If itemId provided and matches current streaming, just append
    if (itemId && itemId === streamingItemId && streamingElement) {
      streamingContent = mergeStreamingText(streamingContent, content);
      updateStreamingElementContent();
      if (autoScroll) scrollToBottom();
      return;
    }

    // If itemId changed or type changed, finalize current and start new
    if ((itemId && itemId !== streamingItemId) || streamingType !== type) {
      finalizeStreamingMessage();
    }

    // Start new streaming message if needed
    if (!streamingElement) {
      streamingType = type;
      streamingContent = "";
      streamingItemId = itemId || null;

      // Remove empty state if present
      container.find(".chat-empty").remove();

      // Create new streaming element with itemId as data attribute
      const div = document.createElement("div");
      div.className = "chat-message streaming";
      div.id = "streamingMessage";
      if (itemId) {
        div.dataset.itemId = itemId;
      }

      if (type === "assistant") {
        div.className += " assistant";
        // Start empty - cursor will be added when content arrives
        div.innerHTML = '';
      } else if (type === "reasoning") {
        div.className += " reasoning";
        div.innerHTML = `
          <div class="reasoning-header collapsed">
            <i class="fal fa-lightbulb"></i>
            <span class="reasoning-title">Thinking...</span>
            <i class="fal fa-spinner fa-spin" style="margin-left: auto;"></i>
            <i class="fal fa-chevron-down reasoning-toggle"></i>
          </div>
          <div class="reasoning-content streaming-text hidden"></div>
        `;
      }

      container.append(div);
      streamingElement = div;
    }

    // Append content
    streamingContent = mergeStreamingText(streamingContent, content);
    updateStreamingElementContent();

    if (autoScroll) {
      scrollToBottom();
    }
  }

  // Update the streaming element's display content
  function updateStreamingElementContent() {
    if (!streamingElement) return;

    if (streamingType === "reasoning") {
      const strippedContent = stripLeadingBlankLines(streamingContent);
      const textEl = streamingElement.querySelector(".streaming-text");
      if (textEl) {
        textEl.textContent = strippedContent;
      }
    } else {
      // Use markdown for assistant messages - only show cursor when there's content
      if (streamingContent && streamingContent.trim()) {
        const renderedContent = renderMarkdown(streamingContent);
        streamingElement.innerHTML =
          '<div class="markdown-content">' + renderedContent + '</div>' +
          '<span class="streaming-cursor">|</span>';
      }
    }
  }

  // Finalize the current streaming message (remove cursor, mark complete)
  function finalizeStreamingMessage() {
    if (!streamingElement) return;

    streamingElement.classList.remove("streaming");

    if (streamingType === "assistant") {
      // Render final content with markdown
      streamingElement.innerHTML = '<div class="markdown-content">' +
        renderMarkdown(streamingContent) + '</div>';
      // Save content to detect duplicates from ChatLogUpdate
      lastFinalizedContent = streamingContent;
    } else if (streamingType === "reasoning") {
      // Remove spinner when done (chevron already exists)
      const header = streamingElement.querySelector(".reasoning-header");
      if (header) {
        const spinner = header.querySelector(".fa-spinner");
        if (spinner) {
          spinner.remove();
        }
      }
    }

    streamingElement = null;
    streamingContent = "";
    streamingType = null;
    streamingItemId = null; // Reset itemId for next message
  }

  function populateCodeReviewMenu() {
    var menu = document.getElementById('codeReviewMenu');
    if (!menu) return;
    var agents = Object.keys(AGENT_NAMES);
    menu.innerHTML = agents.map(function(agentId) {
      var name = AGENT_NAMES[agentId];
      var icon = AGENT_REVIEW_ICONS[agentId] || 'images/codex.png';
      return '<button class="log-switcher-item" type="button" data-agent="' + agentId + '" data-tauri-drag-region="false">' +
        '<img class="log-switcher-item-icon" src="' + icon + '" alt="" aria-hidden="true" />' +
        name +
        '</button>';
    }).join('');
  }

  function initiateCodeReview(reviewAgentId) {
    if (codeReviewLoading) return;
    if (!currentTaskPath) {
      window.alert('No project path available for this task.');
      return;
    }

    codeReviewLoading = true;
    var btn = document.getElementById('codeReviewBtn');
    var labelEl = document.getElementById('codeReviewLabel');
    var originalLabel = labelEl ? labelEl.textContent : 'Review';
    if (btn) btn.classList.add('loading');
    if (labelEl) labelEl.innerHTML = 'Gathering\u2026 <span class="code-review-spinner"></span>';

    var tauriCore = window.__TAURI__ && window.__TAURI__.core;
    var tauriInvokeFn = tauriCore && tauriCore.invoke;
    if (!tauriInvokeFn) {
      console.error('[ChatLog] Tauri invoke not available for code review');
      resetCodeReviewButton(btn, labelEl, originalLabel);
      return;
    }

    tauriInvokeFn('gather_code_review_context', { projectPath: currentTaskPath })
      .then(function(context) {
        var prompt = buildCodeReviewPrompt(context);
        ipcRenderer.send('CreateAgentSession', {
          agentId: reviewAgentId,
          prompt: prompt,
          projectPath: currentTaskPath,
          baseBranch: null,
          planMode: false,
          thinking: true,
          useWorktree: false,
          permissionMode: 'bypassPermissions',
          execModel: 'default',
          reasoningEffort: null,
          agentMode: null,
          codexMode: null,
          multiCreate: false,
          attachments: []
        });
      })
      .catch(function(err) {
        console.error('[ChatLog] gather_code_review_context error:', err);
        window.alert('Failed to gather code review context: ' + (err.message || err));
      })
      .finally(function() {
        resetCodeReviewButton(btn, labelEl, originalLabel);
      });
  }

  function resetCodeReviewButton(btn, labelEl, originalLabel) {
    codeReviewLoading = false;
    if (btn) btn.classList.remove('loading');
    if (labelEl) labelEl.textContent = originalLabel;
  }

  function buildCodeReviewPrompt(context) {
    var lines = [];
    lines.push('You are performing a code review on the changes in the current branch.');
    lines.push('');
    lines.push('The current branch is **' + context.current_branch + '**, and the target branch is **origin/' + context.base_branch + '**.');
    lines.push('');
    lines.push('## Code Review Instructions');
    lines.push('');
    lines.push('**CRITICAL: EVERYTHING YOU NEED IS ALREADY PROVIDED BELOW.** The complete git diff and full commit history are included in this message.');
    lines.push('');
    lines.push('**DO NOT run git diff, git log, git status, or ANY other git commands.** All the information you need to perform this review is already here.');
    lines.push('');
    lines.push('When reviewing the diff:');
    lines.push('1. **Focus on logic and correctness** - Check for bugs, edge cases, and potential issues.');
    lines.push('2. **Consider readability** - Is the code clear and maintainable? Does it follow best practices in this repository?');
    lines.push('3. **Evaluate performance** - Are there obvious performance concerns or optimizations that could be made?');
    lines.push('4. **Assess test coverage** - Does the repository have testing patterns? If so, are there adequate tests for these changes?');
    lines.push('5. **Security review** - Check for injection risks, auth issues, hardcoded secrets, or other vulnerabilities.');
    lines.push('6. **Ask clarifying questions** - Ask the user for clarification if you are unsure about the changes or need more context.');
    lines.push('7. **Don\'t be overly pedantic** - Nitpicks are fine, but only if they are relevant issues within reason.');
    lines.push('');
    lines.push('In your output:');
    lines.push('- Provide a short summary overview of the general code quality.');
    lines.push('- Present findings as a numbered list (no tables, no HTML).');
    lines.push('- For each finding, use this exact structure with labels on separate lines:');
    lines.push('  - Location: file path + line number(s) if available');
    lines.push('  - Snippet: fenced code block (keep it short)');
    lines.push('  - Issue: what is wrong and why it matters');
    lines.push('  - Recommendation: concrete fix or next step');
    lines.push('  - Severity: low | medium | high');
    lines.push('- If no issues are found, write: "Findings: None" and briefly state why.');
    lines.push('- End with an overall verdict line: "Verdict: APPROVE" or "Verdict: REQUEST CHANGES" or "Verdict: NEEDS DISCUSSION".');
    lines.push('- Avoid markdown tables entirely. Use headings and lists only.');
    lines.push('');

    if (context.commit_log && context.commit_log.trim()) {
      lines.push('## Commit History');
      lines.push('');
      lines.push('```');
      lines.push(context.commit_log.trim());
      lines.push('```');
      lines.push('');
    }

    lines.push('## Full Diff');
    lines.push('');
    lines.push('**REMINDER: DO NOT use any tools to fetch git information.** Simply read the diff and commit history provided above.');
    lines.push('');
    if (context.diff_truncated) {
      lines.push('> **Note:** The diff was truncated due to size (~100KB limit). Focus on the included changes and note that some files may be missing.');
      lines.push('');
    }
    if (context.diff && context.diff.trim()) {
      lines.push('```diff');
      lines.push(context.diff.trim());
      lines.push('```');
    } else {
      lines.push('No changes detected between `' + context.current_branch + '` and `origin/' + context.base_branch + '`.');
    }

    return lines.join('\n');
  }

  // Update header with task info
  function updateHeader(taskInfo) {
    const agent = taskInfo.agent || "codex";
    const logo = AGENT_LOGOS[agent] || AGENT_LOGOS["codex"];
    const name = AGENT_NAMES[agent] || "Agent";

    $("#agentLogo").html(logo);
    // Use AI-generated title summary if available, otherwise default header
    if (taskInfo.title_summary) {
      $("#agentName").text(taskInfo.title_summary);
    } else {
      $("#agentName").text(name + " Chat Log");
    }
  }

  // Update status bar
  function updateStatus(status, statusState) {
    const statusDot = $("#statusDot");
    const statusText = $("#statusText");

    const safeStatus = (status || "Ready").trim();
    const isGenericWork =
      /^thinking\b/i.test(safeStatus) ||
      /^responding\b/i.test(safeStatus) ||
      /^tool completed\b/i.test(safeStatus);
    const showWorking = statusState === "running" && isGenericWork;

    statusText.text(showWorking ? "Working" : safeStatus);
    statusText.toggleClass("status-thinking", showWorking);

    // Remove all state classes
    statusDot.removeClass("running error");

    // Add appropriate class
    if (statusState === "running") {
      statusDot.addClass("running");
      startElapsedTimer();
    } else if (statusState === "idle" || statusState === "completed") {
      finishGeneration();
    } else if (statusState === "error") {
      statusDot.addClass("error");
      stopElapsedTimer();
    } else {
      stopElapsedTimer();
    }
  }

  // Start elapsed time timer
  function startElapsedTimer() {
    if (elapsedTimer) return; // Already running

    startTime = Date.now();
    updateElapsedTime();
    elapsedTimer = setInterval(updateElapsedTime, 1000);
  }

  // Stop elapsed time timer
  function stopElapsedTimer() {
    if (elapsedTimer) {
      clearInterval(elapsedTimer);
      elapsedTimer = null;
    }
  }

  // Update elapsed time display
  function updateElapsedTime() {
    if (!startTime) return;

    const elapsed = Math.floor((Date.now() - startTime) / 1000);
    const minutes = Math.floor(elapsed / 60);
    const seconds = elapsed % 60;

    const display =
      minutes > 0 ? minutes + "m " + seconds + "s" : seconds + "s";

    $("#elapsedTime").text(display);
  }

  // Update pending prompt UI based on current state
  function updatePendingUI() {
    const actions = document.getElementById("pendingActions");
    const chatInput = document.getElementById("chatInput");
    const sendButton = document.getElementById("sendButton");

    if (pendingPrompt && taskStatusState === "idle") {
      // Show pending message
      addPendingMessage(pendingPrompt);
      if (actions) actions.style.display = "flex";
      if (chatInput) {
        chatInput.disabled = true;
        chatInput.placeholder = "Start the session first...";
      }
      if (sendButton) sendButton.disabled = true;
    } else {
      if (actions) actions.style.display = "none";
      if (chatInput) {
        chatInput.disabled = false;
        chatInput.placeholder = "Send a message to the agent...";
      }
      if (sendButton) sendButton.disabled = false;
    }
  }

  // Add a pending (draft) message to the chat
  function addPendingMessage(prompt) {
    const container = $("#chatContainer");
    container.find(".chat-empty").remove();
    container.find(".chat-message.pending").remove(); // Remove any existing pending message

    const div = document.createElement("div");
    div.className = "chat-message pending user";
    div.id = "pendingPromptMessage";
    div.innerHTML =
      '<div class="draft-label">Draft - Not Sent</div>' + escapeHtml(prompt);
    container.append(div);
    scrollToBottom();
  }

  // Track if generation is in progress
  let isGenerating = false;
  let pendingAttachments = [];

  // Send a message to the agent
  function sendMessage() {
    const input = $("#chatInput");
    const message = input.val().trim();
    const hasAttachments = pendingAttachments.length > 0;

    if (!message && !hasAttachments) return;

    // Disable input while sending
    input.prop("disabled", true);
    $("#sendButton").hide();
    $("#stopButton").show();
    isGenerating = true;

    // Add user message to chat
    const outgoing = {
      type: "user",
      content: message,
      timestamp: new Date().toISOString(),
    };
    if (hasAttachments) {
      outgoing.attachments = pendingAttachments.map((att) => ({
        id: att.id,
        fileName: att.fileName,
        mimeType: att.mimeType,
        dataUrl: att.dataUrl,
      }));
    }
    addMessage(outgoing);

    // Clear input
    input.val("");
    input.css("height", "auto");
    clearPendingAttachments();

    // Send to backend
    if (ipcRenderer && currentTaskId) {
      ipcRenderer.send("SendChatMessage", currentTaskId, message);
      updateStatus("Sending...", "running");
    }

    // Re-enable input (backend will update status when done)
    setTimeout(function () {
      input.prop("disabled", false);
      input.focus();
    }, 100);
  }

  // Stop the current generation
  function stopGeneration() {
    if (!isGenerating) return;

    // Send stop request to backend
    if (ipcRenderer && currentTaskId) {
      ipcRenderer.send("StopGeneration", currentTaskId);
      addSystemMessage("Stopping generation...");
    }

    // Update UI state
    finishGeneration();
  }

  // Called when generation is complete (success, error, or stopped)
  function finishGeneration() {
    isGenerating = false;
    $("#stopButton").hide();
    $("#sendButton").show().prop("disabled", false);
    finalizeStreamingMessage();
  }

  // Get icon HTML for a tool name
  function getToolIconHtml(toolName) {
    const iconClass = TOOL_ICONS[toolName];
    if (iconClass) {
      return `<i class="fal ${iconClass}"></i>`;
    }
    // Use ghost SVG for unknown tools
    return GHOST_SVG;
  }

  // Add a message to the chat
  function addMessage(message, shouldScroll = true) {
    if (!message) return;

    const container = $("#chatContainer");

    // Remove empty state if present
    container.find(".chat-empty").remove();

    const type = message.type || message.message_type || "system";
    const content = message.content || message.text || "";

    if (type === "plan_update") {
      let payload = message;
      if (typeof message.content === "string") {
        try {
          payload = JSON.parse(message.content);
        } catch (e) {
          payload = message;
        }
      }
      upsertPlanPanel(payload);
      return;
    }

    if (type === "plan_content") {
      // Handle plan_content - render as a special assistant message with the plan
      let payload = message;
      if (typeof message.content === "string" && message.content.startsWith("{")) {
        try {
          payload = JSON.parse(message.content);
        } catch (e) {
          payload = message;
        }
      }
      // Fall through to render as a message with the content
    }

    // Skip duplicate assistant messages (already rendered via streaming)
    if ((type === "assistant" || type === "assistant_message") && streamingType === "assistant") {
      if (streamingElement && content && streamingContent && content.trim() === streamingContent.trim()) {
        finalizeStreamingMessage();
        return;
      }
    }
    if ((type === "assistant" || type === "assistant_message") && lastFinalizedContent) {
      if (content && content === lastFinalizedContent) {
        console.log("[ChatLog] Skipping duplicate assistant message");
        lastFinalizedContent = ""; // Reset after checking
        return;
      }
      lastFinalizedContent = ""; // Reset if content doesn't match
    }

    // Handle tool_return specially - try to merge into pending tool call first
    if (type === "tool_return" || type === "tool_return_message") {
      const rawReturn = message.tool_return || message.result || message.content || "";
      const pendingToolCall = container.find(".tool-call.pending").last();

      if (pendingToolCall.length > 0) {
        // Merge into the pending tool call
        mergeToolResult(rawReturn);
        if (shouldScroll && autoScroll) {
          scrollToBottom();
        }
        return;
      }
      // Fall through to create standalone if no pending tool call
    }

    const msgElement = createMessageElement(message);
    // Skip if createMessageElement returned null (e.g., empty tool return)
    if (!msgElement) return;

    container.append(msgElement);

    if (shouldScroll && autoScroll) {
      scrollToBottom();
    }
  }

  // Merge a tool result into the most recent pending tool call
  function mergeToolResult(rawContent) {
    const container = $("#chatContainer");
    const pendingToolCall = container.find(".tool-call.pending").last();

    if (pendingToolCall.length === 0) {
      // No pending tool call found - render as standalone (fallback)
      addMessage({
        type: "tool_return",
        tool_return: rawContent,
      });
      return;
    }

    const returnValue = (rawContent || "").toString().trim();

    // Skip empty results
    if (!returnValue || returnValue === "null" || returnValue === "undefined") {
      // Just mark as complete without adding result section
      pendingToolCall.removeClass("pending").addClass("complete");
      return;
    }

    const returnDetails = extractToolReturnDetails(rawContent);

    // Handle diff results specially
    if (returnDetails.diffs.length > 0) {
      // For diffs, render them after the tool call as a separate element
      pendingToolCall.removeClass("pending").addClass("complete");

      const diffDiv = document.createElement("div");
      diffDiv.className = "chat-message diff";
      returnDetails.diffs.forEach((diffText) => {
        diffDiv.appendChild(renderDiffBlock(diffText));
      });
      pendingToolCall.after(diffDiv);

      if (autoScroll) scrollToBottom();
      return;
    }

    // Mark tool call as complete
    pendingToolCall.removeClass("pending").addClass("complete");

    // Inject result into the tool card body
    const resultSection = pendingToolCall.find(".tool-result-section");
    const resultText = returnDetails.text || returnValue;
    const truncatedResult = truncateText(resultText, 500);

    resultSection.html(`
      <div class="tool-section-header" data-section="result">
        <span class="tool-section-label">RESULT</span>
      </div>
      <div class="tool-section-content">${escapeHtml(truncatedResult)}</div>
    `);

    if (autoScroll) scrollToBottom();
  }

  // Create DOM element for a message
  function createMessageElement(message) {
    const div = document.createElement("div");
    div.className = "chat-message";

    const type = message.type || message.message_type || "system";
    const content = message.content || message.text || "";
    const timestamp = message.timestamp
      ? formatTimestamp(message.timestamp)
      : "";

    switch (type) {
      case "user":
      case "user_message":
        div.className += " user";
        const attachments = message.attachments || [];

        if (attachments.length > 0) {
          // User message with images - show image strip above text
          div.className += " has-images";

          // Create wrapper
          const wrapper = document.createElement("div");
          wrapper.className = "user-message-wrapper";

          // Create image strip
          const imageStrip = document.createElement("div");
          imageStrip.className = "user-image-strip";

          attachments.forEach((att, index) => {
            const img = document.createElement("img");
            img.className = "user-image-thumb";
            img.src = att.dataUrl || `phantom://attachment/${att.id}`;
            img.alt = att.fileName || `Image ${index + 1}`;
            img.title = att.fileName || `Image ${index + 1}`;
            img.dataset.attachmentId = att.id;
            img.dataset.index = index;
            // Click to show lightbox
            img.onclick = () => showImageLightbox(img.src, img.alt);
            imageStrip.appendChild(img);
          });

          wrapper.appendChild(imageStrip);

          // Create text bubble with inline placeholders
          if (content.trim()) {
            const textBubble = document.createElement("div");
            textBubble.className = "user-message-text";

            // Replace [Image X] or similar with animated placeholders
            let processedContent = escapeHtml(content);
            attachments.forEach((att, index) => {
              const placeholderHtml = `<span class="image-placeholder" data-attachment-id="${att.id}" onclick="showImageLightbox('${att.dataUrl || ""}', '${escapeHtml(att.fileName || "Image " + (index + 1))}')"><i class="fal fa-image"></i> Image ${index + 1}</span>`;
              // Replace common image reference patterns
              processedContent = processedContent
                .replace(
                  new RegExp(`\\[Image\\s*${index + 1}\\]`, "gi"),
                  placeholderHtml,
                )
                .replace(
                  new RegExp(`\\[image${index + 1}\\]`, "gi"),
                  placeholderHtml,
                );
            });

            textBubble.innerHTML = processedContent;
            wrapper.appendChild(textBubble);
          }

          div.appendChild(wrapper);
        } else {
          // Regular user message without images
          div.innerHTML = escapeHtml(content);
        }

        if (content && content.trim()) {
          const copyBtn = document.createElement("button");
          copyBtn.className = "copy-btn";
          copyBtn.type = "button";
          copyBtn.setAttribute("aria-label", "Copy message");
          copyBtn.innerHTML = '<i class="fal fa-copy"></i>';
          copyBtn.addEventListener("click", (event) => {
            event.stopPropagation();
            copyToClipboard(content, copyBtn);
          });
          div.appendChild(copyBtn);
        }
        break;

      case "assistant":
      case "assistant_message":
        div.className += " assistant";
        div.innerHTML = '<div class="markdown-content">' + renderMarkdown(content) + '</div>';
        break;

      case "reasoning":
      case "reasoning_message":
        div.className += " reasoning";
        const reasoning = stripLeadingBlankLines(
          (message.reasoning || content || "").trimEnd(),
        );
        div.innerHTML = `
          <div class="reasoning-header collapsed">
            <i class="fal fa-lightbulb"></i>
            <span class="reasoning-title">Thinking...</span>
            <i class="fal fa-chevron-down reasoning-toggle"></i>
          </div>
          <div class="reasoning-content hidden">${escapeHtml(truncateText(reasoning, 1000))}</div>
        `;
        break;

      case "tool_call":
      case "tool_call_message": {
        const toolName = message.tool_call
          ? message.tool_call.name
          : message.name || "Tool";
        const toolArgs = message.tool_call
          ? message.tool_call.arguments
          : message.arguments || "";

        if (toolName.toLowerCase() === "askuserquestion") {
          div.className += " permission-request user-input-request ask-user-question";
          let parsedArgs = null;
          if (toolArgs) {
            try {
              parsedArgs = typeof toolArgs === "string" ? JSON.parse(toolArgs) : toolArgs;
            } catch (e) {
              parsedArgs = null;
            }
          }
          const questions = Array.isArray(parsedArgs?.questions)
            ? parsedArgs.questions
            : null;

          if (questions && questions.length > 0) {
            const questionsHtml = questions
              .map((q, idx) => {
                const qid = q?.id ? q.id.toString() : `askuserquestion-${idx + 1}`;
                const header = q?.header || `Question ${idx + 1}`;
                const questionText =
                  q?.question || q?.prompt || q?.text || q?.message || "";
                const opts = Array.isArray(q?.options)
                  ? q.options
                  : Array.isArray(q?.choices)
                    ? q.choices
                    : null;
                const multiSelect = q?.multiSelect === true;

                if (opts && opts.length > 0) {
                  const optionButtons = opts
                    .map((opt, optIdx) => {
                      const label =
                        typeof opt === "string"
                          ? opt
                          : opt.label || opt.value || "";
                      const desc =
                        typeof opt === "string" ? "" : opt.description || "";
                      return `
                        <button
                          class="user-input-option ${optIdx === 0 ? "is-selected" : ""}"
                          type="button"
                          data-qid="${escapeHtml(qid)}"
                          data-value="${escapeHtml(label)}"
                          data-index="${optIdx}"
                        >
                          <span class="user-input-option-number">${optIdx + 1}.</span>
                          <span class="user-input-option-text">
                            <strong>${escapeHtml(label)}</strong>
                            ${desc ? `<span class="user-input-option-desc"> — ${escapeHtml(desc)}</span>` : ""}
                          </span>
                        </button>
                      `;
                    })
                    .join("");
                  return `
                    <div class="permission-action user-input-question" data-qid="${escapeHtml(qid)}" style="margin-top: 10px;">
                      <div class="permission-action-label">
                        ${escapeHtml(header)}
                        <span class="user-input-active-badge">Active</span>
                      </div>
                      <div class="permission-reason">${escapeHtml(questionText)}</div>
                      <div class="user-input-choices" data-qid="${escapeHtml(qid)}" data-multiselect="${multiSelect ? "true" : "false"}">${optionButtons}</div>
                    </div>
                  `;
                }

                return `
                  <div class="permission-action user-input-question" data-qid="${escapeHtml(qid)}" style="margin-top: 10px;">
                    <div class="permission-action-label">
                      ${escapeHtml(header)}
                      <span class="user-input-active-badge">Active</span>
                    </div>
                    <div class="permission-reason">${escapeHtml(questionText)}</div>
                    <input class="user-input-freeform" data-qid="${escapeHtml(qid)}" type="text" placeholder="Type your answer" style="width: 100%; margin-top: 8px;" />
                  </div>
                `;
              })
              .join("");

            div.innerHTML = `
              <div class="permission-header">
                <span class="permission-icon"><i class="fal fa-question-circle"></i></span>
                <div class="permission-header-text">
                  <h4>Question</h4>
                </div>
              </div>
              <div class="permission-body">
                ${questionsHtml}
              </div>
              <div class="permission-actions" style="margin-top: 10px;">
                <button class="permission-btn approve ask-question-submit">Submit</button>
              </div>
              <div class="permission-status">
                <div class="status-dot"></div>
                <span>Waiting for your answer</span>
              </div>
            `;
            break;
          }

          const questionText =
            parsedArgs?.question ||
            parsedArgs?.prompt ||
            parsedArgs?.text ||
            parsedArgs?.message ||
            "";
          const opts = Array.isArray(parsedArgs?.options)
            ? parsedArgs.options
            : Array.isArray(parsedArgs?.choices)
              ? parsedArgs.choices
              : null;
          const optionButtons = opts
            ? opts
                .map((opt, idx) => {
                  const label = typeof opt === "string" ? opt : opt.label || "";
                  const desc = typeof opt === "string" ? "" : opt.description || "";
                  return `
                    <button
                      class="user-input-option ${idx === 0 ? "is-selected" : ""}"
                      type="button"
                      data-qid="askuserquestion"
                      data-value="${escapeHtml(label)}"
                      data-index="${idx}"
                    >
                      <span class="user-input-option-number">${idx + 1}.</span>
                      <span class="user-input-option-text">
                        <strong>${escapeHtml(label)}</strong>
                        ${desc ? `<span class="user-input-option-desc"> — ${escapeHtml(desc)}</span>` : ""}
                      </span>
                    </button>
                  `;
                })
                .join("")
            : "";

          div.innerHTML = `
            <div class="permission-header">
              <span class="permission-icon"><i class="fal fa-question-circle"></i></span>
              <div class="permission-header-text">
                <h4>Question</h4>
              </div>
            </div>
            <div class="permission-body">
              ${questionText ? `<div class="permission-reason">${escapeHtml(questionText)}</div>` : ""}
              ${optionButtons ? `<div class="user-input-choices" data-qid="askuserquestion">${optionButtons}</div>` : `
                <input class="user-input-freeform" data-qid="askuserquestion" type="text" placeholder="Type your answer" style="width: 100%; margin-top: 8px;" />
              `}
            </div>
            <div class="permission-actions" style="margin-top: 10px;">
              <button class="permission-btn approve ask-question-submit">Submit</button>
            </div>
            <div class="permission-status">
              <div class="status-dot"></div>
              <span>Waiting for your answer</span>
            </div>
          `;
          break;
        }

        div.className += " tool-call pending";
        const toolIconHtml = getToolIconHtml(toolName);
        const formattedArgs = formatArgs(toolArgs);
        const hasArgs = formattedArgs && formattedArgs.trim() !== "{}";

        div.innerHTML = `
          <div class="tool-card-header collapsed">
            <span class="tool-icon">${toolIconHtml}</span>
            <span class="tool-name-text">${escapeHtml(toolName)}</span>
            <i class="fal fa-chevron-down tool-card-toggle"></i>
          </div>
          <div class="tool-card-body hidden">
            ${hasArgs ? `
            <div class="tool-section tool-args-section">
              <div class="tool-section-header" data-section="args">
                <span class="tool-section-label">ARGUMENTS</span>
              </div>
              <div class="tool-section-content">${escapeHtml(formattedArgs)}</div>
            </div>
            ` : ''}
            <div class="tool-result-section"></div>
          </div>
        `;
        break;
      }

      case "tool_return":
      case "tool_return_message":
        // Tool returns are now merged into tool calls via mergeToolResult()
        // This case only handles standalone tool returns (e.g., from history loading)
        const rawReturn =
          message.tool_return || message.result || content || "";
        const returnValue = rawReturn.toString().trim();
        // Skip rendering empty tool returns
        if (
          !returnValue ||
          returnValue === "null" ||
          returnValue === "undefined"
        ) {
          return null;
        }
        const returnDetails = extractToolReturnDetails(rawReturn);
        if (returnDetails.diffs.length > 0) {
          div.className += " diff";
          returnDetails.diffs.forEach((diffText) => {
            div.appendChild(renderDiffBlock(diffText, message.title));
          });
          break;
        }
        // Render as standalone result (fallback for history)
        div.className += " tool-return standalone";
        div.innerHTML = `
          <div class="tool-card-header">
            <span class="tool-icon"><i class="fal fa-check-circle"></i></span>
            <span class="tool-name-text">Result</span>
          </div>
          <div class="tool-section">
            <div class="tool-section-content">${escapeHtml(truncateText(returnDetails.text || returnValue, 500))}</div>
          </div>
        `;
        break;

      case "file_edit":
      case "file_edit_message":
        div.className += " file-edit";
        const filePath = message.file_path || message.path || "Unknown file";
        const editContent = message.edit_content || content;

        div.innerHTML = `
          <div class="file-path">
            <i class="fal fa-file-edit"></i>
            ${escapeHtml(filePath)}
          </div>
          <div class="edit-preview">${escapeHtml(truncateText(editContent, 300))}</div>
        `;
        break;

      case "diff":
      case "diff_message":
        div.className += " diff";
        const diffText = message.diff || content || "";
        div.appendChild(renderDiffBlock(diffText, message.title));
        break;

      case "error":
      case "error_message":
        div.className += " error";
        div.innerHTML =
          '<i class="fal fa-exclamation-triangle" style="margin-right: 6px;"></i>' +
          escapeHtml(content);
        break;

      case "permission_request":
        div.className += " permission-request";
        div.setAttribute("data-request-id", message.request_id || "");
        const prToolName = message.tool_name || "Action";
        const prDescription = message.description || "Permission required";
        const prRawInput = message.raw_input || "";
        const prOptions = message.options || [];
        const prDetails = normalizePermissionRequest({
          toolName: prToolName,
          description: prDescription,
          rawInput: prRawInput,
        });

        const actionsHtml = buildPermissionActions(prOptions);

        div.innerHTML = `
          <div class="permission-header">
            <span class="permission-icon"><i class="fal fa-lightbulb"></i></span>
            <div class="permission-header-text">
              <h4>Thinking</h4>
            </div>
          </div>
          <div class="permission-body">
            <div class="permission-meta">
              <span>Tool</span>
              <code>${escapeHtml(prToolName)}</code>
            </div>
            <div class="permission-action">
              <div class="permission-action-label">${escapeHtml(prDetails.actionLabel)}</div>
              ${
                prDetails.command
                  ? `<div class="permission-action-command">${escapeHtml(prDetails.command)}</div>`
                  : ""
              }
            </div>
            ${
              prDetails.reason
                ? `<div class="permission-reason">${escapeHtml(prDetails.reason)}</div>`
                : ""
            }
            ${
              prDetails.amendment
                ? `<div class="permission-amendment"><span>Proposed Amendment</span><code>${escapeHtml(
                    prDetails.amendment,
                  )}</code></div>`
                : ""
            }
          </div>
          ${
            prDetails.raw
              ? `
            <div class="permission-raw-toggle">
              <i class="fal fa-caret-right"></i>
              <span>View Request Details</span>
            </div>
            <div class="permission-raw-content">${escapeHtml(prDetails.raw)}</div>
          `
              : ""
          }
          ${actionsHtml}
          <div class="permission-status">
            <div class="status-dot"></div>
            <span>Waiting for confirmation</span>
          </div>
        `;
        break;

      case "user_input_request":
        div.className += " permission-request user-input-request";
        let requestId = message.request_id;
        let questions = Array.isArray(message.questions) ? message.questions : [];
        if ((!requestId || questions.length === 0) && typeof message.content === "string") {
          try {
            const parsed = JSON.parse(message.content);
            if (!requestId && parsed && typeof parsed.requestId === "string") {
              requestId = parsed.requestId;
            }
            if (questions.length === 0 && Array.isArray(parsed?.questions)) {
              questions = parsed.questions;
            }
            if (questions.length === 0 && Array.isArray(parsed)) {
              questions = parsed;
            }
          } catch (e) {
            // ignore parse errors
          }
        }
        div.setAttribute("data-request-id", requestId || "");

        const qHtml = questions
          .map((q) => {
            const opts = Array.isArray(q.options) ? q.options : null;
            if (opts && opts.length > 0) {
              const optionButtons = opts
                .map(
                  (opt, idx) => `
                    <button
                      class="user-input-option ${idx === 0 ? "is-selected" : ""}"
                      type="button"
                      data-qid="${escapeHtml(q.id)}"
                      data-value="${escapeHtml(opt.label)}"
                      data-index="${idx}"
                    >
                      <span class="user-input-option-number">${idx + 1}.</span>
                      <span class="user-input-option-text">
                        <strong>${escapeHtml(opt.label)}</strong>
                        ${opt.description ? `<span class="user-input-option-desc"> — ${escapeHtml(opt.description)}</span>` : ""}
                      </span>
                    </button>
                  `,
                )
                .join("");
              return `
                <div class="permission-action user-input-question" data-qid="${escapeHtml(q.id)}" style="margin-top: 10px;">
                  <div class="permission-action-label">
                    ${escapeHtml(q.header || "Question")}
                    <span class="user-input-active-badge">Active</span>
                  </div>
                  <div class="permission-reason">${escapeHtml(q.question || "")}</div>
                  <div class="user-input-choices" data-qid="${escapeHtml(q.id)}">${optionButtons}</div>
                </div>
              `;
            }

            return `
              <div class="permission-action user-input-question" data-qid="${escapeHtml(q.id)}" style="margin-top: 10px;">
                <div class="permission-action-label">
                  ${escapeHtml(q.header || "Question")}
                  <span class="user-input-active-badge">Active</span>
                </div>
                <div class="permission-reason">${escapeHtml(q.question || "")}</div>
                <input class="user-input-freeform" data-qid="${escapeHtml(q.id)}" type="text" placeholder="Type your answer" style="width: 100%; margin-top: 8px;" />
              </div>
            `;
          })
          .join("");

        div.innerHTML = `
          <div class="permission-header">
            <span class="permission-icon"><i class="fal fa-question-circle"></i></span>
            <div class="permission-header-text">
              <h4>Question</h4>
            </div>
          </div>
          <div class="permission-body">
            ${qHtml || "<div class=\"permission-reason\">Codex requested input.</div>"}
          </div>
          <div class="permission-actions" style="margin-top: 10px;">
            <button class="permission-btn approve" data-action="submit_user_input">Submit</button>
          </div>
          <div class="permission-status">
            <div class="status-dot"></div>
            <span>Waiting for your answer</span>
          </div>
        `;
        break;

      case "plan_update": {
        div.className += " plan-panel";
        let payload = message;
        if (typeof message.content === "string") {
          try {
            payload = JSON.parse(message.content);
          } catch (e) {
            payload = message;
          }
        }
        div.innerHTML = renderPlanPanel(payload);
        break;
      }

      case "plan_content": {
        // Plan content from ExitPlanMode - display the plan as a markdown message
        div.className += " assistant plan-content";
        let planContent = message.content || "";
        let filePath = message.file_path || "";
        
        // Parse JSON payload if needed
        if (typeof message.content === "string" && message.content.startsWith("{")) {
          try {
            const payload = JSON.parse(message.content);
            planContent = payload.content || message.content;
            filePath = payload.file_path || "";
          } catch (e) {
            // Use raw content
          }
        }
        
        const headerHtml = filePath 
          ? `<div class="plan-content-header"><i class="fal fa-file-alt"></i> ${escapeHtml(filePath)}</div>`
          : "";
        
        div.innerHTML = headerHtml + '<div class="markdown-content">' + renderMarkdown(planContent) + '</div>';
        break;
      }

      case "system":
      default:
        div.className += " system";
        div.innerHTML = escapeHtml(content);
        break;
    }

    if (timestamp && type !== "system" && type !== "tool_call" && type !== "tool_call_message") {
      const timeDiv = document.createElement("div");
      timeDiv.className = "timestamp";
      timeDiv.textContent = timestamp;
      div.appendChild(timeDiv);
    }

    return div;
  }

  function copyToClipboard(text, button) {
    const finish = () => {
      if (!button) return;
      button.classList.add("copied");
      button.innerHTML = '<i class="fal fa-check"></i>';
      setTimeout(() => {
        button.classList.remove("copied");
        button.innerHTML = '<i class="fal fa-copy"></i>';
      }, 1200);
    };

    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard
        .writeText(text)
        .then(finish)
        .catch(() => {
          fallbackCopy(text, finish);
        });
      return;
    }

    fallbackCopy(text, finish);
  }

  function fallbackCopy(text, finish) {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.top = "-9999px";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    try {
      document.execCommand("copy");
      finish();
    } catch (err) {
      console.error("[ChatLog] Failed to copy message:", err);
    } finally {
      document.body.removeChild(textarea);
    }
  }

  // Add a system message
  function addSystemMessage(text) {
    addMessage({
      type: "system",
      content: text,
    });
  }

  // Clear all messages
  function clearMessages() {
    const container = $("#chatContainer");
    container.empty();
    container.append('<div class="chat-empty">No messages yet</div>');
    resetDiffCount();
  }

  // Scroll to bottom of chat
  function scrollToBottom() {
    const container = document.getElementById("chatContainer");
    if (container) {
      container.scrollTop = container.scrollHeight;
    }
  }

  // Format timestamp
  function formatTimestamp(ts) {
    if (!ts) return "";
    const date = new Date(ts);
    return date.toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  // Escape HTML to prevent XSS
  function escapeHtml(text) {
    if (!text) return "";
    const div = document.createElement("div");
    div.textContent = text;
    return div.innerHTML;
  }

  // Format tool arguments for display
  function formatArgs(args) {
    if (!args) return "";
    if (typeof args === "string") {
      try {
        const parsed = JSON.parse(args);
        return JSON.stringify(parsed, null, 2);
      } catch (e) {
        return args;
      }
    }
    return JSON.stringify(args, null, 2);
  }

  // Truncate long text
  function truncateText(text, maxLength) {
    if (!text || text.length <= maxLength) return text;
    return text.substring(0, maxLength) + "... (truncated)";
  }

  function renderAttachmentStrip() {
    const strip = document.getElementById("chatAttachmentStrip");
    if (!strip) return;
    strip.innerHTML = "";
    if (!pendingAttachments.length) {
      strip.classList.add("hidden");
      return;
    }
    strip.classList.remove("hidden");
    pendingAttachments.forEach((att) => {
      const wrapper = document.createElement("div");
      wrapper.className = "chat-attachment";
      wrapper.dataset.attachmentId = att.id;

      const img = document.createElement("img");
      img.src = att.dataUrl;
      img.alt = att.fileName || "attachment";
      img.title = att.fileName || "attachment";
      img.onclick = () => showImageLightbox(img.src, img.alt);

      const remove = document.createElement("div");
      remove.className = "chat-attachment-remove";
      remove.innerHTML = '<i class="fal fa-times"></i>';
      remove.title = "Remove image";
      remove.onclick = (e) => {
        e.preventDefault();
        e.stopPropagation();
        removePendingAttachment(att.id);
      };

      wrapper.appendChild(img);
      wrapper.appendChild(remove);
      strip.appendChild(wrapper);
    });
  }

  function clearPendingAttachments() {
    pendingAttachments = [];
    renderAttachmentStrip();
  }

  async function removePendingAttachment(attachmentId) {
    if (!ipcRenderer || !currentTaskId) {
      pendingAttachments = pendingAttachments.filter(
        (a) => a.id !== attachmentId,
      );
      renderAttachmentStrip();
      return;
    }
    try {
      await ipcRenderer.invoke(
        "delete_attachment",
        attachmentId,
        currentTaskId,
      );
      pendingAttachments = pendingAttachments.filter(
        (a) => a.id !== attachmentId,
      );
      renderAttachmentStrip();
    } catch (err) {
      console.error("[ChatLog] Failed to remove attachment:", err);
    }
  }

  async function processChatImageFile(file) {
    if (!file || !file.type || !file.type.startsWith("image/")) return;
    if (file.size > 5 * 1024 * 1024) {
      alert("Image exceeds 5MB size limit. Please use a smaller image.");
      return;
    }
    if (!ipcRenderer || !currentTaskId) {
      console.warn("[ChatLog] No task/IPC available for attachment");
      return;
    }
    const reader = new FileReader();
    reader.onload = async (e) => {
      const dataUrl = e.target.result;
      const base64 = dataUrl.split(",")[1];
      try {
        const attachment = await ipcRenderer.invoke("save_attachment", {
          taskId: currentTaskId,
          fileName: file.name || "image.png",
          mimeType: file.type,
          data: base64,
        });
        attachment.dataUrl = dataUrl;
        attachment.fileName = attachment.file_name || file.name || "image.png";
        attachment.mimeType = attachment.mime_type || file.type;
        pendingAttachments.push(attachment);
        renderAttachmentStrip();
      } catch (err) {
        console.error("[ChatLog] Failed to upload attachment:", err);
        alert("Failed to upload image: " + err);
      }
    };
    reader.readAsDataURL(file);
  }

  function stripLeadingBlankLines(text) {
    if (!text) return "";
    return text.replace(/^(?:[ \t]*\r?\n)+/, "");
  }

  function isUnifiedDiff(text) {
    if (!text) return false;
    return /(^|\n)diff --git /.test(text) || /(^|\n)@@ -\d+/.test(text);
  }

  function tryParseJson(text) {
    if (!text || typeof text !== "string") return null;
    const trimmed = text.trim();
    if (!trimmed) return null;
    if (trimmed[0] !== "{" && trimmed[0] !== "[") return null;
    try {
      return JSON.parse(trimmed);
    } catch (err) {
      return null;
    }
  }

  function normalizePermissionRequest({ toolName, description, rawInput }) {
    let rawText = "";
    if (typeof rawInput === "string") {
      rawText = rawInput.trim();
    } else if (rawInput != null) {
      try {
        rawText = JSON.stringify(rawInput, null, 2);
      } catch (err) {
        rawText = String(rawInput);
      }
      rawText = rawText.trim();
    }

    const parsed =
      typeof rawInput === "object" && rawInput !== null
        ? rawInput
        : tryParseJson(rawText);
    const payload =
      parsed && typeof parsed === "object" && !Array.isArray(parsed)
        ? parsed
        : null;

    const toolCall =
      payload?.toolCall ||
      payload?.tool_call ||
      (payload && payload.kind ? payload : null);
    const callData =
      toolCall && typeof toolCall === "object" ? toolCall : payload;

    const kind = (callData?.kind || "").toString();
    const title = callData?.title || payload?.title || description || "";
    const contentText = extractPermissionContentText(callData?.content);

    const command =
      callData?.cmd ||
      callData?.command ||
      payload?.cmd ||
      payload?.command ||
      payload?.shell ||
      payload?.args?.cmd ||
      payload?.parameters?.cmd ||
      payload?.input?.cmd ||
      payload?.input?.command ||
      "";

    const reason =
      payload?.justification ||
      payload?.reason ||
      payload?.description ||
      title ||
      description ||
      "";

    const amendment =
      payload?.proposed_amendment ||
      payload?.proposedAmendment ||
      payload?.amendment ||
      payload?.proposal ||
      "";

    const summary = payload?.summary || payload?.message || "";

    const looksLikeCommand = (value) =>
      typeof value === "string" &&
      /^(git|npm|pnpm|yarn|bun|cargo|python|node|rg|ls|cat|sed|awk|grep|find|mkdir|rm|mv|cp|chmod|chown)\b/i.test(
        value.trim(),
      );

    let displayCommand =
      command ||
      (looksLikeCommand(rawText) ? rawText : "") ||
      (looksLikeCommand(summary) ? summary : "");

    let actionLabel = formatPermissionActionLabel(
      toolName,
      displayCommand || summary || rawText,
    );

    if (kind === "switch_mode") {
      actionLabel = "Exit Plan Mode";
      displayCommand = displayCommand || title || "Switch mode";
    }

    const finalAmendment =
      amendment || (displayCommand && reason ? displayCommand : "");

    const displayReason =
      kind === "switch_mode" && contentText
        ? truncateText(contentText, 400)
        : reason;

    return {
      actionLabel,
      command: displayCommand || summary,
      reason: displayReason,
      amendment: finalAmendment,
      raw: rawText,
    };
  }

  function extractPermissionContentText(content) {
    if (!content) return "";
    if (typeof content === "string") return content.trim();
    if (Array.isArray(content)) {
      const parts = content
        .map((item) => {
          if (!item) return "";
          if (typeof item === "string") return item;
          if (item.text) return item.text;
          if (item.content) return item.content;
          return "";
        })
        .filter(Boolean);
      return parts.join("\n").trim();
    }
    if (typeof content === "object") {
      if (content.text) return content.text.toString().trim();
      if (content.content) return content.content.toString().trim();
    }
    return "";
  }

  function formatPermissionActionLabel(toolName, hint) {
    const raw = (toolName || "").toLowerCase();
    if (raw.includes("switch_mode") || raw.includes("switch mode")) {
      return "Exit Plan Mode";
    }
    if (
      raw.includes("command") ||
      raw.includes("exec") ||
      raw.includes("shell")
    ) {
      return "Run Command";
    }
    if (
      raw.includes("write") ||
      raw.includes("edit") ||
      raw.includes("patch")
    ) {
      return "Edit File";
    }
    if (raw.includes("read")) {
      return "Read File";
    }
    if (raw.includes("open") || raw.includes("browse")) {
      return "Open Resource";
    }
    if (raw.includes("deploy") || raw.includes("publish")) {
      return "Publish Changes";
    }
    if (hint && hint.length < 48) {
      return "Run " + hint;
    }
    return "Review Request";
  }

  function formatPermissionOptionLabel(option) {
    if (!option) return "Confirm";
    const kind = (option.kind || "").toLowerCase();
    if (kind === "allow_always") return "Always";
    if (kind === "allow_once") return "Yes";
    if (kind === "reject_once" || kind === "reject_always") {
      return "No, provide feedback";
    }
    const id = (option.id || "").toLowerCase();
    if (id === "auto_accept") return "Always";
    if (id === "manual") return "Yes";
    if (id === "deny") return "No, provide feedback";
    return option.shortLabel || option.label || "Confirm";
  }

  function isDenyPermissionOption(option) {
    const id = (option?.id || "").toLowerCase();
    const label = (option?.label || "").toLowerCase();
    const style = (option?.style || "").toLowerCase();
    const kind = (option?.kind || "").toLowerCase();
    return (
      kind.startsWith("reject") ||
      style === "danger" ||
      id.includes("deny") ||
      id.includes("reject") ||
      label.startsWith("no") ||
      label.includes("deny") ||
      label.includes("reject")
    );
  }

  function splitPermissionOptions(options) {
    const allow = [];
    const deny = [];
    (options || []).forEach((option) => {
      if (isDenyPermissionOption(option)) {
        deny.push(option);
      } else {
        allow.push(option);
      }
    });
    return { allow, deny };
  }

  function buildPermissionActions(options) {
    if (!options || options.length === 0) {
      return "";
    }

    const { allow, deny } = splitPermissionOptions(options);
    const useDropdown =
      allow.length > 1 || deny.length > 1 || options.length > 3;

    if (!useDropdown) {
      let buttons = "";
      options.forEach((opt) => {
        const shortLabel = formatPermissionOptionLabel(opt);
        const iconClass =
          opt.icon === "check-double"
            ? "fa-check-double"
            : opt.icon === "check"
              ? "fa-check"
              : opt.icon === "times"
                ? "fa-times"
                : "fa-question";
        const btnStyle = opt.style || "secondary";
        buttons += `
          <button class="permission-btn ${escapeHtml(btnStyle)}" data-response-id="${escapeHtml(opt.id)}">
            <span class="btn-content">
              <span class="btn-icon"><i class="fal ${iconClass}"></i></span>
              <span class="btn-label">${escapeHtml(shortLabel)}</span>
            </span>
            ${opt.shortcut ? `<span class="btn-shortcut">${escapeHtml(opt.shortcut)}</span>` : ""}
          </button>
        `;
      });

      return `<div class="permission-actions">${buttons}</div>`;
    }

    const allowDefault = allow[0] || options[0];
    const denyDefault = deny[0] || null;

    return `
      <div class="permission-actions permission-actions-dropdown">
        ${buildPermissionActionGroup("allow", allow, allowDefault, "primary")}
        ${denyDefault ? buildPermissionActionGroup("deny", deny, denyDefault, "danger") : ""}
      </div>
    `;
  }

  function buildPermissionActionGroup(kind, options, selected, style) {
    if (!selected) return "";
    const label = formatPermissionOptionLabel(selected);
    const menuItems = (options || [])
      .map((option) => {
        const iconClass =
          option.icon === "check-double"
            ? "fa-check-double"
            : option.icon === "check"
              ? "fa-check"
              : option.icon === "times"
                ? "fa-times"
                : "fa-question";
        return `
          <button class="permission-menu-item" data-response-id="${escapeHtml(option.id)}" data-label="${escapeHtml(
            formatPermissionOptionLabel(option),
          )}">
            <span class="btn-content">
              <span class="btn-icon"><i class="fal ${iconClass}"></i></span>
              <span class="btn-label">${escapeHtml(option.label || formatPermissionOptionLabel(option))}</span>
            </span>
            ${
              option.shortcut
                ? `<span class="btn-shortcut">${escapeHtml(option.shortcut)}</span>`
                : ""
            }
          </button>
        `;
      })
      .join("");

    return `
      <div class="permission-action-group" data-group="${escapeHtml(
        kind,
      )}" data-response-id="${escapeHtml(selected.id)}">
        <button class="permission-btn ${escapeHtml(
          style,
        )} permission-action-trigger" data-response-id="${escapeHtml(
          selected.id,
        )}">
          <span class="btn-content">
            <span class="btn-label">${escapeHtml(label)}</span>
          </span>
        </button>
        <button class="permission-btn permission-menu-toggle" type="button" aria-label="More options">
          <i class="fal fa-chevron-down"></i>
        </button>
        <div class="permission-menu">
          ${menuItems}
        </div>
      </div>
    `;
  }

  function collectTextBlocks(value, collector) {
    if (value == null) return collector;
    if (typeof value === "string") {
      collector.push(value);
      return collector;
    }
    if (Array.isArray(value)) {
      value.forEach((item) => collectTextBlocks(item, collector));
      return collector;
    }
    if (typeof value === "object") {
      const type = value.type || value.kind;
      if (type === "diff" && typeof value.diff === "string") {
        collector.push(value.diff);
        return collector;
      }
      if (type === "diff" && typeof value.text === "string") {
        collector.push(value.text);
        return collector;
      }
      const prioritizedKeys = [
        "diff",
        "patch",
        "unified_diff",
        "text",
        "content",
        "output",
        "result",
        "message",
        "raw_output",
        "rawOutput",
        "data",
      ];
      prioritizedKeys.forEach((key) => {
        if (value[key] !== undefined) {
          collectTextBlocks(value[key], collector);
        }
      });
    }
    return collector;
  }

  function sliceUnifiedDiff(text) {
    if (!text) return "";
    const diffStart = text.search(/(^|\n)diff --git /);
    if (diffStart !== -1) {
      return text.slice(diffStart).trim();
    }
    const hunkStart = text.search(/(^|\n)@@ -\d+/);
    if (hunkStart !== -1) {
      return text.slice(hunkStart).trim();
    }
    return text.trim();
  }

  function extractDiffBlocksFromText(text) {
    if (!text) return [];
    const diffBlocks = [];
    const fenceRegex = /```(?:diff|patch)?\n([\s\S]*?)```/g;
    let match;
    while ((match = fenceRegex.exec(text)) !== null) {
      const inner = (match[1] || "").trim();
      if (isUnifiedDiff(inner)) {
        diffBlocks.push(sliceUnifiedDiff(inner));
      }
    }
    if (diffBlocks.length === 0 && isUnifiedDiff(text)) {
      diffBlocks.push(sliceUnifiedDiff(text));
    }
    return diffBlocks;
  }

  function extractToolReturnDetails(rawValue) {
    let rawText = "";
    if (typeof rawValue === "string") {
      rawText = rawValue.trim();
    } else if (rawValue != null) {
      try {
        rawText = JSON.stringify(rawValue);
      } catch (err) {
        rawText = String(rawValue);
      }
      rawText = rawText.trim();
    }
    const parsed = tryParseJson(rawText);
    const textCandidates = parsed ? collectTextBlocks(parsed, []) : [];
    const uniqueCandidates = Array.from(
      new Set(
        textCandidates
          .map((entry) =>
            entry && entry.toString ? entry.toString().trim() : "",
          )
          .filter(Boolean),
      ),
    );

    const diffBlocks = [];
    const sources =
      uniqueCandidates.length > 0 ? uniqueCandidates : rawText ? [rawText] : [];
    sources.forEach((text) => {
      extractDiffBlocksFromText(text).forEach((diff) => diffBlocks.push(diff));
    });

    const uniqueDiffs = Array.from(
      new Set(diffBlocks.map((diff) => diff.trim()).filter(Boolean)),
    );
    if (uniqueDiffs.length > 0) {
      return { diffs: uniqueDiffs, text: null };
    }

    if (uniqueCandidates.length > 0) {
      return { diffs: [], text: uniqueCandidates.join("\n\n") };
    }

    return { diffs: [], text: rawText };
  }

  function extractDiffTitle(text, fallback) {
    const fileLines = text.match(/^diff --git a\/.*$/gm);
    if (fileLines && fileLines.length > 0) {
      const first = fileLines[0].split(" ");
      const path = first[2] ? first[2].replace(/^a\//, "") : "Diff";
      if (fileLines.length > 1) {
        return `${path} (+${fileLines.length - 1})`;
      }
      return path;
    }
    const plusLine = text.match(/^\+\+\+ b\/(.+)$/m);
    if (plusLine && plusLine[1]) {
      return plusLine[1];
    }
    return fallback || "Diff";
  }

  function parseUnifiedDiff(text) {
    const lines = text.split("\n");
    const parsed = [];
    let oldLine = 0;
    let newLine = 0;
    let inHunk = false;
    const hunkRegex = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

    for (const line of lines) {
      if (
        line.startsWith("diff --git") ||
        line.startsWith("index ") ||
        line.startsWith("--- ") ||
        line.startsWith("+++ ")
      ) {
        parsed.push({ type: "meta", oldLine: null, newLine: null, text: line });
        continue;
      }

      if (line.startsWith("@@")) {
        const match = hunkRegex.exec(line);
        if (match) {
          oldLine = Number(match[1]);
          newLine = Number(match[3]);
        }
        parsed.push({ type: "hunk", oldLine: null, newLine: null, text: line });
        inHunk = true;
        continue;
      }

      if (!inHunk) {
        if (line.trim()) {
          parsed.push({
            type: "meta",
            oldLine: null,
            newLine: null,
            text: line,
          });
        }
        continue;
      }

      if (line.startsWith("+")) {
        parsed.push({
          type: "add",
          oldLine: null,
          newLine: newLine,
          text: line.slice(1),
        });
        newLine += 1;
        continue;
      }

      if (line.startsWith("-")) {
        parsed.push({
          type: "del",
          oldLine: oldLine,
          newLine: null,
          text: line.slice(1),
        });
        oldLine += 1;
        continue;
      }

      if (line.startsWith(" ")) {
        parsed.push({
          type: "context",
          oldLine: oldLine,
          newLine: newLine,
          text: line.slice(1),
        });
        oldLine += 1;
        newLine += 1;
        continue;
      }

      if (line.startsWith("\\")) {
        parsed.push({ type: "meta", oldLine: null, newLine: null, text: line });
      }
    }

    return parsed;
  }

  const DIFFS_MODULE_URL =
    "https://cdn.jsdelivr.net/npm/@pierre/diffs@1.0.8/+esm";
  const PHANTOM_DIFFS_CSS = `
    :host {
      --diffs-font-family: "Roboto Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
      --diffs-header-font-family: "Proxima Nova", system-ui, sans-serif;
      --diffs-font-size: 12px;
      --diffs-line-height: 1.5;
      --diffs-bg: rgba(16, 14, 20, 0.92);
      --diffs-bg-context: rgba(255, 255, 255, 0.03);
      --diffs-bg-separator: rgba(255, 255, 255, 0.08);
      --diffs-bg-hover: rgba(255, 255, 255, 0.06);
      --diffs-bg-addition: rgba(65, 181, 136, 0.18);
      --diffs-bg-addition-hover: rgba(65, 181, 136, 0.28);
      --diffs-bg-deletion: rgba(255, 105, 105, 0.2);
      --diffs-bg-deletion-hover: rgba(255, 105, 105, 0.3);
      --diffs-fg: rgba(255, 255, 255, 0.9);
      --diffs-fg-number: rgba(255, 255, 255, 0.35);
      --diffs-selection-number-fg: #f78a97;
      --diffs-bg-selection: rgba(247, 138, 151, 0.18);
      --diffs-tab-size: 2;
      --diffs-min-number-column-width: 36px;
    }
  `;
  let diffsModulePromise;

  function loadDiffsModule() {
    if (!diffsModulePromise) {
      diffsModulePromise = import(DIFFS_MODULE_URL);
    }
    return diffsModulePromise;
  }

  function countDiffFiles(diffText) {
    const matches = diffText.match(/^diff --git /gm);
    return matches ? matches.length : 0;
  }

  function getDiffFileStats(fileDiff) {
    let additions = 0;
    let deletions = 0;
    (fileDiff.hunks || []).forEach((hunk) => {
      additions += hunk.additionLines ?? hunk.additionCount ?? 0;
      deletions += hunk.deletionLines ?? hunk.deletionCount ?? 0;
    });
    return { additions, deletions };
  }

  function buildDiffFileHeader(fileDiff) {
    const header = document.createElement("div");
    header.className = "diff-file-header";

    const left = document.createElement("div");
    left.className = "diff-file-title";

    const fileName = document.createElement("span");
    fileName.className = "diff-file-name";
    fileName.textContent = fileDiff.name || "Untitled file";
    left.appendChild(fileName);

    if (fileDiff.prevName && fileDiff.prevName !== fileDiff.name) {
      const rename = document.createElement("span");
      rename.className = "diff-file-rename";
      rename.textContent = `← ${fileDiff.prevName}`;
      left.appendChild(rename);
    }

    const right = document.createElement("div");
    right.className = "diff-file-meta";

    const type = document.createElement("span");
    type.className = `diff-file-type diff-file-type-${fileDiff.type || "change"}`;
    type.textContent =
      fileDiff.type === "new"
        ? "New"
        : fileDiff.type === "deleted"
          ? "Deleted"
          : fileDiff.type === "rename-pure" ||
              fileDiff.type === "rename-changed"
            ? "Renamed"
            : "Modified";

    const stats = getDiffFileStats(fileDiff);
    const summary = document.createElement("span");
    summary.className = "diff-file-stats";
    summary.innerHTML = `<span class="diff-add">+${stats.additions}</span><span class="diff-del">-${stats.deletions}</span>`;

    right.appendChild(type);
    right.appendChild(summary);

    header.appendChild(left);
    header.appendChild(right);

    return header;
  }

  function buildDiffToggle(style) {
    const toggle = document.createElement("div");
    toggle.className = "diff-toggle";
    toggle.innerHTML = `
      <button class="diff-toggle-btn" data-style="unified">Unified</button>
      <button class="diff-toggle-btn" data-style="split">Split</button>
    `;

    const buttons = toggle.querySelectorAll(".diff-toggle-btn");
    buttons.forEach((btn) => {
      btn.classList.toggle("is-active", btn.dataset.style === style);
    });

    return toggle;
  }

  function setDiffStyle(card, style) {
    card.dataset.diffStyle = style;
    const buttons = card.querySelectorAll(".diff-toggle-btn");
    buttons.forEach((btn) => {
      btn.classList.toggle("is-active", btn.dataset.style === style);
    });
    const instances = card._diffInstances || [];
    instances.forEach((instance) => {
      instance.setOptions({ diffStyle: style });
      instance.rerender();
    });
  }

  function renderDiffsWithLibrary(card, diffText) {
    const body = card.querySelector(".diff-body");
    if (!body) return;

    loadDiffsModule()
      .then((diffs) => {
        const { FileDiff, parsePatchFiles } = diffs;
        const patches = parsePatchFiles(diffText);
        const files = patches.flatMap((patch) => patch.files || []);
        if (!files.length) {
          throw new Error("No diff files parsed");
        }

        const currentStyle = card.dataset.diffStyle || "unified";
        body.innerHTML = "";
        card._diffInstances = [];

        files.forEach((fileDiff) => {
          const fileWrap = document.createElement("div");
          fileWrap.className = "diff-file";

          const fileHeader = buildDiffFileHeader(fileDiff);
          const fileContainer = document.createElement("div");
          fileContainer.className = "diff-file-render";

          fileWrap.appendChild(fileHeader);
          fileWrap.appendChild(fileContainer);
          body.appendChild(fileWrap);

          const instance = new FileDiff({
            diffStyle: currentStyle,
            lineDiffType: "word",
            diffIndicators: "bars",
            theme: "pierre-dark",
            themeType: "dark",
            overflow: "wrap",
            disableFileHeader: true,
            unsafeCSS: PHANTOM_DIFFS_CSS,
          });

          instance.render({ fileDiff, fileContainer });
          card._diffInstances.push(instance);
        });
      })
      .catch((err) => {
        console.error("[ChatLog] Failed to render diff with library:", err);
        // Render legacy diff content directly into the body
        renderLegacyDiffContent(body, diffText);
      });
  }

  function renderLegacyDiffBlock(diffText, title) {
    const parsed = parseUnifiedDiff(diffText);
    let additions = 0;
    let deletions = 0;
    parsed.forEach((line) => {
      if (line.type === "add") additions += 1;
      if (line.type === "del") deletions += 1;
    });

    const card = document.createElement("div");
    card.className = "diff-card";

    const header = document.createElement("div");
    header.className = "diff-card-header";

    const titleWrap = document.createElement("div");
    titleWrap.className = "diff-title";

    const chip = document.createElement("span");
    chip.className = "diff-chip";
    chip.textContent = "Diff";

    const titleText = document.createElement("span");
    titleText.className = "diff-title-text";
    titleText.textContent = extractDiffTitle(diffText, title);

    titleWrap.appendChild(chip);
    titleWrap.appendChild(titleText);

    const summary = document.createElement("div");
    summary.className = "diff-summary";
    summary.innerHTML = `
      <span class="diff-add">+${additions}</span>
      <span class="diff-del">-${deletions}</span>
    `;

    header.appendChild(titleWrap);
    header.appendChild(summary);

    const output = document.createElement("div");
    output.className = "diff-output";

    parsed.forEach((line) => {
      const row = document.createElement("div");
      row.className = `diff-line diff-line-${line.type}`;

      const gutter = document.createElement("div");
      gutter.className = "diff-gutter";

      const oldLine = document.createElement("span");
      oldLine.className = "diff-line-number";
      oldLine.textContent = line.oldLine !== null ? line.oldLine : "";

      const newLine = document.createElement("span");
      newLine.className = "diff-line-number";
      newLine.textContent = line.newLine !== null ? line.newLine : "";

      gutter.appendChild(oldLine);
      gutter.appendChild(newLine);

      const content = document.createElement("div");
      content.className = "diff-line-content";
      content.textContent = line.text;

      row.appendChild(gutter);
      row.appendChild(content);
      output.appendChild(row);
    });

    card.appendChild(header);
    card.appendChild(output);

    return card;
  }

  // Render legacy diff content into an existing container element
  function renderLegacyDiffContent(container, diffText) {
    const parsed = parseUnifiedDiff(diffText);
    container.innerHTML = "";

    const output = document.createElement("div");
    output.className = "diff-output";

    parsed.forEach((line) => {
      const row = document.createElement("div");
      row.className = `diff-line diff-line-${line.type}`;

      const gutter = document.createElement("div");
      gutter.className = "diff-gutter";

      const oldLine = document.createElement("span");
      oldLine.className = "diff-line-number";
      oldLine.textContent = line.oldLine !== null ? line.oldLine : "";

      const newLine = document.createElement("span");
      newLine.className = "diff-line-number";
      newLine.textContent = line.newLine !== null ? line.newLine : "";

      gutter.appendChild(oldLine);
      gutter.appendChild(newLine);

      const content = document.createElement("div");
      content.className = "diff-line-content";
      content.textContent = line.text;

      row.appendChild(gutter);
      row.appendChild(content);
      output.appendChild(row);
    });

    container.appendChild(output);
  }

  function renderDiffBlock(diffText, title) {
    const parsed = parseUnifiedDiff(diffText);
    let additions = 0;
    let deletions = 0;
    parsed.forEach((line) => {
      if (line.type === "add") additions += 1;
      if (line.type === "del") deletions += 1;
    });

    // Update header diff counter
    addDiffLines(additions, deletions);

    const card = document.createElement("div");
    card.className = "diff-card";

    // Header
    const header = document.createElement("div");
    header.className = "diff-card-header";

    const titleWrap = document.createElement("div");
    titleWrap.className = "diff-title";

    const chip = document.createElement("span");
    chip.className = "diff-chip";
    chip.textContent = "DIFF";

    const titleText = document.createElement("span");
    titleText.className = "diff-title-text";
    titleText.textContent = extractDiffTitle(diffText, title);

    titleWrap.appendChild(chip);
    titleWrap.appendChild(titleText);

    const summary = document.createElement("div");
    summary.className = "diff-summary";
    summary.innerHTML = `
      <span class="diff-add">+${additions}</span>
      <span class="diff-del">-${deletions}</span>
    `;

    header.appendChild(titleWrap);
    header.appendChild(summary);

    // Diff output (unified view only)
    const output = document.createElement("div");
    output.className = "diff-output";

    parsed.forEach((line) => {
      const row = document.createElement("div");
      row.className = `diff-line diff-line-${line.type}`;

      const gutter = document.createElement("div");
      gutter.className = "diff-gutter";

      const oldLineNum = document.createElement("span");
      oldLineNum.className = "diff-line-number";
      oldLineNum.textContent = line.oldLine !== null ? line.oldLine : "";

      const newLineNum = document.createElement("span");
      newLineNum.className = "diff-line-number";
      newLineNum.textContent = line.newLine !== null ? line.newLine : "";

      gutter.appendChild(oldLineNum);
      gutter.appendChild(newLineNum);

      const content = document.createElement("div");
      content.className = "diff-line-content";
      content.textContent = line.text;

      row.appendChild(gutter);
      row.appendChild(content);
      output.appendChild(row);
    });

    card.appendChild(header);
    card.appendChild(output);

    return card;
  }

  // Mock conversation for UI testing
  const MOCK_CONVERSATION = [
    { type: 'user', content: 'Help me fix the authentication bug in the login flow' },
    { type: 'reasoning', reasoning: 'Let me analyze the authentication module. I should first read the auth controller to understand the current implementation, then check the session handling logic...' },
    { type: 'tool_call', name: 'Read', arguments: '{"file_path": "src/controllers/auth.ts"}' },
    { type: 'tool_return', tool_return: 'export class AuthController {\n  async login(req, res) {\n    const { email, password } = req.body;\n    // TODO: Add proper validation\n    const user = await User.findByEmail(email);\n    if (!user) {\n      return res.status(401).json({ error: "Invalid credentials" });\n    }\n    const token = jwt.sign({ userId: user.id }, SECRET);\n    res.json({ token });\n  }\n}' },
    { type: 'assistant', content: '**I found the issue!** The login flow has a security vulnerability:\n\n1. The password is never actually verified against the stored hash\n2. There\'s no rate limiting on login attempts\n\nHere\'s the fix:\n\n```typescript\nasync login(req, res) {\n  const { email, password } = req.body;\n  const user = await User.findByEmail(email);\n  \n  if (!user || !await bcrypt.compare(password, user.passwordHash)) {\n    return res.status(401).json({ error: "Invalid credentials" });\n  }\n  \n  const token = jwt.sign({ userId: user.id }, SECRET);\n  res.json({ token });\n}\n```\n\nWould you like me to implement this fix?' },
    { type: 'tool_call', name: 'Edit', arguments: '{"file_path": "src/controllers/auth.ts", "old_string": "const user = await User.findByEmail(email);", "new_string": "const user = await User.findByEmail(email);\\n    if (!user || !await bcrypt.compare(password, user.passwordHash)) {"}' },
    { type: 'diff', title: 'src/controllers/auth.ts', diff: '--- a/src/controllers/auth.ts\n+++ b/src/controllers/auth.ts\n@@ -3,8 +3,11 @@ export class AuthController {\n   async login(req, res) {\n     const { email, password } = req.body;\n     const user = await User.findByEmail(email);\n-    if (!user) {\n+    \n+    if (!user || !await bcrypt.compare(password, user.passwordHash)) {\n       return res.status(401).json({ error: "Invalid credentials" });\n     }\n+    \n     const token = jwt.sign({ userId: user.id }, SECRET);\n     res.json({ token });\n   }' },
    { type: 'tool_call', name: 'Bash', arguments: '{"command": "rg -n \\"findByEmail\\" src/"}' },
    { type: 'tool_return', tool_return: 'src/controllers/auth.ts:5:    const user = await User.findByEmail(email);\nsrc/models/user.ts:23:  static async findByEmail(email: string)' },
    { type: 'reasoning', reasoning: 'Good, I can see the User model has the findByEmail method. Now I need to check if bcrypt is already a dependency...' },
    { type: 'assistant', content: 'I see `findByEmail` is used in **2 places**. Let me also check if `bcrypt` is already installed as a dependency.' },
  ];

  // Mock todo list states that progress during the conversation
  const MOCK_TODO_STATES = [
    {
      explanation: 'Fixing authentication bug in login flow',
      plan: [
        { step: 'Read auth controller code', status: 'in-progress' },
        { step: 'Identify security vulnerabilities', status: 'pending' },
        { step: 'Fix password verification', status: 'pending' },
        { step: 'Check bcrypt dependency', status: 'pending' },
        { step: 'Run tests', status: 'pending' },
      ]
    },
    {
      explanation: 'Fixing authentication bug in login flow',
      plan: [
        { step: 'Read auth controller code', status: 'completed' },
        { step: 'Identify security vulnerabilities', status: 'in-progress' },
        { step: 'Fix password verification', status: 'pending' },
        { step: 'Check bcrypt dependency', status: 'pending' },
        { step: 'Run tests', status: 'pending' },
      ]
    },
    {
      explanation: 'Fixing authentication bug in login flow',
      plan: [
        { step: 'Read auth controller code', status: 'completed' },
        { step: 'Identify security vulnerabilities', status: 'completed' },
        { step: 'Fix password verification', status: 'in-progress' },
        { step: 'Check bcrypt dependency', status: 'pending' },
        { step: 'Run tests', status: 'pending' },
      ]
    },
    {
      explanation: 'Fixing authentication bug in login flow',
      plan: [
        { step: 'Read auth controller code', status: 'completed' },
        { step: 'Identify security vulnerabilities', status: 'completed' },
        { step: 'Fix password verification', status: 'completed' },
        { step: 'Check bcrypt dependency', status: 'in-progress' },
        { step: 'Run tests', status: 'pending' },
      ]
    },
  ];

  function loadMockConversation() {
    clearMessages();
    resetTodoSidebar();
    console.log('[ChatLog] Loading mock conversation...');

    // Simulate streaming with delays
    let index = 0;
    let todoIndex = 0;
    function addNext() {
      if (index < MOCK_CONVERSATION.length) {
        addMessage(MOCK_CONVERSATION[index]);

        // Update todo sidebar at certain points in the conversation
        if (index === 0 && todoIndex < MOCK_TODO_STATES.length) {
          updateTodoSidebar(MOCK_TODO_STATES[todoIndex++]);
        } else if (index === 3 && todoIndex < MOCK_TODO_STATES.length) {
          updateTodoSidebar(MOCK_TODO_STATES[todoIndex++]);
        } else if (index === 4 && todoIndex < MOCK_TODO_STATES.length) {
          updateTodoSidebar(MOCK_TODO_STATES[todoIndex++]);
        } else if (index === 6 && todoIndex < MOCK_TODO_STATES.length) {
          updateTodoSidebar(MOCK_TODO_STATES[todoIndex++]);
        }

        index++;
        setTimeout(addNext, 300);
      }
    }
    addNext();
  }

  // Keyboard shortcut: Ctrl+M to load mock data
  document.addEventListener('keydown', function(e) {
    if (e.ctrlKey && e.key === 'm') {
      e.preventDefault();
      loadMockConversation();
    }
  });

  // Expose for console testing
  window.loadMockConversation = loadMockConversation;

  // Initialize when DOM is ready
  $(document).ready(init);
})();
