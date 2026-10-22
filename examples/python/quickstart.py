# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

import asyncio

from openfeature import api
from openfeature.client import OpenFeatureClient
from openfeature.evaluation_context import EvaluationContext

from partly import Client, PartlyError
from partly.openfeature import PartlyProvider

CONTROL_PLANE_URL = "http://127.0.0.1:8080"
ENVIRONMENT = "development"
APPLICATION_ID = "YOUR_APPLICATION_ID"
API_KEY = "YOUR_API_KEY"


def checkout(flags: OpenFeatureClient, context: EvaluationContext) -> str:
    result = flags.get_boolean_details("new_checkout", False, context)
    if result.error_code:
        raise RuntimeError(result.error_code)  # Choose how your application handles errors.
    return "Use the new checkout" if result.value else "Use the current checkout"


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
        api.set_provider_and_wait(PartlyProvider(client), "partly")
        flags = api.get_client("partly")
        # Two example requests reuse the client; evaluation is local.
        for user_id in ("user-123", "user-456"):
            print(checkout(flags, EvaluationContext(targeting_key=user_id)))
    except (PartlyError, RuntimeError) as error:
        # Your application chooses how to handle errors; this example stops.
        raise SystemExit(f"Partly: {error}") from None
    finally:
        api.shutdown()
        await client.aclose()  # Application shutdown.


if __name__ == "__main__":
    asyncio.run(main())
