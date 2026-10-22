# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Translate the contract's JSON commands into calls to the installed SDK."""

import asyncio
import io
import json
import runpy
import sys
from contextlib import redirect_stdout

from partly import Client, ContextTypeError, Decision, InvalidContextError, PartlyError

ERRORS = {
    "FlagNotFoundError": "flag_not_found",
    "InvalidContextError": "invalid_context",
    "ContextTypeError": "context_type",
    "PolicyNotReadyError": "policy_not_ready",
    "KeyRevokedError": "key_revoked",
    "AuthenticationError": "authentication",
    "InvalidPolicyError": "invalid_policy",
    "RefreshError": "refresh",
    "ClientClosedError": "client_closed",
}


async def main():
    client = None
    checkpoint = None
    try:
        while line := await asyncio.to_thread(sys.stdin.readline):
            command = json.loads(line)
            try:
                op = command["op"]
                result = {"ok": True}
                if op == "build":
                    builder = (Client.builder().url(command["url"]).environment(command["environment"]).application_id(command["application_id"]).secret_key(command["key"])
                               .poll_interval(command["interval"]).timeout(command["timeout"]))
                    if command.get("ca_bundle"):
                        builder.ca_bundle(command["ca_bundle"])
                    client = builder.build()
                elif op == "init":
                    await client.init()
                elif op == "evaluate":
                    decision = client.evaluate(command["flag"], context=command["context"])
                    result = {"decision": {Decision.ENABLED: "enabled", Decision.DISABLED: "disabled"}[decision]}
                elif op == "refresh":
                    result = {"updated": await client.refresh()}
                elif op == "close":
                    await client.aclose()
                elif op == "checkpoint":
                    checkpoint = client._snapshot, client._etag
                elif op == "assert_preserved":
                    assert client._snapshot is checkpoint[0] and client._etag == checkpoint[1]
                elif op == "assert_cleared":
                    assert client._snapshot is None and client._etag is None
                elif op == "state":
                    assert client._secret_key not in str(client.last_refresh_error)
                    result = {"polling": client._poll_task is not None,
                              "poll_done": client._poll_task is not None and client._poll_task.done(),
                              "http_closed": client._http is None or client._http.is_closed,
                              "refresh_failed": client.last_refresh_error is not None,
                              "etag": client._etag}
                elif op == "population":
                    result = {"selected": [index for index in range(command.get("start", 0), command["count"])
                                           if client.evaluate(command["flag"], context={"id": f"user-{index}"}) is Decision.ENABLED]}
                elif op == "quickstart":
                    original_init = Client.init
                    clients = []

                    async def initialized(instance, original_init=original_init, clients=clients):
                        await original_init(instance)
                        clients.append(instance)
                        print(json.dumps({"ready": True}), file=sys.__stdout__, flush=True)
                        assert json.loads(await asyncio.to_thread(sys.stdin.readline))["op"] == "continue"

                    example = runpy.run_path(command["path"])
                    example["main"].__globals__.update(CONTROL_PLANE_URL=command["url"], API_KEY=command["key"],
                                                      APPLICATION_ID=command["application_id"])
                    Client.init = initialized
                    captured = io.StringIO()
                    try:
                        with redirect_stdout(captured):
                            await example["main"]()
                    finally:
                        Client.init = original_init
                    assert len(clients) == 1 and clients[0]._http.is_closed and clients[0]._poll_task.done()
                    result = {"lines": captured.getvalue().splitlines()}
                else:
                    raise ValueError(f"Unknown contract operation: {op}")
            except PartlyError as error:
                result = {"error": ERRORS[type(error).__name__]}
                if isinstance(error, ContextTypeError):
                    assert isinstance(error, InvalidContextError)
                    assert "private-context-value" not in str(error) and "sam@company.com" not in str(error)
                    result.update({name: getattr(error, name) for name in (
                        "flag_key", "k", "op", "expected_type", "actual_type",
                    )})
            print(json.dumps(result), flush=True)
    finally:
        if client is not None:
            await client.aclose()


asyncio.run(main())
