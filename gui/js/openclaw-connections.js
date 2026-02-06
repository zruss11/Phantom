// Connections tab: OpenClaw install + gateway toggle + embedded Control UI.
// No frameworks: keep this small and resilient to missing APIs in browser/mock mode.

(function () {
  "use strict";

  const bridge = window.tauriBridge;
  const ipcRenderer = bridge && bridge.ipcRenderer ? bridge.ipcRenderer : null;

  const POLL_MS = 4000;
  let pollTimer = null;
  let lastDashboardUrl = null;
  let lastPageId = null;
  let busy = false;

  function notify(message, color) {
    try {
      if (typeof window.sendNotification === "function") {
        window.sendNotification(message, color || "green");
        return;
      }
    } catch (e) {
      // ignore
    }
    try {
      console.log("[Connections]", message);
    } catch (e) {
      // ignore
    }
  }

  function $(id) {
    return document.getElementById(id);
  }

  function setBadge(el, kind, text) {
    if (!el) return;
    el.classList.remove(
      "badge-success",
      "badge-warning",
      "badge-danger",
      "badge-secondary"
    );
    if (kind === "ok") el.classList.add("badge-success");
    else if (kind === "warn") el.classList.add("badge-warning");
    else if (kind === "err") el.classList.add("badge-danger");
    else el.classList.add("badge-secondary");
    el.textContent = text;
  }

  function setText(el, text) {
    if (!el) return;
    el.textContent = text || "";
  }

  function setDisabled(el, disabled) {
    if (!el) return;
    el.disabled = !!disabled;
  }

  async function invoke(channel, ...args) {
    if (!ipcRenderer || typeof ipcRenderer.invoke !== "function") {
      throw new Error("Tauri IPC is unavailable (are you running in browser mode?)");
    }
    return await ipcRenderer.invoke(channel, ...args);
  }

  function isOnConnectionsPage() {
    const section = $("connectionsPage");
    if (!section) return false;
    return !section.hasAttribute("hidden");
  }

  function stopPolling() {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }

  function startPolling() {
    stopPolling();
    pollTimer = setInterval(() => {
      if (!isOnConnectionsPage()) return;
      refreshAll({ quiet: true });
    }, POLL_MS);
  }

  function setFrameVisible(visible) {
    const frame = $("openclawControlUiFrame");
    const empty = $("openclawControlUiEmpty");
    if (frame) frame.hidden = !visible;
    if (empty) empty.hidden = !!visible;
  }

  function setFrameUrl(url) {
    const label = $("openclawControlUiUrlLabel");
    const frame = $("openclawControlUiFrame");
    if (!url) {
      lastDashboardUrl = null;
      if (label) label.textContent = "";
      if (frame) frame.removeAttribute("src");
      setFrameVisible(false);
      return;
    }
    lastDashboardUrl = url;
    if (label) label.textContent = url;
    if (frame) frame.src = url;
    setFrameVisible(true);
  }

  function summarizeTool(probe) {
    if (!probe) return "";
    const v = probe.version ? probe.version : "";
    const p = probe.path ? probe.path : "";
    if (v && p) return `${v} · ${p}`;
    if (v) return v;
    if (p) return p;
    return "";
  }

  function summarizeNode(node) {
    if (!node) return "";
    const v = node.version ? node.version : "";
    const p = node.path ? node.path : "";
    if (v && p) return `${v} · ${p}`;
    if (v) return v;
    if (p) return p;
    return "";
  }

  function setBusyState(nextBusy) {
    busy = !!nextBusy;
    setDisabled($("openclawInstallBrewBtn"), busy);
    setDisabled($("openclawInstallNodeBtn"), busy);
    setDisabled($("openclawInstallCliBtn"), busy);
    setDisabled($("openclawGatewayInstallBtn"), busy);
    setDisabled($("openclawGatewayStartBtn"), busy);
    setDisabled($("openclawGatewayStopBtn"), busy);
    setDisabled($("openclawGatewayRestartBtn"), busy);
    setDisabled($("openclawGatewayUninstallBtn"), busy);
    setDisabled($("openclawDoctorFixBtn"), busy);
    setDisabled($("openclawOpenBrowserBtn"), busy);
    setDisabled($("openclawRefreshBtn"), busy);
    setDisabled($("openclawFullSetupBtn"), busy);
    setDisabled($("openclawEmptyStartBtn"), busy);
    setDisabled($("openclawEmptyOpenBrowserBtn"), busy);
    setDisabled($("openclawReloadFrameBtn"), busy);
  }

  async function tryGetDashboardUrl() {
    try {
      const url = await invoke("openclawDashboardUrl");
      if (url && typeof url === "string") return url;
      return null;
    } catch (e) {
      return null;
    }
  }

  async function refreshAll(opts) {
    opts = opts || {};
    if (busy) return;
    if (!isOnConnectionsPage()) return;

    let probe = null;
    try {
      probe = await invoke("openclawProbe");
    } catch (e) {
      if (!opts.quiet) notify(`Failed to probe OpenClaw: ${e.message || e}`, "red");
      // Reset UI to unknown-ish state
      setBadge($("openclawBrewBadge"), "unknown", "Unknown");
      setBadge($("openclawNodeBadge"), "unknown", "Unknown");
      setBadge($("openclawNpmBadge"), "unknown", "Unknown");
      setBadge($("openclawCliBadge"), "unknown", "Unknown");
      setBadge($("openclawGatewaySvcBadge"), "unknown", "Unknown");
      setBadge($("openclawGatewayBadge"), "unknown", "Unknown");
      setFrameVisible(false);
      return;
    }

    // Homebrew (macOS only)
    const isMac = probe.os === "macos";
    const brew = probe.brew;
    if (!isMac) {
      setBadge($("openclawBrewBadge"), "ok", "N/A");
      setText($("openclawBrewDetail"), "Not used on this OS");
      setDisabled($("openclawInstallBrewBtn"), true);
    } else if (brew && brew.installed) {
      setBadge($("openclawBrewBadge"), "ok", "OK");
      setText($("openclawBrewDetail"), summarizeTool(brew));
      setDisabled($("openclawInstallBrewBtn"), true);
    } else {
      setBadge($("openclawBrewBadge"), "warn", "Missing");
      setText($("openclawBrewDetail"), "");
      setDisabled($("openclawInstallBrewBtn"), false);
    }

    // Node
    const node = probe.node;
    if (node && node.installed && node.ok) {
      setBadge($("openclawNodeBadge"), "ok", "OK");
      setText($("openclawNodeDetail"), summarizeNode(node));
      setDisabled($("openclawInstallNodeBtn"), true);
    } else if (node && node.installed && !node.ok) {
      setBadge($("openclawNodeBadge"), "warn", "Upgrade");
      setText($("openclawNodeDetail"), summarizeNode(node));
      setDisabled($("openclawInstallNodeBtn"), !isMac);
    } else {
      setBadge($("openclawNodeBadge"), "warn", "Missing");
      setText($("openclawNodeDetail"), "");
      setDisabled($("openclawInstallNodeBtn"), !isMac);
    }

    // npm
    const npm = probe.npm;
    if (npm && npm.installed) {
      setBadge($("openclawNpmBadge"), "ok", "OK");
      setText($("openclawNpmDetail"), summarizeTool(npm));
    } else {
      setBadge($("openclawNpmBadge"), "warn", "Missing");
      setText($("openclawNpmDetail"), "");
    }

    // OpenClaw CLI
    const cli = probe.openclaw;
    if (cli && cli.installed) {
      setBadge($("openclawCliBadge"), "ok", "OK");
      setText($("openclawCliDetail"), summarizeTool(cli));
      setDisabled($("openclawInstallCliBtn"), true);
    } else {
      setBadge($("openclawCliBadge"), "warn", "Missing");
      setText($("openclawCliDetail"), "");
      setDisabled($("openclawInstallCliBtn"), false);
    }

    // Gateway
    const gw = probe.gateway;
    const gwInstalled = gw && gw.installed === true;
    const gwRunning = gw && gw.running === true;
    const gwPort = gw && gw.port ? gw.port : null;

    if (!cli || !cli.installed) {
      setBadge($("openclawGatewaySvcBadge"), "unknown", "Unknown");
      setText($("openclawGatewaySvcDetail"), "Install OpenClaw first");
      setBadge($("openclawGatewayBadge"), "unknown", "Unknown");
      setText($("openclawGatewayDetail"), "");
      setDisabled($("openclawGatewayInstallBtn"), true);
      setDisabled($("openclawGatewayStartBtn"), true);
      setDisabled($("openclawGatewayStopBtn"), true);
      setDisabled($("openclawGatewayRestartBtn"), true);
      setDisabled($("openclawGatewayUninstallBtn"), true);
      setDisabled($("openclawDoctorFixBtn"), true);
      setDisabled($("openclawOpenBrowserBtn"), true);
      setFrameVisible(false);
      return;
    }

    // Gateway service installed badge
    if (gw && gw.installed === true) {
      setBadge($("openclawGatewaySvcBadge"), "ok", "OK");
      setText($("openclawGatewaySvcDetail"), gwPort ? `Port ${gwPort}` : "");
    } else if (gw && gw.installed === false) {
      setBadge($("openclawGatewaySvcBadge"), "warn", "Missing");
      setText($("openclawGatewaySvcDetail"), gw && gw.error ? "Status unavailable" : "");
    } else {
      setBadge($("openclawGatewaySvcBadge"), "unknown", "Unknown");
      setText($("openclawGatewaySvcDetail"), gw && gw.error ? "Status unavailable" : "");
    }

    // Gateway running badge
    if (gwRunning) {
      setBadge($("openclawGatewayBadge"), "ok", "Running");
      setText(
        $("openclawGatewayDetail"),
        gwPort ? `Port ${gwPort}` : (gw && gw.status_message ? gw.status_message : ""),
      );
    } else if (gw && gw.installed === true) {
      setBadge($("openclawGatewayBadge"), "warn", "Stopped");
      setText(
        $("openclawGatewayDetail"),
        gw && gw.status_message ? gw.status_message : "",
      );
    } else {
      setBadge($("openclawGatewayBadge"), "unknown", "Unknown");
      setText($("openclawGatewayDetail"), gw && gw.error ? "Status unavailable" : "");
    }

    setDisabled($("openclawGatewayInstallBtn"), gwInstalled);
    setDisabled($("openclawGatewayStartBtn"), !gwInstalled || gwRunning);
    setDisabled($("openclawGatewayStopBtn"), !gwInstalled || !gwRunning);
    setDisabled($("openclawGatewayRestartBtn"), !gwInstalled);
    setDisabled($("openclawGatewayUninstallBtn"), !gwInstalled);
    setDisabled($("openclawDoctorFixBtn"), false);
    setDisabled($("openclawOpenBrowserBtn"), false);

    // Dashboard URL / iframe
    if (gwInstalled && gwRunning) {
      const url = await tryGetDashboardUrl();
      if (url && url !== lastDashboardUrl) {
        setFrameUrl(url);
      } else if (!url && !lastDashboardUrl) {
        setFrameVisible(false);
      }
    } else {
      setFrameUrl(null);
    }
  }

  async function runAction(label, fn) {
    if (busy) return;
    setBusyState(true);
    try {
      await fn();
      notify(`${label}: done`, "green");
    } catch (e) {
      notify(`${label} failed: ${e.message || e}`, "red");
    } finally {
      setBusyState(false);
      await refreshAll({ quiet: true });
    }
  }

  async function openControlUiInBrowser() {
    if (busy) return;
    setBusyState(true);
    try {
      const url = await invoke("openclawDashboardUrl");
      if (!url) throw new Error("No dashboard URL returned");
      await invoke("openExternalUrl", url);
      notify("Opened Control UI in browser", "green");
    } catch (e) {
      notify(`Open in browser failed: ${e.message || e}`, "red");
    } finally {
      setBusyState(false);
    }
  }

  async function fullSetup() {
    if (busy) return;
    setBusyState(true);
    try {
      let probe = await invoke("openclawProbe");
      const isMac = probe.os === "macos";

      if (isMac && (!probe.brew || !probe.brew.installed)) {
        notify("Installing Homebrew...", "blue");
        await invoke("openclawInstallBrew");
        probe = await invoke("openclawProbe");
      }

      if (isMac && (!probe.node || !probe.node.ok)) {
        notify("Installing/upgrading Node...", "blue");
        await invoke("openclawInstallNode");
        probe = await invoke("openclawProbe");
      }

      if (!probe.openclaw || !probe.openclaw.installed) {
        notify("Installing OpenClaw CLI...", "blue");
        await invoke("openclawInstallCli");
        probe = await invoke("openclawProbe");
      }

      const gw = probe.gateway;
      const gwInstalled = gw && gw.installed === true;
      if (!gwInstalled) {
        notify("Installing gateway service...", "blue");
        await invoke("openclawGatewayInstall");
        probe = await invoke("openclawProbe");
      }

      const gw2 = probe.gateway;
      const gwRunning = gw2 && gw2.running === true;
      if (!gwRunning) {
        notify("Starting gateway...", "blue");
        await invoke("openclawGatewayStart");
      }

      notify("Full setup complete", "green");
    } catch (e) {
      notify(`Full setup failed: ${e.message || e}`, "red");
    } finally {
      setBusyState(false);
      await refreshAll({ quiet: true });
    }
  }

  function bindOnce() {
    const bind = (id, handler) => {
      const el = $(id);
      if (!el) return;
      // Avoid double-binding if script is loaded twice.
      if (el.dataset.bound === "1") return;
      el.dataset.bound = "1";
      el.addEventListener("click", handler);
    };

    bind("openclawRefreshBtn", () => refreshAll({ quiet: false }));
    bind("openclawInstallBrewBtn", () =>
      runAction("Install Homebrew", async () => {
        await invoke("openclawInstallBrew");
      }),
    );
    bind("openclawInstallNodeBtn", () =>
      runAction("Install Node", async () => {
        await invoke("openclawInstallNode");
      }),
    );
    bind("openclawInstallCliBtn", () =>
      runAction("Install OpenClaw", async () => {
        await invoke("openclawInstallCli");
      }),
    );
    bind("openclawGatewayInstallBtn", () =>
      runAction("Install Gateway Service", async () => {
        await invoke("openclawGatewayInstall");
      }),
    );
    bind("openclawGatewayStartBtn", () =>
      runAction("Start Gateway", async () => {
        await invoke("openclawGatewayStart");
      }),
    );
    bind("openclawGatewayStopBtn", () =>
      runAction("Stop Gateway", async () => {
        await invoke("openclawGatewayStop");
      }),
    );
    bind("openclawGatewayRestartBtn", () =>
      runAction("Restart Gateway", async () => {
        await invoke("openclawGatewayRestart");
      }),
    );
    bind("openclawGatewayUninstallBtn", () =>
      runAction("Uninstall Gateway", async () => {
        await invoke("openclawGatewayUninstall");
      }),
    );
    bind("openclawDoctorFixBtn", () =>
      runAction("Doctor (--fix)", async () => {
        await invoke("openclawDoctorFix");
      }),
    );
    bind("openclawOpenBrowserBtn", openControlUiInBrowser);

    bind("openclawEmptyStartBtn", () =>
      runAction("Start Gateway", async () => {
        await invoke("openclawGatewayStart");
      }),
    );
    bind("openclawEmptyOpenBrowserBtn", openControlUiInBrowser);

    bind("openclawReloadFrameBtn", () => {
      const frame = $("openclawControlUiFrame");
      if (frame && frame.src) {
        // Force reload.
        const src = frame.src;
        frame.src = "about:blank";
        setTimeout(() => {
          frame.src = src;
        }, 50);
      }
    });

    bind("openclawFullSetupBtn", fullSetup);
  }

  function onNavigate(pageId) {
    lastPageId = pageId;
    if (pageId === "connectionsPage") {
      bindOnce();
      refreshAll({ quiet: true });
      startPolling();
    } else {
      // Keep polling only when on the page.
      stopPolling();
    }
  }

  // Init
  window.addEventListener("PhantomNavigate", (e) => {
    const pageId = e && e.detail ? e.detail.pageId : null;
    if (pageId) onNavigate(pageId);
  });

  // If the page is already visible on load (rare), initialize immediately.
  setTimeout(() => {
    if (isOnConnectionsPage()) {
      onNavigate("connectionsPage");
    }
  }, 0);
})();
