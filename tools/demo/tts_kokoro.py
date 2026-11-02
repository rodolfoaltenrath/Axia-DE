#!/usr/bin/env python3
"""English voice with Kokoro-82M (Apache 2.0) via kokoro-onnx, for demo.py.
Usage: tts_kokoro.py <model folder> <voice> <output.wav> <text>
The folder holds kokoro-v1.0.onnx and voices-v1.0.bin (thewh1teagle/kokoro-onnx releases).
Voices: af_heart, am_michael (US), bf_emma (UK)… Needs espeak-ng
(ESPEAK_DATA_PATH pointing at its data if the package does not find it)."""
import sys
import soundfile as sf
from kokoro_onnx import Kokoro

model, voice, out, text = sys.argv[1:5]
k = Kokoro(f'{model}/kokoro-v1.0.onnx', f'{model}/voices-v1.0.bin')
samples, sr = k.create(text, voice=voice, speed=1.0, lang='en-gb' if voice.startswith('b') else 'en-us')
sf.write(out, samples, sr)
