#!/bin/bash
# Natasha OmniGraph RTX render launcher
# Uses omni.app.full (has OmniGraph) instead of omni.app.hydra (doesn't)

set -e

KIT_DIR="/home/jkh/.cache/packman/chk/kit-kernel/110.0.0+feature.manylinux_2_35_aarch64.release"
SCRIPT_DIR="$(dirname "$(realpath "$0")")"
RENDER_SCRIPT="$SCRIPT_DIR/natasha_og_render.py"
OUTPUT_DIR="$SCRIPT_DIR/og_output"

mkdir -p "$OUTPUT_DIR"

echo "[natasha] Starting OmniGraph RTX render..."
echo "[natasha] Kit: $KIT_DIR"
echo "[natasha] Script: $RENDER_SCRIPT"
echo "[natasha] Output: $OUTPUT_DIR"

DISPLAY=:1 "$KIT_DIR/omni.app.full.sh" \
    --exec "$RENDER_SCRIPT" \
    --/app/window/width=1920 \
    --/app/window/height=1080 \
    --/rtx/rendermode=PathTracing \
    --/rtx/pathtracing/totalSpp=128 \
    --no-window \
    2>&1 | tee "$SCRIPT_DIR/og_render.log"

echo "[natasha] Render complete. Checking output..."
ls -lh "$OUTPUT_DIR/" 2>/dev/null || echo "No output files found"
