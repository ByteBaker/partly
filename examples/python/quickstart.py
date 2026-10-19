import asyncio

from partly import Client, Decision, PartlyError

CONTROL_PLANE_URL = "http://127.0.0.1:8080"
API_KEY = "YOUR_API_KEY"


async def main():
    client = (
        Client.builder()
        .url(CONTROL_PLANE_URL)
        .secret_key(API_KEY)
        .build()
    )
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
