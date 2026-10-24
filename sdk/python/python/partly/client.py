# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

import asyncio
import logging
import math
import os
import re
import ssl
from enum import Enum
from itertools import islice
from threading import Lock
from time import monotonic
from typing import Never, Self
from urllib.parse import urlsplit

import certifi
import httpx

from ._native import MAX_POLICY_BYTES, InvalidPolicyError, PartlyError, Snapshot

logger = logging.getLogger("partly")


class Decision(Enum):
    ENABLED = "enabled"
    DISABLED = "disabled"

    def __bool__(self) -> Never:
        raise TypeError("Compare explicitly with Decision.ENABLED or Decision.DISABLED")


class PolicyNotReadyError(PartlyError):
    """Call and await init() before evaluating or refreshing."""


class AuthenticationError(PartlyError):
    """The control plane rejected the SDK key."""


class KeyRevokedError(AuthenticationError):
    """This key was revoked; build a new client with a valid key."""


class RefreshError(PartlyError):
    """The control plane could not provide a policy."""


class ClientClosedError(PartlyError):
    """A closed client cannot be initialized or refreshed again."""


class ClientBuilder:
    def __init__(self) -> None:
        self._url: str | None = None
        self._secret_key: str | None = None
        self._environment: str | None = None
        self._application_id: str | None = None
        self._interval = 30.0
        self._timeout = 5.0
        self._ca_bundle: str | os.PathLike[str] | None = None

    def url(self, value: str) -> Self:
        self._url = value
        return self

    def secret_key(self, value: str) -> Self:
        self._secret_key = value
        return self

    def environment(self, value: str) -> Self:
        self._environment = value
        return self

    def application_id(self, value: str) -> Self:
        self._application_id = value
        return self

    def poll_interval(self, seconds: float) -> Self:
        self._interval = seconds
        return self

    def timeout(self, seconds: float) -> Self:
        self._timeout = seconds
        return self

    def ca_bundle(self, path: str | os.PathLike[str]) -> Self:
        self._ca_bundle = path
        return self

    def build(self) -> "Client":
        if not isinstance(self._url, str) or any(ord(c) <= 32 or ord(c) == 127 for c in self._url):
            raise ValueError("Supply a control-plane URL without whitespace or control characters")
        parsed = urlsplit(self._url)
        if (parsed.scheme not in ("http", "https") or not parsed.hostname
                or parsed.username is not None or parsed.password is not None
                or "?" in self._url or "#" in self._url
                or parsed.path not in ("", "/")):
            raise ValueError("Supply a control-plane URL such as http://127.0.0.1:8080")
        # Accessing port validates its range and syntax before any network request.
        _ = parsed.port
        if not isinstance(self._secret_key, str) or not self._secret_key or any(not 33 <= ord(c) <= 126 for c in self._secret_key):
            raise ValueError("Supply a non-empty ASCII SDK secret key without whitespace or control characters")
        if not isinstance(self._environment, str) or not re.fullmatch(r"[a-z0-9_-]{1,64}", self._environment):
            raise ValueError("Supply an environment name")
        if not isinstance(self._application_id, str) or not re.fullmatch(r"[a-z0-9_-]{1,64}", self._application_id):
            raise ValueError("Supply an application ID")
        for value in (self._interval, self._timeout):
            try:
                valid = not isinstance(value, bool) and isinstance(value, (int, float)) and math.isfinite(value) and value > 0
            except OverflowError:
                valid = False
            if not valid:
                raise ValueError("Polling interval and timeout must be finite positive numbers")
        ca_bundle = self._ca_bundle
        if ca_bundle is not None:
            try:
                ca_bundle = os.fspath(ca_bundle)
            except TypeError:
                raise ValueError("Supply a CA bundle path as a string or PathLike") from None
            if not isinstance(ca_bundle, str) or not ca_bundle or "\0" in ca_bundle:
                raise ValueError("Supply a non-empty CA bundle path without null characters")
        return Client(self._url.rstrip("/"), self._secret_key, self._interval, self._timeout, self._environment, self._application_id, ca_bundle)


class Client:
    """Reuse one client per application process and event loop."""

    def __init__(self, url: str, secret_key: str, interval: float, timeout: float, environment: str, application_id: str, ca_bundle: str | None = None) -> None:
        self._url = url
        self._environment = environment
        self._application_id = application_id
        self._secret_key = secret_key
        self._interval = interval
        self._timeout = timeout
        self._ca_bundle = ca_bundle
        self._http: httpx.AsyncClient | None = None
        self._snapshot: Snapshot | None = None
        self._etag: str | None = None
        self._lock = asyncio.Lock()
        self._poll_task: asyncio.Task[None] | None = None
        self._closed = False
        self._revoked = False
        self._loop: asyncio.AbstractEventLoop | None = None
        self._usage_lock = Lock()
        self._usage_pending: dict[str, None] = {}
        self._usage_scheduled = False
        self._usage_task: asyncio.Task[None] | None = None
        self._usage_due = 0.0
        self._usage_retry_at = 0.0
        self.last_refresh_error: PartlyError | None = None

    @classmethod
    def builder(cls) -> ClientBuilder:
        return ClientBuilder()

    async def init(self) -> None:
        async with self._lock:
            self._check_open()
            if self._snapshot is not None:
                return
            if self._http is None:
                try:
                    verify = await asyncio.to_thread(
                        lambda: ssl.create_default_context(cafile=self._ca_bundle or certifi.where())
                    )
                except (OSError, ValueError):
                    error = RefreshError("Cannot load TLS trust certificates; check the CA bundle path and PEM contents")
                    self.last_refresh_error = error
                    raise error from None
                self._check_open()
                self._http = httpx.AsyncClient(timeout=self._timeout, verify=verify, follow_redirects=False, trust_env=False)
            await self._download()
            self._check_open()
            self._loop = asyncio.get_running_loop()
            self._usage_due = monotonic() + 60
            self._poll_task = asyncio.create_task(self._poll(), name="partly-refresh")

    async def refresh(self) -> bool:
        async with self._lock:
            self._check_open()
            if self._snapshot is None:
                raise PolicyNotReadyError("Await client.init() before refreshing")
            return await self._download()

    def evaluate(self, flag_key: str, *, context: dict[str, str | int | float | bool] | None = None) -> Decision:
        if self._revoked:
            raise KeyRevokedError("API Key revoked; build a new client with a valid key")
        snapshot = self._snapshot
        if snapshot is None:
            raise PolicyNotReadyError("No policy loaded; await client.init()")
        if not isinstance(flag_key, str):
            raise TypeError("flag_key must be a string")
        self._record_usage(flag_key, snapshot)
        return Decision.ENABLED if snapshot.evaluate(flag_key, context) else Decision.DISABLED

    def has_flag(self, flag_key: str) -> bool:
        """Check the loaded policy without evaluating context; unavailable policies still raise."""
        if self._revoked:
            raise KeyRevokedError("API Key revoked; build a new client with a valid key")
        if self._snapshot is None:
            raise PolicyNotReadyError("No policy loaded; await client.init()")
        return self._snapshot.contains_flag(flag_key)

    def _check_open(self) -> None:
        if self._revoked:
            raise KeyRevokedError("API Key revoked; build a new client with a valid key")
        if self._closed:
            raise ClientClosedError("This client is closed; build a new client")

    def _revoke(self, error: KeyRevokedError) -> None:
        with self._usage_lock:
            self._revoked = True
            self._snapshot, self._etag = None, None
            self._usage_pending.clear()
        self.last_refresh_error = error
        for task in (self._poll_task, self._usage_task):
            if task is not None and task is not asyncio.current_task():
                task.cancel()

    def _record_usage(self, flag_key: str, snapshot: Snapshot) -> None:
        with self._usage_lock:
            if self._closed or self._revoked or self._loop is None or self._snapshot is None or not snapshot.contains_flag(flag_key):
                return
            if self._snapshot is not snapshot and not self._snapshot.contains_flag(flag_key):
                return
            self._usage_pending[flag_key] = None
            now = monotonic()
            if self._usage_scheduled or now < self._usage_retry_at or (len(self._usage_pending) < 128 and now < self._usage_due):
                return
            self._usage_scheduled = True
            try:
                self._loop.call_soon_threadsafe(self._start_usage)
            except RuntimeError:
                # The application's initialized event loop may already be closed.
                self._usage_scheduled = False

    def _start_usage(self) -> None:
        with self._usage_lock:
            if self._closed or self._revoked:
                self._usage_scheduled = False
                return
            batch = list(islice(self._usage_pending, 128))
            for flag in batch:
                del self._usage_pending[flag]
            if not batch:
                self._usage_scheduled = False
                return
            now = monotonic()
            if not self._usage_pending or now < self._usage_due:
                self._usage_due = now + 60
            self._usage_task = asyncio.create_task(self._send_usage(batch), name="partly-usage")

    async def _send_usage(self, batch: list[str]) -> None:
        try:
            assert self._http is not None, "init must configure HTTP before reporting"
            async with asyncio.timeout(self._timeout):
                async with self._http.stream("POST", f"{self._url}/api/usage", headers=self._headers(), json={"flags": batch}) as response:
                    if response.status_code == 401 and response.headers.get("x-partly-error") == "key_revoked":
                        self._revoke(KeyRevokedError("API Key revoked; build a new client with a valid key"))
                    elif response.status_code != 204:
                        raise RefreshError("Usage request was not accepted")
        except (PartlyError, httpx.HTTPError, asyncio.TimeoutError):
            with self._usage_lock:
                if not self._closed and not self._revoked and self._snapshot is not None:
                    self._usage_pending = {**dict.fromkeys(flag for flag in batch if self._snapshot.contains_flag(flag)), **self._usage_pending}
                    self._usage_retry_at = monotonic() + 60
        finally:
            with self._usage_lock:
                self._usage_scheduled = False

    async def _download(self) -> bool:
        try:
            downloaded = await asyncio.wait_for(self._request(), timeout=self._timeout)
            self._check_open()
            if downloaded is None:
                self.last_refresh_error = None
                return False
            body, etag = downloaded
            snapshot = await asyncio.to_thread(Snapshot, body)
            self._check_open()
            if (snapshot.environment, snapshot.application_id) != (self._environment, self._application_id):
                raise InvalidPolicyError("Policy does not match the configured environment and application")
            if self._snapshot is not None and snapshot.project != self._snapshot.project:
                raise InvalidPolicyError("Policy project or environment changed unexpectedly")
        except KeyRevokedError as error:
            self._revoke(error)
            raise
        except PartlyError as error:
            if self._revoked:
                self._check_open()
            self.last_refresh_error = error
            raise
        except (httpx.HTTPError, asyncio.TimeoutError) as error:
            if self._revoked:
                self._check_open()
            # Do not include response bodies, URLs, or credentials in diagnostics.
            failure = RefreshError(f"Policy request failed ({type(error).__name__})")
            self.last_refresh_error = failure
            raise failure from None
        # Commit only after download, response cleanup, and validation all succeed.
        # No await here: readers see one complete snapshot and matching tag.
        with self._usage_lock:
            self._snapshot, self._etag = snapshot, etag
            self._usage_pending = {flag: None for flag in self._usage_pending if snapshot.contains_flag(flag)}
        self.last_refresh_error = None
        return True

    def _headers(self) -> dict[str, str]:
        return {"Authorization": f"Bearer {self._secret_key}", "X-Partly-Environment": self._environment, "X-Partly-Application": self._application_id}

    async def _request(self) -> tuple[bytes, str] | None:
        assert self._http is not None, "init must configure HTTP before downloading"
        headers = self._headers()
        if self._etag is not None:
            headers["If-None-Match"] = self._etag
        async with self._http.stream("GET", f"{self._url}/api/policy", headers=headers) as response:
            if response.status_code == 401 and response.headers.get("x-partly-error") == "key_revoked":
                raise KeyRevokedError("API Key revoked; build a new client with a valid key")
            if response.status_code in (401, 403):
                raise AuthenticationError("SDK key rejected by the control plane")
            if response.status_code == 304:
                if self._snapshot is None or response.headers.get("etag", self._etag) != self._etag:
                    raise InvalidPolicyError("Unchanged response has no matching local policy")
                return None
            if response.status_code != 200:
                raise RefreshError(f"Policy request returned HTTP {response.status_code}")
            etag = response.headers.get("etag", "")
            if not (len(etag) >= 3 and etag.startswith('"') and etag.endswith('"')
                    and all(33 <= ord(c) <= 126 and c != '"' for c in etag[1:-1])):
                raise InvalidPolicyError("Policy response is missing a valid ETag")
            body = bytearray()
            async for chunk in response.aiter_bytes(chunk_size=64 * 1024):
                body.extend(chunk)
                if len(body) > MAX_POLICY_BYTES:
                    raise InvalidPolicyError("Policy exceeds the 1 MiB POC limit")
            return bytes(body), etag

    async def _poll(self) -> None:
        while True:
            await asyncio.sleep(self._interval)
            try:
                await self.refresh()
            except KeyRevokedError as error:
                logger.warning("Background refresh stopped: %s", error)
                return
            except PartlyError as error:
                logger.warning("Background refresh failed: %s", error)

    async def aclose(self) -> None:
        with self._usage_lock:
            self._closed = True
            self._usage_pending.clear()
        if self._usage_task is not None:
            self._usage_task.cancel()
            try:
                await self._usage_task
            except asyncio.CancelledError:
                pass
        if self._poll_task is not None:
            self._poll_task.cancel()
            try:
                await self._poll_task
            except asyncio.CancelledError:
                pass
        async with self._lock:
            if self._http is not None:
                await self._http.aclose()
        # The accepted policy remains usable for local evaluation after network shutdown.
