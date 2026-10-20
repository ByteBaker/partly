# Partly Python SDK

Reuse one client per application process and event loop. HTTPX handles async downloads; a PyO3 extension evaluates flags in Rust. Evaluation stays local.

## Build locally

From the repository root, with a virtual environment active:

```sh
python -m pip install 'maturin>=1.9.4,<2'
maturin build --release --locked --manifest-path sdk/python/Cargo.toml --out target/wheels
python -m pip install target/wheels/partly_sdk-0.1.0a1-*.whl
```

Requires regular CPython 3.10+, Rust, and a C compiler. A matching prebuilt wheel needs no Rust compiler. Public downloads are not available yet; see [release targets and status](../../RELEASING.md#distribution-targets).

## Use a reusable client

Create `new_checkout` and an API Key in the same environment, then replace the constants below:

```python
import asyncio
from partly import Client, Decision

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
        await client.init()
        decision = client.evaluate("new_checkout", context={"id": "user-123"})
        print("Enabled:", decision is Decision.ENABLED)
    finally:
        await client.aclose()

asyncio.run(main())
```

See the [runnable quickstart](../../examples/python/quickstart.py) for error handling.

| Method | Behavior |
| --- | --- |
| `build()` | Validates configuration without network calls |
| `await init()` | Downloads and validates the first policy, then starts one background poller; failures raise and can be retried |
| `evaluate(key, context=...)` | Evaluates the accepted policy locally |
| `await refresh()` | Returns `True` for an accepted full policy, `False` for an unchanged response; errors preserve the policy and ETag |
| `await aclose()` | Stops polling and closes connections; safe to repeat, including after failed initialization |

Defaults: poll 30 seconds after each refresh; timeout 5 seconds for the entire download; maximum policy size 1 MiB. Change timing with builder `.poll_interval(seconds)` and `.timeout(seconds)`, using finite positive values. Validation runs outside the application event loop. Repeated successful `init()` calls do not add pollers.

Background errors are logged under `partly` and exposed in `client.last_refresh_error`; a successful refresh clears it. Running clients retain their policy during outages, but a new process needs a successful initial download. A closed client can still evaluate its last policy, but cannot reconnect or accept updates.

## HTTPS and private certificate authorities

HTTPS verifies certificates and hostnames using certifi's trust bundle. The SDK ignores proxy and certificate environment variables, rejects redirects, and never falls back to HTTP.

For a private certificate authority (CA), supply its trusted certificates as a PEM bundle:

```python
from partly import Client

client = (
    Client.builder()
    .url("https://partly.example.com:8443")
    .secret_key("YOUR_API_KEY")
    .ca_bundle("/path/to/company-ca.pem")
    .build()
)
```

The path accepts a string or `Path`. This bundle replaces the default trust roots; certificate and hostname checks remain required.

`init()` reads the file asynchronously. An unreadable or invalid bundle, or failed certificate verification, raises `RefreshError`. Correct the configuration and retry initialization, or close the client.

## Decisions and errors

| Result or exception | Meaning |
| --- | --- |
| `Decision.ENABLED` | Known flag selects this user |
| `Decision.DISABLED` | Known flag is off or does not select this user |
| `FlagNotFoundError` | Flag is absent from the accepted policy |
| `InvalidContextError` | Context is malformed or a required field is missing or invalid |
| `ContextTypeError` | Context value has the wrong type; subclass of `InvalidContextError` |
| `PolicyNotReadyError` | No policy has been initialized |
| `AuthenticationError` | Control plane rejected the API Key |
| `InvalidPolicyError` | Downloaded data or its ETag is invalid |
| `RefreshError` | Network, timeout, certificate configuration, or other HTTP failure |
| `ClientClosedError` | Network operation attempted after close |

Compare with `decision is Decision.ENABLED`; boolean conversion raises `TypeError`. All listed exceptions inherit `PartlyError`. The SDK never chooses a fallback decision for an error.

## Context and custom rules

Pass a flat dictionary with string keys and string, number, or boolean values. Context stays in your application:

```python
context = {"id": "user-123", "age": 23, "plan": "pro", "verified": True}
decision = client.evaluate("new_checkout", context=context)
```

- Percentage mode requires a stable, nonblank `id` of 1–256 UTF-8 bytes. Adding users does not change existing users' decisions.
- Enabled and disabled modes accept `{}`. Custom rules require every field referenced by that flag, even if one group already matches. Supply new context fields before publishing rules that use them.
- Strings are case-sensitive. Types are never converted: `"18"` is not a number, and `True` is not `1`. Numeric `18` equals `18.0`.
- Integers must fit between −2^63 and 2^64−1; floats must be finite. Nulls, lists, nested dictionaries, and other objects are rejected. Use integers when exact values matter.

Dashboard rules match all conditions within a group and any group within a flag. A match applies the configured enable/disable action; a non-match gets the opposite decision. See [operators and matching rules](../../crates/partly-core/README.md#custom-rules).

`ContextTypeError` provides `flag_key`, `k`, `op`, `expected_type`, and `actual_type`, without context values. Type names include `number`, `string`, and `boolean`. `op` is `None` for percentage IDs or unsupported extra fields. Catching `InvalidContextError` also catches this subclass.

## Tests

Install the wheel before testing. From the repository root:

```sh
python -m pytest -q sdk/python/tests tests/e2e/test_sdk.py tests/e2e/test_https.py
```

Tests exercise both the native boundary and the real control plane. See [development checks](../../RELEASING.md#build-and-verify) for browser and full-workspace checks.

## License

The SDK and evaluator use [Apache-2.0](LICENSE), including use in proprietary applications. Preserve the packaged license and notices; dependencies keep their own terms. The server's ELv2 restrictions do not apply to the SDK.
