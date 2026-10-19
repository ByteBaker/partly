# Partly control plane

The server uses Axum for HTTP and bundled SQLite for storage. Vanilla HTML, CSS, and JavaScript are included at compile time, so the executable needs no asset directory or frontend runtime.

From the repository root:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

These are also the default values. `--db` is relative to the current working directory. Its parent directory must exist. Stop with Ctrl+C or SIGTERM. Restart with the same path to reuse the data. Admin sessions last up to 12 hours and are cleared on restart.

HTTP and HTTPS stop accepting connections on shutdown and allow up to three seconds for current requests to finish. Connections still open after that deadline are closed.

## HTTPS

The executable can serve HTTPS directly using Rustls. Complete first-run setup on the default loopback address before binding a public address. Then stop the process and supply a PEM certificate chain and its matching private key:

```sh
./target/release/partly serve --host 0.0.0.0 --port 8443 --db ./partly.db \
  --tls-cert ./certs/fullchain.pem --tls-key ./certs/privkey.pem \
  --hostname flags.example.com
```

Open `https://flags.example.com:8443` and use the same URL in the SDK. The DNS name must point to the server, and the certificate must cover that name. `--hostname` allows one extra DNS name, without a scheme or port; direct IP addresses and `localhost` remain accepted. Binding `0.0.0.0` listens on all IPv4 interfaces; keep the default `127.0.0.1` for local access only.

Both TLS flags are required together. The certificate file contains the server certificate first, followed by intermediate certificates. The private key must be unencrypted PEM. Invalid files or mismatched keys stop startup. TLS supports versions 1.2 and 1.3, with HTTP/1.1 and HTTP/2. The TLS listener does not also accept plaintext HTTP, redirect it, or fall back after an error. With neither TLS flag, the existing HTTP mode remains available.

Use a certificate trusted by browsers and the SDK. For a private or local CA, trust its root in the browser and pass its PEM bundle through SDK `.ca_bundle("./certs/ca.pem")`; see the [SDK HTTPS guide](../../sdks/python/README.md#https-and-private-certificate-authorities). TLS keys stay on the server and must not be supplied to the SDK. Certificate files are ignored by Git.

Certificates are read at startup. Obtain and renew them outside Partly, then restart the process to load replacements. Automatic certificate issuance and renewal are not included. HTTPS session cookies include `Secure`; HTTP session cookies keep the local development behavior.

## Setup and storage

When the file is missing, Partly serves setup on a loopback address without creating a database or lock file. An unconfigured server refuses a non-loopback bind. Valid setup exclusively creates a file and writes the schema, hashed admin password, default project, and two environments in one SQLite transaction. Concurrent submissions cannot create two admins. A failed attempt removes only the database that attempt created.

Existing files are opened without SQLite's create flag. Partly checks the schema version, database integrity, foreign-key references, admin password record, and policy data before serving requests. It rejects incomplete or invalid files. Keep the file and its backups private; new files use owner-only permissions on Unix.

The alpha schema is v1, defined in `001_initial.sql`. Flags store their rollout mode and conditions as JSON, so adding a rule does not change the database schema. There are no upgrade migrations. A database created before named keys or JSON flag configuration needs a fresh `--db` path. Partly reports an incompatible schema and leaves the existing file unchanged; it never resets data automatically.

Database operations and password hashing run on blocking worker threads, away from the HTTP event loop. Password hashing does not hold the database mutex. One process serializes its SQLite work through a small storage boundary. A persistent `<database>.lock` sidecar prevents another Partly server or password reset from opening the same database concurrently. Keep it in place; the OS releases its lock when the process exits. No HA behavior or alternative storage adapter is included.

## Password recovery

Stop the server before changing or recovering the admin password. From a terminal with access to the database, run:

```sh
python3 -c 'import getpass; print(getpass.getpass("New admin password: "))' | \
  ./target/release/partly reset-password --db ./partly.db
```

The command reads a single password from standard input. It refuses direct terminal input to avoid showing the password while you type. Passwords must be 12–1024 UTF-8 bytes long (12–1024 characters for ASCII). Do not put the password in command arguments or shell history. This changes the password while preserving the username, flags, and API Keys. Restart the server afterward; old browser sessions are no longer valid. Missing or invalid databases are rejected without creating or replacing them.

## Backup, restore, and upgrades

For this single-server alpha, use a stopped-database backup. Stop Partly with Ctrl+C or SIGTERM and wait for it to exit successfully. Then copy the database:

```sh
umask 077
cp -p ./partly.db ./partly-backup.db
chmod 600 ./partly-backup.db
```

Restart with the original `--db` path. Do not copy a live database file: an in-progress transaction may also depend on SQLite journal files. The `.lock` file contains no application data and is not part of the backup. Backups contain readable API Keys, so keep them private.

To restore, stop the server, copy the backup to a **new** path, and start Partly with that path:

```sh
cp -p ./partly-backup.db ./partly-restored.db
chmod 600 ./partly-restored.db
./target/release/partly serve --db ./partly-restored.db
```

Check the flags and application access before removing any original file. Users must sign in again; browser sessions are kept only in memory. Use the same TLS flags as your usual deployment when restoring an HTTPS server.

Before replacing the executable, make a stopped backup and retain the previous executable. The released v1 schema is the compatibility baseline. Future schema changes need an explicit, tested upgrade path that preserves accounts, keys, and policies. This alpha does not migrate data. A newer or incompatible schema causes a startup error without modification. To roll back a future schema upgrade, restore its pre-upgrade backup with the matching older executable.

## Dashboard

The normal `/` page handles setup and login. After login, the top ribbon switches between **Feature flags**, **Key Management**, and **Docs**, in that order. The top-right environment selector performs a full page navigation. The environment and screen stay visible in the URL, such as `/environments/production/flags`. Unknown environments and screens return 404.

Create flag and Generate key share the same page-header position and open separate dialogs. API Keys read feature flags and rules; use the UI to make changes.

The logo, environment selector, and tab ribbon stay visible while scrolling. Short success messages use existing row padding or a tooltip beside the action. They disappear after three seconds without moving or resizing content.

Create a flag in the modal, which names the current environment without allowing it to change. Choose one rollout mode: enabled for everyone, percentage of users, custom rules, or disabled for everyone. Custom rules match all conditions within a group and any group within the flag. Each condition contains `k`, `op`, and `v`. Number comparisons, case-sensitive string matching, and boolean equality use typed inputs. The SDK checks context types and reports errors; the control plane never receives evaluation context.

The dashboard warns before navigation or modal dismissal would lose unsaved edits. Browser reload and close warnings use the browser's own confirmation prompt, which cannot be guaranteed on every device shutdown. Saving one flag updates its row directly from the response, preserving other drafts.

The dashboard uses the selected blue Console design: compact controls, white and neutral surfaces, clear borders, and restrained blue accents. It has no external font or frontend dependency.

Dropdowns use the same styled menus throughout the dashboard. Use arrows, Home, End, or type a label to highlight an option; Enter, Space, Tab, or a click selects it. Escape or an outside click cancels the choice. Menus overlay the page without moving content, including inside dialogs. This uses the browser's Popover API and requires a current browser.

Select **Generate key** in the page header to open the API Key dialog. Give each key an application name when generating it. Edit the name and select **Save name** to rename it; this does not change the key. Names accept 1–64 characters. Internal IDs are not shown.

The authenticated dashboard loads keys into masked fields. Select **Copy** to copy a key while it stays masked; a message confirms success. **Reveal** displays the value, and **Hide** masks it again. These actions use the value already in the UI and make no API request. Copy and reveal remain available after a restart. If the browser blocks clipboard access, reveal the key and copy it manually. Masking is visual: keys are still present in the authenticated browser.

**Docs** shows the syntax-highlighted `examples/python/quickstart.py` and a language dropdown with Python as the only current option. It includes initialization, key supply, evaluation, error handling, and cleanup. Replace its placeholder URL and API Key constants with your values; it needs no environment variables. The copy icon copies the complete source. Installation instructions stay in the READMEs. The code is bundled into the executable and served at `/docs/python.py`; it contains no credentials.

New keys are 27 characters: `pk_` plus 24 random ASCII letters and digits, generated using the operating system's secure random source. Existing longer keys remain valid. Revocation removes policy-download access; it does not erase a policy already held by an SDK.

## HTTP contract

All admin writes require `X-Partly-Request: dashboard`. Login sets an HttpOnly, SameSite=Strict session cookie, with Secure added in HTTPS mode. The server rejects cross-site browser API requests and provides no cross-origin access. It accepts direct IPs, `localhost`, and the optional configured `--hostname`. Origin checks use the actual listener's HTTP or HTTPS scheme and support HTTP/2 authority. Forwarded headers cannot change the scheme or allowed hostname; TLS termination at a reverse proxy is not configured by this option.

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

Login allows five attempts per peer IP per 60 seconds. Forwarded headers do not change that IP. At most two password operations run at once, and the limiter tracks at most 1,024 peers. A throttled or busy request returns `429` with `Retry-After: 60`; wait before trying again. These limits reset when the server restarts.

The API Key chooses the project and environment; request parameters cannot widen access. The server authenticates before checking `If-None-Match`. `200` includes the complete JSON policy and a quoted SHA-256 content ETag. `304` has no body. Changes to another environment or saving identical flag values leave the current policy unchanged.

Creation returns 409 if the key already exists in that environment. Updating a missing flag returns 404. Both authenticate the administrator before validating rules on a blocking worker. A flag has one of these JSON forms:

```json
{"mode": "enabled"}
{"mode": "disabled"}
{"mode": "percentage", "percentage": 50}
{"mode": "custom", "action": "enable", "groups": [[{"k": "age", "op": "ge", "v": 18}]]}
```

The custom action can be `enable` or `disable`; nonmatching context gets the opposite decision. Empty groups, invalid operator/value combinations, and invalid regex patterns are rejected. Regex uses Rust syntax without look-around, backreferences, or case-insensitive flags. See the [core README](../partly-core/README.md) for operators and evaluation details.

An environment's complete policy must fit within the SDK's 1 MiB download limit. A save that would exceed it returns 400 and rolls back the flag and policy revision together. Remove flags or simplify rules before retrying; the last valid policy stays available.

There is no `/reveal` endpoint. All responses use `Cache-Control: no-store`, including authenticated key lists.

Admin JSON bodies are limited to 16 KiB. Invalid input returns a 4xx response; storage failures return a generic 500 and details in the terminal. Credentials are not logged. API Keys are deliberately plaintext in the database for this local POC; passwords are salted Argon2id hashes.

See the [root README](../../README.md) to get started and [development checks](../../RELEASING.md#build-and-verify) for the full test commands.

## License

The control plane and dashboard are source-available under the [Elastic License 2.0](../../LICENSE). The bundled evaluator and Python quickstart retain their Apache-2.0 license. See [licensing boundaries](../../LICENSING.md) before redistributing the executable.
