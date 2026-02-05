#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export ROOT_DIR

APP_NAME="$(python3 - <<'PY'
import json, os, pathlib
p = pathlib.Path(os.environ["ROOT_DIR"]) / "src-tauri" / "tauri.conf.json"
try:
    data = json.loads(p.read_text())
    print(data.get("productName") or "Phantom")
except Exception:
    print("Phantom")
PY
)"

VERSION="$(python3 - <<'PY'
import json, os, pathlib
p = pathlib.Path(os.environ["ROOT_DIR"]) / "src-tauri" / "tauri.conf.json"
try:
    data = json.loads(p.read_text())
    print(data.get("version") or "0.0.0")
except Exception:
    print("0.0.0")
PY
)"

BG_IMAGE="${ROOT_DIR}/scripts/dmg/phantom-dmg-bg.png"

if [[ ! -f "$BG_IMAGE" ]]; then
  echo "Missing DMG background image at $BG_IMAGE" >&2
  exit 1
fi

APP_PATH="${APP_PATH:-}"
if [[ -z "$APP_PATH" ]]; then
  APP_PATH="$(find "${ROOT_DIR}/src-tauri/target/release/bundle/macos" -maxdepth 1 -name "*.app" -print -quit 2>/dev/null || true)"
fi

if [[ -z "$APP_PATH" || ! -d "$APP_PATH" ]]; then
  echo "App bundle not found. Build first with: cargo tauri build" >&2
  exit 1
fi

APP_BUNDLE_NAME="$(basename "$APP_PATH")"

OUT_DMG="${OUT_DMG:-${ROOT_DIR}/src-tauri/target/release/bundle/dmg/${APP_NAME}-${VERSION}-custom.dmg}"
mkdir -p "$(dirname "$OUT_DMG")"

WINDOW_WIDTH=720
WINDOW_HEIGHT=420
WINDOW_X=200
WINDOW_Y=200
WINDOW_RIGHT=$((WINDOW_X + WINDOW_WIDTH))
WINDOW_BOTTOM=$((WINDOW_Y + WINDOW_HEIGHT))
ICON_SIZE=128
APP_ICON_X=180
APP_ICON_Y=230
APPS_ICON_X=540
APPS_ICON_Y=230

STAGING_DIR="$(mktemp -d)"
TMP_DMG="$(mktemp -u /tmp/${APP_NAME}-rw-XXXXXX.dmg)"

cleanup() {
  if [[ -n "${DEVICE:-}" ]]; then
    hdiutil detach "$DEVICE" >/dev/null 2>&1 || true
  fi
  rm -rf "$STAGING_DIR" >/dev/null 2>&1 || true
  rm -f "$TMP_DMG" >/dev/null 2>&1 || true
}
trap cleanup EXIT

ditto "$APP_PATH" "$STAGING_DIR/$APP_BUNDLE_NAME"
ln -s /Applications "$STAGING_DIR/Applications"

SIZE_KB="$(du -sk "$STAGING_DIR" | awk '{print $1}')"
SIZE_MB="$(( (SIZE_KB / 1024) + 20 ))"
hdiutil create -size "${SIZE_MB}m" -fs HFS+ -volname "$APP_NAME" -type UDIF "$TMP_DMG" >/dev/null

EXISTING_DEVICE="$(hdiutil info | awk -v vol="$APP_NAME" '$1 ~ /^\/dev/ {dev=$1} $0 ~ ("/Volumes/" vol "$") {print dev}')"
if [[ -n "$EXISTING_DEVICE" ]]; then
  hdiutil detach "$EXISTING_DEVICE" >/dev/null 2>&1 || true
fi

ATTACH_OUTPUT="$(hdiutil attach -readwrite -noverify -noautoopen "$TMP_DMG")"
DEVICE="$(echo "$ATTACH_OUTPUT" | awk 'NR==1 {print $1}')"
MOUNT_POINT="$(echo "$ATTACH_OUTPUT" | awk 'END {print $3}')"

ditto "$STAGING_DIR/$APP_BUNDLE_NAME" "$MOUNT_POINT/$APP_BUNDLE_NAME"
ln -s /Applications "$MOUNT_POINT/Applications"

mkdir -p "$MOUNT_POINT/.background"
cp "$BG_IMAGE" "$MOUNT_POINT/.background/phantom-dmg-bg.png"

osascript <<EOF
  tell application "Finder"
    tell disk "${APP_NAME}"
      open
      set current view of container window to icon view
      set toolbar visible of container window to false
      set statusbar visible of container window to false
      set the bounds of container window to {${WINDOW_X}, ${WINDOW_Y}, ${WINDOW_RIGHT}, ${WINDOW_BOTTOM}}
      set viewOptions to the icon view options of container window
      set arrangement of viewOptions to not arranged
      set icon size of viewOptions to ${ICON_SIZE}
      set text size of viewOptions to 12
      set background picture of viewOptions to file ".background:phantom-dmg-bg.png"
      set position of item "${APP_BUNDLE_NAME}" of container window to {${APP_ICON_X}, ${APP_ICON_Y}}
      set position of item "Applications" of container window to {${APPS_ICON_X}, ${APPS_ICON_Y}}
      close
      open
      update without registering applications
      delay 1
    end tell
  end tell
EOF

hdiutil detach "$DEVICE" >/dev/null
DEVICE=""

hdiutil convert "$TMP_DMG" -format UDZO -imagekey zlib-level=9 -o "$OUT_DMG" >/dev/null

echo "Custom DMG created: $OUT_DMG"
