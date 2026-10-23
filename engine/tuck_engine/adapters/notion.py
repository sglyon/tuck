"""Publish a text-only meeting snapshot to a user-selected Notion data source.

Notion's databases/data_sources, property schemas and block JSON stop at this
adapter. The translator functions are pure. Facade and transport are collapsed:
only resolve destination and create page are needed. No generic sync framework.
"""

from __future__ import annotations

import re
from urllib.parse import urlparse
from uuid import UUID

import httpx


class PublishError(RuntimeError):
    pass


class PublishUncertain(PublishError):
    code = "publish_uncertain"


def target_id(target: str) -> str:
    value = target.strip()
    if value.startswith(("http://", "https://")):
        parsed = urlparse(value)
        if parsed.scheme != "https" or not (
            parsed.hostname in {"notion.so", "www.notion.so", "notion.site"}
            or (parsed.hostname or "").endswith(".notion.site")
        ):
            raise PublishError(
                "Use a Notion database URL or its database/data source ID."
            )
        value = parsed.path.rstrip("/").split("/")[-1]
        match = re.search(
            r"([0-9a-fA-F]{32}|[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})$",
            value,
        )
        if not match:
            raise PublishError(
                "This Notion URL has no database ID. Copy the full database link."
            )
        value = match.group(1)
    try:
        return str(UUID(value))
    except (ValueError, AttributeError) as exc:
        raise PublishError(
            "Enter a valid Notion database URL or data source ID."
        ) from exc


def _rich(text: str) -> list[dict]:
    return [
        {"type": "text", "text": {"content": text[i : i + 1900]}}
        for i in range(0, len(text), 1900)
    ] or []


def text_blocks(text: str) -> list[dict]:
    blocks = []
    in_code = False
    for line in text.splitlines():
        if line.startswith("```"):
            in_code = not in_code
            continue
        if not line.strip():
            continue
        kind, value = "paragraph", line
        extra = {}
        if not in_code:
            for heading, name in (
                ("### ", "heading_3"),
                ("## ", "heading_2"),
                ("# ", "heading_1"),
            ):
                if line.startswith(heading):
                    kind, value = name, line[len(heading) :]
                    break
            if re.match(r"^\s*[-*] \[[ xX]\] ", line):
                kind, value = "to_do", re.sub(r"^\s*[-*] \[[ xX]\] ", "", line)
                extra["checked"] = bool(re.match(r"^\s*[-*] \[[xX]\]", line))
            elif re.match(r"^\s*[-*] ", line):
                kind, value = "bulleted_list_item", re.sub(r"^\s*[-*] ", "", line)
        # A block's rich text array supports 100 items. Split long lines safely.
        for start in range(0, len(value), 20_000):
            blocks.append(
                {
                    "object": "block",
                    "type": kind,
                    kind: {"rich_text": _rich(value[start : start + 20_000]), **extra},
                }
            )
    return blocks


def page_payload(source_id: str, title_property: str, meeting: dict) -> dict:
    title = str(meeting.get("title") or "Untitled meeting")[:1900]
    content = "# Meeting notes\n" + str(meeting.get("summary") or "")
    if meeting.get("notes"):
        content += "\n\n## Personal notes\n" + str(meeting["notes"])
    content += "\n\nTuck meeting: " + str(meeting["id"])
    children = text_blocks(content)
    if len(children) > 100 or len(content.encode("utf-8")) > 350_000:
        raise PublishError(
            "These notes exceed Notion's page-create limit. Shorten the notes or export Markdown; nothing was uploaded."
        )
    return {
        "parent": {"type": "data_source_id", "data_source_id": source_id},
        "properties": {title_property: {"type": "title", "title": _rich(title)}},
        "children": children,
    }


class NotionPublisher:
    def __init__(self, secrets, client: httpx.Client | None = None):
        self._secrets = secrets
        self._client = client or httpx.Client(
            timeout=35, trust_env=False, follow_redirects=False
        )

    @staticmethod
    def _checked(response):
        if response.status_code in {401, 403, 404}:
            raise PublishError(
                "Tuck cannot access this Notion destination. Check the integration token and share the database with that integration."
            )
        if response.status_code == 429:
            raise PublishError(
                "Notion is rate limiting requests. Your notes are saved locally; try publishing later."
            )
        response.raise_for_status()
        return response.json()

    def publish(self, target: str, meeting: dict) -> str:
        token = self._secrets.get("notion")
        if not token:
            raise PublishError("Add a Notion integration token in Settings first.")
        identifier = target_id(target)
        headers = {
            "Authorization": "Bearer " + token,
            "Notion-Version": "2025-09-03",
            "Content-Type": "application/json",
        }
        base = "https://api.notion.com/v1"
        try:
            response = self._client.get(
                base + "/data_sources/" + identifier, headers=headers
            )
            if response.status_code == 404:
                database = self._checked(
                    self._client.get(base + "/databases/" + identifier, headers=headers)
                )
                sources = database.get("data_sources", [])
                if (
                    not isinstance(sources, list)
                    or len(sources) != 1
                    or not sources[0].get("id")
                ):
                    raise PublishError(
                        "This database has multiple data sources. Paste the specific data source ID in Settings."
                    )
                identifier = str(UUID(sources[0]["id"]))
                response = self._client.get(
                    base + "/data_sources/" + identifier, headers=headers
                )
            source = self._checked(response)
            properties = source.get("properties", {})
            title_property = next(
                (
                    name
                    for name, prop in properties.items()
                    if prop.get("type") == "title"
                ),
                None,
            )
            if not title_property:
                raise PublishError(
                    "This Notion data source has no title property. Choose another destination."
                )
            payload = page_payload(identifier, title_property, meeting)
        except httpx.HTTPError as exc:
            raise PublishError(
                "Could not reach the Notion destination. Your notes are saved locally; check your connection."
            ) from exc
        except (ValueError, TypeError, AttributeError) as exc:
            raise PublishError(
                "Notion returned an unreadable destination. Check the database link in Settings."
            ) from exc
        try:
            # No automatic retries: POST may have succeeded before a timeout.
            response = self._client.post(base + "/pages", headers=headers, json=payload)
            if response.status_code >= 500:
                raise PublishUncertain(
                    "Notion may have created the page. Check the destination before publishing again; Tuck paused automatic publishing to avoid duplicates."
                )
            created = self._checked(response)
            url = created.get("url")
            if not isinstance(url, str) or urlparse(url).scheme != "https":
                raise PublishUncertain(
                    "Notion accepted the page but did not return its link. Check your database before publishing again."
                )
            return url
        except PublishUncertain:
            raise
        except httpx.HTTPStatusError as exc:
            raise PublishError(
                "Notion rejected these notes. Check destination access and property requirements; your notes are saved locally."
            ) from exc
        except (httpx.HTTPError, ValueError, TypeError, AttributeError) as exc:
            raise PublishUncertain(
                "The connection ended while publishing. Check Notion for the page; Tuck paused automatic publishing to avoid duplicates."
            ) from exc
