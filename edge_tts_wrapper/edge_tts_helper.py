import os
import subprocess
import tempfile
import numpy as np
import soundfile as sf

class EdgeTTSHelper:
    def __init__(self, default_voice="uk-UA-OstapNeural", sample_rate=44100):
        self.default_voice = default_voice
        self.sample_rate = sample_rate

    def _split_text_chunks(self, text, max_chunk_len=1200):
        if len(text) <= max_chunk_len:
            return [text]
        import re
        sentences = re.split(r'(?<=[.!?\n])\s+', text)
        chunks = []
        current = []
        current_len = 0
        for s in sentences:
            if not s.strip():
                continue
            if current_len + len(s) > max_chunk_len and current:
                chunks.append(" ".join(current))
                current = [s]
                current_len = len(s)
            else:
                current.append(s)
                current_len += len(s) + 1
        if current:
            chunks.append(" ".join(current))
        return chunks

    def _synthesize_chunk(self, text, output_wav_path, voice, rate, pitch):
        with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as temp_mp3:
            temp_mp3_path = temp_mp3.name

        try:
            # 1. Run edge-tts to generate mp3
            subprocess.run([
                "edge-tts",
                "--voice", voice,
                "--text", text,
                f"--rate={rate}",
                f"--pitch={pitch}",
                "--write-media", temp_mp3_path
            ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120.0)
            
            # 2. Convert mp3 to WAV using ffmpeg, appending 400ms tail silence to prevent audio clipping
            subprocess.run([
                "ffmpeg", "-y",
                "-i", temp_mp3_path,
                "-af", "apad=pad_dur=0.4",
                "-ac", "1",
                "-ar", str(self.sample_rate),
                output_wav_path
            ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120.0)
            
        finally:
            if os.path.exists(temp_mp3_path):
                os.remove(temp_mp3_path)

    def synthesize_to_wav(self, text, output_wav_path, voice=None, rate="+0%", pitch="+0Hz"):
        """
        Synthesizes text using edge-tts and converts it directly to a mono WAV file at the target sample rate.
        Supports rate (e.g. '+10%') and pitch (e.g. '-5Hz') parameters.
        Automatically chunks long text to prevent timeouts and audio truncation.
        """
        voice = voice or self.default_voice
        chunks = self._split_text_chunks(text, max_chunk_len=1200)

        if len(chunks) == 1:
            self._synthesize_chunk(chunks[0], output_wav_path, voice, rate, pitch)
        else:
            temp_wavs = []
            try:
                for idx, chunk in enumerate(chunks):
                    with tempfile.NamedTemporaryFile(suffix=f"_{idx}.wav", delete=False) as tw:
                        temp_wav_path = tw.name
                    self._synthesize_chunk(chunk, temp_wav_path, voice, rate, pitch)
                    temp_wavs.append(temp_wav_path)
                
                self.concatenate_files(temp_wavs, output_wav_path, gap_seconds=0.2, normalize_peak=0.95)
            finally:
                for tw_path in temp_wavs:
                    if os.path.exists(tw_path):
                        os.remove(tw_path)

    def trim_silence(self, audio_data, threshold=0.001, pad_ms=350):
        """
        Removes silence from the start and end of a numpy audio array.
        Uses low threshold (0.001) and generous padding (350ms) to ensure quiet sentence endings/consonants are not cut off.
        """
        pad_samples = int(self.sample_rate * (pad_ms / 1000.0))
        above_threshold = np.where(np.abs(audio_data) > threshold)[0]
        
        if len(above_threshold) > 0:
            start_idx = max(0, above_threshold[0] - pad_samples)
            end_idx = min(len(audio_data), above_threshold[-1] + pad_samples)
            return audio_data[start_idx:end_idx]
        return audio_data

    def trim_silence_file(self, wav_path, output_wav_path, threshold=0.001, pad_ms=350):
        """
        Loads a WAV file, trims silence, and saves it.
        """
        data, sr = sf.read(wav_path)
        trimmed = self.trim_silence(data, threshold=threshold, pad_ms=pad_ms)
        sf.write(output_wav_path, trimmed, self.sample_rate)


    def pitch_shift_file(self, wav_path, output_wav_path, pitch_factor=0.92):
        """
        Applies a pitch shift effect using ffmpeg's rubberband filter.
        """
        subprocess.run([
            "ffmpeg", "-y",
            "-i", wav_path,
            "-af", f"rubberband=pitch={pitch_factor}",
            "-ac", "1",
            "-ar", str(self.sample_rate),
            output_wav_path
        ], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def concatenate_files(self, wav_paths, output_wav_path, gap_seconds=0.1, normalize_peak=0.95):
        """
        Concatenates multiple WAV files with a silent gap between them, optionally normalizing the output.
        """
        all_data = []
        gap_samples = int(self.sample_rate * gap_seconds)
        silence_gap = np.zeros(gap_samples)
        
        for idx, path in enumerate(wav_paths):
            data, sr = sf.read(path)
            all_data.append(data)
            if idx < len(wav_paths) - 1:
                all_data.append(silence_gap)
                
        combined = np.concatenate(all_data)
        
        if normalize_peak:
            peak = np.max(np.abs(combined))
            if peak > 0:
                combined = combined / peak * normalize_peak
                
        sf.write(output_wav_path, combined, self.sample_rate)
