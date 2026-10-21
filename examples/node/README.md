# Node.js quickstart

Follow the [server setup](../../README.md#build-locally) to create `new_checkout` and an app that selects it. Replace the URL, environment, application ID, and API Key in [quickstart.js](quickstart.js), then run from the repository root:

```sh
npm ci --ignore-scripts
node sdk/node/build.js
node examples/node/quickstart.js
```

The example imports `partly-sdk`; `npm ci` links the local workspace package. It initializes at application startup, then passes the same client to `checkout` for two example requests. Keep the client in application state and share it with request handlers. Handlers only evaluate; close the client at shutdown, including after failures. See the [SDK guide](../../sdk/node/README.md).

The dashboard serves this same source, and the end-to-end suite runs it for enabled, disabled, and rejected-key cases.
