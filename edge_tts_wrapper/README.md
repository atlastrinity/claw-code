# Reusable Edge TTS Audio Generator (Ukrainian Male Voice)

Цей пакет містить готову інструментальну обв'язку для синтезу високоякісного українського чоловічого голосу за допомогою **`edge-tts`** (Microsoft Edge Neural TTS) з додатковою постобробкою для отримання глибокого "дикторського" тембру тривожних сповіщень.

---

## 🛠️ Передумови

Перед початком використання переконайтеся, що у вас встановлено:
1. **FFmpeg** з підтримкою фільтра `rubberband`. На macOS це встановлюється через:
   ```bash
   brew install ffmpeg rubberband
   ```
2. **Python 3.8+**

---

## 📦 Встановлення

1. Встановіть необхідні Python-бібліотеки:
   ```bash
   pip install -r requirements.txt
   ```

---

## 🚀 Використання

Модуль надає клас `EdgeTTSHelper`, який автоматизує:
1. Синтез тексту через Edge TTS та конвертацію в WAV за допомогою `ffmpeg`.
2. Автоматичне обрізання тиші (trim silence) на початку та в кінці фраз.
3. Склеювання фраз із заданими мікро-паузами.
4. Пониження тональності (pitch shift) через `rubberband`.

### Швидкий запуск демо:
```bash
python example.py
```
В результаті буде створено файл `output_demo.wav` із глибоким природним голосом Остапа (`uk-UA-OstapNeural` з пітчем `0.92`).

### Приклад імпорту в свій проект:
```python
from edge_tts_helper import EdgeTTSHelper

# Ініціалізація хелпера
helper = EdgeTTSHelper(default_voice="uk-UA-OstapNeural", sample_rate=44100)

# Синтез WAV
helper.synthesize_to_wav("Увага! Загрозу знято.", "speech.wav")

# Зниження тональності до басового тембру
helper.pitch_shift_file("speech.wav", "speech_deep.wav", pitch_factor=0.92)
```
