"""
Central configuration for the Avatar Overlay module.

All tuneable parameters are collected here so that nothing is hard-coded
in the rendering, inference or windowing layers.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

# Root of the avatar_overlay package on disk.
_MODULE_DIR = Path(__file__).resolve().parent


@dataclass
class AvatarConfig:
    """Immutable-ish configuration object passed to every subsystem."""

    # ── Rendering ───────────────────────────────────────────────────────
    mesh_color: tuple[float, float, float, float] = (0.0, 1.0, 0.4, 0.95)
    """RGBA colour for the front wireframe.  Default: crisp neon green (#00FF66)."""

    front_color: tuple[float, float, float, float] = (0.0, 1.0, 0.4, 0.95)
    """RGBA colour for the front of the face (eyes, nose, mouth)."""

    back_color: tuple[float, float, float, float] = (0.0, 0.4, 0.15, 0.10)
    """RGBA colour for the back/rear layer of the skull (translucent and dim)."""

    wireframe_width: float = float(os.environ.get("CLAW_AVATAR_LINE_WIDTH", "1.0"))
    """GL line width in pixels. Default: 1.0 for a thin, sharp look."""

    target_fps: int = int(os.environ.get("CLAW_AVATAR_FPS", "30"))
    """Frame rate for the render timer (and FaceFormer output rate)."""

    background_color: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    """Fully transparent background for the overlay."""

    # ── Window ──────────────────────────────────────────────────────────
    window_width: int = int(os.environ.get("CLAW_AVATAR_WIDTH", "1000"))
    window_height: int = int(os.environ.get("CLAW_AVATAR_HEIGHT", "1000"))

    window_x: int = int(os.environ.get("CLAW_AVATAR_X", "-1"))
    """X position on screen.  -1 = auto (bottom-right corner)."""

    window_y: int = int(os.environ.get("CLAW_AVATAR_Y", "-1"))
    """Y position on screen.  -1 = auto (bottom-right corner)."""

    window_scale: float = float(os.environ.get("CLAW_AVATAR_SCALE", "1.5"))
    """Extra uniform scale applied to the mesh inside the viewport (default: 1.5x)."""

    # ── Model / Weights ─────────────────────────────────────────────────
    model_path: str = ""
    """Path to FaceFormer checkpoint (vocaset.pth).  Empty → default."""

    template_path: str = ""
    """Path to FLAME_sample.ply.  Empty → default."""

    templates_pkl_path: str = ""
    """Path to templates.pkl.  Empty → default."""

    device: str = "mps"
    """PyTorch device.  'mps' for Apple Silicon, 'cpu' as fallback."""

    # ── Audio ───────────────────────────────────────────────────────────
    audio_sample_rate: int = 16000
    """Target sample rate expected by wav2vec2."""

    wav2vec_model: str = "facebook/wav2vec2-base-960h"
    """HuggingFace model id for the audio encoder."""

    # ── Idle Animation ──────────────────────────────────────────────────
    idle_animation: bool = True
    """Enable subtle 'breathing' displacement when not speaking."""

    idle_amplitude: float = 0.0008
    """Peak displacement (in mesh units) for the idle breathing cycle."""

    idle_period: float = 3.5
    """Full breathing period in seconds."""

    # ── FaceFormer hyper-params (inference-only) ────────────────────────
    feature_dim: int = 64
    vertice_dim: int = 15069  # 5023 vertices × 3
    period: int = 30
    fps: int = 30
    num_train_subjects: int = 8
    """Number of one-hot training identities (vocaset default = 8)."""

    condition_idx: int = 0
    """Index into the one-hot speaker identity vector (style control)."""

    subject: str = "FaceTalk_170728_03272_TA"
    """Identity key for template lookup in templates.pkl."""

    # ── Derived helpers ─────────────────────────────────────────────────
    def resolve_model_path(self) -> Path:
        if self.model_path:
            return Path(self.model_path)
        return _MODULE_DIR / "models" / "vocaset.pth"

    def resolve_template_path(self) -> Path:
        if self.template_path:
            return Path(self.template_path)
        return _MODULE_DIR / "models" / "FLAME_sample.ply"

    def resolve_templates_pkl_path(self) -> Path:
        if self.templates_pkl_path:
            return Path(self.templates_pkl_path)
        return _MODULE_DIR / "models" / "templates.pkl"

    @property
    def timer_interval_ms(self) -> int:
        """Milliseconds between render ticks."""
        return max(1, int(1000 / self.target_fps))
