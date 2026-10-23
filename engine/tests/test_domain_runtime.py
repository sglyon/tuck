from __future__ import annotations
from copy import deepcopy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from uuid import uuid4

from tuck_engine.domain.model import (
    Meeting,
    TranscriptSegment,
    DomainError,
    ConflictError,
    DEFAULT_SETTINGS,
    updated_settings,
)
from tuck_engine.application.service import MeetingService
from tuck_engine.adapters.sqlite_store import sqlite_factory


class MemoryMeetings:
    def __init__(self, state):
        self.state = state

    def get(self, id):
        if id not in self.state["meetings"]:
            raise DomainError("Meeting missing")
        return deepcopy(self.state["meetings"][id])

    def list(self):
        return sorted(
            deepcopy(list(self.state["meetings"].values())),
            key=lambda m: m.created_at,
            reverse=True,
        )

    def save(self, meeting):
        current = self.state["meetings"].get(meeting.id)
        if (current.version if current else 0) != meeting.version:
            raise ConflictError("Stale meeting")
        meeting.version += 1
        self.state["meetings"][meeting.id] = deepcopy(meeting)

    def delete(self, meeting):
        if self.get(meeting.id).version != meeting.version:
            raise ConflictError("Stale meeting")
        del self.state["meetings"][meeting.id]
        for name in ("segments", "speakers", "messages", "bookmarks"):
            self.state[name].pop(meeting.id, None)


class MemoryEvidence:
    def __init__(self, state):
        self.state = state

    def append(self, segment):
        values = self.state["segments"].setdefault(segment.meeting_id, [])
        if any(s["id"] == segment.id for s in values):
            return False
        values.append(segment.dto())
        speakers = self.state["speakers"].setdefault(segment.meeting_id, [])
        if not any(s["id"] == segment.speaker_id for s in speakers):
            speakers.append(
                {
                    "id": segment.speaker_id,
                    "name": "You" if segment.speaker_id == "you" else "Speaker 1",
                    "color": len(speakers) % 6,
                }
            )
        return True

    def segments(self, id):
        return sorted(
            deepcopy(self.state["segments"].get(id, [])),
            key=lambda s: (s["start"], s["id"]),
        )

    def speakers(self, id):
        return deepcopy(self.state["speakers"].get(id, []))

    def rename_speaker(self, id, speaker, name):
        for s in self.state["speakers"].get(id, []):
            if s["id"] == speaker:
                s["name"] = name
                return
        raise DomainError("Unknown speaker")

    def messages(self, id):
        return deepcopy(self.state["messages"].get(id, []))

    def add_message(self, id, message):
        self.state["messages"].setdefault(id, []).append(deepcopy(message))

    def bookmarks(self, id):
        return deepcopy(self.state["bookmarks"].get(id, []))

    def add_bookmark(self, id, bookmark):
        self.state["bookmarks"].setdefault(id, []).append(deepcopy(bookmark))

    def remove_bookmark(self, id, bookmark_id):
        self.state["bookmarks"][id] = [
            b for b in self.state["bookmarks"].get(id, []) if b["id"] != bookmark_id
        ]


class MemorySettings:
    def __init__(self, state):
        self.state = state

    def get(self):
        return deepcopy(self.state["settings"])

    def save(self, values):
        self.state["settings"] = deepcopy(values)


class MemoryUnitOfWork:
    def __init__(self, store):
        self.store = store

    def __enter__(self):
        self.local = deepcopy(self.store)
        self.meetings, self.evidence, self.settings = (
            MemoryMeetings(self.local),
            MemoryEvidence(self.local),
            MemorySettings(self.local),
        )
        return self

    def commit(self):
        self.store.clear()
        self.store.update(deepcopy(self.local))

    def __exit__(self, *args):
        pass


def memory_factory():
    store = {
        "meetings": {},
        "segments": {},
        "speakers": {},
        "messages": {},
        "bookmarks": {},
        "settings": deepcopy(DEFAULT_SETTINGS),
    }
    return lambda: MemoryUnitOfWork(store)


class CaptureFake:
    def __init__(self):
        self.on_segment = None
        self.id = None
        self.fail = False
        self.stopped = False

    def status(self):
        return {"captureAvailable": True, "modelInstalled": True}

    def start(self, meeting_id, on_segment, **kwargs):
        if self.fail:
            raise RuntimeError("Permission unavailable")
        self.id, self.on_segment = meeting_id, on_segment

    def pause(self):
        pass

    def resume(self):
        pass

    def stop(self):
        self.stopped = True
        if self.on_segment:
            self.on_segment(segment(self.id, words="Final words on stop"))
        return 25.1

    def shutdown(self):
        pass

    def install_model(self, kind):
        return {"modelInstalled": True}

    def unload(self):
        pass

    def import_file(self, path, meeting_id, on_segment, **kwargs):
        on_segment(segment(meeting_id))
        return 3


class SecretsFake:
    def __init__(self):
        self.values = {}

    def get(self, kind):
        return self.values.get(kind)

    def set(self, kind, value):
        self.values[kind] = value

    def delete(self, kind):
        self.values.pop(kind, None)


class AccountsFake:
    def accounts(self):
        return []


class IntelligenceFake:
    def __init__(self):
        self.calls = []
        self.before_return = None
        self.fail = False

    def models(self, settings):
        return [{"id": "test", "name": "Test"}]

    def complete(self, settings, system, messages, on_delta):
        self.calls.append((settings, system, messages))
        if self.fail:
            raise RuntimeError("No model available")
        if self.before_return:
            self.before_return()
        on_delta("Grounded ")
        on_delta("answer [00:01]")
        return "Grounded answer [00:01]"


class PublishUncertain(RuntimeError):
    code = "publish_uncertain"


class PublisherFake:
    def __init__(self):
        self.calls = 0
        self.uncertain = False

    def publish(self, target, meeting):
        self.calls += 1
        if self.uncertain:
            raise PublishUncertain("Check Notion")
        return "https://www.notion.so/test-page"


class ArchiveFake:
    def __init__(self):
        self.deleted = []
        self.exports = []

    def remove(self, id):
        self.deleted.append(id)

    def export(self, path, content):
        self.exports.append((path, content))


def segment(meeting_id, start=1, end=2, words="A meaningful decision", id=None):
    return {
        "id": id or str(uuid4()),
        "meetingId": meeting_id,
        "start": start,
        "end": end,
        "text": words,
        "speakerId": "you",
        "source": "microphone",
    }


class DomainTests(unittest.TestCase):
    def test_lifecycle_rejects_illegal_transitions(self):
        meeting = Meeting.create("Call")
        with self.assertRaises(DomainError):
            meeting.pause()
        meeting.start()
        meeting.pause()
        meeting.resume()
        meeting.process()
        meeting.complete(3)
        with self.assertRaises(DomainError):
            meeting.start()
        with self.assertRaises(DomainError):
            meeting.resume()

    def test_notes_and_summary_have_separate_ownership(self):
        meeting = Meeting.create("Call")
        meeting.update({"notes": "Handwritten"})
        meeting.replace_summary("AI")
        self.assertEqual(meeting.notes, "Handwritten")
        with self.assertRaises(DomainError):
            meeting.update({"status": "complete"})

    def test_validation_is_atomic(self):
        meeting = Meeting.create("Original")
        with self.assertRaises(DomainError):
            meeting.update({"title": "New", "starred": "yes"})
        self.assertEqual(meeting.title, "Original")
        with self.assertRaises(DomainError):
            TranscriptSegment.from_dto(segment(meeting.id, start=4, end=2))
        with self.assertRaises(DomainError):
            TranscriptSegment.from_dto(segment(meeting.id, start=float("nan")))

    def test_uncertain_publication_requires_reconciliation(self):
        meeting = Meeting.create("Call")
        meeting.update({"notes": "Useful"})
        meeting.begin_publication()
        meeting.publication_failed(True)
        with self.assertRaises(DomainError):
            meeting.begin_publication()

    def test_settings_block_remote_local_endpoint_and_invalid_types(self):
        for changes in (
            {"localUrl": "https://example.com"},
            {"threads": True},
            {"threads": 0},
            {"hostedConsent": "true"},
            {"anything": "else"},
        ):
            with self.assertRaises(DomainError):
                updated_settings(DEFAULT_SETTINGS, changes)
        self.assertEqual(
            updated_settings(DEFAULT_SETTINGS, {"threads": 1})["threads"], 1
        )


class RepositoryContract:
    def test_persistence_and_rollback(self):
        meeting = Meeting.create("Call")
        with self.factory() as uow:
            uow.meetings.save(meeting)
            uow.commit()
        with self.factory() as uow:
            loaded = uow.meetings.get(meeting.id)
            loaded.update({"notes": "Uncommitted"})
            uow.meetings.save(loaded)
        with self.factory() as uow:
            self.assertEqual(uow.meetings.get(meeting.id).notes, "")

    def test_optimistic_concurrency_rejects_stale_aggregate(self):
        meeting = Meeting.create("Call")
        with self.factory() as uow:
            uow.meetings.save(meeting)
            uow.commit()
        with self.factory() as uow:
            stale = uow.meetings.get(meeting.id)
        with self.factory() as uow:
            fresh = uow.meetings.get(meeting.id)
            fresh.update({"title": "Changed"})
            uow.meetings.save(fresh)
            uow.commit()
        with self.factory() as uow:
            stale.update({"title": "Lost write"})
            with self.assertRaises(ConflictError):
                uow.meetings.save(stale)

    def test_evidence_deduplicates_and_delete_cascades(self):
        meeting = Meeting.create("Call")
        with self.factory() as uow:
            uow.meetings.save(meeting)
            evidence = TranscriptSegment.from_dto(segment(meeting.id))
            self.assertTrue(uow.evidence.append(evidence))
            self.assertFalse(uow.evidence.append(evidence))
            uow.evidence.add_message(
                meeting.id,
                {"id": "m", "role": "user", "content": "Hi", "createdAt": "now"},
            )
            uow.commit()
        with self.factory() as uow:
            self.assertEqual(len(uow.evidence.segments(meeting.id)), 1)
            self.assertEqual(len(uow.evidence.speakers(meeting.id)), 1)
            uow.meetings.delete(uow.meetings.get(meeting.id))
            uow.commit()
        with self.factory() as uow:
            self.assertEqual(uow.evidence.segments(meeting.id), [])
            self.assertEqual(uow.evidence.speakers(meeting.id), [])
            self.assertEqual(uow.evidence.messages(meeting.id), [])


class MemoryRepositoryTests(RepositoryContract, unittest.TestCase):
    def setUp(self):
        self.factory = memory_factory()


class SqliteRepositoryTests(RepositoryContract, unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.factory = sqlite_factory(Path(self.temp.name))

    def tearDown(self):
        self.temp.cleanup()


class ApplicationTests(unittest.TestCase):
    def setUp(self):
        self.factory = memory_factory()
        self.capture = CaptureFake()
        self.ai = IntelligenceFake()
        self.publisher = PublisherFake()
        self.secrets = SecretsFake()
        self.archive = ArchiveFake()
        self.events = []
        self.service = MeetingService(
            self.factory,
            self.capture,
            self.ai,
            self.publisher,
            self.secrets,
            AccountsFake(),
            self.archive,
            lambda event, payload: self.events.append((event, payload)),
        )

    def recorded(self):
        meeting = self.service.create_meeting("Test meeting")
        self.service.start_recording(meeting["id"])
        return self.service.stop_recording(meeting["id"])

    def test_empty_bootstrap_has_no_sample_and_no_model_calls(self):
        result = self.service.bootstrap()
        self.assertEqual(result["meetings"], [])
        self.assertEqual(self.ai.calls, [])

    def test_retention_deletes_only_after_durable_completion(self):
        self.service.update_settings({"keepAudio": False})
        meeting = self.service.create_meeting()
        self.service.start_recording(meeting["id"])

        def remove(identifier):
            self.assertEqual(self.service.meeting(identifier)["status"], "complete")
            self.archive.deleted.append(identifier)

        self.archive.remove = remove
        self.service.stop_recording(meeting["id"])
        self.assertEqual(self.archive.deleted, [meeting["id"]])

    def test_completion_failure_preserves_recovery_audio(self):
        self.service.update_settings({"keepAudio": False})
        meeting = self.service.create_meeting()
        self.service.start_recording(meeting["id"])
        original = MemoryMeetings.save

        def fail_complete(repository, value):
            if value.status == "complete":
                raise RuntimeError("simulated final persistence failure")
            return original(repository, value)

        with patch.object(MemoryMeetings, "save", fail_complete):
            with self.assertRaises(RuntimeError):
                self.service.stop_recording(meeting["id"])
        self.assertEqual(self.archive.deleted, [])
        recovered = self.service.meeting(meeting["id"])
        self.assertEqual(recovered["status"], "interrupted")
        self.assertTrue(recovered["segments"])

    def test_stop_drains_and_persists_final_segment_before_return(self):
        meeting = self.recorded()
        self.assertEqual(meeting["status"], "complete")
        self.assertEqual(meeting["segments"][0]["text"], "Final words on stop")
        self.assertEqual(meeting["duration"], 25.1)

    def test_only_one_meeting_records_and_active_cannot_delete(self):
        first = self.service.create_meeting()
        second = self.service.create_meeting()
        self.service.start_recording(first["id"])
        with self.assertRaises(DomainError):
            self.service.start_recording(second["id"])
        with self.assertRaises(DomainError):
            self.service.delete_meeting(first["id"])
        self.service.stop_recording(first["id"])

    def test_failed_start_returns_pristine_meeting_to_draft(self):
        meeting = self.service.create_meeting()
        self.capture.fail = True
        with self.assertRaises(RuntimeError):
            self.service.start_recording(meeting["id"])
        self.assertEqual(self.service.meeting(meeting["id"])["status"], "draft")
        self.assertIsNone(self.service.get_status()["activeMeetingId"])

    def test_summary_preserves_notes_edited_during_inference(self):
        meeting = self.recorded()
        id = meeting["id"]
        self.ai.before_return = lambda: self.service.update_meeting(
            id, {"notes": "Edited while AI worked"}
        )
        result = self.service.summarize(id)
        self.assertEqual(result["notes"], "Edited while AI worked")
        self.assertIn("Grounded", result["summary"])
        self.assertFalse(self.ai.calls[0][0]["webSearch"])

    def test_long_summary_covers_every_evidence_section(self):
        meeting = self.service.create_meeting()
        id = meeting["id"]
        self.service.start_recording(id)
        self.capture.on_segment(
            segment(id, words="EARLY DECISION " + "evidence " * 2900)
        )
        self.capture.on_segment(
            segment(id, start=400, end=410, words="LATE DECISION " + "evidence " * 2900)
        )
        self.service.stop_recording(id)
        self.service.summarize(id)
        section_calls = [
            call for call in self.ai.calls if "TRANSCRIPT SECTION" in call[1]
        ]
        self.assertGreaterEqual(len(section_calls), 2)
        self.assertTrue(any("EARLY DECISION" in call[1] for call in section_calls))
        self.assertTrue(any("LATE DECISION" in call[1] for call in section_calls))
        self.assertIn("COVERING ALL TRANSCRIPT SECTIONS", self.ai.calls[-1][1])

    def test_pause_does_not_hold_a_transaction_during_capture_flush(self):
        temp = tempfile.TemporaryDirectory()
        try:
            factory = sqlite_factory(Path(temp.name))
            service = MeetingService(
                factory,
                self.capture,
                self.ai,
                self.publisher,
                self.secrets,
                AccountsFake(),
                self.archive,
                lambda *args: None,
            )
            meeting = service.create_meeting()
            id = meeting["id"]
            service.start_recording(id)
            self.capture.pause = lambda: self.capture.on_segment(segment(id))
            result = service.pause_recording(id)
            self.assertEqual(result["status"], "paused")
            self.assertEqual(len(result["segments"]), 1)
        finally:
            temp.cleanup()

    def test_uncertain_publish_is_not_retried(self):
        meeting = self.recorded()
        id = meeting["id"]
        self.service.update_meeting(id, {"notes": "Notes to send"})
        self.service.update_settings({"notionTarget": "database"})
        self.publisher.uncertain = True
        with self.assertRaises(PublishUncertain):
            self.service.publish_notion(id)
        with self.assertRaises(DomainError):
            self.service.publish_notion(id)
        self.assertEqual(self.publisher.calls, 1)
        self.assertEqual(self.service.meeting(id)["notionStatus"], "uncertain")

    def test_example_explicit_idempotent_and_cannot_publish(self):
        first = self.service.load_example()
        second = self.service.load_example()
        self.assertEqual(first["id"], second["id"])
        self.assertTrue(first["isExample"])
        self.service.update_settings({"notionTarget": "database"})
        with self.assertRaises(DomainError):
            self.service.publish_notion(first["id"])

    def test_export_includes_actual_speaker_and_evidence(self):
        meeting = self.recorded()
        id = meeting["id"]
        self.service.rename_speaker(id, "you", "SGL")
        srt = self.service.export_meeting(id, "srt")["content"]
        self.assertIn("00:00:01,000 --> 00:00:02,000", srt)
        self.assertIn("SGL: Final words on stop", srt)
        exported = json.loads(self.service.export_meeting(id, "json")["content"])
        self.assertEqual(exported["id"], id)

    def test_failed_chat_retains_user_message_without_fake_answer(self):
        meeting = self.recorded()
        self.ai.fail = True
        with self.assertRaises(RuntimeError):
            self.service.chat(meeting["id"], "What did we decide?", "request")
        messages = self.service.meeting(meeting["id"])["messages"]
        self.assertEqual(len(messages), 1)
        self.assertEqual(messages[0]["role"], "user")

    def test_startup_recovers_abandoned_recording_and_ambiguous_publish(self):
        meeting = self.service.create_meeting()
        self.service.start_recording(meeting["id"])
        replacement = MeetingService(
            self.factory,
            CaptureFake(),
            self.ai,
            self.publisher,
            self.secrets,
            AccountsFake(),
            self.archive,
            lambda *args: None,
        )
        self.assertEqual(replacement.meeting(meeting["id"])["status"], "interrupted")

    def test_credential_failure_keeps_bootstrap_available(self):
        def unavailable(_kind):
            raise RuntimeError("Locked keyring")

        self.secrets.get = unavailable
        result = self.service.bootstrap()
        self.assertFalse(result["settings"]["hasOpenAIKey"])
        self.assertIn("credential store", result["status"]["error"])

    def test_segment_event_occurs_after_commit(self):
        meeting = self.service.create_meeting()
        self.service.start_recording(meeting["id"])

        def assert_saved(event, payload):
            if event == "segment":
                self.assertTrue(self.service.meeting(meeting["id"])["segments"])

        self.service.emit = assert_saved
        self.capture.on_segment(segment(meeting["id"]))


if __name__ == "__main__":
    unittest.main()
