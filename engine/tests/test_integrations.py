from __future__ import annotations
import json
from pathlib import Path
import tempfile
import time
import threading
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, urlparse

import httpx
import jwt
from cryptography.hazmat.primitives.asymmetric import rsa

from tuck_engine.adapters.chatgpt import (
    ChatGPTAuth,
    AccountError,
    PLAN_SCOPE,
    model_choices,
)
from tuck_engine.adapters.llm import (
    Intelligence,
    IntelligenceError,
    local_endpoint,
    sse_events,
)
from tuck_engine.adapters.notion import (
    NotionPublisher,
    PublishError,
    PublishUncertain,
    page_payload,
    target_id,
)
from tuck_engine.adapters.secrets import SecretStore, CredentialStoreUnavailable


class MemorySecrets:
    def __init__(self):
        self.values = {}

    def get(self, key):
        return self.values.get(key)

    def set(self, key, value):
        self.values[key] = value

    def delete(self, key):
        self.values.pop(key, None)


class ProviderTests(unittest.TestCase):
    def setUp(self):
        self.secrets = MemorySecrets()
        self.secrets.set("openai", "test-api-key")
        self.auth = type("Auth", (), {"access_token": lambda s: "test-oauth-token"})()
        self.settings = {
            "provider": "chatgpt",
            "model": "account-model",
            "hostedConsent": True,
            "webSearch": False,
        }

    def client(self, fn):
        return httpx.Client(transport=httpx.MockTransport(fn))

    def stream(self, events):
        return httpx.Response(
            200,
            text="".join("data: " + json.dumps(e) + "\n\n" for e in events),
            headers={"Content-Type": "text/event-stream"},
        )

    def test_hosted_request_does_not_send_unsupported_oauth_fields(self):
        def handle(request):
            body = json.loads(request.content)
            self.assertEqual(str(request.url), "https://api.openai.com/v1/responses")
            self.assertEqual(
                request.headers["Authorization"], "Bearer test-oauth-token"
            )
            self.assertEqual(body["instructions"], "Use evidence.")
            self.assertFalse(body["store"])
            self.assertTrue(body["stream"])
            self.assertEqual(
                set(body), {"model", "instructions", "input", "store", "stream"}
            )
            return self.stream(
                [
                    {"type": "response.output_text.delta", "delta": "Answer"},
                    {"type": "response.completed"},
                ]
            )

        deltas = []
        result = Intelligence(self.auth, self.secrets, self.client(handle)).complete(
            self.settings,
            "Use evidence.",
            [{"role": "user", "content": "Question"}],
            deltas.append,
        )
        self.assertEqual(result, "Answer")
        self.assertEqual(deltas, ["Answer"])

    def test_usage_error_after_delta_does_not_commit_partial_answer(self):
        client = self.client(
            lambda _: self.stream(
                [
                    {"type": "response.output_text.delta", "delta": "Partial"},
                    {
                        "type": "response.failed",
                        "response": {
                            "error": {
                                "code": "subscription_sharing_usage_limit_exceeded"
                            }
                        },
                    },
                ]
            )
        )
        deltas = []
        with self.assertRaisesRegex(IntelligenceError, "usage limit"):
            Intelligence(self.auth, self.secrets, client).complete(
                self.settings, "", [{"role": "user", "content": "Hi"}], deltas.append
            )
        self.assertEqual(deltas, ["Partial"])

    def test_interrupted_stream_is_not_success(self):
        client = self.client(
            lambda _: self.stream(
                [{"type": "response.output_text.delta", "delta": "Partial"}]
            )
        )
        with self.assertRaisesRegex(IntelligenceError, "before the answer completed"):
            Intelligence(self.auth, self.secrets, client).complete(
                self.settings, "", [{"role": "user", "content": "Hi"}], lambda _: None
            )

    def test_no_hosted_call_without_consent(self):
        calls = []
        client = self.client(lambda req: calls.append(req))
        with self.assertRaisesRegex(IntelligenceError, "Allow hosted"):
            Intelligence(self.auth, self.secrets, client).complete(
                {**self.settings, "hostedConsent": False},
                "",
                [{"role": "user", "content": "Hi"}],
                lambda _: None,
            )
        self.assertEqual(calls, [])

    def test_web_lookup_only_sent_when_enabled(self):
        def handle(request):
            self.assertEqual(
                json.loads(request.content)["tools"], [{"type": "web_search"}]
            )
            return self.stream(
                [
                    {"type": "response.output_text.delta", "delta": "Evidence"},
                    {
                        "type": "response.output_text.annotation.added",
                        "annotation": {
                            "type": "url_citation",
                            "title": "Official source",
                            "url": "https://example.com/a",
                        },
                    },
                    {"type": "response.completed"},
                ]
            )

        result = Intelligence(self.auth, self.secrets, self.client(handle)).complete(
            {**self.settings, "webSearch": True},
            "",
            [{"role": "user", "content": "Hi"}],
            lambda _: None,
        )
        self.assertIn("[Official source](https://example.com/a)", result)

    def test_local_stream_stays_on_loopback_and_requires_done(self):
        def handle(request):
            self.assertEqual(str(request.url), "http://127.0.0.1:11434/api/chat")
            self.assertNotIn("authorization", request.headers)
            body = json.loads(request.content)
            self.assertFalse(body["think"])
            self.assertEqual(body["keep_alive"], "2m")
            return httpx.Response(
                200,
                text='{"message":{"content":"Local answer"},"done":false}\n{"done":true}\n',
            )

        result = Intelligence(self.auth, self.secrets, self.client(handle)).complete(
            {"provider": "local", "model": "qwen3:4b"},
            "Evidence.",
            [{"role": "user", "content": "Hi"}],
            lambda _: None,
        )
        self.assertEqual(result, "Local answer")

    def test_local_mode_rejects_web_fallback(self):
        with self.assertRaisesRegex(IntelligenceError, "Web lookup needs"):
            Intelligence(self.auth, self.secrets, self.client(lambda _: None)).complete(
                {"provider": "local", "model": "local", "webSearch": True},
                "",
                [{"role": "user", "content": "Hi"}],
                lambda _: None,
            )

    def test_local_endpoint_rejects_remote_and_credential_urls(self):
        for endpoint in [
            "https://example.com",
            "http://127.0.0.1.evil.com",
            "http://user:secret@127.0.0.1",
            "file:///tmp/ollama",
            "http://localhost:11434/path",
            "http://localhost?token=secret",
        ]:
            with self.subTest(endpoint=endpoint), self.assertRaises(IntelligenceError):
                local_endpoint(endpoint)
        for endpoint in [
            "http://localhost:11434",
            "http://127.0.0.1:11434",
            "http://[::1]:11434",
        ]:
            self.assertEqual(local_endpoint(endpoint), endpoint)

    def test_sse_multiline_and_comments(self):
        self.assertEqual(
            list(
                sse_events(
                    [
                        ":heartbeat",
                        'data: {"type":',
                        'data: "response.completed"}',
                        "",
                        "data: [DONE]",
                        "",
                    ]
                )
            ),
            [{"type": "response.completed"}],
        )

    def test_oauth_model_catalog_preserves_visible_order(self):
        self.assertEqual(
            model_choices(
                {
                    "models": [
                        {"slug": "b", "display_name": "B", "visibility": "list"},
                        {"slug": "hidden", "display_name": "H", "visibility": "hidden"},
                        {"slug": "a", "display_name": "A", "visibility": "list"},
                    ]
                }
            ),
            [{"id": "b", "name": "B"}, {"id": "a", "name": "A"}],
        )
        with self.assertRaises(AccountError):
            model_choices({"data": []})


class NotionTests(unittest.TestCase):
    def setUp(self):
        self.secrets = MemorySecrets()
        self.secrets.set("notion", "test-notion-key")
        self.identifier = "12345678-1234-1234-1234-123456789012"
        self.meeting = {
            "id": "meeting-id",
            "title": "Sales call",
            "summary": "## Decisions\n- Ship the pilot\n- [ ] Send proposal",
            "notes": "Budget is tentative.",
        }

    def test_notion_database_url_resolution_ignores_view_query(self):
        self.assertEqual(
            target_id(
                "https://www.notion.so/workspace/Meetings-"
                + self.identifier.replace("-", "")
                + "?v=anything"
            ),
            self.identifier,
        )
        with self.assertRaises(PublishError):
            target_id("https://evil.example/" + self.identifier)

    def test_publish_translates_title_schema_and_never_uploads_audio_or_raw_transcript(
        self,
    ):
        calls = []

        def handle(request):
            calls.append(request)
            self.assertEqual(request.headers["Notion-Version"], "2025-09-03")
            if request.method == "GET":
                return httpx.Response(
                    200, json={"properties": {"Meeting name": {"type": "title"}}}
                )
            body = json.loads(request.content)
            self.assertEqual(
                body["parent"],
                {"type": "data_source_id", "data_source_id": self.identifier},
            )
            self.assertIn("Meeting name", body["properties"])
            self.assertNotIn("secret raw audio", request.content.decode())
            self.assertNotIn("private transcript", request.content.decode())
            self.assertIn("Tuck meeting: meeting-id", request.content.decode())
            return httpx.Response(200, json={"url": "https://www.notion.so/page-id"})

        meeting = {
            **self.meeting,
            "audio": "secret raw audio",
            "segments": [{"text": "private transcript"}],
        }
        result = NotionPublisher(
            self.secrets, httpx.Client(transport=httpx.MockTransport(handle))
        ).publish(self.identifier, meeting)
        self.assertEqual(result, "https://www.notion.so/page-id")
        self.assertEqual(len(calls), 2)

    def test_database_is_resolved_to_unique_data_source(self):
        def handle(request):
            if "/databases/" in str(request.url):
                return httpx.Response(
                    200, json={"data_sources": [{"id": self.identifier}]}
                )
            if request.method == "POST":
                return httpx.Response(
                    200, json={"url": "https://www.notion.so/page-id"}
                )
            if not hasattr(handle, "seen"):
                handle.seen = True
                return httpx.Response(404)
            return httpx.Response(200, json={"properties": {"Name": {"type": "title"}}})

        result = NotionPublisher(
            self.secrets, httpx.Client(transport=httpx.MockTransport(handle))
        ).publish(self.identifier, self.meeting)
        self.assertEqual(result, "https://www.notion.so/page-id")

    def test_ambiguous_create_timeout_is_never_retried(self):
        creates = []

        def handle(request):
            if request.method == "GET":
                return httpx.Response(
                    200, json={"properties": {"Name": {"type": "title"}}}
                )
            creates.append(request)
            raise httpx.ReadTimeout("hidden raw details", request=request)

        with self.assertRaisesRegex(PublishUncertain, "Check Notion") as caught:
            NotionPublisher(
                self.secrets, httpx.Client(transport=httpx.MockTransport(handle))
            ).publish(self.identifier, self.meeting)
        self.assertNotIn("hidden raw", str(caught.exception))
        self.assertEqual(len(creates), 1)

    def test_oversize_notes_rejected_without_truncation(self):
        with self.assertRaisesRegex(PublishError, "exceed"):
            page_payload(
                self.identifier,
                "Name",
                {**self.meeting, "summary": "\n".join("line" for _ in range(101))},
            )

    def test_rich_text_splits_at_notion_limit(self):
        body = page_payload(
            self.identifier, "Name", {**self.meeting, "notes": "x" * 5000}
        )
        for block in body["children"]:
            for item in block[block["type"]]["rich_text"]:
                self.assertLessEqual(len(item["text"]["content"]), 1900)


class AuthTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name)
        self.secrets = MemorySecrets()

    def auth(self, handle):
        return ChatGPTAuth(
            self.path, self.secrets, httpx.Client(transport=httpx.MockTransport(handle))
        )

    def connected(self, auth):
        auth._state["profiles"] = [
            {
                "id": "one",
                "clientId": "oaiapp_one",
                "email": "person@example.com",
                "subject": "subject-one",
                "label": "Account 1",
            }
        ]
        auth._state["activeId"] = "one"
        auth._save()
        self.secrets.set(
            "chatgpt:one",
            json.dumps(
                {
                    "access_token": "secret-access",
                    "refresh_token": "secret-refresh",
                    "id_token": "secret-identity",
                    "scopes": [PLAN_SCOPE],
                    "expiresAt": time.time() + 3600,
                }
            ),
        )

    def test_bootstrap_account_status_has_no_tokens_and_no_network(self):
        auth = self.auth(lambda _: self.fail("No startup network call"))
        self.connected(auth)
        accounts = auth.accounts()
        self.assertTrue(accounts[0]["sharing"])
        self.assertNotIn("secret", json.dumps(accounts))
        self.assertNotIn("secret", auth._path.read_text())
        first = auth._state["hostId"]
        second = self.auth(lambda _: None)
        self.assertEqual(first, second._state["hostId"])

    def test_identity_only_account_cannot_use_plan(self):
        auth = self.auth(lambda _: self.fail("No request without permission"))
        self.connected(auth)
        self.secrets.set(
            "chatgpt:one",
            json.dumps({"access_token": "identity-only", "scopes": ["openid"]}),
        )
        with self.assertRaisesRegex(AccountError, "not enabled"):
            auth.access_token()

    def test_refresh_rotation_is_serialized_and_saved_together(self):
        calls = []

        def handle(request):
            calls.append(request)
            self.assertIn("client_id=oaiapp_one", request.content.decode())
            self.assertNotIn("scope=", request.content.decode())
            return httpx.Response(
                200,
                json={
                    "access_token": "new-access",
                    "refresh_token": "new-refresh",
                    "scope": PLAN_SCOPE,
                    "expires_in": 3600,
                },
            )

        auth = self.auth(handle)
        self.connected(auth)
        old = json.loads(self.secrets.get("chatgpt:one"))
        old["expiresAt"] = 0
        self.secrets.set("chatgpt:one", json.dumps(old))
        results = []
        threads = [
            threading.Thread(target=lambda: results.append(auth.access_token()))
            for _ in range(4)
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertEqual(len(calls), 1)
        self.assertEqual(results, ["new-access"] * 4)
        self.assertEqual(
            json.loads(self.secrets.get("chatgpt:one"))["refresh_token"], "new-refresh"
        )

    def test_sign_out_clears_locally_when_remote_revocation_cannot_be_confirmed(self):
        def handle(request):
            if request.method == "GET":
                return httpx.Response(
                    200, json={"revocation_endpoint": "https://auth.openai.com/revoke"}
                )
            return httpx.Response(503)

        auth = self.auth(handle)
        self.connected(auth)
        with patch("tuck_engine.adapters.chatgpt.time.sleep"):
            result = auth.sign_out("one")
        self.assertFalse(result["remoteRevoked"])
        self.assertFalse(result["accounts"][0]["connected"])
        self.assertIsNone(self.secrets.get("chatgpt:one"))
        self.assertEqual(auth._state["profiles"][0]["clientId"], "oaiapp_one")

    def test_actual_signature_and_nonce_validation(self):
        private = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        auth = self.auth(
            lambda _: httpx.Response(
                200,
                json={
                    "issuer": "https://auth.openai.com",
                    "jwks_uri": "https://auth.openai.com/.well-known/jwks.json",
                },
            )
        )
        claims = {
            "iss": "https://auth.openai.com",
            "aud": "oaiapp_test",
            "sub": "subject",
            "nonce": "expected",
            "exp": int(time.time()) + 300,
        }
        token = jwt.encode(claims, private, algorithm="RS256")
        key = type("Key", (), {"key": private.public_key()})()
        with patch("tuck_engine.adapters.chatgpt.jwt.PyJWKClient") as jwks:
            jwks.return_value.get_signing_key_from_jwt.return_value = key
            self.assertEqual(
                auth._verify_identity(token, "oaiapp_test", "expected")["sub"],
                "subject",
            )
            with self.assertRaises(AccountError):
                auth._verify_identity(token, "oaiapp_test", "wrong")
            with self.assertRaises(AccountError):
                auth._verify_identity(token, "wrong-audience", "expected")

    def test_reauthorization_updates_canonical_identity_and_rejects_account_switch(
        self,
    ):
        auth = self.auth(
            lambda _: httpx.Response(
                200,
                json={
                    "access_token": "new-access",
                    "id_token": "new-identity",
                    "scope": PLAN_SCOPE,
                    "expires_in": 3600,
                },
            )
        )
        auth._state["profiles"] = [
            {
                "id": "pending",
                "clientId": "oaiapp_pending",
                "email": "",
                "subject": None,
                "label": "Account 1",
            }
        ]
        auth._save()
        threads = []

        def open_browser(url):
            query = parse_qs(urlparse(url).query)
            target = (
                query["redirect_uri"][0]
                + "?code=test-code&client_id=oaiapp_pending&state="
                + query["state"][0]
            )

            def callback():
                with httpx.Client(trust_env=False) as client:
                    client.get(target)

            thread = threading.Thread(target=callback)
            thread.start()
            threads.append(thread)
            return True

        with patch(
            "tuck_engine.adapters.chatgpt.webbrowser.open", side_effect=open_browser
        ):
            with patch.object(
                auth,
                "_verify_identity",
                return_value={
                    "sub": "verified-subject",
                    "email": "verified@example.com",
                },
            ):
                auth.sign_in("pending")
            self.assertEqual(auth._state["profiles"][0]["subject"], "verified-subject")
            self.assertEqual(auth.accounts()[0]["email"], "verified@example.com")
            saved = json.loads(auth._path.read_text())
            self.assertEqual(saved["profiles"][0]["subject"], "verified-subject")
            before = self.secrets.get("chatgpt:pending")
            with patch.object(
                auth,
                "_verify_identity",
                return_value={"sub": "different-person", "email": "other@example.com"},
            ):
                with self.assertRaisesRegex(AccountError, "different account"):
                    auth.sign_in("pending")
            self.assertEqual(self.secrets.get("chatgpt:pending"), before)
        for thread in threads:
            thread.join()

    def test_dynamic_registration_saves_issued_client_before_exchange_failure(self):
        def handle(request):
            if request.method == "POST":
                form = parse_qs(request.content.decode())
                self.assertEqual(form["client_id"], ["oaiapp_issued"])
                return httpx.Response(400, json={"error": "invalid_grant"})
            self.fail("Unexpected request")

        auth = self.auth(handle)
        callback_threads = []

        def open_browser(url):
            query = parse_qs(urlparse(url).query)
            self.assertEqual(query["client_id"], ["dynamic_agent_client"])
            self.assertEqual(query["agent_name_hint"], ["Tuck"])
            self.assertTrue(query["ext_agent_host_id"][0].startswith("urn:uuid:"))
            target = (
                query["redirect_uri"][0]
                + "?code=test-code&client_id=oaiapp_issued&state="
                + query["state"][0]
            )

            def callback():
                with httpx.Client(trust_env=False) as client:
                    client.get(target)

            thread = threading.Thread(target=callback)
            thread.start()
            callback_threads.append(thread)
            return True

        with patch(
            "tuck_engine.adapters.chatgpt.webbrowser.open", side_effect=open_browser
        ):
            with self.assertRaisesRegex(AccountError, "saved account"):
                auth.sign_in()
        for t in callback_threads:
            t.join()
        self.assertEqual(auth._state["profiles"][0]["clientId"], "oaiapp_issued")
        self.assertIsNone(auth._state["activeId"])
        self.assertEqual(self.secrets.values, {})


if __name__ == "__main__":
    unittest.main()
