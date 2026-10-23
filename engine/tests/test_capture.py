from __future__ import annotations

import hashlib
import io
from pathlib import Path
import queue
import sys
import tarfile
import tempfile
import time
import unittest
from unittest.mock import patch
import wave

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tuck_engine.adapters.capture import CaptureEngine, CaptureError, meeting_audio_dir
from tuck_engine.adapters.model_manager import (
    ModelManager,
    ModelError,
    PARAKEET_FILES,
    PARAKEET_NAME,
    extract_parakeet,
)
from tuck_engine.adapters.speech import SpeakerClusters


class FakeRuntime:
    def __init__(self, models):
        self.loaded = False

    def load(self, threads):
        self.loaded = True

    def unload(self):
        self.loaded = False

    def transcribe(self, samples, start, source, meeting_id):
        return {
            "id": f"segment-{start}",
            "meetingId": meeting_id,
            "start": start,
            "end": start + len(samples) / 16000,
            "text": "Transcribed speech",
            "source": source,
            "speakerId": "you" if source == "microphone" else "speaker-1",
        }


class FakeSegmenter:
    def __init__(self, model, callback):
        self.callback = callback
        self.pending = []

    def feed(self, samples, start):
        self.pending.append((samples, start))

    def flush(self):
        for samples, start in self.pending:
            self.callback(samples, start)
        self.pending = []


class FakeSource:
    instances = []

    def __init__(self, receive, failed):
        self.receive = receive
        self.failed = failed
        self.stopped = False
        self.__class__.instances.append(self)

    def start(self):
        pass

    def stop(self):
        self.stopped = True


class CaptureBehaviorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.data = Path(self.directory.name)
        self.events = []
        self.patches = [
            patch("tuck_engine.adapters.capture.SpeechRuntime", FakeRuntime),
            patch("tuck_engine.adapters.capture.SileroSegmenter", FakeSegmenter),
            patch("tuck_engine.adapters.capture._Microphone", FakeSource),
        ]
        for p in self.patches:
            p.start()
        self.engine = CaptureEngine(
            self.data, lambda event, payload: self.events.append((event, payload))
        )
        FakeSource.instances = []

    def tearDown(self):
        self.engine.shutdown()
        for p in self.patches:
            p.stop()
        self.directory.cleanup()

    def test_pause_closes_hardware_excludes_paused_audio_and_stop_drains_transcript(
        self,
    ):
        segments = []
        self.engine.start("meeting-1", segments.append, False, True, keep_audio=True)
        self.engine._receive("microphone", np.full(1600, 0.1, dtype=np.float32))
        source = FakeSource.instances[-1]
        self.engine.pause()
        self.assertTrue(source.stopped)
        self.engine._receive("microphone", np.ones(1600, dtype=np.float32))
        self.engine.resume()
        self.assertIsNot(FakeSource.instances[-1], source)
        self.engine._receive("microphone", np.full(1600, 0.2, dtype=np.float32))
        self.engine.stop()
        self.assertEqual(len(segments), 2)
        self.assertIsNone(self.engine.status()["activeMeetingId"])
        with wave.open(str(self.data / "audio/meeting-1/microphone.wav")) as audio:
            self.assertEqual(audio.getnframes(), 3200)
            self.assertEqual(audio.getframerate(), 16000)

    def test_no_audio_sources_is_rejected(self):
        with self.assertRaisesRegex(CaptureError, "Choose microphone"):
            self.engine.start("meeting-1", lambda x: None, False, False)
        self.assertFalse((self.data / "audio/meeting-1").exists())

    def test_failed_requested_source_does_not_leave_partial_recording(self):
        class BrokenSource(FakeSource):
            def start(self):
                raise CaptureError("Permission denied")

        with patch("tuck_engine.adapters.capture._Microphone", BrokenSource):
            with self.assertRaisesRegex(CaptureError, "Permission denied"):
                self.engine.start("meeting-1", lambda x: None, False, True)
        self.assertFalse(self.engine._accepting)
        self.assertIsNone(self.engine.status()["activeMeetingId"])
        self.assertFalse(self.engine._worker.is_alive())
        self.assertFalse((self.data / "audio/meeting-1").exists())
        self.engine.start("meeting-1", lambda x: None, False, True)
        self.engine.stop()

    def test_reader_failure_callback_during_start_cannot_return_a_live_recording(self):
        class CallbackFailureSource(FakeSource):
            def start(self):
                self.failed("Reader failed during startup")

        with patch("tuck_engine.adapters.capture._Microphone", CallbackFailureSource):
            with self.assertRaisesRegex(CaptureError, "Reader failed during startup"):
                self.engine.start("meeting-1", lambda x: None, False, True)
        self.assertFalse(self.engine._accepting)
        self.assertIsNone(self.engine.status()["activeMeetingId"])
        self.assertEqual(self.engine.status()["recordingState"], "idle")
        self.assertFalse(self.engine._worker.is_alive())
        self.assertFalse((self.data / "audio/meeting-1").exists())

    def test_reader_failure_callback_during_resume_cannot_return_a_live_recording(self):
        self.engine.start("meeting-1", lambda x: None, False, True)
        self.engine._receive("microphone", np.ones(1600, dtype=np.float32))
        self.engine.pause()

        class CallbackFailureSource(FakeSource):
            def start(self):
                self.failed("Reader failed while resuming")

        with patch("tuck_engine.adapters.capture._Microphone", CallbackFailureSource):
            with self.assertRaisesRegex(CaptureError, "Reader failed while resuming"):
                self.engine.resume()
        self.assertFalse(self.engine._accepting)
        self.assertEqual(self.engine.status()["recordingState"], "interrupted")
        with self.assertRaisesRegex(CaptureError, "Reader failed while resuming"):
            self.engine.stop()
        with wave.open(str(self.data / "audio/meeting-1/microphone.wav")) as audio:
            self.assertEqual(audio.getnframes(), 1600)

    def test_successful_stop_preserves_audio_for_application_retention_finalization(
        self,
    ):
        self.engine.start("meeting-1", lambda x: None, False, True, keep_audio=False)
        self.engine._receive("microphone", np.ones(1600, dtype=np.float32))
        self.engine.stop()
        self.assertTrue((self.data / "audio/meeting-1/microphone.wav").is_file())

    def test_failed_transcription_preserves_audio_even_when_retention_is_disabled(self):
        def fail(segment):
            raise RuntimeError("storage failure")

        self.engine.start("meeting-1", fail, False, True, keep_audio=False)
        self.engine._receive("microphone", np.ones(1600, dtype=np.float32))
        with self.assertRaisesRegex(CaptureError, "preserved"):
            self.engine.stop()
        self.assertTrue((self.data / "audio/meeting-1/microphone.wav").is_file())
        self.assertTrue(any(event == "engine_error" for event, payload in self.events))

    def test_capture_cannot_overwrite_existing_audio(self):
        folder = self.data / "audio/meeting-1"
        folder.mkdir(parents=True)
        (folder / "original.wav").write_bytes(b"original")
        with self.assertRaisesRegex(CaptureError, "already contains"):
            self.engine.start("meeting-1", lambda x: None, False, True)
        self.assertEqual((folder / "original.wav").read_bytes(), b"original")

    def test_import_checks_format_before_creating_audio_directory(self):
        source = self.data / "source.wav"
        with wave.open(str(source), "wb") as audio:
            audio.setnchannels(1)
            audio.setsampwidth(3)
            audio.setframerate(48000)
            audio.writeframes(b"\0" * 9600)
        with self.assertRaisesRegex(CaptureError, "16-bit PCM"):
            self.engine.import_file(str(source), "meeting-1", lambda x: None)
        self.assertFalse((self.data / "audio/meeting-1").exists())

    def test_import_copies_original_and_drains_final_partial_chunk(self):
        source = self.data / "source.wav"
        with wave.open(str(source), "wb") as audio:
            audio.setnchannels(2)
            audio.setsampwidth(2)
            audio.setframerate(16000)
            audio.writeframes(np.ones((2000, 2), dtype="<i2").tobytes())
        segments = []
        duration = self.engine.import_file(str(source), "meeting-1", segments.append)
        self.assertEqual(duration, 0.125)
        self.assertEqual(len(segments), 2)
        self.assertEqual(
            (self.data / "audio/meeting-1/import.wav").read_bytes(), source.read_bytes()
        )
        self.assertTrue(all(segment["source"] == "import" for segment in segments))

    def test_import_resamples_48khz_without_loading_whole_recording(self):
        source = self.data / "source.wav"
        with wave.open(str(source), "wb") as audio:
            audio.setnchannels(1)
            audio.setsampwidth(2)
            audio.setframerate(48000)
            audio.writeframes(np.ones(48000, dtype="<i2").tobytes())
        segments = []
        duration = self.engine.import_file(str(source), "meeting-1", segments.append)
        self.assertEqual(duration, 1.0)
        samples = sum(
            round((segment["end"] - segment["start"]) * 16000) for segment in segments
        )
        self.assertEqual(samples, 16000)
        self.assertAlmostEqual(segments[-1]["end"], 1.0, places=3)

    def test_paths_reject_traversal(self):
        for meeting_id in ("../escape", "a/b", "", ".hidden"):
            with self.subTest(meeting_id=meeting_id), self.assertRaises(CaptureError):
                meeting_audio_dir(self.data, meeting_id)

    def test_status_never_downloads_or_reports_uninstalled_model_as_ready(self):
        status = self.engine.status()
        self.assertFalse(status["modelInstalled"])
        self.assertFalse(status["modelDownloading"])
        self.assertFalse(status["speakerModelInstalled"])
        self.assertFalse((self.data / "models").exists())


class SpeakerClusteringTests(unittest.TestCase):
    def test_similar_turns_keep_id_and_distinct_speakers_get_different_ids(self):
        clusters = SpeakerClusters()
        self.assertEqual(clusters.identify(np.array([1, 0, 0])), "speaker-1")
        self.assertEqual(clusters.identify(np.array([0.95, 0.05, 0])), "speaker-1")
        self.assertEqual(clusters.identify(np.array([0, 1, 0])), "speaker-2")
        self.assertEqual(clusters.identify(np.array([0, 10, 0])), "speaker-2")

    def test_bad_embedding_does_not_create_a_false_identity(self):
        clusters = SpeakerClusters()
        self.assertEqual(clusters.identify(np.array([float("nan"), 0])), "speaker-1")
        self.assertEqual(len(clusters.centroids), 0)

    def test_clusters_are_bounded(self):
        clusters = SpeakerClusters(maximum=2)
        for vector in np.eye(3):
            clusters.identify(vector)
        self.assertEqual(len(clusters.centroids), 2)


class ModelSafetyTests(unittest.TestCase):
    def test_extraction_copies_only_known_regular_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / "model.tar.bz2"
            destination = root / "out"
            destination.mkdir()
            with tarfile.open(archive, "w:bz2") as package:
                for name in (*PARAKEET_FILES, "../../escape"):
                    member = tarfile.TarInfo(f"{PARAKEET_NAME}/{name}")
                    member.size = 3
                    package.addfile(member, io.BytesIO(b"abc"))
            extract_parakeet(archive, destination)
            self.assertEqual(
                sorted(p.name for p in destination.iterdir()), sorted(PARAKEET_FILES)
            )
            self.assertFalse((root / "escape").exists())

    def test_extraction_rejects_symlinks_at_expected_paths(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archive = root / "model.tar.bz2"
            destination = root / "out"
            destination.mkdir()
            with tarfile.open(archive, "w:bz2") as package:
                member = tarfile.TarInfo(f"{PARAKEET_NAME}/{PARAKEET_FILES[0]}")
                member.type = tarfile.SYMTYPE
                member.linkname = "/tmp/escape"
                package.addfile(member)
            with self.assertRaises(ModelError):
                extract_parakeet(archive, destination)

    def test_failed_download_does_not_mark_installation_ready(self):
        with tempfile.TemporaryDirectory() as temp:
            models = ModelManager(Path(temp), lambda *args: None)
            with patch.object(
                models, "_download", side_effect=ModelError("Bad checksum")
            ):
                with self.assertRaisesRegex(ModelError, "checksum"):
                    models.install("parakeet")
            self.assertFalse(models.installed())
            self.assertFalse(models.downloading)
            self.assertEqual(list(models.root.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
