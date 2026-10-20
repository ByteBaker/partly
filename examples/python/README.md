# Python examples

Start with [quickstart.py](quickstart.py), also shown in the dashboard's **Docs** tab. It initializes one client at startup, passes it to a local `checkout` handler for two example requests, and closes it at shutdown.

Follow the [root setup guide](../../README.md) to create `new_checkout`, generate an API Key, and install the SDK. Set the URL and key constants in `quickstart.py`, then run from the repository root:

```sh
python examples/python/quickstart.py
```

It prints `Use the new checkout` or `Use the current checkout` for each request. In a web application, await `init()` in the startup hook and keep the client in application state. Request handlers call `evaluate()` on that client; they do not initialize, refresh, or close it. Close it in the shutdown hook. See the [SDK guide](../../sdk/python/README.md) for refresh, context, and errors.

## Check a larger rollout

[rollout.py](rollout.py) checks 10,000 stable IDs and prints totals. Set `new_checkout` to **Percentage of users: 50** in the same environment as the API Key. In Bash, with the virtual environment active:

```bash
export PARTLY_URL=http://127.0.0.1:8080
read -r -s PARTLY_SDK_KEY
export PARTLY_SDK_KEY
python examples/python/rollout.py
```

Paste the key when `read` waits, then press Enter. Only this diagnostic uses environment variables; `PARTLY_FLAG` optionally selects another flag. Expect roughly 5,000 selected users. Adding users does not change earlier decisions.

`Policy updated: False` means the manual refresh found no change. End-to-end tests run both examples.

## License

[Apache-2.0](LICENSE). Preserve license and attribution notices when redistributing.
