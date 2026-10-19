import asyncio
import os

from partly import Client, Decision, PartlyError


async def main():
    try:
        # Build once and reuse this client in your application's event loop.
        client = (
            Client.builder()
            .url(os.environ.get("PARTLY_URL", "http://127.0.0.1:8080"))
            .secret_key(os.environ["PARTLY_SDK_KEY"])
            .build()
        )
    except (KeyError, ValueError):
        raise SystemExit("Set PARTLY_SDK_KEY and a valid PARTLY_URL before starting.") from None

    try:
        await client.init()  # Downloads policy and starts background polling.
        decision = client.evaluate("new_checkout", context={"id": "user-123"})
        if decision is Decision.ENABLED:
            print("Use the new checkout")
        else:
            print("Use the current checkout")
    except PartlyError as error:
        # Your application chooses how to handle errors; this example stops.
        raise SystemExit(f"Partly: {error}") from None
    finally:
        await client.aclose()


if __name__ == "__main__":
    asyncio.run(main())
