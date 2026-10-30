"""
OpenGL wireframe mesh renderer.

Renders a FLAME-topology face mesh as a neon-green wireframe (GL_LINES)
inside a ``QOpenGLWidget``.  The widget is embedded in the transparent
overlay window and redraws on a fixed-interval timer driven by the
controller.

Key design points
-----------------
* Uses raw OpenGL (via PyOpenGL) through the PySide6 ``QOpenGLWidget``
  surface — this avoids a separate GL context and plays nicely with the
  macOS compositor.
* Edge list is pre-computed from triangle faces at init time.
* Vertex buffer is updated every frame from a ``queue.Queue`` fed by the
  inference thread.
"""

from __future__ import annotations

import logging
import math
import queue
import time
from typing import Optional

import numpy as np
from OpenGL import GL
from PySide6.QtCore import Qt, QTimer
from PySide6.QtOpenGLWidgets import QOpenGLWidget

from modules.avatar_overlay.config import AvatarConfig

logger = logging.getLogger(__name__)


def _faces_to_edges(
    faces: np.ndarray,
    initial_vertices: Optional[np.ndarray] = None,
) -> np.ndarray:
    """Convert triangle face array (F×3) to unique edge index array (E×2).

    Filters out internal eyeball spheres (vertices >= 3931), deep interior
    throat geometry, and slit-bridging edges to produce a clean, seamless
    resting face and a realistic dark hollow mouth cavity during speech.
    """
    if len(faces) == 0:
        return np.zeros((0, 2), dtype=np.uint32)

    # 1. Separate head skin faces (< 3931) and eyeball faces (>= 3931)
    if initial_vertices is not None and len(initial_vertices) > 3931:
        v = initial_vertices
        head_mask = (faces < 3931).all(axis=1)
        head_faces = faces[head_mask]

        # 2. Filter out deep throat, internal oral cavity, gums, and corner spikes
        mouth_z_thresh = np.where(np.abs(v[:, 0]) > 0.016, 0.043, 0.048)
        is_inner_oral = (
            (np.abs(v[:, 0]) <= 0.027)
            & (v[:, 1] > -0.055)
            & (v[:, 1] < -0.025)
            & (v[:, 2] < mouth_z_thresh)
        )
        is_throat = (
            (np.abs(v[:, 0]) < 0.020)
            & (v[:, 1] > -0.060)
            & (v[:, 1] < -0.025)
            & (v[:, 2] < 0.030)
        )
        clean_head_faces = [
            f for f in head_faces
            if not (any(is_inner_oral[vi] for vi in f) or any(is_throat[vi] for vi in f))
        ]

        # 3. Include visible front iris & pupil rings (eyeball faces with Z >= 0.034)
        eye_mask = (faces >= 3931).all(axis=1)
        eye_faces = faces[eye_mask]
        clean_eye_faces = [f for f in eye_faces if all(v[vi, 2] >= 0.034 for vi in f)]

        clean_faces = list(clean_head_faces) + list(clean_eye_faces)
    else:
        v = None
        clean_faces = faces

    edges = set()
    for f in clean_faces:
        a, b, c = int(f[0]), int(f[1]), int(f[2])
        edges.add((min(a, b), max(a, b)))
        edges.add((min(b, c), max(b, c)))
        edges.add((min(c, a), max(c, a)))

    # 3. Filter mouth slit bridging edges for clean dark oral cavity opening
    if v is not None:
        slit_y = -0.0405 + 0.00004 * (np.abs(v[:, 0]) / 0.027)
        is_upper = (
            (v[:, 1] >= slit_y)
            & (v[:, 1] <= -0.020)
            & (np.abs(v[:, 0]) <= 0.030)
            & (v[:, 2] > 0.030)
        )
        is_lower = (
            (v[:, 1] < slit_y)
            & (v[:, 1] >= -0.062)
            & (np.abs(v[:, 0]) <= 0.030)
            & (v[:, 2] > 0.030)
        )

        filtered_edges = set()
        for (a, b) in edges:
            if (is_upper[a] and is_lower[b]) or (is_upper[b] and is_lower[a]):
                continue
            filtered_edges.add((a, b))
        edges = filtered_edges

    result = np.array(sorted(edges), dtype=np.uint32)
    logger.debug("Edge list: %d unique edges from %d clean faces.", len(result), len(clean_faces))
    return result


def _build_mvp(
    width: int,
    height: int,
    scale: float = 1.0,
    mesh_center: Optional[np.ndarray] = None,
) -> np.ndarray:
    """Build a simple orthographic MVP matrix (column-major, float32).

    The projection maps mesh coordinates → NDC so that the entire head
    (including skull top, chin, neck, and ears) fits with comfortable margins.
    """
    aspect = width / max(height, 1)

    # Orthographic bounds (FLAME mesh height is ~0.31: Y in [-0.178, +0.136])
    # Using s = 0.185 gives 85% vertical fill with full margin on top and bottom.
    s = 0.185 / max(scale, 0.01)
    left, right = -s * aspect, s * aspect
    bottom, top_ = -s, s
    near, far = -1.0, 1.0

    # Column-major orthographic matrix
    mvp = np.zeros((4, 4), dtype=np.float32)
    mvp[0, 0] = 2.0 / (right - left)
    mvp[1, 1] = 2.0 / (top_ - bottom)
    mvp[2, 2] = -2.0 / (far - near)
    mvp[3, 0] = -(right + left) / (right - left)
    mvp[3, 1] = -(top_ + bottom) / (top_ - bottom)
    mvp[3, 2] = -(far + near) / (far - near)
    mvp[3, 3] = 1.0

    # Centre the mesh using bounding-box center
    if mesh_center is not None:
        mvp[3, 0] -= mesh_center[0] * mvp[0, 0]
        mvp[3, 1] -= mesh_center[1] * mvp[1, 1]

    return mvp


# ── GLSL Shaders ────────────────────────────────────────────────────

_VERTEX_SHADER = """
#version 120
uniform mat4 mvp;
uniform vec4 front_color;
uniform vec4 back_color;
attribute vec3 in_position;

varying vec4 v_color;

void main() {
    gl_Position = mvp * vec4(in_position, 1.0);
    
    // Depth normalization for FLAME topology [-0.158 .. +0.075]
    // Front face features (nose, lips, eyes, jaw) are Z >= -0.02
    // Back of skull/neck is Z <= -0.10
    float z_norm = clamp((in_position.z - (-0.10)) / (0.075 - (-0.10)), 0.0, 1.0);
    
    // Non-linear ease-in: front facial features are vibrant and distinct,
    // while the back of the head is faint and translucent
    float factor = pow(z_norm, 2.2);
    v_color = mix(back_color, front_color, factor);
}
"""

_FRAGMENT_SHADER = """
#version 120
varying vec4 v_color;

void main() {
    gl_FragColor = v_color;
}
"""


class MeshGLWidget(QOpenGLWidget):
    """OpenGL widget that renders a green wireframe face mesh.

    Frames are consumed from ``frame_queue`` on each timer tick.
    If the queue is empty the last known frame is re-displayed,
    allowing idle / static rendering at zero CPU cost.
    """

    def __init__(
        self,
        config: AvatarConfig,
        initial_vertices: np.ndarray,
        faces: np.ndarray,
        parent=None,
    ):
        super().__init__(parent)
        self._config = config
        self._neutral_vertices = np.ascontiguousarray(initial_vertices, dtype=np.float32)
        self._vertices = self._neutral_vertices.copy()
        self._edges = _faces_to_edges(faces, self._neutral_vertices)

        # Compute true bounding-box centre for camera framing
        bbox_min = self._neutral_vertices.min(axis=0)
        bbox_max = self._neutral_vertices.max(axis=0)
        self._mesh_center = (bbox_min + bbox_max) / 2.0

        # GL object handles (created in initializeGL)
        self._program: int = 0
        self._vbo: int = 0
        self._ebo: int = 0
        self._mvp_loc: int = -1
        self._front_color_loc: int = -1
        self._back_color_loc: int = -1

        self._gl_ready = False

        # Frame queue (legacy queue fallback if needed)
        self.frame_queue: queue.Queue[np.ndarray] = queue.Queue(maxsize=120)

        # Time-synchronized animation state
        self._anim_frames: Optional[np.ndarray] = None
        self._anim_fps: float = float(config.target_fps)
        self._anim_start_time: float = 0.0

        # Idle animation state
        self._idle_phase: float = 0.0
        self._last_tick: float = time.monotonic()
        self._is_idle: bool = True

        # Render timer
        self._timer = QTimer(self)
        self._timer.setTimerType(Qt.TimerType.PreciseTimer)
        self._timer.timeout.connect(self._on_timer_tick)

    # ── Public API ──────────────────────────────────────────────────

    def start_render_loop(self) -> None:
        """Start the fixed-interval render timer."""
        self._timer.start(self._config.timer_interval_ms)

    def stop_render_loop(self) -> None:
        self._timer.stop()

    def set_idle(self, idle: bool) -> None:
        self._is_idle = idle
        if idle:
            self._anim_frames = None

    def play_animation_sequence(
        self,
        frames: np.ndarray,
        fps: float = 30.0,
        start_time: Optional[float] = None,
    ) -> None:
        """Set an animation sequence to be played synchronized to the system clock."""
        # Empty queue of any stale frames
        while not self.frame_queue.empty():
            try:
                self.frame_queue.get_nowait()
            except queue.Empty:
                break

        self._anim_frames = frames
        self._anim_fps = fps
        self._anim_start_time = start_time or time.monotonic()
        self._is_idle = False

    def clear_animation(self) -> None:
        """Immediately reset to neutral idle state."""
        self._anim_frames = None
        self._is_idle = True
        self._vertices = self._neutral_vertices.copy()
        while not self.frame_queue.empty():
            try:
                self.frame_queue.get_nowait()
            except queue.Empty:
                break

    # ── Timer tick ──────────────────────────────────────────────────

    def _on_timer_tick(self) -> None:
        """Update animation frame based on elapsed time and request repaint."""
        now = time.monotonic()

        if self._anim_frames is not None:
            elapsed = now - self._anim_start_time
            idx = int(elapsed * self._anim_fps)
            if 0 <= idx < len(self._anim_frames):
                self._vertices = np.ascontiguousarray(self._anim_frames[idx], dtype=np.float32)
                self._is_idle = False
            elif idx >= len(self._anim_frames):
                # Animation finished
                self._anim_frames = None
                self._is_idle = True
                self._vertices = self._neutral_vertices.copy()

        elif not self.frame_queue.empty():
            try:
                frame = self.frame_queue.get_nowait()
                self._vertices = np.ascontiguousarray(frame, dtype=np.float32)
                self._is_idle = False
            except queue.Empty:
                pass

        if self._is_idle and self._config.idle_animation:
            self._apply_idle_animation(now)

        self.update()  # schedule paintGL

    def _apply_idle_animation(self, now: float) -> None:
        """Apply a subtle sinusoidal 'breathing' displacement without cumulative drift."""
        dt = now - self._last_tick
        self._last_tick = now

        self._idle_phase += dt * (2.0 * math.pi / self._config.idle_period)
        if self._idle_phase > 2.0 * math.pi:
            self._idle_phase -= 2.0 * math.pi

        # Vertical (Y-axis) displacement — simulates breathing
        amplitude = self._config.idle_amplitude * math.sin(self._idle_phase)

        # Apply displacement relative to pristine neutral vertices
        v = self._neutral_vertices.copy()
        v[:, 1] += amplitude
        self._vertices = v

    # ── OpenGL lifecycle ────────────────────────────────────────────

    def initializeGL(self) -> None:
        """Compile shaders, create buffers."""
        GL.glClearColor(0.0, 0.0, 0.0, 0.0)
        GL.glEnable(GL.GL_BLEND)
        GL.glBlendFunc(GL.GL_SRC_ALPHA, GL.GL_ONE_MINUS_SRC_ALPHA)
        GL.glEnable(GL.GL_LINE_SMOOTH)
        GL.glHint(GL.GL_LINE_SMOOTH_HINT, GL.GL_NICEST)
        GL.glLineWidth(self._config.wireframe_width)

        # Compile shaders
        self._program = self._compile_program(_VERTEX_SHADER, _FRAGMENT_SHADER)
        self._mvp_loc = GL.glGetUniformLocation(self._program, "mvp")
        self._front_color_loc = GL.glGetUniformLocation(self._program, "front_color")
        self._back_color_loc = GL.glGetUniformLocation(self._program, "back_color")

        # Vertex buffer
        self._vbo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self._vbo)
        GL.glBufferData(
            GL.GL_ARRAY_BUFFER,
            self._vertices.nbytes,
            self._vertices,
            GL.GL_DYNAMIC_DRAW,
        )

        # Element buffer (edge indices)
        self._ebo = GL.glGenBuffers(1)
        GL.glBindBuffer(GL.GL_ELEMENT_ARRAY_BUFFER, self._ebo)
        edge_data = np.ascontiguousarray(self._edges.flatten(), dtype=np.uint32)
        GL.glBufferData(
            GL.GL_ELEMENT_ARRAY_BUFFER,
            edge_data.nbytes,
            edge_data,
            GL.GL_STATIC_DRAW,
        )

        self._gl_ready = True
        logger.debug("OpenGL initialised with depth fading (program=%d).", self._program)

    def resizeGL(self, w: int, h: int) -> None:
        GL.glViewport(0, 0, w, h)

    def paintGL(self) -> None:
        """Draw the wireframe mesh with depth fading."""
        if not self._gl_ready:
            return

        GL.glClear(GL.GL_COLOR_BUFFER_BIT | GL.GL_DEPTH_BUFFER_BIT)
        GL.glUseProgram(self._program)
        GL.glLineWidth(self._config.wireframe_width)

        # Update MVP
        mvp = _build_mvp(
            self.width(),
            self.height(),
            scale=self._config.window_scale,
            mesh_center=self._mesh_center,
        )
        GL.glUniformMatrix4fv(self._mvp_loc, 1, GL.GL_FALSE, mvp)

        # Set depth-faded colours
        GL.glUniform4f(self._front_color_loc, *self._config.front_color)
        GL.glUniform4f(self._back_color_loc, *self._config.back_color)

        # Upload current vertices
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self._vbo)
        GL.glBufferSubData(
            GL.GL_ARRAY_BUFFER, 0, self._vertices.nbytes, self._vertices
        )

        # Vertex attribute
        pos_loc = GL.glGetAttribLocation(self._program, "in_position")
        GL.glEnableVertexAttribArray(pos_loc)
        GL.glVertexAttribPointer(pos_loc, 3, GL.GL_FLOAT, GL.GL_FALSE, 0, None)

        # Draw edges
        GL.glBindBuffer(GL.GL_ELEMENT_ARRAY_BUFFER, self._ebo)
        GL.glDrawElements(
            GL.GL_LINES,
            self._edges.size,
            GL.GL_UNSIGNED_INT,
            None,
        )

        GL.glDisableVertexAttribArray(pos_loc)
        GL.glUseProgram(0)

    # ── Shader compilation ──────────────────────────────────────────

    @staticmethod
    def _compile_shader(source: str, shader_type: int) -> int:
        shader = GL.glCreateShader(shader_type)
        GL.glShaderSource(shader, source)
        GL.glCompileShader(shader)
        if not GL.glGetShaderiv(shader, GL.GL_COMPILE_STATUS):
            info = GL.glGetShaderInfoLog(shader).decode()
            GL.glDeleteShader(shader)
            raise RuntimeError(f"Shader compilation failed:\n{info}")
        return shader

    @staticmethod
    def _compile_program(vert_src: str, frag_src: str) -> int:
        vert = MeshGLWidget._compile_shader(vert_src, GL.GL_VERTEX_SHADER)
        frag = MeshGLWidget._compile_shader(frag_src, GL.GL_FRAGMENT_SHADER)
        prog = GL.glCreateProgram()
        GL.glAttachShader(prog, vert)
        GL.glAttachShader(prog, frag)
        GL.glLinkProgram(prog)
        if not GL.glGetProgramiv(prog, GL.GL_LINK_STATUS):
            info = GL.glGetProgramInfoLog(prog).decode()
            GL.glDeleteProgram(prog)
            raise RuntimeError(f"Shader link failed:\n{info}")
        GL.glDeleteShader(vert)
        GL.glDeleteShader(frag)
        return prog
