"""
Transparent click-through overlay window for macOS.

Creates a frameless PySide6 window with full alpha transparency that
floats above all other applications.  Mouse and keyboard events pass
through to the windows underneath (click-through / input-transparent).

The window hosts a ``MeshGLWidget`` (from mesh_renderer.py) as its
central widget and delegates all rendering to OpenGL.
"""

from __future__ import annotations

import logging
import platform
from typing import TYPE_CHECKING

from PySide6.QtCore import Qt
from PySide6.QtGui import QScreen
from PySide6.QtWidgets import QApplication, QMainWindow

if TYPE_CHECKING:
    from modules.avatar_overlay.config import AvatarConfig
    from modules.avatar_overlay.mesh_renderer import MeshGLWidget

logger = logging.getLogger(__name__)


class OverlayWindow(QMainWindow):
    """Frameless, transparent, always-on-top, click-through overlay."""

    def __init__(self, config: "AvatarConfig", gl_widget: "MeshGLWidget"):
        super().__init__()
        self._config = config

        # ── Window flags ────────────────────────────────────────────
        self.setWindowFlags(
            Qt.WindowType.FramelessWindowHint
            | Qt.WindowType.WindowStaysOnTopHint
            | Qt.WindowType.WindowTransparentForInput
        )

        # ── Transparency & macOS visibility ────────────────────────
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.setAttribute(Qt.WidgetAttribute.WA_NoSystemBackground, True)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating, True)

        # ── Geometry ────────────────────────────────────────────────
        self._apply_geometry(config)

        # ── Central GL widget ───────────────────────────────────────
        self.setCentralWidget(gl_widget)

        self.setWindowTitle("AvatarOverlay")

    # ── Geometry helpers ────────────────────────────────────────────

    def _apply_geometry(self, config: "AvatarConfig") -> None:
        """Position and size the window according to config."""
        w = config.window_width
        h = config.window_height

        if config.window_x >= 0 and config.window_y >= 0:
            self.setGeometry(config.window_x, config.window_y, w, h)
        else:
            # Auto-place: bottom-right corner with 40 px margin
            screen = self._primary_screen()
            if screen is not None:
                geom = screen.availableGeometry()
                x = geom.right() - w - 40
                y = geom.bottom() - h - 40
                self.setGeometry(x, y, w, h)
            else:
                self.resize(w, h)

    @staticmethod
    def _primary_screen() -> QScreen | None:
        app = QApplication.instance()
        if app is not None:
            return app.primaryScreen()
        return None
