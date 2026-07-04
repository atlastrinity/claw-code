#!/usr/bin/env python3
"""
🎙️ CLAW Voice Narrator — Природна озвучка виводу claw-code українськими голосами

Кожна секція виводу програми озвучується відповідним «агентом»:
  🎙️ Тетяна (Диктор)     — загальні повідомлення, заголовки, ініціалізація
  ⚙️ Дмитро (Рантайм)    — контекст, setup, кроки запуску
  🔍 Олекса (Маршрутизатор) — пошук команд/інструментів, виконання
  📊 Лада   (Аналітик)    — результати ходу, статистика, історія
  🛡️ Микита (Безпека)     — відмови доступу, критичні попередження

Якщо процес не завершився повністю, озвучка продовжується у наступних ходах.
"""

from __future__ import annotations

import os
import re
import sys
import time
import json
import subprocess
from pathlib import Path
from typing import Optional, List

# Додаємо корінь проекту до шляху імпорту
project_root = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(project_root))

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
    "tetiana":  "\033[95m",   # Magenta — Диктор
    "dmytro":   "\033[96m",   # Cyan — Рантайм
    "oleksa":   "\033[93m",   # Yellow — Маршрутизатор
    "lada":     "\033[92m",   # Green — Аналітик
    "mykyta":   "\033[91m",   # Red — Безпека
    "system":   "\033[94m",   # Blue — Системні логи
    "reset":    "\033[0m",
    "bold":     "\033[1m",
    "dim":      "\033[2m",
}

AGENT_EMOJI = {
    "tetiana": "🎙️",
    "dmytro":  "⚙️",
    "oleksa":  "🔍",
    "lada":    "📊",
    "mykyta":  "🛡️",
}

AGENT_NAME_UA = {
    "tetiana": "Тетяна · Диктор",
    "dmytro":  "Дмитро · Рантайм-агент",
    "oleksa":  "Олекса · Маршрутизатор",
    "lada":    "Лада · Аналітик",
    "mykyta":  "Микита · Безпека",
}

# ──────────────────────────── Translation Helpers ────────────────────────

TRANSLATE_MAP = {
    "completed": "завершено успішно",
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
    
    if voice == "dmytro":  # Runtime
        if "Сесія" in title or "Session" in title or "Запит" in title:
            prompt = extract_value(raw_text, r"(?:Запит|Prompt):\s*(.*)") or clean_text
            return f"Вітаю. Рантайм-агент Дмитро готовий до роботи. Отримано новий запит на аналіз: {prompt}."
        
        elif "Контекст" in title or "Context" in title:
            py_files = extract_value(raw_text, r"(?:Файли Пайтон|Python files):\s*(\d+)") or "68"
            test_files = extract_value(raw_text, r"(?:Тестові файли|Test files):\s*(\d+)") or "7"
            archive = "доступний" if "Так" in raw_text or "True" in raw_text else "наразі недоступний"
            
            return (f"Проводжу сканування робочого простору. Виявлено {py_files} файлів мовою Пайтон "
                    f"та {test_files} файлів з тестами. Локальний архів коду {archive}.")
            
        elif "Налаштування" in title or "Setup" in title:
            py_ver = extract_value(raw_text, r"(?:Python):\s*([^\s(]*)") or "3.11"
            platform = "на платформі мак о-ес" if "macOS" in raw_text or "mac" in raw_text.lower() else "на робочій платформі"
            test_cmd = extract_value(raw_text, r"(?:Команда тестування|Test command):\s*(.*)")
            
            speech = f"Середовище повністю підготовлено. Версія Пайтон {py_ver}, працюємо {platform}."
            if test_cmd:
                speech += f" Тестування налаштовано через команду: {test_cmd}."
            return speech
            
        elif "Кроки запуску" in title or "Startup Steps" in title:
            return ("Починаю ініціалізацію модулів. Виконую запуск попереднього завантаження, "
                    "будую контекст робочого простору, завантажую знімки команд та інструментів, "
                    "після чого готую хуки аудиту та застосовую відкладену ініціалізацію.")

    elif voice == "tetiana":  # Narrator
        if "Ініціалізація системи" in title or "System Init" in title:
            if "порожня" in clean_text.lower() or not clean_text:
                return "Початкова ініціалізація системи завершена без зауважень."
            cmds = extract_value(raw_text, r"(?:Завантажені записи команд|Loaded command entries):\s*(\d+)") or "207"
            tools = extract_value(raw_text, r"(?:Завантажені записи інструментів|Loaded tool entries):\s*(\d+)") or "184"
            return f"Систему успішно ініціалізовано. Усі системи готові до роботи. Завантажено {cmds} команд та {tools} інструментів."

    elif voice == "oleksa":  # Router
        if "Знайдені маршрути" in title or "Routed Matches" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "Маршрутизатор Олекса повідомляє: підходящих команд або інструментів для цього запиту не знайдено."
            
            matches = []
            for line in raw_text.split('\n'):
                if '—' in line or '(' in line:
                    parts = line.replace('•', '').replace('-', '').strip().split(' ')
                    if len(parts) > 1:
                        matches.append(parts[1])
            if matches:
                return f"Маршрутизатор знайшов наступні відповідності в реєстрі: {', '.join(matches)}."
            return "Знайдено відповідні системні маршрути для обробки запиту."
            
        elif "Виконання команд" in title or "Command Execution" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "Жодних зовнішніх команд на цьому кроці не виконувалось."
            return f"Проводжу виконання відповідної команди. Результат роботи наступний: {clean_text}."
            
        elif "Виконання інструментів" in title or "Tool Execution" in title:
            if "нічого" in clean_text.lower() or "none" in clean_text.lower() or not clean_text:
                return "Жодних допоміжних інструментів не було запущено."
            return f"Запускаю необхідні інструменти. Отримано наступну відповідь від системи: {clean_text}."

    elif voice == "lada":  # Analyst
        if "Потокові події" in title or "Stream Events" in title:
            return "Починаю приймати потокові події від мовної моделі. Дані надходять у реальному часі."
            
        elif "Результат ходу" in title or "Turn Result" in title:
            stop_reason_raw = extract_value(raw_text, r"(?:stop_reason|причина зупинки)=\s*(\w+)") or "completed"
            stop_reason = TRANSLATE_MAP.get(stop_reason_raw, "виконання триває")
            denials = extract_value(raw_text, r"(?:Відмови доступу|Permission denials):\s*(\d+)") or "0"
            
            speech = "Аналітик Лада на зв'язку. Отримано результат поточного кроку. "
            if denials and int(denials) > 0:
                speech += f"Увага! Зафіксовано {denials} відмов у дозволах на виконання інструментів! "
            speech += f"Статус виконання ходу: {stop_reason}."
            return speech
            
        elif "Історія сесії" in title or "Session History" in title:
            return f"Аналіз сесії завершено. Всі дані та контекст збережено у сховище сесій."

    elif voice == "mykyta":  # Security
        return f"Увага! Обмеження безпеки. Виявлено наступну загрозу або відмову: {clean_text}."

    # Fallback to direct translation if no match
    translated = clean_text
    for eng, ua in TRANSLATE_MAP.items():
        translated = translated.replace(eng, ua)
    return translated


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
        
        # Load TTS
        try:
            from ukrainian_tts.tts import TTS
            print(f"\n{COLORS['system']}⏳ Завантаження TTS моделі українського мовлення...{COLORS['reset']}")
            self.tts_engine = TTS(device="cpu")
            print(f"{COLORS['system']}✅ TTS модель успішно завантажена!{COLORS['reset']}\n")
        except ImportError:
            print(f"\n{COLORS['mykyta']}⚠️ Бібліотека ukrainian-tts не встановлена.{COLORS['reset']}")
            print("Озвучка відбуватиметься лише в текстовому режимі.")
        except Exception as e:
            print(f"\n{COLORS['mykyta']}⚠️ Помилка ініціалізації TTS: {e}{COLORS['reset']}")

    def get_success_speech(self, action: str) -> str:
        templates = [
            f"Дію з {action} виконано успішно.",
            f"Операцію з {action} завершено успішно.",
            f"Успішно завершено {action}.",
            f"Крок із {action} виконано без помилок.",
        ]
        speech = templates[self.success_index % len(templates)]
        self.success_index += 1
        return speech

    def get_failure_speech(self, action: str, error: str) -> str:
        templates = [
            f"Не вдалося виконати {action} через помилку: {error}.",
            f"Виникла помилка під час {action}: {error}.",
            f"Операція з {action} завершилася невдачею. Причина: {error}.",
        ]
        speech = templates[self.failure_index % len(templates)]
        self.failure_index += 1
        return speech

    def speak(self, voice: str, title: str, text: str):
        """Generates natural text, prints it, and plays the audio."""
        natural_text = make_natural_speech(voice, title, text)
        if not natural_text.strip():
            return

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
        if self.tts_engine:
            from ukrainian_tts.tts import Voices, Stress
            import hashlib
            import shutil
            
            voice_map = {
                "tetiana": Voices.Tetiana.value,
                "dmytro":  Voices.Dmytro.value,
                "oleksa":  Voices.Oleksa.value,
                "lada":    Voices.Tetiana.value,
                "mykyta":  Voices.Mykyta.value,
            }
            voice_val = voice_map.get(voice, Voices.Tetiana.value)
            
            # Limit TTS chunk length to avoid crashes
            speech_text = natural_text
            if len(speech_text) > 450:
                speech_text = speech_text[:450] + "... далі скорочено."

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
                    while api_lock_path.exists():
                        time.sleep(0.1)
                        
                    with open(cached_wav_path, "wb") as f:
                        self.tts_engine.tts(speech_text, voice_val, Stress.Dictionary.value, f)
                
                # Copy cached file to session play file
                wav_path = self.output_dir / f"play_{self.seg_count:03d}_{voice}_{title[:15].replace(' ', '_')}.wav"
                shutil.copyfile(str(cached_wav_path), str(wav_path))
                
                # Store cached path for final narration compilation
                self.wav_paths.append(cached_wav_path)
                self.seg_count += 1
                
                # Keep only the last 10 play_*.wav files in the output directory
                play_files = sorted(list(self.output_dir.glob("play_*.wav")))
                while len(play_files) > 10:
                    oldest = play_files.pop(0)
                    try:
                        oldest.unlink()
                    except Exception:
                        pass
                
                # Play audio using afplay (built-in on macOS)
                subprocess.run(["afplay", str(wav_path)], check=True)
            except Exception as e:
                print(f"  {COLORS['mykyta']}⚠️ Помилка відтворення аудіо: {e}{reset}")
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
            print(f"\n{COLORS['mykyta']}⚠️  Не вдалось склеїти повний аудіозапис: {e}{COLORS['reset']}")


# ──────────────────────────── Dynamic Turn Loop Narrator ─────────────────

def run_narrated_session(prompt: str, max_turns: int = 3):
    audio_dir = project_root / "audio_output"
    player = VoicePlayer(audio_dir)

    # --- 1. Startup & Setup (Dmytro & Tetiana) ---
    player.speak("dmytro", "Запит", f"Запит: {prompt}")
    
    # Context
    context = build_port_context()
    raw_ctx = f"Файли Пайтон: {context.python_file_count}\nТестові файли: {context.test_files_count if hasattr(context, 'test_files_count') else 7}\nАрхів доступний: {'Так' if context.archive_available else 'Ні'}"
    player.speak("dmytro", "Контекст", raw_ctx)

    # Setup
    setup_report = run_setup(trusted=True)
    setup = setup_report.setup
    raw_setup = f"Python: {setup.python_version}\nPlatform: {setup.platform_name}\nTest command: {setup.test_command}"
    player.speak("dmytro", "Налаштування", raw_setup)
    
    # Startup steps
    player.speak("dmytro", "Кроки запуску", "")

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

    # Speak matches (Oleksa)
    raw_matches = "\n".join(f"• {m.kind} {m.name}" for m in matches) if matches else "none"
    player.speak("oleksa", "Знайдені маршрути", raw_matches)

    # Loop through turns dynamically
    process_finished = False
    
    for turn in range(max_turns):
        player.speak("tetiana", "Статус виконання", f"Розпочинаю хід номер {turn + 1}...")
        
        turn_prompt = prompt if turn == 0 else f'{prompt} [turn {turn + 1}]'
        
        # Execute turn
        result = engine.submit_message(turn_prompt, command_names, tool_names, ())
        
        # Narrate what was executed (Oleksa)
        if result.matched_commands:
            player.speak("oleksa", "Виконання команд", f"Виконую наступні команди: {', '.join(result.matched_commands)}")
        if result.matched_tools:
            player.speak("oleksa", "Виконання інструментів", f"Запускаю відповідні інструменти: {', '.join(result.matched_tools)}")

        # Narrate turn results (Lada)
        player.speak("lada", "Результат ходу", f"stop_reason={result.stop_reason}\nВідмови доступу={len(result.permission_denials)}")
        
        # Check if the process is finished
        if result.stop_reason == 'completed':
            process_finished = True
            player.speak("tetiana", "Статус процесу", "Усі завдання поточного процесу успішно виконано.")
            break
        else:
            player.speak("mykyta", "Процес не закінчився", 
                         f"Процес зупинено з причиною: {result.stop_reason}. Процес не закінчився, озвучка продовжується.")
            time.sleep(1.0)

    # --- 3. Finalization ---
    if process_finished:
        player.speak("tetiana", "Завершення роботи", "Всі кроки успішно виконано. Озвучку повністю завершено. До зустрічі!")
    else:
        player.speak("mykyta", "Критичне завершення", "Увага! Досягнуто ліміту спроб, але процес так і не завершився. Озвучку зупинено у зв'язку з тайм-аутом.")

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
        spoken_text = f"Виконую команду для {action_desc}."
        
    elif tool_name in ("read_file", "view_file"):
        path = params.get("AbsolutePath", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        action_desc = f"читання файлу {filename}"
        spoken_text = f"Зчитую вміст файлу {filename} для аналізу його коду."
        
    elif tool_name in ("write_to_file", "write_file", "create_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        action_desc = f"запису у файл {filename}"
        spoken_text = f"Створюю або перезаписую файл {filename} з новими налаштуваннями."
        
    elif tool_name in ("replace_file_content", "multi_replace_file_content", "edit_file"):
        path = params.get("TargetFile", params.get("path", ""))
        filename = Path(path).name if path else "файлу"
        action_desc = f"редагування файлу {filename}"
        spoken_text = f"Вношу необхідні зміни та редагую код у файлі {filename}."
        
    elif tool_name == "grep_search":
        query = params.get("Query", params.get("query", ""))
        action_desc = f"пошуку тексту '{query}' у коді"
        spoken_text = f"Шукаю фрагмент коду із запитом '{query}' по всьому репозиторію."
        
    elif tool_name == "glob_search":
        pattern = params.get("Pattern", params.get("pattern", ""))
        action_desc = f"пошуку файлів за шаблоном '{pattern}'"
        spoken_text = f"Шукаю файли за шаблоном '{pattern}' у структурі проекту."
        
    elif tool_name == "list_dir":
        path = params.get("DirectoryPath", params.get("path", ""))
        dirname = Path(path).name if path else "директорії"
        action_desc = f"перегляду вмісту папки {dirname}"
        spoken_text = f"Отримую список файлів у папці {dirname}."
        
    elif tool_name == "TaskGraph":
        op = params.get("operation", "")
        if op == "update_status":
            action_desc = "оновлення статусу завдань у чек-листі"
            spoken_text = "Оновлюю статус завдань у нашому чек-листі."
        else:
            action_desc = "оновлення списку завдань планування"
            spoken_text = "Вношу нові завдання до нашого чек-листа планування."
            
    else:
        tool_name_ua = TOOL_NAMES_UA.get(tool_name, tool_name)
        action_desc = f"виконання інструменту {tool_name_ua}"
        spoken_text = f"Запускаю системний інструмент {tool_name_ua}."
        
    return spoken_text, action_desc

def summarize_thinking_ua(thinking_text: str) -> str:
    if not thinking_text.strip():
        return "Проводжу обмірковування наступних кроків."
        
    text = clean_for_speech(thinking_text)
    sentences = re.split(r'(?<=[.!?])\s+', text)
    brief = " ".join(sentences[:2])
    
    translations = {
        "Користувач просить": "Користувач запитує",
        "Користувач хоче": "Користувач бажає",
        "Почну з": "Почну з",
        "Нужно проверить": "Необхідно перевірити",
        "Для этого": "Для цього",
        "Мне нужно": "Мені потрібно",
        "Сначала": "Спочатку",
        "Затем": "Потім",
        "В первую очередь": "Перш за все",
        "Посмотрю на": "Подивлюся на",
        "Проверю": "Перевірю",
        "Запущу": "Запущу",
        "Попробую": "Спробую",
        "I need to": "Mental need",
        "First, I will": "First, I will",
        "Next, I will": "Next, I will",
        "Then I will": "Then I will",
        "To do this": "To do this",
        "Let me check": "Let me check",
    }
    
    # Wait, let's write correct translation logic in summarize_thinking_ua
    translations = {
        "Користувач просить": "Користувач запитує",
        "Користувач хоче": "Користувач бажає",
        "Почну з": "Почну з",
        "Нужно проверить": "Необхідно перевірити",
        "Для этого": "Для цього",
        "Мне нужно": "Мені потрібно",
        "Сначала": "Спочатку",
        "Затем": "Потім",
        "В первую очередь": "Перш за все",
        "Посмотрю на": "Подивлюся на",
        "Проверю": "Перевірю",
        "Запущу": "Запущу",
        "Попробую": "Спробую",
        "I need to": "Мені потрібно",
        "First, I will": "Спочатку я",
        "Next, I will": "Далі я",
        "Then I will": "Потім я",
        "To do this": "Для цього",
        "Let me check": "Дозвольте перевірити",
    }
    
    for eng_ru, ua in translations.items():
        brief = re.sub(r'\b' + re.escape(eng_ru) + r'\b', ua, brief, flags=re.IGNORECASE)
        
    replacements = [
        ("системы", "системи"),
        ("ресурсы", "ресурси"),
        ("файлы", "файли"),
        ("команды", "команди"),
        ("запрос", "запит"),
        ("загружена", "завантажена"),
        ("свободно", "вільно"),
        ("памяти", "пам'яті"),
        ("диска", "диска"),
        ("процессов", "процесів"),
        ("инструменты", "інструменти"),
    ]
    for src, dst in replacements:
        brief = re.sub(r'\b' + re.escape(src) + r'\b', dst, brief, flags=re.IGNORECASE)

    if len(brief) > 200:
        brief = brief[:200] + "..."
        
    return f"Обмірковую план дій. {brief}"

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
            player.speak("dmytro", "Запит", f"Отримано новий запит від користувача: {text}")
            
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
                    if has_real_text:
                        continue
                    thinking_val = block.get("thinking", "")
                    natural_thinking = summarize_thinking_ua(thinking_val)
                    player.speak("lada", "Аналіз", natural_thinking)
                elif block_type == "text":
                    text_content = block.get("text", "")
                    if text_content and not is_tool_call_text(text_content):
                        player.speak("lada", "Результат", text_content)
                elif block_type == "tool_use":
                    tool_name = block.get("name", "")
                    input_str = block.get("input", "")
                    if tool_name:
                        natural_tool, action_desc = make_natural_tool_use(tool_name, input_str)
                        player.last_action_desc = action_desc
                        player.last_action_tool = tool_name
                        player.speak("oleksa", "Дія", natural_tool)
                        
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
                        
                    if is_error:
                        error_msg = clean_error_message(output_val)
                        speech = player.get_failure_speech(action_desc, error_msg)
                    else:
                        speech = player.get_success_speech(action_desc)
                        
                    player.speak("dmytro", "Результат інструменту", speech)

def tail_session_loop():
    sessions_dir = Path.cwd() / ".claw" / "sessions"
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
                    f.seek(0, 1)
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
