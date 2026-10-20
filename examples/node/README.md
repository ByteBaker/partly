# Node.js quickstart

Create `new_checkout` and an API Key in the same environment. Replace the constants in [quickstart.js](quickstart.js), then run from the repository root:

```sh
node sdk/node/build.js
node examples/node/quickstart.js
```

The example loads the local SDK, awaits initialization at application startup, then passes the same client to `checkout` for two example requests. Keep the client in application state and share it with request handlers. Handlers only evaluate; close the client at application shutdown, including after failures. See the [SDK guide](../../sdk/node/README.md).

The end-to-end suite runs this source for enabled, disabled, and rejected-key cases.
