#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────
# download_weights.sh — Download pretrained FaceFormer (vocaset) model
# and FLAME template files into modules/avatar_overlay/models/
# ─────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODELS_DIR="${SCRIPT_DIR}/models"

mkdir -p "${MODELS_DIR}"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Avatar Overlay — Model Weights Downloader                  ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ── 1. FaceFormer vocaset checkpoint ────────────────────────────────
VOCASET_PTH="${MODELS_DIR}/vocaset.pth"
# Google Drive file ID for vocaset.pth from EvelynFan/FaceFormer
VOCASET_GDRIVE_ID="1GUQBk9FqUimoT6UNgU0gyQnjGv-2_Lyp"

if [ -f "${VOCASET_PTH}" ]; then
    echo "✓ vocaset.pth already exists, skipping."
else
    echo "⬇ Downloading vocaset.pth from Google Drive..."
    echo "  (File ID: ${VOCASET_GDRIVE_ID})"
    echo ""
    echo "  ⚠  Google Drive may require manual confirmation for large files."
    echo "     If the automatic download fails, please download manually from:"
    echo "     https://drive.google.com/file/d/${VOCASET_GDRIVE_ID}/view?usp=sharing"
    echo "     and place the file at: ${VOCASET_PTH}"
    echo ""

    # Attempt gdown first (pip install gdown)
    if command -v gdown &>/dev/null; then
        gdown "${VOCASET_GDRIVE_ID}" -O "${VOCASET_PTH}" || {
            echo "✗ gdown failed. Please download manually."
        }
    else
        # Fallback: curl with Google Drive confirm trick
        CONFIRM=$(curl -sc /tmp/gdrive_cookie \
            "https://drive.google.com/uc?export=download&id=${VOCASET_GDRIVE_ID}" | \
            grep -o 'confirm=[^&]*' | head -1 | cut -d= -f2)
        
        if [ -n "${CONFIRM}" ]; then
            curl -Lb /tmp/gdrive_cookie \
                "https://drive.google.com/uc?export=download&confirm=${CONFIRM}&id=${VOCASET_GDRIVE_ID}" \
                -o "${VOCASET_PTH}"
        else
            curl -L \
                "https://drive.google.com/uc?export=download&id=${VOCASET_GDRIVE_ID}" \
                -o "${VOCASET_PTH}"
        fi
        rm -f /tmp/gdrive_cookie
    fi

    if [ -f "${VOCASET_PTH}" ] && [ "$(stat -f%z "${VOCASET_PTH}" 2>/dev/null || stat -c%s "${VOCASET_PTH}" 2>/dev/null)" -gt 1000000 ]; then
        echo "✓ vocaset.pth downloaded successfully."
    else
        echo "✗ Download may have failed (file too small). Please download manually."
        rm -f "${VOCASET_PTH}"
    fi
fi

# ── 2. FLAME template PLY ──────────────────────────────────────────
FLAME_PLY="${MODELS_DIR}/FLAME_sample.ply"

if [ -f "${FLAME_PLY}" ]; then
    echo "✓ FLAME_sample.ply already exists, skipping."
else
    echo "⬇ Downloading FLAME_sample.ply from VOCA repo..."
    curl -fSL \
        "https://raw.githubusercontent.com/TimoBolkart/voca/master/template/FLAME_sample.ply" \
        -o "${FLAME_PLY}" && \
        echo "✓ FLAME_sample.ply downloaded." || \
        echo "✗ Failed to download FLAME_sample.ply. Please download manually."
fi

# ── 3. VOCASET templates.pkl ───────────────────────────────────────
TEMPLATES_PKL="${MODELS_DIR}/templates.pkl"

if [ -f "${TEMPLATES_PKL}" ]; then
    echo "✓ templates.pkl already exists, skipping."
else
    echo ""
    echo "⚠  templates.pkl must be obtained from the VOCASET dataset."
    echo "   Request access at: https://voca.is.tue.mpg.de/"
    echo "   After downloading, place templates.pkl at:"
    echo "   ${TEMPLATES_PKL}"
    echo ""
    echo "   Alternatively, the module can generate a synthetic template"
    echo "   from FLAME_sample.ply for demo purposes (reduced quality)."
fi

echo ""
echo "Done. Model directory contents:"
ls -lh "${MODELS_DIR}/" 2>/dev/null || echo "(empty)"
