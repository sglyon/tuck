"""Same PCM, models, provider, threads, decode workload as the Rust executable."""
import json
import os
from pathlib import Path
import sys
from time import perf_counter
import numpy as np
import sherpa_onnx

models, pcm = Path(sys.argv[1]), Path(sys.argv[2])
threads = int(sys.argv[3]) if len(sys.argv) > 3 else 2
provider = sys.argv[4] if len(sys.argv) > 4 else "cpu"
samples = np.fromfile(pcm, dtype="<f4")
start = perf_counter()
recognizer = sherpa_onnx.OfflineRecognizer.from_transducer(
    encoder=str(models / "encoder.int8.onnx"),
    decoder=str(models / "decoder.int8.onnx"),
    joiner=str(models / "joiner.int8.onnx"),
    tokens=str(models / "tokens.txt"),
    num_threads=threads, sample_rate=16000,
    decoding_method="greedy_search", provider=provider, model_type="nemo_transducer",
)
load = perf_counter() - start

def decode(data):
    stream = recognizer.create_stream()
    stream.accept_waveform(16000, data)
    recognizer.decode_stream(stream)
    return stream.result.text

start = perf_counter()
text = decode(samples)
first = perf_counter() - start
assert "ask not" in text.lower(), text
warm = []
for _ in range(5):
    start = perf_counter()
    assert decode(samples) == text
    warm.append(perf_counter() - start)
short_only = "TUCK_BENCH_SHORT" in os.environ
chunked, joined_time, joined_text = None, None, ""
if not short_only:
    start = perf_counter()
    for _ in range(8):
        assert decode(samples) == text
    chunked = perf_counter() - start
    joined = np.tile(samples, 8)
    start = perf_counter()
    joined_text = decode(joined)
    joined_time = perf_counter() - start
print(json.dumps(dict(language="python", short_only=short_only, sherpa_onnx=sherpa_onnx.__version__, provider_requested=provider, threads=threads, sample_count=len(samples), audio_seconds=len(samples)/16000, load_seconds=load, first_decode_seconds=first, cold_load_decode_seconds=load+first, warm_decode_seconds=warm, eight_chunks_seconds=chunked, joined_seconds=joined_time, text=text, joined_text=joined_text)))
