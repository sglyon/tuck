"""Official public Sign in with ChatGPT adapter.

OAuth registration, token and model schemas stop here; the application receives
redacted account/model choices. Facade and adapter share one small HTTP client;
public_account and model_choices are the stateless translation boundary.
Tokens stay in the OS credential store. Registration metadata contains no tokens.
"""

from __future__ import annotations

import base64
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import secrets
import threading
import time
from urllib.parse import parse_qs, urlencode, urlparse
from uuid import uuid4
import webbrowser

import httpx
import jwt

AUTHORITY = "https://auth.openai.com"
TOKEN_ENDPOINT = AUTHORITY + "/api/accounts/oauth/token"
RESOURCE = "https://api.openai.com/v1"
PLAN_SCOPE = "chatgpt.tokens.use.direct"
SCOPES = "openid profile email offline_access resource.invoke " + PLAN_SCOPE


class AccountError(RuntimeError):
    pass


def atomic_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_suffix("." + uuid4().hex + ".tmp")
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(value, f)
            f.flush()
            os.fsync(f.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def model_choices(raw: dict) -> list[dict]:
    """OAuth catalog differs from the API-key catalog: preserve server order."""
    models = raw.get("models")
    if not isinstance(models, list):
        raise AccountError(
            "ChatGPT returned an unreadable model catalog. Reconnect and try again."
        )
    result = []
    for model in models:
        if isinstance(model, dict) and model.get("visibility") == "list":
            slug, name = model.get("slug"), model.get("display_name")
            if isinstance(slug, str) and isinstance(name, str):
                result.append({"id": slug, "name": name})
    return result


class ChatGPTAuth:
    def __init__(self, data_dir: Path, secrets, client: httpx.Client | None = None):
        self._secrets = secrets
        self._path = Path(data_dir) / "chatgpt-registrations.json"
        self._client = client or httpx.Client(
            timeout=30, follow_redirects=False, trust_env=False
        )
        self._lock = threading.RLock()
        self._signin_lock = threading.Lock()
        if self._path.exists():
            try:
                self._state = json.loads(self._path.read_text())
                if not isinstance(
                    self._state.get("profiles"), list
                ) or not self._state.get("hostId"):
                    raise ValueError()
            except (ValueError, OSError) as exc:
                raise AccountError(
                    "ChatGPT registration data could not be read. Restore its local backup before reconnecting."
                ) from exc
        else:
            self._state = {
                "hostId": "urn:uuid:" + str(uuid4()),
                "activeId": None,
                "profiles": [],
            }
            self._save()

    def _save(self):
        atomic_json(self._path, self._state)

    def _profile(self, account_id=None) -> dict:
        selected = account_id or self._state.get("activeId")
        profile = next(
            (p for p in self._state["profiles"] if p["id"] == selected), None
        )
        if profile is None:
            raise AccountError("Connect your ChatGPT account in Settings first.")
        return profile

    def _credentials(self, profile: dict) -> dict:
        raw = self._secrets.get("chatgpt:" + profile["id"])
        if not raw:
            return {}
        try:
            credentials = json.loads(raw)
            return credentials if isinstance(credentials, dict) else {}
        except ValueError:
            return {}

    def accounts(self) -> list[dict]:
        with self._lock:
            result = []
            for profile in self._state["profiles"]:
                credentials = self._credentials(profile)
                connected = bool(credentials.get("access_token"))
                result.append(
                    {
                        "id": profile["id"],
                        "email": profile.get("email", ""),
                        "label": profile.get("label", "ChatGPT account"),
                        "connected": connected,
                        "sharing": connected
                        and PLAN_SCOPE in credentials.get("scopes", []),
                        "active": profile["id"] == self._state.get("activeId"),
                    }
                )
            return result

    def select(self, account_id: str) -> list[dict]:
        with self._lock:
            self._profile(account_id)
            self._state["activeId"] = account_id
            self._save()
            return self.accounts()

    def _verify_identity(self, token: str, client_id: str, nonce: str) -> dict:
        try:
            discovery = self._client.get(
                AUTHORITY + "/.well-known/openid-configuration"
            )
            discovery.raise_for_status()
            config = discovery.json()
            jwks_uri = config["jwks_uri"]
            parsed = urlparse(jwks_uri)
            if (
                config.get("issuer") != AUTHORITY
                or parsed.scheme != "https"
                or parsed.hostname != "auth.openai.com"
            ):
                raise ValueError("Unexpected identity authority")
            # PyJWKClient verifies signatures using the published rotating JWKS.
            key = jwt.PyJWKClient(jwks_uri, timeout=20).get_signing_key_from_jwt(token)
            claims = jwt.decode(
                token,
                key.key,
                algorithms=["RS256"],
                audience=client_id,
                issuer=AUTHORITY,
                options={"require": ["exp", "iss", "aud", "sub", "nonce"]},
            )
            if not secrets.compare_digest(str(claims["nonce"]), nonce):
                raise ValueError("Nonce mismatch")
            return claims
        except (
            jwt.PyJWTError,
            httpx.HTTPError,
            ValueError,
            KeyError,
            TypeError,
        ) as exc:
            raise AccountError(
                "ChatGPT identity verification failed. No new credentials were saved; please reconnect."
            ) from exc

    def sign_in(self, account_id=None) -> list[dict]:
        if not self._signin_lock.acquire(blocking=False):
            raise AccountError("A ChatGPT sign-in is already open in your browser.")
        try:
            return self._sign_in(account_id)
        finally:
            self._signin_lock.release()

    def _sign_in(self, account_id=None) -> list[dict]:
        with self._lock:
            selected = self._profile(account_id).copy() if account_id else None
            old = self._credentials(selected) if selected else {}
        state, nonce, verifier = (secrets.token_urlsafe(48) for _ in range(3))
        challenge = (
            base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest())
            .decode()
            .rstrip("=")
        )
        callback = {}

        class Callback(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass  # authorization codes must never enter logs

            def do_GET(self):
                parsed = urlparse(self.path)
                values = {
                    k: v[0] for k, v in parse_qs(parsed.query).items() if len(v) == 1
                }
                valid = parsed.path == "/auth/callback" and secrets.compare_digest(
                    values.get("state", ""), state
                )
                if valid:
                    callback.update(values)
                self.send_response(200 if valid else 400)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Cache-Control", "no-store")
                self.send_header(
                    "Content-Security-Policy",
                    "default-src 'none'; style-src 'unsafe-inline'",
                )
                self.end_headers()
                text = (
                    "Return to Tuck to finish connecting."
                    if valid
                    else "This sign-in link is invalid. Start again in Tuck."
                )
                self.wfile.write(
                    (
                        "<!doctype html><html><title>Tuck</title><body style='font:18px system-ui;padding:64px'><h1>Tuck</h1><p>"
                        + text
                        + "</p></body></html>"
                    ).encode()
                )

        with HTTPServer(("127.0.0.1", 0), Callback) as server:
            server.timeout = 0.5
            redirect = f"http://127.0.0.1:{server.server_port}/auth/callback"
            client_id = selected["clientId"] if selected else "dynamic_agent_client"
            params = {
                "client_id": client_id,
                "ext_agent_host_id": self._state["hostId"],
                "response_type": "code",
                "redirect_uri": redirect,
                "scope": SCOPES,
                "resource": RESOURCE,
                "state": state,
                "nonce": nonce,
                "code_challenge_method": "S256",
                "code_challenge": challenge,
            }
            if not selected:
                params["agent_name_hint"] = "Tuck"
            elif old.get("id_token"):
                params["id_token_hint"] = old["id_token"]
            if selected and selected.get("email"):
                params["login_hint"] = selected["email"]
            if not webbrowser.open(
                AUTHORITY + "/api/accounts/authorize?" + urlencode(params)
            ):
                raise AccountError(
                    "Tuck could not open your browser. Set a default browser and try again."
                )
            deadline = time.monotonic() + 240
            while not callback and time.monotonic() < deadline:
                server.handle_request()
            if not callback:
                raise AccountError(
                    "ChatGPT sign-in timed out. Start again from Settings."
                )
        if callback.get("error"):
            raise AccountError(
                "ChatGPT sign-in was not approved. Your existing account has not changed."
            )
        issued = callback.get("client_id") or (
            selected["clientId"] if selected else None
        )
        if (
            not issued
            or issued == "dynamic_agent_client"
            or (selected and issued != selected["clientId"])
        ):
            raise AccountError(
                "ChatGPT returned an unexpected registration. Please start sign-in again."
            )
        code = callback.get("code")
        if not code:
            raise AccountError(
                "ChatGPT did not return an authorization code. Try again."
            )
        with self._lock:
            # Persist registration before exchange so an expired code does not lose it.
            pending = (
                self._profile(selected["id"])
                if selected
                else next(
                    (p for p in self._state["profiles"] if p["clientId"] == issued),
                    None,
                )
            )
            if pending is None:
                pending = {
                    "id": str(uuid4()),
                    "clientId": issued,
                    "email": "",
                    "subject": None,
                    "label": "ChatGPT account " + str(len(self._state["profiles"]) + 1),
                }
                self._state["profiles"].append(pending)
            self._save()
        try:
            response = self._client.post(
                TOKEN_ENDPOINT,
                data={
                    "grant_type": "authorization_code",
                    "client_id": issued,
                    "code": code,
                    "code_verifier": verifier,
                    "redirect_uri": redirect,
                    "resource": RESOURCE,
                },
            )
            response.raise_for_status()
            tokens = response.json()
        except (httpx.HTTPError, ValueError) as exc:
            raise AccountError(
                "ChatGPT could not complete sign-in. Retry with this saved account in Settings."
            ) from exc
        if (
            not isinstance(tokens, dict)
            or not tokens.get("id_token")
            or not tokens.get("access_token")
        ):
            raise AccountError(
                "ChatGPT returned incomplete credentials. Please reconnect."
            )
        identity = self._verify_identity(tokens["id_token"], issued, nonce)
        if pending.get("subject") and identity["sub"] != pending["subject"]:
            raise AccountError(
                "This sign-in belongs to a different account. Add it as a separate ChatGPT account."
            )
        tokens["scopes"] = str(tokens.get("scope", "")).split()
        tokens["expiresAt"] = time.time() + float(tokens.get("expires_in", 3600))
        with self._lock:
            self._secrets.set("chatgpt:" + pending["id"], json.dumps(tokens))
            pending["subject"] = identity["sub"]
            pending["email"] = identity.get("email", "")
            self._state["activeId"] = pending["id"]
            self._save()
        return self.accounts()

    def access_token(self) -> str:
        with self._lock:
            profile = self._profile()
            credentials = self._credentials(profile)
            if PLAN_SCOPE not in credentials.get("scopes", []):
                raise AccountError(
                    "ChatGPT plan usage is not enabled for this account. Reconnect and approve plan usage."
                )
            if not credentials.get("access_token"):
                raise AccountError("Reconnect your ChatGPT account in Settings.")
            if credentials.get("expiresAt", 0) <= time.time() + 90:
                if not credentials.get("refresh_token"):
                    raise AccountError(
                        "Your ChatGPT connection expired. Reconnect in Settings."
                    )
                try:
                    response = self._client.post(
                        TOKEN_ENDPOINT,
                        data={
                            "grant_type": "refresh_token",
                            "client_id": profile["clientId"],
                            "refresh_token": credentials["refresh_token"],
                            "resource": RESOURCE,
                        },
                    )
                    response.raise_for_status()
                    renewed = response.json()
                    if not renewed.get("access_token"):
                        raise ValueError()
                except (httpx.HTTPError, ValueError, AttributeError) as exc:
                    raise AccountError(
                        "Your ChatGPT connection could not be renewed. Reconnect in Settings."
                    ) from exc
                # One lock covers refresh and atomic keyring replacement of rotating tokens.
                credentials.update(renewed)
                if "scope" in renewed:
                    credentials["scopes"] = str(renewed["scope"]).split()
                credentials["expiresAt"] = time.time() + float(
                    renewed.get("expires_in", 3600)
                )
                self._secrets.set("chatgpt:" + profile["id"], json.dumps(credentials))
                if PLAN_SCOPE not in credentials.get("scopes", []):
                    raise AccountError(
                        "ChatGPT plan permission is no longer enabled. Reconnect in Settings."
                    )
            return credentials["access_token"]

    def models(self) -> list[dict]:
        try:
            response = self._client.get(
                RESOURCE + "/models",
                headers={"Authorization": "Bearer " + self.access_token()},
            )
            response.raise_for_status()
            return model_choices(response.json())
        except (httpx.HTTPError, ValueError) as exc:
            raise AccountError(
                "Could not load models from ChatGPT. Check your connection and account access."
            ) from exc

    def sign_out(self, account_id: str) -> dict:
        with self._lock:
            profile = self._profile(account_id)
            credentials = self._credentials(profile)
            revoked = not bool(credentials.get("refresh_token"))
            if not revoked:
                try:
                    discovery = self._client.get(
                        AUTHORITY + "/.well-known/openid-configuration"
                    )
                    discovery.raise_for_status()
                    endpoint = discovery.json().get("revocation_endpoint", "")
                    parsed = urlparse(endpoint)
                    if parsed.scheme != "https" or parsed.hostname != "auth.openai.com":
                        raise ValueError()
                    for attempt in range(3):
                        try:
                            response = self._client.post(
                                endpoint,
                                data={
                                    "token": credentials["refresh_token"],
                                    "token_type_hint": "refresh_token",
                                    "client_id": profile["clientId"],
                                },
                            )
                            if response.status_code == 200:
                                revoked = True
                                break
                            if response.status_code < 500:
                                break
                        except httpx.HTTPError:
                            pass
                        if attempt < 2:
                            time.sleep(0.5 * 2**attempt)
                except (httpx.HTTPError, ValueError):
                    pass
            self._secrets.delete("chatgpt:" + profile["id"])
            if self._state["activeId"] == profile["id"]:
                self._state["activeId"] = None
            self._save()
            return {"accounts": self.accounts(), "remoteRevoked": revoked}
