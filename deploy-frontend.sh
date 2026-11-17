#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
FRONTEND_DIR="$REPO_ROOT/frontend"
TAURI_CONFIG="$FRONTEND_DIR/src-tauri/tauri.conf.json"
ENV_FILE="$FRONTEND_DIR/.env"
TARGET_DIR="$FRONTEND_DIR/src-tauri/target/x86_64-pc-windows-msvc"
RELEASE_DIR="$TARGET_DIR/release"
BUCKET="gs://releases_screen_kei"

CURRENT_VERSION="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$TAURI_CONFIG" | head -n 1)"
if [[ -z "$CURRENT_VERSION" ]]; then
  echo "Could not read the current app version from $TAURI_CONFIG." >&2
  exit 1
fi

read -r -p "Generate a new release version? [y/N] " NEW_RELEASE
case "$NEW_RELEASE" in
  y|Y|yes|YES|Yes)
    read -r -p "New release version (current: $CURRENT_VERSION): " VERSION
    ;;
  n|N|no|NO|No|'')
    VERSION="$CURRENT_VERSION"
    ;;
  *)
    echo "Please answer y or n." >&2
    exit 1
    ;;
esac

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid version '$VERSION'. Use semantic versioning, for example 1.2.3 or 1.2.3-rc.1." >&2
  exit 1
fi

if [[ ! -f "$ENV_FILE" ]] || ! grep -qE '^[[:space:]]*SIGNALING_WS_URL[[:space:]]*=' "$ENV_FILE"; then
  echo "Set SIGNALING_WS_URL in $ENV_FILE before building." >&2
  exit 1
fi

for tool in cargo rustup trunk zip gcloud makensis; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Required command '$tool' was not found in PATH." >&2
    exit 1
  fi
done

if ! cargo tauri --version >/dev/null 2>&1 || ! cargo xwin --version >/dev/null 2>&1; then
  echo "Install the Tauri CLI and cargo-xwin before running this script." >&2
  exit 1
fi

rustup target add wasm32-unknown-unknown x86_64-pc-windows-msvc

if ! gcloud auth print-access-token >/dev/null 2>&1; then
  echo "Authenticate gcloud first, for example with: gcloud auth login" >&2
  exit 1
fi

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
CONFIG_OVERRIDE="$TEMP_DIR/tauri-release.json"
ARCHIVE="$TARGET_DIR/release.zip"
printf '{"version":"%s"}\n' "$VERSION" > "$CONFIG_OVERRIDE"

echo "Building screen-kei $VERSION for Windows..."
(
  cd "$FRONTEND_DIR"
  cargo tauri build \
    --runner cargo-xwin \
    --target x86_64-pc-windows-msvc \
    --bundles nsis \
    --config "$CONFIG_OVERRIDE"
)

if [[ ! -d "$RELEASE_DIR" ]] || [[ ! -f "$RELEASE_DIR/screen-kei.exe" ]]; then
  echo "The complete Windows application was not found at $RELEASE_DIR." >&2
  exit 1
fi

rm -f "$ARCHIVE"
(
  cd "$TARGET_DIR"
  zip -r "$ARCHIVE" release
)

VERSIONED_OBJECT="$BUCKET/windows/$VERSION/release.zip"
LATEST_OBJECT="$BUCKET/windows/latest/release.zip"
echo "Uploading $VERSION to $VERSIONED_OBJECT"
gcloud storage cp "$ARCHIVE" "$VERSIONED_OBJECT"
echo "Updating $LATEST_OBJECT"
gcloud storage cp "$ARCHIVE" "$LATEST_OBJECT"

echo "Release $VERSION uploaded. If this version already existed, its bucket objects were replaced."
