# Python examples

Start with [quickstart.py](quickstart.py). It builds one client, downloads the policy, checks one flag locally, and closes the client. Initialization or evaluation errors stop the example with a clear message. The dashboard's **Docs** screen serves this exact file.

1. Build and run Partly and install the Python wheel using the [root README](../../README.md).
2. In **Development**, select **Create flag**, enter `new_checkout`, choose **Percentage of users**, and set 50.
3. In **Key Management**, name and generate an SDK key, then select **Copy**.
4. With your virtual environment active, run these commands from the repository root:

```sh
export PARTLY_URL=http://127.0.0.1:8080
read -r -s PARTLY_SDK_KEY
export PARTLY_SDK_KEY
python examples/python/quickstart.py
```

Paste the key at the `read` prompt and press Enter. The example prints `Use the new checkout` or `Use the current checkout`. Replace the flag name and context with your application data. Context stays local. Keep `context["id"]` stable for percentage rollouts.

In your application, reuse the client across requests and close it during shutdown. It polls for policy changes every 30 seconds. See the [SDK README](../../sdks/python/README.md) for manual refresh, custom rules, and specific error types.

## Check a larger rollout

[rollout.py](rollout.py) evaluates 10,000 stable IDs and prints a total instead of listing every user:

```sh
python examples/python/rollout.py
```

It uses `new_checkout` by default; `PARTLY_FLAG` can select a different existing flag. A 50% rollout selects roughly half the users, not exactly 5,000. The same flag and ID produce the same result when the policy is unchanged. Adding users does not change earlier users' decisions.

The last line normally reads `Policy updated: False`, showing an unchanged manual refresh. Both examples are run by the end-to-end tests and close the client in `finally`.
