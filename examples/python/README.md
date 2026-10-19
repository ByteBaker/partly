# Python rollout example

This is a small async application using the installed Partly SDK. It evaluates 10,000 stable user IDs (`user-1` through `user-10000`) and performs an unchanged refresh. The end-to-end suite runs this same file.

1. Build and run the control plane using the [root README](../../README.md).
2. In **Development**, select **New flag**, enter `new_checkout`, choose **Percentage of users**, and set 50. Creation uses the current environment.
3. In **Key Management**, name and generate an SDK key, then select **Copy**.
4. Install the Python wheel and activate your virtual environment.

From the repository root:

```sh
export PARTLY_URL=http://127.0.0.1:8080
read -r -s PARTLY_SDK_KEY
export PARTLY_SDK_KEY
python examples/python/rollout.py
```

Paste the key at the `read` prompt and press Enter. `PARTLY_FLAG` can select a different existing flag; the default is `new_checkout`.

The example prints one line per user, with either `new checkout` or `current checkout`. The same policy and stable `context["id"]` produce the same result. A 50% rollout should select roughly half the users; it does not promise exactly 5,000.

The last line normally reads `Policy updated: False`. Errors are allowed to reach the terminal so a missing flag or failed initialization is visible. The client is always closed in `finally`.

## Complete quickstart

`quickstart.py` checks one user and shows explicit error handling for configuration, initialization, refresh, and evaluation. The dashboard's **Docs** screen serves this exact file. After the same setup above, run:

```sh
python examples/python/quickstart.py
```

`PARTLY_USER_ID` supplies `context["id"]` and defaults to `user-123`. The quickstart also supplies `age: 23`, `plan: "pro"`, and `verified: True` as sample context for custom rules. Replace them with your application data. Context stays local; only the policy is downloaded. A wrong context type is handled through `ContextTypeError`, including the field and expected type. A failed initialization or evaluation exits with a clear error instead of silently choosing a feature value. A failed manual refresh reports the problem and keeps the last accepted policy. Both examples close the client in `finally`.
