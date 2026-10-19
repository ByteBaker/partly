# Partly control plane

The server uses Axum for HTTP and bundled SQLite for storage. Vanilla HTML, CSS, and JavaScript are included at compile time, so the executable needs no asset directory or frontend runtime.

From the repository root:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

These are also the default values. `--db` is relative to the current working directory. Its parent directory must exist. Stop with Ctrl+C. Restart with the same path to reuse the data. Admin sessions last up to 12 hours and are cleared on restart.

## Setup and storage

When the file is missing, Partly serves setup without creating it. Valid setup exclusively creates a file and writes the schema, hashed admin password, default project, and two environments in one SQLite transaction. Concurrent submissions cannot create two admins. A failed attempt removes only the file that attempt created.

Existing files are opened without SQLite's create flag. Partly checks the schema version, database integrity, admin password record, and policy data before serving requests. It rejects incomplete or invalid files. Keep the file and its backups private; new files use owner-only permissions on Unix.

The development schema stays at v1, with changes made in `001_initial.sql`. Flags store their rollout mode and conditions as JSON, so adding a rule does not change the database schema. There are no upgrade migrations. A database created before named keys or JSON flag configuration needs a fresh `--db` path. Partly reports an incompatible schema and leaves the existing file unchanged; it never resets data automatically.

Database operations and password hashing run on blocking worker threads, away from the HTTP event loop. One process serializes its SQLite work through a small storage boundary. No HA behavior or alternative storage adapter is included.

## Dashboard

The normal `/` page handles setup and login. After login, the top ribbon switches between **Feature flags**, **Key Management**, and **Docs**, in that order. The top-right environment selector performs a full page navigation. The environment and screen stay visible in the URL, such as `/environments/production/flags`. Unknown environments and screens return 404.

Create a flag in the modal, which names the current environment without allowing it to change. Choose one rollout mode: enabled for everyone, percentage of users, custom rules, or disabled for everyone. Custom rules match all conditions within a group and any group within the flag. Each condition contains `k`, `op`, and `v`. Number comparisons, case-sensitive string matching, and boolean equality use typed inputs. The SDK checks context types and reports errors; the control plane never receives evaluation context.

The dashboard warns before navigation or modal dismissal would lose unsaved edits. Browser reload and close warnings use the browser's own confirmation prompt, which cannot be guaranteed on every device shutdown. Saving one flag updates its row directly from the response, preserving other drafts.

The dashboard uses the selected blue Console design: compact controls, white and neutral surfaces, clear borders, and restrained blue accents. It has no external font or frontend dependency.

Give each key an application name when generating it. Edit the name and select **Save name** to rename it; this does not change the key. Names accept 1–64 characters. Internal IDs are not shown.

The authenticated dashboard loads keys into masked fields. Select **Copy** to copy a key while it stays masked; a message confirms success. **Reveal** displays the value, and **Hide** masks it again. These actions use the value already in the UI and make no API request. Copy and reveal remain available after a restart. If the browser blocks clipboard access, reveal the key and copy it manually. Masking is visual: keys are still present in the authenticated browser.

**Docs** has a language dropdown with Python as the only current option. It shows installation instructions and the complete, runnable `examples/python/quickstart.py`, including initialization, key supply, evaluation, refresh, error handling, and cleanup. The code is bundled into the executable and served at `/docs/python.py`; it contains no credentials. Select **Copy code** to use it in your application.

New keys are 27 characters: `pk_` plus 24 random ASCII letters and digits, generated using the operating system's secure random source. Existing longer keys remain valid. Revocation removes policy-download access; it does not erase a policy already held by an SDK.

## HTTP contract

All admin writes require `X-Partly-Request: dashboard`. Login sets an HttpOnly, SameSite=Strict session cookie. The server rejects cross-site browser API requests, provides no cross-origin access, and accepts only direct IPs or `localhost` as hostnames. This is a local HTTP POC, not a reverse-proxy or public hosting configuration.

| Method and path | Access and result |
| --- | --- |
| `GET /api/status` | Setup and session state |
| `POST /api/setup` | Initial `{username, password}`; one-time setup |
| `POST /api/login` | `{username, password}`; admin session |
| `POST /api/logout` | Clear session |
| `GET /api/environments` | Admin; default project and environments |
| `GET /api/environments/{env}/flags` | Admin; flag map |
| `POST /api/environments/{env}/flags` | Admin; create `{key, ...flag}`; return saved flag with key, status 201 |
| `PUT /api/environments/{env}/flags/{key}` | Admin; update existing flag; return saved flag with key, status 200 |
| `DELETE /api/environments/{env}/flags/{key}` | Admin; delete flag |
| `GET /api/environments/{env}/keys` | Admin; `{keys: [{id, name, secret}]}` |
| `POST /api/environments/{env}/keys` | Admin; accept `{name}`, generate and return `{id, name, secret}` |
| `PUT /api/keys/{id}` | Admin; rename with `{name}` |
| `DELETE /api/keys/{id}` | Admin; revoke key |
| `GET /api/policy` | SDK bearer key; full policy or `304` |

The SDK key chooses the project and environment; request parameters cannot widen access. The server authenticates before checking `If-None-Match`. `200` includes the complete JSON policy and a quoted SHA-256 content ETag. `304` has no body. Changes to another environment or saving identical flag values leave the current policy unchanged.

Creation returns 409 if the key already exists in that environment. Updating a missing flag returns 404. Both validate the full configuration before writing. A flag has one of these JSON forms:

```json
{"mode": "enabled"}
{"mode": "disabled"}
{"mode": "percentage", "percentage": 50}
{"mode": "custom", "action": "enable", "groups": [[{"k": "age", "op": "ge", "v": 18}]]}
```

The custom action can be `enable` or `disable`; nonmatching context gets the opposite decision. Empty groups, invalid operator/value combinations, and invalid regex patterns are rejected. Regex uses Rust syntax without look-around, backreferences, or case-insensitive flags. See the [core README](../partly-core/README.md) for operators and evaluation details.

There is no `/reveal` endpoint. All responses use `Cache-Control: no-store`, including authenticated key lists.

Admin JSON bodies are limited to 16 KiB. Invalid input returns a 4xx response; storage failures return a generic 500 and details in the terminal. Credentials are not logged. SDK keys are deliberately plaintext in the database for this local POC; passwords are salted Argon2id hashes.

The [root README](../../README.md) has the complete build and test commands.
