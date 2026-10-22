# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

"""Check a larger population against a flag created in the Partly dashboard."""
import asyncio
import os

from openfeature import api
from openfeature.evaluation_context import EvaluationContext

from partly import Client
from partly.openfeature import PartlyProvider


async def main() -> None:
    client = (
        Client.builder()
        .url(os.environ.get("PARTLY_URL", "http://127.0.0.1:8080"))
        .environment(os.environ["PARTLY_ENVIRONMENT"])
        .application_id(os.environ["PARTLY_APPLICATION_ID"])
        .secret_key(os.environ["PARTLY_SDK_KEY"])
        .build()
    )
    try:
        await client.init()
        api.set_provider_and_wait(PartlyProvider(client), "partly")
        flags = api.get_client("partly")
        flag = os.environ.get("PARTLY_FLAG", "new_checkout")
        users = 10_000
        enabled = 0
        for number in range(1, users + 1):
            result = flags.get_boolean_details(flag, False, EvaluationContext(targeting_key=f"user-{number}"))
            if result.error_code:
                raise RuntimeError(result.error_code)
            enabled += result.value
        print(f"Enabled for {enabled:,} of {users:,} users ({enabled / users:.1%}).")
        print("Policy updated:", await client.refresh())
    finally:
        api.shutdown()
        await client.aclose()


if __name__ == "__main__":
    asyncio.run(main())
