# Node.js quickstart

Follow the [server setup](../../README.md#build-locally) to create `new_checkout` and an app that selects it. Replace the URL, environment, application ID, and API Key in [quickstart.js](quickstart.js), then run from the repository root:

```sh
npm ci --ignore-scripts
node sdk/node/build.js
node examples/node/quickstart.js
```

`npm ci` installs the official OpenFeature SDK and links the local Partly package. The example initializes Partly and registers its OpenFeature provider once at startup. Two requests reuse the OpenFeature client for local checks and handle errors explicitly. Keep both clients in application state and close them at shutdown. See the [SDK guide](../../sdk/node/README.md).

The dashboard serves this same source, and the end-to-end suite runs it for enabled, disabled, unknown-flag, invalid-context, and rejected-key cases, including installed release packages.

## Typed result example

[typed_results.js](typed_results.js) uses OpenFeature's object getter with the same initialize-once lifecycle. Create a `Checkout` record type with one `layout: string` field, then create and assign `checkout_settings`, with `{"layout":"wide"}` selected and `{"layout":"compact"}` otherwise. Set the example's connection constants before running it. [Value types and optional Partly SDK enhancements](../../docs/typed-results.md).
