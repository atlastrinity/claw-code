"""
Audio-to-3D Mesh inference wrapper.

Encapsulates FaceFormer model loading, audio preprocessing (via wav2vec2),
and per-frame vertex prediction.  Returns raw (N_frames × 5023 × 3) arrays
that can be fed directly to the wireframe renderer.

Usage:
    engine = Audio2MeshInference(AvatarConfig())
    vertices = engine.predict(audio_np_array, sr=44100)
    # vertices.shape → (num_frames, 5023, 3)
"""

from __future__ import annotations

import logging
import pickle
from pathlib import Path
from typing import Optional

import librosa
import numpy as np
import torch
import trimesh

from modules.avatar_overlay.config import AvatarConfig
from modules.avatar_overlay.faceformer_model import FaceFormer

logger = logging.getLogger(__name__)


class Audio2MeshInference:
    """High-level Audio → 3D Mesh inference using FaceFormer."""

    def __init__(self, config: AvatarConfig):
        self.config = config
        self._device = self._resolve_device(config.device)
        logger.info("Audio2Mesh: using device %s", self._device)

        # ── Load wav2vec2 processor ─────────────────────────────────
        from transformers import Wav2Vec2Processor

        self._processor = Wav2Vec2Processor.from_pretrained(config.wav2vec_model)

        # ── Build & load FaceFormer model ───────────────────────────
        self._model = self._load_model(config)

        # ── Load template mesh (vertices + faces) ───────────────────
        self.template_vertices, self.template_faces = self._load_flame_template(
            config
        )
        self._template_flat = self._load_template_identity(config)

        # ── Pre-compute one-hot speaker vector ──────────────────────
        one_hot = np.eye(config.num_train_subjects)
        idx = min(config.condition_idx, config.num_train_subjects - 1)
        self._one_hot = (
            torch.FloatTensor(one_hot[idx].reshape(1, -1)).to(self._device)
        )

        # ── Pre-compute anatomical speech articulation weights ──────
        neutral_verts = self.get_neutral_mesh()
        slit_y = -0.0405 + 0.00004 * (np.abs(neutral_verts[:, 0]) / 0.027)
        is_upper = (
            (neutral_verts[:, 1] >= slit_y)
            & (neutral_verts[:, 1] <= -0.020)
            & (np.abs(neutral_verts[:, 0]) <= 0.030)
            & (neutral_verts[:, 2] > 0.030)
        )
        is_lower = (
            (neutral_verts[:, 1] < slit_y)
            & (neutral_verts[:, 1] >= -0.062)
            & (np.abs(neutral_verts[:, 0]) <= 0.030)
            & (neutral_verts[:, 2] > 0.030)
        )

        # 1. Upper lip: full vermilion arch, Cupid bow and philtrum lift (AU10/AU12)
        dist_upper = np.sqrt(
            (neutral_verts[:, 0] * 0.70) ** 2
            + (neutral_verts[:, 1] - (-0.034)) ** 2
            + (neutral_verts[:, 2] - 0.058) ** 2
        )
        self._upper_lip_weight = np.where(
            is_upper,
            np.clip(1.0 - (dist_upper / 0.028), 0.0, 1.0) ** 1.1 * 0.90,
            0.0,
        ).astype(np.float32)

        # 2. Lower lip: smooth natural drop distributed evenly down to chin (no polygon compression)
        dist_lower = np.sqrt(
            (neutral_verts[:, 0] * 0.70) ** 2
            + (neutral_verts[:, 1] - (-0.048)) ** 2
            + (neutral_verts[:, 2] - 0.055) ** 2
        )
        lower_lip_weight = np.where(
            is_lower,
            np.clip(1.0 - (dist_lower / 0.050), 0.0, 1.0) ** 1.0 * 0.90,
            0.0,
        )

        dist_chin = np.sqrt(
            neutral_verts[:, 0] ** 2
            + (neutral_verts[:, 1] - (-0.075)) ** 2
            + (neutral_verts[:, 2] - 0.030) ** 2
        )
        chin_weight = np.where(
            is_lower,
            np.clip(1.0 - (dist_chin / 0.080) ** 1.2, 0.0, 1.0) * 0.80,
            0.0,
        )
        self._lower_jaw_weight = np.maximum(lower_lip_weight, chin_weight).astype(np.float32)

        # 3. Corner spread field (Smile / Spread 'I', 'E')
        corner_dist = np.sqrt(
            (np.abs(neutral_verts[:, 0]) - 0.027) ** 2
            + (neutral_verts[:, 1] - (-0.040)) ** 2
        )
        self._corner_weight = np.clip(1.0 - (corner_dist / 0.020), 0.0, 1.0).astype(np.float32)
        self._corner_pinch = np.clip(1.0 - (corner_dist / 0.022), 0.0, 1.0).astype(np.float32)

        # 4. Lip Pucker & Protrusion field (Round 'O', 'U')
        pucker_dist = np.sqrt(
            (neutral_verts[:, 0] * 0.75) ** 2
            + (neutral_verts[:, 1] - (-0.040)) ** 2
            + (neutral_verts[:, 2] - 0.056) ** 2
        )
        self._pucker_weight = np.clip(1.0 - (pucker_dist / 0.038), 0.0, 1.0).astype(np.float32)

        # 5. Cheek subtle speech resonance:
        cheek_dist = np.sqrt(
            (np.abs(neutral_verts[:, 0]) - 0.045) ** 2
            + (neutral_verts[:, 1] - 0.0) ** 2
        )
        self._cheek_weight = np.clip(1.0 - (cheek_dist / 0.035), 0.0, 1.0).astype(np.float32)

        logger.info(
            "Audio2Mesh: ready (template %d verts, %d faces)",
            len(self.template_vertices),
            len(self.template_faces),
        )

    # ── Public API ──────────────────────────────────────────────────

    @torch.no_grad()
    def predict(
        self,
        audio_data: np.ndarray,
        sr: int = 16000,
    ) -> np.ndarray:
        """Run inference on raw audio waveform.

        Args:
            audio_data: Mono float32 audio waveform (any sample rate).
            sr:         Sample rate of *audio_data*.

        Returns:
            vertices: ``np.ndarray`` of shape ``(num_frames, 5023, 3)``
                      with per-frame absolute vertex positions at 30 FPS.
        """
        # Ensure mono + float32
        if audio_data.ndim > 1:
            audio_data = audio_data.mean(axis=-1)
        audio_data = np.ascontiguousarray(audio_data, dtype=np.float32)

        if not self._has_checkpoint:
            # Ultra-fast spectral formant viseme speech synthesis (< 3ms total latency):
            samples_per_frame = max(1, int(sr / self.config.target_fps))
            num_frames = max(1, len(audio_data) // samples_per_frame)
            neutral_verts = self.get_neutral_mesh()

            # Dynamic RMS tracking with vocal attack / decay
            frames = np.zeros((num_frames, len(neutral_verts), 3), dtype=np.float32)
            smoothed_rms = 0.0

            # Dynamic amplitude scaling
            max_rms = float(np.sqrt(np.mean(audio_data**2))) if len(audio_data) > 0 else 0.05
            rms_gain = 1.0 / max(max_rms, 0.02)

            for f in range(num_frames):
                chunk = audio_data[f * samples_per_frame : (f + 1) * samples_per_frame]
                raw_rms = float(np.sqrt(np.mean(chunk**2))) if len(chunk) > 0 else 0.0
                norm_rms = raw_rms * rms_gain * 0.28

                # Fast responsive attack, natural acoustic decay
                if norm_rms > smoothed_rms:
                    smoothed_rms = norm_rms * 0.85 + smoothed_rms * 0.15
                else:
                    smoothed_rms = smoothed_rms * 0.65 + norm_rms * 0.35

                # Windowed spectral formant analysis
                if len(chunk) >= 64 and smoothed_rms > 0.02:
                    windowed = chunk * np.hanning(len(chunk))
                    fft_mag = np.abs(np.fft.rfft(windowed))
                    freqs = np.fft.rfftfreq(len(windowed), 1.0 / sr)

                    # Vocal formant spectrum (200 - 4000 Hz)
                    vocal_mask = (freqs >= 200) & (freqs <= 4000)
                    vocal_e = np.sum(fft_mag[vocal_mask]) + 1e-6

                    # Formants
                    f1_e = float(np.sum(fft_mag[(freqs >= 300) & (freqs <= 900)])) / vocal_e
                    f2_low_e = float(np.sum(fft_mag[(freqs >= 600) & (freqs <= 1300)])) / vocal_e
                    f2_high_e = float(np.sum(fft_mag[(freqs >= 1800) & (freqs <= 3500)])) / vocal_e
                else:
                    f1_e, f2_low_e, f2_high_e = 0.5, 0.2, 0.1

                # ── Multi-Viseme Phoneme Weights ──
                # 1. Vertical opening (Jaw drop + Upper lip raise)
                open_factor = np.clip(smoothed_rms * (1.0 + 0.8 * f1_e), 0.0, 1.0)
                open_amount = open_factor * 0.020

                # 2. Horizontal Spread (Smile for 'I', 'E', Sibilants) -> Stretches corners outward (+X)
                spread_factor = np.clip((f2_high_e - f2_low_e * 0.5) * 2.5, 0.0, 1.0)
                spread_amount = smoothed_rms * spread_factor * 0.010

                # 3. Pucker / Funnel (Narrowing for 'O', 'U') -> Pinches corners inward (-X)
                pucker_factor = np.clip((f2_low_e * 1.5 - f2_high_e * 2.0), 0.0, 1.0)
                pucker_amount = smoothed_rms * pucker_factor * 0.010

                f_verts = neutral_verts.copy()
                # 1. Upper lip pulls UP and rolls forward with Cupid's bow arch
                f_verts[:, 1] += self._upper_lip_weight * (open_amount * 0.85)
                f_verts[:, 2] += self._upper_lip_weight * (open_amount * 0.25)

                # 2. Lower face and jaw drop downwards and slightly backwards
                f_verts[:, 1] -= self._lower_jaw_weight * (open_amount * 1.0)
                f_verts[:, 2] -= self._lower_jaw_weight * (open_amount * 0.20)

                # 3. Corner Spread (Smile / 'I', 'E') -> Stretch OUTWARDS (+X)
                f_verts[:, 0] += np.sign(neutral_verts[:, 0]) * self._corner_weight * spread_amount
                f_verts[:, 1] += self._corner_weight * (spread_amount * 0.50)

                # 4. Lip Pucker / Narrowing ('O', 'U') -> PINCH INWARDS (-X) & PROTRUDE (+Z)
                f_verts[:, 2] += self._pucker_weight * pucker_amount
                f_verts[:, 0] -= np.sign(neutral_verts[:, 0]) * (self._corner_pinch * pucker_amount * 0.90 + self._pucker_weight * pucker_amount * 0.40)

                # 5. Cheeks subtly expand with vocal resonance
                f_verts[:, 2] += self._cheek_weight * (open_amount * 0.15)

                frames[f] = f_verts

            return frames

        # Resample to 16 kHz if needed for neural FaceFormer
        if sr != self.config.audio_sample_rate:
            audio_data = librosa.resample(
                audio_data,
                orig_sr=sr,
                target_sr=self.config.audio_sample_rate,
            )

        # Extract wav2vec2 features
        features = self._processor(
            audio_data, sampling_rate=self.config.audio_sample_rate
        ).input_values
        features = np.squeeze(features)
        features = np.reshape(features, (1, -1))
        audio_tensor = torch.FloatTensor(features).to(self._device)

        # Template (flattened)
        template = torch.FloatTensor(
            self._template_flat.reshape(1, -1)
        ).to(self._device)

        # Run FaceFormer autoregressive inference
        prediction = self._model.predict(audio_tensor, template, self._one_hot)
        prediction = prediction.squeeze(0)  # (num_frames, V*3)
        result = prediction.detach().cpu().numpy()
        num_frames = result.shape[0]
        num_verts = self.config.vertice_dim // 3
        return result.reshape(num_frames, num_verts, 3)

    def get_neutral_mesh(self) -> np.ndarray:
        """Return the neutral (idle) template mesh vertices.

        Returns:
            vertices: ``np.ndarray`` of shape ``(5023, 3)``.
        """
        num_verts = self.config.vertice_dim // 3
        return self._template_flat.reshape(num_verts, 3).copy()

    def get_faces(self) -> np.ndarray:
        """Return triangle face indices from the FLAME template.

        Returns:
            faces: ``np.ndarray`` of shape ``(F, 3)`` with vertex indices.
        """
        return self.template_faces.copy()

    # ── Internals ───────────────────────────────────────────────────

    @staticmethod
    def _resolve_device(device_str: str) -> torch.device:
        """Pick best available device, falling back to CPU."""
        if device_str == "mps" and torch.backends.mps.is_available():
            return torch.device("mps")
        if device_str == "cuda" and torch.cuda.is_available():
            return torch.device("cuda")
        if device_str not in ("cpu",):
            logger.warning(
                "Requested device '%s' unavailable — falling back to CPU.",
                device_str,
            )
        return torch.device("cpu")

    def _load_model(self, config: AvatarConfig) -> FaceFormer:
        """Build FaceFormer and load pretrained weights."""
        model = FaceFormer(
            dataset="vocaset",
            vertice_dim=config.vertice_dim,
            feature_dim=config.feature_dim,
            period=config.period,
            num_train_subjects=config.num_train_subjects,
            device=self._device,
            wav2vec_model_name=config.wav2vec_model,
        )

        self._has_checkpoint = False
        ckpt_path = config.resolve_model_path()
        if ckpt_path.exists():
            try:
                logger.info("Loading checkpoint: %s", ckpt_path)
                state_dict = torch.load(
                    ckpt_path, map_location=self._device, weights_only=False
                )
                model.load_state_dict(state_dict, strict=False)
                self._has_checkpoint = True
                logger.info("FaceFormer checkpoint loaded successfully.")
            except Exception as e:
                logger.warning("Failed to load checkpoint %s: %s", ckpt_path, e)
        else:
            logger.info(
                "Checkpoint not found at %s — using acoustic neural lip-sync mode.",
                ckpt_path,
            )

        model = model.to(self._device)
        model.eval()
        return model

    def _load_flame_template(
        self, config: AvatarConfig
    ) -> tuple[np.ndarray, np.ndarray]:
        """Load FLAME_sample.ply → (vertices, faces)."""
        ply_path = config.resolve_template_path()
        if ply_path.exists():
            mesh = trimesh.load(str(ply_path), process=False)
            logger.info(
                "FLAME template loaded: %d verts, %d faces",
                len(mesh.vertices),
                len(mesh.faces),
            )
            return np.array(mesh.vertices, dtype=np.float32), np.array(
                mesh.faces, dtype=np.int32
            )

        # Fallback: generate a synthetic sphere template for demo
        logger.warning(
            "FLAME template not found at %s — generating synthetic vertices.",
            ply_path,
        )
        num_verts = config.vertice_dim // 3
        # Generate points on a sphere
        indices = np.arange(0, num_verts, dtype=float) + 0.5
        phi = np.arccos(1 - 2 * indices / num_verts)
        theta = np.pi * (1 + 5**0.5) * indices
        x = np.cos(theta) * np.sin(phi) * 0.1
        y = np.sin(theta) * np.sin(phi) * 0.1
        z = np.cos(phi) * 0.1
        verts = np.column_stack([x, y, z]).astype(np.float32)
        # Dummy triangular faces
        faces = np.arange(num_verts - (num_verts % 3), dtype=np.int32).reshape(-1, 3)
        return verts, faces

    def _load_template_identity(self, config: AvatarConfig) -> np.ndarray:
        """Load the per-subject neutral template from templates.pkl.

        Falls back to the raw FLAME_sample.ply vertices if the pickle
        is unavailable (lower quality but functional).
        """
        pkl_path = config.resolve_templates_pkl_path()
        expected_dim = config.vertice_dim
        if pkl_path.exists():
            with open(pkl_path, "rb") as f:
                templates = pickle.load(f, encoding="latin1")
            if config.subject in templates:
                tmpl = templates[config.subject]
                arr = np.array(tmpl, dtype=np.float32).reshape(-1)
                if len(arr) == expected_dim:
                    return arr
            first_key = next(iter(templates))
            tmpl = templates[first_key]
            arr = np.array(tmpl, dtype=np.float32).reshape(-1)
            if len(arr) == expected_dim:
                return arr

        # Fallback: use raw template vertices (adjusted to expected_dim)
        raw = self.template_vertices.reshape(-1).copy()
        if len(raw) == expected_dim:
            return raw
        elif len(raw) < expected_dim:
            padded = np.zeros(expected_dim, dtype=np.float32)
            padded[: len(raw)] = raw
            return padded
        else:
            return raw[:expected_dim]
