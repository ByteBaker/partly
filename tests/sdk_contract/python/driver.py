# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Translate the contract's JSON commands into calls to the installed SDK."""

import asyncio
import json
import sys

from partly import Client, ContextTypeError, Decision, PartlyError

ERRORS = {
    "FlagNotFoundError": "flag_not_found",
    "InvalidContextError": "invalid_context",
    "ContextTypeError": "context_type",
    "PolicyNotReadyError": "policy_not_ready",
    "AuthenticationError": "authentication",
    "InvalidPolicyError": "invalid_policy",
    "RefreshError": "refresh",
    "ClientClosedError": "client_closed",
}


async def main():
    client = None
    try:
        while line := await asyncio.to_thread(sys.stdin.readline):
            command = json.loads(line)
            try:
                op = command["op"]
                result = {"ok": True}
                if op == "build":
                    builder = (Client.builder().url(command["url"]).secret_key(command["key"])
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
                else:
                    raise ValueError(f"Unknown contract operation: {op}")
            except PartlyError as error:
                result = {"error": ERRORS[type(error).__name__]}
                if isinstance(error, ContextTypeError):
                    result.update({name: getattr(error, name) for name in (
                        "flag_key", "k", "op", "expected_type", "actual_type",
                    )})
            print(json.dumps(result), flush=True)
    finally:
        if client is not None:
            await client.aclose()


asyncio.run(main())
