# Partly licensing

Copyright 2026 ByteBaker.

Partly's control plane and dashboard are **source-available under the Elastic License 2.0 (ELv2)**. The SDK, shared evaluator, and Python examples are **open source under Apache-2.0**.

| Scope | License |
| --- | --- |
| Control plane, dashboard, and other project files unless listed below | [Elastic License 2.0](LICENSE), SPDX `Elastic-2.0` |
| `crates/partly-core/` | [Apache License 2.0](crates/partly-core/LICENSE), SPDX `Apache-2.0` |
| `sdk/python/` | [Apache License 2.0](sdk/python/LICENSE), SPDX `Apache-2.0` |
| Shared workspace build metadata: `Cargo.toml` and `Cargo.lock` | [Apache License 2.0](crates/partly-core/LICENSE), SPDX `Apache-2.0` |
| `examples/python/` and application integration snippets in the READMEs and dashboard docs | [Apache License 2.0](examples/python/LICENSE), SPDX `Apache-2.0` |

The exceptions apply to those components even when they are bundled into the control-plane executable. Each third-party component keeps its original license; the Partly licenses do not replace those terms. License texts and notices themselves retain their applicable terms.

The paths above refer to the source repository. In a server archive, `LICENSE-APACHE` contains the Apache text for the bundled components.

## Using the server

ELv2 allows use, modification, and redistribution, subject to its terms. You can self-host the free features for your business and use Partly internally to control features in your own applications.

ELv2 restricts providing the software to third parties as a hosted or managed service that exposes a substantial set of its features. It also prohibits changing or bypassing license-key functionality, removing or obscuring features protected by license keys, and removing required notices. Compliant redistribution remains allowed, and ELv2 does not require forks to publish their changes. The [full license](LICENSE) governs these rights and restrictions.

This alpha has no premium activation or subscription requirement. Future premium features may use free or paid activation keys in the same executable. The source license does not implement those features or define subscription terms. It does not expire into an open-source license on a schedule.

## Using the SDK and examples

The Python SDK and its embedded Rust evaluator use Apache-2.0 so applications can use them under their own license, including proprietary licenses. Preserve the required license and attribution notices when redistributing these components. The server's ELv2 restrictions do not apply to these Apache-licensed components.

## Redistributing packages

Keep the supplied license texts and notices with copies of Partly. The server archive includes both Partly licenses and its third-party notices. Python wheels and source archives include the Apache license, Partly attribution, and native dependency notices; they do not include the server's ELv2 license.

This document explains the component boundaries. It does not replace the license texts or grant a separate commercial exception.
