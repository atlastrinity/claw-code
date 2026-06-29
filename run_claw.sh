#!/bin/bash

# Змінюємо робочу директорію на ту, де знаходиться сам скрипт
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Завантажуємо змінні оточення (API ключі тощо) з глобального .env
if [ -f "$SCRIPT_DIR/.env" ]; then
    set -a
    source "$SCRIPT_DIR/.env"
    set +a
fi
# 0. Прибираємо зомбі-процеси, якщо минулого разу термінал впав
echo "🧹 Перевірка та очищення завислих процесів..."
pkill -f "claw-rag-service" 2>/dev/null
pkill -f "mcpbridge" 2>/dev/null
pkill -f "ios-simulator-mcp" 2>/dev/null
sleep 0.5

# 1. Вибір моделі з .claw.json
echo "🤖 Завантаження списку моделей..."
ALIASES_OUTPUT=$(python3 -c '
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

SELECTED_MODEL="gemini-lite"

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
    echo " Натисніть Enter для вибору 'gemini-lite' за замовчуванням"
    read -p " Введіть номер моделі: " choice
    
    if [ -n "$choice" ] && [ -n "${MODEL_KEYS[$choice]}" ]; then
        SELECTED_MODEL="${MODEL_KEYS[$choice]}"
        echo " ✅ Обрано модель: $SELECTED_MODEL"
    else
        echo " ✅ Використовується за замовчуванням: $SELECTED_MODEL"
    fi
else
    echo " ⚠️ Не вдалося прочитати .claw.json. Використовується gemini-lite."
fi
echo ""

# 2. Перевірка та запуск Xcode (потрібен для mcpbridge)
if ! pgrep -q -x "Xcode"; then
  echo "🍏 Запуск Xcode (необхідно для xcode-bridge MCP)..."
  open -a Xcode
  # Чекаємо кілька секунд, щоб Xcode встиг запуститися
  sleep 3
fi

# 3. Запускаємо RAG-сервіс у фоновому режимі
echo "🚀 Запуск claw-rag-service у фоні..."
"$HOME/.claw/bin/claw-rag-service" serve >> "$HOME/.claw/logs/claw-rag-startup.err" 2>&1 &
RAG_PID=$!
sleep 1
if ! kill -0 $RAG_PID 2>/dev/null; then
  echo "❌ УВАГА: claw-rag-service відразу завершився помилкою! Див. ~/.claw/logs/claw-rag-startup.err"
fi

# 3. Налаштовуємо автоматичне вимкнення RAG-сервісу при виході з claw
trap "echo '🛑 Зупинка claw-rag-service...'; kill $RAG_PID 2>/dev/null" EXIT

# 5. Запускаємо основний клієнт claw у циклі захисту
echo "🚀 Запуск основного клієнта Claw ($SELECTED_MODEL) з авто-перезапуском..."

RESUME_ARGS=""

while true; do
  "$HOME/.claw/bin/claw" \
    --model "$SELECTED_MODEL" \
    --skip-permissions \
    --accept-danger-non-interactive \
    --attach-skill "$SCRIPT_DIR/.claw/skills/workflows/apple-development-workflow/SKILL.md" \
    --attach-skill "$SCRIPT_DIR/.claw/skills/xcode_project_setup/SKILL.md" \
    $RESUME_ARGS "$@"
    
  EXIT_CODE=$?
  
  if [ $EXIT_CODE -eq 0 ]; then
    echo "👋 Роботу завершено (Код 0)."
    break
  elif [ $EXIT_CODE -eq 130 ] || [ $EXIT_CODE -eq 143 ] || [ $EXIT_CODE -eq 137 ]; then
    echo "🛑 Процес було примусово зупинено (Код $EXIT_CODE). Перезапуск скасовано."
    break
  fi
  
  echo "⚠️ Agent exited with error or timeout (Code $EXIT_CODE). Auto-restarting in 3 seconds..."
  RESUME_ARGS="--resume latest"
  sleep 3
done
