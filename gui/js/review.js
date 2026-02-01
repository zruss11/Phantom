(function () {
  const bridge = window.tauriBridge;
  const ipcRenderer = bridge ? bridge.ipcRenderer : null;

  const state = {
    initialized: false,
    eventsBound: false,
    tasks: [],
    projects: [],
    selectedProjectPath: null,
    selectedTaskId: null,
    selectedFilePath: null,
    compareMode: "main",
    viewMode: "split",
    commits: [],
    visibilityObserver: null,
  };

  // Custom dropdown instances
  let reviewProjectDropdown = null;
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
          ? "No changed files found."
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

    if (!leftLines.length && !rightLines.length) {
      body.innerHTML = '<div class="review-diff-placeholder">No changes in this file.</div>';
      return;
    }

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

    if (!diffText || !diffText.trim()) {
      body.innerHTML = '<div class="review-diff-placeholder">No changes in this file.</div>';
      return;
    }

    const wrap = document.createElement("div");
    wrap.className = "review-unified-view";
    wrap.innerHTML = `<pre>${escapeHtml(diffText || "")}</pre>`;
    body.innerHTML = "";
    body.appendChild(wrap);
  }

  function renderCommitTimeline(timeline) {
    const track = $("reviewTimelineTrack");
    const meta = $("reviewTimelineMeta");

    if (!track) return;

    const commits = Array.isArray(timeline?.commits) ? timeline.commits : [];
    state.commits = commits;

    // Update meta text
    if (meta) {
      const base = timeline?.base_branch || "main";
      const current = timeline?.current_branch || "HEAD";
      meta.textContent = `${commits.length} commit${commits.length !== 1 ? "s" : ""} · ${current} vs ${base}`;
    }

    track.innerHTML = "";

    if (!commits.length) {
      track.innerHTML = '<div class="review-timeline-empty">No commits found since base branch.</div>';
      return;
    }

    commits.forEach((commit) => {
      const item = document.createElement("div");
      item.className = "review-timeline-item";

      const dot = document.createElement("div");
      dot.className = "review-timeline-dot";

      const content = document.createElement("div");
      content.className = "review-timeline-content";

      const row = document.createElement("div");
      row.className = "review-commit-row";

      const hash = document.createElement("span");
      hash.className = "review-commit-hash";
      hash.textContent = commit.hash || "";

      const subject = document.createElement("span");
      subject.className = "review-commit-subject";
      subject.textContent = commit.subject || "";
      subject.title = commit.subject || "";

      row.appendChild(hash);
      row.appendChild(subject);

      const commitMeta = document.createElement("div");
      commitMeta.className = "review-commit-meta";
      commitMeta.innerHTML = `<span class="review-commit-author">${escapeHtml(commit.author || "")}</span> · ${escapeHtml(commit.time_ago || "")}`;

      content.appendChild(row);
      content.appendChild(commitMeta);

      item.appendChild(dot);
      item.appendChild(content);

      track.appendChild(item);
    });
  }

  async function loadProjectsIntoSelector() {
    const container = $("reviewProjectSelector");
    if (!container) return;

    // Initialize dropdown if not already done
    if (!reviewProjectDropdown && window.CustomDropdown) {
      reviewProjectDropdown = new window.CustomDropdown({
        container: container,
        items: [{ value: "", name: "All projects", description: "" }],
        placeholder: "All projects",
        defaultValue: "",
        onChange: async function (value) {
          state.selectedProjectPath = value || null;
          state.selectedTaskId = null;
          state.selectedFilePath = null;
          await loadTasksIntoSelector();
          renderFiles([]);
          renderDiffPlaceholder();
          renderCommitTimeline({ commits: [] });
        },
      });
    }

    if (!ipcRenderer) return;

    try {
      const result = await ipcRenderer.invoke("getReviewProjects");
      state.projects = Array.isArray(result?.projects) ? result.projects : [];

      const items = [{ value: "", name: "All projects", description: "" }];

      state.projects.forEach((project) => {
        items.push({
          value: project.path,
          name: project.name || project.path,
          description: `${project.task_count} task${project.task_count !== 1 ? "s" : ""}`,
        });
      });

      if (reviewProjectDropdown) {
        reviewProjectDropdown.setOptions(items);
      }
    } catch (err) {
      console.warn("[Review] getReviewProjects failed:", err);
    }
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
          await refreshCommitTimeline();
        },
      });
    }

    if (!ipcRenderer) return;

    try {
      const tasks = await ipcRenderer.invoke("loadTasks");
      state.tasks = Array.isArray(tasks) ? tasks : [];

      const items = [{ value: "", name: "Select a task...", description: "" }];

      // Filter by selected project if one is chosen
      // Consider both project_path and worktree_path (use project_path if available, else worktree_path)
      const filteredTasks = state.selectedProjectPath
        ? state.tasks.filter((t) => {
            const taskPath = t.project_path || t.projectPath || t.worktree_path || t.worktreePath;
            return taskPath === state.selectedProjectPath;
          })
        : state.tasks;

      filteredTasks.forEach((task) => {
        const taskId = getTaskId(task);
        if (!taskId) return;
        const prompt = task.prompt || "";
        const truncatedPrompt =
          prompt.length > 50 ? prompt.substring(0, 50) + "..." : prompt;
        items.push({
          value: taskId,
          name: friendlyTaskName(task),
          description: truncatedPrompt,
        });
      });

      if (reviewTaskDropdown) {
        reviewTaskDropdown.setOptions(items);
        // Reset selection if current task is not in filtered list
        if (
          state.selectedTaskId &&
          !filteredTasks.find((t) => getTaskId(t) === state.selectedTaskId)
        ) {
          reviewTaskDropdown.setValue("");
          state.selectedTaskId = null;
        }
      }
    } catch (err) {
      console.warn("[Review] loadTasks failed:", err);
    }
  }

  function getTaskId(task) {
    if (!task) return null;
    const rawId =
      task.id ??
      task.ID ??
      task.task_id ??
      task.taskId ??
      task.taskID ??
      null;
    if (rawId === null || rawId === undefined) return null;
    return String(rawId);
  }

  function findTask(taskId) {
    if (!taskId) return null;
    return state.tasks.find((t) => getTaskId(t) === taskId) || null;
  }

  /**
   * Extract a short, friendly ID from a full task ID.
   * "task-1769976653565-4bddf02d" → "4bddf02d"
   */
  function shortTaskId(fullId) {
    if (!fullId) return "";
    // Task IDs are formatted as: task-{timestamp}-{uuid_prefix}
    const parts = fullId.split("-");
    if (parts.length >= 3) {
      // Return the uuid prefix (last part)
      return parts[parts.length - 1];
    }
    // Fallback: return last 8 chars
    return fullId.slice(-8);
  }

  /**
   * Get a friendly display name for a task.
   * Priority: title_summary > branch > short ID
   */
  function friendlyTaskName(task) {
    const shortId = shortTaskId(getTaskId(task));
    const title = task.title_summary || task.titleSummary;
    const branch = task.branch || task.branch_name;

    if (title) {
      return `#${shortId} — ${title}`;
    }
    if (branch) {
      return `#${shortId} — ${branch}`;
    }
    return `Task #${shortId}`;
  }

  async function refreshCommitTimeline() {
    if (!state.selectedTaskId || !ipcRenderer) {
      renderCommitTimeline({ commits: [] });
      return;
    }

    try {
      const result = await ipcRenderer.invoke("getTaskCommitTimeline", {
        taskId: state.selectedTaskId,
        compare: state.compareMode,
      });
      renderCommitTimeline(result || { commits: [] });
    } catch (err) {
      console.warn("[Review] getTaskCommitTimeline failed:", err);
      renderCommitTimeline({ commits: [] });
    }
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
      renderFiles([]);
      return;
    }

    try {
      const result = await ipcRenderer.invoke("getTaskDiffFiles", {
        taskId: state.selectedTaskId,
        compare: state.compareMode,
      });
      const files = Array.isArray(result?.files) ? result.files : [];
      renderFiles(files);
    } catch (err) {
      console.warn("[Review] getTaskDiffFiles failed:", err);
      renderFiles([]);
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
      if (state.viewMode === "unified") {
        renderUnifiedDiff("");
      } else {
        renderSplitDiff({ left: [], right: [] });
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
        renderUnifiedDiff(result?.diff || "");
      } else {
        renderSplitDiff(result?.diff || { left: [], right: [] });
      }
    } catch (err) {
      console.warn("[Review] getTaskFileDiff failed:", err);
      if (state.viewMode === "unified") {
        renderUnifiedDiff("");
      } else {
        renderSplitDiff({ left: [], right: [] });
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
    ipcRenderer.send("OpenTaskDirectory", path, "finder");
  }

  function bindEvents() {
    if (state.eventsBound) return;
    state.eventsBound = true;

    // Note: Selector events are handled by CustomDropdown's onChange callbacks

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
        await refreshCommitTimeline();
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
      await loadProjectsIntoSelector();
      await loadTasksIntoSelector();
      await refreshFiles();
      await refreshCommitTimeline();
    });

    $("reviewOpenWorktreeBtn")?.addEventListener("click", openSelectedWorktree);

    // Placeholder actions
    $("reviewCommentBtn")?.addEventListener("click", () => {
      console.log("[Review] Comment clicked (TODO)");
    });

    $("reviewApproveBtn")?.addEventListener("click", () => {
      console.log("[Review] Approve clicked (TODO)");
    });

    window.addEventListener("PhantomNavigate", (event) => {
      const pageId = event?.detail?.pageId;
      if (pageId !== "reviewPage") return;
      initReviewCenter();
    });

    watchReviewVisibility();
  }

  function watchReviewVisibility() {
    const page = $("reviewPage");
    if (!page || state.visibilityObserver) return;

    state.visibilityObserver = new MutationObserver(() => {
      if (!page.hasAttribute("hidden")) {
        initReviewCenter();
      }
    });

    state.visibilityObserver.observe(page, {
      attributes: true,
      attributeFilter: ["hidden"],
    });

    if (!page.hasAttribute("hidden")) {
      initReviewCenter();
    }
  }

  async function initReviewCenter() {
    if (state.initialized) return;
    state.initialized = true;

    await loadProjectsIntoSelector();
    await loadTasksIntoSelector();
    renderFiles([]);
    renderDiffPlaceholder();
    renderCommitTimeline({ commits: [] });
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
