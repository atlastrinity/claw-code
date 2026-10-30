"""
FaceFormer model architecture — ported from EvelynFan/FaceFormer (CVPR 2022).

Adapted for:
  • PyTorch ≥ 2.0 compatibility (removed deprecated APIs)
  • MPS (Apple Silicon) device support
  • Inference-only mode (no training artefacts)
  • Typed arguments via AvatarConfig instead of argparse.Namespace

Original: https://github.com/EvelynFan/FaceFormer/blob/main/faceformer.py
Paper:    "FaceFormer: Speech-Driven 3D Facial Animation with Transformers"
"""

from __future__ import annotations

import copy
import math
from typing import Optional

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from transformers import Wav2Vec2Model


# ── Temporal Bias (ALiBi-inspired) ──────────────────────────────────


def _get_slopes(n: int) -> list[float]:
    """Compute attention-bias slopes for *n* heads (ALiBi-style)."""

    def _power_of_two(n: int) -> list[float]:
        start = 2 ** (-(2 ** -(math.log2(n) - 3)))
        ratio = start
        return [start * ratio**i for i in range(n)]

    if math.log2(n).is_integer():
        return _power_of_two(n)
    closest = 2 ** math.floor(math.log2(n))
    return _power_of_two(closest) + _get_slopes(2 * closest)[0::2][: n - closest]


def init_biased_mask(n_head: int, max_seq_len: int, period: int) -> torch.Tensor:
    """Build a causal + periodic ALiBi attention mask."""
    slopes = torch.Tensor(_get_slopes(n_head))
    bias = (
        torch.arange(start=0, end=max_seq_len, step=period)
        .unsqueeze(1)
        .repeat(1, period)
        .view(-1)
        // period
    )
    bias = -torch.flip(bias, dims=[0])

    alibi = torch.zeros(max_seq_len, max_seq_len)
    for i in range(max_seq_len):
        alibi[i, : i + 1] = bias[-(i + 1) :]

    alibi = slopes.unsqueeze(1).unsqueeze(1) * alibi.unsqueeze(0)

    mask = torch.triu(torch.ones(max_seq_len, max_seq_len)).transpose(0, 1)
    mask = mask.float().masked_fill(mask == 0, float("-inf")).masked_fill(mask == 1, 0.0)
    mask = mask.unsqueeze(0) + alibi
    return mask


# ── Alignment Bias ──────────────────────────────────────────────────


def enc_dec_mask(
    device: torch.device, dataset: str, T: int, S: int
) -> torch.Tensor:
    """Encoder–decoder alignment mask.

    For *vocaset* (30 FPS, 1:1 audio-to-mesh alignment) each mesh frame
    attends to a single audio frame.  For *BIWI* (25 FPS, 1:2) each mesh
    frame attends to two consecutive audio frames.
    """
    mask = torch.ones(T, S)
    if dataset == "BIWI":
        for i in range(T):
            mask[i, i * 2 : i * 2 + 2] = 0
    elif dataset == "vocaset":
        for i in range(T):
            mask[i, i] = 0
    return (mask == 1).to(device=device)


# ── Periodic Positional Encoding ────────────────────────────────────


class PeriodicPositionalEncoding(nn.Module):
    def __init__(
        self,
        d_model: int,
        dropout: float = 0.1,
        period: int = 25,
        max_seq_len: int = 600,
    ):
        super().__init__()
        self.dropout = nn.Dropout(p=dropout)
        pe = torch.zeros(period, d_model)
        position = torch.arange(0, period, dtype=torch.float).unsqueeze(1)
        div_term = torch.exp(
            torch.arange(0, d_model, 2).float() * (-math.log(10000.0) / d_model)
        )
        pe[:, 0::2] = torch.sin(position * div_term)
        pe[:, 1::2] = torch.cos(position * div_term)
        pe = pe.unsqueeze(0)  # (1, period, d_model)
        # Tile to cover max_seq_len
        num_repeats = max_seq_len // period + 1
        pe = pe.repeat(1, num_repeats, 1)[:, :max_seq_len, :]
        self.register_buffer("pe", pe)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = x + self.pe[:, : x.size(1), :]
        return self.dropout(x)


# ── FaceFormer Model ───────────────────────────────────────────────


class FaceFormer(nn.Module):
    """Speech-driven 3D facial animation via a Transformer decoder.

    Architecture:
      wav2vec2 (frozen) → linear proj → Transformer decoder → vertex offsets

    The decoder autoregressively predicts per-frame vertex displacements
    relative to a neutral template mesh.
    """

    def __init__(
        self,
        *,
        dataset: str = "vocaset",
        vertice_dim: int = 15069,
        feature_dim: int = 64,
        period: int = 30,
        num_train_subjects: int = 8,
        device: torch.device | str = "cpu",
        wav2vec_model_name: str = "facebook/wav2vec2-base-960h",
    ):
        super().__init__()
        self.dataset = dataset
        self.vertice_dim = vertice_dim
        self.feature_dim = feature_dim

        # ── Audio encoder (frozen wav2vec2) ─────────────────────────
        self.audio_encoder = Wav2Vec2Model.from_pretrained(wav2vec_model_name)
        self.audio_encoder.feature_extractor._freeze_parameters()
        self.audio_feature_map = nn.Linear(768, feature_dim)

        # ── One-hot speaker conditioning ────────────────────────────
        self.obj_vector = nn.Linear(num_train_subjects, feature_dim, bias=False)

        # ── Vertex I/O projections ──────────────────────────────────
        self.vertice_map = nn.Linear(vertice_dim, feature_dim)
        self.vertice_map_r = nn.Linear(feature_dim, vertice_dim)

        # ── Positional encoding ─────────────────────────────────────
        self.PPE = PeriodicPositionalEncoding(feature_dim, period=period)

        # ── Biased causal mask ──────────────────────────────────────
        self.biased_mask = init_biased_mask(
            n_head=4, max_seq_len=600, period=period
        )

        # ── Transformer decoder ─────────────────────────────────────
        decoder_layer = nn.TransformerDecoderLayer(
            d_model=feature_dim,
            nhead=4,
            dim_feedforward=2 * feature_dim,
            batch_first=True,
        )
        self.transformer_decoder = nn.TransformerDecoder(
            decoder_layer, num_layers=1
        )

    def forward(
        self,
        audio: torch.Tensor,
        template: torch.Tensor,
        one_hot: torch.Tensor,
        vertice: Optional[torch.Tensor] = None,
    ) -> torch.Tensor:
        """Training forward pass (teacher-forced)."""
        # Not used at inference — kept for checkpoint compatibility.
        raise NotImplementedError("Use .predict() for inference.")

    @torch.no_grad()
    def predict(
        self,
        audio: torch.Tensor,
        template: torch.Tensor,
        one_hot: torch.Tensor,
    ) -> torch.Tensor:
        """Autoregressive inference: audio → sequence of vertex offsets.

        Args:
            audio:    (1, T_audio) raw waveform features from wav2vec2 processor.
            template: (1, V*3)  neutral template vertices (flattened).
            one_hot:  (1, num_subjects) speaker-style one-hot vector.

        Returns:
            vertices: (1, num_frames, V*3)  per-frame vertex positions.
        """
        device = audio.device

        # Encode audio
        hidden_states = self.audio_encoder(
            audio, self.dataset, output_hidden_states=True
        ).last_hidden_state  # (1, T_enc, 768)
        frame_num = hidden_states.shape[1]

        hidden_states = self.audio_feature_map(hidden_states)  # (1, T_enc, feat)

        # Style embedding
        obj_embedding = self.obj_vector(one_hot)  # (1, feat)

        # Seed with template
        vertice_emb = obj_embedding.unsqueeze(1)  # (1, 1, feat)
        style_emb = vertice_emb  # cache

        all_vertices = []

        for i in range(frame_num):
            if i == 0:
                vertice_input = self.PPE(style_emb)
            else:
                vertice_input = self.PPE(vertice_emb)

            tgt_mask = self.biased_mask[: 4, : vertice_input.shape[1], : vertice_input.shape[1]].clone().detach().to(device)

            memory_mask = enc_dec_mask(
                device, self.dataset, vertice_input.shape[1], frame_num
            )

            out = self.transformer_decoder(
                vertice_input,
                hidden_states,
                tgt_mask=tgt_mask,
                memory_mask=memory_mask,
            )
            out = self.vertice_map_r(out)  # (1, seq, V*3)

            new_output = out[:, -1:, :]  # last predicted frame
            new_vertice = template.unsqueeze(1) + new_output  # add to template
            all_vertices.append(new_vertice)

            # Feed back
            new_emb = self.vertice_map(new_vertice) + style_emb
            vertice_emb = torch.cat([vertice_emb, new_emb], dim=1)

        vertices = torch.cat(all_vertices, dim=1)  # (1, T, V*3)
        return vertices


# ── Wav2Vec2 monkey-patch ──────────────────────────────────────────
# The original FaceFormer passes `dataset` as a positional arg to
# Wav2Vec2Model.forward(), which the HuggingFace API does not accept.
# We wrap the model to silently ignore that extra argument.

_orig_wav2vec2_forward = Wav2Vec2Model.forward


def _patched_wav2vec2_forward(self, input_values, *args, **kwargs):
    """Drop any extra positional args (like `dataset`) silently."""
    # Keep only known kwargs
    accepted = {
        "attention_mask",
        "mask_time_indices",
        "output_attentions",
        "output_hidden_states",
        "return_dict",
    }
    filtered = {k: v for k, v in kwargs.items() if k in accepted}
    return _orig_wav2vec2_forward(self, input_values, **filtered)


Wav2Vec2Model.forward = _patched_wav2vec2_forward
