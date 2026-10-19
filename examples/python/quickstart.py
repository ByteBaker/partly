import asyncio
import os
import sys

from partly import (
    AuthenticationError,
    Client,
    ContextTypeError,
    Decision,
    FlagNotFoundError,
    InvalidContextError,
    InvalidPolicyError,
    PartlyError,
    PolicyNotReadyError,
    RefreshError,
)


async def main():
    try:
        # Build once, then reuse this client in your application's event loop.
        client = (
            Client.builder()
            .url(os.environ.get("PARTLY_URL", "http://127.0.0.1:8080"))
            .secret_key(os.environ["PARTLY_SDK_KEY"])
            .build()
        )
    except (KeyError, ValueError):
        raise SystemExit("Set PARTLY_SDK_KEY and a valid PARTLY_URL before starting.") from None

    try:
        try:
            await client.init()  # Downloads policy and starts background polling.
        except AuthenticationError:
            raise SystemExit("Initialization failed: check your SDK key and environment.") from None
        except (RefreshError, InvalidPolicyError):
            raise SystemExit("Initialization failed: no usable policy could be loaded.") from None

        try:
            # Optional: request an update now; otherwise polling runs every 30 seconds.
            changed = await client.refresh()
            print("Policy updated:", changed)  # False means the policy is unchanged.
        except PartlyError:
            print("Refresh failed; keeping the last accepted policy.", file=sys.stderr)

        try:
            # Context stays local. Keep id stable for percentage rollouts.
            decision = client.evaluate(
                os.environ.get("PARTLY_FLAG", "new_checkout"),
                context={
                    "id": os.environ.get("PARTLY_USER_ID", "user-123"),
                    "age": 23,
                    "plan": "pro",
                    "verified": True,
                },
            )
        except FlagNotFoundError:
            raise SystemExit("Unknown flag: check its name and the key's environment.") from None
        except ContextTypeError as error:
            raise SystemExit(
                f"Cannot evaluate: context key '{error.k}' expects {error.expected_type}, "
                f"received {error.actual_type}."
            ) from None
        except (InvalidContextError, PolicyNotReadyError):
            raise SystemExit("Cannot evaluate: check required context and initialization.") from None

        # This example stops on evaluation errors. Your application chooses its response.
        if decision is Decision.ENABLED:
            print("Use the new checkout")
        elif decision is Decision.DISABLED:
            print("Use the current checkout")
    finally:
        await client.aclose()  # Stop polling and close network connections.


if __name__ == "__main__":
    asyncio.run(main())
