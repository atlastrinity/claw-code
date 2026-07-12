import os
import numpy as np
import soundfile as sf
from edge_tts_helper import EdgeTTSHelper

def main():
    print("🎙️ Initializing EdgeTTSHelper...")
    helper = EdgeTTSHelper(default_voice="uk-UA-OstapNeural", sample_rate=44100)
    
    # Text to speak
    phrase1_text = "Увага!"
    phrase2_text = "Виявлено нову загрозу!"
    
    os.makedirs("/tmp/tts_test", exist_ok=True)
    raw_p1_path = "/tmp/tts_test/p1_raw.wav"
    raw_p2_path = "/tmp/tts_test/p2_raw.wav"
    
    trimmed_p1_path = "/tmp/tts_test/p1_trimmed.wav"
    trimmed_p2_path = "/tmp/tts_test/p2_trimmed.wav"
    
    combined_raw_path = "/tmp/tts_test/combined_raw.wav"
    final_output_path = "./output_demo.wav"
    
    # 1. Synthesize phrases to WAV
    helper.synthesize_to_wav(phrase1_text, raw_p1_path)
    helper.synthesize_to_wav(phrase2_text, raw_p2_path)
    
    # 2. Trim silence from generated audio
    print("⏳ Trimming silence from phrases...")
    helper.trim_silence_file(raw_p1_path, trimmed_p1_path)
    helper.trim_silence_file(raw_p2_path, trimmed_p2_path)
    
    # 3. Concatenate trimmed phrases with a 0.1s silence gap
    print("⏳ Concatenating phrases with 0.1s gap...")
    helper.concatenate_files(
        [trimmed_p1_path, trimmed_p2_path],
        combined_raw_path,
        gap_seconds=0.1,
        normalize_peak=0.95
    )
    
    # 4. Pitch shift the combined speech to 0.92x to make it deeper/resonant
    print("⏳ Pitch shifting to 0.92x (Ostap male voice tuning)...")
    helper.pitch_shift_file(combined_raw_path, final_output_path, pitch_factor=0.92)
    
    print(f"🎉 Success! Generated professional voice file saved to: {final_output_path}")

if __name__ == '__main__':
    main()
