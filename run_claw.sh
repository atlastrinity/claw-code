#!/bin/bash

# Збережемо оригінальну директорію, звідки запустили скрипт
export CLAW_CALLER_CWD="$PWD"
export CLAW_BYPASS_WORKSPACE_CHECK="${CLAW_BYPASS_WORKSPACE_CHECK:-true}"
export CLICOLOR_FORCE="${CLICOLOR_FORCE:-1}"
export FORCE_COLOR="${FORCE_COLOR:-true}"

# Змінюємо робочу директорію на ту, де знаходиться сам скрипт
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# Знаходимо інтерпретатор Python із підтримкою edge_tts
PYTHON_BIN=""
for candidate in "$HOME/.pyenv/shims/python3" "$(command -v python3 2>/dev/null)" "/usr/bin/python3" "/opt/homebrew/bin/python3"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ] && "$candidate" -c "import edge_tts" >/dev/null 2>&1; then
        PYTHON_BIN="$candidate"
        break
    fi
done
if [ -z "$PYTHON_BIN" ]; then
    if [ -x "/opt/homebrew/bin/python3" ]; then
        PYTHON_BIN="/opt/homebrew/bin/python3"
    else
        PYTHON_BIN="python3"
    fi
fi

# Завантажуємо змінні оточення (API ключі тощо) з різних джерел у порядку пріоритету
# 1. Глобальний .env у домашній папці
if [ -f "$HOME/.claw/.env" ]; then
    set -a
    source "$HOME/.claw/.env"
    set +a
fi

# 2. .env у папці встановлення скрипта
if [ -f "$SCRIPT_DIR/.env" ]; then
    set -a
    source "$SCRIPT_DIR/.env"
    set +a
fi

# 3. Локальний .env у директорії, звідки запустили команду (пріоритетний)
if [ -n "$CLAW_CALLER_CWD" ] && [ -f "$CLAW_CALLER_CWD/.env" ]; then
    set -a
    source "$CLAW_CALLER_CWD/.env"
    set +a
fi

export CLICOLOR_FORCE="${CLICOLOR_FORCE:-1}"
export FORCE_COLOR="${FORCE_COLOR:-true}"

# Синхронізуємо конфігурації (settings, CLAW.md, .env, skills) з глобальною папкою перед запуском
source "${SCRIPT_DIR}/scripts/lib_sync.sh"
sync_all "$SCRIPT_DIR"

# Очищаємо файли планування та завдань для нової сесії, якщо встановлено CLAW_NEW_SESSION
if [ "$CLAW_NEW_SESSION" = "true" ]; then
    echo "🧹 Очищення файлів завдань попередньої сесії..."
    rm -f "${CLAW_CALLER_CWD:-.}/task.md" "${CLAW_CALLER_CWD:-.}/.clawd-task-graph.json"
fi

# 0. Прибираємо зомбі-процеси, якщо минулого разу термінал впав
echo "🧹 Перевірка та очищення завислих процесів..."
pkill -f "claw-rag-service" 2>/dev/null
pkill -f "mcpbridge" 2>/dev/null
pkill -f "ios-simulator-mcp" 2>/dev/null
sleep 0.5

# 1. Вибір моделі з .claw.json
echo "🤖 Завантаження списку моделей..."
ALIASES_OUTPUT=$("$PYTHON_BIN" -c '
import json, os, sys
try:
    settings_path = os.path.expanduser("~/.claw/settings.json")
    with open(settings_path) as f:
        data = json.load(f)
    for i, (k, v) in enumerate(data.get("aliases", {}).items(), 1):
        print(f"{i}|{k}|{v}")
except Exception as e:
    sys.exit(1)
' )

update_env_var() {
  local key="$1"
  local val="$2"
  # Update SCRIPT_DIR/.env
  if [ -f "$SCRIPT_DIR/.env" ]; then
    "$PYTHON_BIN" -c '
import sys, os
key = sys.argv[1]
val = sys.argv[2]
env_path = sys.argv[3]
with open(env_path, "r") as f:
    lines = f.readlines()
updated = False
for i, line in enumerate(lines):
    if line.strip().startswith(key + "="):
        lines[i] = f"{key}=\"{val}\"\n"
        updated = True
        break
if not updated:
    lines.append(f"{key}=\"{val}\"\n")
with open(env_path, "w") as f:
    f.writelines(lines)
' "$key" "$val" "$SCRIPT_DIR/.env"
  fi
  # Update CLAW_CALLER_CWD/.env
  if [ -n "$CLAW_CALLER_CWD" ] && [ -f "$CLAW_CALLER_CWD/.env" ] && [ "$CLAW_CALLER_CWD" != "$SCRIPT_DIR" ]; then
    "$PYTHON_BIN" -c '
import sys, os
key = sys.argv[1]
val = sys.argv[2]
env_path = sys.argv[3]
with open(env_path, "r") as f:
    lines = f.readlines()
updated = False
for i, line in enumerate(lines):
    if line.strip().startswith(key + "="):
        lines[i] = f"{key}=\"{val}\"\n"
        updated = True
        break
if not updated:
    lines.append(f"{key}=\"{val}\"\n")
with open(env_path, "w") as f:
    f.writelines(lines)
' "$key" "$val" "$CLAW_CALLER_CWD/.env"
  fi
}

DEFAULT_MODEL="${SELECTED_MODEL:-gemini-lite}"
DEFAULT_NARRATION="${CLAW_NARRATION_MODEL:-gemini-lite}"
DEFAULT_GRISHA="${GRISHA_MODEL:-glm2}"

if [ $? -eq 0 ] && [ -n "$ALIASES_OUTPUT" ]; then
    echo "============================================================================"
    echo "                             Доступні AI Моделі                             "
    echo "============================================================================"
    
    declare -a MODEL_KEYS
    
    OLDIFS=$IFS
    IFS=$'\n'
    for line in $ALIASES_OUTPUT; do
        num=$(echo "$line" | cut -d"|" -f1)
        key=$(echo "$line" | cut -d"|" -f2)
        val=$(echo "$line" | cut -d"|" -f3)
        MODEL_KEYS[$num]=$key
        
        # Вирівнювання тексту для красивого виводу
        printf " %2d) \033[1;36m%-15s\033[0m -> %s\n" "$num" "$key" "$val"
    done
    IFS=$OLDIFS
    
    echo "============================================================================"
    echo " Натисніть Enter для вибору '$DEFAULT_MODEL' за замовчуванням"
    read -p " Введіть номер основної моделі: " choice
    if [[ "$choice" =~ ^[0-9]+$ ]] && [ -n "${MODEL_KEYS[$choice]:-}" ]; then
        SELECTED_MODEL="${MODEL_KEYS[$choice]}"
        echo " ✅ Обрано модель: $SELECTED_MODEL"
    else
        SELECTED_MODEL="$DEFAULT_MODEL"
        echo " ✅ Використовується за замовчуванням: $SELECTED_MODEL"
    fi
    
    echo "============================================================================"
    echo " Натисніть Enter для вибору '$DEFAULT_NARRATION' за замовчуванням для озвучки"
    read -p " Введіть номер моделі на озвучку: " choice_narration
    SELECTED_NARRATION_MODEL="$DEFAULT_NARRATION"
    if [[ "$choice_narration" =~ ^[0-9]+$ ]] && [ -n "${MODEL_KEYS[$choice_narration]:-}" ]; then
        SELECTED_NARRATION_MODEL="${MODEL_KEYS[$choice_narration]}"
        echo " ✅ Обрано модель на озвучку: $SELECTED_NARRATION_MODEL"
    else
        echo " ✅ Використовується на озвучку за замовчуванням: $SELECTED_NARRATION_MODEL"
    fi
    export CLAW_NARRATION_MODEL=$SELECTED_NARRATION_MODEL

    echo "============================================================================"
    echo " Натисніть Enter для вибору '$DEFAULT_GRISHA' за замовчуванням для контролера Гріші"
    read -p " Введіть номер моделі для Гріші: " choice_grisha
    SELECTED_GRISHA_MODEL="$DEFAULT_GRISHA"
    if [[ "$choice_grisha" =~ ^[0-9]+$ ]] && [ -n "${MODEL_KEYS[$choice_grisha]:-}" ]; then
        SELECTED_GRISHA_MODEL="${MODEL_KEYS[$choice_grisha]}"
        echo " ✅ Обрано модель для Гріші: $SELECTED_GRISHA_MODEL"
    else
        echo " ✅ Використовується для Гріші за замовчуванням: $SELECTED_GRISHA_MODEL"
    fi
    export GRISHA_MODEL=$SELECTED_GRISHA_MODEL

    # Зберігаємо вибір у .env для синхронізації
    update_env_var "SELECTED_MODEL" "$SELECTED_MODEL"
    update_env_var "CLAW_NARRATION_MODEL" "$SELECTED_NARRATION_MODEL"
    update_env_var "GRISHA_MODEL" "$SELECTED_GRISHA_MODEL"
else
    SELECTED_MODEL="$DEFAULT_MODEL"
    export CLAW_NARRATION_MODEL="$DEFAULT_NARRATION"
    export GRISHA_MODEL="$DEFAULT_GRISHA"
    echo " ⚠️ Не вдалося прочитати .claw.json. Використовуються поточні налаштування з .env."
fi
echo ""

FORWARD_ARGS=()
for arg in "$@"; do
  FORWARD_ARGS+=("$arg")
done

# Повертаємося до директорії запуску перед стартом сервісів та клієнта
cd "${CLAW_CALLER_CWD:-.}"

# 3. Запускаємо RAG-сервіс у фоновому режимі
export RAG_BASE_URL="${RAG_BASE_URL:-http://127.0.0.1:8787}"
echo "🚀 Запуск claw-rag-service у фоні..."
"$HOME/.claw/bin/claw-rag-service" serve >> "$HOME/.claw/logs/claw-rag-startup.err" 2>&1 &
RAG_PID=$!
disown $RAG_PID 2>/dev/null || true
sleep 1
if ! kill -0 $RAG_PID 2>/dev/null; then
  echo "❌ УВАГА: claw-rag-service відразу завершився помилкою! Див. ~/.claw/logs/claw-rag-startup.err"
fi

# 4. Запускаємо фоновий процес диктора озвучки (CLAW Voice Narrator), якщо не відключено в env
NARRATOR_PID=""
if [ "${CLAW_TTS:-true}" != "false" ] && [ "${CLAW_VOICE_NARRATOR:-true}" != "false" ]; then
  echo "🎙️ Запуск CLAW Voice Narrator (озвучка у фоні)..."
  mkdir -p "$HOME/.claw/logs"
  "$PYTHON_BIN" "${SCRIPT_DIR}/scripts/claw_voice_narrator.py" --tail >> "$HOME/.claw/logs/voice-narrator.log" 2>&1 &
  NARRATOR_PID=$!
  disown $NARRATOR_PID 2>/dev/null || true
else
  echo "🔇 Озвучку CLAW Voice Narrator відключено через змінні оточення (CLAW_TTS=false / CLAW_VOICE_NARRATOR=false)."
fi

# 5. Налаштовуємо автоматичне очищення при виході з claw
cleanup() {
  echo "🛑 Зупинка claw-rag-service та Voice Narrator..."
  {
    if [ -n "$RAG_PID" ]; then
      kill "$RAG_PID" 2>/dev/null || true
    fi
    if [ -n "$NARRATOR_PID" ]; then
      kill "$NARRATOR_PID" 2>/dev/null || true
    fi
    pkill -f "claw-rag-service" 2>/dev/null || true
    pkill -f "claw_voice_narrator.py" 2>/dev/null || true
    pkill -f "afplay" 2>/dev/null || true
    rm -f ~/.claw/narration.lock ~/.claw/voice_narrator.pid 2>/dev/null || true
  } 2>/dev/null

  TARGET_CLAW_JSON="${CLAW_CALLER_CWD:-.}/.claw.json"
  if [ -f "$TARGET_CLAW_JSON.bak" ]; then
    echo "🔄 Відновлення оригінального .claw.json..."
    mv "$TARGET_CLAW_JSON.bak" "$TARGET_CLAW_JSON" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# 5. Запускаємо основний клієнт claw у циклі захисту
echo "🚀 Запуск основного клієнта Claw ($SELECTED_MODEL)..."

while true; do
  "$HOME/.claw/bin/claw" \
    --model "$SELECTED_MODEL" \
    --permission-mode danger-full-access \
    --skip-permissions \
    --accept-danger-non-interactive \
    "${FORWARD_ARGS[@]}"
    
  EXIT_CODE=$?
  
  if [ $EXIT_CODE -eq 0 ]; then
    echo "👋 Роботу завершено (Код 0)."
    break
  elif [ $EXIT_CODE -eq 130 ] || [ $EXIT_CODE -eq 143 ] || [ $EXIT_CODE -eq 137 ]; then
    echo "🛑 Процес було примусово зупинено (Код $EXIT_CODE). Перезапуск скасовано."
    break
  fi
  
  echo "⚠️ Agent exited with error or timeout (Code $EXIT_CODE). Auto-restarting in 3 seconds..."
  sleep 3
done
