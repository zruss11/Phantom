#!/usr/bin/env bash
set -euo pipefail

# Dev helper: run the GUI dev server + Tauri app together.
# Matches src-tauri/tauri.conf.json devUrl: http://127.0.0.1:8000/menu.html

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUI_DIR="$ROOT_DIR/gui"
TAURI_DIR="$ROOT_DIR/src-tauri"

PORT="${PORT:-8000}"
HOST="${HOST:-127.0.0.1}"
DEV_URL="http://${HOST}:${PORT}/menu.html"

SERVER_PID=""
REUSED_SERVER="0"
SERVER_LOG="${ROOT_DIR}/.dev-server.log"
PROBE_TMP=""

cleanup() {
  set +e
  if [[ -n "${SERVER_PID}" && "${REUSED_SERVER}" != "1" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${PROBE_TMP}" && -f "${PROBE_TMP}" ]]; then
    rm -f "${PROBE_TMP}" >/dev/null 2>&1 || true
  fi
}

trap cleanup EXIT INT TERM

if ! command -v python3 >/dev/null 2>&1; then
  echo "[dev-tauri] python3 is required to run the GUI dev server (python3 -m http.server)." >&2
  exit 1
fi

if command -v lsof >/dev/null 2>&1; then
  if lsof -nP -iTCP:"${PORT}" -sTCP:LISTEN >/dev/null 2>&1; then
    pid="$(lsof -nP -t -iTCP:"${PORT}" -sTCP:LISTEN 2>/dev/null | head -n 1 || true)"
    cmd=""
    if [[ -n "${pid}" ]]; then
      cmd="$(ps -p "${pid}" -o command= 2>/dev/null || true)"
    fi

    # If the listener is already a python http.server serving this repo's GUI, reuse it.
    if [[ -n "${pid}" ]] && [[ "${cmd}" == *"-m http.server"* ]]; then
      cwd="$(lsof -p "${pid}" 2>/dev/null | rg \"\\scwd\\s\" | awk '{print $NF}' | head -n 1 || true)"
      if [[ "${cwd}" == "${GUI_DIR}" ]]; then
        echo "[dev-tauri] Reusing existing GUI server on ${DEV_URL} (pid ${pid})"
        REUSED_SERVER="1"
      else
        echo "[dev-tauri] Port ${PORT} is already in use (pid ${pid}, cwd ${cwd}). Stop it or set PORT=<port>." >&2
        exit 1
      fi
    else
      echo "[dev-tauri] Port ${PORT} is already in use. Stop the existing server or set PORT=<port>." >&2
      exit 1
    fi
  fi
fi

rm -f "${SERVER_LOG}" >/dev/null 2>&1 || true

if [[ "${REUSED_SERVER}" != "1" ]]; then
  echo "[dev-tauri] Starting GUI server: ${DEV_URL} (serving ${GUI_DIR})"
  (
    cd "${GUI_DIR}"
    python3 -m http.server "${PORT}" --bind "${HOST}"
  ) >"${SERVER_LOG}" 2>&1 &
  SERVER_PID="$!"

  sleep 0.15
  if ! kill -0 "${SERVER_PID}" >/dev/null 2>&1; then
    echo "[dev-tauri] GUI server failed to start. Log:" >&2
    tail -n 50 "${SERVER_LOG}" >&2 || true
    exit 1
  fi
fi

if command -v curl >/dev/null 2>&1; then
  # Ensure we're serving the repo's GUI, not some other stale server.
  PROBE_TMP="$(mktemp -t phantom-gui-probe.XXXXXX)"
  ok="0"
  for _ in {1..30}; do
    if curl -fsS --max-time 1 "${DEV_URL}" -o "${PROBE_TMP}" >/dev/null 2>&1; then
      if rg -q "data-page=\\\"notesPage\\\"|id=\\\"notesPage\\\"" "${PROBE_TMP}"; then
        ok="1"
        break
      fi
    fi
    sleep 0.1
  done
  if [[ "${ok}" != "1" ]]; then
    echo "[dev-tauri] GUI server is up but doesn't look like this repo's gui/menu.html (missing notesPage)." >&2
    echo "[dev-tauri] You may have another server responding on ${HOST}:${PORT}." >&2
    echo "[dev-tauri] Server log (tail):" >&2
    tail -n 30 "${SERVER_LOG}" >&2 || true
    exit 1
  fi
fi

echo "[dev-tauri] Starting Tauri dev app (from ${TAURI_DIR})"
cd "${TAURI_DIR}"
cargo tauri dev
