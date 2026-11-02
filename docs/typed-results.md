# Typed results

Flags can return Booleans, strings, integers, floats, named enums, or named JSON records. Evaluation stays local in every SDK.

## Create a type and flag

1. For enum or record results, create a definition in **Types**. An enum lists string choices; a record defines named fields. Built-in Boolean, string, integer, and float types need no definition. Named types belong to the current environment.
2. Create a flag and choose its result type. For non-Boolean flags, set **Selected** and **Otherwise** values.
3. Choose everyone, nobody, a percentage, or custom rules. The rollout selects which value to return.
4. Assign the flag to an application under **Apps & Keys**. Upgrade that application's SDK before assigning typed flags.

For example, create a `Checkout` record with `layout: string`. Create `checkout_settings` returning `Checkout`, with `{"layout":"wide"}` selected and `{"layout":"compact"}` otherwise. The **Docs → Typed results** example reads this record through OpenFeature.

Managers and instance administrators create and delete types. Viewers and Editors can inspect them; Editors can choose existing types for flags. Type creation and deletion appear in Audits. Flag edits keep the affected-application warning and exact environment/flag confirmation.

Names and definitions are immutable. A flag's result type is also fixed. Create a new type and flag when changing an application's contract. Delete a type only when no flag or record uses it; its name stays reserved in that environment.

## Values and limits

| Type | Accepted value |
| --- | --- |
| Boolean | Existing enabled/disabled decisions; unchanged native API |
| String | Text, including an empty string; at most 4 KiB of UTF-8 |
| Integer | Whole JSON integer, −9,007,199,254,740,991 through +9,007,199,254,740,991 |
| Float | Finite number; a whole value such as `3` is also valid |
| Enum | One of 1–64 unique strings, each 1–256 UTF-8 bytes; case-sensitive |
| Record | 1–32 required fields, each a built-in type or an existing enum |

Type and field names start with an ASCII letter and contain only letters, digits, or underscores, up to 64 bytes. Type names cannot reuse built-in names. Both reject reserved names such as `constructor` and `prototype`. Extra or missing record fields, nulls, arrays, and nested records are rejected. There is no implicit conversion from strings or Booleans to numbers. Definitions validate both result branches before saving and before a downloaded policy is accepted.

The 16 KiB write-request and 1 MiB policy limits still apply. Numeric context and rule comparisons retain their existing 64-bit integer behavior; the smaller integer limit applies to **results** so every supported language can represent them exactly.

## OpenFeature and Partly enhancements

| Capability | OpenFeature with Partly provider | Native Partly SDK |
| --- | --- | --- |
| Primitive results | Standard typed getters | Typed evaluation methods |
| Enum / record | String / object getter | String / object getter |
| Validate values against the configured schema | ✅ | ✅ |
| Inspect named type | **Partly metadata through OpenFeature:** `partly.result_type` | `resolve` result's named type |
| Require an exact named type | No standard argument; inspect metadata yourself | **Partly SDK enhancement:** named evaluation |
| No caller fallback, structured native errors | Standard error/default behavior applies | **Partly SDK enhancement** |
| Refresh, refresh health, shutdown | Use the underlying native client | **Partly SDK enhancement** |

The standard metadata container carries a Partly-specific name. A returned record contains only its configured fields, without a Partly wrapper. Schema validation, targeting, RBAC, audits, application isolation, and usage reporting apply through either interface.

Use detailed OpenFeature getters and check errors before accepting the returned value. Python, Go, and Node may return the caller's default on failure; that default is **not validated against the Partly schema**. Rust's pinned OpenFeature SDK returns `Result` and has [documented upstream differences](openfeature.md#coverage-and-limits). This feature does not establish full specification conformance.

### Native methods

All methods reuse the initialized native client already borrowed by the OpenFeature provider. No extra initialization or cache is needed. These are optional direct SDK calls:

| Language | String / integer / float / object | Exact named type |
| --- | --- | --- |
| Python | `evaluate_string`, `evaluate_integer`, `evaluate_float`, `evaluate_object` | `client.evaluate_named("checkout_settings", "Checkout", context=context)` |
| Rust | `evaluate_string`, `evaluate_integer`, `evaluate_float`, `evaluate_object` | `client.evaluate_named("checkout_settings", &context, "Checkout")?` |
| Go | `EvaluateString`, `EvaluateInteger`, `EvaluateFloat`, `EvaluateObject` | `client.EvaluateNamed("checkout_settings", context, "Checkout")` |
| Node.js | `evaluateString`, `evaluateInteger`, `evaluateFloat`, `evaluateObject` | `client.evaluateNamed("checkout_settings", "Checkout", context)` |

`resolve` (`Resolve` in Go) returns the value, base kind, variant, and optional named type. Built-in kinds are `boolean`, `string`, `integer`, `float`, and `object`; `named` requires an exact expected name. Node's OpenFeature provider uses `number`, which accepts integer and float flags. Native integer and float getters remain distinct. Enums return strings and records return owned dictionaries/maps/objects. They do not generate application classes.

Wrong getters raise a result-type error; wrong expected names raise a dedicated named-type error. Python and Node use `ResultTypeError` and its `NamedTypeError` subclass. Go uses `ErrResultType` / `ErrNamedType`; Rust uses `EvaluationError::ResultType` / `NamedType`. Errors carry `flag_key`, `expected_type`, and `actual_type` (Go uses exported field names), never result or context values. Providers map these failures to `TYPE_MISMATCH`. Unknown flags, invalid context, readiness, and revocation retain their separate errors.

## Policy compatibility

Applications selecting only Boolean flags still receive the unchanged v1 policy shape. Applications selecting typed flags receive v2, including only their referenced named types and enum dependencies. New SDKs accept both formats. Unrelated types do not change an application's ETag.

Older SDKs reject v2. Upgrade before assigning typed flags: an old client fails first initialization or keeps its last accepted policy on a failed refresh. Never treat a refresh failure as proof that the new policy is active.

Refresh replaces policy and ETag only after complete validation. Returned records cannot mutate the cached policy. Ordinary failures retain it; confirmed key revocation clears it. Usage reports contain only distinct known flag names, including failed type checks, never result values or context.

SQLite remains at development schema v1, with no migration. Older incompatible development databases need a fresh path; Partly rejects and preserves the existing file. Back it up before changing versions.

Arrays, optional fields, nested records, generated application models, and weighted multivariate experiments are outside this scope.
