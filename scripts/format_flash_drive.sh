#!/bin/bash

echo "============================================================"
echo "        Скрипт пошуку та форматування флешки 32GB"
echo "============================================================"
echo ""

# Пошук USB флешки 32GB
echo "🔍 Пошук USB флешки 32GB..."
FOUND_DISKS=$(diskutil list | grep -B 1 "31.5 GB\|32G" | grep "/dev/disk" | awk '{print $1}' | grep -v "disk0")

if [ -z "$FOUND_DISKS" ]; then
    echo "❌ Флешка не знайдена!"
    echo ""
    echo "Спробуйте:"
    echo "1. Перевірте, чи флешка підключена"
    echo "2. Відкрийте Дискову утиліту (Disk Utility)"
    echo "3. У Дисковій утиліті ви повинні побачити флешку"
    echo ""
    echo "Потім поверніться сюди і запустіть скрипт знову"
    exit 1
fi

echo "✅ Знайдено диск(и):"
echo "$FOUND_DISKS"
echo ""

# Запит вибору диска
echo "Виберіть диск для форматування:"
select DISK in $FOUND_DISKS; do
    if [ -n "$DISK" ]; then
        break
    else
        echo "❌ Невірний вибір"
    fi
done

echo ""
echo "============================================================"
echo "        Підготовка до форматування"
echo "============================================================"
echo "Диск: $DISK"
echo "Розмір: $(diskutil info $DISK | grep "Disk Size" | awk '{print $3, $4}')"
echo ""

# Перевірка, чи диск не зайнятий
if diskutil info $DISK | grep -q "Mounted:"; then
    echo "⚠️  Диск зайнятий. Вимонтування..."
    diskutil umount force $(echo $DISK | sed 's/disk/rdisk/') 2>/dev/null || true
    sleep 2
fi

# Питання про форматування
read -p "Ви впевнені, що хочете форматувати $DISK? Це знищить ВСІ дані! (yes/NO): " confirm

if [ "$confirm" != "yes" ]; then
    echo "Скасовано"
    exit 0
fi

echo ""
echo "============================================================"
echo "        Форматування в FAT32"
echo "============================================================"

# Форматування
sudo diskutil eraseDisk FAT32 CLAW_USB GPT $DISK

if [ $? -eq 0 ]; then
    echo ""
    echo "✅ Форматування успішно завершено!"
    echo ""
    echo "Точка монтування:"
    diskutil info $DISK | grep "Mount Point" | awk '{print $3}'
else
    echo ""
    echo "❌ Помилка форматування!"
    exit 1
fi

echo ""
echo "============================================================"
echo "        Підготовка до копіювання файлів"
echo "============================================================"
echo ""
echo "Архіви знайдено в: /Users/dev/Downloads/"
echo "Архіви для копіювання:"
ls -lh /Users/dev/Downloads/*HURMA*LIUBOV*.zip 2>/dev/null | awk '{print "  - " $9 " (" $5 ")"}'

echo ""
echo "Готово! Тепер ви можете:"
echo "1. Відкрити диск у Finder (натисніть Cmd+Shift+G і введіть точку монтування)"
echo "2. Копіювати архіви на флешку"
echo ""
