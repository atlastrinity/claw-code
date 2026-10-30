"""
Main controller / orchestrator for the Avatar Overlay module.

Manages the lifecycle of:
  1. **Audio2MeshInference** — neural network inference (background thread)
  2. **OverlayWindow** + **MeshGLWidget** — transparent overlay + renderer
  3. **QApplication** event loop

Data flow:
  TTS callback → inference QThread → frame queue → GL render timer

Usage from external code
------------------------
    from modules.avatar_overlay import AvatarOverlayModule

    avatar = AvatarOverlayModule()
    avatar.start()                            # blocks on QApp (or threaded)
    avatar.on_speech_start(audio_np_array)     # from TTS callback
    avatar.on_speech_end()                     # transition to idle
    avatar.stop()
"""

from __future__ import annotations

import logging
import queue
import sys
import threading
from typing import TYPE_CHECKING, Optional

import numpy as np

from modules.avatar_overlay.config import AvatarConfig

if TYPE_CHECKING:
    from PySide6.QtWidgets import QApplication
    from modules.avatar_overlay.audio2mesh import Audio2MeshInference
    from modules.avatar_overlay.mesh_renderer import MeshGLWidget
    from modules.avatar_overlay.overlay_window import OverlayWindow

logger = logging.getLogger(__name__)


class _InferenceWorker:
    """Runs Audio2MeshInference.predict() in a background thread.

    Results (per-frame vertex arrays) are pushed to the GL widget's
    ``frame_queue`` one frame at a time, paced to ~30 FPS delivery.
    """

    def __init__(self, config: AvatarConfig):
        self._config = config
        self._engine: Optional[Audio2MeshInference] = None
        self._thread: Optional[threading.Thread] = None
        self._stop_event = threading.Event()

    def initialise_engine(self) -> None:
        """Heavy init: loads model weights (call once, on startup)."""
        from modules.avatar_overlay.audio2mesh import Audio2MeshInference

        self._engine = Audio2MeshInference(self._config)

    @property
    def engine(self) -> Audio2MeshInference:
        assert self._engine is not None, "Call initialise_engine() first."
        return self._engine

    def submit(
        self,
        audio_data: np.ndarray,
        sr: int,
        gl_widget: "MeshGLWidget",
        start_time: float,
    ) -> None:
        """Submit an audio buffer for instant async lip-sync inference.

        Predicted vertex animation is immediately dispatched to *gl_widget*
        and played strictly synchronized to *start_time*.
        """
        import time

        # Cancel any previous job
        self._stop_event.set()
        if self._thread is not None and self._thread.is_alive():
            self._thread.join(timeout=0.5)

        self._stop_event.clear()

        def _worker() -> None:
            try:
                vertices = self.engine.predict(audio_data, sr=sr)
                if self._stop_event.is_set():
                    return
                gl_widget.play_animation_sequence(
                    vertices,
                    fps=self._config.target_fps,
                    start_time=start_time,
                )
                logger.debug(
                    "Inference complete: %d frames synced (latency=%.1f ms).",
                    vertices.shape[0],
                    (time.monotonic() - start_time) * 1000,
                )
            except Exception:
                logger.exception("Inference worker error.")

        self._thread = threading.Thread(target=_worker, daemon=True, name="avatar-inference")
        self._thread.start()

    def cancel(self) -> None:
        self._stop_event.set()


class AvatarOverlayModule:
    """Top-level facade for the avatar overlay system.

    Parameters
    ----------
    config : AvatarConfig, optional
        Override default settings (colour, position, model path, …).
    """

    def __init__(self, config: Optional[AvatarConfig] = None):
        self._config = config or AvatarConfig()
        self._worker = _InferenceWorker(self._config)
        self._window: Optional["OverlayWindow"] = None
        self._gl_widget: Optional["MeshGLWidget"] = None
        self._app: Optional["QApplication"] = None
        self._ready_event = threading.Event()
        self._started = False

    # ── Lifecycle ───────────────────────────────────────────────────

    def start(self, blocking: bool = True) -> None:
        """Initialise the model, open the overlay, and enter the event loop.

        Args:
            blocking: If ``True`` (default), this call blocks until the
                      window is closed.  Set to ``False`` to run the
                      QApplication on a background thread (useful when
                      integrating with an existing event loop).
        """
        if self._started:
            logger.warning("AvatarOverlayModule is already running.")
            return

        if blocking:
            self._run()
        else:
            t = threading.Thread(target=self._run, daemon=True, name="avatar-overlay")
            t.start()

    def _run(self) -> None:
        """Internal: full init + event loop."""
        from PySide6.QtWidgets import QApplication

        from modules.avatar_overlay.mesh_renderer import MeshGLWidget
        from modules.avatar_overlay.overlay_window import OverlayWindow

        # ── 1. Load model (heavy) ──────────────────────────────────
        logger.info("Loading FaceFormer model…")
        self._worker.initialise_engine()
        engine = self._worker.engine

        neutral = engine.get_neutral_mesh()
        faces = engine.get_faces()
        logger.info(
            "Model ready: %d vertices, %d faces.", len(neutral), len(faces)
        )

        # ── 2. Create QApplication (if not already running) ────────
        self._app = QApplication.instance() or QApplication(sys.argv)

        # ── 3. Create GL widget + overlay window ───────────────────
        self._gl_widget = MeshGLWidget(
            self._config,
            initial_vertices=neutral,
            faces=faces,
        )
        self._window = OverlayWindow(self._config, self._gl_widget)
        self._window.show()

        # ── 4. Start render timer ──────────────────────────────────
        self._gl_widget.start_render_loop()

        self._started = True
        self._ready_event.set()
        logger.info("Avatar overlay started (FPS=%d).", self._config.target_fps)

        # ── 5. Enter event loop ────────────────────────────────────
        self._app.exec()

    # ── TTS Integration ─────────────────────────────────────────────

    def on_speech_start(
        self,
        audio_data: np.ndarray,
        sample_rate: int = 16000,
    ) -> None:
        """Feed audio from TTS to the inference pipeline.

        This method is **thread-safe** and designed to be called from
        the TTS module's callback thread.

        Args:
            audio_data: Mono audio waveform as a float32 numpy array.
            sample_rate: Sample rate of the audio (default 16 kHz).
        """
        import time

        if not self._started:
            if not self._ready_event.wait(timeout=10.0):
                logger.warning("on_speech_start called before overlay became ready.")
                return

        if self._gl_widget is None:
            return

        start_time = time.monotonic()
        self._worker.submit(audio_data, sample_rate, self._gl_widget, start_time)

    def on_speech_end(self) -> None:
        """Signal that speech has finished — transition to idle pose."""
        if self._gl_widget is not None:
            self._gl_widget.clear_animation()

    def stop(self) -> None:
        """Clean shutdown: cancel inference, close window, quit app."""
        self._worker.cancel()

        if self._gl_widget is not None:
            self._gl_widget.stop_render_loop()

        if self._window is not None:
            self._window.close()

        if self._app is not None:
            self._app.quit()

        self._started = False
        logger.info("Avatar overlay stopped.")
