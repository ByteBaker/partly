// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const { execFileSync } = require("node:child_process");
const { copyFileSync, mkdirSync } = require("node:fs");
const { resolve } = require("node:path");

const library = { darwin: "libpartly_node.dylib", linux: "libpartly_node.so" }[process.platform];
if (!library) throw new Error("Local builds currently support macOS and Linux");
const root = resolve(__dirname, "../..");
execFileSync("cargo", ["build", "--locked", "--release", "-p", "partly-node",
  "--target-dir", resolve(root, "target")], { cwd: root, stdio: "inherit" });
const output = resolve(__dirname, "native", `${process.platform}-${process.arch}`);
mkdirSync(output, { recursive: true });
copyFileSync(resolve(root, "target/release", library), resolve(output, "partly.node"));
