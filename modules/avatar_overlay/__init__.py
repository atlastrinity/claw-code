"""
Avatar Overlay Module — 3D Green Mesh Overlay for TTS Atlas.

Receives audio from the TTS engine, runs FaceFormer (CVPR 2022) inference
to produce per-frame 3D face vertices, and renders them as a neon-green
wireframe on a transparent click-through overlay window (macOS).

Quick start
-----------
    from modules.avatar_overlay import AvatarOverlayModule

    avatar = AvatarOverlayModule()
    avatar.start()                          # opens overlay window
    avatar.on_speech_start(audio_array)     # feed TTS audio
    avatar.on_speech_end()                  # return to idle pose
    avatar.stop()                           # tear down
"""

from modules.avatar_overlay.controller import AvatarOverlayModule

__all__ = ["AvatarOverlayModule"]
