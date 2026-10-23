"""Native credential-store adapter. No plaintext or browser-storage fallback."""

from __future__ import annotations

import keyring
from keyring.errors import KeyringError, PasswordDeleteError


class CredentialStoreUnavailable(RuntimeError):
    pass


class SecretStore:
    SERVICE = "com.sglyon.tuck"

    def __init__(self, backend=None):
        self._backend = backend or keyring

    @staticmethod
    def _key(kind: str) -> str:
        if kind not in {"openai", "notion"} and not kind.startswith("chatgpt:"):
            raise ValueError("Unknown credential kind.")
        if len(kind) > 200:
            raise ValueError("Invalid credential identifier.")
        return kind

    def get(self, kind: str) -> str | None:
        try:
            return self._backend.get_password(self.SERVICE, self._key(kind))
        except (KeyringError, RuntimeError) as exc:
            raise CredentialStoreUnavailable(
                "Your system credential store is unavailable. Unlock it and try again."
            ) from exc

    def set(self, kind: str, value: str) -> None:
        if not isinstance(value, str) or not value.strip() or len(value) > 100_000:
            raise ValueError("Enter a valid credential.")
        try:
            self._backend.set_password(self.SERVICE, self._key(kind), value.strip())
        except (KeyringError, RuntimeError) as exc:
            raise CredentialStoreUnavailable(
                "Tuck could not save to the system credential store. Unlock it and try again."
            ) from exc

    def delete(self, kind: str) -> None:
        try:
            self._backend.delete_password(self.SERVICE, self._key(kind))
        except PasswordDeleteError:
            pass
        except (KeyringError, RuntimeError) as exc:
            raise CredentialStoreUnavailable(
                "Tuck could not clear the credential. Unlock your system credential store and try again."
            ) from exc
