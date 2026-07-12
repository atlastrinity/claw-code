#!/bin/bash

# Збережемо оригінальну директорію, звідки запустили скрипт
export CLAW_CALLER_CWD="$PWD"

# Змінюємо робочу директорію на ту, де знаходиться сам скрипт
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

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

# Синхронізуємо конфігурації (settings та CLAW.md) з глобальною папкою перед запуском
GLOBAL_DIR="$HOME/.claw"
mkdir -p "$GLOBAL_DIR"

LOCAL_SETTINGS="$SCRIPT_DIR/.claw.json"
GLOBAL_SETTINGS="$GLOBAL_DIR/settings.json"

if [ -f "$LOCAL_SETTINGS" ] && [ ! -f "$GLOBAL_SETTINGS" ]; then
    cp "$LOCAL_SETTINGS" "$GLOBAL_SETTINGS"
elif [ ! -f "$LOCAL_SETTINGS" ] && [ -f "$GLOBAL_SETTINGS" ]; then
    cp "$GLOBAL_SETTINGS" "$LOCAL_SETTINGS"
elif [ -f "$LOCAL_SETTINGS" ] && [ -f "$GLOBAL_SETTINGS" ]; then
    if [ "$LOCAL_SETTINGS" -nt "$GLOBAL_SETTINGS" ]; then
        cp "$LOCAL_SETTINGS" "$GLOBAL_SETTINGS"
    elif [ "$GLOBAL_SETTINGS" -nt "$LOCAL_SETTINGS" ]; then
        cp "$GLOBAL_SETTINGS" "$LOCAL_SETTINGS"
    fi
fi

LOCAL_CLAW="$SCRIPT_DIR/CLAW.md"
GLOBAL_CLAW="$GLOBAL_DIR/CLAW.md"

if [ -f "$LOCAL_CLAW" ] && [ ! -f "$GLOBAL_CLAW" ]; then
    cp "$LOCAL_CLAW" "$GLOBAL_CLAW"
elif [ ! -f "$LOCAL_CLAW" ] && [ -f "$GLOBAL_CLAW" ]; then
    cp "$GLOBAL_CLAW" "$LOCAL_CLAW"
elif [ -f "$LOCAL_CLAW" ] && [ -f "$GLOBAL_CLAW" ]; then
    if [ "$LOCAL_CLAW" -nt "$GLOBAL_CLAW" ]; then
        cp "$LOCAL_CLAW" "$GLOBAL_CLAW"
    elif [ "$GLOBAL_CLAW" -nt "$LOCAL_CLAW" ]; then
        cp "$GLOBAL_CLAW" "$LOCAL_CLAW"
    fi
fi

# Синхронізуємо скіли з глобальною папкою перед запуском
if [ -d "$SCRIPT_DIR/.claw/skills" ]; then
    mkdir -p "$GLOBAL_DIR/skills"
    rsync -a --exclude=".build" --exclude=".git" "$SCRIPT_DIR/.claw/skills/" "$GLOBAL_DIR/skills/"
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
    read -p " Введіть номер основної моделі: " choice
    if [ -n "$choice" ] && [ -n "${MODEL_KEYS[$choice]}" ]; then
        SELECTED_MODEL="${MODEL_KEYS[$choice]}"
        echo " ✅ Обрано модель: $SELECTED_MODEL"
    else
        echo " ✅ Використовується за замовчуванням: $SELECTED_MODEL"
    fi
    
    echo "============================================================================"
    echo " Натисніть Enter для вибору 'gemini-lite' за замовчуванням для озвучки"
    read -p " Введіть номер моделі на озвучку: " choice_narration
    SELECTED_NARRATION_MODEL="gemini-lite"
    if [ -n "$choice_narration" ] && [ -n "${MODEL_KEYS[$choice_narration]}" ]; then
        SELECTED_NARRATION_MODEL="${MODEL_KEYS[$choice_narration]}"
        echo " ✅ Обрано модель на озвучку: $SELECTED_NARRATION_MODEL"
    else
        echo " ✅ Використовується на озвучку за замовчуванням: $SELECTED_NARRATION_MODEL"
    fi
    export CLAW_NARRATION_MODEL=$SELECTED_NARRATION_MODEL
else
    echo " ⚠️ Не вдалося прочитати .claw.json. Використовується gemini-lite."
fi
echo ""

# 1.5 Перевірка наявності iOS-компонентів для автозапуску Xcode
FORWARD_ARGS=()
for arg in "$@"; do
  FORWARD_ARGS+=("$arg")
done

# Якщо є згадки Xcode/iOS в аргументах або відповідні файли в проекті, запускаємо Xcode
IS_APPLE_DEV="false"
if find "${CLAW_CALLER_CWD:-.}" -maxdepth 1 \( -name "*.xcodeproj" -o -name "*.xcworkspace" -o -name "Podfile" \) -print -quit | grep -q .; then
  IS_APPLE_DEV="true"
fi
for arg in "${FORWARD_ARGS[@]}"; do
  if echo "$arg" | grep -iqE "ios|xcode|swift|swiftui|cocoapods|podfile|simulator|watchos|tvos|macos|iphonesimulator"; then
    IS_APPLE_DEV="true"
    break
  fi
done

if [ "$IS_APPLE_DEV" = "true" ]; then
  if ! pgrep -q -x "Xcode"; then
    echo "🍏 Запуск Xcode (необхідно для xcode-bridge MCP)..."
    open -a Xcode
    sleep 3
  fi
else
  # Тимчасово відключаємо iOS MCP сервери у .claw.json, якщо вони не використовуються
  TARGET_CLAW_JSON="${CLAW_CALLER_CWD:-.}/.claw.json"
  if [ -f "$TARGET_CLAW_JSON" ]; then
    echo "🧹 Тимчасове відключення iOS MCP серверів у $TARGET_CLAW_JSON..."
    cp "$TARGET_CLAW_JSON" "$TARGET_CLAW_JSON.bak"
    python3 -c '
import json, sys
try:
    with open(sys.argv[1], "r") as f:
        data = json.load(f)
    if "mcpServers" in data:
        data["mcpServers"] = {}
    with open(sys.argv[1], "w") as f:
        json.dump(data, f, indent=2)
except Exception as e:
    print("Warning: failed to strip mcpServers:", e)
' "$TARGET_CLAW_JSON"
  fi
fi

# Повертаємося до директорії запуску перед стартом сервісів та клієнта
cd "${CLAW_CALLER_CWD:-.}"

# 3. Запускаємо RAG-сервіс у фоновому режимі
echo "🚀 Запуск claw-rag-service у фоні..."
"$HOME/.claw/bin/claw-rag-service" serve >> "$HOME/.claw/logs/claw-rag-startup.err" 2>&1 &
RAG_PID=$!
sleep 1
if ! kill -0 $RAG_PID 2>/dev/null; then
  echo "❌ УВАГА: claw-rag-service відразу завершився помилкою! Див. ~/.claw/logs/claw-rag-startup.err"
fi

# 4. Налаштовуємо автоматичне очищення при виході з claw
cleanup() {
  echo "🛑 Зупинка claw-rag-service..."
  kill $RAG_PID 2>/dev/null
  
  TARGET_CLAW_JSON="${CLAW_CALLER_CWD:-.}/.claw.json"
  if [ -f "$TARGET_CLAW_JSON.bak" ]; then
    echo "🔄 Відновлення оригінального .claw.json..."
    mv "$TARGET_CLAW_JSON.bak" "$TARGET_CLAW_JSON"
  fi
}
trap cleanup EXIT

# 5. Запускаємо основний клієнт claw у циклі захисту
echo "🚀 Запуск основного клієнта Claw ($SELECTED_MODEL)..."

# Перевіряємо, чи є вже існуючі сесії, щоб продовжити останню
SESSIONS_DIR="${CLAW_CALLER_CWD:-.}/.claw/sessions"
if [ -d "$SESSIONS_DIR" ] && [ "$(find "$SESSIONS_DIR" -name "*.jsonl" 2>/dev/null | wc -l)" -gt 0 ]; then
  echo "🔄 Знайдено попередню сесію. Продовжуємо роботу з останнього місця..."
  RESUME_ARGS="--resume latest"
else
  echo "🌱 Попередніх сесій не знайдено. Запускаємо нову сесію..."
  RESUME_ARGS=""
fi

while true; do
  "$HOME/.claw/bin/claw" \
    --model "$SELECTED_MODEL" \
    --skip-permissions \
    --accept-danger-non-interactive \
    $RESUME_ARGS "${FORWARD_ARGS[@]}"
    
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
