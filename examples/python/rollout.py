"""Check a larger population against a flag created in the Partly dashboard."""
import asyncio
import os

from partly import Client, Decision


async def main():
    client = (
        Client.builder()
        .url(os.environ.get("PARTLY_URL", "http://127.0.0.1:8080"))
        .secret_key(os.environ["PARTLY_SDK_KEY"])
        .build()
    )
    try:
        await client.init()
        flag = os.environ.get("PARTLY_FLAG", "new_checkout")
        users = 10_000
        enabled = sum(
            client.evaluate(flag, context={"id": f"user-{number}"}) is Decision.ENABLED
            for number in range(1, users + 1)
        )
        print(f"Enabled for {enabled:,} of {users:,} users ({enabled / users:.1%}).")
        print("Policy updated:", await client.refresh())
    finally:
        await client.aclose()


if __name__ == "__main__":
    asyncio.run(main())
