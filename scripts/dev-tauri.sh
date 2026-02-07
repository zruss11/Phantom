#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

read -r GUI_HOST GUI_PORT GUI_PATH <<<"$(
  python3 - <<'PY'
import json
import os
import sys
from urllib.parse import urlparse

root = os.environ.get("ROOT_DIR")
if not root:
  print("127.0.0.1 8000 /menu.html")
  raise SystemExit(0)

conf_path = os.path.join(root, "src-tauri", "tauri.conf.json")
try:
  with open(conf_path) as f:
    conf = json.load(f)
except Exception:
  print("127.0.0.1 8000 /menu.html")
  raise SystemExit(0)

build = conf.get("build", {}) or {}
dev = build.get("devPath") or build.get("devUrl") or "http://127.0.0.1:8000/menu.html"
u = urlparse(dev)

host = u.hostname or "127.0.0.1"
port = u.port or 8000
path = u.path or "/menu.html"

print(f"{host} {port} {path}")
PY
)"

echo "[dev] Starting GUI server on http://${GUI_HOST}:${GUI_PORT}${GUI_PATH}"

if ! command -v python3 >/dev/null 2>&1; then
  echo "[dev] python3 is required to run the GUI dev server (python3 -m http.server)."
  exit 1
fi

if command -v lsof >/dev/null 2>&1; then
  if lsof -ti "tcp:${GUI_PORT}" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "[dev] Port ${GUI_PORT} is already in use. Stop the existing server and retry."
    exit 1
  fi
fi

GUI_PID=""
cleanup() {
  if [[ -n "${GUI_PID}" ]]; then
    kill "${GUI_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

(
  cd "${ROOT_DIR}/gui"
  python3 -m http.server "${GUI_PORT}" --bind "${GUI_HOST}" >/dev/null 2>&1
) &
GUI_PID="$!"

python3 - <<'PY'
import os
import socket
import time

host = os.environ.get("GUI_HOST", "127.0.0.1")
port = int(os.environ.get("GUI_PORT", "8000"))

deadline = time.time() + 3.0
while time.time() < deadline:
  s = socket.socket()
  s.settimeout(0.2)
  try:
    s.connect((host, port))
    s.close()
    raise SystemExit(0)
  except OSError:
    time.sleep(0.1)
  finally:
    try:
      s.close()
    except Exception:
      pass

print(f"[dev] GUI server did not start listening on {host}:{port} within 3s.")
raise SystemExit(1)
PY

cd "${ROOT_DIR}/src-tauri"
exec cargo tauri dev
