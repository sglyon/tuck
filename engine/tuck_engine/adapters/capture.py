"""Desktop capture adapter. Speech stays local; the application owns all meeting persistence.

Audio is written before it enters the bounded inference queue. A slow/failed inference worker
stops acquisition and preserves WAVs rather than quietly losing part of a conversation.
"""

from __future__ import annotations

import os
from pathlib import Path
import queue
import re
import shutil
import subprocess
import sys
import threading
import time
from typing import Callable
import wave

import numpy as np

from .model_manager import ModelManager, ModelError
from .speech import SAMPLE_RATE, SileroSegmenter, SpeechRuntime

BLOCK = 1600  # 100 ms; bounds UI meter traffic and hardware read buffers.


class CaptureError(RuntimeError):
    pass


def meeting_audio_dir(data_dir: Path, meeting_id: str) -> Path:
    if not re.fullmatch(r"[a-zA-Z0-9_-]{1,100}", meeting_id):
        raise CaptureError("Invalid meeting identifier.")
    root = (Path(data_dir) / "audio").resolve()
    directory = root / meeting_id
    if directory.is_symlink() or directory.resolve().parent != root:
        raise CaptureError("The audio destination is not a meeting folder.")
    return directory


def audio_helper() -> Path | None:
    candidates = []
    if os.environ.get("TUCK_AUDIO_HELPER"):
        candidates.append(Path(os.environ["TUCK_AUDIO_HELPER"]))
    if getattr(sys, "frozen", False):
        candidates.extend(
            (
                Path(sys.executable).parent / "tuck-audio",
                Path(getattr(sys, "_MEIPASS", ".")) / "tuck-audio",
            )
        )
    candidates.append(
        Path(__file__).resolve().parents[3] / "src-tauri" / "binaries" / "tuck-audio"
    )
    return next(
        (path for path in candidates if path.is_file() and os.access(path, os.X_OK)),
        None,
    )


class _Microphone:
    def __init__(self, receive, failed):
        self.receive, self.failed = receive, failed
        self.stopping = threading.Event()
        self.stream = None
        self.thread = None

    def start(self):
        import sounddevice as sd

        try:
            self.stream = sd.InputStream(
                samplerate=SAMPLE_RATE, channels=1, dtype="float32", blocksize=BLOCK
            )
            self.stream.start()
        except Exception as exc:
            if self.stream:
                self.stream.close()
            raise CaptureError(
                "The microphone could not start. Check microphone permission and your input device."
            ) from exc
        self.thread = threading.Thread(
            target=self._read, name="tuck-microphone", daemon=True
        )
        self.thread.start()

    def _read(self):
        try:
            while not self.stopping.is_set():
                samples, overflowed = self.stream.read(BLOCK)
                if overflowed:
                    raise CaptureError(
                        "The microphone audio buffer overflowed. Recording stopped; saved audio is available for recovery."
                    )
                if not self.stopping.is_set():
                    self.receive("microphone", samples.reshape(-1))
        except Exception:
            if not self.stopping.is_set():
                self.failed(
                    "Microphone capture was interrupted. Check your audio device. Saved audio has been preserved."
                )

    def stop(self):
        self.stopping.set()
        if self.stream:
            try:
                self.stream.abort()
                self.stream.close()
            except Exception:
                pass
        if self.thread and self.thread is not threading.current_thread():
            self.thread.join(timeout=3)


class _MacSystem:
    def __init__(self, receive, failed):
        self.receive, self.failed = receive, failed
        self.stopping = threading.Event()
        self.process = None
        self.thread = None
        self.ready = queue.Queue(maxsize=1)

    def start(self):
        helper = audio_helper()
        if helper is None:
            raise CaptureError(
                "The macOS audio helper is missing. Run npm run engine:setup, then restart Tuck."
            )
        self.process = subprocess.Popen(
            [str(helper)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        threading.Thread(
            target=self._diagnostics, name="tuck-audio-status", daemon=True
        ).start()
        try:
            success = self.ready.get(timeout=20)
        except queue.Empty:
            success = False
        if not success:
            self.stop()
            raise CaptureError(
                "System audio could not start. Allow Tuck in System Settings → Privacy & Security → Screen & System Audio Recording, then restart Tuck."
            )
        self.thread = threading.Thread(
            target=self._read, name="tuck-system-audio", daemon=True
        )
        self.thread.start()

    def _diagnostics(self):
        announced = False
        try:
            for line in iter(self.process.stderr.readline, b""):
                if not announced:
                    announced = True
                    self.ready.put(line.strip() == b"READY")
            if not announced:
                self.ready.put(False)
        except (OSError, ValueError):
            if not announced:
                self.ready.put(False)

    def _read(self):
        pending = b""
        try:
            while not self.stopping.is_set():
                block = self.process.stdout.read(BLOCK * 4 - len(pending))
                if not block:
                    break
                pending += block
                if len(pending) == BLOCK * 4:
                    self.receive("system", np.frombuffer(pending, dtype="<f4").copy())
                    pending = b""
            if not self.stopping.is_set():
                self.failed(
                    "System audio capture stopped unexpectedly. Saved audio has been preserved."
                )
        except (OSError, ValueError):
            if not self.stopping.is_set():
                self.failed(
                    "System audio capture was interrupted. Saved audio has been preserved."
                )

    def stop(self):
        self.stopping.set()
        if self.process:
            try:
                if self.process.stdin:
                    self.process.stdin.close()
                self.process.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired):
                self.process.kill()
                self.process.wait(timeout=3)
            for pipe in (self.process.stdout, self.process.stderr):
                if pipe:
                    try:
                        pipe.close()
                    except OSError:
                        pass
        if self.thread and self.thread is not threading.current_thread():
            self.thread.join(timeout=3)


class _Loopback:
    """WASAPI loopback / PulseAudio monitor. Retrieve hardware afresh on each resume."""

    def __init__(self, receive, failed):
        self.receive, self.failed = receive, failed
        self.stopping = threading.Event()
        self.ready = queue.Queue(maxsize=1)
        self.thread = None

    def start(self):
        self.thread = threading.Thread(
            target=self._read, name="tuck-system-audio", daemon=True
        )
        self.thread.start()
        try:
            success = self.ready.get(timeout=15)
        except queue.Empty:
            success = False
        if not success:
            self.stop()
            raise CaptureError(
                "System audio could not start. Check your default output device and WASAPI or PulseAudio/PipeWire monitor availability."
            )

    def _read(self):
        announced = False
        try:
            import soundcard as sc

            speaker = sc.default_speaker()
            monitor = sc.get_microphone(id=speaker.id, include_loopback=True)
            if not monitor.isloopback:
                raise CaptureError("The default output has no loopback monitor.")
            # Keep all channels: the SoundCard WASAPI backend has a documented mono-capture bug.
            with monitor.recorder(
                samplerate=SAMPLE_RATE, blocksize=BLOCK * 2
            ) as recorder:
                self.ready.put(True)
                announced = True
                while not self.stopping.is_set():
                    data = recorder.record(numframes=BLOCK)
                    if not self.stopping.is_set():
                        samples = np.asarray(data, dtype=np.float32)
                        self.receive(
                            "system",
                            samples.mean(axis=1) if samples.ndim == 2 else samples,
                        )
        except Exception:
            if not announced:
                self.ready.put(False)
            elif not self.stopping.is_set():
                self.failed(
                    "System audio capture was interrupted. Check your output device. Saved audio has been preserved."
                )

    def stop(self):
        self.stopping.set()
        if self.thread and self.thread is not threading.current_thread():
            self.thread.join(timeout=4)
            if self.thread.is_alive():
                raise CaptureError(
                    "The system audio driver did not stop. Restart Tuck before recording again."
                )


class CaptureEngine:
    def __init__(self, data_dir: Path, emit: Callable[[str, dict], None]):
        self.data_dir = Path(data_dir)
        self.emit = emit
        self.models = ModelManager(self.data_dir, emit)
        self.runtime = SpeechRuntime(self.models)
        self._lock = threading.RLock()
        self._audio_lock = threading.RLock()
        self._meeting_id: str | None = None
        self._state = "idle"
        self._accepting = False
        self._sources: list = []
        self._writers: dict[str, wave.Wave_write] = {}
        self._queue: queue.Queue | None = None
        self._worker: threading.Thread | None = None
        self._segmenters: dict[str, SileroSegmenter] = {}
        self._on_segment = None
        self._error: str | None = None
        self._accumulated = 0.0
        self._epoch = 0.0
        self._last_level = 0.0
        self._levels = {"microphone": 0.0, "system": 0.0}
        self._system_audio = False
        self._microphone = False
        self._keep_audio = True

    def status(self) -> dict:
        return {
            "modelInstalled": self.models.installed(),
            "modelDownloading": self.models.downloading,
            "modelProgress": self.models.progress,
            "speakerModelInstalled": self.models.installed("speakers"),
            "activeMeetingId": self._meeting_id,
            "recordingState": self._state,
            "captureAvailable": sys.platform in ("darwin", "win32", "linux"),
            **({"error": self._error} if self._error else {}),
        }

    def _elapsed(self) -> float:
        return self._accumulated + (
            time.monotonic() - self._epoch if self._accepting else 0
        )

    def _receive(self, source: str, samples: np.ndarray):
        with self._audio_lock:
            if not self._accepting or self._queue is None:
                return
            samples = np.nan_to_num(
                np.asarray(samples, dtype=np.float32).reshape(-1), copy=True
            )
            if not samples.size:
                return
            timestamp = max(0.0, self._elapsed() - samples.size / SAMPLE_RATE)
            writer = self._writers[source]
            # Update WAV headers after every write so a crash still leaves a readable recording.
            writer.writeframes(
                (np.clip(samples, -1, 1) * 32767).astype("<i2").tobytes()
            )
            self._levels[source] = min(
                1.0, float(np.sqrt(np.mean(samples * samples))) * 4
            )
            now = time.monotonic()
            if now - self._last_level >= 0.1:
                self._last_level = now
                self.emit("audio_level", dict(self._levels))
            try:
                self._queue.put_nowait((source, samples, timestamp))
            except queue.Full:
                self._failure(
                    "Speech processing could not keep up. Recording stopped to prevent transcript gaps; saved audio can be imported for recovery."
                )

    def _failure(self, message: str):
        # Serialize failure with activation. Reader callbacks may fail during source.start().
        with self._audio_lock:
            if self._error is not None:
                return
            self._error = message
            self._accumulated = self._elapsed()
            # Stopping hardware may block and must not run in its own read thread.
            self._accepting = False
            self._state = "interrupted"
        self.emit(
            "engine_error",
            {
                "code": "capture_interrupted",
                "message": message,
                "meetingId": self._meeting_id,
            },
        )
        self.emit(
            "recording_state", {"meetingId": self._meeting_id, "state": "interrupted"}
        )
        threading.Thread(
            target=self._stop_sources_quietly, name="tuck-capture-cleanup", daemon=True
        ).start()

    def _stop_sources_quietly(self):
        for source in self._sources:
            try:
                source.stop()
            except Exception:
                pass

    def _inference(self):
        try:
            while True:
                item = self._queue.get()
                if item is None:
                    break
                if item == "flush":
                    for segmenter in self._segmenters.values():
                        segmenter.flush()
                    continue
                source, samples, start = item
                self._segmenters[source].feed(samples, start)
            for segmenter in self._segmenters.values():
                segmenter.flush()
        except Exception:
            self._failure(
                "Local transcription was interrupted. The original audio has been preserved; restart Tuck and import the recording to recover it."
            )

    def _speech(self, source: str, samples: np.ndarray, start: float):
        segment = self.runtime.transcribe(samples, start, source, self._meeting_id)
        if segment is not None:
            self._on_segment(segment)

    def _open_sources(self):
        self._sources = []
        try:
            if self._system_audio:
                self._sources.append(
                    (_MacSystem if sys.platform == "darwin" else _Loopback)(
                        self._receive, self._failure
                    )
                )
            if self._microphone:
                self._sources.append(_Microphone(self._receive, self._failure))
            for source in self._sources:
                source.start()
        except Exception:
            self._accepting = False
            self._stop_sources_quietly()
            raise

    def _activate_sources(self):
        with self._audio_lock:
            if self._error is not None or self._state == "interrupted":
                raise CaptureError(
                    self._error or "Audio capture was interrupted while starting."
                )
            self._epoch = time.monotonic()
            self._accepting = True
            self._state = "recording"

    def start(
        self,
        meeting_id: str,
        on_segment: Callable[[dict], None],
        system_audio: bool,
        microphone: bool,
        threads: int = 2,
        keep_audio: bool = True,
    ) -> None:
        with self._lock:
            if self._meeting_id is not None:
                raise CaptureError(
                    "A recording is already active. Stop it before starting another."
                )
            if not system_audio and not microphone:
                raise CaptureError(
                    "Choose microphone, system audio, or both before recording."
                )
            directory = meeting_audio_dir(self.data_dir, meeting_id)
            if directory.exists() and any(directory.iterdir()):
                raise CaptureError(
                    "This meeting already contains saved audio. Start a new meeting to preserve the original."
                )
            self.runtime.load(threads)
            directory.mkdir(parents=True, exist_ok=True, mode=0o700)
            self._meeting_id = meeting_id
            self._on_segment = on_segment
            self._system_audio, self._microphone, self._keep_audio = (
                system_audio,
                microphone,
                keep_audio,
            )
            self._error = None
            self._queue = queue.Queue(maxsize=200)
            self._segmenters = {}
            try:
                for source, enabled in (
                    ("microphone", microphone),
                    ("system", system_audio),
                ):
                    if not enabled:
                        continue
                    writer = wave.open(str(directory / f"{source}.wav"), "wb")
                    writer.setnchannels(1)
                    writer.setsampwidth(2)
                    writer.setframerate(SAMPLE_RATE)
                    self._writers[source] = writer
                    self._segmenters[source] = SileroSegmenter(
                        self.models.vad_path,
                        lambda samples, start, source=source: self._speech(
                            source, samples, start
                        ),
                    )
                self._accumulated = 0
                self._epoch = time.monotonic()
                self._accepting = False
                self._state = "recording"
                self._worker = threading.Thread(
                    target=self._inference, name="tuck-local-inference", daemon=True
                )
                self._worker.start()
                self._open_sources()
                self._activate_sources()
            except Exception:
                self._accepting = False
                self._stop_sources_quietly()
                self._finish_worker()
                self._close_writers()
                self._remove_empty_attempt(directory)
                self._meeting_id = None
                self._state = "idle"
                self.runtime.unload()
                raise

    @staticmethod
    def _remove_empty_attempt(directory: Path):
        # A permission failure must be retryable. Remove only WAVs we created with zero frames.
        for filename in ("microphone.wav", "system.wav"):
            path = directory / filename
            if not path.is_file() or path.is_symlink():
                continue
            try:
                with wave.open(str(path), "rb") as audio:
                    empty = audio.getnframes() == 0
                if empty:
                    path.unlink()
            except (OSError, wave.Error):
                pass
        try:
            directory.rmdir()  # succeeds only if no recorded evidence remains
        except OSError:
            pass

    def pause(self) -> None:
        with self._lock:
            if self._state != "recording":
                raise CaptureError("Only an active recording can be paused.")
            with self._audio_lock:
                self._accumulated = self._elapsed()
                self._accepting = False
            self._stop_sources_quietly()
            self._enqueue_control("flush")
            self._state = "paused"
            self.emit("audio_level", {"microphone": 0, "system": 0})

    def resume(self) -> None:
        with self._lock:
            if self._state != "paused":
                raise CaptureError("There is no paused recording to resume.")
            self._accepting = False
            try:
                self._open_sources()
                self._activate_sources()
            except Exception:
                self._accepting = False
                self._stop_sources_quietly()
                self._state = "interrupted" if self._error else "paused"
                raise

    def _enqueue_control(self, item):
        if self._worker and self._worker.is_alive():
            try:
                self._queue.put(item, timeout=120)
            except queue.Full as exc:
                raise CaptureError(
                    "Local transcription is still busy. Audio has been preserved; restart Tuck to recover it."
                ) from exc

    def _finish_worker(self):
        self._enqueue_control(None)
        if self._worker:
            self._worker.join(timeout=120)
            if self._worker.is_alive():
                raise CaptureError(
                    "Transcription did not finish in time. Audio has been preserved; restart Tuck to recover it."
                )

    def _close_writers(self):
        with self._audio_lock:
            for writer in self._writers.values():
                writer.close()
            self._writers = {}

    def stop(self) -> float:
        with self._lock:
            if self._meeting_id is None:
                raise CaptureError("There is no active recording to stop.")
            with self._audio_lock:
                duration = self._elapsed()
                self._accumulated = duration
                self._accepting = False
            self._stop_sources_quietly()
            self._state = "processing"
            try:
                self._finish_worker()
            finally:
                self._close_writers()
                if self._worker is None or not self._worker.is_alive():
                    self.runtime.unload()
            self._meeting_id = None
            self._state = "idle"
            self.emit("audio_level", {"microphone": 0, "system": 0})
            if self._error:
                raise CaptureError(self._error)
            # The application applies retention only after completing its durable meeting transaction.
            return round(duration, 3)

    def import_file(
        self,
        path: str,
        meeting_id: str,
        on_segment: Callable[[dict], None],
        threads: int = 2,
    ) -> float:
        with self._lock:
            if self._meeting_id:
                raise CaptureError("Stop the active recording before importing audio.")
            source = Path(path)
            if not source.is_file() or source.suffix.lower() != ".wav":
                raise CaptureError(
                    "Choose a 16-bit PCM WAV file. Convert other audio formats to WAV before importing."
                )
            directory = meeting_audio_dir(self.data_dir, meeting_id)
            if directory.exists() and any(directory.iterdir()):
                raise CaptureError(
                    "This meeting already contains audio. Import into a new meeting."
                )
            try:
                with wave.open(str(source), "rb") as audio:
                    if (
                        audio.getsampwidth() != 2
                        or audio.getcomptype() != "NONE"
                        or audio.getnchannels() not in (1, 2)
                    ):
                        raise CaptureError("Use a mono or stereo 16-bit PCM WAV file.")
                    if not 8000 <= audio.getframerate() <= 192000:
                        raise CaptureError(
                            "Use a WAV sample rate between 8 kHz and 192 kHz."
                        )
                    duration = audio.getnframes() / audio.getframerate()
                    if duration <= 0:
                        raise CaptureError("This audio file is empty.")
                self.runtime.load(threads)
                directory.mkdir(parents=True, exist_ok=True, mode=0o700)
                stored = directory / "import.wav"
                shutil.copyfile(source, stored)

                def transcribe(samples, start):
                    segment = self.runtime.transcribe(
                        samples, start, "import", meeting_id
                    )
                    if segment:
                        on_segment(segment)

                segmenter = SileroSegmenter(self.models.vad_path, transcribe)
                with wave.open(str(stored), "rb") as audio:
                    count = 0
                    input_rate = audio.getframerate()
                    resampler = None
                    if input_rate != SAMPLE_RATE:
                        import soxr

                        resampler = soxr.ResampleStream(
                            input_rate, SAMPLE_RATE, 1, dtype="float32", quality="HQ"
                        )
                    while block := audio.readframes(max(1, input_rate // 10)):
                        samples = (
                            np.frombuffer(block, dtype="<i2")
                            .astype(np.float32)
                            .reshape(-1, audio.getnchannels())
                            .mean(axis=1)
                            / 32768
                        )
                        if resampler is not None:
                            samples = resampler.resample_chunk(
                                samples, last=audio.tell() == audio.getnframes()
                            )
                        if len(samples):
                            segmenter.feed(samples, count / SAMPLE_RATE)
                            count += len(samples)
                    segmenter.flush()
                return round(duration, 3)
            except wave.Error as exc:
                raise CaptureError(
                    "This file is not a supported PCM WAV recording."
                ) from exc
            finally:
                self.runtime.unload()

    def install_model(self, kind: str) -> dict:
        with self._lock:
            if self._meeting_id:
                raise CaptureError("Stop recording before installing a model.")
            self.runtime.unload()
            return self.models.install(kind)

    def unload(self) -> None:
        with self._lock:
            if self._meeting_id:
                raise CaptureError("Stop recording before unloading speech models.")
            self.runtime.unload()

    def shutdown(self) -> None:
        with self._lock:
            if self._meeting_id:
                try:
                    self.stop()
                except Exception:
                    self._accepting = False
                    self._stop_sources_quietly()
                    self._close_writers()
            self.runtime.unload()
