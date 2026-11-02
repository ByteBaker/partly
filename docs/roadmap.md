# Roadmap

## Available

- ✅ One self-hosted Rust executable, SQLite, and the bundled dashboard.
- ✅ Boolean, string, integer, float, named enum, and flat JSON record results.
- ✅ Everyone, percentage, custom-rule, and nobody rollouts; local evaluation in all SDKs.
- ✅ Python, Rust, Go, and Node.js SDKs with Partly providers for the official OpenFeature SDKs.
- ✅ Standard typed getters, named-type metadata, and optional native exact-type assertions. See [typed results](typed-results.md).
- ✅ Environment-scoped shared flags, app assignments, API Key rotation/revocation, and usage reports.
- ✅ Individual accounts, RBAC, transactional audits, HTTPS, backup/restore, and operational logging.
- ✅ OpenAPI contract generated from Rust and checked against real responses.

## Finish the alpha release

- ⬜ Pass the full build, vulnerability, packaged-consumer, and release workflows on the final commit.
- ⬜ Publish verified binaries and SDK archives to [GitHub Releases](https://github.com/ByteBaker/partly/releases).
- ⬜ Complete registry distribution and verify first use on clean machines. See [publishing](publishing.md).

## Evaluate next

- ⬜ Git integration: start with reviewed export/import and clear ownership of changes. Automatic two-way synchronization needs a separate decision.
- ⬜ Flag lifecycle: expiry reminders and lightweight ownership. Usage reports cannot prove a flag is unused; never delete automatically.
- ⬜ Review newer OpenFeature baselines and upstream Rust gaps. Do not claim full conformance. [Coverage and limits](openfeature.md#coverage-and-limits).
- ⬜ Protect stored API Key secrets before production use; document recovery and rotation.

## Outside the current scope

- ❌ HA, Redis, required external databases, SSO, billing, or premium activation.
- ❌ Browser/mobile SDKs or additional server languages before demand justifies them.
- ❌ Arrays, optional fields, nested records, generated application classes, arbitrary rule code, or weighted experiments.
- ❌ Reusable audiences, stored user profiles, per-evaluation analytics, and automatic flag removal.

The typed-result design and limits are recorded in [the implementation guide](typed-results.md). Boolean-only applications retain policy v1; typed applications use v2. New SDKs accept both. Upgrade before assigning typed flags. SQLite remains development schema v1: incompatible databases are rejected and preserved, never reset automatically.
