#!/usr/bin/env python3
"""
Standalone test launcher and daemon for the Avatar Overlay module.

Loads a .wav file, runs FaceFormer inference, and displays the animated
green wireframe mesh overlay on screen. In daemon mode, it listens for
speech events from CLAW Voice Narrator via local IPC (UDP).

Usage
-----
    # Daemon mode (used by run_claw.sh):
    python -m modules.avatar_overlay.run_avatar --daemon

    # With a real WAV file:
    python -m modules.avatar_overlay.run_avatar --wav path/to/speech.wav

    # Quick demo (no model weights needed — just shows idle wireframe):
    python -m modules.avatar_overlay.run_avatar --demo

    # With EdgeTTS live synthesis:
    python -m modules.avatar_overlay.run_avatar --tts "Привіт, я Атлас!"
"""

from __future__ import annotations

import argparse
import logging
import os
from pathlib import Path
import signal
import sys
import time

# Ensure project root is in sys.path
_project_root = Path(__file__).resolve().parent.parent.parent
if str(_project_root) not in sys.path:
    sys.path.insert(0, str(_project_root))

import numpy as np


def _setup_logging(verbose: bool = False) -> None:
    level = logging.DEBUG if verbose else logging.INFO
    logging.basicConfig(
        level=level,
        format="%(asctime)s [%(name)s] %(levelname)s: %(message)s",
        datefmt="%H:%M:%S",
    )


def _parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Avatar Overlay — standalone test launcher and daemon"
    )
    mode = p.add_mutually_exclusive_group()
    mode.add_argument(
        "--daemon",
        action="store_true",
        help="Run in background daemon mode listening for TTS speech events via IPC.",
    )
    mode.add_argument(
        "--wav",
        type=str,
        help="Path to a .wav file to animate.",
    )
    mode.add_argument(
        "--tts",
        type=str,
        help="Text to synthesise with EdgeTTS and animate.",
    )
    mode.add_argument(
        "--demo",
        action="store_true",
        help="Just show the idle wireframe (no model weights needed).",
    )
    p.add_argument(
        "--device",
        type=str,
        default=os.environ.get("CLAW_AVATAR_DEVICE", "mps"),
        help="PyTorch device: mps | cuda | cpu  (default: mps)",
    )
    p.add_argument(
        "--scale",
        type=float,
        default=float(os.environ.get("CLAW_AVATAR_SCALE", "1.5")),
        help="Mesh scale factor (default: 1.5).",
    )
    p.add_argument(
        "--fps",
        type=int,
        default=int(os.environ.get("CLAW_AVATAR_FPS", "30")),
        help="Render FPS (default: 30).",
    )
    p.add_argument("-v", "--verbose", action="store_true")
    return p.parse_args()


def _load_wav(path: str) -> tuple[np.ndarray, int]:
    """Load a WAV file and return (audio_data, sample_rate)."""
    import soundfile as sf

    data, sr = sf.read(path, dtype="float32")
    if data.ndim > 1:
        data = data.mean(axis=-1)
    return data, sr


def _synthesise_tts(text: str, rate: str = "-5%") -> tuple[np.ndarray, int]:
    """Synthesise text via EdgeTTSHelper and return (audio_data, sr)."""
    import tempfile

    from edge_tts_wrapper.edge_tts_helper import EdgeTTSHelper

    helper = EdgeTTSHelper()
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        tmp_path = tmp.name

    helper.synthesize_to_wav(text, tmp_path, rate=rate)
    return _load_wav(tmp_path)


def main() -> int:
    args = _parse_args()
    _setup_logging(args.verbose)

    from modules.avatar_overlay.config import AvatarConfig
    from modules.avatar_overlay.controller import AvatarOverlayModule
    from modules.avatar_overlay.ipc import AvatarIPCServer

    config = AvatarConfig(
        device=args.device,
        window_scale=args.scale,
        target_fps=args.fps,
    )

    avatar = AvatarOverlayModule(config)

    # Handle graceful exit signals
    def _sig_handler(signum, frame):
        logging.info("Avatar overlay received termination signal.")
        avatar.stop()
        sys.exit(0)

    signal.signal(signal.SIGINT, _sig_handler)
    signal.signal(signal.SIGTERM, _sig_handler)

    if args.daemon:
        logging.info("Starting Avatar Overlay in IPC daemon mode (device=%s, fps=%d)...", args.device, args.fps)

        def _on_speech_start(wav_path: str, voice: str):
            try:
                if not os.path.exists(wav_path):
                    logging.warning("WAV file not found: %s", wav_path)
                    return
                audio_data, sr = _load_wav(wav_path)
                logging.debug("Playing speech (%s, %d samples @ %d Hz)", voice, len(audio_data), sr)
                avatar.on_speech_start(audio_data, sample_rate=sr)
            except Exception as e:
                logging.error("Failed to process speech start for avatar: %s", e)

        def _on_speech_end():
            avatar.on_speech_end()

        ipc_server = AvatarIPCServer(
            on_speech_start=_on_speech_start,
            on_speech_end=_on_speech_end,
        )
        ipc_server.start()

        # Run QApplication main loop on main thread
        try:
            avatar.start(blocking=True)
        finally:
            ipc_server.stop()
        return 0

    if args.demo:
        # Demo mode: just show idle wireframe, no inference
        logging.info("Demo mode — showing idle wireframe overlay.")
        avatar.start(blocking=True)
        return 0

    if args.wav:
        audio_data, sr = _load_wav(args.wav)
        logging.info("Loaded %s (%d samples @ %d Hz).", args.wav, len(audio_data), sr)
    elif args.tts:
        logging.info("Synthesising TTS: '%s'…", args.tts[:60])
        audio_data, sr = _synthesise_tts(args.tts)
        logging.info("TTS synthesis done (%d samples @ %d Hz).", len(audio_data), sr)
    else:
        logging.info("No audio source specified — showing idle overlay. Use --daemon, --wav, --tts, or --demo.")
        avatar.start(blocking=True)
        return 0

    # Start playback worker thread that waits for overlay readiness
    import threading

    def _play_speech_delayed():
        if avatar._ready_event.wait(timeout=10.0):
            time.sleep(0.3)
            avatar.on_speech_start(audio_data, sample_rate=sr)

    threading.Thread(target=_play_speech_delayed, daemon=True, name="test-playback").start()

    # Run QApplication on main thread
    avatar.start(blocking=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
