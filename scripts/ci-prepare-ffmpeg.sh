#!/usr/bin/env bash
# Deprecated wrapper — use: node scripts/prepare-ffmpeg.mjs
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec node "$ROOT/scripts/prepare-ffmpeg.mjs"
