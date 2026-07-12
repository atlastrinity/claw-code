#!/usr/bin/env python3
"""
🎙️ CLAW Voice Narrator — Природна озвучка виводу claw-code українськими голосами

Кожна секція виводу програми озвучується відповідним «агентом»:
  ⚙️ Атлас (Основний)     — запуск, контекст, налаштування, маршрути та команди
  🎙️ Тетяна (Координатор) — аналіз ходу, результати, потокові події та історія
  🛡️ Гріша (Безпека)      — відмови доступу, критичні попередження

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
    "atlas":   "Атлас · Основний",
    "grisha":  "Гріша · Безпека",
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
    Transforms dry, technical execution text into natural, flowing Ukrainian.
    """
    clean_text = clean_for_speech(raw_text)
    
    # 1. ATLAS (Main Agent)
    if voice == "atlas":
        if "Сесія" in title or "Session" in title or "Запит" in title:
            prompt = extract_value(raw_text, r"(?:Запит|Prompt|Отримано новий запит від користувача):\s*(.*)") or clean_text
            prompt = re.sub(r"^Отримано новий запит від користувача:\s*", "", prompt).strip()
            templates = [
                f"Привіт, народ! Я Атлас. Отримав новий запит від юзера: {prompt}. Зараз у всьому розберемося!",
                f"Здоров колеги, це Атлас. Маємо нову задачу в роботі: {prompt}. Зараз перевірю оточення.",
                f"Всім привіт! Я Атлас. Юзер просить зробити таке: {prompt}. Беруся до налаштування!"
            ]
            return random.choice(templates)
        
        elif "Контекст" in title or "Context" in title:
            py_files = extract_value(raw_text, r"(?:Файли Пайтон|Python files):\s*(\d+)") or "68"
            test_files = extract_value(raw_text, r"(?:Тестові файли|Test files):\s*(\d+)") or "7"
            archive = "до речі, локальний архів коду повністю доступний" if "Так" in raw_text or "True" in raw_text else "але локальний архів коду чомусь недоступний"
            
            templates = [
                f"Так, команда, я розібрався з контекстом. Тут у нас {py_files} файлів мовою Пайтон та {test_files} файлів з тестами. І {archive}.",
                f"Звітую по контексту: бачу {py_files} пайтон-файлів та {test_files} файлів тестів. І {archive}. Робоча область готова.",
                f"Глянув контекст проекту. Загалом маємо {py_files} файлів Пайтон та {test_files} тестових файлів. {archive}."
            ]
            return random.choice(templates)
            
        elif "Налаштування" in title or "Setup" in title:
            py_ver = extract_value(raw_text, r"(?:Python):\s*([^\s(]*)") or "3.11"
            platform = "на мак о-ес" if "macOS" in raw_text or "mac" in raw_text.lower() else "на поточній системі"
            test_cmd = extract_value(raw_text, r"(?:Команда тестування|Test command):\s*(.*)")
            
            speech = f"По налаштуваннях: версія Пайтон {py_ver}, крутимося {platform}."
            if test_cmd:
                speech += f" Тести будемо запускати через команду {test_cmd}."
            
            templates = [
                f"Оточення готове! {speech} Можна працювати далі.",
                f"Перевірив сетап. {speech} Все налаштовано без проблем.",
                f"Звітую: сетап завершено. {speech} Колеги, які будуть думки?"
            ]
            return random.choice(templates)
            
        elif "Кроки запуску" in title or "Startup Steps" in title:
            templates = [
                "Запускаю первинні модулі. Робимо переднавантаження, підтягуємо контекст і готуємо хуки аудиту.",
                "Починаю ініціалізацію. Зчитуємо конфігурацію, готуємо реєстр команд та підвантажуємо хуки аудиту.",
                "Так, поїхали! Запускаємо ініціалізацію, будуємо дерево контексту та активуємо відкладену ініціалізацію."
            ]
            return random.choice(templates)

        elif "Знайдені маршрути" in title or "Routed Matches" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "Атлас тут. Народ, я перевірив реєстр, але підходящих команд чи інструментів не знайшов."
            
            matches = []
            for line in raw_text.split('\n'):
                if '—' in line or '(' in line:
                    parts = line.replace('•', '').replace('-', '').strip().split(' ')
                    if len(parts) > 1:
                        matches.append(parts[1])
            if matches:
                templates = [
                    f"Так, колеги, це Атлас. Я підібрав оптимальні маршрути: {', '.join(matches)}. Зараз їх запущу.",
                    f"Привіт! Атлас на зв'язку. Знайшов у реєстрі такі відповідності: {', '.join(matches)}. Беру їх в роботу.",
                    f"Дивіться, я просканував доступні команди та інструменти. Вибрав {', '.join(matches)}. Запускаю."
                ]
                return random.choice(templates)
            return "Знайшов робочі системні маршрути для виконання нашого завдання."
            
        elif "Виконання команд" in title or "Command Execution" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "Атлас повідомляє: на цьому кроці команди не запускалися."
            return f"Атлас звітує: виконую команду. Результат такий: {clean_text}."
            
        elif "Виконання інструментів" in title or "Tool Execution" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "Атлас на зв'язку. Інструменти не використовувалися."
            return f"Запускаю інструмент. Дивіться, отримав такий результат від системи: {clean_text}."

        elif "Дія" in title or "Action" in title or "Результат" in title or "Result" in title:
            return raw_text



    # 2. TETIANA (Coordinator / Other)
    elif voice == "tetiana":
        if "Ініціалізація системи" in title or "System Init" in title:
            if "порожня" in clean_text.lower() or not clean_text:
                return "Привіт усім, я Тетяна! Початкову ініціалізацію завершено, все чисто."
            cmds = extract_value(raw_text, r"(?:Завантажені записи команд|Loaded command entries):\s*(\d+)") or "207"
            tools = extract_value(raw_text, r"(?:Завантажені записи інструментів|Loaded tool entries):\s*(\d+)") or "184"
            templates = [
                f"Всім привіт, я Тетяна! Рада бачити команду. Систему успішно ініціалізовано. У нас завантажено {cmds} команд та {tools} інструментів.",
                f"Вітаю, колеги! На зв'язку Тетяна. Ініціалізація пройшла вдало: маємо в базі {cmds} команд і {tools} робочих інструментів. Працюємо!",
                f"Привіт, команда! Тетяна тут. Запуск відбувся штатно. Завантажила {cmds} команд та {tools} інструментів. Готові до першого ходу."
            ]
            return random.choice(templates)
        
        elif "Статус виконання" in title:
            templates = [
                f"Так, друзі, розпочинаю хід номер {clean_text}. Слідкуємо за оновленнями.",
                f"Починаю хід номер {clean_text}. Дивимось, що запропонує модель.",
                f"Переходимо до ходу номер {clean_text}. Колеги, підключайтеся."
            ]
            return random.choice(templates)

        elif "Потокові події" in title or "Stream Events" in title:
            return "Привіт! Це Тетяна. Отримую потокові дані від мовної моделі. Слухаю уважно."
            
        elif "Результат ходу" in title or "Turn Result" in title:
            stop_reason_raw = extract_value(raw_text, r"(?:stop_reason|причина зупинки)=\s*(\w+)") or "completed"
            stop_reason = TRANSLATE_MAP.get(stop_reason_raw, "виконання триває")
            denials = extract_value(raw_text, r"(?:Відмови доступу|Permission denials):\s*(\d+)") or "0"
            
            speech = "Це Тетяна. Я проаналізувала поточний хід. "
            if denials and int(denials) > 0:
                speech += f"Обережно! Зафіксовано {denials} відмов у дозволах на запуск інструментів! "
            speech += f"Статус виконання наразі: {stop_reason}."
            
            templates = [
                f"Колеги, Тетяна тут. {speech}",
                f"Всім привіт від Тетяни. {speech}",
                f"Так, команда, проглянула хід. {speech}"
            ]
            return random.choice(templates)
            
        elif "Історія сесії" in title or "Session History" in title:
            return "Тетяна завершила аналіз історії сесії. Всі дані збережено, колеги."

        elif "Аналіз" in title or "Analysis" in title:
            return raw_text

        elif "Результат" in title or "Result" in title:
            return raw_text

    # 3. GRISHA (Security Specialist)
    elif voice == "grisha":
        if "Результат інструменту" in title or "Tool Result" in title:
            return raw_text
            
        templates = [
            f"Увага! Маємо проблему з безпекою або системний збій. Гріша на зв'язку: {clean_text}.",
            f"Обережно, колеги! Це Гріша. Зафіксовано помилку або обмеження: {clean_text}.",
            f"Попередження від служби безпеки: {clean_text}. Перевірте конфігурацію!"
        ]
        return random.choice(templates)

    # Fallback to direct translation if no match
    translated = clean_text
    for eng, ua in TRANSLATE_MAP.items():
        translated = translated.replace(eng, ua)
    return translated


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

    def get_success_speech(self, action: str) -> str:
        import random
        templates = [
            f"Все чітко: {action} виконано в повному обсязі.",
            f"Інструмент {action} відпрацював на відмінно.",
            f"Завершили {action} без пригод, статус успішний.",
            f"Крок {action} виконано повністю, перешкод немає.",
            f"Усе пройшло штатно, {action} завершено.",
            f"Звітую: {action} завершено успішно, жодних багів.",
            f"Операція {action} відпрацювала без жодного пилу.",
            f"Завдання {action} виконано, рухаємося за планом.",
            f"Все готово по {action}, результат позитивний.",
            f"З {action} розібралися, все працює як годинник.",
            f"Крок {action} виконано успішно, рухаємося до наступного.",
            f"Інструмент {action} завершив роботу без нарікань.",
            f"Все зелене: {action} виконано без зауважень.",
            f"Звіт по кроку: {action} завершено вдало.",
            f"Все готово: {action} відпрацював без жодної помарки.",
            f"Чудово, {action} завершився з успішним результатом.",
            f"По кроку {action} все виконано, ніяких затримок.",
            f"Операцію {action} закрито успішно, все під контролем.",
            f"Все зроблено: {action} виконано на всі сто."
        ]
        return random.choice(templates)

    def get_failure_speech(self, action: str, error: str) -> str:
        import random
        templates = [
            f"Не вдалося виконати {action} через помилку: {error}.",
            f"Виникла помилка під час {action}. Деталі такі: {error}.",
            f"Запуск {action} завершився невдачею. Повідомлення системи: {error}.",
            f"На жаль, крок {action} провалився. Помилка: {error}.",
            f"Щось пішло не так із {action}. Маємо збій: {error}.",
            f"Зафіксовано помилку під час {action}: {error}.",
            f"Виникли проблеми з {action}. Опис збою: {error}.",
            f"Не вдалося завершити {action}. Помилка в системі: {error}.",
            f"Операція {action} впала з помилкою: {error}.",
            f"Збій на кроці {action}. Причина помилки: {error}.",
            f"Крок {action} не виконано. Отримали виняток: {error}.",
            f"Маємо невдалий запуск {action}. Деталі збою: {error}.",
            f"Виникла критична помилка в {action}: {error}.",
            f"Не вдалося виконати {action}. Помилка: {error}.",
            f"Операція {action} завершилась аварійно. Опис: {error}.",
            f"Крок {action} завершився з помилкою. Повідомлення: {error}.",
            f"Завдання {action} провалилося. Система каже: {error}.",
            f"Маємо збій у виконанні {action}. Помилка: {error}.",
            f"Звіт про помилку на кроці {action}: {error}.",
            f"Не вдалося опрацювати {action}. Деталі помилки: {error}.",
            f"Помилка при виконанні {action}. Опис помилки: {error}.",
            f"Крок {action} не пройшов. Причина збою: {error}.",
            f"Спроба запустити {action} завершилася помилкою: {error}.",
            f"Операція {action} заблокована через помилку: {error}.",
            f"Помилка під час кроку {action}. Код або опис: {error}.",
            f"Крок {action} закінчився збоєм. Деталі помилки: {error}.",
            f"Не змогли завершити {action} через виняток: {error}.",
            f"Виникла помилка в процесі {action}. Повідомлення: {error}.",
            f"Процес {action} перервано через помилку: {error}.",
            f"Запуск {action} провалився. Помилка виконання: {error}."
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
        if not is_actually_error and clean_out:
            if any(term in lower_out for term in ("traceback (most", "error:", "❌ помилка", "no such option:", "exception:", "command not found")):
                is_actually_error = True

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

        natural_text = make_natural_speech(voice, title, text)
        if not natural_text.strip():
            return

        # Translate and clean text for speech narration and display BEFORE printing
        natural_text = translate_to_ukrainian(natural_text, voice=voice)

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
        print(f"  {color}{natural_text}{reset}")
        print(f"{dim}{color}{'─' * 60}{reset}")

        # 2. Generate and play audio if TTS is available
        if self.tts_engine and os.environ.get("CLAW_TTS", "true").lower() != "false":
            import hashlib
            import shutil
            import re
            
            # Map narrator agents to Edge-TTS voices with distinct rate/pitch settings
            voice_settings = {
                "tetiana": ("uk-UA-PolinaNeural", "+3%", "+5Hz"),
                "atlas":   ("uk-UA-OstapNeural", "+5%", "+3Hz"),
                "grisha":  ("uk-UA-OstapNeural", "-8%", "-15Hz"),
            }
            # Allow overriding the voice settings via environment variables (e.g. CLAW_TTS_ATLAS_VOICE="tetiana")
            voice_override = os.environ.get(f"CLAW_TTS_{voice.upper()}_VOICE", voice)
            voice_val, rate_val, pitch_val = voice_settings.get(voice_override, voice_settings.get(voice, ("uk-UA-PolinaNeural", "+0%", "+0Hz")))
            
            # Prepare text for TTS (transcribe English terms, clean up formatting)
            speech_text = prepare_text_for_tts(natural_text)
            
            # Allow up to 3000 characters for full analysis/summaries to be read in full
            if len(speech_text) > 3000:
                speech_text = speech_text[:3000] + "... далі скорочено."

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
                        if time.time() - wait_start > 60.0:
                            print(f"\n⚠️  Попередження: виявлено застаріле блокування api.lock. Продовжуємо без очікування.")
                            try:
                                api_lock_path.unlink()
                            except Exception:
                                pass
                            break
                        if time.time() - wait_start > 5.0 and not warned:
                            print(f"\n⏳ Очікування завершення API запиту Claw (блокування api.lock)...")
                            warned = True
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
                
                # Play audio using afplay (built-in on macOS)
                time.sleep(0.1) # Даємо системі час синхронізувати файл на диск перед відтворенням
                subprocess.run(["afplay", str(wav_path)], check=True)
            except Exception as e:
                print(f"  {COLORS['grisha']}⚠️ Помилка відтворення аудіо: {e}{reset}")
            finally:
                if lock_path.exists():
                    try:
                        lock_path.unlink()
                    except Exception:
                        pass

    def finalize(self):
        """Concatenates all segments into one final file."""
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
        "check claw directory structure": "аналізу структури папок проекту",
        "list available skills": "перегляду доступних навичок",
        "count skill documentation files": "підрахунку файлів інструкцій для навичок",
        "check workspace size": "визначення обсягу папки проекту",
        "stop all claw-related processes": "зупинки всіх фонових процесів агента",
        "check if the build has completed": "перевірки результатів компіляції проекту",
        "check the task status and log file location": "перевірки стану запущених завдань",
        "get list of all connected disks and drives": "отримання списку всіх підключених дисків та накопичувачів",
    }
    
    desc_lower = desc.lower()
    for eng, ua in translations.items():
        if eng in desc_lower:
            return ua
            
    # Try general clean up of description
    desc_clean = desc.strip()
    if desc_clean:
        # Convert first word to noun if it is a common Ukrainian verb or noun in wrong case
        words = desc_clean.split()
        first_word = words[0].lower().rstrip(":,.") if words else ""
        
        verb_to_noun = {
            "перевірити": "перевірки",
            "перевірка": "перевірки",
            "переглянути": "перегляду",
            "перегляд": "перегляду",
            "знайти": "пошуку",
            "пошук": "пошуку",
            "пошукати": "пошуку",
            "запустити": "запуску",
            "запуск": "запуску",
            "створити": "створення",
            "створення": "створення",
            "записати": "запису",
            "запис": "запису",
            "редагувати": "редагування",
            "відредагувати": "редагування",
            "редагування": "редагування",
            "видалити": "видалення",
            "видалення": "видалення",
            "отримати": "отримання",
            "отримання": "отримання",
            "зчитати": "зчитування",
            "зчитування": "зчитування",
            "виконати": "виконання",
            "виконання": "виконання",
            "зупинити": "зупинки",
            "зупинка": "зупинки",
        }
        
        if first_word in verb_to_noun:
            words[0] = verb_to_noun[first_word]
            # Ensure the rest of the string has appropriate case/formatting
            return " ".join(words)

        if desc_clean.lower().startswith("check "):
            return "перевірки " + desc_clean[6:]
        elif desc_clean.lower().startswith("run "):
            return "запуску " + desc_clean[4:]
        elif desc_clean.lower().startswith("list "):
            return "отримання списку " + desc_clean[5:]
        elif desc_clean.lower().startswith("find "):
            return "пошуку " + desc_clean[5:]
        return desc_clean

    return "виконання системної операції"

def make_natural_tool_use(tool_name: str, input_str: str) -> tuple[str, str]:
    try:
        params = json.loads(input_str)
    except Exception:
        params = {}
        
    cmd = params.get("command", params.get("CommandLine", ""))
    desc = params.get("description", params.get("Description", ""))
    
    action_desc = ""
    spoken_text = ""
    
    if tool_name in ("bash", "run_command"):
        cmd_str = str(cmd).strip()
        desc_str = str(desc).strip()
        action_desc = get_command_description_ua(cmd_str, desc_str)
        spoken_text = f"Запускаю команду для {action_desc}..."
        
    elif tool_name in ("read_file", "view_file"):
        path = params.get("AbsolutePath", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        parent = Path(path).parent.name if path else ""
        if parent and parent != "claw-code":
            action_desc = f"читання файлу {filename} у папці {parent}"
            spoken_text = f"Так... Зчитую вміст файлу {filename} у папці {parent}... глянемо, який там код."
        else:
            action_desc = f"читання файлу {filename}"
            spoken_text = f"Так... Зчитую вміст файлу {filename}... глянемо, який там код."
        
    elif tool_name in ("write_to_file", "write_file", "create_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        parent = Path(path).parent.name if path else ""
        if parent and parent != "claw-code":
            action_desc = f"запису у файл {filename} у папці {parent}"
            spoken_text = f"Добре, створюю або перезаписую файл {filename} у папці {parent}..."
        else:
            action_desc = f"запису у файл {filename}"
            spoken_text = f"Добре, створюю або перезаписую файл {filename}..."
        
    elif tool_name in ("replace_file_content", "multi_replace_file_content", "edit_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        parent = Path(path).parent.name if path else ""
        if parent and parent != "claw-code":
            action_desc = f"редагування файлу {filename} у папці {parent}"
            spoken_text = f"Редагую файл {filename} у папці {parent}... зараз внесу потрібні зміни."
        else:
            action_desc = f"редагування файлу {filename}"
            spoken_text = f"Редагую файл {filename}... зараз внесу потрібні зміни."
        
    elif tool_name == "grep_search":
        query = params.get("Query", params.get("query", ""))
        action_desc = f"пошуку тексту '{query}' у коді"
        spoken_text = f"Шукаю фрагмент '{query}' у коді... сподіваюся, зараз знайдеться."
        
    elif tool_name == "glob_search":
        pattern = params.get("Pattern", params.get("pattern", ""))
        action_desc = f"пошуку файлів за шаблоном '{pattern}'"
        spoken_text = f"Шукаю файли за шаблоном '{pattern}' у структурі проекту..."
        
    elif tool_name == "list_dir":
        path = params.get("DirectoryPath", params.get("path", ""))
        dirname = Path(path).name if path else "директорії"
        parent = Path(path).parent.name if path else ""
        if parent and parent != "claw-code" and dirname:
            action_desc = f"перегляду вмісту папки {dirname} у папці {parent}"
            spoken_text = f"Гляну, які файли є в папці {dirname} у папці {parent}..."
        else:
            action_desc = f"перегляду вмісту папки {dirname}"
            spoken_text = f"Гляну, які файли є в папці {dirname}..."
        
    elif tool_name == "TaskGraph":
        op = params.get("operation", "")
        if op == "update_status":
            action_desc = "оновлення статусу завдань у чек-листі"
            spoken_text = "Оновлюю наш чек-лист... позначу виконане."
        else:
            action_desc = "оновлення списку завдань планування"
            spoken_text = "Оновлюю наш список завдань планування..."
            
    else:
        tool_name_ua = TOOL_NAMES_UA.get(tool_name, tool_name)
        action_desc = f"виконання інструменту {tool_name_ua}"
        spoken_text = f"Запускаю інструмент {tool_name_ua}..."
        
    return spoken_text, action_desc
        
def resolve_narration_api_config(model: str) -> tuple[str, str, str]:
    api_key = ""
    base_url = ""
    model_id = ""
    
    if model == "gemini-lite":
        base_url = os.environ.get("GEMINI_BASE_URL", "https://generativelanguage.googleapis.com/v1beta/openai/").rstrip('/')
        # Collect all available Gemini API Keys for rotation to avoid 429 rate limits
        keys = []
        primary_key = os.environ.get("GEMINI_API_KEY", "")
        if primary_key:
            keys.append(primary_key)
        for i in range(1, 10):
            k = os.environ.get(f"GEMINI_API_KEY{i}", "")
            if k:
                keys.append(k)
        if keys:
            api_key = random.choice(keys)
        else:
            api_key = ""
        model_id = "gemini-3.1-flash-lite"
    elif model in ("glm", "glm2", "glm3"):
        if model == "glm2":
            base_url = os.environ.get("GLM_BASE_URL2", os.environ.get("GLM_BASE_URL", "https://api.z.ai/api/paas/v4")).rstrip('/')
            api_key = os.environ.get("GLM_API_KEY2", os.environ.get("GLM_API_KEY", ""))
        elif model == "glm3":
            base_url = os.environ.get("GLM_BASE_URL3", os.environ.get("GLM_BASE_URL", "https://api.z.ai/api/paas/v4")).rstrip('/')
            api_key = os.environ.get("GLM_API_KEY3", os.environ.get("GLM_API_KEY", ""))
        else:
            base_url = os.environ.get("GLM_BASE_URL", "https://api.z.ai/api/paas/v4").rstrip('/')
            api_key = os.environ.get("GLM_API_KEY", "")
        model_id = "glm-4-flash"
    else:
        base_url = os.environ.get("OPENAI_BASE_URL", "https://openrouter.ai/api/v1").rstrip('/')
        api_key = os.environ.get("OPENAI_API_KEY", "")
        model_id = model

    # Fallback на ключі за замовчуванням
    if not api_key:
        api_key = os.environ.get("OPENAI_API_KEY", "")
            
    return base_url, api_key, model_id

def narrate_tool_result_via_llm(tool_name: str, action_desc: str, is_error: bool, output_val: str) -> str:
    if not output_val.strip() and not is_error:
        return ""

    model = os.environ.get("CLAW_NARRATION_MODEL", "gemini-lite")
    base_url, api_key, model_id = resolve_narration_api_config(model)

    if not base_url or not api_key:
        return ""

    # Limit output length to prevent payload bloat
    output_summary = output_val.strip()
    
    # Спробуємо розпарсити вивід як JSON, щоб дістати чистий stdout/stderr
    try:
        parsed_out = json.loads(output_val)
        if isinstance(parsed_out, dict):
            stdout = parsed_out.get("stdout", "")
            stderr = parsed_out.get("stderr", "")
            if stdout or stderr:
                output_summary = f"STDOUT:\n{stdout}\nSTDERR:\n{stderr}".strip()
            elif "nodes_updated" in parsed_out:
                output_summary = f"TaskGraph updated nodes: {parsed_out['nodes_updated']}"
            elif "output" in parsed_out:
                output_summary = str(parsed_out["output"]).strip()
    except Exception:
        pass

    if len(output_summary) > 1000:
        output_summary = output_summary[:1000] + "... [вивід скорочено]"

    url = f"{base_url}/chat/completions"
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}"
    }
    
    prompt_system = (
        "You are Grisha, a male Ukrainian software engineer and security/operations specialist. "
        "Summarize the outcome of a tool execution in a single, short, natural, highly conversational sentence in Ukrainian (UA). "
        "Tell the team clearly what happened. Did the search/read find the files or keys? Did the command fail or not find anything? "
        "State the actual verdict. RULES: 1. Talk like a friendly male tech teammate. Always use masculine verbs and forms when referring to yourself (e.g. 'я перевірив' instead of 'я перевірила', 'я не знайшов' instead of 'я не знайшла', 'я виконав' instead of 'я виконала'). "
        "2. Do NOT use English words or Latin letters at all. Translate every English term/file/variable name into its phonetic Ukrainian equivalent (e.g. 'config' -> 'конфіг', 'id_rsa' -> 'айді ер ес ей'). "
        "3. Be honest: if the output says 'not found' or is empty, state clearly that nothing was found, even if there was no exit error. Keep it under 25 words. Output ONLY the Ukrainian sentence."
    )
    
    error_context = "CRITICAL: The tool failed with an ERROR." if is_error else "The tool executed normally."
    prompt_user = f"The tool was run for: '{action_desc}'. {error_context} The raw output was: '{output_summary}'."
    
    payload = {
        "model": model_id,
        "messages": [
            {
                "role": "system",
                "content": prompt_system
            },
            {
                "role": "user",
                "content": prompt_user
            }
        ],
        "temperature": 0.5
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
            summary_text = res_data['choices'][0]['message']['content'].strip()
            if summary_text:
                return summary_text
    except Exception as e:
        print(f"\n⚠️ Помилка автоозвучки результату інструменту через {model}: {e}")
        
    return ""

def translate_and_summarize_thinking(text: str) -> str:
    if not text.strip():
        return ""

    model = os.environ.get("CLAW_NARRATION_MODEL", "gemini-lite")
    base_url, api_key, model_id = resolve_narration_api_config(model)

    if not base_url or not api_key:
        return ""

    url = f"{base_url}/chat/completions"
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}"
    }
    
    payload = {
        "model": model_id,
        "messages": [
            {
                "role": "system",
                "content": "You are a professional Ukrainian software engineer and a friendly teammate. Summarize the given English thinking process of an AI coding agent into a single, natural, highly conversational sentence in Ukrainian (UA). Talk like a real developer explaining what they are doing to a colleague. RULES: 1. Use a warm, relaxed, pair-programming teammate tone. Include natural conversational fillers and expressions (e.g., 'так...', 'отже...', 'схоже...', 'дивись...', 'ось...', 'чудово'). 2. Use ellipses ('...') to mark natural pauses, reflection, or transitions to make the speech feel alive and human. 3. Translate technical concepts into developer slang and write English terms phonetically in Ukrainian (e.g., 'main.rs' -> 'мейн крапка ер ес'). Keep it under 25 words. 4. IMPORTANT: Since this summary is voiced by a female narrator (Tetiana), always use feminine verbs when referring to yourself (e.g., 'я знайшла' instead of 'я знайшов', 'я розібралася' instead of 'я розібрався'). Output ONLY the resulting sentence."
            },
            {
                "role": "user",
                "content": text
            }
        ],
        "temperature": 0.5
    }
    
    for attempt in range(3):
        try:
            req = urllib.request.Request(
                url, 
                data=json.dumps(payload).encode('utf-8'), 
                headers=headers,
                method='POST'
            )
            with urllib.request.urlopen(req, timeout=10) as response:
                res_data = json.loads(response.read().decode('utf-8'))
                summary_text = res_data['choices'][0]['message']['content'].strip()
                if summary_text:
                    return summary_text
        except urllib.error.HTTPError as e:
            if e.code == 429 and attempt < 2:
                if model == "gemini-lite":
                    base_url, api_key, model_id = resolve_narration_api_config(model)
                    headers["Authorization"] = f"Bearer {api_key}"
                time.sleep(1.0)
                continue
            print(f"\\n⚠️ Помилка автоперекладу та підсумку думок через {model} (спроба {attempt+1}): {e}")
            break
        except Exception as e:
            print(f"\\n⚠️ Помилка автоперекладу та підсумку думок через {model} (спроба {attempt+1}): {e}")
            break
        
    return ""

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
        return f"Зараз я використаю інструмент {t_pron} для роботи з файлом {files[0]}."
    elif tools:
        t_name = tools[0]
        t_pron = tool_pronunciation.get(t_name.lower(), t_name)
        return f"Потрібно запустити системний інструмент {t_pron} для виконання цього кроку."
    elif files:
        return f"Вивчаю структуру проекту та аналізую зміни у файлі {files[0]}."
    elif commands:
        return f"Готую до виконання в терміналі команду {commands[0]}."
        
    # Identify agent intent based on keywords
    if any(k in text_lower for k in ["read", "view", "file", "content", "open"]):
        brief = random.choice([
            "Мені потрібно детальніше ознайомитися з вмістом файлів проєкту.",
            "Аналізую вміст файлів, щоб краще зрозуміти логіку роботи.",
            "Потрібно переглянути код у файлах для подальшого аналізу."
        ])
    elif any(k in text_lower for k in ["search", "find", "glob", "grep", "locate"]):
        brief = random.choice([
            "Проводжу пошук потрібних файлів та аналізую структуру коду.",
            "Шукаю необхідні компоненти та файли у проекті.",
            "Виконую пошук за ключовими словами у коді."
        ])
    elif any(k in text_lower for k in ["task", "plan", "graph", "roadmap", "todo"]):
        brief = random.choice([
            "Оновлюю план дій та структуризую наступні кроки для виконання завдання.",
            "Коригую наш чек-лист та планую подальші кроки.",
            "Аналізую поточні завдання та оновлюю план роботи."
        ])
    elif any(k in text_lower for k in ["test", "run", "build", "execute", "compile"]):
        brief = random.choice([
            "Готуюся до запуску тестів або збірки проєкту для перевірки працездатності.",
            "Перевіряю працездатність коду шляхом запуску тестів.",
            "Запускаю збірку проекту, щоб переконатися у відсутності помилок."
        ])
    elif any(k in text_lower for k in ["fix", "bug", "error", "modify", "replace", "edit"]):
        brief = random.choice([
            "Планую внесення виправлень або редагування коду для усунення проблеми.",
            "Готую зміни до коду для виправлення виявлених помилок.",
            "Потрібно відредагувати код для усунення цієї проблеми."
        ])
    else:
        brief = random.choice([
            "Аналізую поточний стан системи та обмірковую наступні кроки.",
            "Розглядаю можливі варіанти розв'язання задачі.",
            "Визначаю оптимальний шлях вирішення проблеми."
        ])
        
    return brief

def translate_to_ukrainian(text: str, voice: str = "tetiana") -> str:
    if not text.strip():
        return text

    # Check if text needs translation or cleaning for TTS.
    # 1. Contains Latin characters (files, commands, emails, keys)
    # 2. Contains Russian characters (ы, э, ъ, ё)
    # 3. Contains markdown structures (headers, tables, bold list items, horizontal lines)
    # 4. Contains paths or numbers/tokens
    needs_processing = False
    if re.search(r'[a-zA-Z]', text):
        needs_processing = True
    elif re.search(r'[ыЫэЭъЪёЁ]', text):
        needs_processing = True
    elif re.search(r'[|#*`_─━]', text):  # Markdown/visual separators
        needs_processing = True
    elif re.search(r'/\w+/', text) or re.search(r'\\\w+', text): # Paths
        needs_processing = True
    elif re.search(r'\d{8,}', text): # Long numbers/tokens
        needs_processing = True
    elif '@' in text: # Emails
        needs_processing = True

    if not needs_processing:
        # Check cyrillic proportion
        cyrillic_chars = len(re.findall(r'[а-яА-ЯёЁєЄіІїЇґҐ]', text))
        total_chars = len(re.sub(r'\s+', '', text))
        if total_chars > 0 and (cyrillic_chars / total_chars) > 0.4:
            if re.search(r'[єЄіІїЇґҐ]', text):
                return text

    model = os.environ.get("CLAW_NARRATION_MODEL", "gemini-lite")
    base_url, api_key, model_id = resolve_narration_api_config(model)

    if not base_url or not api_key:
        return text

    url = f"{base_url}/chat/completions"
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}"
    }
    
    if voice == "tetiana":
        gender_rules = "IMPORTANT: Since this translation is voiced by a female narrator (Tetiana), always use feminine verbs and forms when referring to yourself (e.g., 'я зробила', 'я знайшла'). Use a warm, natural, friendly teammate tone. Include conversational filler words and expressions (e.g., 'так...', 'отже...', 'схоже...', 'дивіться...', 'чудово'). Use ellipses ('...') to add natural pauses and breaths."
    else:
        gender_rules = "IMPORTANT: Since this translation is voiced by a male narrator (Atlas/Grisha), always use masculine verbs and forms when referring to yourself (e.g., 'я зробив', 'я знайшов'). Use a warm, natural, friendly teammate tone. Include conversational filler words and expressions (e.g., 'так...', 'отже...', 'дивись...', 'схоже...', 'супер'). Use ellipses ('...') to add natural pauses and breaths."

    payload = {
        "model": model_id,
        "messages": [
            {
                "role": "system",
                "content": f"You are a professional Ukrainian software engineer and narrator. Translate or rewrite the given text into natural, fluent Ukrainian (UA). RULES: 1. Talk like a friendly tech teammate speaking to a colleague. Translate programming concepts and standard terms directly into natural Ukrainian developer slang (e.g. 'concurrency' -> 'паралельність', 'performance' -> 'продуктивність', 'cache' -> 'кеш', 'bug' -> 'баг', 'error' -> 'помилка'). 2. Do NOT use any English words or Latin letters. Translate every English code element, file name, path, variable, class/function name, command or tool name into its phonetic Ukrainian equivalent (e.g., 'run_claw.sh' -> 'ран клоу крапка ес ейч', 'VoicePlayer' -> 'войс плеєр', 'grep_search' -> 'ґреп серч', 'git status' -> 'ґіт статус'). 3. {gender_rules} 4. IMPORTANT FOR SPEECH SYNTHESIS (TTS): This text will be read aloud. You MUST strip out or simplify all heavy technical visual elements. Do NOT read long SSH keys, API bot tokens, email lists, full path directories, or long numeric IDs literally. Replace them with brief natural Ukrainian summaries (e.g., 'ssh-ed25519 AAA...' -> 'публічний ключ деплою', 'dima1203@gmail.com' -> 'електронні пошти отримувачів', '/home/dima/scripts/x.py' -> 'скрипт ікс', '8562512293:AAEX...' -> 'токен телеграм-бота'). Remove all markdown structures, headers, lists, and tables, converting them into smooth, conversational, easy-to-read paragraphs. Output ONLY the clean, speech-friendly Ukrainian text, with no introductory or concluding remarks."
            },
            {
                "role": "user",
                "content": text
            }
        ],
        "temperature": 0.3
    }
    
    for attempt in range(3):
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
            if e.code == 429 and attempt < 2:
                if model == "gemini-lite":
                    base_url, api_key, model_id = resolve_narration_api_config(model)
                    headers["Authorization"] = f"Bearer {api_key}"
                time.sleep(1.0)
                continue
            print(f"\\n⚠️ Помилка автоперекладу через {model} (спроба {attempt+1}): {e}")
            break
        except Exception as e:
            print(f"\\n⚠️ Помилка автоперекладу через {model} (спроба {attempt+1}): {e}")
            break
        
    return text

def is_tool_call_text(text: str) -> bool:
    text_lower = text.lower().strip()
    return (
        text_lower.startswith("[assistant called") or 
        text_lower.startswith("[асистент викликав") or
        "called tool '" in text_lower or
        "викликав інструмент" in text_lower
    )

def process_session_entry(data: dict, player: VoicePlayer):
    entry_type = data.get("type")
    if entry_type == "session_meta":
        model = data.get("model", "невідома модель")
        player.speak("tetiana", "Системний запуск", f"Розпочато нову сесію основного клієнта Claw за допомогою моделі {model}.")
        
    elif entry_type == "prompt_history":
        text = data.get("text", "")
        if text:
            player.speak("atlas", "Запит", f"Отримано новий запит від користувача: {text}")
            
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
                        translated_content = translate_to_ukrainian(text_content, voice="atlas")
                        player.speak("atlas", "Результат", translated_content)
                elif block_type == "tool_use":
                    tool_name = block.get("name", "")
                    input_str = block.get("input", "")
                    if tool_name:
                        natural_tool, action_desc = make_natural_tool_use(tool_name, input_str)
                        player.last_action_desc = action_desc
                        player.last_action_tool = tool_name
                        player.speak("atlas", "Дія", natural_tool)
                        
        elif role == "tool":
            for block in blocks:
                block_type = block.get("type")
                if block_type == "tool_result":
                    tool_name = block.get("tool_name", "")
                    is_error = block.get("is_error", False)
                    output_val = block.get("output", "")
                    
                    action_desc = getattr(player, "last_action_desc", "")
                    if not action_desc or getattr(player, "last_action_tool", "") != tool_name:
                        action_desc = TOOL_NAMES_UA.get(tool_name, tool_name)
                        
                    has_error_traces = False
                    if not is_error and output_val:
                        lower_out = output_val.lower()
                        # Detect hidden failures (e.g. script errors reported as success)
                        if any(term in lower_out for term in ("traceback (most", "error:", "❌ помилка", "no such option:", "exception:", "command not found")):
                            has_error_traces = True
                            
                    speech = player.get_tool_verdict_speech(tool_name, action_desc, is_error, output_val)
                    player.speak("grisha", "Результат інструменту", speech)

def tail_session_loop():
    caller_cwd = os.environ.get("CLAW_CALLER_CWD")
    if caller_cwd:
        sessions_dir = Path(caller_cwd) / ".claw" / "sessions"
    else:
        sessions_dir = original_cwd / ".claw" / "sessions"
        
    if not sessions_dir.exists():
        sessions_dir = project_root / ".claw" / "sessions"

    print(f"{COLORS['bold']}{COLORS['system']}🎙️ Режим реального часу (Tailing Mode) запущено.{COLORS['reset']}")
    print(f"{COLORS['system']}Очікування нових записів у сесіях...{COLORS['reset']}\n")
    
    latest_file = None
    while not latest_file:
        latest_file = find_latest_session_file(sessions_dir)
        if not latest_file:
            time.sleep(1.0)
            
    print(f"{COLORS['system']}👀 Стеження за файлом сесії: {latest_file}{COLORS['reset']}\n")
    
    audio_dir = project_root / "audio_output"
    player = VoicePlayer(audio_dir)
    
    with open(latest_file, "r") as f:
        # Seek to the end of the file immediately to only process new entries
        f.seek(0, 2)
            
        try:
            while True:
                line = f.readline()
                if not line:
                    # Перевіряємо чи не з'явився новий файл сесії
                    current_latest = find_latest_session_file(sessions_dir)
                    if current_latest and current_latest != latest_file:
                        print(f"\n{COLORS['system']}🔄 Виявлено нову активну сесію: {current_latest}{COLORS['reset']}")
                        latest_file = current_latest
                        f.close()
                        f = open(latest_file, "r")
                        continue
                    
                    time.sleep(0.5)
                    # Скидаємо EOF прапор для зчитування нових рядків
                    f.seek(f.tell())
                    continue
                
                try:
                    data = json.loads(line)
                    process_session_entry(data, player)
                except Exception:
                    pass
        except KeyboardInterrupt:
            print(f"\n{COLORS['system']}🛑 Озвучку в реальному часі зупинено.{COLORS['reset']}")
            player.finalize()


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

    run_narrated_session(prompt)


if __name__ == "__main__":
    main()
