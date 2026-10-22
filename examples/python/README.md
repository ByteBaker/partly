# Python examples

Start with [quickstart.py](quickstart.py), also shown in the dashboard's **Docs** tab. It initializes Partly and registers its OpenFeature provider once at startup. Two example requests share the OpenFeature client, then both clients close at shutdown.

Follow the [server setup](../../README.md#build-locally) to create `new_checkout` and an app that selects it, then [install the Python SDK](../../sdk/python/README.md#build-locally). Set the URL, environment, application ID, and API Key constants in `quickstart.py`, then run from the repository root:

```sh
python -m pip install 'openfeature-sdk>=0.10,<0.11'
python examples/python/quickstart.py
```

It prints `Use the new checkout` or `Use the current checkout` for each request. In a web application, initialize and register in the startup hook, then keep both clients in application state. Handlers call the detailed Boolean getter and check errors. Close both clients in the shutdown hook. See the [SDK guide](../../sdk/python/README.md) for refresh, context, and errors.

## Check a larger rollout

[rollout.py](rollout.py) checks 10,000 stable IDs and prints totals. Set `new_checkout` to **Percentage of users: 50** in the same environment as the API Key. In Bash, with the virtual environment active:

```bash
export PARTLY_URL=http://127.0.0.1:8080
export PARTLY_ENVIRONMENT=development
export PARTLY_APPLICATION_ID=YOUR_APPLICATION_ID
read -r -s PARTLY_SDK_KEY
export PARTLY_SDK_KEY
python examples/python/rollout.py
```

Paste the key when `read` waits, then press Enter. Only this diagnostic uses environment variables; `PARTLY_FLAG` optionally selects another flag. Expect roughly 5,000 selected users. Adding users does not change earlier decisions.

`Policy updated: False` means the manual refresh found no change. End-to-end tests run both examples.

## License

[Apache-2.0](LICENSE). Preserve license and attribution notices when redistributing.
