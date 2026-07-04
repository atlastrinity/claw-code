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

# Очищаємо файли планування та завдань для справді нової сесії
echo "🧹 Очищення файлів завдань попередньої сесії..."
rm -f "./task.md" "./.clawd-task-graph.json"

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

# 1.5 Перевірка необхідності iOS (Xcode та спеціальних скілів)
ENABLE_IOS=""
FORWARD_ARGS=()
for arg in "$@"; do
  case "$arg" in
    --ios)
      ENABLE_IOS="true"
      ;;
    --no-ios)
      ENABLE_IOS="false"
      ;;
    *)
      FORWARD_ARGS+=("$arg")
      ;;
  esac
done

if [ -z "$ENABLE_IOS" ]; then
  # Перевірка наявності iOS/Apple файлів у безпосередній директорії (depth 1)
  if find "$SCRIPT_DIR" -maxdepth 1 \( -name "*.xcodeproj" -o -name "*.xcworkspace" -o -name "Podfile" \) -print -quit | grep -q .; then
    IMMEDIATE_HAS_IOS="true"
  else
    IMMEDIATE_HAS_IOS="false"
  fi

  # Перевірка ключових слів у аргументах запуску
  PROMPT_HAS_IOS="false"
  for arg in "${FORWARD_ARGS[@]}"; do
    if echo "$arg" | grep -iqE "ios|xcode|swift|swiftui|cocoapods|podfile|simulator|watchos|tvos|macos|iphonesimulator"; then
      PROMPT_HAS_IOS="true"
      break
    fi
  done

  if [ "$IMMEDIATE_HAS_IOS" = "true" ] || [ "$PROMPT_HAS_IOS" = "true" ]; then
    echo "🍏 [Автодетекція] Виявлено iOS-проект або тему Apple-розробки. Режим iOS увімкнено."
    ENABLE_IOS="true"
  else
    ENABLE_IOS="false"
  fi
fi

SKILL_ARGS=()

if [ "$ENABLE_IOS" = "true" ]; then
  echo " ✅ Режим розробки iOS УВІМКНЕНО."
  
  # 2. Перевірка та запуск Xcode (потрібен для mcpbridge)
  if ! pgrep -q -x "Xcode"; then
    echo "🍏 Запуск Xcode (необхідно для xcode-bridge MCP)..."
    open -a Xcode
    # Чекаємо кілька секунд, щоб Xcode встиг запуститися
    sleep 3
  fi

  # Додаємо iOS скіли
  SKILL_ARGS=(
    --attach-skill "$SCRIPT_DIR/.claw/skills/workflows/apple-development-workflow/SKILL.md"
    --attach-skill "$SCRIPT_DIR/.claw/skills/xcode_project_setup/SKILL.md"
  )
else
  echo " ℹ️ Режим розробки iOS ВИМКНЕНО."
  
  # Тимчасово відключаємо iOS MCP сервери у .claw.json, якщо файл існує
  if [ -f "$SCRIPT_DIR/.claw.json" ]; then
    echo "🧹 Тимчасове відключення iOS MCP серверів у .claw.json..."
    cp "$SCRIPT_DIR/.claw.json" "$SCRIPT_DIR/.claw.json.bak"
    python3 -c '
import json
try:
    with open(".claw.json", "r") as f:
        data = json.load(f)
    if "mcpServers" in data:
        data["mcpServers"] = {}
    with open(".claw.json", "w") as f:
        json.dump(data, f, indent=2)
except Exception as e:
    print("Warning: failed to strip mcpServers:", e)
'
  fi
fi

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
  
  if [ -f "$SCRIPT_DIR/.claw.json.bak" ]; then
    echo "🔄 Відновлення оригінального .claw.json..."
    mv "$SCRIPT_DIR/.claw.json.bak" "$SCRIPT_DIR/.claw.json"
  fi
}
trap cleanup EXIT

# 5. Запускаємо основний клієнт claw (нова сесія)
echo "🚀 Запуск нової сесії основного клієнта Claw ($SELECTED_MODEL) з авто-відновленням..."

RESUME_ARGS=""

while true; do
  "$HOME/.claw/bin/claw" \
    --model "$SELECTED_MODEL" \
    --skip-permissions \
    --accept-danger-non-interactive \
    "${SKILL_ARGS[@]}" \
    $RESUME_ARGS "${FORWARD_ARGS[@]}"
    
  EXIT_CODE=$?
  
  # Код 0 (нормальний вихід) або 130 (Ctrl+C користувачем) зупиняє цикл
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
