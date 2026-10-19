# Working on Partly

Read this file and the relevant package READMEs before changing the project. They record the supported POC behavior. Keep them current when decisions change. Local proposals, design studies, test output, screenshots, databases, and caches are not tracked in Git.

## Scope and implementation

- Prepare the smallest complete self-hosted alpha. Avoid speculative abstractions, extra services, and features added only for future customers.
- Control plane: Rust, Axum, embedded SQLite, and a vanilla HTML/CSS/JavaScript dashboard bundled into one executable. No required Docker, external database, or frontend runtime.
- SDK: Python only for now, with shared Rust evaluation through PyO3 and Maturin. No browser SDK or frontend application example.
- Flags return boolean decisions through four rollout modes: everyone, percentage, custom rules, or nobody. Evaluate one flat `context` dictionary; percentage selection uses its stable `id`. Custom rules use plain context keys, with AND within groups and OR between groups. Keep rules inside each flag for now; no reusable audiences, stored profiles, variants, or evaluation-result LRU.
- Keep the Rust evaluator independent of HTTP, storage, and language bindings.
- Release preparation is authorized: versioning, package metadata, binaries, wheels, checks, and operational basics. The PyPI name is `partly-sdk`; `partly.sh` URLs are placeholders until deployed. The user selected ELv2 for the control plane/dashboard and Apache-2.0 for the SDK, shared evaluator, and Python integration examples; see LICENSING.md. Describe the server as source-available and the SDK as open source. Preserve these boundaries and include the relevant license texts and notices in release artifacts. The public repository remains undecided; do not publish without a confirmed destination. Enterprise features, HA, billing, and premium activation are deferred. SSE is optional.

## Contracts to preserve

- Building a client makes no network calls. `init()`, `refresh()`, and network cleanup are async; initialization failure raises an error. Never block the application's event loop with network work.
- Flag evaluation is local. Return explicit enabled/disabled decisions; raise specific errors for unknown flags, unavailable policy, or invalid required context. Never silently allow or deny an undefined flag.
- Custom rules are authored in the dashboard and evaluated in Rust. Each condition uses exactly `k`, `op`, and `v`; `k` names a context key. Number operators: `le`, `lt`, `ge`, `gt`, `eq`. String operators: `prefix`, `suffix`, `contains`, `matches` (exact equality), `regex`. Boolean operator: `is`. All string operations are case-sensitive and types are never implicitly converted. No SDK callbacks or free-form code.
- Keep Rust evaluation errors in `Result` and Python errors as specific exceptions. Missing required context raises `InvalidContextError`; wrong context types raise its dedicated subclass `ContextTypeError`, with structured field/operator/type details and no raw context values. The operator dropdown shows each operator's label and type; selecting it sets the value input to a number field, text field, or True/False radios. No separate type selector. Keep the warning about wrong context types.
- Refresh uses `200` with a complete policy or `304` without a body. Keep the last accepted snapshot on errors; replace policy and ETag only after validation succeeds.
- Support native HTTPS with paired certificate/key flags. Validate TLS configuration before listening; never fall back to HTTP after a TLS failure. Use actual transport settings for Origin checks and Secure cookies, not forwarding headers. Allow custom DNS names only when explicitly configured. The Python SDK uses verified async HTTPX requests; optional `ca_bundle(path)` supplies private trust roots without disabling certificate or hostname checks.
- Missing database at startup means show setup without creating a file. Create and populate it after valid setup submission. Never overwrite an existing invalid database automatically.
- First setup requires a loopback bind. Existing databases may be served remotely. Use a persistent database-path `.lock` sidecar to exclude other Partly processes; never create it on empty first boot or remove it while in use. Offline password reset preserves all other data. Support graceful SIGINT and SIGTERM shutdown.
- Bound password hashing concurrency and run it outside the database mutex. Limit login attempts by the actual peer IP, ignoring forwarded headers. Return 429 with Retry-After when busy or throttled.
- Generate named SDK keys in the dashboard. The authenticated key list includes secrets; keep them in masked fields and copy or reveal locally, without a `/reveal` endpoint. Keep IDs internal. Plaintext SDK-key storage in SQLite is explicitly allowed for the POC; admin passwords must still be hashed. Do not log credentials.
- Keep the database schema at v1; do not add a migration to this alpha. The released v1 format becomes a compatibility baseline. Later schema changes need a tested, data-preserving upgrade path. Never reset existing databases automatically. Test stopped backup/restore with flags, keys, and admin access.
- Mark the key-storage limit in code: `shortcut: SDK keys are plaintext in the local POC, add protected storage before production use.`

## Quality and documentation

- Keep the separation between the Rust core, control plane, Python SDK, examples, and tests. Add files and language folders when needed, not as empty placeholders.
- Keep the blue Console design: compact layout, white and neutral surfaces, restrained blue accents, clear borders, readable controls, and keyboard accessibility. Avoid gradients, decorative shadows, and generic AI-style decoration.
- Use direct tool labels instead of slogans, decorative section labels, or repeated technical metadata. Keep the default project internal. Environment names are labels; do not give production special UI treatment or show environment warning banners. Align Create flag and Generate key in the same page-header position; use deliberate creation dialogs.
- Preserve the Partly logo. Keep the brand, environment selector, and tab ribbon together in a sticky header. Success feedback must not move or resize existing content when it appears or disappears. Use existing space beside the affected item or an anchored tooltip, with short labels such as Copied, Saved, or Created. Never insert a message row or page banner for success feedback.
- Call application credentials API Keys in the UI. Explain that API Keys read flags and rules, while edits are made through the UI. Keep existing SDK method names and key storage contracts.
- Keep three dashboard screens in this ribbon order: Feature flags, Key Management, Docs. Docs shows the tested complete Python quickstart with syntax colors, placeholder URL/API key constants, a Python-only language selector, and an accessible copy icon. Do not show installation steps or require environment variables in the quickstart.
- Keep the environment selector at the top right. The environment is part of the URL; switching environments reloads the page. Warn before losing unsaved edits through navigation, reload, or closing a dirty modal.
- Use styled dropdowns throughout the dashboard, including their option menus. Preserve keyboard navigation, type-ahead search, accessible labels, and selected states. Opening menus must not move content. Escape and outside clicks cancel the pending choice.
- Create flags through a modal in the current environment only. Display the environment as read-only context, never as a selectable field. Create and edit responses return the saved flag so the dashboard can update it in place without refetching the list or discarding other drafts.
- Write READMEs in CEFR-B2 English. Explain necessary terms and provide working examples.
- Start first-party code files with `Copyright 2026 ByteBaker` and the component's SPDX license identifier in language-appropriate comments. Preserve required shebangs and doctypes; leave generated and third-party material unchanged.
- Keep the root README focused on what Partly does, getting started, key alpha limits, and links to package and release guides.
- Keep `examples/python` runnable and exercise it through end-to-end tests. Start with a minimal working quickstart; keep detailed options and error types in the SDK reference. Cover all supported POC workflows, including setup, keys, async SDK lifecycle, refresh, and failures.
- Add focused tests for native evaluation and percentage consistency. Run checks appropriate to each change; documentation-only edits do not require application tests.
- Do not claim performance gains without measurements that include Python binding overhead.
- When research is needed, use primary sources and the strongest available research model where model selection is supported.
