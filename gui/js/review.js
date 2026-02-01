(function () {
  const bridge = window.tauriBridge;
  const ipcRenderer = bridge ? bridge.ipcRenderer : null;

  const state = {
    initialized: false,
    eventsBound: false,
    tasks: [],
    selectedTaskId: null,
    selectedFilePath: null,
    compareMode: "main",
    viewMode: "split",
  };

  // Custom dropdown instance for task selector
  let reviewTaskDropdown = null;

  function $(id) {
    return document.getElementById(id);
  }

  function escapeHtml(text) {
    const div = document.createElement("div");
    div.textContent = text || "";
    return div.innerHTML;
  }

  function setActiveButton(container, selectorOrEl) {
    if (!container) return;
    container.querySelectorAll(".active").forEach((el) => {
      el.classList.remove("active");
    });
    const el =
      typeof selectorOrEl === "string"
        ? container.querySelector(selectorOrEl)
        : selectorOrEl;
    if (el) el.classList.add("active");
  }

  function setDisabled(id, disabled) {
    const el = $(id);
    if (!el) return;
    el.disabled = !!disabled;
  }

  function renderFiles(files) {
    const list = $("reviewFileList");
    const count = $("reviewFileCount");
    const empty = $("reviewFilesEmpty");

    if (!list || !count) return;

    list.innerHTML = "";

    const safeFiles = Array.isArray(files) ? files : [];
    count.textContent = String(safeFiles.length);

    if (!safeFiles.length) {
      if (empty) {
        empty.textContent = state.selectedTaskId
          ? "No changed files found (diff wiring TODO)."
          : "Select a task to load diffs.";
        list.appendChild(empty);
      } else {
        list.innerHTML =
          '<div class="review-empty">Select a task to load diffs.</div>';
      }
      return;
    }

    safeFiles.forEach((file) => {
      const item = document.createElement("div");
      item.className = "review-file-item";
      item.dataset.filePath = file.path;
      if (file.path === state.selectedFilePath) item.classList.add("selected");

      const icon = document.createElement("i");
      icon.className = "fal fa-file-code review-file-icon";

      const name = document.createElement("span");
      name.className = "review-file-name";
      name.textContent = file.path;
      name.title = file.path;

      const stats = document.createElement("span");
      stats.className = "review-file-stats";

      const add = document.createElement("span");
      add.className = "review-stat-add";
      add.textContent = `+${file.additions || 0}`;

      const del = document.createElement("span");
      del.className = "review-stat-del";
      del.textContent = `-${file.deletions || 0}`;

      stats.appendChild(add);
      stats.appendChild(del);

      item.appendChild(icon);
      item.appendChild(name);
      item.appendChild(stats);

      item.addEventListener("click", () => {
        state.selectedFilePath = file.path;
        list
          .querySelectorAll(".review-file-item")
          .forEach((el) => el.classList.remove("selected"));
        item.classList.add("selected");
        setDisabled("reviewCommentBtn", false);
        setDisabled("reviewApproveBtn", false);
        refreshDiff();
      });

      list.appendChild(item);
    });
  }

  function renderDiffPlaceholder() {
    const path = $("reviewDiffPath");
    const body = $("reviewDiffBody");
    if (path) {
      path.innerHTML =
        '<i class="fal fa-file-code"></i><span>Select a file</span>';
    }
    if (body) {
      body.innerHTML = '<div class="review-diff-placeholder">No diff loaded.</div>';
    }
    setDisabled("reviewCommentBtn", true);
    setDisabled("reviewApproveBtn", true);
  }

  function renderSplitDiff(diff) {
    const body = $("reviewDiffBody");
    if (!body) return;

    const leftLines = Array.isArray(diff?.left) ? diff.left : [];
    const rightLines = Array.isArray(diff?.right) ? diff.right : [];

    const container = document.createElement("div");
    container.className = "review-split-view";

    const left = document.createElement("div");
    left.className = "review-split-pane";
    left.innerHTML =
      '<div class="review-split-header old"><i class="fal fa-minus-circle"></i> base</div>';

    const leftContent = document.createElement("div");
    leftContent.className = "review-split-content";
    leftLines.forEach((line) => {
      const row = document.createElement("div");
      row.className = `review-split-line ${line.type || ""}`.trim();
      row.innerHTML = `<span class="review-split-num">${
        line.number || ""
      }</span><span class="review-split-text">${escapeHtml(
        line.text || ""
      )}</span>`;
      leftContent.appendChild(row);
    });
    left.appendChild(leftContent);

    const right = document.createElement("div");
    right.className = "review-split-pane";
    right.innerHTML =
      '<div class="review-split-header new"><i class="fal fa-plus-circle"></i> current</div>';

    const rightContent = document.createElement("div");
    rightContent.className = "review-split-content";
    rightLines.forEach((line) => {
      const row = document.createElement("div");
      row.className = `review-split-line ${line.type || ""}`.trim();
      row.innerHTML = `<span class="review-split-num">${
        line.number || ""
      }</span><span class="review-split-text">${escapeHtml(
        line.text || ""
      )}</span>`;
      rightContent.appendChild(row);
    });
    right.appendChild(rightContent);

    container.appendChild(left);
    container.appendChild(right);

    body.innerHTML = "";
    body.appendChild(container);
  }

  function renderUnifiedDiff(diffText) {
    const body = $("reviewDiffBody");
    if (!body) return;

    const wrap = document.createElement("div");
    wrap.className = "review-unified-view";
    wrap.innerHTML = `<pre>${escapeHtml(diffText || "")}</pre>`;
    body.innerHTML = "";
    body.appendChild(wrap);
  }

  function mockFiles() {
    return [
      { path: "src-tauri/src/main.rs", additions: 12, deletions: 3 },
      { path: "gui/menu.html", additions: 44, deletions: 0 },
      { path: "gui/js/review.js", additions: 120, deletions: 0 },
    ];
  }

  function mockDiff() {
    return {
      unified:
        "diff --git a/src-tauri/src/main.rs b/src-tauri/src/main.rs\n" +
        "index 0000000..1111111 100644\n" +
        "--- a/src-tauri/src/main.rs\n" +
        "+++ b/src-tauri/src/main.rs\n" +
        "@@ -1,3 +1,7 @@\n" +
        "+// TODO(review): wire actual git diff rendering\n" +
        "+// Placeholder diff so the UI can be exercised\n" +
        " fn main() {\n" +
        "   println!(\"hello\");\n" +
        "+  println!(\"review center\");\n" +
        " }\n",
      split: {
        left: [
          { number: 1, text: "fn main() {", type: "" },
          { number: 2, text: "  println!(\"hello\");", type: "" },
          { number: 3, text: "}", type: "" },
        ],
        right: [
          { number: 1, text: "// TODO(review): wire actual git diff rendering", type: "add" },
          { number: 2, text: "fn main() {", type: "" },
          { number: 3, text: "  println!(\"hello\");", type: "" },
          { number: 4, text: "  println!(\"review center\");", type: "add" },
          { number: 5, text: "}", type: "" },
        ],
      },
    };
  }

  async function loadTasksIntoSelector() {
    const container = $("reviewTaskSelector");
    if (!container) return;

    // Initialize dropdown if not already done
    if (!reviewTaskDropdown && window.CustomDropdown) {
      reviewTaskDropdown = new window.CustomDropdown({
        container: container,
        items: [{ value: "", name: "Select a task...", description: "" }],
        placeholder: "Select a task...",
        defaultValue: "",
        onChange: async function (value) {
          state.selectedTaskId = value || null;
          state.selectedFilePath = null;
          await refreshFiles();
        },
      });
    }

    if (!ipcRenderer) return;

    try {
      const tasks = await ipcRenderer.invoke("loadTasks");
      state.tasks = Array.isArray(tasks) ? tasks : [];

      const items = [{ value: "", name: "Select a task...", description: "" }];

      state.tasks.forEach((task) => {
        const id = task.id;
        const branch = task.branch || task.branch_name || "";
        const prompt = task.prompt || "";
        const truncatedPrompt =
          prompt.length > 50 ? prompt.substring(0, 50) + "..." : prompt;
        items.push({
          value: id,
          name: branch ? `#${id} — ${branch}` : `Task #${id}`,
          description: truncatedPrompt,
        });
      });

      if (reviewTaskDropdown) {
        reviewTaskDropdown.setOptions(items);
      }
    } catch (err) {
      console.warn("[Review] loadTasks failed:", err);
    }
  }

  function findTask(taskId) {
    if (!taskId) return null;
    return state.tasks.find((t) => t.id === taskId) || null;
  }

  async function refreshFiles() {
    const empty = $("reviewFilesEmpty");
    if (!state.selectedTaskId) {
      renderFiles([]);
      renderDiffPlaceholder();
      setDisabled("reviewOpenWorktreeBtn", true);
      if (empty) empty.textContent = "Select a task to load diffs.";
      return;
    }

    const task = findTask(state.selectedTaskId);
    const path = task?.worktree_path || task?.worktreePath || task?.project_path || task?.projectPath;
    setDisabled("reviewOpenWorktreeBtn", !path);

    if (!ipcRenderer) {
      renderFiles(mockFiles());
      return;
    }

    try {
      const result = await ipcRenderer.invoke("getTaskDiffFiles", {
        taskId: state.selectedTaskId,
        compare: state.compareMode,
      });
      const files = Array.isArray(result?.files) ? result.files : [];
      renderFiles(files.length ? files : mockFiles());
    } catch (err) {
      console.warn("[Review] getTaskDiffFiles failed:", err);
      renderFiles(mockFiles());
    }

    state.selectedFilePath = null;
    renderDiffPlaceholder();
  }

  async function refreshDiff() {
    const filePath = state.selectedFilePath;
    if (!state.selectedTaskId || !filePath) {
      renderDiffPlaceholder();
      return;
    }

    const path = $("reviewDiffPath");
    if (path) {
      path.innerHTML = `<i class="fal fa-file-code"></i><span>${escapeHtml(
        filePath
      )}</span>`;
    }

    if (!ipcRenderer) {
      const fallback = mockDiff();
      if (state.viewMode === "unified") {
        renderUnifiedDiff(fallback.unified);
      } else {
        renderSplitDiff(fallback.split);
      }
      return;
    }

    try {
      const result = await ipcRenderer.invoke("getTaskFileDiff", {
        taskId: state.selectedTaskId,
        filePath: filePath,
        compare: state.compareMode,
        view: state.viewMode,
      });

      if (state.viewMode === "unified") {
        renderUnifiedDiff(result?.diff || mockDiff().unified);
      } else {
        renderSplitDiff(result?.diff || mockDiff().split);
      }
    } catch (err) {
      console.warn("[Review] getTaskFileDiff failed:", err);
      const fallback = mockDiff();
      if (state.viewMode === "unified") {
        renderUnifiedDiff(fallback.unified);
      } else {
        renderSplitDiff(fallback.split);
      }
    }
  }

  function openSelectedWorktree() {
    if (!ipcRenderer || !state.selectedTaskId) return;

    const task = findTask(state.selectedTaskId);
    const path =
      task?.worktree_path ||
      task?.worktreePath ||
      task?.project_path ||
      task?.projectPath ||
      null;
    if (!path) return;

    // Follow existing bridge pattern (send, not invoke).
    ipcRenderer.send("OpenTaskDirectory", path, null);
  }

  function bindEvents() {
    if (state.eventsBound) return;
    state.eventsBound = true;

    // Note: Task selector events are handled by CustomDropdown's onChange callback

    const compareToggle = $("reviewCompareToggle");
    if (compareToggle) {
      compareToggle.addEventListener("click", async (e) => {
        const btn = e.target.closest(".review-toggle-btn");
        if (!btn) return;
        const mode = btn.dataset.compare;
        if (!mode || mode === state.compareMode) return;
        state.compareMode = mode;
        setActiveButton(compareToggle, btn);
        await refreshFiles();
      });
    }

    const unifiedBtn = $("reviewUnifiedBtn");
    const splitBtn = $("reviewSplitBtn");

    function setViewMode(next) {
      state.viewMode = next;
      if (next === "unified") {
        unifiedBtn?.classList.add("active");
        splitBtn?.classList.remove("active");
      } else {
        splitBtn?.classList.add("active");
        unifiedBtn?.classList.remove("active");
      }
      refreshDiff();
    }

    unifiedBtn?.addEventListener("click", () => setViewMode("unified"));
    splitBtn?.addEventListener("click", () => setViewMode("split"));

    $("reviewRefreshBtn")?.addEventListener("click", async () => {
      await loadTasksIntoSelector();
      await refreshFiles();
    });

    $("reviewOpenWorktreeBtn")?.addEventListener("click", openSelectedWorktree);

    // Placeholder actions
    $("reviewCommentBtn")?.addEventListener("click", () => {
      console.log("[Review] Comment clicked (TODO)");
    });

    $("reviewApproveBtn")?.addEventListener("click", () => {
      console.log("[Review] Approve clicked (TODO)");
    });

    window.addEventListener("phantom:navigate", (event) => {
      const pageId = event?.detail?.pageId;
      if (pageId !== "reviewPage") return;
      initReviewCenter();
    });
  }

  async function initReviewCenter() {
    if (state.initialized) return;
    state.initialized = true;

    await loadTasksIntoSelector();
    renderFiles([]);
    renderDiffPlaceholder();
  }

  // Boot: bind events early so navigation works, but only initialize once.
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => {
      bindEvents();
    });
  } else {
    bindEvents();
  }

  window.initReviewCenter = initReviewCenter;
})();
