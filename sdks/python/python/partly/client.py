import asyncio
import logging
import math
import os
import ssl
from enum import Enum
from urllib.parse import urlsplit

import certifi
import httpx

from ._native import MAX_POLICY_BYTES, InvalidPolicyError, PartlyError, Snapshot

logger = logging.getLogger("partly")


class Decision(Enum):
    ENABLED = "enabled"
    DISABLED = "disabled"

    def __bool__(self):
        raise TypeError("Compare explicitly with Decision.ENABLED or Decision.DISABLED")


class PolicyNotReadyError(PartlyError):
    """Call and await init() before evaluating or refreshing."""


class AuthenticationError(PartlyError):
    """The control plane rejected the SDK key."""


class RefreshError(PartlyError):
    """The control plane could not provide a policy."""


class ClientClosedError(PartlyError):
    """A closed client cannot be initialized or refreshed again."""


class ClientBuilder:
    def __init__(self):
        self._url = None
        self._secret_key = None
        self._interval = 30.0
        self._timeout = 5.0
        self._ca_bundle = None

    def url(self, value: str):
        self._url = value
        return self

    def secret_key(self, value: str):
        self._secret_key = value
        return self

    def poll_interval(self, seconds: float):
        self._interval = seconds
        return self

    def timeout(self, seconds: float):
        self._timeout = seconds
        return self

    def ca_bundle(self, path: str | os.PathLike[str]):
        self._ca_bundle = path
        return self

    def build(self):
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
        return Client(self._url.rstrip("/"), self._secret_key, self._interval, self._timeout, ca_bundle)


class Client:
    """Reuse one client per application process and event loop."""

    def __init__(self, url: str, secret_key: str, interval: float, timeout: float, ca_bundle: str | None = None):
        self._url = url
        self._secret_key = secret_key
        self._interval = interval
        self._timeout = timeout
        self._ca_bundle = ca_bundle
        self._http = None
        self._snapshot = None
        self._etag = None
        self._lock = asyncio.Lock()
        self._poll_task = None
        self._closed = False
        self.last_refresh_error = None

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
            self._poll_task = asyncio.create_task(self._poll(), name="partly-refresh")

    async def refresh(self) -> bool:
        async with self._lock:
            self._check_open()
            if self._snapshot is None:
                raise PolicyNotReadyError("Await client.init() before refreshing")
            return await self._download()

    def evaluate(self, flag_key: str, *, context: dict[str, str | int | float | bool] | None = None) -> Decision:
        snapshot = self._snapshot
        if snapshot is None:
            raise PolicyNotReadyError("No policy loaded; await client.init()")
        if not isinstance(flag_key, str):
            raise TypeError("flag_key must be a string")
        return Decision.ENABLED if snapshot.evaluate(flag_key, context) else Decision.DISABLED

    def _check_open(self):
        if self._closed:
            raise ClientClosedError("This client is closed; build a new client")

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
            if self._snapshot is not None and (snapshot.project, snapshot.environment) != (self._snapshot.project, self._snapshot.environment):
                raise InvalidPolicyError("Policy project or environment changed unexpectedly")
        except PartlyError as error:
            self.last_refresh_error = error
            raise
        except (httpx.HTTPError, asyncio.TimeoutError) as error:
            # Do not include response bodies, URLs, or credentials in diagnostics.
            failure = RefreshError(f"Policy request failed ({type(error).__name__})")
            self.last_refresh_error = failure
            raise failure from None
        # Commit only after download, response cleanup, and validation all succeed.
        # No await here: readers see one complete snapshot and matching tag.
        self._snapshot, self._etag = snapshot, etag
        self.last_refresh_error = None
        return True

    async def _request(self) -> tuple[bytes, str] | None:
        headers = {"Authorization": f"Bearer {self._secret_key}"}
        if self._etag is not None:
            headers["If-None-Match"] = self._etag
        async with self._http.stream("GET", f"{self._url}/api/policy", headers=headers) as response:
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

    async def _poll(self):
        while True:
            await asyncio.sleep(self._interval)
            try:
                await self.refresh()
            except PartlyError as error:
                logger.warning("Background refresh failed: %s", error)

    async def aclose(self) -> None:
        self._closed = True
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
