# Partly control plane

One executable serves the dashboard and API, with Axum and embedded SQLite. No separate assets or frontend runtime are needed.

From the repository root:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

These flags show the defaults. The database's parent directory must exist. Reuse the same path on restart. Ctrl+C or SIGTERM stops the server, allowing up to three seconds for requests to finish. Account sessions expire after 12 hours or a restart.

## HTTPS

Complete setup on `127.0.0.1` first. Stop the server, then restart with a certificate and key:

```sh
./target/release/partly serve --host 0.0.0.0 --port 8443 --db ./partly.db \
  --tls-cert ./certs/fullchain.pem --tls-key ./certs/privkey.pem \
  --hostname flags.example.com
```

Use `https://flags.example.com:8443` in the browser and SDK. Point that DNS name to the server and use a certificate covering it. `--hostname` accepts one DNS name without a scheme or port; direct IPs and `localhost` also remain accepted. `0.0.0.0` listens on all IPv4 interfaces.

- Supply both TLS flags. The certificate PEM must list the server certificate first, then intermediates. The matching private key must be unencrypted PEM.
- Invalid TLS configuration stops startup. HTTPS supports TLS 1.2/1.3 and HTTP/1.1/2, with no plaintext fallback or redirect. Omit both TLS flags for local HTTP.
- For a private certificate authority, trust its root in the browser and configure the [SDK CA bundle](../../sdk/python/README.md#https-and-private-certificate-authorities). Keep the private key on the server.
- Obtain and renew certificates externally, then restart Partly to load them.

## Setup and storage

Missing database: Partly shows setup on a loopback address without creating files. Submitting valid credentials creates the database, the initial administrator, and Development and Production environments in one transaction.

Existing database: Partly checks its schema, integrity, credentials, and policies before serving requests. Invalid or incompatible files are rejected, never reset. Schema v1 has no migrations; older incompatible POC databases need a fresh `--db` path.

Passwords use salted Argon2id hashes. API Keys are plaintext, so keep the database and backups private. New database files use owner-only permissions on Unix.

A persistent `<database>.lock` file prevents concurrent servers or password resets. Do not remove it while Partly is running; the OS releases the lock on exit. Unix databases with multiple hard links are rejected; remove extra links or use an independent copy. Symbolic links share the original database lock.

## Users and permissions

The initial administrator creates individual accounts in **Users**. Passwords are required; email invitations and SSO are not included. Assign a role independently for each environment, or grant instance administrator access.

| Access | Viewer | Editor | Manager | Instance administrator |
| --- | --- | --- | --- | --- |
| Read flags, app selections, masked key identifiers, and audits | ✅ | ✅ | ✅ | ✅ |
| Create, change, and delete flags | ❌ | ✅ | ✅ | ✅ |
| Manage apps, assignments, and keys; read key secrets | ❌ | ❌ | ✅ | ✅ |
| Manage users and permissions; access every environment | ❌ | ❌ | ❌ | ✅ |

Roles apply only to assigned environments. Accounts without assignments can sign in but cannot read environment data. Changes to permissions, passwords, or account status invalidate all of that account's sessions. Existing API Keys are application credentials: disabling their creator does not revoke those keys. Revoke them separately when needed.

Usernames cannot be renamed. Disable accounts instead of deleting them so audit identities remain clear. The initial administrator cannot be disabled or demoted; the offline recovery command resets that account's password only.

## Password recovery

Stop the server before changing or recovering the admin password. From a terminal with access to the database, run:

```sh
python3 -c 'import getpass; print(getpass.getpass("New admin password: "))' | \
  ./target/release/partly reset-password --db ./partly.db
```

Passwords must be 12–1024 UTF-8 bytes. The command reads one line from standard input and refuses direct terminal input; the prompt above hides your typing. Keep passwords out of command arguments and shell history.

The username, flags, and API Keys stay unchanged. Restart afterward and sign in again. Missing or invalid databases are rejected without replacement.

## Backup, restore, and upgrades

Stop Partly and wait for it to exit, then copy the database:

```sh
umask 077
cp -p ./partly.db ./partly-backup.db
chmod 600 ./partly-backup.db
```

Restart with the original path. Never copy a live database: transactions may depend on journal files. The `.lock` file is not part of the backup. Backups contain readable API Keys.

To restore, stop the server, copy the backup to a **new** path, and start Partly with that path:

```sh
cp -p ./partly-backup.db ./partly-restored.db
chmod 600 ./partly-restored.db
./target/release/partly serve --db ./partly-restored.db
```

For HTTPS, add your usual TLS flags. Sign in and verify flags and application access before removing the original file.

Before upgrading, back up the stopped database and keep the old executable. This alpha has no schema migrations. If a future upgrade changes the schema, rollback requires the matching older executable and its pre-upgrade backup.

## Dashboard

Use a current browser; dropdowns require the Popover API. Sign in at `/`. The environment selector reloads the page with the environment in the URL, such as `/environments/production/flags`.

- **Feature flags:** create and edit flags in the current environment. Choose everyone, percentage, custom rules, or nobody. Unsaved changes trigger navigation warnings; saving one flag preserves other drafts. Existing edits and deletions always list affected apps (including an empty list) and require the exact `environment/flag_name`. Concurrent changes invalidate the confirmation.
- **Apps & Keys:** create an app, select its flags, and copy its permanent application ID. A Default API Key is generated automatically. Create extra keys for rotation; rename, copy, reveal, or revoke active keys. Revoked keys remain in history with a partial identifier and dates, but no copy/reveal. Apps are separate in each environment; newly created flags are not automatically assigned.
- **Audits:** browse readable state changes, affected applications, and action/application filters. Updates show before → after; creation shows the initial state. Entries and their changes are saved in one transaction. Each change identifies the signed-in account. User and permission changes are recorded too.
- **Docs:** choose Python, Rust, Go, or Node.js. Copy the tested quickstart and replace its URL, environment, application ID, and API Key constants. Each example initializes once, reuses the client for local evaluation, handles errors, and closes at shutdown.
- **Users** (administrators only): create accounts, set environment roles, reset passwords, or disable access. Changes sign the affected user out. The initial administrator stays enabled for offline recovery.

API Keys only read policies. Masking hides keys visually; administrators and environment managers receive their values; other users receive only masked identifiers. When a refresh confirms revocation, the SDK clears its policy and stops evaluation and polling. Disconnected SDKs cannot discover revocation until they communicate again. Ordinary network and server failures preserve the accepted policy.

## HTTP contract

Dashboard writes require `X-Partly-Request: dashboard`. Sessions use HttpOnly, SameSite=Strict cookies, plus Secure over HTTPS. Cross-site browser API requests are rejected. Origin checks use the listener's scheme and allowed hostname, ignoring forwarded headers; TLS termination at a reverse proxy is not configured by `--hostname`.

| Method and path | Access and result |
| --- | --- |
| `GET /api/status` | Setup and session state |
| `POST /api/setup` | Initial `{username, password}`; one-time setup |
| `POST /api/login` | `{username, password}`; account session |
| `POST /api/logout` | Clear session |
| `GET /api/environments` | Signed-in account; allowed environments and its own permissions |
| `GET /api/environments/{env}/flags` | Viewer or higher; flag map |
| `POST /api/environments/{env}/flags` | Editor or higher; create `{key, ...flag}`; return saved flag with key, status 201 |
| `PUT /api/environments/{env}/flags/{key}` | Editor or higher; update existing flag; return saved flag with key, status 200 |
| `DELETE /api/environments/{env}/flags/{key}` | Editor or higher; delete flag |
| `GET /api/environments/{env}/flags/{key}/impact` | Viewer or higher; affected applications and confirmation token |
| `GET /api/environments/{env}/apps` | Viewer or higher; applications with selected flags and keys |
| `POST /api/environments/{env}/apps` | Manager or higher; `{name, flags: [key, ...]}`; returns app ID, selections, and first key, status 201 |
| `PUT /api/environments/{env}/apps/{id}` | Manager or higher; update `{name, flags}` without changing the ID |
| `POST /api/environments/{env}/apps/{id}/keys` | Manager or higher; `{name}`; returns a new key |
| `GET /api/environments/{env}/audits` | Viewer or higher; newest 100 entries, optional `before` ID, `action`, and `application_id` filters |
| `PUT /api/keys/{id}` | Manager or higher; rename with `{name}` |
| `DELETE /api/keys/{id}` | Manager or higher; revoke key |
| `GET /api/users` | Instance administrator; accounts and permissions, never passwords |
| `POST /api/users` | Instance administrator; create `{username, password, is_admin, enabled, permissions}`; saved user, status 201 |
| `PUT /api/users/{id}` | Instance administrator; same fields, optional password; saved user, status 200 |
| `GET /api/policy` | SDK bearer key; full policy or `304` |

The `permissions` object maps environment names to `viewer`, `editor`, or `manager`; omit environments with no access. Administrators have an empty permissions object because their access is global. API Keys cannot call dashboard or account APIs. Denied signed-in requests return `403`.

Login allows five attempts per peer IP per minute, ignoring forwarded headers. Busy or throttled requests return `429` with `Retry-After: 60`. Limits reset on restart.

Policy downloads authenticate before checking `If-None-Match`. The key fixes the environment and application. Supply both `X-Partly-Environment` and `X-Partly-Application`; a mismatch is rejected. The policy includes `application_id` and only the app's selected flags. A revoked key receives `401`, JSON `code: "key_revoked"`, and `X-Partly-Error: key_revoked`, even with an unchanged ETag. A `200` response contains the full policy and a quoted SHA-256 ETag; `304` has no body. Identical saves and changes outside an app's selected flags leave its ETag unchanged. The content hash identifies the snapshot; each app's revision increases only when its selected policy changes.

See the [policy format and rule operators](../partly-core/README.md#policy-format) for flag JSON. Duplicate creation returns `409`; updating a missing flag returns `404`. Custom actions are `enable` or `disable`, with the opposite decision for nonmatching context. Invalid rules are rejected.

Admin JSON bodies are limited to 16 KiB; each environment's complete policy to 1 MiB. An oversized policy save returns `400` and preserves the previous flag and revision.

Flag PUT/DELETE requests require `X-Partly-Confirm: environment/flag_name` and `X-Partly-Impact` from the impact endpoint. A stale preview returns `409`; review it again.

All responses use `Cache-Control: no-store`. Active key lists include full secrets only for managers and instance administrators. Other roles and revoked entries receive `secret: null`, with masked identifiers and dates. Revocation removes the full stored secret, retaining its SHA-256 hash and partial identifier. Audit details never include full credentials. Audit entries cannot be edited through the API but are not protected from direct database access.

There is no `/reveal` endpoint. Storage failures return a generic `500`, with details in the terminal. Credentials are not logged.

## License

The control plane and dashboard are source-available under the [Elastic License 2.0](../../LICENSE). The bundled evaluator and quickstart examples retain their Apache-2.0 license. See [licensing boundaries](../../docs/licensing.md) before redistributing the executable.
