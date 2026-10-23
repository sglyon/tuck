"""Prepare one identical input for both harnesses; uses existing public fixture only."""
from pathlib import Path
import hashlib
import sys
import wave

import numpy as np
import soxr

fixture, destination = map(Path, sys.argv[1:3])
assert hashlib.sha256(fixture.read_bytes()).hexdigest() == (
    "148b936b43ce7c546a866e64da059f0458aee2d65e617f16e9d94f06e8d99ed6"
), "Expected the pinned public Kennedy fixture"
with wave.open(str(fixture)) as wav:
    assert wav.getnchannels() == 1 and wav.getsampwidth() == 2
    samples = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2")
    samples = samples.astype(np.float32) / 32768
    samples = soxr.resample(samples, wav.getframerate(), 16000).astype("<f4")
samples.tofile(destination)
print(len(samples), hashlib.sha256(samples.tobytes()).hexdigest())
