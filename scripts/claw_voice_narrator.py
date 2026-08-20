#!/usr/bin/env python3
"""
🎙️ CLAW Voice Narrator — Природна озвучка виводу claw-code українськими голосами

Кожна секція виводу програми озвучується відповідним «агентом»:
  ⚙️ Атлас (Виконавець)   — дії, виконання команд та інструментів
  🎙️ Тетяна (Координатор) — аналіз ходу, координування плану та стратегія
  🛡️ Гріша (Контроль)     — верифікація результатів, критичні попередження

Якщо процес не завершився повністю, озвучка продовжується у наступних ходах.
"""

from __future__ import annotations

import os
import re
import sys
import time
import json
import random
import subprocess
from pathlib import Path
from typing import Optional, List
import urllib.request
import urllib.error

# Додаємо корінь проекту до шляху імпорту
project_root = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(project_root))
original_cwd = Path.cwd()
os.chdir(project_root)

# Prepend typical macOS tool paths to PATH (Homebrew and user local python bin)
_extra_paths = [
    "/opt/homebrew/bin",
    "/usr/local/bin",
    str(Path.home() / "Library/Python/3.9/bin"),
]
# Dynamically add any other user python bin folders to cover different Python versions
_user_py_dir = Path.home() / "Library/Python"
if _user_py_dir.exists():
    for _p in _user_py_dir.glob("*/bin"):
        _extra_paths.append(str(_p))

_current_path = os.environ.get("PATH", "")
_paths = _current_path.split(os.pathsep)
for _p in _extra_paths:
    if _p not in _paths:
        _paths.insert(0, _p)
os.environ["PATH"] = os.pathsep.join(_paths)

# Load .env files automatically to ensure API keys are populated even if run in raw shell
for env_dir in (project_root, Path.home() / ".claw"):
    env_file = env_dir / ".env"
    if env_file.exists():
        try:
            with open(env_file, "r", encoding="utf-8") as f:
                for line in f:
                    line_str = line.strip()
                    if line_str and not line_str.startswith("#") and "=" in line_str:
                        k, v = line_str.split("=", 1)
                        k = k.strip()
                        v = v.strip().strip("\"").strip("'")
                        if k and k not in os.environ:
                            os.environ[k] = v
        except Exception:
            pass


try:
    from src.runtime import PortRuntime, RoutedMatch
    from src.query_engine import QueryEnginePort, TurnResult
    from src.models import PermissionDenial
    from src.setup import run_setup
    from src.context import build_port_context
except ImportError as e:
    print(f"⚠️ Помилка імпорту модулів проекту claw-code: {e}")
    print("Переконайтеся, що скрипт запускається з кореня репозиторію.")
    sys.exit(1)

# ──────────────────────────── Colour & Emoji ────────────────────────────

COLORS = {
    "tetiana":  "\033[95m",   # Magenta — Тетяна
    "atlas":    "\033[96m",   # Cyan — Атлас
    "grisha":   "\033[91m",   # Red — Гріша
    "system":   "\033[94m",   # Blue — Системні логи
    "reset":    "\033[0m",
    "bold":     "\033[1m",
    "dim":      "\033[2m",
}

AGENT_EMOJI = {
    "tetiana": "🎙️",
    "atlas":   "⚙️",
    "grisha":  "🛡️",
}

AGENT_NAME_UA = {
    "tetiana": "Тетяна · Координатор",
    "atlas":   "Атлас · Виконавець",
    "grisha":  "Гріша · Контроль",
}

# ──────────────────────────── Translation Helpers ────────────────────────

TRANSLATE_MAP = {
    "completed": "хід завершено",
    "max_turns_reached": "досягнуто ліміту ходів",
    "max_budget_reached": "вичерпано бюджет токенів",
}

def extract_value(text: str, pattern: str) -> str:
    """Helper to extract values from raw text via regex."""
    match = re.search(pattern, text)
    return match.group(1).strip() if match else ""

def strip_emojis(text: str) -> str:
    """Removes all emojis, dingbats, pictographs and symbols from text to prevent TTS from vocalizing emoji names."""
    if not text:
        return ""
    # Strip variation selectors, zero-width joiners, skin tones, keycap combining characters
    text = re.sub(r"[\ufe00-\ufe0f\u200d\u20e3\U0001f3fb-\U0001f3ff]", "", text)
    # Strip emojis and pictographic Unicode ranges
    emoji_pattern = re.compile(
        r"[\U00010000-\U0010ffff]"  # SMP: All modern emojis (Faces, objects, symbols, etc.)
        r"|[\u2600-\u27bf]"         # Dingbats, Misc Symbols (⚙, ⚠️, ✨, 🎙, 🛡, ⚡, ☕, ✅, ❌, etc.)
        r"|[\u2300-\u23ff]"         # Misc Technical (⌚, ⏰, ⌛, ⏱, ⏳, etc.)
        r"|[\u2b00-\u2bff]"         # Misc Symbols and Arrows (⭐, ⭕, etc.)
        r"|[\u25a0-\u25ff]"         # Geometric Shapes (■, □, ▲, ▼, ◆, ●, etc.)
        r"|[\u3297\u3299\u3030\u303d\u00a9\u00ae]"
    )
    text = emoji_pattern.sub("", text)
    # Fix spacing around punctuation and collapse whitespace
    text = re.sub(r"\s+([,.:;!?])", r"\1", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text

def clean_for_speech(text: str) -> str:
    """Removes technical symbols, hashes, paths, emojis and cleans text for TTS."""
    text = strip_emojis(text)
    text = re.sub(r'^#+\s*', '', text, flags=re.MULTILINE)
    text = re.sub(r'\*{1,2}([^*]+)\*{1,2}', r'\1', text)
    text = text.replace('`', '')
    # Strip parenthetical English transliterations e.g. (Oleh Mykolayovych Kizyma)
    text = re.sub(r'\([a-zA-Z\s,.-]+\)', '', text)
    text = re.sub(r'/[\w/.-]+/(\w+\.?\w*)', r'\1', text)
    text = re.sub(r"\{[^}]+\}", "", text)
    text = re.sub(r'[a-f0-9]{16,}', '', text)
    text = re.sub(r'•\s*', '', text)
    lines = [l.strip() for l in text.split('\n') if l.strip() and not re.match(r'^[\s\-=_*#]+$', l.strip())]
    return ' '.join(lines)

# ──────────────────────────── Natural Language generator ──────────────────

def make_natural_speech(voice: str, title: str, raw_text: str) -> str:
    """
    Cleans raw text for natural Ukrainian speech output without hardcoded templates.
    """
    clean_text = clean_for_speech(raw_text)
    if not clean_text:
        return ""
    
    if voice == "grisha" and ("alert" in title.lower() or "зауваження" in title.lower()):
        if not clean_text.lower().startswith("security alert:") and not clean_text.lower().startswith("зауваження"):
            return f"Зауваження контролю: {clean_text}"
        return clean_text

    return clean_text



# ──────────────────────────── TTS Phonetic Transcription ─────────────────

TECH_GLOSSARY = {
    "performance": "перформанс", "optimization": "оптимізація", "cache": "кеш", "buffer": "буфер",
    "setup": "сетап", "git": "гіт", "run": "ран", "test": "тест", "compile": "компіляція",
    "build": "білд", "file": "файл", "app": "апп", "swift": "свіфт", "ios": "ай ос",
    "macos": "мак ос", "thread": "тред", "memory": "меморі", "debug": "дебаг", "error": "еррор",
    "warning": "ворнінг", "fail": "фейл", "success": "саксес", "connection": "коннекшн",
    "relay": "рілей", "server": "сервер", "client": "клієнт", "host": "хост", "port": "порт",
    "quic": "квік", "udp": "ю ді пі", "tcp": "ті сі пі", "webrtc": "веб ер ті сі", "bonjour": "бонжур",
    "mdns": "ем ді ен ес", "cloudkit": "клаудкіт", "signaling": "сигналінг", "audio": "аудіо",
    "video": "відео", "stream": "стрім", "capture": "кепчур", "renderer": "рендерер",
    "metal": "метал", "trackpad": "трекпад", "mouse": "маус", "keyboard": "кіборд",
    "volume": "волюм", "unzip": "анзіп", "zip": "зіп", "mkdir": "мейкдір", "lsof": "ел ес оф",
    "kill": "кілл", "find": "файнд", "grep": "греп", "bash": "беш", "command": "команд",
    "tool": "тул", "project": "проджект", "code": "код", "pin": "пін", "package": "пекедж",
    "dependency": "депенденсі", "framework": "фреймворк", "library": "лайбрері", "api": "ей пі ай",
    "url": "ю ар ел", "ip": "ай пі", "ping": "пінг", "pong": "понг", "ack": "ек", "latency": "лейтенсі",
    "quality": "кволіті", "fps": "еф пі ес", "bitrate": "бітрів", "codec": "кодек",
    "h264": "ейч два шість чотири", "h265": "ейч два шість п'ять", "aac": "ей еі сі",
    "avfoundation": "аудіо відео фаундейшн", "screencapturekit": "скрін кепчур кіт",
    "appkit": "апп кіт", "uikit": "юі кіт", "swiftui": "свіфт юі", "combine": "комбайн",
    "coregraphics": "кор графікс", "videotoolbox": "відео тулбокс", "analog": "аналог",
    "language": "ленгвідж", "session": "сешн", "history": "хісторі", "meta": "мета",
    "prompt": "промпт", "assistant": "асистент", "thinking": "сінкінг", "thought": "сот",
    "trace": "трейс", "flow": "флоу", "runtime": "рантайм", "router": "роутер",
    "analyst": "аналіст", "security": "сек'юріті", "system": "систем", "lock": "лок",
    "unlock": "анлок", "binary": "байнарі", "data": "дейта", "length": "ленгс",
    "byte": "байт", "integer": "інтіджер", "string": "стрінг", "boolean": "булеан",
    "float": "флоат", "double": "дабл", "class": "клас", "struct": "стракт", "enum": "інум",
    "function": "фанкшн", "method": "метод", "variable": "веріабл", "constant": "констант",
    "import": "імпорт", "init": "ініт", "deinit": "деініт", "override": "оверрайд",
    "super": "супер", "self": "селф", "nil": "ніл", "null": "нал", "true": "тру",
    "false": "фолс", "and": "енд", "or": "ор", "not": "нот", "if": "іф", "else": "елс",
    "switch": "світч", "case": "кейс", "default": "дефолт", "for": "фор", "while": "вайл",
    "do": "ду", "break": "брейк", "continue": "контінью", "return": "рітурн", "throw": "сроу",
    "try": "трай", "catch": "кетч", "finally": "файналі", "async": "есинк", "await": "евейт",
    "actor": "ектор", "nonisolated": "нонізолейтед", "isolated": "ізолейтед", "sendable": "сендабл",
    "mainactor": "мейн ектор"
}

def transliterate_eng_to_ukr(text: str) -> str:
    rules = [
        ("sh", "ш"), ("ch", "ч"), ("th", "т"), ("ph", "ф"), ("kh", "х"),
        ("oo", "у"), ("ee", "і"), ("ea", "і"), ("ai", "ей"), ("ay", "ей"),
        ("oy", "ой"), ("ou", "ау"), ("ow", "ау"), ("ck", "к"), ("qu", "кв"),
        ("a", "а"), ("b", "б"), ("c", "к"), ("d", "д"), ("e", "е"),
        ("f", "ф"), ("g", "г"), ("h", "х"), ("i", "і"), ("j", "дж"),
        ("k", "к"), ("l", "л"), ("m", "м"), ("n", "н"), ("o", "о"),
        ("p", "п"), ("q", "к"), ("r", "р"), ("s", "с"), ("t", "т"),
        ("u", "у"), ("v", "в"), ("w", "в"), ("x", "кс"), ("y", "і"),
        ("z", "з")
    ]
    
    def repl(match):
        word = match.group(0)
        res = word.lower()
        for eng, ukr in rules:
            res = res.replace(eng, ukr)
        return res
        
    return re.sub(r'[a-zA-Z]+', repl, text)

def simplify_path_for_speech(text: str) -> str:
    """
    Simplifies file paths and command lines for cleaner speech output.
    E.g. /Users/dev/Documents/GitHub/claw-code/rust/Cargo.toml -> Cargo.toml у папці rust
    And strips extensions like .py, .sh, .swift, .rs, .yaml, .json
    """
    # 1. Simplify absolute paths pointing to the project
    def path_repl(match):
        full_path = match.group(0)
        # Try to make it relative to claw-code
        if "claw-code/" in full_path:
            rel = full_path.split("claw-code/")[-1]
        elif "claw-code\\" in full_path:
            rel = full_path.split("claw-code\\")[-1]
        else:
            rel = Path(full_path).name
            
        parts = rel.replace('\\', '/').split('/')
        filename = parts[-1]
        
        # Strip common developer file extensions
        filename_no_ext = re.sub(r'\.(py|sh|swift|rs|yaml|yml|json|md|txt|log)$', '', filename, flags=re.IGNORECASE)
        
        if len(parts) > 1:
            parent_dir = parts[-2]
            return f"{filename_no_ext} у папці {parent_dir}"
        return filename_no_ext

    # Match absolute paths
    text = re.sub(r'/[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+/[a-zA-Z0-9_./-]+', path_repl, text)
    
    # 2. Strip extensions from isolated files with extensions (e.g. script.py, run_claw.sh)
    text = re.sub(r'\b([a-zA-Z0-9_-]+)\.(py|sh|swift|rs|yaml|yml|json|md|txt|log)\b', r'\1', text, flags=re.IGNORECASE)
    
    return text

def prepare_text_for_tts(text: str) -> str:
    # 0. Прибирання емодзі та символів перед синтезом мови
    text = strip_emojis(text)

    # 1. Спрощення шляхів та прибирання розширень (.py, .sh тощо)
    text = simplify_path_for_speech(text)

    # 1. Ukrainian (English) -> Ukrainian (e.g. Кізима Олег Миколайович (Oleh Mykolayovych Kizyma) -> Кізима Олег Миколайович)
    pattern_ukr_eng = r'([^\na-zA-Z]+?)\s*\([a-zA-Z0-9_./\\#@$%^&*()+\-\s]+\)'
    processed = re.sub(pattern_ukr_eng, r'\1', text)

    # 2. English (Ukrainian) -> Ukrainian (e.g. cache (кеш) -> кеш)
    pattern_eng_ukr = r'[a-zA-Z0-9_./\\#@$%^&*()+\-\s]+\s*\(([^a-zA-Z]+)\)'
    processed = re.sub(pattern_eng_ukr, r'\1', processed)
    
    # 3. Замінюємо популярні терміни за словником
    for eng, ua in TECH_GLOSSARY.items():
        processed = re.sub(r'\b' + re.escape(eng) + r'\b', ua, processed, flags=re.IGNORECASE)
        
    # 4. Транслітеруємо решту англійських слів, щоб вони читалися українськими буквами
    processed = transliterate_eng_to_ukr(processed)
    
    # 5. Прибираємо зайві пробіли перед розділовими знаками
    processed = re.sub(r'\s+([,.:;!?])', r'\1', processed)
    
    # 6. Прибираємо символи дужок та зайві пробіли
    processed = processed.replace("(", "").replace(")", "")
    processed = re.sub(r'\s+', ' ', processed).strip()
    return processed

def smart_truncate_sentence(text: str, max_chars: int = 4000) -> str:
    """Truncates text at sentence boundaries to prevent audio chopping."""
    if len(text) <= max_chars:
        return text
    truncated = text[:max_chars]
    for p in ['. ', '? ', '! ', '.\n', '?\n', '!\n']:
        last_p = truncated.rfind(p)
        if last_p > max_chars // 2:
            return truncated[:last_p + 1].strip()
    last_space = truncated.rfind(' ')
    if last_space > max_chars // 2:
        return truncated[:last_space].strip() + '.'
    return truncated.strip() + '.'

# ──────────────────────────── Audio Player ──────────────────────────────


class VoicePlayer:
    def __init__(self, output_dir: Path):
        self.output_dir = output_dir
        self.output_dir.mkdir(exist_ok=True)
        self.tts_engine = None
        self.wav_paths: list[Path] = []
        self.seg_count = 0
        self.last_action_desc = ""
        self.last_action_tool = ""
        self.success_index = 0
        self.failure_index = 0
        self.current_proc = None
        
        # Remove stale lock
        lock_path = Path.home() / ".claw" / "narration.lock"
        if lock_path.exists():
            try:
                lock_path.unlink()
            except Exception:
                pass
                
        # Clean cache directory on startup to delete old TTS files
        cache_dir = self.output_dir / "cache"
        if cache_dir.exists():
            import shutil
            try:
                shutil.rmtree(cache_dir)
            except Exception:
                pass
        cache_dir.mkdir(exist_ok=True)
        
        # Load TTS
        try:
            from edge_tts_wrapper.edge_tts_helper import EdgeTTSHelper
            print(f"\n{COLORS['system']}⏳ Ініціалізація EdgeTTSHelper...{COLORS['reset']}")
            self.tts_engine = EdgeTTSHelper(default_voice="uk-UA-OstapNeural", sample_rate=44100)
            print(f"{COLORS['system']}✅ EdgeTTSHelper успішно ініціалізовано!{COLORS['reset']}\n")
        except ImportError:
            print(f"\n{COLORS['grisha']}⚠️ Бібліотека edge_tts_wrapper не встановлена.{COLORS['reset']}")
            print("Озвучка відбуватиметься лише в текстовому режимі.")
        except Exception as e:
            print(f"\n{COLORS['grisha']}⚠️ Помилка ініціалізації TTS: {e}{COLORS['reset']}")

        # Audio playback queue and background thread
        import queue
        import threading
        self.play_queue = queue.Queue()
        self.play_thread = threading.Thread(target=self._play_loop, daemon=True)
        self.play_thread.start()

    def _play_loop(self):
        lock_path = Path.home() / ".claw" / "narration.lock"
        while True:
            item = self.play_queue.get()
            if item is None:
                self.play_queue.task_done()
                break
                
            # If the agent generated a rapid flurry of actions, skip stale backlog and take the freshest item
            while not self.play_queue.empty() and self.play_queue.qsize() >= 1:
                try:
                    # Drain older intermediate item
                    stale_item = item
                    item = self.play_queue.get_nowait()
                    self.play_queue.task_done()
                except Exception:
                    break

            wav_path = item
            try:
                # Ensure narration lock exists while playback is running
                lock_path.parent.mkdir(parents=True, exist_ok=True)
                lock_path.touch(exist_ok=True)
                time.sleep(0.05)  # Give system time to sync file to disk
                self.current_proc = subprocess.Popen(
                    ["afplay", str(wav_path)],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True
                )
                try:
                    stdout, stderr = self.current_proc.communicate(timeout=120.0)  # Timeout after 120 seconds
                    if self.current_proc.returncode != 0:
                        print(f"\n{COLORS['grisha']}⚠️ afplay повернув код {self.current_proc.returncode}{COLORS['reset']}")
                        if stderr:
                            print(f"  Помилка: {stderr.strip()}")
                    time.sleep(0.2)  # Allow coreaudiod buffer to fully drain before closing
                except subprocess.TimeoutExpired:
                    print(f"\n{COLORS['grisha']}⚠️ afplay перевищив ліміт часу (120 с), завершення процесу.{COLORS['reset']}")
                    self.current_proc.terminate()
                    try:
                        self.current_proc.wait(timeout=1.0)
                    except Exception:
                        pass
            except Exception as e:
                print(f"\n{COLORS['grisha']}⚠️ Помилка відтворення аудіо: {e}{COLORS['reset']}")
            finally:
                self.current_proc = None
                self.play_queue.task_done()
                # If no more items are currently playing or queued, release narration lock
                if self.play_queue.empty():
                    if lock_path.exists():
                        try:
                            lock_path.unlink()
                        except Exception:
                            pass

    def get_success_speech(self, action: str) -> str:
        import random
        templates = [
            f"{action} виконано.",
            f"{action} завершено успішно.",
            f"{action} готово, статус ОК.",
            f"Крок {action} пройшов.",
            f"Готово: {action}.",
            f"{action} без помилок.",
            f"{action} виконано, рухаємось далі.",
        ]
        return random.choice(templates)

    def get_failure_speech(self, action: str, error: str) -> str:
        import random
        templates = [
            f"{action} впало: {error}.",
            f"Збій {action}: {error}.",
            f"Помилка в {action}: {error}.",
            f"Не вдалося {action}: {error}.",
            f"{action} провалилось: {error}.",
            f"Крок {action} не пройшов: {error}.",
            f"Аварія {action}: {error}.",
        ]
        return random.choice(templates)
    def get_tool_verdict_speech(self, tool_name: str, action_desc: str, is_error: bool, output_val: str) -> str:
        clean_out = output_val.strip() if output_val else ""
        lower_out = clean_out.lower()
        
        # Спробуємо отримати вердикт через LLM
        llm_verdict = narrate_tool_result_via_llm(tool_name, action_desc, is_error, output_val)
        if llm_verdict:
            return llm_verdict

        # Локальні правила (fallback)
        is_actually_error = is_error
        if is_actually_error:
            error_msg = clean_error_message(output_val)
            if "not found" in lower_out or "no such file" in lower_out:
                return f"Аналіз показав, що {action_desc} закінчилося нічим: об'єкт або файл не знайдено."
            return self.get_failure_speech(action_desc, error_msg)

        if not clean_out:
            if tool_name in ("grep_search", "glob_search", "list_dir", "bash", "run_command"):
                return f"Агент виконав {action_desc}, але ніяких результатів не знайдено. Там порожньо."
            return f"Операцію {action_desc} виконано, але ніяких даних система не повернула."

        if "not found" in lower_out or "no such file" in lower_out:
            return f"Агент спробував виконати {action_desc}, але в результаті нічого не знайдено."
            
        return self.get_success_speech(action_desc)

    def speak(self, voice: str, title: str, text: str):
        """Generates natural text, prints it, and plays the audio."""
        # Check if the specific voice is enabled via environment variables
        if voice == "atlas" and os.environ.get("CLAW_TTS_ATLAS", "true").lower() == "false":
            return
        if voice == "grisha" and os.environ.get("CLAW_TTS_GRISHA", "true").lower() == "false":
            return
        if voice == "tetiana" and os.environ.get("CLAW_TTS_TETIANA", "true").lower() == "false":
            return

        # Acquire narration lock immediately at the start of speak() to prevent CLI from advancing
        lock_path = Path.home() / ".claw" / "narration.lock"
        try:
            lock_path.parent.mkdir(parents=True, exist_ok=True)
            lock_path.touch(exist_ok=True)
        except Exception:
            pass

        natural_text = make_natural_speech(voice, title, text)
        if not natural_text.strip():
            if self.play_queue.empty() and lock_path.exists():
                try:
                    lock_path.unlink()
                except Exception:
                    pass
            return

        # Translate English/mixed natural text to Ukrainian for BOTH display and TTS!
        # This ensures the printed text on the screen matches the spoken voice perfectly.
        natural_text_ua = translate_to_ukrainian(natural_text, voice=voice, title=title)

        # 1. Print beautifully to terminal
        color = COLORS.get(voice, "")
        emoji = AGENT_EMOJI.get(voice, "🔈")
        name = AGENT_NAME_UA.get(voice, voice)
        reset = COLORS["reset"]
        bold = COLORS["bold"]
        dim = COLORS["dim"]

        print(f"\n{bold}{color}{'─' * 60}{reset}")
        print(f"{color}{emoji}  {name}  —  {title}{reset}")
        print(f"{dim}{color}{'─' * 60}{reset}")
        print(f"  {color}{natural_text_ua}{reset}")
        print(f"{dim}{color}{'─' * 60}{reset}")

        # 2. Generate and play audio if TTS is available
        if self.tts_engine and os.environ.get("CLAW_TTS", "true").lower() != "false":
            import hashlib
            import shutil
            import re
            
            # Map narrator agents to Edge-TTS voices with distinct rate/pitch settings
            voice_settings = {
                "tetiana": ("uk-UA-PolinaNeural", "+15%", "+5Hz"),
                "atlas":   ("uk-UA-OstapNeural", "+18%", "+3Hz"),
                "grisha":  ("uk-UA-OstapNeural", "+10%", "-15Hz"),
            }
            # Allow overriding the voice settings via environment variables (e.g. CLAW_TTS_ATLAS_VOICE="tetiana")
            voice_override = os.environ.get(f"CLAW_TTS_{voice.upper()}_VOICE", voice)
            voice_val, rate_val, pitch_val = voice_settings.get(voice_override, voice_settings.get(voice, ("uk-UA-PolinaNeural", "+0%", "+0Hz")))
            
            # Prepare text for TTS (transcribe English terms, clean up formatting)
            speech_text = prepare_text_for_tts(natural_text_ua)
            
            # Truncate strictly at complete sentence boundaries if exceeding 4000 characters
            speech_text = smart_truncate_sentence(speech_text, max_chars=4000)

            # Cache check by generating md5 hash of the voice and speech text

            hash_key = hashlib.md5(f"{voice}:{speech_text}".encode("utf-8")).hexdigest()
            cache_dir = self.output_dir / "cache"
            cache_dir.mkdir(exist_ok=True)
            cached_wav_path = cache_dir / f"{hash_key}.wav"

            lock_path = Path.home() / ".claw" / "narration.lock"
            try:
                # Ensure parent directory exists
                lock_path.parent.mkdir(parents=True, exist_ok=True)
                lock_path.touch(exist_ok=True)
                
                # If not cached, generate the file
                if not cached_wav_path.exists():
                    # Wait if Claw is currently making an API request to prevent concurrent API calls
                    api_lock_path = Path.home() / ".claw" / "api.lock"
                    wait_start = time.time()
                    warned = False
                    while api_lock_path.exists():
                        if time.time() - wait_start > 3.0:
                            print(f"\n⚠️  Попередження: очищено застаріле блокування api.lock.")
                            try:
                                api_lock_path.unlink()
                            except Exception:
                                pass
                            break
                        time.sleep(0.1)
                        
                    self.tts_engine.synthesize_to_wav(
                        speech_text, 
                        str(cached_wav_path), 
                        voice=voice_val, 
                        rate=rate_val, 
                        pitch=pitch_val
                    )
                
                # Copy cached file to session play file
                wav_path = self.output_dir / f"play_{self.seg_count:03d}_{voice}_{title[:15].replace(' ', '_')}.wav"
                shutil.copyfile(str(cached_wav_path), str(wav_path))
                
                # Store cached path for final narration compilation
                self.wav_paths.append(cached_wav_path)
                self.seg_count += 1
                
                # Keep only the last 10 play_*.wav files in the output directory
                play_files = sorted(list(self.output_dir.glob("play_*.wav")), key=lambda p: p.stat().st_mtime)
                while len(play_files) > 10:
                    oldest = play_files.pop(0)
                    try:
                        oldest.unlink()
                    except Exception:
                        pass
                
                # Enqueue the generated WAV path for background playback
                self.play_queue.put(wav_path)
            except Exception as e:
                print(f"  {COLORS['grisha']}⚠️ Помилка генерації аудіо: {e}{reset}")
                # Since generation failed and we touched the lock, release it if queue is empty
                if self.play_queue.empty() and lock_path.exists():
                    try:
                        lock_path.unlink()
                    except Exception:
                        pass

    def finalize(self):
        """Concatenates all segments into one final file."""
        # Stop background thread
        self.play_queue.put(None)
        
        # Kill active playback process if running
        if self.current_proc:
            try:
                self.current_proc.terminate()
                self.current_proc.wait(timeout=1.0)
            except Exception:
                try:
                    self.current_proc.kill()
                except Exception:
                    pass
            self.current_proc = None
            
        # Remove lock
        lock_path = Path.home() / ".claw" / "narration.lock"
        if lock_path.exists():
            try:
                lock_path.unlink()
            except Exception:
                pass

        if not self.wav_paths:
            return
        
        final_path = self.output_dir / "full_narration.wav"
        try:
            import soundfile as sf
            import numpy as np

            all_data = []
            sample_rate = None

            for path in self.wav_paths:
                if path.exists():
                    data, sr = sf.read(str(path))
                    if sample_rate is None:
                        sample_rate = sr
                    all_data.append(data)
                    # 0.4s pause between agents
                    pause = np.zeros(int(sr * 0.4))
                    all_data.append(pause)

            if all_data and sample_rate:
                combined = np.concatenate(all_data)
                sf.write(str(final_path), combined, sample_rate)
                print(f"\n{COLORS['system']}🎵 Повний трек зустрічі збережено: {final_path}{COLORS['reset']}")
        except Exception as e:
            print(f"\n{COLORS['grisha']}⚠️  Не вдалось склеїти повний аудіозапис: {e}{COLORS['reset']}")


# ──────────────────────────── Dynamic Turn Loop Narrator ─────────────────

def run_narrated_session(prompt: str, max_turns: int = 3):
    audio_dir = project_root / "audio_output"
    player = VoicePlayer(audio_dir)

    # --- 1. Startup & Setup ---
    # Session starts cleanly without hardcoded strings
    context = build_port_context()

    # --- 2. Execution Loop ---
    runtime = PortRuntime()
    engine = QueryEnginePort.from_workspace()
    engine.config = engine.config.__class__(max_turns=max_turns)
    
    # Determine routes
    matches = runtime.route_prompt(prompt, limit=5)
    command_names = tuple(match.name for match in matches if match.kind == 'command')
    tool_names = tuple(match.name for match in matches if match.kind == 'tool')

    # Speak matches (Atlas)
    raw_matches = "\n".join(f"• {m.kind} {m.name}" for m in matches) if matches else "none"
    player.speak("atlas", "Знайдені маршрути", raw_matches)

    # Loop through turns dynamically
    process_finished = False
    
    for turn in range(max_turns):
        player.speak("tetiana", "Статус виконання", f"Розпочинаю хід номер {turn + 1}...")
        
        turn_prompt = prompt if turn == 0 else f'{prompt} [turn {turn + 1}]'
        
        # Execute turn
        result = engine.submit_message(turn_prompt, command_names, tool_names, ())
        
        # Narrate what was executed (Atlas)
        if result.matched_commands:
            player.speak("atlas", "Виконання команд", f"Виконую наступні команди: {', '.join(result.matched_commands)}")
        if result.matched_tools:
            player.speak("atlas", "Виконання інструментів", f"Запускаю відповідні інструменти: {', '.join(result.matched_tools)}")

        # Narrate turn results (Tetiana)
        player.speak("tetiana", "Результат ходу", "Обробила результати поточного кроку.")
        
        # Check if the process is finished
        if result.stop_reason == 'completed':
            process_finished = True
            player.speak("tetiana", "Статус процесу", "Усі завдання поточного процесу успішно виконано.")
            break
        else:
            player.speak("grisha", "Процес не закінчився", 
                         f"Процес зупинено з причиною: {result.stop_reason}. Процес не закінчився, озвучка продовжується.")
            time.sleep(1.0)

    # --- 3. Finalization ---
    if process_finished:
        player.speak("tetiana", "Завершення роботи", "Всі кроки успішно виконано. Озвучку повністю завершено. До зустрічі!")
    else:
        player.speak("grisha", "Критичне завершення", "Увага! Досягнуто ліміту спроб, але процес так і не завершився. Озвучку зупинено у зв'язку з тайм-аутом.")

    player.finalize()


# ──────────────────────────── Tailing & Real-Time Narration ──────────────────

def find_latest_session_file(sessions_dir: Path) -> Optional[Path]:
    jsonl_files = list(sessions_dir.glob("**/*.jsonl"))
    if not jsonl_files:
        return None
    # Сортуємо за часом останньої модифікації
    jsonl_files.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return jsonl_files[0]

TOOL_NAMES_UA = {
    "bash": "командного рядка",
    "run_command": "командного рядка",
    "read_file": "читання файлу",
    "view_file": "перегляду файлу",
    "write_to_file": "запису у файл",
    "write_file": "запису у файл",
    "create_file": "створення файлу",
    "replace_file_content": "редагування файлу",
    "multi_replace_file_content": "пакетного редагування файлу",
    "edit_file": "редагування файлу",
    "grep_search": "пошуку тексту",
    "glob_search": "пошуку файлів",
    "list_dir": "перегляду папки",
    "TaskGraph": "оновлення списку завдань",
    "WebSearch": "веб-пошуку",
    "WebFetch": "завантаження веб-сторінки",
}

def clean_error_message(content: str) -> str:
    if not content:
        return "невідома помилка"
    try:
        data = json.loads(content)
        if isinstance(data, dict):
            for key in ("message", "error", "detail", "errorMessage"):
                if key in data and data[key]:
                    return clean_for_speech(str(data[key]))
    except Exception:
        pass
        
    clean = clean_for_speech(content)
    if not clean:
        return "невідома помилка"
    if len(clean) > 120:
        clean = clean[:120] + "..."
    return clean

def get_command_description_ua(cmd: str, desc: str) -> str:
    cmd = cmd.strip()
    cmd_lower = cmd.lower()
    
    # 0. Git, Cargo, and Python developer commands
    if cmd_lower.startswith("git status"):
        return "перевірки статусу репозиторію"
    elif cmd_lower.startswith("git diff"):
        return "перегляду зміненого коду"
    elif cmd_lower.startswith("git add"):
        files = cmd[7:].strip()
        if files in (".", "-A", "--all"):
            return "додавання всіх змінених файлів"
        return f"індексації файлів {files}"
    elif cmd_lower.startswith("git commit"):
        msg = re.search(r'-m\s+["\']([^"\']+)["\']', cmd)
        msg_str = f" з повідомленням '{msg.group(1)}'" if msg else ""
        return f"створення комміту{msg_str}"
    elif cmd_lower.startswith("git push"):
        return "відправки змін у віддалений репозиторій"
    elif cmd_lower.startswith("git pull"):
        return "отримання свіжих змін із сервера"
    elif cmd_lower.startswith("git log"):
        return "перегляду історії змін"
    elif "cargo test" in cmd_lower:
        package = re.search(r'-p\s+(\S+)', cmd)
        pkg_str = f" пакета {package.group(1)}" if package else ""
        return f"запуску тестів{pkg_str}"
    elif "cargo build" in cmd_lower:
        return "компіляції проекту"
    elif "cargo run" in cmd_lower:
        return "запуску програми"
    elif "cargo check" in cmd_lower:
        return "швидкої перевірки коду"
    elif "cargo clippy" in cmd_lower:
        return "статичного аналізу коду"
    elif cmd_lower.startswith("python") or cmd_lower.startswith("python3"):
        parts = cmd.split(" ")
        script = parts[1] if len(parts) > 1 else ""
        script_name = Path(script).name if script else "скрипта"
        return f"запуску пайтон скрипта {script_name}"
        
    # 1. diskutil commands
    if cmd.startswith("diskutil list"):
        return "отримання списку підключених дисків та розділів"
        
    m = re.search(r'diskutil\s+eraseDisk\s+(\S+)\s+"([^"]+)"\s+GPT\s+(\S+)', cmd)
    if m:
        fs, name, disk = m.groups()
        return f"форматування диска {disk} у формат {fs} з назвою {name}"
        
    m = re.search(r'diskutil\s+eraseDisk\s+(\S+)\s+(\S+)\s+GPT\s+(\S+)', cmd)
    if m:
        fs, name, disk = m.groups()
        return f"форматування диска {disk} у формат {fs} з назвою {name}"

    m = re.search(r'diskutil\s+(?:unmount|umount)\s+force\s+(\S+)', cmd)
    if m:
        return f"примусового розмонтування розділу {m.group(1)}"
        
    m = re.search(r'diskutil\s+(?:unmount|umount)\s+(\S+)', cmd)
    if m:
        return f"розмонтування розділу {m.group(1)}"

    m = re.search(r'diskutil\s+mount\s+(\S+)', cmd)
    if m:
        return f"монтування розділу {m.group(1)}"
        
    # 2. process management
    m = re.search(r'lsof\s+\+D\s+(\S+)', cmd)
    if m:
        return f"пошуку процесів, які використовують папку {m.group(1)}"
    m = re.search(r'lsof\s+(\S+)', cmd)
    if m:
        return f"пошуку процесів, які використовують пристрій або файл {m.group(1)}"
        
    m = re.search(r'kill\s+-9\s+(\d+)', cmd)
    if m:
        return f"примусового завершення процесу з ідентифікатором {m.group(1)}"
    m = re.search(r'kill\s+(\d+)', cmd)
    if m:
        return f"завершення процесу з ідентифікатором {m.group(1)}"

    # 3. File operations
    m = re.search(r'cp\s+(\S+)\s+(\S+)', cmd)
    if m:
        src, dest = m.groups()
        return f"копіювання {Path(src).name} у {Path(dest).name}"
        
    m = re.search(r'mv\s+(\S+)\s+(\S+)', cmd)
    if m:
        src, dest = m.groups()
        return f"перенесення {Path(src).name} у {Path(dest).name}"

    m = re.search(r'rm\s+-rf\s+(\S+)', cmd)
    if m:
        return f"видалення папки або файлу за шляхом {Path(m.group(1)).name}"
    m = re.search(r'rm\s+-f\s+(\S+)', cmd)
    if m:
        return f"видалення файлу {Path(m.group(1)).name}"
    m = re.search(r'rm\s+(\S+)', cmd)
    if m:
        return f"видалення файлу {Path(m.group(1)).name}"

    m = re.search(r'mkdir\s+-p\s+(\S+)', cmd)
    if m:
        return f"створення директорії {Path(m.group(1)).name}"
    m = re.search(r'mkdir\s+(\S+)', cmd)
    if m:
        return f"створення директорії {Path(m.group(1)).name}"

    # 4. Search and archives
    m = re.search(r'find\s+(\S+)\s+-name\s+"([^"]+)"', cmd)
    if m:
        loc, name = m.groups()
        return f"пошуку файлів з назвою {name} у {loc}"
        
    m = re.search(r'find\s+(\S+)\s+-iname\s+"([^"]+)"', cmd)
    if m:
        loc, name = m.groups()
        return f"пошуку файлів з назвою {name} у {loc}"

    m = re.search(r'unzip\s+(\S+)\s+-d\s+(\S+)', cmd)
    if m:
        zipfile, dest = m.groups()
        return f"розпакування архіву {Path(zipfile).name} у папку {Path(dest).name}"
    m = re.search(r'unzip\s+(\S+)', cmd)
    if m:
        return f"розпакування архіву {Path(m.group(1)).name}"

    m = re.search(r'zip\s+-r\s+(\S+)\s+(\S+)', cmd)
    if m:
        zipfile, src = m.groups()
        return f"архівування папки {Path(src).name} у файл {Path(zipfile).name}"

    # General commands
    if cmd.startswith("df -h"):
        return "перевірки вільного місця на підключених дисках"
    if cmd.startswith("ps aux") or cmd.startswith("ps -ef"):
        return "перегляду списку запущених процесів"
        
    # If no command-specific match, check the description
    translations = {
        "check disk space usage": "перевірки вільного місця на диску",
        "check memory usage statistics": "аналізу використання оперативної пам'яті",
        "check top cpu processes": "виявлення найбільш активних процесів процесора",
        "count source files in project": "підрахунку кількості вихідних файлів коду",
        "count mcp-related processes": "перевірки запущених mcp серверів",
    }
    
    for eng, ua in translations.items():
        if eng in desc.lower():
            return ua
            
    return "виконання команди"

_key_indices = {}

def rotate_api_key_index(env_var_name: str):
    global _key_indices
    idx = _key_indices.get(env_var_name, 0)
    _key_indices[env_var_name] = idx + 1

def parse_env_keys(env_var_name: str) -> list[str]:
    import os
    raw_val = os.environ.get(env_var_name, "")
    keys = []
    if raw_val:
        if "," in raw_val:
            keys.extend([k.strip() for k in raw_val.split(",") if k.strip()])
        else:
            keys.append(raw_val.strip())
    for i in range(2, 21):
        k = os.environ.get(f"{env_var_name}{i}", "")
        if k and k.strip() not in keys:
            keys.append(k.strip())
    return keys

def resolve_narration_api_config(model: str) -> tuple[str, str, str, str]:
    api_key = ""
    base_url = ""
    model_id = ""
    target_env = ""

    # Dynamically resolve alias mapping from .claw.json or ~/.claw/settings.json
    resolved_model = model
    claw_json_path = Path(os.environ.get("CLAW_CALLER_CWD", ".")) / ".claw.json"
    if not claw_json_path.exists():
        claw_json_path = Path.home() / ".claw" / "settings.json"
        
    if claw_json_path.exists():
        try:
            with open(claw_json_path, "r") as f:
                cfg = json.load(f)
                aliases = cfg.get("aliases", {})
                if model in aliases:
                    resolved_model = aliases[model]
        except Exception:
            pass

    def get_current_key(env_var_name: str) -> str:
        keys = parse_env_keys(env_var_name)
        if not keys:
            return ""
        global _key_indices
        idx = _key_indices.get(env_var_name, 0)
        selected = keys[idx % len(keys)]
        return selected

    if model == "gemini-lite" or "gemini" in resolved_model.lower():
        base_url = os.environ.get("GEMINI_BASE_URL", "https://generativelanguage.googleapis.com/v1beta/openai/").rstrip('/')
        target_env = "GEMINI_API_KEY"
        api_key = get_current_key(target_env)
        model_id = resolved_model
    elif model in ("glm", "glm2", "glm3") or "glm" in resolved_model.lower():
        base_url = os.environ.get("GLM_BASE_URL", "https://api.z.ai/api/paas/v4").rstrip('/')
        keys = parse_env_keys("GLM_API_KEY")
        target_env = "GLM_API_KEY"
        if keys:
            if model == "glm2" and len(keys) >= 2:
                api_key = keys[1]
            elif model == "glm3" and len(keys) >= 3:
                api_key = keys[2]
            else:
                api_key = get_current_key(target_env)
        else:
            api_key = ""
        model_id = resolved_model
    else:
        env_var_name = "OPENAI_API_KEY"
        if "silicon" in model.lower() or "silicon" in resolved_model.lower():
            env_var_name = "SILICONFLOW_API_KEY"
            base_url = os.environ.get("SILICONFLOW_BASE_URL", "https://api.siliconflow.com/v1").rstrip('/')
        elif "anthropic" in model.lower() or "claude" in model.lower() or "anthropic" in resolved_model.lower():
            env_var_name = "ANTHROPIC_API_KEY"
            base_url = os.environ.get("ANTHROPIC_BASE_URL", "https://api.anthropic.com/v1").rstrip('/')
        else:
            base_url = os.environ.get("OPENAI_BASE_URL", "https://openrouter.ai/api/v1").rstrip('/')
            
        target_env = env_var_name
        api_key = get_current_key(target_env)
        model_id = resolved_model

    if not api_key:
        target_env = "OPENAI_API_KEY"
        api_key = get_current_key(target_env)
            
    return base_url, api_key, model_id, target_env


def call_narration_llm_chain(system_prompt: str, user_prompt: str) -> str:
    import urllib.request, urllib.error, time
    model_setting = os.environ.get("CLAW_NARRATION_MODEL", "gemini-lite")
    
    candidates = []
    
    # 1. First candidates: All available keys for the strictly configured CLAW_NARRATION_MODEL
    base_url, primary_key, model_id, target_env = resolve_narration_api_config(model_setting)
    all_keys = parse_env_keys(target_env) if target_env else []
    if not all_keys and primary_key:
        all_keys = [primary_key]
        
    if base_url and all_keys:
        for k in all_keys:
            candidates.append((base_url, k, model_id, target_env))
    elif base_url:
        candidates.append((base_url, primary_key, model_id, target_env))
        
    # 2. Backup candidates if primary provider keys fail
    openrouter_key = os.environ.get("OPENROUTER_API_KEY") or os.environ.get("OPENAI_API_KEY")
    if openrouter_key and "sk-or-" in openrouter_key:
        candidates.append(("https://openrouter.ai/api/v1", openrouter_key, "meta-llama/llama-3.2-3b-instruct", "OPENROUTER_API_KEY"))
        
    candidates.append(("http://127.0.0.1:11434/v1", "", "qwen2.5:latest", "LOCAL_OLLAMA"))
    candidates.append(("http://127.0.0.1:11434/v1", "", "llama3.2:latest", "LOCAL_OLLAMA"))
    candidates.append(("http://127.0.0.1:11434/v1", "", "qwen2.5-coder:1.5b", "LOCAL_OLLAMA"))

    for base_url, api_key, model_id, target_env in candidates:
        if not base_url:
            continue
            
        url = f"{base_url.rstrip('/')}/chat/completions"
        headers = {"Content-Type": "application/json"}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
            
        payload = {
            "model": model_id,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": 0.5,
            "max_tokens": 4000
        }
        
        try:
            req = urllib.request.Request(
                url, 
                data=json.dumps(payload).encode('utf-8'), 
                headers=headers,
                method='POST'
            )
            with urllib.request.urlopen(req, timeout=10) as response:
                res_data = json.loads(response.read().decode('utf-8'))
                choices = res_data.get("choices", [])
                if choices:
                    text = choices[0].get("message", {}).get("content", "").strip()
                    if text:
                        return strip_agent_names(text)
        except urllib.error.HTTPError as e:
            if target_env and target_env not in ("LOCAL_OLLAMA", ""):
                rotate_api_key_index(target_env)
            continue
        except Exception:
            if target_env and target_env not in ("LOCAL_OLLAMA", ""):
                rotate_api_key_index(target_env)
            continue
            
    return ""


def translate_and_summarize_thinking(text: str) -> str:
    if not text.strip():
        return ""

    # Strip system tags and internal prompts
    text = re.sub(r"(?s)<system-reminder>.*?</system-reminder>", "", text)
    text = re.sub(r"(?s)<[^>]+>", "", text)
    if not text.strip() or "CLAW_" in text or "TaskGraph" in text:
        # Check if anything remains after stripping meta-instructions
        cleaned = re.sub(r"(?i)\b(claw[-_]?code|taskgraph|system-reminder|mandate|prompt|environment_context)\b", "", text).strip()
        if not cleaned:
            return ""

    system_prompt = (
        "You are Tetiana, a female software coordinator. Voice the agent's reasoning concisely in Ukrainian. "
        "RULES: "
        "1. NEVER mention agent names (Атлас, Тетяна, Гріша) or system names (Claw, TaskGraph, MCP, prompt, mandate). "
        "2. NEVER say tool names or function names (grep_search, replace_file_content, run_command, mcp__*, McpSearch). You MAY mention filenames if relevant. "
        "3. Use feminine verbs ('думаю', 'вирішила', 'бачу'). "
        "4. Keep it between 5 and 15 words. Be natural but concise. "
        "5. No prefixes, no ellipses, no questions. "
        "If thinking is about internal AI rules, output NOTHING. "
        "Output ONLY the Ukrainian phrase."
    )
    return call_narration_llm_chain(system_prompt, text)


def summarize_tool_action_via_llm(tool_name: str, params: dict) -> str:
    """
    Uses the secondary narration model (Gemini-lite / Mistral / user-selected model)
    to transform technical tool actions and parameters into concise, natural Ukrainian action phrases (4 to 9 words).
    NEVER reads whole files, raw tool names, or JSON parameters.
    """
    if not tool_name:
        return ""

    # Sanitize and extract only high-level parameters for narration
    clean_params = {}
    if isinstance(params, dict):
        for k in ("url", "URL", "command", "CommandLine", "path", "AbsolutePath", "TargetFile", "Query", "query", "pattern", "Pattern", "DirectoryPath", "selector", "text", "key", "operation"):
            if k in params and params[k]:
                val = str(params[k]).strip()
                if len(val) > 150:
                    val = val[:150] + "..."
                clean_params[k] = val
        if "description" in params and params["description"]:
            clean_params["description"] = str(params["description"])[:150]
        if "Description" in params and params["Description"]:
            clean_params["Description"] = str(params["Description"])[:150]

    system_prompt = (
        "You are Atlas's voice narrator. Describe the action naturally in Ukrainian. "
        "RULES: "
        "1. NEVER name tools or functions (grep_search, replace_file_content, run_command, mcp__*, McpSearch, TaskGraph). "
        "2. You MAY mention filenames for context (e.g. 'Переглядаю prompt.rs', 'Редагую settings.json'). "
        "3. NEVER read JSON, code blocks, or raw parameters. "
        "4. Describe the pure action: "
        "   'Переглядаю код у prompt.rs', 'Редагую конфігурацію', 'Відкриваю сторінку'. "
        "5. Start with a verb. Keep between 5 and 12 words. Output ONLY Ukrainian."
    )
    
    # Only pass description context, never raw tool name
    desc = clean_params.get("Description") or clean_params.get("description") or ""
    cmd = clean_params.get("command") or clean_params.get("CommandLine") or ""
    context_hint = desc or cmd or "action"
    if len(context_hint) > 80:
        context_hint = context_hint[:80]
    return call_narration_llm_chain(system_prompt, context_hint)


def summarize_taskgraph_via_llm(op: str, nodes: list, descriptions: dict) -> str:
    """Summarize task graph status updates for Tetiana via the narration LLM into natural, concise Ukrainian."""
    if op == "add" and (not nodes or len(nodes) > 3):
        return "Склала покроковий план завдань."

    task_items = []
    for n in (nodes or []):
        nid = n.get("id", "")
        content = n.get("content") or descriptions.get(str(nid), "")
        status = n.get("status", "")
        if content:
            task_items.append(f"Task: '{content}', Status: '{status}'")

    if not task_items:
        return "Оновила структуру завдань у графіку."

    system_prompt = (
        "You are Tetiana, a female Ukrainian coordinator. "
        "Summarize the task update concisely in Ukrainian. "
        "RULES: "
        "1. ZERO tool names, function names, MCP names (no 'MCP', 'Playwright', 'TaskGraph', 'McpSearch', 'grep_search'). You MAY mention filenames. "
        "2. Completed -> 'Зафіксувала...' / In progress -> 'Переходимо до...' "
        "3. Feminine verbs only ('Склала', 'Зафіксувала', 'Оновила'). "
        "4. Keep between 4 and 10 words. Output ONLY Ukrainian."
    )
    user_payload = f"Operation: {op}\nTasks: {'; '.join(task_items[:2])}"
    return call_narration_llm_chain(system_prompt, user_payload)


def narrate_tool_result_via_llm(tool_name: str, action_desc: str, is_error: bool, output_val: str) -> str:
    if not output_val.strip():
        return ""

    output_summary = output_val.strip()
    
    try:
        parsed_out = json.loads(output_val)
        if isinstance(parsed_out, dict):
            if parsed_out.get("grisha_review"):
                reviews = parsed_out["grisha_review"]
                if isinstance(reviews, list):
                    output_summary = " ".join(str(r) for r in reviews)
                else:
                    output_summary = str(reviews)
            elif parsed_out.get("alert"):
                output_summary = str(parsed_out["alert"])
            else:
                stdout = parsed_out.get("stdout", "")
                stderr = parsed_out.get("stderr", "")
                if stdout or stderr:
                    output_summary = f"STDOUT:\n{stdout}\nSTDERR:\n{stderr}".strip()
                elif "output" in parsed_out:
                    output_summary = str(parsed_out["output"]).strip()
    except Exception:
        pass

    if len(output_summary) > 4000:
        output_summary = output_summary[:4000] + "... [вивід скорочено]"

    prompt_system = (
        "You are Grisha, a male Ukrainian quality control specialist. "
        "Summarize the problem or correction in 1 concise Ukrainian sentence. "
        "RULES: 1. NO agent names (Атлас, Тетяна, Гріша), no tool names (grep_search, mcp__*, TaskGraph). You MAY mention filenames. "
        "2. State what went wrong concretely. "
        "3. Masculine verbs ('виявив', 'зафіксував', 'потрібно'). "
        "4. ZERO tool or function names. "
        "5. Keep between 8 and 15 words. Output ONLY Ukrainian."
    )
    
    prompt_user = f"Tool: '{action_desc}'. Output / issue detected: '{output_summary}'."
    return call_narration_llm_chain(prompt_system, prompt_user)


def load_task_descriptions() -> dict[str, str]:
    descriptions = {}
    store_var = os.environ.get("CLAWD_TASK_GRAPH_STORE")
    caller_cwd = os.environ.get("CLAW_CALLER_CWD")
    paths = []
    if store_var:
        paths.append(Path(store_var))
    if caller_cwd:
        paths.append(Path(caller_cwd) / ".clawd-task-graph.json")
    if "original_cwd" in globals():
        paths.append(original_cwd / ".clawd-task-graph.json")
    paths.extend([
        Path(".clawd-task-graph.json"),
        Path.home() / ".claw/task_graph.json",
        project_root / ".clawd-task-graph.json"
    ])
    
    for path in paths:
        if path.exists():
            try:
                with open(path, "r", encoding="utf-8") as f:
                    data = json.load(f)
                    if isinstance(data, list):
                        for node in data:
                            nid = node.get("id")
                            content = node.get("content")
                            if nid and content:
                                descriptions[str(nid)] = content
            except Exception:
                pass
    return descriptions

OFFSET_FILE = Path.home() / ".claw" / "narration_offsets.json"

def save_narration_offset(file_path: Path, offset: int):
    try:
        OFFSET_FILE.parent.mkdir(parents=True, exist_ok=True)
        offsets = {}
        if OFFSET_FILE.exists():
            try:
                with open(OFFSET_FILE, "r", encoding="utf-8") as f:
                    offsets = json.load(f)
            except Exception:
                pass
        offsets[str(file_path.resolve())] = offset
        if len(offsets) > 20:
            sorted_keys = sorted(offsets.keys(), key=lambda k: Path(k).stat().st_mtime if Path(k).exists() else 0)
            for old_key in sorted_keys[:-10]:
                offsets.pop(old_key, None)
        with open(OFFSET_FILE, "w", encoding="utf-8") as f:
            json.dump(offsets, f)
    except Exception:
        pass

def get_narration_offset(file_path: Path) -> int:
    try:
        if OFFSET_FILE.exists():
            with open(OFFSET_FILE, "r", encoding="utf-8") as f:
                offsets = json.load(f)
                return offsets.get(str(file_path.resolve()), 0)
    except Exception:
        pass
    return 0

def kill_all_narration_processes():
    try:
        subprocess.run(["killall", "afplay"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception:
        pass


        




def summarize_thinking_ua(thinking_text: str) -> str:
    import random
    
    clean_text = thinking_text.strip()
    if not clean_text or len(clean_text) < 15:
        return ""
        
    # Спробуємо зробити розумний підсумок через LLM
    llm_summary = translate_and_summarize_thinking(clean_text)
    if llm_summary:
        return llm_summary
        
    # Якщо LLM не відповіла, використовуємо покращений евристичний fallback
    text_lower = clean_text.lower()

    # Визначаємо чисту дію без назв інструментів чи файлів
    if any(k in text_lower for k in ["replace", "edit", "write", "modify"]):
        return "Редагую код."
    elif any(k in text_lower for k in ["read", "view", "open", "file", "content"]):
        return "Переглядаю код."
    elif any(k in text_lower for k in ["search", "find", "grep", "locate", "glob"]):
        return "Шукаю у коді."
    elif any(k in text_lower for k in ["test", "build", "compile", "cargo"]):
        return "Збираю проект."
    elif any(k in text_lower for k in ["run", "command", "execute", "bash", "shell"]):
        return "Виконую команду."
    elif any(k in text_lower for k in ["task", "plan", "graph", "roadmap", "todo"]):
        return "Оновлюю план."
    elif any(k in text_lower for k in ["browser", "navigate", "click", "page", "url"]):
        return "Переходжу на сторінку."
    elif any(k in text_lower for k in ["list", "dir", "directory"]):
        return "Переглядаю структуру."
    elif any(k in text_lower for k in ["web", "internet", "http"]):
        return "Шукаю в інтернеті."
    elif any(k in text_lower for k in ["fix", "bug", "error"]):
        return "Виправляю помилку."
    else:
        return "Аналізую."

def translate_to_ukrainian(text: str, voice: str = "tetiana", title: str = "") -> str:
    if not text.strip():
        return text

    # If the text is already predominantly Ukrainian/Cyrillic, speak it directly as-is!
    clean_text = clean_for_speech(text)
    cyrillic_chars = len(re.findall(r'[а-яА-ЯёЁєЄіІїЇґҐ]', clean_text))
    total_chars = len(re.sub(r'\s+', '', clean_text))
    if total_chars > 0 and (cyrillic_chars / total_chars) > 0.4:
        return clean_text

    system_prompt = (
        "You are a professional Ukrainian translator and narrator. "
        "Translate the given text accurately and faithfully into natural, fluent Ukrainian (UA). "
        "CRITICAL RULES: "
        "1. Do NOT invent, fabricate, or add any facts, numbers, hardware/software metrics, or actions not present in the original text. "
        "2. Do NOT pretend to have performed actions if the text is simple conversation or questions. "
        "3. Maintain the exact meaning, tone, and intent of the input text. "
        "4. Output ONLY the clean Ukrainian translation with no extra commentary, prefixes, or explanations."
    )

    prompt_payload = clean_text[:1500] if len(clean_text) > 1500 else clean_text
    translated = call_narration_llm_chain(system_prompt, prompt_payload)
    if translated and translated.strip():
        return translated.strip()
    return clean_text

def translate_to_english(text: str) -> str:
    if not text.strip():
        return text

    model = os.environ.get("CLAW_NARRATION_MODEL", "gemini-lite")
    base_url, api_key, model_id, target_env = resolve_narration_api_config(model)
    max_retries = max(1, len(parse_env_keys(target_env)))

    url = f"{base_url}/chat/completions"

    payload = {
        "model": model_id,
        "messages": [
            {
                "role": "system",
                "content": "You are a professional software engineer. Translate the given Ukrainian user request/prompt for an AI coding assistant into professional, direct English (US). Output ONLY the translated English text, with no introductory or concluding remarks."
            },
            {
                "role": "user",
                "content": text
            }
        ],
        "temperature": 0.3
    }

    for attempt in range(max_retries):
        if attempt > 0:
            base_url, api_key, model_id, target_env = resolve_narration_api_config(model)
            
        if not base_url or not api_key:
            return text

        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}"
        }

        try:
            req = urllib.request.Request(
                url, 
                data=json.dumps(payload).encode('utf-8'), 
                headers=headers,
                method='POST'
            )
            with urllib.request.urlopen(req, timeout=10) as response:
                res_data = json.loads(response.read().decode('utf-8'))
                translated_text = res_data['choices'][0]['message']['content'].strip()
                if translated_text:
                    return translated_text
        except urllib.error.HTTPError as e:
            if e.code in (429, 401, 403, 500, 502, 503, 504):
                rotate_api_key_index(target_env)
                time.sleep(1.0)
                continue
            break
        except Exception:
            rotate_api_key_index(target_env)
            time.sleep(1.0)
            continue
        
    return text

def get_heuristic_tool_use_ua(tool_name: str, params: dict) -> tuple[str, str]:
    t_lower = tool_name.lower()
    
    # 1. Playwright / Browser automation tools
    if "playwright" in t_lower or "browser" in t_lower:
        if "navigate" in t_lower or "goto" in t_lower or "open" in t_lower:
            url = params.get("url", params.get("Url", ""))
            if "google" in url.lower():
                return "Відкриваю Google у вікні браузера", "відкриття Google у браузері"
            domain = re.sub(r'^https?://(www\.)?', '', url).split('/')[0] if url else "сторінку"
            return f"Відкриваю {domain} у браузері", f"відкриття {domain} у браузері"
        elif "click" in t_lower:
            text = params.get("text", params.get("selector", ""))
            target = f"'{text}'" if text and len(text) < 30 else "елемент"
            return f"Клікаю на {target}", f"клік по {target}"
        elif "visible_text" in t_lower or "get_text" in t_lower or "extract" in t_lower:
            return "Зчитую текст та результати зі сторінки", "зчитування результатів зі сторінки"
        elif "screenshot" in t_lower:
            return "Роблю знімок екрана браузера для перевірки", "створення знімка екрана"
        elif "fill" in t_lower or "type" in t_lower:
            text = params.get("text", params.get("value", ""))
            if text:
                return f"Вводжу '{text}' у поле пошуку", f"введення тексту '{text}'"
            return "Вводжу необхідні дані у форму", "введення даних у форму"
        elif "press_key" in t_lower or "keyboard" in t_lower:
            key = params.get("key", "")
            return f"Натискаю клавішу {key}", f"натискання клавіші {key}"
        elif "hover" in t_lower:
            return "Наводжу курсор на елемент сторінки", "наведення курсора"
        elif "evaluate" in t_lower or "eval" in t_lower:
            return "Перевіряю стан сторінки та відеоплеєра", "перевірка стану сторінки"
        elif "select" in t_lower:
            return "Обираю опцію зі списку", "вибір опції"
        elif "close" in t_lower:
            return "Закриваю вікно браузера", "закриття вікна браузера"
        else:
            return "Виконую дію у браузері", "дія у браузері"

    # 2. MCP / Server management
    if t_lower in ("mcpsearch", "mcp_search", "load_server"):
        server = params.get("load_server", params.get("server", ""))
        if server:
            return f"Підключаю сервіс {server}", f"підключення сервісу {server}"
        return "Шукаю доступні інструменти", "пошук інструментів"

    # 3. Web Search / Fetch
    if t_lower in ("websearch", "search_web"):
        q = params.get("query", params.get("Query", ""))
        if q:
            return f"Шукаю '{q}' в інтернеті", f"пошук '{q}' в інтернеті"
        return "Виконую пошук в інтернеті", "пошук в інтернеті"

    if t_lower in ("webfetch", "read_url_content"):
        return "Завантажую вміст веб-сторінки", "завантаження веб-сторінки"

    # 4. Command line & Bash
    if t_lower in ("bash", "run_command"):
        cmd = params.get("command", params.get("CommandLine", ""))
        desc = params.get("description", params.get("Description", params.get("toolSummary", params.get("toolAction", ""))))
        action_desc = get_command_description_ua(str(cmd).strip(), str(desc).strip())
        return f"Запускаю {action_desc}", action_desc

    # 5. File Operations
    if t_lower in ("read_file", "view_file"):
        path = params.get("AbsolutePath", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        return f"Переглядаю файл {filename}", f"читання файлу {filename}"

    if t_lower in ("write_to_file", "write_file", "create_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        return f"Записую дані у файл {filename}", f"запис у файл {filename}"

    if t_lower in ("replace_file_content", "multi_replace_file_content", "edit_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        return f"Вношу зміни у файл {filename}", f"редагування файлу {filename}"

    if t_lower == "grep_search":
        query = params.get("Query", params.get("query", ""))
        if query:
            return f"Шукаю '{query}' у коді", f"пошук '{query}' у коді"
        return "Шукаю збіги у коді", "пошук у коді"

    if t_lower == "glob_search":
        pattern = params.get("Pattern", params.get("pattern", ""))
        if pattern:
            return f"Шукаю файли за шаблоном '{pattern}'", f"пошук файлів '{pattern}'"
        return "Шукаю файли у проекті", "пошук файлів"

    if t_lower == "list_dir":
        path = params.get("DirectoryPath", params.get("path", ""))
        dirname = Path(path).name if path else "директорії"
        return f"Переглядаю файли у папці {dirname}", f"перегляд папки {dirname}"

    # 6. TaskGraph
    if t_lower == "taskgraph":
        op = params.get("operation", "")
        nodes = params.get("nodes", [])
        descriptions = load_task_descriptions()
        return get_heuristic_taskgraph_ua(op, nodes, descriptions)

    # Default fallback: Never use raw 'mcp__' or raw technical identifiers
    return "Виконую дію.", "виконання дії"


def get_heuristic_taskgraph_ua(op: str, nodes: list, descriptions: dict) -> tuple[str, str]:
    if op == "add":
        # Check if adding the initial roadmap
        has_progress = any(n.get("status") in ("in_progress", "completed") for n in (nodes or []))
        if not has_progress or len(nodes) > 2:
            return "Склала покроковий план завдань.", "формування плану"

    for n in (nodes or []):
        nid = n.get("id")
        content = n.get("content") or descriptions.get(str(nid), "")
        status = str(n.get("status", "")).lower()
        if content:
            c_low = content.lower()
            if "playwright" in c_low or "mcp" in c_low:
                topic = "підключення інструментів браузера"
            elif "open google" in c_low or "navigate to google" in c_low:
                topic = "відкриття пошуку Google"
            elif "search" in c_low or "фільм" in c_low or "movie" in c_low:
                topic = "пошук фільмів онлайн"
            elif "analy" in c_low or "rating" in c_low or "page" in c_low:
                topic = "аналіз результатів та рейтингів"
            elif "full" in c_low or "screen" in c_low:
                topic = "розгортання фільму на весь екран"
            elif "verify" in c_low or "ad" in c_low or "play" in c_low:
                topic = "перевірка відтворення відео"
            else:
                # Extract clean cyrillic words
                cyrillic = re.findall(r'[\u0400-\u04FF\w]+', content)
                topic = " ".join(cyrillic[:4]) if cyrillic else "виконання кроку"

            if "completed" in status:
                return f"Зафіксувала виконання: {topic}.", "оновлення статусу завдань"
            elif "in_progress" in status or "inprogress" in status:
                return f"Переходимо до: {topic}.", "оновлення статусу завдань"
            elif "failed" in status:
                return f"Зафіксувала проблему: {topic}.", "оновлення статусу завдань"
            else:
                return f"Оновила статус: {topic}.", "оновлення статусу завдань"

    if op == "update_status":
        return "Оновила статус завдань у графіку.", "оновлення статусу завдань"
    return "Склала покроковий план завдань.", "формування плану"


def make_natural_tool_use(tool_name: str, input_str: str) -> tuple[str, str]:
    try:
        params = json.loads(input_str) if isinstance(input_str, str) else (input_str or {})
    except Exception:
        params = {}
    if not isinstance(params, dict):
        params = {}

    # 1. Handle TaskGraph specifically for Tetiana (Coordinator)
    if tool_name == "TaskGraph":
        op = params.get("operation", "")
        nodes = params.get("nodes", [])
        descriptions = load_task_descriptions()
        spoken_heuristic, action_desc = get_heuristic_taskgraph_ua(op, nodes, descriptions)

        if nodes:
            try:
                llm_spoken = summarize_taskgraph_via_llm(op, nodes, descriptions)
                if llm_spoken and len(llm_spoken.strip()) > 3:
                    clean_llm = clean_for_speech(llm_spoken.strip())
                    clean_llm = strip_agent_names(clean_llm)
                    # Strictly verify NO Latin letters leaked into Tetiana's speech
                    if clean_llm and not re.search(r'[a-zA-Z]{3,}', clean_llm):
                        return clean_llm, action_desc
            except Exception:
                pass
        return spoken_heuristic, action_desc

    # 2. Fast heuristic baseline for other tools (Atlas)
    spoken_heuristic, action_desc_heuristic = get_heuristic_tool_use_ua(tool_name, params)

    # 3. For rich custom tools, enhance into a lively phrase via the narration LLM
    try:
        llm_spoken = summarize_tool_action_via_llm(tool_name, params)
        if llm_spoken and len(llm_spoken.strip()) > 3:
            clean_llm = clean_for_speech(llm_spoken.strip())
            clean_llm = strip_agent_names(clean_llm)
            # Verify no raw identifier leaked in
            if clean_llm and not any(k in clean_llm.lower() for k in ("mcp__", "tool:", "called tool", "executing")):
                return clean_llm, action_desc_heuristic
    except Exception:
        pass

    return spoken_heuristic, action_desc_heuristic

def is_tool_call_text(text: str) -> bool:
    text_lower = text.lower().strip()
    if (
        text_lower.startswith("[assistant called") or 
        text_lower.startswith("[асистент викликав") or
        "called tool '" in text_lower or
        "викликав інструмент" in text_lower or
        "mcp__" in text_lower or
        "taskgraph(" in text_lower or
        "taskgraph:" in text_lower or
        "sequentialthinking" in text_lower or
        "playwright_" in text_lower or
        "grep_search" in text_lower or
        "replace_file_content" in text_lower or
        "view_file" in text_lower or
        "run_command" in text_lower or
        "read_file" in text_lower or
        "write_to_file" in text_lower
    ):
        return True
    if (text.strip().startswith("{") and text.strip().endswith("}")) or ("\"description\":" in text_lower and "\"url\":" in text_lower):
        return True
    if re.search(r'mcp__\w+|useeland|\b(?:playwright|sequentialthinking)\b\s*\{', text_lower):
        return True
    return False

def clean_assistant_phrases(text: str) -> str:
    if not text:
        return text
    # Strip internal system tags
    text = re.sub(r"(?s)<system-reminder>.*?</system-reminder>", "", text)
    text = re.sub(r"(?s)<[^>]+>", "", text)
    phrases = [
        r"(?i)\bмені подобається (?:твій|ваш) план\b[.!?]*\s*",
        r"(?i)\bчудовий план\b[.!?]*\s*",
        r"(?i)\bзгоден з (?:цим|твоїм|вашим) планом\b[.!?]*\s*",
        r"(?i)\bзгоден, (?:я|почну)\b",
        r"(?i)\bя згоден\b[.!?]*\s*",
        r"(?i)\bi agree (?:with the plan|with your plan)?[.!?]*\s*",
        r"(?i)\bi like (?:the|your|this) plan\b[.!?]*\s*",
        r"(?i)\bgreat plan\b[.!?]*\s*",
        r"(?i)\bthat (?:sounds like a|is a) good plan\b[.!?]*\s*",
        r"(?i)\bзгоден\b[.!?]*\s*",
        r"(?i)\bзгода\b[.!?]*\s*",
        r"(?i)\bдобре, я згоден\b[.!?]*\s*",
    ]
    for pattern in phrases:
        text = re.sub(pattern, "", text)
    
    # Clean up double spaces or leading/trailing punctuation left behind
    text = re.sub(r'\s+', ' ', text)
    text = re.sub(r'^\s*[,.;!?]\s*', '', text) # Strip leading punctuation
    text = text.strip()
    return text

def strip_agent_names(text: str) -> str:
    """Remove agent name addressing from narration text."""
    if not text:
        return text
    patterns = [
        r"(?i)\bАтласе?,?\s*",
        r"(?i)\bТетяно?,?\s*",
        r"(?i)\bГрішо?,?\s*",
        r"(?i)\bГріша?,?\s*",
    ]
    for p in patterns:
        text = re.sub(p, "", text).strip()
    # Clean up leading punctuation left behind
    text = re.sub(r"^\s*[,.:;!?]\s*", "", text)
    text = re.sub(r"\s+", " ", text).strip()
    return text

def process_session_entry(data: dict, player: VoicePlayer):
    entry_type = data.get("type")
    if entry_type == "session_meta":
        pass
        
    elif entry_type == "prompt_history":
        pass
            
    elif entry_type == "message":
        message = data.get("message", {})
        role = message.get("role")
        blocks = message.get("blocks", [])
        
        if role == "assistant":
            # 1. Filter and identify real conversational text blocks
            real_text_blocks = []
            for block in blocks:
                if block.get("type") == "text":
                    text_val = block.get("text", "")
                    if text_val and not is_tool_call_text(text_val):
                        real_text_blocks.append(text_val)
            
            for block in blocks:
                block_type = block.get("type")
                
                # 🎙️ TETIANA: Internal reasoning, strategy, and strategic thinking
                if block_type == "thinking":
                    thinking_val = block.get("thinking", "")
                    natural_thinking = summarize_thinking_ua(thinking_val)
                    if natural_thinking:
                        player.speak("tetiana", "Аналіз", natural_thinking)
                        
                # ⚙️ ATLAS: Conversational answers and final results to the user
                elif block_type == "text":
                    text_content = block.get("text", "")
                    if text_content and not is_tool_call_text(text_content):
                        text_content = clean_assistant_phrases(text_content)
                        if text_content.strip():
                            player.speak("atlas", "Результат", text_content)
                            
                # 🛠️ TOOLS: Distinct separation of concerns
                elif block_type == "tool_use":
                    tool_name = block.get("name", "")
                    input_str = block.get("input", "")
                    if tool_name:
                        natural_tool, action_desc = make_natural_tool_use(tool_name, input_str)
                        player.last_action_desc = action_desc
                        player.last_action_tool = tool_name
                        
                        # TaskGraph belongs exclusively to Tetiana (Coordinator)
                        if tool_name == "TaskGraph":
                            player.speak("tetiana", "План", natural_tool)
                        else:
                            # Physical execution actions (Browser, Files, Terminal, etc.) belong to Atlas (Executor)
                            player.speak("atlas", "Дія", natural_tool)
                        
        elif role == "tool":
            for block in blocks:
                block_type = block.get("type")
                if block_type == "tool_result":
                    tool_name = block.get("tool_name", "")
                    is_error = block.get("is_error", False)
                    output_val = block.get("output", "")
                    
                    # Detect if an actual problem, error, or Grisha advisory was returned
                    has_problem = is_error
                    lower_val = output_val.lower() if output_val else ""
                    if not has_problem and output_val:
                        if any(k in lower_val for k in ["grisha_review", "alert", "error", "failed", "permission denied", "non-zero exit code"]):
                            try:
                                parsed = json.loads(output_val)
                                if isinstance(parsed, dict) and (parsed.get("grisha_review") or parsed.get("alert") or parsed.get("is_error")):
                                    has_problem = True
                            except Exception:
                                pass
                    
                    if not has_problem:
                        # Skip smooth successful results to save TTS time and keep narration fast
                        continue
                    
                    action_desc = getattr(player, "last_action_desc", "")
                    if not action_desc or getattr(player, "last_action_tool", "") != tool_name:
                        action_desc = TOOL_NAMES_UA.get(tool_name, tool_name)
                        
                    speech = player.get_tool_verdict_speech(tool_name, action_desc, is_error=True, output_val=output_val)
                    if speech and speech.strip():
                        player.speak("grisha", "Зауваження контролю", speech)

def tail_session_loop():
    import signal
    
    # 0. Kill any pre-existing afplay processes on startup to clean up leftover audio
    kill_all_narration_processes()
    
    pid_file = Path.home() / ".claw" / "voice_narrator.pid"
    pid_file.parent.mkdir(parents=True, exist_ok=True)
    if pid_file.exists():
        try:
            with open(pid_file, "r") as pf:
                old_pid = int(pf.read().strip())
            os.kill(old_pid, 0)
            print(f"\n{COLORS['system']}🎙️  Синхронізатор озвучки вже запущено (PID: {old_pid}). Вихід.{COLORS['reset']}\n")
            sys.exit(0)
        except (ValueError, OSError):
            pass
            
    try:
        pid_file.write_text(str(os.getpid()))
    except Exception:
        pass

    audio_dir = project_root / "audio_output"
    player = VoicePlayer(audio_dir)
    player_ref = [player]

    def handle_signal(signum, frame):
        print(f"\n{COLORS['system']}🛑 Озвучку зупинено користувачем (Ctrl+C). Вбиваємо процеси відтворення...{COLORS['reset']}")
        kill_all_narration_processes()
        if player_ref[0]:
            player_ref[0].finalize()
        if pid_file.exists():
            try:
                pid_file.unlink()
            except Exception:
                pass
        sys.exit(0)

    signal.signal(signal.SIGINT, handle_signal)
    signal.signal(signal.SIGTERM, handle_signal)

    try:
        caller_cwd = os.environ.get("CLAW_CALLER_CWD")
        if caller_cwd:
            sessions_dir = Path(caller_cwd) / ".claw" / "sessions"
        else:
            sessions_dir = original_cwd / ".claw" / "sessions"
            
        sessions_dir.mkdir(parents=True, exist_ok=True)

        print(f"{COLORS['bold']}{COLORS['system']}🎙️ Режим реального часу (Tailing Mode) запущено.{COLORS['reset']}")
        print(f"{COLORS['system']}Очікування нових записів у сесіях...{COLORS['reset']}\n")
        
        latest_file = None
        while not latest_file:
            latest_file = find_latest_session_file(sessions_dir)
            if not latest_file:
                time.sleep(1.0)
                
        print(f"{COLORS['system']}👀 Стеження за файлом сесії: {latest_file}{COLORS['reset']}\n")
        
        # Open file and restore offset if present
        is_new_session = os.environ.get("CLAW_NEW_SESSION", "").lower() == "true"
        f = open(latest_file, "r", encoding="utf-8")
        if is_new_session:
            f.seek(0, 2)
            save_narration_offset(latest_file, f.tell())
        else:
            start_offset = get_narration_offset(latest_file)
            if start_offset > 0:
                print(f"{COLORS['system']}↩️ Відновлюємо озвучку з позиції {start_offset} у файлі сесії.{COLORS['reset']}")
                f.seek(start_offset)
            else:
                f.seek(0, 2)
                save_narration_offset(latest_file, f.tell())
                
        try:
            while True:
                line = f.readline()
                if not line:
                    # Check if a new session file has appeared
                    current_latest = find_latest_session_file(sessions_dir)
                    if current_latest and current_latest != latest_file:
                        print(f"\n{COLORS['system']}🔄 Виявлено нову активну сесію: {current_latest}{COLORS['reset']}")
                        latest_file = current_latest
                        f.close()
                        f = open(latest_file, "r", encoding="utf-8")
                        start_offset = get_narration_offset(latest_file)
                        if start_offset > 0:
                            f.seek(start_offset)
                        else:
                            f.seek(0, 2)
                            save_narration_offset(latest_file, f.tell())
                        continue
                    
                    time.sleep(0.5)
                    # Close and reopen the file to reset the EOF flag and clear buffer on macOS
                    pos = f.tell()
                    f.close()
                    f = open(latest_file, "r", encoding="utf-8")
                    f.seek(pos)
                    continue
                
                try:
                    data = json.loads(line)
                    process_session_entry(data, player)
                    save_narration_offset(latest_file, f.tell())
                except Exception:
                    pass
        except KeyboardInterrupt:
            handle_signal(signal.SIGINT, None)
    finally:
        if pid_file.exists():
            try:
                pid_file.unlink()
            except Exception:
                pass



# ──────────────────────────── Main Entry Point ──────────────────────────

def main():
    if len(sys.argv) > 1 and sys.argv[1] in ("--tail", "-t"):
        tail_session_loop()
        return

    if len(sys.argv) > 1:
        prompt = " ".join(sys.argv[1:])
    else:
        print(f"{COLORS['bold']}Введіть ваш запит для системи аналізу (наприклад, 'bootstrap summary' або 'run tests'):{COLORS['reset']}")
        prompt = input("> ")
        if not prompt.strip():
            prompt = "Запуск системи аналізу коду"

    # Translate prompt to English if it contains Cyrillic characters (Ukrainian/Russian)
    if re.search(r'[а-яА-ЯёЁєЄіІїЇґҐ]', prompt):
        prompt_en = translate_to_english(prompt)
        print(f"\n{COLORS['system']}📝 Translated request to English: {prompt_en}{COLORS['reset']}\n")
        prompt = prompt_en

    run_narrated_session(prompt)


if __name__ == "__main__":
    main()
