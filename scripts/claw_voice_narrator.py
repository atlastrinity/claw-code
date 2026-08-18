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

def clean_for_speech(text: str) -> str:
    """Removes technical symbols, hashes, paths and cleans text for TTS."""
    text = re.sub(r'^#+\s*', '', text, flags=re.MULTILINE)
    text = re.sub(r'\*{1,2}([^*]+)\*{1,2}', r'\1', text)
    text = text.replace('`', '')
    text = re.sub(r'/[\w/.-]+/(\w+\.?\w*)', r'\1', text)
    text = re.sub(r"\{[^}]+\}", "", text)
    text = re.sub(r'[a-f0-9]{16,}', '', text)
    text = re.sub(r'•\s*', '', text)
    lines = [l.strip() for l in text.split('\n') if l.strip() and not re.match(r'^[\s\-=_*#]+$', l.strip())]
    return ' '.join(lines)

# ──────────────────────────── Natural Language generator ──────────────────

def make_natural_speech(voice: str, title: str, raw_text: str) -> str:
    """
    Transforms dry, technical execution text into natural, flowing English.
    """
    clean_text = clean_for_speech(raw_text)
    
    # 1. ATLAS (Main Agent)
    if voice == "atlas":
        if "Сесія" in title or "Session" in title or "Запит" in title:
            prompt = extract_value(raw_text, r"(?:Запит|Prompt|Отримано новий запит від користувача):\s*(.*)") or clean_text
            prompt = re.sub(r"^Отримано новий запит від користувача:\s*", "", prompt).strip()
            return prompt
        
        elif "Контекст" in title or "Context" in title:
            py_files = extract_value(raw_text, r"(?:Файли Пайтон|Python files):\s*(\d+)") or "68"
            test_files = extract_value(raw_text, r"(?:Тестові файли|Test files):\s*(\d+)") or "7"
            archive = "local code archive is fully available" if "Так" in raw_text or "True" in raw_text or "yes" in raw_text.lower() else "local code archive is not available"
            return f"Project context: {py_files} python files, {test_files} test files. {archive}."
            
        elif "Налаштування" in title or "Setup" in title:
            py_ver = extract_value(raw_text, r"(?:Python):\s*([^\s(]*)") or "3.11"
            platform = "macOS" if "macOS" in raw_text or "mac" in raw_text.lower() else "current system"
            test_cmd = extract_value(raw_text, r"(?:Команда тестування|Test command):\s*(.*)")
            
            speech = f"Python version {py_ver}, running on {platform}."
            if test_cmd:
                speech += f" Test command: {test_cmd}."
            return speech
            
        elif "Кроки запуску" in title or "Startup Steps" in title:
            return "Starting initialization and preparing the environment."

        elif "Знайдені маршрути" in title or "Routed Matches" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "No matching routes or commands found."
            
            matches = []
            for line in raw_text.split('\n'):
                if '—' in line or '(' in line:
                    parts = line.replace('•', '').replace('-', '').strip().split(' ')
                    if len(parts) > 1:
                        matches.append(parts[1])
            if matches:
                return f"Found matching routes: {', '.join(matches)}."
            return "Found matching system routes."
            
        elif "Виконання команд" in title or "Command Execution" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "No commands executed at this step."
            return f"Executing command: {clean_text}"
            
        elif "Виконання інструментів" in title or "Tool Execution" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "No tools used at this step."
            return f"Tool execution result: {clean_text}"

        elif "Дія" in title or "Action" in title or "Результат" in title or "Result" in title:
            return raw_text

    # 2. TETIANA (Coordinator / Other)
    elif voice == "tetiana":
        if "Системний запуск" in title or "System Start" in title:
            return f"Запускаю сесію з моделлю {clean_text}."
            
        elif "Ініціалізація системи" in title or "System Init" in title:
            if "порожня" in clean_text.lower() or not clean_text:
                return "Ініціалізацію завершено, система готова до роботи."
            cmds = extract_value(raw_text, r"(?:Завантажені записи команд|Loaded command entries):\s*(\d+)") or "207"
            tools = extract_value(raw_text, r"(?:Завантажені записи інструментів|Loaded tool entries):\s*(\d+)") or "184"
            return f"Систему ініціалізовано. Завантажено {cmds} команд та {tools} інструментів."
        
        elif "Статус виконання" in title or "Execution Status" in title:
            return f"Виконую крок номер {clean_text}."

        elif "Потокові події" in title or "Stream Events" in title:
            return "Receiving data from the language model."
            
        elif "Результат ходу" in title or "Turn Result" in title:
            stop_reason = extract_value(raw_text, r"(?:stop_reason|причина зупинки)=\s*(\w+)") or "completed"
            denials = extract_value(raw_text, r"(?:Відмови доступу|Permission denials):\s*(\d+)") or "0"
            
            speech = f"Turn analysis: Stop reason: {stop_reason}."
            if denials and int(denials) > 0:
                speech += f" Found {denials} permission denials."
            return speech
            
        elif "Історія сесії" in title or "Session History" in title:
            return "Session history analysis completed."

        elif "Аналіз" in title or "Analysis" in title:
            return raw_text

        elif "Результат" in title or "Result" in title:
            return raw_text

    # 3. GRISHA (Security Specialist)
    elif voice == "grisha":
        if "Результат інструменту" in title or "Tool Result" in title:
            return raw_text
            
        return f"Security alert: {clean_text}"

    # Fallback directly
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
    # 0. Спрощення шляхів та прибирання розширень (.py, .sh тощо)
    text = simplify_path_for_speech(text)

    # 1. English (Ukrainian) -> Ukrainian (e.g. cache (кеш) -> кеш)
    # We match any English term followed by parentheses containing NO English letters
    pattern_eng_ukr = r'[a-zA-Z0-9_./\\#@$%^&*()+-]+\s*\(([^a-zA-Z]+)\)'
    processed = re.sub(pattern_eng_ukr, r'\1', text)
    
    # 2. Ukrainian (English) -> Ukrainian (e.g. звіт (report) -> звіт)
    # We match any non-English text followed by parentheses containing English term
    pattern_ukr_eng = r'([^a-zA-Z]+)\s*\([a-zA-Z0-9_./\\#@$%^&*()+-]+\)'
    processed = re.sub(pattern_ukr_eng, r'\1', processed)
    
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

    # --- 1. Startup & Setup (Atlas & Tetiana) ---
    player.speak("atlas", "Запит", f"Запит: {prompt}")
    
    # Context
    context = build_port_context()
    raw_ctx = f"Файли Пайтон: {context.python_file_count}\nТестові файли: {context.test_files_count if hasattr(context, 'test_files_count') else 7}\nАрхів доступний: {'Так' if context.archive_available else 'Ні'}"
    player.speak("atlas", "Контекст", raw_ctx)

    # Setup
    setup_report = run_setup(trusted=True)
    setup = setup_report.setup
    raw_setup = f"Python: {setup.python_version}\nPlatform: {setup.platform_name}\nTest command: {setup.test_command}"
    player.speak("atlas", "Налаштування", raw_setup)
    
    # Startup steps
    player.speak("atlas", "Кроки запуску", "")

    # System Init
    player.speak("tetiana", "Ініціалізація системи", "Loaded command entries: 207\nLoaded tool entries: 184")

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
        player.speak("tetiana", "Результат ходу", f"stop_reason={result.stop_reason}\nВідмови доступу={len(result.permission_denials)}")
        
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
        "You are Tetiana, a female software coordinator and strategist. Your role is to voice the agent's internal reasoning and strategy (WHY we are doing something) in Ukrainian based on the thinking block. "
        "RULES: "
        "1. NEVER mention any agent names (Атлас, Тетяна, Гріша). "
        "2. NEVER mention internal system names (Claw, Claw Code, TaskGraph, system reminders, prompt mandates). Speak ONLY about the user's software project (e.g. iOS app, Swift code, UI, architecture, tests). "
        "3. Use feminine verbs (e.g. 'думаю', 'вирішила', 'перевіряю', 'бачу'). "
        "4. Focus on the THOUGHT PROCESS and STRATEGY, not the exact tool action. "
        "5. Keep it under 15 words. "
        "6. No conversational prefixes, no ellipses, no trailing questions. "
        "If the thinking is purely about internal AI system rules, output NOTHING (empty string). "
        "Output ONLY the Ukrainian reasoning text."
    )
    return call_narration_llm_chain(system_prompt, text)


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
        "You are Grisha, a male Ukrainian quality control and operations specialist intervening because an issue, advisory, or error was detected. "
        "Summarize the problem, required correction, or diagnostic outcome directly in 1 informative sentence in Ukrainian (UA). "
        "RULES: 1. NEVER use agent names (Атлас, Атласе, Тетяна, Тетяно, Гріша, Грішо). "
        "2. State what went wrong or what correction is needed concretely with specific details. "
        "3. Always use masculine verbs (e.g. 'виявив', 'зафіксував', 'потрібно'). "
        "4. Do NOT use English words or Latin letters. Translate every English term into its phonetic Ukrainian equivalent. "
        "5. Keep it under 18 words. Output ONLY the Ukrainian sentence."
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
    
    # 1. Пошук файлів
    files = re.findall(r'\b([\w_-]+\.(?:py|rs|swift|sh|json|toml|md|txt|yml|yaml))\b', clean_text)
    files = list(dict.fromkeys(files))
    
    # 2. Пошук інструментів
    tools = re.findall(r'\b(TaskGraph|replace_file_content|read_file|view_file|run_command|grep_search|list_dir|write_to_file|multi_replace_file_content|search_web|read_url_content)\b', clean_text)
    tools = list(dict.fromkeys(tools))
    
    # 3. Пошук команд у бектіках
    commands = re.findall(r'`([^`]+)`', clean_text)
    commands = [c for c in commands if any(cmd in c.lower() for cmd in ["cargo", "test", "git", "xcodebuild", "run", "python", "sh", "npm", "npm run"])]
    commands = list(dict.fromkeys(commands))

    # Словник для транслітерації назв інструментів для приємнішого озвучення
    tool_pronunciation = {
        "taskgraph": "таск граф",
        "replace_file_content": "реплейс файл контент",
        "read_file": "рід файл",
        "view_file": "в'ю файл",
        "run_command": "ран команд",
        "grep_search": "греп серч",
        "list_dir": "ліст дір",
        "write_to_file": "райт ту файл",
        "multi_replace_file_content": "мульті реплейс файл контент",
        "search_web": "серч веб",
        "read_url_content": "рід ю-ар-ел контент"
    }

    if tools and files:
        t_name = tools[0]
        t_pron = tool_pronunciation.get(t_name.lower(), t_name)
        return f"Використовую {t_pron} для {files[0]}."
    elif tools:
        t_name = tools[0]
        t_pron = tool_pronunciation.get(t_name.lower(), t_name)
        return f"Запуск інструменту {t_pron}."
    elif files:
        return f"Аналіз файлу {files[0]}."
    elif commands:
        return f"Запуск команди {commands[0]}."
        
    # Identify agent intent based on keywords
    if any(k in text_lower for k in ["read", "view", "file", "content", "open"]):
        brief = "Аналіз вмісту файлів."
    elif any(k in text_lower for k in ["search", "find", "glob", "grep", "locate"]):
        brief = "Пошук файлів або коду."
    elif any(k in text_lower for k in ["task", "plan", "graph", "roadmap", "todo"]):
        brief = "Оновлення чек-листа."
    elif any(k in text_lower for k in ["test", "run", "build", "execute", "compile"]):
        brief = "Тестування або збірка проекту."
    elif any(k in text_lower for k in ["fix", "bug", "error", "modify", "replace", "edit"]):
        brief = "Редагування коду."
    else:
        brief = "Аналіз стану системи."
        
    return brief

def translate_to_ukrainian(text: str, voice: str = "tetiana", title: str = "") -> str:
    if not text.strip():
        return text

    # If the text is already Ukrainian (e.g. from preview_msg or summarize_thinking_ua), do NOT re-translate through LLM
    if text.startswith("Планую наступний крок:") or title in ("Аналіз", "Системний запуск", "Статус виконання", "Ініціалізація системи", "Статус процесу", "Завершення роботи"):
        return text

    # Always process text through LLM narration model for Atlas to guarantee clean interactive Ukrainian
    if voice != "atlas" and not re.search(r'[ыЫэЭъЪёЁ]', text) and not re.search(r'[|#*`_─━]', text):
        cyrillic_chars = len(re.findall(r'[а-яА-ЯёЁєЄіІїЇґҐ]', text))
        total_chars = len(re.sub(r'\s+', '', text))
        if total_chars > 0 and (cyrillic_chars / total_chars) > 0.5:
            return text

    if title == "Запит":
        gender_rules = (
            "IMPORTANT: Summarize the user's request into 1-2 natural, concise Ukrainian sentences for voice narration (under 25 words). "
            "Highlight the core goal clearly and what needs to be done. "
            "Do NOT write in the first person. "
            "NEVER include agent names (Атлас, Тетяна, Гріша). "
            "Output ONLY the concise Ukrainian summary with no extra text or prefixes."
        )
    elif voice == "tetiana":
        gender_rules = (
            "IMPORTANT: You are Tetiana, a female coordinator. "
            "Translate the strategy or preview directly into 1 concise Ukrainian sentence (under 12 words). "
            "Use feminine verbs (e.g., 'планую', 'перевіряю', 'координую'). "
            "NEVER invent unmentioned tools, scripts, git commands, databases, or actions. "
            "NEVER include agent names. Keep strictly faithful to the input text."
        )
    elif voice == "atlas":
        if title == "Результат":
            gender_rules = (
                "IMPORTANT: You are Atlas, a male action executor. "
                "Your role is to report the final execution result to the user with rich substantive details. "
                "Highlight key findings, specific numbers, hardware/software metrics, filenames, or decisions made. "
                "Do NOT give empty answers like 'я виконав'. Give an informative summary in 2-3 substantive sentences (30-50 words). "
                "Always use masculine verbs (e.g., 'зібрав', 'підготував', 'виявив', 'перевірив'). "
                "NEVER include agent names like 'Тетяно', 'Гріша', 'Атлас'. "
                "Keep direct, fact-filled, and speech-friendly without rambling."
            )
        else:
            gender_rules = (
                "IMPORTANT: You are Atlas, a male action executor. "
                "Your role is to perform tasks and describe what you are doing with concrete specifics. "
                "State the specific tool, file, command, or component being investigated (e.g. 'Зчитую конфігурацію системи через системний профайлер'). "
                "Do NOT use vague phrases like 'виконую дію' or generic essays. State the concrete action in 1 clear, substantive sentence (10-18 words). "
                "Always use masculine verbs (e.g., 'аналізую', 'зчитую', 'запускаю', 'редагую'). "
                "NEVER include agent names. Keep strictly concrete, informative, and synchronized with execution."
            )
    else:
        gender_rules = (
            "IMPORTANT: You are Grisha, a male operations, quality control, and verification specialist. "
            "Your role is to verify tool execution results and highlight concrete outcomes or findings. "
            "State what was verified or discovered with specific technical facts (under 15 words). "
            "Do NOT use generic phrases like 'перевірка пройшла'. Give the concrete verdict. "
            "Always use masculine verbs (e.g., 'підтвердив', 'перевірив', 'виявив'). "
            "NEVER include agent names. Keep highly informative, constructive, and professional."
        )

    system_prompt = (
        f"You are a professional Ukrainian software engineer and narrator. Translate or rewrite the given text into natural, fluent Ukrainian (UA). "
        f"RULES: 1. Talk like a friendly tech teammate speaking to a colleague. Translate programming concepts directly into natural Ukrainian developer terminology. "
        f"2. Do NOT use any Latin letters. Transliterate English terms, tool names, and filenames phonetically into Ukrainian. NEVER invent or mention unmentioned tools, scripts, or git commands. "
        f"3. {gender_rules} "
        f"4. IMPORTANT FOR SPEECH SYNTHESIS (TTS): Strip out markdown tables and technical code noise, but PRESERVE all core facts. Output ONLY clean, speech-friendly Ukrainian text."
    )

    prompt_payload = text[:1500] if len(text) > 1500 else text
    translated = call_narration_llm_chain(system_prompt, prompt_payload)
    if translated:
        return translated
    if title == "Запит":
        return "Отримано завдання від користувача на налаштування та реалізацію проекту."
    return text

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

def make_natural_tool_use(tool_name: str, input_str: str) -> tuple[str, str]:
    try:
        params = json.loads(input_str)
    except Exception:
        params = {}
        
    cmd = params.get("command", params.get("CommandLine", ""))
    desc = params.get("description", params.get("Description", params.get("toolSummary", params.get("toolAction", ""))))
    
    action_desc = ""
    spoken_text = f"Tool: {tool_name}. "
    if desc:
        spoken_text += f"Context: {desc}. "
    
    if tool_name in ("bash", "run_command"):
        cmd_str = str(cmd).strip()
        desc_str = str(desc).strip()
        action_desc = get_command_description_ua(cmd_str, desc_str)
        spoken_text += f"Command: {cmd_str}"
        
    elif tool_name in ("read_file", "view_file"):
        path = params.get("AbsolutePath", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        action_desc = f"читання файлу {filename}"
        spoken_text += f"Target: {filename}"
        
    elif tool_name in ("write_to_file", "write_file", "create_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        action_desc = f"запису у файл {filename}"
        spoken_text += f"Target: {filename}"
        
    elif tool_name in ("replace_file_content", "multi_replace_file_content", "edit_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        action_desc = f"редагування файлу {filename}"
        spoken_text += f"Target: {filename}"
        
    elif tool_name == "grep_search":
        query = params.get("Query", params.get("query", ""))
        action_desc = f"пошуку тексту '{query}' у коді"
        spoken_text += f"Query: '{query}'"
        
    elif tool_name == "glob_search":
        pattern = params.get("Pattern", params.get("pattern", ""))
        action_desc = f"пошуку файлів за шаблоном '{pattern}'"
        spoken_text += f"Pattern: '{pattern}'"
        
    elif tool_name == "list_dir":
        path = params.get("DirectoryPath", params.get("path", ""))
        dirname = Path(path).name if path else "директорії"
        action_desc = f"перегляду вмісту папки {dirname}"
        spoken_text += f"Target: {dirname}"
        
    elif tool_name == "TaskGraph":
        op = params.get("operation", "")
        nodes = params.get("nodes", [])
        
        # Load task descriptions for lookup
        descriptions = load_task_descriptions()
        
        # Determine updated or added task descriptions
        details = []
        for n in nodes:
            nid = n.get("id")
            content = n.get("content")
            # If not in params, lookup from JSON
            if not content and nid:
                content = descriptions.get(str(nid))
            
            status = n.get("status")
            status_str = ""
            if status:
                status_val = str(status).lower()
                if "in_progress" in status_val or "inprogress" in status_val:
                    status_str = "у процесі"
                elif "completed" in status_val:
                    status_str = "виконано"
                elif "failed" in status_val:
                    status_str = "провалено"
                elif "pending" in status_val:
                    status_str = "в очікуванні"
            
            if content:
                cleaned_desc = clean_for_speech(content)
                if len(cleaned_desc) > 60:
                    cleaned_desc = cleaned_desc[:60].strip() + "..."
                if status_str:
                    details.append(f"'{cleaned_desc}' ({status_str})")
                else:
                    details.append(f"'{cleaned_desc}'")
            elif nid:
                if status_str:
                    details.append(f"завдання {nid} ({status_str})")
                else:
                    details.append(f"завдання {nid}")
        
        if op == "update_status":
            action_desc = "оновлення статусу завдань"
            if details:
                spoken_text = f"Оновив статус: {', '.join(details[:3])}."
            else:
                spoken_text = "Оновив статус завдань у графіку."
        else:
            action_desc = "оновлення планування"
            spoken_text = "Оновив графік завдань."
            
    else:
        tool_name_ua = TOOL_NAMES_UA.get(tool_name, tool_name)
        action_desc = f"виконання інструменту {tool_name_ua}"
        spoken_text += f"Executing {tool_name}."
        
    return spoken_text, action_desc

def is_tool_call_text(text: str) -> bool:
    text_lower = text.lower().strip()
    return (
        text_lower.startswith("[assistant called") or 
        text_lower.startswith("[асистент викликав") or
        "called tool '" in text_lower or
        "викликав інструмент" in text_lower
    )

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
        model = data.get("model", "невідома модель")
        player.speak("tetiana", "Системний запуск", model)
        
    elif entry_type == "prompt_history":
        text = data.get("text", "")
        if text:
            player.speak("atlas", "Запит", text)
            
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
            
            has_real_text = len(real_text_blocks) > 0
            taskgraph_pending_speech = None
            
            # Generate synthetic thinking for non-reasoning models (so Tetiana previews upcoming actions)
            has_thinking = any(b.get("type") == "thinking" and b.get("thinking", "").strip() for b in blocks)
            if not has_thinking:
                for b in blocks:
                    if b.get("type") == "tool_use":
                        t_name = b.get("name", "")
                        t_in = b.get("input", "")
                        if t_name:
                            _, act_desc = make_natural_tool_use(t_name, t_in)
                            if act_desc:
                                preview_msg = f"Планую наступний крок: {act_desc[0].lower()}{act_desc[1:]}."
                                player.speak("tetiana", "Аналіз", preview_msg)
                                break
            
            for block in blocks:
                block_type = block.get("type")
                if block_type == "thinking":
                    thinking_val = block.get("thinking", "")
                    natural_thinking = summarize_thinking_ua(thinking_val)
                    if natural_thinking:
                        player.speak("tetiana", "Аналіз", natural_thinking)
                elif block_type == "text":
                    text_content = block.get("text", "")
                    if text_content and not is_tool_call_text(text_content):
                        if taskgraph_pending_speech:
                            text_content = f"{taskgraph_pending_speech}. {text_content}"
                            taskgraph_pending_speech = None
                        text_content = clean_assistant_phrases(text_content)
                        if text_content.strip():
                            player.speak("atlas", "Результат", text_content)
                elif block_type == "tool_use":
                    tool_name = block.get("name", "")
                    input_str = block.get("input", "")
                    if tool_name:
                        natural_tool, action_desc = make_natural_tool_use(tool_name, input_str)
                        player.last_action_desc = action_desc
                        player.last_action_tool = tool_name
                        
                        if tool_name == "TaskGraph":
                            # Buffer TaskGraph speech instead of speaking immediately
                            taskgraph_pending_speech = natural_tool.rstrip('.') if natural_tool else None
                        else:
                            if taskgraph_pending_speech:
                                # Pair it with the next action
                                if natural_tool:
                                    first_char = natural_tool[0].lower()
                                    rest = natural_tool[1:]
                                    combined_tool = f"{taskgraph_pending_speech}, а тепер {first_char}{rest}"
                                else:
                                    combined_tool = f"{taskgraph_pending_speech}, а тепер виконую наступну дію"
                                taskgraph_pending_speech = None
                                player.speak("atlas", "Дія", combined_tool)
                            else:
                                player.speak("atlas", "Дія", natural_tool)
                                
            if taskgraph_pending_speech:
                player.speak("atlas", "Дія", taskgraph_pending_speech)
                        
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
        f = open(latest_file, "r", encoding="utf-8")
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
