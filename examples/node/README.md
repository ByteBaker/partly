# Node.js quickstart

Create `new_checkout` and an API Key in the same environment. Replace the constants in [quickstart.js](quickstart.js), then run from the repository root:

```sh
node sdk/node/build.js
node examples/node/quickstart.js
```

The example loads the local SDK, awaits initialization, evaluates locally, and closes on success or failure. See the [SDK guide](../../sdk/node/README.md).

The end-to-end suite runs this source for enabled, disabled, and rejected-key cases.
