# Roadmap

Current evaluation results are Boolean. The items below are planned, not available APIs.

## Release the current alpha

- ✅ Local evaluation, four SDKs, OpenFeature providers, permissions, audits, and usage reporting.
- ⬜ Resolve dependency advisories and pass the full build and release workflows on the final commit.
- ⬜ Publish verified binaries and SDK archives through [GitHub Releases](https://github.com/ByteBaker/partly/releases).
- ⬜ Complete registry distribution and test first use on clean machines. See [publishing](publishing.md).

## Typed evaluation results

Support built-in non-Boolean results **and named custom result types defined in the dashboard**. A custom type defines the structure of a JSON evaluation result: field names and their allowed types. Both configured values and SDK results stay JSON-compatible. It is not an extra payload attached to a Boolean decision, and it does not generate language classes. OpenFeature's [value model](https://openfeature.dev/specification/types/) supplies the baseline for Boolean, string, number, and structured results.

### Proposed first scope

| Result type | Example | Planned behavior |
| --- | --- | --- |
| Boolean | `true` | Preserve the existing API and decisions |
| String | `"compact"` | Return text without conversions |
| Integer | `3` | Return an exact whole number |
| Float | `0.25` | Return a finite floating-point number |
| Named enum | `CheckoutLayout`: `compact`, `expanded` | Return one of the dashboard-defined string choices |
| Named record | `CheckoutSettings`: `layout`, `retry_limit`, `show_help` | Return fields that satisfy the dashboard-defined type |

Start with required record fields using built-in types or named enums. Reject extra fields and wrong types. No recursive types, record inheritance, arbitrary validation code, nullable fields, or schema language in the first implementation. This keeps both authoring and error messages understandable.

Named types belong to one environment and can be reused by that environment's flags. The dashboard needs a **Types** screen to create and inspect enums and records, show where they are used, and manage unused types. Managers and administrators manage types; Editors select an existing type when creating flags. All backend handlers enforce the same permissions.

Keep type names and definitions immutable once created. A changed contract gets a new named type; existing flag types cannot change in place. Delete a type only when no flag or other type references it. Show references and a confirmation before deletion, and audit type creation/deletion. This avoids a schema-version registry and prevents silent changes to application contracts.

### Selection and values

Keep the existing percentage hashing and custom-rule matching. Selection chooses between **two configured results of the same type**:

- Everyone receives the selected result.
- Nobody receives the other result.
- Percentage selects the existing stable cohort.
- Custom rules select the matching result, with the other result for nonmatches.

For Boolean flags these remain true/false, including the current custom `enable`/`disable` action. For other types, show **Everyone / Percentage / Custom rules / Nobody receives this value**, with **Selected value** and **Other value** editors. Do not call a string or record “enabled.” No weighted multi-variant experimentation in this step.

Example: define `CheckoutSettings` with `layout: CheckoutLayout`, `retry_limit: Integer`, and `show_help: Boolean`. Create a flag returning that type. Select `{"layout":"expanded","retry_limit":3,"show_help":true}` for 25% and `{"layout":"compact","retry_limit":1,"show_help":false}` for the rest. The evaluation result is JSON matching `CheckoutSettings`, not a Boolean plus a separate body.

Both results must validate before saving. Require a result for each branch; do not invent a fallback when a required value is missing. Start new integer results at JavaScript's exact range, ±9,007,199,254,740,991, and reject non-finite floats. Apply these limits within records too. Existing numeric context/rule behavior remains unchanged. Keep request/policy byte limits and define explicit field-count, string-length, and type-name limits before implementation.

### SDK and OpenFeature behavior

- Put value types, schema validation, and selection in `partly-core`; all SDKs use the same implementation.
- Add typed native getters without changing the current Boolean `evaluate` return type. Named results include their type name and validated value, with a caller-supplied expected type name. A mismatch is a dedicated error, separate from a context-type error.
- Return idiomatic data: Rust values, Python dictionaries/enums-as-strings, Go values, and Node objects. Dashboard-defined types cannot create compiled application structs automatically. Code generation and application model conversion are separate, deferred conveniences.
- Map enums to OpenFeature string getters and records to structure/object getters. Preserve the named type in resolution metadata. OpenFeature's standard interface cannot enforce an application-specific struct or an expected Partly type name; document that boundary. Check the pinned SDKs' integer/float mappings explicitly, especially Node's single number getter.
- Keep native errors explicit. Wrong getters report result-type mismatch, unknown flags remain unknown, and missing required context remains an error. OpenFeature retains its caller-default and error-detail behavior.
- Parse and validate complete snapshots before swapping the cache and ETag. Return owned or immutable record values so callers cannot mutate the accepted policy. No evaluation-time HTTP, per-result cache, or new service.
- Continue reporting known flag attempts, including type errors, through the existing usage batching. Never send the returned values, type field values, or context in usage reports or operational logs.

The pinned Rust OpenFeature SDK exposes a top-level structure but no null value; the initial record/enum scope avoids depending on unsupported representations. These additions do not remove its other documented [upstream limitations](openfeature.md#coverage-and-limits).

### Compatibility and delivery order

1. ⬜ Specify types, errors, size bounds, serialized fixtures, and the cross-language contract before exposing UI controls.
2. ⬜ Extend the Rust core and add focused selection, validation, and numeric-boundary tests. Keep every existing Boolean hash vector unchanged.
3. ⬜ Add Python, Rust, Go, then Node bindings and OpenFeature resolution. Test real providers and native clients against the same fixtures.
4. ⬜ Add environment-scoped type storage/APIs, permissions, audits, and typed flag validation. Download only types referenced by an app's selected flags, including referenced enums.
5. ⬜ Add dashboard type forms, typed result editors, readable audit states, impact confirmation, and complete examples. Refresh the README screenshots when the UI changes.
6. ⬜ Run installed-package, API, SDK, browser, restart, backup/restore, outage, and revocation tests before release.

Old SDKs reject unknown policy fields. Do not silently extend a v1 response or turn a typed flag into a Boolean for an older client. Plan a new **policy format version** for snapshots containing typed flags, while continuing to serve the exact v1 format for apps selecting only existing Boolean flags. New SDKs accept both. Upgrade an application's SDK before assigning typed flags; show this requirement in the assignment dialog. Test mixed-version clients, failed first initialization, and rejected refreshes retaining their last accepted snapshot.

The SQLite schema stays v1 during this unreleased alpha; a policy-format version is separate from a database version. Do not reset existing databases. If typed storage lands after a public v1 release, add a tested data-preserving database upgrade as part of that work. Before release, any incompatible development schema remains explicitly rejected without modification.

Acceptance requires no silent conversion or defaults, exact named-type validation, stable rollout membership, no secret/result values in logs, bounded policy sizes, app isolation, unchanged revocation/refresh behavior, and identical results across all four SDKs. Include Unicode, numeric boundaries, invalid records/enums, missing type definitions, unsafe returned-object mutation, and readable before/after type/value changes in the tests.

## Still outside this plan

Git synchronization, approvals, lifecycle automation, HA, SSO, more SDK languages, generated model classes, and experimentation analytics remain uncommitted. Typed evaluation does not require them.
