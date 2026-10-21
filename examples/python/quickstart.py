# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

import asyncio

from partly import Client, Decision, PartlyError

CONTROL_PLANE_URL = "http://127.0.0.1:8080"
ENVIRONMENT = "development"
APPLICATION_ID = "YOUR_APPLICATION_ID"
API_KEY = "YOUR_API_KEY"


def checkout(client: Client, context: dict[str, str | int | float | bool]) -> str:
    decision = client.evaluate("new_checkout", context=context)
    return "Use the new checkout" if decision is Decision.ENABLED else "Use the current checkout"


async def main() -> None:
    client = (
        Client.builder()
        .url(CONTROL_PLANE_URL)
        .environment(ENVIRONMENT)
        .application_id(APPLICATION_ID)
        .secret_key(API_KEY)
        .build()
    )
    try:
        await client.init()  # Application startup, before accepting requests.
        # Two example requests reuse the initialized client and its policy.
        for user_id in ("user-123", "user-456"):
            print(checkout(client, {"id": user_id}))
    except PartlyError as error:
        # Your application chooses how to handle errors; this example stops.
        raise SystemExit(f"Partly: {error}") from None
    finally:
        await client.aclose()  # Application shutdown.


if __name__ == "__main__":
    asyncio.run(main())
