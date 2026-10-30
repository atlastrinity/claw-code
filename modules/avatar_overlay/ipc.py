"""
Inter-Process Communication (IPC) helper for Avatar Overlay.

Uses lightweight local UDP datagrams (127.0.0.1:48124) to synchronize
speech audio between CLAW Voice Narrator and the 3D Avatar Overlay window.
"""

from __future__ import annotations

import json
import logging
import socket
from typing import Callable, Optional

DEFAULT_IPC_HOST = "127.0.0.1"
DEFAULT_IPC_PORT = 48124

logger = logging.getLogger(__name__)


def send_avatar_event(
    event: str,
    wav_path: Optional[str] = None,
    voice: Optional[str] = None,
    host: str = DEFAULT_IPC_HOST,
    port: int = DEFAULT_IPC_PORT,
) -> bool:
    """Send an event notification to the Avatar Overlay daemon.

    Non-blocking and safe to call even if the daemon is not running.
    """
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.settimeout(0.1)
        payload = {
            "event": event,
            "wav_path": str(wav_path) if wav_path else None,
            "voice": voice or "atlas",
        }
        msg = json.dumps(payload).encode("utf-8")
        sock.sendto(msg, (host, port))
        sock.close()
        return True
    except Exception as e:
        logger.debug("Failed to send avatar IPC event: %s", e)
        return False


class AvatarIPCServer:
    """Listens for speech events from CLAW Voice Narrator and notifies callbacks."""

    def __init__(
        self,
        on_speech_start: Callable[[str, str], None],
        on_speech_end: Callable[[], None],
        host: str = DEFAULT_IPC_HOST,
        port: int = DEFAULT_IPC_PORT,
    ):
        self.on_speech_start = on_speech_start
        self.on_speech_end = on_speech_end
        self.host = host
        self.port = port
        self._sock: Optional[socket.socket] = None
        self._running = False

    def start(self) -> None:
        import threading

        self._sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind((self.host, self.port))
        self._running = True

        def _listen_loop() -> None:
            while self._running and self._sock:
                try:
                    data, _ = self._sock.recvfrom(65535)
                    if not data:
                        continue
                    payload = json.loads(data.decode("utf-8"))
                    event = payload.get("event")
                    if event == "speech_start":
                        wav = payload.get("wav_path")
                        voice = payload.get("voice", "atlas")
                        if wav:
                            self.on_speech_start(wav, voice)
                    elif event == "speech_end":
                        self.on_speech_end()
                except Exception:
                    if self._running:
                        logger.debug("Error in Avatar IPC listener loop", exc_info=True)

        t = threading.Thread(target=_listen_loop, daemon=True, name="avatar-ipc-server")
        t.start()

    def stop(self) -> None:
        self._running = False
        if self._sock:
            try:
                self._sock.close()
            except Exception:
                pass
            self._sock = None
