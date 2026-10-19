# Python examples

Start with [quickstart.py](quickstart.py). It builds one client, downloads the policy, checks one flag locally, and closes the client. Initialization or evaluation errors stop the example with a clear message. The dashboard's **Docs** screen serves this exact file.

1. Build and run Partly and install the Python wheel using the [root README](../../README.md).
2. In **Development**, select **Create flag**, enter `new_checkout`, choose **Percentage of users**, and set 50.
3. In **Key Management**, select **Generate key**, enter a name, generate the key, then select **Copy**.
4. In `quickstart.py`, replace `YOUR_API_KEY` with that key and set `CONTROL_PLANE_URL` to your server address.
5. With your virtual environment active, run from the repository root:

```sh
python examples/python/quickstart.py
```

The example prints `Use the new checkout` or `Use the current checkout`. Replace the flag name and context with your application data. Context stays local. Keep `context["id"]` stable for percentage rollouts. The URL and API Key are ordinary Python constants; the SDK does not require environment variables.

In your application, reuse the client across requests and close it during shutdown. It polls for policy changes every 30 seconds. See the [SDK README](../../sdks/python/README.md) for manual refresh, custom rules, and specific error types.

## Check a larger rollout

[rollout.py](rollout.py) evaluates 10,000 stable IDs and prints a total instead of listing every user:

```sh
export PARTLY_URL=http://127.0.0.1:8080
read -r -s PARTLY_SDK_KEY
export PARTLY_SDK_KEY
python examples/python/rollout.py
```

Paste the key at the `read` prompt and press Enter. This optional diagnostic uses environment variables for convenience. It uses `new_checkout` by default; `PARTLY_FLAG` can select a different existing flag. A 50% rollout selects roughly half the users, not exactly 5,000. The same flag and ID produce the same result when the policy is unchanged. Adding users does not change earlier users' decisions.

The last line normally reads `Policy updated: False`, showing an unchanged manual refresh. Both examples are run by the end-to-end tests and close the client in `finally`.

## License

The examples use the [Apache License 2.0](LICENSE), so you can adapt them in proprietary or open-source applications. Preserve the required license and attribution notices when redistributing them.
