# Partly control plane

One executable serves the dashboard and API, with Axum and embedded SQLite. No separate assets or frontend runtime are needed.

From the repository root:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

These flags show the defaults. The database's parent directory must exist. Reuse the same path on restart. Ctrl+C or SIGTERM stops the server, allowing up to three seconds for requests to finish. Admin sessions expire after 12 hours or a restart.

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
- For a private certificate authority, trust its root in the browser and configure the [SDK CA bundle](../../sdks/python/README.md#https-and-private-certificate-authorities). Keep the private key on the server.
- Obtain and renew certificates externally, then restart Partly to load them.

## Setup and storage

Missing database: Partly shows setup on a loopback address without creating files. Submitting valid credentials creates the database, one admin, and Development and Production environments in one transaction.

Existing database: Partly checks its schema, integrity, credentials, and policies before serving requests. Invalid or incompatible files are rejected, never reset. Schema v1 has no migrations; older incompatible POC databases need a fresh `--db` path.

Passwords use salted Argon2id hashes. API Keys are plaintext, so keep the database and backups private. New database files use owner-only permissions on Unix.

A persistent `<database>.lock` file prevents concurrent servers or password resets. Do not remove it while Partly is running; the OS releases the lock on exit. Unix databases with multiple hard links are rejected; remove extra links or use an independent copy. Symbolic links share the original database lock.

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

- **Feature flags:** create and edit flags in the current environment. Choose everyone, percentage, custom rules, or nobody. Unsaved changes trigger navigation warnings; saving one flag preserves other drafts.
- **Key Management:** generate named API Keys, rename them without changing the secret, or revoke them. Copy and reveal work locally and remain available after restart. If clipboard access fails, reveal and copy manually.
- **Docs:** copy the tested [Python quickstart](../../examples/python/quickstart.py) and replace its URL and API Key constants.

API Keys only read policies. Masking hides keys visually; authenticated browsers still hold their values. Revoking a key stops future downloads, but cannot erase a policy already held by an SDK.

## HTTP contract

Admin writes require `X-Partly-Request: dashboard`. Sessions use HttpOnly, SameSite=Strict cookies, plus Secure over HTTPS. Cross-site browser API requests are rejected. Origin checks use the listener's scheme and allowed hostname, ignoring forwarded headers; TLS termination at a reverse proxy is not configured by `--hostname`.

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

Login allows five attempts per peer IP per minute, ignoring forwarded headers. Busy or throttled requests return `429` with `Retry-After: 60`. Limits reset on restart.

Policy downloads authenticate before checking `If-None-Match`. The key fixes the project and environment. A `200` response contains the full policy and a quoted SHA-256 ETag; `304` has no body. Identical saves and changes in other environments leave the ETag unchanged.

See the [policy format and rule operators](../partly-core/README.md#policy-format) for flag JSON. Duplicate creation returns `409`; updating a missing flag returns `404`. Custom actions are `enable` or `disable`, with the opposite decision for nonmatching context. Invalid rules are rejected.

Admin JSON bodies are limited to 16 KiB; each environment's complete policy to 1 MiB. An oversized policy save returns `400` and preserves the previous flag and revision.

All responses use `Cache-Control: no-store`. Key lists include secrets; there is no `/reveal` endpoint. Storage failures return a generic `500`, with details in the terminal. Credentials are not logged.

## License

The control plane and dashboard are source-available under the [Elastic License 2.0](../../LICENSE). The bundled evaluator and Python quickstart retain their Apache-2.0 license. See [licensing boundaries](../../LICENSING.md) before redistributing the executable.
