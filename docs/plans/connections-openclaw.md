# Connections Tab (OpenClaw) - Manual Verification Checklist

This checklist verifies the new **Connections** tab that installs/manages OpenClaw and embeds the Control UI.

## Scenarios

1. Fresh machine (no brew/node/openclaw)
- Open Phantom.
- Go to `Connections`.
- Expected: Homebrew/Node/OpenClaw show `Missing` (or `Upgrade` for Node), buttons enabled in a sensible order.
- Click `Run Full Setup`.
- Expected: Progress notifications appear; status updates to `OK`; gateway ends `Running`.

2. Node installed but too old
- Ensure `node -v` is `< 22`.
- Open `Connections`.
- Expected: Node badge shows `Upgrade`.
- Click `Install Node`.
- Expected: Node becomes `OK` and `node -v` becomes `>= 22`.

3. OpenClaw already installed
- Ensure `openclaw --version` works.
- Open `Connections`.
- Expected: OpenClaw CLI badge is `OK` and shows version/path.

4. Gateway service not installed
- Ensure OpenClaw is installed but gateway service is not.
- Open `Connections`.
- Expected: `Gateway Service` shows `Missing` or `Unknown`.
- Click `Install Gateway Service`.
- Expected: gateway service becomes `OK`.

5. Gateway start/stop/restart
- Click `Start Gateway`.
- Expected: Gateway badge shows `Running`.
- Click `Stop Gateway`.
- Expected: Gateway badge shows `Stopped`.
- Click `Restart Gateway`.
- Expected: Gateway returns to `Running`.

6. Embedded Control UI loads
- With gateway `Running`, confirm the iframe becomes visible.
- Expected: Sessions/Chat UI renders inside the `Control UI (Sessions + Chat)` card.

7. Open in browser
- Click `Open Control UI in Browser`.
- Expected: Default browser opens the dashboard URL.

## Notes
- The embedded UI comes from `openclaw dashboard --no-open` and is expected to be served from `http://127.0.0.1:<port>/...`.
- On non-macOS platforms, Homebrew install should show `N/A`.

