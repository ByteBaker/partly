import asyncio
import logging
import math
from enum import Enum
from urllib.parse import urlsplit

import httpx

from ._native import InvalidPolicyError, PartlyError, Snapshot

logger = logging.getLogger("partly")
MAX_POLICY_BYTES = 1024 * 1024


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

    def build(self):
        parsed = urlsplit(self._url or "")
        if (parsed.scheme not in ("http", "https") or not parsed.hostname
                or parsed.username or parsed.password or parsed.query or parsed.fragment
                or parsed.path not in ("", "/")):
            raise ValueError("Supply a control-plane URL such as http://127.0.0.1:8080")
        # Accessing port validates its range and syntax before any network request.
        _ = parsed.port
        if not isinstance(self._secret_key, str) or not self._secret_key or any(c.isspace() for c in self._secret_key) or not self._secret_key.isascii():
            raise ValueError("Supply a non-empty SDK secret key without whitespace")
        for value in (self._interval, self._timeout):
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value <= 0:
                raise ValueError("Polling interval and timeout must be finite positive numbers")
        return Client(self._url.rstrip("/"), self._secret_key, self._interval, self._timeout)


class Client:
    """Reuse one client per application process and event loop."""

    def __init__(self, url: str, secret_key: str, interval: float, timeout: float):
        self._url = url
        self._secret_key = secret_key
        self._interval = interval
        self._timeout = timeout
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
                self._http = httpx.AsyncClient(timeout=self._timeout, follow_redirects=False, trust_env=False)
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
            if downloaded is None:
                self.last_refresh_error = None
                return False
            body, etag = downloaded
            snapshot = Snapshot(body)
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
            if not (len(etag) >= 3 and etag.startswith('"') and etag.endswith('"')):
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
