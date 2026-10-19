# Partly core

This crate validates policy documents and evaluates flags. It has no HTTP, SQLite, or Python dependency. The server and the Python native extension use the same model.

## Policy format

```json
{
  "schema_version": 1,
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

Flag, project, and environment keys contain 1–64 lowercase ASCII letters, digits, underscores, or hyphens. Percentages are whole numbers from 0 to 100. Unknown document fields, wrong types, and unsupported schema versions are rejected. An empty `flags` object is valid. Policy documents are limited to 1 MiB, shared with the server and SDK through `MAX_POLICY_BYTES`.

Use `Policy::from_json()` to parse and validate downloaded bytes once. For direct flag JSON, use `validate_json_number_limits()` on the original bytes as well as normal deserialization; this prevents large integer literals from being silently rounded. `evaluate(flag_key, &context)` returns `Result<Decision, EvaluationError>`. Context is a flat map of plain string keys to strings, numbers, or booleans. Nulls, arrays, and nested objects are not supported. Percentage mode requires `context["id"]`: a nonblank string of 1–256 UTF-8 bytes. Other modes only require their rule fields.

## Custom rules

All conditions inside a group must match; any group may match. Empty groups and empty rulesets are invalid. With `action: "enable"`, a match enables the feature and a non-match disables it. `action: "disable"` reverses those decisions. Missing or wrongly typed required fields are errors for either action, never implicit decisions.

| Operators | Type | Comparison |
| --- | --- | --- |
| `le`, `lt`, `ge`, `gt`, `eq` | Number | ≤, <, ≥, >, = |
| `prefix`, `suffix`, `contains` | String | Starts with, ends with, contains |
| `matches` | String | Exact equality |
| `regex` | String | Pattern search; add `^` and `$` to match the whole string |
| `is` | Boolean | Equals literal true or false |

Strings are case-sensitive, without trimming or normalization. Types are never converted. Numeric `18` equals `18.0`; string `"18"` and boolean `true` are not numbers. Integer comparisons preserve precision, including values beyond 2^53. Integer rule values and context values must be between −2^63 and 2^64−1; larger integer literals are rejected instead of rounded. Floating-point values must be finite.

Regex patterns use Rust's `regex` syntax. Look-around, backreferences, and inline case-insensitive flags are rejected. Patterns compile when accepting the policy, not on each evaluation. Invalid or overly large patterns cannot be accepted.

Every required field in the selected flag is checked before matching, even if another group would already match. Errors identify the flag and key, without including context values. Wrong types also identify the operator, expected type, and actual type. Conditions in other flags do not require their context fields.

## Stable percentage selection

The version 1 hash input is:

1. ASCII bytes `partly:v1:`.
2. Flag-key byte length as a four-byte unsigned big-endian integer, then the UTF-8 key bytes.
3. Context-ID byte length encoded the same way, then its UTF-8 bytes.

Compute SHA-256. Read the first four digest bytes as an unsigned big-endian integer, then take the remainder after division by 10,000. Percentage mode selects the subject when this bucket is less than `percentage * 100`.

The flag key is immutable in the dashboard. Identical flag keys and context IDs use the same bucket across environments. Neither rollout percentage nor policy revision enters the hash, so raising a percentage keeps previously selected users. Adding more users does not change existing decisions.

Fixed vectors:

| Flag key | Context ID | Bucket |
| --- | --- | --- |
| `checkout` | `user-42` | 9426 |
| `new_checkout` | `user-123` | 4195 |
| `checkout` | `用户-1` | 8114 |

Percentages describe a share of possible hash buckets. Small samples do not always match the percentage exactly. No user list or profile is stored.

```sh
cargo test -p partly-core
```
