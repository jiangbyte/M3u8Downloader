#!/usr/bin/env bash
# Download a platform-matched ffmpeg binary into src-tauri/binaries/ for Tauri externalBin.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="${ROOT}/src-tauri/binaries"
mkdir -p "${OUT_DIR}"

TARGET_TRIPLE="${TARGET_TRIPLE:-}"
if [[ -z "${TARGET_TRIPLE}" ]]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) TARGET_TRIPLE="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64) TARGET_TRIPLE="aarch64-unknown-linux-gnu" ;;
    Darwin-arm64) TARGET_TRIPLE="aarch64-apple-darwin" ;;
    Darwin-x86_64) TARGET_TRIPLE="x86_64-apple-darwin" ;;
    MINGW*|MSYS*|CYGWIN*) TARGET_TRIPLE="x86_64-pc-windows-msvc" ;;
    *)
      echo "Cannot infer TARGET_TRIPLE; set TARGET_TRIPLE=..."
      exit 1
      ;;
  esac
fi

TMP="$(mktemp -d)"
cleanup() { rm -rf "${TMP}"; }
trap cleanup EXIT

echo "Preparing ffmpeg for ${TARGET_TRIPLE}"

case "${TARGET_TRIPLE}" in
  x86_64-pc-windows-msvc)
    URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip"
    curl -fsSL "${URL}" -o "${TMP}/ffmpeg.zip"
    unzip -q "${TMP}/ffmpeg.zip" -d "${TMP}/extract"
    BIN="$(find "${TMP}/extract" -type f -name 'ffmpeg.exe' | head -n1)"
    DEST="${OUT_DIR}/ffmpeg-${TARGET_TRIPLE}.exe"
    cp "${BIN}" "${DEST}"
    ;;
  x86_64-unknown-linux-gnu)
    URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz"
    curl -fsSL "${URL}" -o "${TMP}/ffmpeg.tar.xz"
    tar -xJf "${TMP}/ffmpeg.tar.xz" -C "${TMP}"
    BIN="$(find "${TMP}" -type f -path '*/bin/ffmpeg' | head -n1)"
    DEST="${OUT_DIR}/ffmpeg-${TARGET_TRIPLE}"
    cp "${BIN}" "${DEST}"
    chmod +x "${DEST}"
    ;;
  aarch64-unknown-linux-gnu)
    URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linuxarm64-gpl.tar.xz"
    curl -fsSL "${URL}" -o "${TMP}/ffmpeg.tar.xz"
    tar -xJf "${TMP}/ffmpeg.tar.xz" -C "${TMP}"
    BIN="$(find "${TMP}" -type f -path '*/bin/ffmpeg' | head -n1)"
    DEST="${OUT_DIR}/ffmpeg-${TARGET_TRIPLE}"
    cp "${BIN}" "${DEST}"
    chmod +x "${DEST}"
    ;;
  aarch64-apple-darwin|x86_64-apple-darwin)
    if [[ "$(uname -s)" != "Darwin" ]]; then
      echo "macOS ffmpeg must be prepared on a macOS runner"
      exit 1
    fi
    if ! command -v brew >/dev/null 2>&1; then
      echo "Homebrew is required to fetch ffmpeg on macOS"
      exit 1
    fi
    brew install ffmpeg
    DEST="${OUT_DIR}/ffmpeg-${TARGET_TRIPLE}"
    if [[ "${TARGET_TRIPLE}" == "x86_64-apple-darwin" && "$(uname -m)" == "arm64" ]]; then
      # Prefer an x86_64 bottle when cross-building Intel dmg on Apple Silicon.
      BREW_FFMPEG="$(brew --prefix ffmpeg)/bin/ffmpeg"
      if file "${BREW_FFMPEG}" | grep -q 'x86_64\|universal'; then
        cp "${BREW_FFMPEG}" "${DEST}"
      else
        echo "Warning: using host ffmpeg for x86_64-apple-darwin (may be arm64)."
        cp "${BREW_FFMPEG}" "${DEST}"
      fi
    else
      cp "$(brew --prefix ffmpeg)/bin/ffmpeg" "${DEST}"
    fi
    chmod +x "${DEST}"
    ;;
  *)
    echo "Unsupported TARGET_TRIPLE=${TARGET_TRIPLE}"
    exit 1
    ;;
esac

ls -lh "${OUT_DIR}"/ffmpeg-*
echo "ffmpeg sidecar ready"
