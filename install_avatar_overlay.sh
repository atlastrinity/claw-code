#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# install_avatar_overlay.sh — 3D Avatar Overlay Installer
#
# Installs Python requirements, downloads FLAME 3D template and neural model
# weights, and verifies the Apple Silicon MPS acceleration.
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AVATAR_DIR="${SCRIPT_DIR}/modules/avatar_overlay"

# Pretty printing
if [ -t 1 ] && command -v tput >/dev/null 2>&1 && [ "$(tput colors 2>/dev/null || echo 0)" -ge 8 ]; then
    COLOR_RESET="$(tput sgr0)"
    COLOR_BOLD="$(tput bold)"
    COLOR_DIM="$(tput dim 2>/dev/null || echo '')"
    COLOR_RED="$(tput setaf 1)"
    COLOR_GREEN="$(tput setaf 2)"
    COLOR_YELLOW="$(tput setaf 3)"
    COLOR_CYAN="$(tput setaf 6)"
else
    COLOR_RESET=''
    COLOR_BOLD=''
    COLOR_DIM=''
    COLOR_RED=''
    COLOR_GREEN=''
    COLOR_YELLOW=''
    COLOR_CYAN=''
fi

info()    { printf '%s==>%s %s%s%s\n' "${COLOR_CYAN}" "${COLOR_RESET}" "${COLOR_BOLD}" "$*" "${COLOR_RESET}"; }
ok()      { printf '%s==>%s %s%s%s\n' "${COLOR_GREEN}" "${COLOR_RESET}" "${COLOR_BOLD}" "$*" "${COLOR_RESET}"; }
warn()    { printf '%s==>%s %s%s%s\n' "${COLOR_YELLOW}" "${COLOR_RESET}" "${COLOR_BOLD}" "$*" "${COLOR_RESET}" >&2; }
error()   { printf '%s==>%s %s%s%s\n' "${COLOR_RED}" "${COLOR_RESET}" "${COLOR_BOLD}" "$*" "${COLOR_RESET}" >&2; }

echo ""
echo "╔══════════════════════════════════════════════════════════════════════╗"
echo "║  3D Avatar Overlay (Green Mesh) — Installation & Verification        ║"
echo "╚══════════════════════════════════════════════════════════════════════╝"
echo ""

# 1. Locate Python
PYTHON_BIN=""
for candidate in "$HOME/.pyenv/shims/python3" "$(command -v python3 2>/dev/null)" "/usr/bin/python3" "/opt/homebrew/bin/python3"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then
        PYTHON_BIN="$candidate"
        break
    fi
done

if [ -z "$PYTHON_BIN" ]; then
    error "Python 3 is required but not found in PATH."
    exit 1
fi

info "Using Python: $PYTHON_BIN ($("$PYTHON_BIN" --version))"

# 2. Install Python dependencies
info "Installing Python dependencies from modules/avatar_overlay/requirements.txt..."
"$PYTHON_BIN" -m pip install -r "${AVATAR_DIR}/requirements.txt" || {
    error "Failed to install Python dependencies."
    exit 1
}
ok "Python dependencies installed successfully."

# 3. Download weights and FLAME template
info "Checking 3D templates and model weights..."
if [ -f "${AVATAR_DIR}/download_weights.sh" ]; then
    chmod +x "${AVATAR_DIR}/download_weights.sh"
    bash "${AVATAR_DIR}/download_weights.sh" || warn "Model weights downloader reported non-fatal notice."
fi

# 4. Verification smoke test
info "Running verification smoke test..."
"$PYTHON_BIN" -u -c "
import sys
try:
    import torch
    import PySide6
    import trimesh
    from modules.avatar_overlay.config import AvatarConfig
    from modules.avatar_overlay.audio2mesh import Audio2MeshInference
    
    device = 'mps' if torch.backends.mps.is_available() else 'cpu'
    config = AvatarConfig(device=device)
    engine = Audio2MeshInference(config)
    
    neutral = engine.get_neutral_mesh()
    faces = engine.get_faces()
    print(f'✅ Avatar engine ready: {len(neutral)} vertices, {len(faces)} faces, device={device}')
except Exception as e:
    print(f'❌ Avatar verification failed: {e}', file=sys.stderr)
    sys.exit(1)
" || {
    error "Avatar overlay verification failed."
    exit 1
}

ok "3D Avatar Overlay system successfully installed and verified!"
echo ""
