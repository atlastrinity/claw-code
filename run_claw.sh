#!/bin/bash

# 0. Прибираємо зомбі-процеси RAG-сервісу, якщо минулого разу термінал впав
echo "🧹 Перевірка та очищення завислих процесів..."
pkill -f "claw-rag-service" 2>/dev/null
sleep 0.5

# 1. Запускаємо RAG-сервіс у фоновому режимі
echo "🚀 Запуск claw-rag-service у фоні..."
cargo run --manifest-path rust/Cargo.toml -p claw-rag-service -- serve > /dev/null 2>&1 &
RAG_PID=$!

# 2. Налаштовуємо автоматичне вимкнення RAG-сервісу при виході з claw
# (Цей trap спрацює навіть якщо claw впаде з помилкою, але не якщо закрити сам термінал)
trap "echo '🛑 Зупинка claw-rag-service...'; kill $RAG_PID 2>/dev/null" EXIT

# 3. Запускаємо основний клієнт claw
echo "🚀 Запуск основного клієнта Claw..."
cargo run --manifest-path rust/Cargo.toml --bin claw -- \
  --model gemini-lite \
  --skip-permissions \
  --accept-danger-non-interactive \
  --attach-skill .claw/skills/project_specific/ios_remote_client.md "$@"
