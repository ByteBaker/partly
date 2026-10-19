# Partly Python SDK

Use one client per application process and event loop. Python handles async HTTP; a small PyO3 extension holds and evaluates the policy in Rust. Evaluation makes no network or database call.

## Build locally

From the repository root, with a virtual environment active:

```sh
python -m pip install 'maturin>=1.9.4,<2'
maturin build --release --locked --manifest-path sdks/python/Cargo.toml --out target/wheels
python -m pip install target/wheels/partly_sdk-*.whl
```

Building needs Rust and a C compiler. Installing a matching wheel does not need Rust. The wheel targets regular CPython 3.10+ through ABI3 and the platform where it was built. The full local test environment is Python 3.12 on macOS ARM64, and the focused SDK suite also passes on Python 3.14. Python 3.10/3.11, free-threaded builds, and other platforms are not verified.

## Use a reusable client

```python
import asyncio
import os
from partly import Client, ContextTypeError, Decision, FlagNotFoundError

async def main():
    client = (
        Client.builder()
        .url("http://127.0.0.1:8080")
        .secret_key(os.environ["PARTLY_SDK_KEY"])
        .poll_interval(30)
        .timeout(5)
        .build()
    )
    try:
        await client.init()
        try:
            decision = client.evaluate("new_checkout", context={"id": "user-123", "age": 23})
        except ContextTypeError as error:
            print(error.k, error.expected_type, error.actual_type)
            raise  # Report the error; the SDK chooses no default decision.
        except FlagNotFoundError:
            raise  # Choose and report your application's response here.
        if decision is Decision.ENABLED:
            print("Use the new checkout")
        print("Policy updated:", await client.refresh())
    finally:
        await client.aclose()

asyncio.run(main())
```

`build()` validates configuration and does no network work. `init()` downloads and validates the first policy before starting a background poller. A repeated successful `init()` returns without starting another poller. Failed initialization can be retried unless the client has been closed.

The default poll interval is 30 seconds after the previous refresh completes. The request timeout is 5 seconds, including the full response download. Both settings must be finite positive values. Policy documents are limited to 1 MiB. The client rejects redirects and ignores proxy environment variables.

`refresh()` returns `True` after accepting a full policy and `False` for an unchanged response. A failed download raises and keeps the previous policy and ETag. Background failures are logged under `partly` and exposed as `client.last_refresh_error`; a successful refresh clears that value.

`aclose()` stops polling and closes HTTP connections. Use it even after failed initialization. It is safe to call again. A closed client cannot reconnect, but its accepted policy remains available for local checks. Create a new client to reconnect.

## Decisions and errors

| Result or exception | Meaning |
| --- | --- |
| `Decision.ENABLED` | Known flag selects this user |
| `Decision.DISABLED` | Known flag is off or does not select this user |
| `FlagNotFoundError` | Flag is absent from the accepted policy |
| `InvalidContextError` | Context is malformed or a required field is missing or invalid |
| `ContextTypeError` | Context value has the wrong type; subclass of `InvalidContextError` |
| `PolicyNotReadyError` | No policy has been initialized |
| `AuthenticationError` | Control plane rejected the SDK key |
| `InvalidPolicyError` | Downloaded data or its ETag is invalid |
| `RefreshError` | Network, timeout, or other HTTP failure |
| `ClientClosedError` | Network operation attempted after close |

These exceptions inherit `PartlyError`. Compare decisions explicitly: `decision is Decision.ENABLED`. Testing a decision as a boolean raises `TypeError`, which prevents treating a disabled enum as truthy by accident. No fallback is chosen for the application.

Keep `context["id"]` stable between requests. The [core README](../../crates/partly-core/README.md) defines the hash. The SDK stores rules in memory, not evaluation results or user profiles. A control-plane outage does not stop already initialized evaluation. A new application process cannot initialize during that outage.

## Context and custom rules

Pass one flat `context` dictionary. Keys are strings; values are strings, numbers, or booleans. Context describes a user, account, device, or another subject and never leaves the application. For percentage mode, `id` must be a nonblank string of 1–256 UTF-8 bytes. Enabled and disabled modes accept an empty dictionary; custom mode only needs its configured keys.

```python
context = {"id": "user-123", "age": 23, "plan": "pro", "verified": True}
decision = client.evaluate("new_checkout", context=context)
```

Custom rules are defined in the dashboard. All conditions in a group must match, and any group can match. The flag's match action enables or disables matching subjects; non-matching subjects receive the opposite decision. All referenced fields are validated before matching. Supply new context fields in your application before publishing rules that require them.

Number operators are `le`, `lt`, `ge`, `gt`, and `eq`. String operators are `prefix`, `suffix`, `contains`, `matches` (exact equality), and `regex`. Boolean comparisons use `is`. All strings are case-sensitive. Regex searches for a pattern; anchors `^` and `$` restrict it to the whole string. Rust regex does not support look-around or backreferences; case-insensitive inline flags are not allowed.

No type conversion occurs: `"18"` is not numeric, and `True` is not `1`. Integers and floats are numbers; `18` equals `18.0`. Python integers must be between −2^63 and 2^64−1, and floats must be finite. Numbers outside those limits, nulls, arrays, nested dictionaries, and other Python objects raise context errors. Keep integers as integers when exact values matter; Python floats already have limited precision.

`ContextTypeError` exposes `flag_key`, `k`, `op`, `expected_type`, and `actual_type`. Types use logical names such as `number`, `string`, and `boolean`. `op` is `None` for percentage IDs or unsupported extra context fields. Messages do not include context values. Existing `except InvalidContextError` handlers also catch this subclass.

## Package layout and tests

`python/partly` contains the public API and async transport. `src/lib.rs` exposes the shared Rust core as `partly._native`. Maturin packages both in one wheel. The binding uses `abi3-py310`; it deliberately does not enable the deprecated PyO3 `extension-module` Cargo feature, so normal workspace tests can link Python correctly.

Install the wheel before testing. From the repository root:

```sh
python -m pytest -q sdks/python/tests tests/e2e/test_sdk.py
```

Tests exercise both the native boundary and the real control plane. The root README includes the browser and full-workspace checks.
