# Partly Python SDK

Reuse one client per application process and event loop. HTTPX handles async downloads; a PyO3 extension evaluates flags in Rust. Evaluation stays local.

The package includes type hints for mypy and editors, including the native evaluator and its errors.

## Build locally

From the repository root, create a CPython 3.14 virtual environment and install the SDK:

```sh
python3.14 -m venv .venv
source .venv/bin/activate
python -m pip install 'maturin>=1.9.4,<2'
maturin build --release --locked --manifest-path sdk/python/Cargo.toml --out target/wheels -i python3.14
python -m pip install target/wheels/partly_sdk-0.1.0a1-cp314-cp314-*.whl
```

Supports regular CPython 3.14 only. Source builds also require Rust and a C compiler; a matching prebuilt wheel needs no Rust compiler. Free-threaded Python is not supported. Public downloads are not available yet; see [release targets and status](../../docs/releasing.md#distribution-targets).

## Use OpenFeature

Start with the [complete quickstart](../../examples/python/quickstart.py), also shown in the dashboard. Install `openfeature-sdk>=0.10,<0.11` alongside the local wheel, or use `partly-sdk[openfeature]` after publication.

Await native `init()` once at startup, then register `PartlyProvider`. Keep the native and OpenFeature clients in application state. Handlers use OpenFeature's detailed Boolean getter and check its error before using the value. Polling and manual refresh update the same policy; close both clients at shutdown. See [context mapping, errors, and tested limits](../../docs/openfeature.md).

## Native client reference

The native API remains available without OpenFeature:

| Method | Behavior |
| --- | --- |
| `build()` | Validates configuration without network calls |
| `await init()` | Downloads and validates the first policy, then starts one background poller; failures raise and can be retried |
| `evaluate(key, context=...)` | Evaluates the accepted policy locally |
| `await refresh()` | Returns `True` for an accepted full policy, `False` for an unchanged response; ordinary errors preserve the policy and ETag; confirmed revocation clears them |
| `await aclose()` | Stops polling and closes connections; safe to repeat, including after failed initialization |

Defaults: poll 30 seconds after each refresh; timeout 5 seconds for the entire download; maximum policy size 1 MiB. Change timing with builder `.poll_interval(seconds)` and `.timeout(seconds)`, using finite positive values. Validation runs outside the application event loop. After initialization, policy updates use background polling or `refresh()`.

Background errors are logged under `partly` and exposed in `client.last_refresh_error`; a successful refresh clears it. Running clients retain their policy during outages, but a new process needs a successful initial download. A closed client can still evaluate its last policy, but cannot reconnect or accept updates.

## Flag usage reporting

Native evaluation records distinct known flag names, including disabled decisions and context errors. An evaluation starts a background report when 128 names are pending or 60 seconds have passed since initialization or the previous report. Reports contain only flag names; context values and decisions stay local. `has_flag()`, unknown flags, and OpenFeature checks that fail before native evaluation do not count.

One report runs at a time, with up to 128 names. Failed reports retry only after another evaluation and a 60-second cooldown; they do not change decisions or `last_refresh_error`. Accepted policies remove pending names that are no longer assigned. Confirmed key revocation stops reporting and clears the policy. Closing cancels reporting and discards pending names; cached evaluation remains available without recording more usage. Usage is best effort, so short-lived or idle clients may leave names unreported.

## HTTPS and private certificate authorities

HTTPS verifies certificates and hostnames using certifi's trust bundle. The SDK ignores proxy and certificate environment variables, rejects redirects, and never falls back to HTTP.

For a private certificate authority (CA), supply its trusted certificates as a PEM bundle:

```python
from partly import Client

client = (
    Client.builder()
    .url("https://partly.example.com:8443")
    .environment("development")
    .application_id("YOUR_APPLICATION_ID")
    .secret_key("YOUR_API_KEY")
    .ca_bundle("/path/to/company-ca.pem")
    .build()
)
```

The path accepts a string or `Path`. This bundle replaces the default trust roots; certificate and hostname checks remain required.

`init()` reads the file asynchronously. An unreadable or invalid bundle, or failed certificate verification, raises `RefreshError`. Correct the configuration and retry initialization, or close the client.

## Native decisions and errors

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

## Native context and custom rules

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
python -m pytest -q sdk/python/tests
cargo test --locked -p partly-e2e --test e2e python
cargo test --locked -p partly-e2e --test e2e api::https::
```

Tests exercise both the native boundary and the real control plane. See [development checks](../../docs/releasing.md#build-and-verify) for browser and full-workspace checks.

The [shared SDK contract](../../tests/sdk_contract/README.md) runs the same end-to-end scenarios for each language.

## License

The SDK and evaluator use [Apache-2.0](LICENSE), including use in proprietary applications. Preserve the packaged license and notices; dependencies keep their own terms. The server's ELv2 restrictions do not apply to the SDK.

## Application scope and revocation

Create an application in **Apps & Keys** and select the flags it can use. Supply its environment and permanent application ID on the builder with `.environment("development").application_id("YOUR_APPLICATION_ID")`, along with its API Key. All are required; the key must match both scope values. Even the first download is checked against this scope. Removing a selected flag makes it unknown after refresh.

A confirmed revoked-key response clears policy and ETag, stops polling, and makes evaluation, initialization, and refresh return `KeyRevokedError` (an `AuthenticationError` subclass). Build a new client with a valid replacement key. Closing a revoked client does not restore evaluation. Ordinary authentication, network, and validation failures preserve the accepted policy. Offline clients learn about revocation only when a refresh or usage report reaches the server.
