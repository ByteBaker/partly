# Partly core

Shared policy validation and flag evaluation for the server and all language SDKs. No HTTP, storage, or Python dependencies.

## Policy format

```json
{
  "schema_version": 1,
  "application_id": "app_example",
  "project": "default",
  "environment": "development",
  "revision": 1,
  "flags": {
    "new_checkout": {"mode": "percentage", "percentage": 25},
    "search": {"mode": "enabled"},
    "banner": {"mode": "disabled"},
    "pro_checkout": {
      "mode": "custom",
      "action": "enable",
      "groups": [[
        {"k": "plan", "op": "matches", "v": "pro"},
        {"k": "age", "op": "ge", "v": 18}
      ]]
    }
  }
}
```

Flag, project, environment, and application keys use 1–64 lowercase ASCII letters, digits, `_`, or `-`. Percentages are integers from 0 to 100. Unknown fields, wrong types, and unsupported schema versions are rejected. Empty `flags` is valid. Policies are limited to 1 MiB (`MAX_POLICY_BYTES`).

`Policy::from_json()` parses and validates a policy once. For standalone flag JSON, also run `validate_json_number_limits()` on the original bytes to prevent rounded integer literals. `evaluate(flag_key, &context)` returns `Result<Decision, EvaluationError>`.

Context is a flat map of string keys to strings, numbers, or booleans. Nulls, arrays, and nested objects are rejected. Percentage mode requires a nonblank `id` of 1–256 UTF-8 bytes; other modes only need their rule fields.

## Custom rules

All conditions within a group must match; any group may match. Groups and rulesets cannot be empty. `action: "enable"` enables matches and disables non-matches; `"disable"` reverses this. Missing or wrongly typed required fields always return errors.

| Operators | Type | Comparison |
| --- | --- | --- |
| `le`, `lt`, `ge`, `gt`, `eq` | Number | ≤, <, ≥, >, = |
| `prefix`, `suffix`, `contains` | String | Starts with, ends with, contains |
| `matches` | String | Exact equality |
| `regex` | String | Pattern search; add `^` and `$` to match the whole string |
| `is` | Boolean | Equals literal true or false |

Strings are case-sensitive, with no trimming, normalization, or type conversion. Numeric `18` equals `18.0`; `"18"` and `true` are not numbers. Integers retain precision, including beyond 2^53, and must fit between −2^63 and 2^64−1. Out-of-range literals are rejected. Floats must be finite.

Rust regex patterns compile when accepting the policy. Invalid or oversized patterns, look-around, backreferences, and inline case-insensitive flags are rejected.

All required fields in the selected flag are checked before matching, even if another group matches. Errors identify the flag and key, plus operator and types for type errors, without context values. Other flags' fields are not required.

## Stable percentage selection

The version 1 hash input is:

1. ASCII bytes `partly:v1:`.
2. Flag-key byte length as a four-byte unsigned big-endian integer, then the UTF-8 key bytes.
3. Context-ID byte length encoded the same way, then its UTF-8 bytes.

Compute SHA-256. Read the first four digest bytes as an unsigned big-endian integer, then take the remainder after division by 10,000. Percentage mode selects the subject when this bucket is less than `percentage * 100`.

Flag keys are immutable. The same flag key and ID use the same bucket across environments. Percentage and revision are excluded from the hash: increasing rollout keeps selected users, and adding users leaves existing decisions unchanged.

Fixed vectors:

| Flag key | Context ID | Bucket |
| --- | --- | --- |
| `checkout` | `user-42` | 9426 |
| `new_checkout` | `user-123` | 4195 |
| `checkout` | `用户-1` | 8114 |

Percentages describe hash buckets, not exact user counts. Small samples may differ. No user list is stored.

```sh
cargo test -p partly-core
```

## License

[Apache-2.0](LICENSE), independent of the server's ELv2 license. Preserve the license and attribution notices when redistributing.

Policy `application_id` identifies the selected application. SDKs verify it and the environment against their required builder settings on every full download, including initialization.
