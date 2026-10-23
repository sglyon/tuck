"""AI provider adapters. No audio leaves the device; hosted text use is opt-in.

The application consumes text deltas and model choices, never provider events.
SSE/NDJSON translation is centralized here. Each response must end successfully;
partial text is not promoted to a completed answer after a provider failure.
"""

from __future__ import annotations

import ipaddress
import json
from urllib.parse import urlparse

import httpx

from .chatgpt import RESOURCE


class IntelligenceError(RuntimeError):
    pass


def local_endpoint(value: str) -> str:
    try:
        parsed = urlparse(value.rstrip("/"))
        if (
            parsed.scheme not in {"http", "https"}
            or parsed.username
            or parsed.password
            or parsed.query
            or parsed.fragment
        ):
            raise ValueError()
        host = parsed.hostname or ""
        if host != "localhost" and not ipaddress.ip_address(host).is_loopback:
            raise ValueError()
        if parsed.path not in {"", "/"}:
            raise ValueError()
        # Access .port to validate malformed ports before constructing requests.
        _ = parsed.port
        return value.rstrip("/")
    except (ValueError, TypeError) as exc:
        raise IntelligenceError(
            "Local AI must use a loopback address such as http://127.0.0.1:11434."
        ) from exc


def sse_events(lines):
    data = []
    for line in lines:
        if not line:
            if data:
                joined = "\n".join(data)
                data.clear()
                if joined != "[DONE]":
                    yield json.loads(joined)
        elif line.startswith("data:"):
            data.append(line[5:].lstrip())
    if data and "\n".join(data) != "[DONE]":
        yield json.loads("\n".join(data))


def _provider_error(code: str) -> str:
    if code in {
        "subscription_sharing_usage_limit_exceeded",
        "rate_limit_exceeded",
        "insufficient_quota",
    }:
        return "Your AI usage limit was reached. Manage usage in Settings or switch to local AI."
    if code == "subscription_sharing_usage_unavailable":
        return "ChatGPT plan usage is unavailable for this request. Check your account permissions in Settings."
    return "The AI provider could not finish this answer. Your transcript is saved; try again."


class Intelligence:
    def __init__(self, auth, secrets, client: httpx.Client | None = None):
        self._auth = auth
        self._secrets = secrets
        self._client = client or httpx.Client(
            timeout=httpx.Timeout(180, connect=10),
            trust_env=False,
            follow_redirects=False,
        )

    def models(self, settings: dict) -> list[dict]:
        provider = settings.get("provider", "local")
        try:
            if provider == "chatgpt":
                return self._auth.models()
            if provider == "local":
                response = self._client.get(
                    local_endpoint(settings.get("localUrl", "http://127.0.0.1:11434"))
                    + "/api/tags"
                )
                response.raise_for_status()
                raw = response.json().get("models")
                if not isinstance(raw, list):
                    raise ValueError()
                return [
                    {"id": m["name"], "name": m["name"]}
                    for m in raw
                    if isinstance(m, dict) and isinstance(m.get("name"), str)
                ]
            if provider != "openai":
                raise IntelligenceError("Choose a supported AI provider in Settings.")
            key = self._secrets.get("openai")
            if not key:
                raise IntelligenceError("Add your OpenAI API key in Settings first.")
            response = self._client.get(
                RESOURCE + "/models", headers={"Authorization": "Bearer " + key}
            )
            response.raise_for_status()
            raw = response.json().get("data")
            if not isinstance(raw, list):
                raise ValueError()
            # BYOK catalog includes non-text models; keep common text families only.
            return [
                {"id": m["id"], "name": m["id"]}
                for m in raw
                if isinstance(m, dict)
                and isinstance(m.get("id"), str)
                and m["id"].startswith(("gpt-", "o1", "o3", "o4"))
                and not any(
                    part in m["id"]
                    for part in (
                        "audio",
                        "realtime",
                        "transcribe",
                        "tts",
                        "image",
                        "search-preview",
                    )
                )
            ]
        except httpx.HTTPError as exc:
            if provider == "local":
                raise IntelligenceError(
                    "Could not reach Ollama. Start Ollama, install a chat model, then refresh models."
                ) from exc
            raise IntelligenceError(
                "Could not load AI models. Check your connection and credentials."
            ) from exc
        except (ValueError, KeyError, TypeError) as exc:
            raise IntelligenceError(
                "The AI provider returned an unreadable model list."
            ) from exc

    def complete(
        self, settings: dict, system: str, messages: list[dict], on_delta
    ) -> str:
        provider = settings.get("provider", "local")
        model = settings.get("model", "").strip()
        if not model:
            raise IntelligenceError("Choose an AI model in Settings first.")
        clean_messages = [
            {"role": m["role"], "content": str(m["content"])}
            for m in messages
            if m.get("role") in {"user", "assistant"} and m.get("content")
        ]
        if not clean_messages:
            raise IntelligenceError("Enter a question first.")
        try:
            if provider == "local":
                if settings.get("webSearch"):
                    # No quiet hosted fallback: local mode has no web-search tool.
                    raise IntelligenceError(
                        "Web lookup needs a hosted AI provider. Turn web lookup off to use local AI."
                    )
                return self._local(settings, system, clean_messages, on_delta)
            if provider not in {"chatgpt", "openai"}:
                raise IntelligenceError("Choose a supported AI provider in Settings.")
            if not settings.get("hostedConsent"):
                raise IntelligenceError(
                    "Allow hosted transcript processing in Settings before using this provider."
                )
            token = (
                self._auth.access_token()
                if provider == "chatgpt"
                else self._secrets.get("openai")
            )
            if not token:
                raise IntelligenceError("Connect your AI provider in Settings first.")
            body = {
                "model": model,
                "instructions": system,
                "input": clean_messages,
                "store": False,
                "stream": True,
            }
            if settings.get("webSearch"):
                body["tools"] = [{"type": "web_search"}]
            parts, size, completed = [], 0, False
            links = []
            with self._client.stream(
                "POST",
                RESOURCE + "/responses",
                headers={"Authorization": "Bearer " + token},
                json=body,
            ) as response:
                if response.status_code in {401, 403}:
                    raise IntelligenceError(
                        "Your AI provider denied this request. Check your account, model, and permissions in Settings."
                    )
                if response.status_code == 429:
                    raise IntelligenceError(_provider_error("rate_limit_exceeded"))
                response.raise_for_status()
                for event in sse_events(response.iter_lines()):
                    if not isinstance(event, dict):
                        raise IntelligenceError(
                            "The AI provider returned an invalid stream."
                        )
                    kind = event.get("type")
                    if kind == "response.output_text.delta":
                        delta = event.get("delta", "")
                        if not isinstance(delta, str):
                            raise IntelligenceError(
                                "The AI provider returned invalid answer text."
                            )
                        size += len(delta)
                        if size > 150_000:
                            raise IntelligenceError(
                                "The answer exceeded Tuck's size limit. Ask a narrower question."
                            )
                        parts.append(delta)
                        on_delta(delta)
                    elif kind == "response.output_text.annotation.added":
                        annotation = event.get("annotation", {})
                        url = annotation.get("url", "")
                        if (
                            annotation.get("type") == "url_citation"
                            and isinstance(url, str)
                            and url.startswith("https://")
                        ):
                            links.append((annotation.get("title", "Source"), url))
                    elif kind == "response.completed":
                        completed = True
                    elif kind in {"response.failed", "response.incomplete", "error"}:
                        error = (
                            event.get("error")
                            or event.get("response", {}).get("error")
                            or {}
                        )
                        raise IntelligenceError(
                            _provider_error(error.get("code", "unknown"))
                        )
            if not completed:
                raise IntelligenceError(
                    "The AI connection ended before the answer completed. Your transcript is saved; try again."
                )
            answer = "".join(parts).strip()
            if not answer:
                raise IntelligenceError(
                    "The model completed without an answer. Choose a text model and try again."
                )
            unique = list(dict.fromkeys(links))
            if unique:
                # Model citation annotations become explicit clickable source links.
                appendix = "\n\nSources: " + " · ".join(
                    f"[{str(title).replace('[', '').replace(']', '')}]({url.replace(')', '%29')})"
                    for title, url in unique
                )
                answer += appendix
                on_delta(appendix)
            return answer
        except httpx.HTTPStatusError as exc:
            raise IntelligenceError(
                "The AI provider rejected this request. Check that your selected model supports text and any enabled web lookup."
            ) from exc
        except httpx.HTTPError as exc:
            if provider == "local":
                raise IntelligenceError(
                    "Could not reach local AI. Start Ollama and check the model in Settings."
                ) from exc
            raise IntelligenceError(
                "The AI connection was interrupted. Your transcript is saved; try again."
            ) from exc
        except (ValueError, KeyError, TypeError) as exc:
            raise IntelligenceError(
                "The AI provider returned an unreadable response. Try again."
            ) from exc

    def _local(self, settings, system, messages, on_delta):
        endpoint = local_endpoint(settings.get("localUrl", "http://127.0.0.1:11434"))
        body = {
            "model": settings["model"],
            "messages": [{"role": "system", "content": system}, *messages],
            "stream": True,
            "think": False,
            "keep_alive": "2m",
            "options": {"num_ctx": 16384},
        }
        parts, completed, size = [], False, 0
        with self._client.stream("POST", endpoint + "/api/chat", json=body) as response:
            if response.status_code == 404:
                raise IntelligenceError(
                    "This local model is not installed. Install it in Ollama and refresh models in Settings."
                )
            response.raise_for_status()
            for line in response.iter_lines():
                if not line.strip():
                    continue
                event = json.loads(line)
                if event.get("error"):
                    raise IntelligenceError(
                        "Local AI could not finish the answer. Check that Ollama has enough available memory."
                    )
                delta = event.get("message", {}).get("content", "")
                if not isinstance(delta, str):
                    raise IntelligenceError("Local AI returned invalid answer text.")
                if delta:
                    size += len(delta)
                    if size > 150_000:
                        raise IntelligenceError(
                            "The answer exceeded Tuck's size limit. Ask a narrower question."
                        )
                    parts.append(delta)
                    on_delta(delta)
                if event.get("done"):
                    if event.get("done_reason") == "length":
                        raise IntelligenceError(
                            "Local AI ran out of context. Ask a narrower question or choose another model."
                        )
                    completed = True
        if not completed:
            raise IntelligenceError(
                "Local AI stopped before completing the answer. Try again."
            )
        result = "".join(parts).strip()
        if not result:
            raise IntelligenceError(
                "Local AI returned an empty answer. Try a different model."
            )
        return result
