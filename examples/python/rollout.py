"""Run against a flag created in the Partly dashboard."""
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
        for number in range(1, 10_001):
            user_id = f"user-{number}"
            decision = client.evaluate(flag, context={"id": user_id})
            message = "new checkout" if decision is Decision.ENABLED else "current checkout"
            print(f"{user_id}: {message}")
        print("Policy updated:", await client.refresh())
    finally:
        await client.aclose()


if __name__ == "__main__":
    asyncio.run(main())
