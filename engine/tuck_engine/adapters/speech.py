"""Local CPU inference and provisional acoustic clustering; no persistence or domain rules."""

from __future__ import annotations

from collections import deque
from pathlib import Path
import uuid
from typing import Callable

import numpy as np

from .model_manager import ModelManager, ModelError, sha256_file, ASSETS

SAMPLE_RATE = 16000


class SpeakerClusters:
    """Meeting-scoped centroid matching. IDs are provisional, never personal identities."""

    def __init__(self, threshold: float = 0.65, maximum: int = 12):
        self.threshold = threshold
        self.maximum = maximum
        self.centroids: list[np.ndarray] = []
        self.counts: list[int] = []

    def identify(self, embedding: np.ndarray) -> str:
        vector = np.asarray(embedding, dtype=np.float32).reshape(-1)
        norm = float(np.linalg.norm(vector))
        if not np.isfinite(norm) or norm <= 1e-8:
            return "speaker-1"
        vector = vector / norm
        if self.centroids:
            scores = np.array(
                [float(np.dot(center, vector)) for center in self.centroids]
            )
            best = int(np.argmax(scores))
            if scores[best] >= self.threshold or len(self.centroids) >= self.maximum:
                # Bound adaptation so one bad/noisy turn cannot move a mature speaker far.
                count = min(self.counts[best], 20)
                center = self.centroids[best] * count + vector
                self.centroids[best] = center / np.linalg.norm(center)
                self.counts[best] += 1
                return f"speaker-{best + 1}"
        self.centroids.append(vector)
        self.counts.append(1)
        return f"speaker-{len(self.centroids)}"


class SileroSegmenter:
    def __init__(self, model: Path, on_speech: Callable[[np.ndarray, float], None]):
        import sherpa_onnx

        config = sherpa_onnx.VadModelConfig()
        config.silero_vad.model = str(model)
        config.silero_vad.min_silence_duration = 0.45
        config.silero_vad.min_speech_duration = 0.25
        config.silero_vad.max_speech_duration = 5.0
        config.sample_rate = SAMPLE_RATE
        config.num_threads = 1
        self.vad = sherpa_onnx.VoiceActivityDetector(config, buffer_size_in_seconds=20)
        self.window_size = config.silero_vad.window_size
        self.pending = np.empty(0, dtype=np.float32)
        self.on_speech = on_speech
        self.offset: float | None = None
        self.total_samples = 0

    def feed(self, samples: np.ndarray, start: float):
        if self.offset is None:
            self.offset = start
        self.total_samples += len(samples)
        self.pending = np.concatenate(
            (self.pending, np.asarray(samples, dtype=np.float32))
        )
        consumed = 0
        while len(self.pending) - consumed >= self.window_size:
            self.vad.accept_waveform(
                self.pending[consumed : consumed + self.window_size]
            )
            consumed += self.window_size
            self._drain()
        self.pending = self.pending[consumed:]

    def _drain(self):
        while not self.vad.empty():
            speech = self.vad.front
            samples = np.asarray(speech.samples, dtype=np.float32)[
                : max(0, self.total_samples - speech.start)
            ]
            if len(samples):
                self.on_speech(samples, (self.offset or 0) + speech.start / SAMPLE_RATE)
            self.vad.pop()

    def flush(self):
        if self.pending.size:
            self.vad.accept_waveform(
                np.pad(self.pending, (0, self.window_size - len(self.pending)))
            )
            self.pending = np.empty(0, dtype=np.float32)
        self.vad.flush()
        self._drain()
        self.vad.reset()
        self.offset = None
        self.total_samples = 0


class SpeechRuntime:
    def __init__(self, models: ModelManager):
        self.models = models
        self.recognizer = None
        self.extractor = None
        self.threads = None
        self.clusters = SpeakerClusters()

    def load(self, threads: int = 2):
        threads = max(1, min(4, int(threads)))
        import sherpa_onnx

        if self.recognizer is None or self.threads != threads:
            self.models.verify()
            directory = self.models.parakeet_dir
            self.recognizer = sherpa_onnx.OfflineRecognizer.from_transducer(
                encoder=str(directory / "encoder.int8.onnx"),
                decoder=str(directory / "decoder.int8.onnx"),
                joiner=str(directory / "joiner.int8.onnx"),
                tokens=str(directory / "tokens.txt"),
                num_threads=threads,
                sample_rate=SAMPLE_RATE,
                decoding_method="greedy_search",
                provider="cpu",
                model_type="nemo_transducer",
            )
            self.threads = threads
        if self.extractor is None and self.models.installed("speakers"):
            if sha256_file(self.models.speaker_path) != ASSETS["speakers"][1]:
                raise ModelError(
                    "The speaker model failed its integrity check. Reinstall it in Settings."
                )
            config = sherpa_onnx.SpeakerEmbeddingExtractorConfig(
                model=str(self.models.speaker_path), num_threads=1, provider="cpu"
            )
            if not config.validate():
                raise ModelError(
                    "The speaker model is not compatible with this runtime."
                )
            self.extractor = sherpa_onnx.SpeakerEmbeddingExtractor(config)
        self.clusters = SpeakerClusters()

    def transcribe(
        self, samples: np.ndarray, start: float, source: str, meeting_id: str
    ) -> dict | None:
        if self.recognizer is None:
            raise ModelError("The speech model is not loaded.")
        stream = self.recognizer.create_stream()
        stream.accept_waveform(SAMPLE_RATE, samples)
        self.recognizer.decode_stream(stream)
        text = stream.result.text.strip()
        if not text:
            return None
        speaker = "you" if source == "microphone" else "speaker-1"
        if (
            source != "microphone"
            and self.extractor is not None
            and len(samples) >= SAMPLE_RATE
        ):
            speaker_stream = self.extractor.create_stream()
            speaker_stream.accept_waveform(sample_rate=SAMPLE_RATE, waveform=samples)
            speaker_stream.input_finished()
            if self.extractor.is_ready(speaker_stream):
                speaker = self.clusters.identify(
                    np.asarray(self.extractor.compute(speaker_stream))
                )
        return {
            "id": str(uuid.uuid4()),
            "meetingId": meeting_id,
            "start": round(max(0, start), 3),
            "end": round(max(0, start) + len(samples) / SAMPLE_RATE, 3),
            "text": text,
            "speakerId": speaker,
            "source": source,
        }

    def unload(self):
        self.recognizer = None
        self.extractor = None
        self.threads = None
        self.clusters = SpeakerClusters()
